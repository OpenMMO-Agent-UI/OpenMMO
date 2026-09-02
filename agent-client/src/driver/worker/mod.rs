//! Rule-based workers: deterministic, LLM-free engines that drive a
//! character toward one purpose (fighting, fishing, scavenging).
//!
//! A worker reuses everything the LLM driver has — the action executor,
//! combat ticks, pathfinding, auto-respawn, the spectator mirror — and
//! replaces the prompt loop with fixed rules over the world state. Every
//! decision below is a plain function of `SharedState` plus config, so it
//! is unit-testable without a server.

mod fighter;
mod fisher;
mod labels;

use std::sync::Arc;
use std::time::{Duration, Instant};

use onlinerpg_shared::{Position, ServerMessage};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::combat::{load_attack_cooldown, tick_combat};
use super::execute::handle_response;
use super::movement::{
    coverage_positions, fetch_furniture_around, fetch_houses_around, fetch_no_spawn_zones_around,
};
use crate::state::SharedState;

/// Which rule engine drives Automatic play, or `none` for the LLM agent.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum WorkerKind {
    #[default]
    None,
    Fighter,
    Fisher,
}

/// Worker settings from the `[npcs.worker]` config table.
#[derive(Debug, Clone, Deserialize)]
pub struct WorkerConfig {
    #[serde(default)]
    pub kind: WorkerKind,
    /// Fight monsters up to `own level + margin`. Zero keeps the worker off
    /// anything above its own level.
    #[serde(default = "default_level_margin")]
    pub level_margin: u32,
    /// Health percentage below which the worker drinks (then flees).
    #[serde(default = "default_low_health_pct")]
    pub low_health_pct: u32,
    /// How many meals a town trip stocks up to. Passive healing is
    /// hunger-gated server-side, so a worker with no food stops regenerating.
    #[serde(default = "default_food_stock")]
    pub food_stock: u32,
    /// Which food `food_stock` restocks. Empty takes whatever food the
    /// nearby merchant's catalog offers first.
    #[serde(default)]
    pub food_item: Option<String>,
    /// How many healing potions a town trip stocks up to.
    #[serde(default = "default_potion_stock")]
    pub potion_stock: u32,
    /// Which potion `potion_stock` restocks and a low-health drink reaches
    /// for. Empty keeps `healing_potion`.
    #[serde(default)]
    pub potion_item: Option<String>,
    /// How many return scrolls a town trip stocks up to. A scroll is the ride
    /// home from the hunting ring, so a worker that runs out walks it.
    #[serde(default = "default_scroll_stock")]
    pub scroll_stock: u32,
    /// Which scroll `scroll_stock` restocks and the town/escape trip reads.
    /// Empty keeps `scroll_of_return`.
    #[serde(default)]
    pub scroll_item: Option<String>,
    /// Carry-weight percentage that sends the worker to town.
    #[serde(default = "default_bag_full_pct")]
    pub bag_full_pct: u32,
    /// Where the fighter works. Unset falls back to the world's spawn point.
    #[serde(default)]
    pub anchor_x: Option<f32>,
    #[serde(default)]
    pub anchor_z: Option<f32>,
    /// How far from the anchor the fighter roams: how far a patrol leg may end
    /// from it, and how far out a monster is still worth walking to.
    #[serde(default = "default_patrol_radius")]
    pub patrol_radius: u32,
}

fn default_level_margin() -> u32 {
    0
}
fn default_low_health_pct() -> u32 {
    70
}
fn default_food_stock() -> u32 {
    10
}
fn default_potion_stock() -> u32 {
    10
}
fn default_scroll_stock() -> u32 {
    5
}
fn default_bag_full_pct() -> u32 {
    90
}
fn default_patrol_radius() -> u32 {
    100
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            kind: WorkerKind::None,
            level_margin: default_level_margin(),
            low_health_pct: default_low_health_pct(),
            food_stock: default_food_stock(),
            food_item: None,
            potion_stock: default_potion_stock(),
            potion_item: None,
            scroll_stock: default_scroll_stock(),
            scroll_item: None,
            bag_full_pct: default_bag_full_pct(),
            anchor_x: None,
            anchor_z: None,
            patrol_radius: default_patrol_radius(),
        }
    }
}

/// What a worker does next. Rendered into the same action JSON the LLM
/// emits, so the executor, the toasts and the log all stay as they are.
#[derive(Debug, PartialEq)]
pub(crate) enum Step {
    Attack(String),
    Pickup(u64),
    Use(String),
    Sell(String, Option<u32>),
    Drop(String),
    Buy(String),
    Fish { x: f32, z: f32 },
    Walk { x: f32, z: f32 },
    Idle,
}

impl Step {
    fn action(&self) -> Option<Value> {
        Some(match self {
            Step::Attack(id) => json!({"type": "attack", "monster_id": id}),
            Step::Pickup(id) => json!({"type": "pickup", "item": id}),
            Step::Use(item) => json!({"type": "use", "item": item}),
            Step::Sell(item, qty) => json!({
                "type": "sell",
                "item": item,
                "qty": qty.map_or_else(|| json!("all"), |n| json!(n)),
            }),
            Step::Drop(item) => json!({"type": "drop", "item": item, "qty": "all"}),
            Step::Buy(item) => json!({"type": "buy", "item": item}),
            Step::Fish { x, z } => json!({"type": "fish", "x": x, "z": z}),
            // Always sprint: the hunger gate is the only governor a worker needs.
            Step::Walk { x, z } => json!({"type": "move", "x": x, "z": z, "sprint": true}),
            Step::Idle => return None,
        })
    }
}

/// Why the town errand is paused, and until when.
///
/// A pause after a visit that achieved something is a breather — whatever
/// sent us there may still read as unfixed for a tick or two. A pause after a
/// town that could *not* help is a verdict, and the difference matters: the
/// fighter is told to stay town-bound for the whole window, and standing
/// town-bound through a verdict is how a hungry worker with no gold stops
/// doing the one thing that earns gold. Five minutes of that, then another
/// five, for as long as it stays hungry.
#[derive(Debug, Clone, Copy)]
struct TownPause {
    until: Instant,
    /// The town had nothing that would fix what sent us there.
    useless: bool,
}

/// Where the worker is in the town round trip.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Errand {
    Work,
    ToTown,
    InTown,
}

// --- Decisions (pure over state + config) ---

/// Our health as a percentage of maximum; 100 when we are not in the world.
pub(crate) fn health_pct(s: &SharedState) -> u32 {
    match s.self_player.as_ref() {
        Some(p) if p.max_health > 0 => (p.health * 100 / p.max_health).min(100),
        _ => 100,
    }
}

fn category(def_id: &str) -> Option<&'static str> {
    crate::item_defs::get(def_id).and_then(|d| d.category.as_deref())
}

/// How many units of a category are carried, whichever specific items they
/// are — a restock that fell back off the configured item still counts.
fn bag_units_in_category(s: &SharedState, cat: &str) -> u32 {
    s.self_bag
        .iter()
        .filter(|i| category(&i.item_def_id) == Some(cat))
        .map(|i| i.quantity)
        .sum()
}

/// One carried item of a category, to drink or read — whatever is actually
/// in the bag, not necessarily the id configured to buy.
fn bag_item_in_category(s: &SharedState, cat: &str) -> Option<String> {
    s.self_bag
        .iter()
        .find(|i| category(&i.item_def_id) == Some(cat))
        .map(|i| i.item_def_id.clone())
}

/// Drink at low health while a potion is left — the fight continues.
pub(crate) fn should_drink_potion(s: &SharedState, cfg: &WorkerConfig) -> bool {
    health_pct(s) < cfg.low_health_pct && bag_units_in_category(s, "healing_potion") > 0
}

/// Out of potions at low health: read the scroll and take the town trip.
pub(crate) fn should_use_return_scroll(s: &SharedState, cfg: &WorkerConfig) -> bool {
    health_pct(s) < cfg.low_health_pct
        && bag_units_in_category(s, "healing_potion") == 0
        && bag_units_in_category(s, "return_scroll") > 0
}

/// Kit the worker lives on — never sold, never dropped.
fn is_keeper(def_id: &str) -> bool {
    matches!(
        category(def_id),
        Some("healing_potion") | Some("return_scroll") | Some("food") | Some("fishing_rod")
    )
}

/// Food in the bag, once hunger has cost us the sprint. Passive HP regen is
/// hunger-gated server-side, so an unfed worker never heals out of combat.
/// A raw catch counts: it is worth less nutrition than a grilled one, but a
/// fisher standing in its own supply should not walk to town starving.
pub(crate) fn should_eat(s: &SharedState) -> Option<String> {
    let (satiation, _) = s.self_hunger?;
    // The sprint threshold, not the hunger band: losing the run is the first
    // thing a worker feels, and it happens one point before Hungry begins.
    if satiation > onlinerpg_shared::hunger::NORMAL_MIN {
        return None;
    }
    s.self_bag
        .iter()
        .find(|i| matches!(category(&i.item_def_id), Some("food") | Some("fish")))
        .map(|i| i.item_def_id.clone())
}

/// Supply carried past its cap, and how many units of it to sell. No label is
/// needed: writing `potion_stock = 10` is already saying ten is all we want.
pub(crate) fn surplus_list(s: &SharedState, cfg: &WorkerConfig) -> Vec<(String, Option<u32>)> {
    let mut out = Vec::new();
    for (cat, cap) in [
        ("healing_potion", cfg.potion_stock),
        ("food", cfg.food_stock),
        ("return_scroll", cfg.scroll_stock),
    ] {
        let of_cat = || {
            s.self_bag
                .iter()
                .filter(|i| category(&i.item_def_id) == Some(cat))
        };
        let mut carried: u32 = of_cat().map(|i| i.quantity).sum();
        for item in of_cat() {
            if carried <= cap {
                break;
            }
            let paid_for = crate::item_defs::get(&item.item_def_id)
                .is_some_and(|d| d.base_price.unwrap_or(0) > 0);
            if !paid_for {
                continue;
            }
            let units = item.quantity.min(carried - cap);
            carried -= units;
            out.push((item.item_def_id.clone(), Some(units)));
        }
    }
    out
}

/// Everything in the bag a merchant pays for. When the app has marked items
/// with the sell label, only those are sold — a fighter keeps everything it
/// did not get the ok to sell.
pub(crate) fn sell_list(s: &SharedState, labels: &labels::BagLabels) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for item in &s.self_bag {
        let id = &item.item_def_id;
        let sellable = crate::item_defs::get(id).is_some_and(|d| d.base_price.unwrap_or(0) > 0);
        if sellable && !is_keeper(id) && !ids.contains(id) && labels.sellable.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// Dead weight, dropped in town. Only what the game itself calls junk: plenty
/// of unpriced items are worth carrying (a coin pouch pays out when used, a
/// worn starting weapon is the one you are holding). When the app has marked
/// items with the drop label, only those are dropped.
pub(crate) fn junk_list(s: &SharedState, labels: &labels::BagLabels) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for item in &s.self_bag {
        let id = &item.item_def_id;
        if category(id) == Some("junk") && !ids.contains(id) && labels.dropable.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// What a town trip should buy for one category, and how many: the gap to
/// the stock cap, never more than the purse can plausibly cover. The server
/// prices the sale (a merchant's markup is its own), so this is a bound, not
/// a quote — it only keeps a broke worker from firing refused purchases
/// every trip.
///
/// The item comes from the merchant in front of us, not a fixed id — bread
/// is Wick's, not Rica's, and Rica herself is not always around (she sleeps
/// at night, per `merchant_defs.rs`). The configured item is preferred when
/// this merchant carries it; otherwise whatever else of the category is on
/// the shelf, the same way an unset config always worked. Ordering what a
/// shop does not stock burns a turn on a refusal, so a category the nearby
/// merchant has none of at all buys nothing this trip.
fn restock_buy(
    s: &SharedState,
    cat: &str,
    preferred: Option<&str>,
    stock: u32,
) -> Option<(String, u32)> {
    let merchant = s.nearest_merchant()?;
    let name = &s.nearby_players.get(&merchant)?.name;
    let (catalog, _) = crate::shop_info::merchant_shop(name)?;
    let preferred = preferred.filter(|id| !id.is_empty());
    let id = preferred
        .filter(|id| catalog.contains(id))
        .or_else(|| catalog.into_iter().find(|id| category(id) == Some(cat)))?
        .to_string();
    let wanted = stock.saturating_sub(bag_units_in_category(s, cat));
    let price = crate::item_defs::get(&id)
        .and_then(|d| d.base_price)
        .unwrap_or(1)
        .max(1);
    let affordable = s
        .self_gold
        .map_or(wanted, |g| wanted.min((g / price).max(0) as u32));
    (affordable > 0).then_some((id, affordable))
}

pub(crate) fn food_to_buy(s: &SharedState, cfg: &WorkerConfig) -> Option<(String, u32)> {
    restock_buy(s, "food", cfg.food_item.as_deref(), cfg.food_stock)
}

pub(crate) fn potions_to_buy(s: &SharedState, cfg: &WorkerConfig) -> Option<(String, u32)> {
    restock_buy(
        s,
        "healing_potion",
        cfg.potion_item.as_deref(),
        cfg.potion_stock,
    )
}

pub(crate) fn scrolls_to_buy(s: &SharedState, cfg: &WorkerConfig) -> Option<(String, u32)> {
    restock_buy(
        s,
        "return_scroll",
        cfg.scroll_item.as_deref(),
        cfg.scroll_stock,
    )
}

/// How full the bag is, as a percentage of the carry cap.
pub(crate) fn bag_load_pct(s: &SharedState) -> u32 {
    let (carried, cap) = s.carry_load();
    if cap <= 0.0 {
        return 0;
    }
    (carried / cap * 100.0).round().max(0.0) as u32
}

/// Head to town when the bag is nearly full, or when starving with nothing
/// to eat — the two states the field cannot fix.
pub(crate) fn should_town_trip(s: &SharedState, cfg: &WorkerConfig) -> bool {
    if bag_load_pct(s) >= cfg.bag_full_pct {
        return true;
    }
    // Out of food and already past the point where the run is gone — not
    // `Weak`, which is two thirds of the way further down. Waiting for that
    // meant starting the walk home from as far out as the ring goes at
    // `WEAK_MOVE_MULT` (0.75x), and `WEAK_CARRY_MULT` shrinks the bag on the
    // way, so the trip that finally fired often read as a full-bag trip
    // instead. `should_eat` uses the same threshold for the same reason:
    // losing the sprint is the first thing a worker actually feels.
    let hungry = s
        .self_hunger
        .is_some_and(|(satiation, _)| satiation <= onlinerpg_shared::hunger::NORMAL_MIN);
    hungry && should_eat(s).is_none()
}

/// Something worth abandoning a walk for.
///
/// Read by `movement::walk_waypoints` between steps, and deliberately the
/// same question `fighter::free_kill` answers: a leg is only thrown away when
/// the very next decision really would be an attack. Sharing the predicate is
/// what keeps the two in step — a walk that stopped for prey the fighter then
/// declined to swing at would stutter in place beside it.
pub(super) fn prey_in_reach(s: &SharedState, level_margin: u32) -> bool {
    fighter::free_kill(
        s,
        &WorkerConfig {
            level_margin,
            ..WorkerConfig::default()
        },
    )
    .is_some()
}

/// Loot lying within `radius` of a point, closest to us first. The caller
/// passes the kill site, so a distant pile never pulls the worker off course.
pub(crate) fn loot_candidates(s: &SharedState, near: Position, radius: f32) -> Vec<u64> {
    s.ground_items_in_sight()
        .into_iter()
        .filter(|(_, item)| item.position.dist_xz_sq(&near) <= radius * radius)
        .map(|(_, item)| item.instance_id)
        .collect()
}

/// Where town is: the centre of the nearest no-spawn zone. Towns are the
/// zones monsters may not spawn in, so the server's own data says where to
/// walk — no hardcoded coordinates.
/// Zones smaller than this on either side are map-editor slivers, not towns —
/// a stray drag in the zone editor leaves 7m boxes, and walking to the centre
/// of one is a trip to nowhere.
const MIN_TOWN_SPAN: f32 = 20.0;

fn nearest_town(s: &SharedState) -> Option<&onlinerpg_shared::NoSpawnZone> {
    let me = s.self_player.as_ref()?.position;
    s.no_spawn_zones
        .iter()
        .filter(|z| z.max_x - z.min_x >= MIN_TOWN_SPAN && z.max_z - z.min_z >= MIN_TOWN_SPAN)
        .min_by(|a, b| {
            let d = |z: &onlinerpg_shared::NoSpawnZone| {
                ((z.min_x + z.max_x) / 2.0 - me.x).powi(2)
                    + ((z.min_z + z.max_z) / 2.0 - me.z).powi(2)
            };
            d(a).total_cmp(&d(b))
        })
}

/// Where to stand while looking for a merchant: the town's centre, then its
/// four quarters. One look from the centre is not a search — sight reaches
/// NPC_SIGHT_RADIUS, and a town is wider than that, so a merchant on the far
/// side stayed invisible and every trip was written off as "no merchant".
pub(crate) fn town_stops(s: &SharedState) -> Vec<(f32, f32)> {
    let Some(z) = nearest_town(s) else {
        return Vec::new();
    };
    let (cx, cz) = ((z.min_x + z.max_x) / 2.0, (z.min_z + z.max_z) / 2.0);
    let (qw, qd) = ((z.max_x - z.min_x) / 4.0, (z.max_z - z.min_z) / 4.0);
    vec![
        (cx, cz),
        (cx - qw, cz - qd),
        (cx + qw, cz - qd),
        (cx - qw, cz + qd),
        (cx + qw, cz + qd),
    ]
}

/// The monster that just hit us, if any — self-defence overrides every
/// eligibility rule, including the level margin.
pub(crate) fn attacker_in(
    events: &[ServerMessage],
    me: Option<&onlinerpg_shared::PlayerId>,
) -> Option<String> {
    events.iter().rev().find_map(|e| match e {
        ServerMessage::MonsterAttackedPlayer {
            monster_id,
            player_id,
            ..
        } if Some(player_id) == me => Some(monster_id.clone()),
        _ => None,
    })
}

// --- Driver loop ---

/// Run one rule-based worker until the connection drops. Mirrors
/// `llm_driver`'s housekeeping (respawn, lapsed trades, housing prefetch)
/// without any of its prompt machinery.
pub async fn worker_driver(
    state: Arc<Mutex<SharedState>>,
    cfg: WorkerConfig,
    label: String,
    api_base_url: String,
    watch: Option<Arc<crate::watch::NpcWatch>>,
    instance_prompt: Option<String>,
) {
    while !state.lock().await.in_game {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    info!("[{label}] Worker ({:?}) in game, ready.", cfg.kind);

    // Pathfinding is blind to buildings until the housing data is in.
    let mut world_data_at = {
        let (world_cache, around) = {
            let s = state.lock().await;
            (
                Arc::clone(&s.world_cache),
                s.self_player.as_ref().map(|p| p.position),
            )
        };
        let area = coverage_positions(&[], around);
        tokio::join!(
            fetch_houses_around(&world_cache, &area, &api_base_url, &label),
            fetch_furniture_around(&world_cache, &area, &api_base_url, &label),
            // Before the first decision, not after: the fighter's very first
            // tick asks whether it is standing in a town, and an empty answer
            // is indistinguishable from "no towns exist".
            fetch_no_spawn_zones_around(&state, &area, &api_base_url, &label),
        );
        around.map(|p| (p.x, p.z))
    };

    let attack_cooldown = load_attack_cooldown();
    let mut attack_target: Option<String> = None;
    let mut last_attack_at = Instant::now() - attack_cooldown;
    let mut dead_since: Option<Instant> = None;
    let mut errand = Errand::Work;
    // Where the last patrol leg was issued from, so a leg that failed to move
    // us reaches further round the arc instead of being reissued unchanged.
    let mut patrol = fighter::Patrol::default();
    // Where the last kill fell (and how many pickups we have tried there), so
    // its drops are the only loot we detour for.
    let mut loot_at: Option<(Position, u32)> = None;
    let mut town_blocked_until: Option<TownPause> = None;
    // How far through the town's stops this trip has looked for a merchant.
    let mut town_stop = 0usize;
    // Where the current target stood the last time we could see it.
    let mut target_last_seen: Option<Position> = None;
    // The last turn mirrored to the spectator feed, so a repeated decision is
    // reported once instead of twice a second.
    let mut last_turn = String::new();
    // Re-read the app's sell/drop marks on every trip so a label change while
    // the worker runs is picked up without a restart.
    let mut labels = labels::labels_from_prompt(instance_prompt.as_deref());

    loop {
        refresh_world_data(&state, &mut world_data_at, &api_base_url, &label).await;

        let tick = if attack_target.is_some() {
            attack_cooldown.saturating_sub(last_attack_at.elapsed())
        } else {
            Duration::from_millis(500)
        };
        tokio::time::sleep(tick.max(Duration::from_millis(50))).await;

        super::decline_lapsed_trade(&state, &label).await;

        let (self_dead, attacker) = {
            let mut s = state.lock().await;
            let events = s.drain_events();
            s.drain_agent_events();
            let me = s.self_player_id;
            let dead = s.in_game && s.self_player.as_ref().is_some_and(|p| p.health == 0);
            (dead, attacker_in(&events, me.as_ref()))
        };

        if super::respawn_due(self_dead, &mut dead_since, Instant::now()) {
            super::request_respawn(&state, &None, &label).await;
        }
        if self_dead {
            attack_target = None;
            continue;
        }

        // Anything that hits us becomes the target, whatever the rules say.
        if let Some(id) = attacker {
            if attack_target.as_deref() != Some(id.as_str()) {
                info!("[{label}] Worker: fighting back against {id}");
                attack_target = Some(id);
            }
        }

        // Staying alive comes before the next swing: health only ever drops
        // mid-fight, so a potion rule that waited for the fight to end would
        // never fire when it matters.
        if let Some(rescue) = self_rescue(&state, &cfg, &mut errand, &label).await {
            run(&state, rescue, &watch, &mut last_turn).await;
            if errand != Errand::Work {
                attack_target = None;
            }
            continue;
        }

        if let Some(target) = attack_target.clone() {
            if last_attack_at.elapsed() >= attack_cooldown {
                // Remember where the target stands while we can still see it.
                // A kill is only noticed on the tick *after* the killing blow,
                // by which time the corpse is out of `nearby_monsters` — read
                // the position then and every kill site comes back None.
                {
                    let s = state.lock().await;
                    if let Some(p) = s.nearby_monsters.get(&target).map(|m| m.position) {
                        target_last_seen = Some(p);
                    } else if let Some(p) = s.self_player.as_ref().map(|p| p.position) {
                        // Never saw it: we are in melee range of whatever it
                        // was, so where we stand is close enough.
                        target_last_seen.get_or_insert(p);
                    }
                }
                // Always sprint: the hunger gate is the only governor a worker needs.
                let keep_fighting = tick_combat(&state, &target, Some(true)).await;
                attack_target = keep_fighting.then_some(target);
                last_attack_at = Instant::now();
                if attack_target.is_none() {
                    loot_at = target_last_seen.take().map(|p| (p, 0));
                }
            }
            continue;
        }

        // Re-read the sell/drop marks while on a town errand, so a label
        // change made while the worker runs is picked up on this trip.
        if errand != Errand::Work {
            labels = labels::labels_from_prompt(instance_prompt.as_deref());
        }
        let step = next_step(
            &state,
            &cfg,
            &mut errand,
            &mut loot_at,
            &mut town_blocked_until,
            &mut town_stop,
            &label,
            &labels,
            &mut patrol,
        )
        .await;
        if let Some(target) = run(&state, step, &watch, &mut last_turn).await {
            attack_target = Some(target);
            last_attack_at = Instant::now() - attack_cooldown;
        }
    }
}

/// Refetch houses/furniture/towns once we have walked a chunk away, the same
/// rule the LLM driver uses — otherwise buildings vanish from pathfinding and
/// a town walked into from a fresh region is invisible.
async fn refresh_world_data(
    state: &Arc<Mutex<SharedState>>,
    world_data_at: &mut Option<(f32, f32)>,
    api_base_url: &str,
    label: &str,
) {
    let (world_cache, pos) = {
        let s = state.lock().await;
        (
            Arc::clone(&s.world_cache),
            s.self_player.as_ref().map(|p| p.position),
        )
    };
    let Some(p) = pos else { return };
    let moved_a_chunk = world_data_at.is_none_or(|(x, z)| {
        let (dx, dz) = (p.x - x, p.z - z);
        dx * dx + dz * dz > 64.0 * 64.0
    });
    if !moved_a_chunk {
        return;
    }
    let area = [(p.x, p.z)];
    tokio::join!(
        fetch_houses_around(&world_cache, &area, api_base_url, label),
        fetch_furniture_around(&world_cache, &area, api_base_url, label),
        fetch_no_spawn_zones_around(state, &area, api_base_url, label),
    );
    *world_data_at = Some((p.x, p.z));
}

/// One tick's decision: survival first, then the town round trip, then the
/// worker's own purpose.
/// Drink, flee or eat — checked every tick, fight or no fight. `None` means
/// nothing needs saving right now.
async fn self_rescue(
    state: &Arc<Mutex<SharedState>>,
    cfg: &WorkerConfig,
    errand: &mut Errand,
    label: &str,
) -> Option<Vec<Step>> {
    let s = state.lock().await;
    if should_drink_potion(&s, cfg) {
        if let Some(potion) = bag_item_in_category(&s, "healing_potion") {
            return Some(vec![Step::Use(potion)]);
        }
    }
    // Only ever one scroll: reading it teleports but does not heal, so the
    // trigger is still true on the next tick and a whole stack would burn.
    // The town errand is what says the escape already happened.
    if *errand == Errand::Work && should_use_return_scroll(&s, cfg) {
        if let Some(scroll) = bag_item_in_category(&s, "return_scroll") {
            info!("[{label}] Worker: reading a return scroll to escape");
            *errand = Errand::InTown;
            return Some(vec![Step::Use(scroll)]);
        }
    }
    should_eat(&s).map(|food| vec![Step::Use(food)])
}

#[allow(clippy::too_many_arguments)] // one call site; a params struct would just relocate the noise
async fn next_step(
    state: &Arc<Mutex<SharedState>>,
    cfg: &WorkerConfig,
    errand: &mut Errand,
    loot_at: &mut Option<(Position, u32)>,
    town_blocked_until: &mut Option<TownPause>,
    town_stop: &mut usize,
    label: &str,
    labels: &labels::BagLabels,
    patrol: &mut fighter::Patrol,
) -> Vec<Step> {
    let mut s = state.lock().await;
    // Cleared before any decision is taken. Everything below this can return
    // early — the town errand, the loot sweep — and a leg walked to reach a
    // merchant must not inherit the arming from the tick that was hunting.
    // Only the fighter's own hunting legs re-arm it.
    s.abandon_leg_for = None;
    let pause = town_blocked_until.filter(|p| Instant::now() < p.until);
    let waiting_out_a_failed_trip = pause.is_some();
    if *errand == Errand::Work && !waiting_out_a_failed_trip && should_town_trip(&s, cfg) {
        let load = bag_load_pct(&s);
        // A scroll spends the walk home for one item. The last one is not
        // spent commuting: that is the low-health escape, and stranded at the
        // far end of the map is survivable in a way stranded at 10% is not.
        // `InTown` is what stops the next tick reading the same trigger and
        // burning the rest of the stack.
        if bag_units_in_category(&s, "return_scroll") > 1 {
            if let Some(scroll) = bag_item_in_category(&s, "return_scroll") {
                info!("[{label}] Worker: reading a return scroll to town (bag {load}%)");
                *errand = Errand::InTown;
                return vec![Step::Use(scroll)];
            }
        }
        info!("[{label}] Worker: heading to town (bag {load}%)");
        *errand = Errand::ToTown;
    }

    if *errand != Errand::Work {
        // A merchant in sight is what "in town" means — sell/buy walk the
        // last stretch themselves.
        if s.nearest_merchant().is_some() {
            *errand = Errand::Work;
            *town_stop = 0;
            let business = town_business(&s, cfg, labels);
            if business.is_empty() {
                // Nothing the shop can fix — starving with no gold for food,
                // say. Coming straight back would be an endless commute.
                warn!(
                    "[{label}] Worker: {}, back to work",
                    why_nothing(&s, cfg, labels)
                );
                *town_blocked_until = Some(TownPause {
                    until: Instant::now() + TOWN_RETRY_DELAY,
                    useless: true,
                });
                return leave_a_useless_town(&s, cfg);
            }
            // Even a productive trip earns a pause: whatever sent us here may
            // still read as unfixed for a tick or two, and the answer to that
            // is work, not a second lap of the same shop.
            *town_blocked_until = Some(TownPause {
                until: Instant::now() + TOWN_VISIT_DELAY,
                useless: false,
            });
            return business;
        }
        // No merchant in view: walk the town's stops until one is in sight.
        *errand = Errand::ToTown;
        let arrived = |x: f32, z: f32| {
            s.self_player
                .as_ref()
                .is_some_and(|p| (p.position.x - x).hypot(p.position.z - z) <= TOWN_ARRIVE_RANGE)
        };
        let stops = town_stops(&s);
        while let Some(&(x, z)) = stops.get(*town_stop) {
            if !arrived(x, z) {
                return vec![Step::Walk { x, z }];
            }
            // Standing on this one with nobody in sight: try the next quarter.
            *town_stop += 1;
        }
        warn!("[{label}] Worker: no merchant anywhere in town, back to work");
        *errand = Errand::Work;
        *town_stop = 0;
        *town_blocked_until = Some(TownPause {
            until: Instant::now() + TOWN_RETRY_DELAY,
            useless: true,
        });
        return leave_a_useless_town(&s, cfg);
    }

    // Fresh drops from our own kill, before anything else. Bounded: an item
    // that will not come up (unreachable, too heavy) must not hold the worker
    // at the corpse forever.
    if let Some((site, tries)) = *loot_at {
        let loot = loot_candidates(&s, site, LOOT_RADIUS);
        match loot.first() {
            Some(id) if tries < MAX_LOOT_TRIES => {
                *loot_at = Some((site, tries + 1));
                return vec![Step::Pickup(*id)];
            }
            Some(_) => info!("[{label}] Worker: giving up on the loot here"),
            None => {}
        }
        *loot_at = None;
    }

    match cfg.kind {
        WorkerKind::Fighter => {
            // Not town-bound while waiting out a town that said it cannot
            // help: it has already answered, and hunting beats standing in
            // front of it hoping the answer changes.
            let town_bound = should_town_trip(&s, cfg) && !pause.is_some_and(|p| p.useless);
            // Armed only for a leg walked to find a fight, which is worth
            // dropping the moment one turns up. A leg walked to reach a
            // merchant is not — abandoning that one every time something
            // wanders past is how a town trip never finishes — and neither is
            // the commute back to the anchor, where nothing is fought unless
            // it strikes first.
            s.abandon_leg_for =
                (!town_bound && !fighter::beyond_circle(&s, cfg)).then_some(cfg.level_margin);
            fighter::step(&s, cfg, town_bound, patrol)
        }
        WorkerKind::Fisher => {
            let job = fisher::water_job(&s);
            drop(s);
            fisher::step(job).await
        }
        WorkerKind::None => vec![Step::Idle],
    }
}

/// How far from a kill its drops count as ours to collect.
const LOOT_RADIUS: f32 = 6.0;
/// Pickups attempted at one kill site before writing the loot off.
const MAX_LOOT_TRIES: u32 = 3;
/// Close enough to the town anchor to count as having arrived.
const TOWN_ARRIVE_RANGE: f32 = 10.0;
/// How long a town trip that achieved nothing stops being retried.
const TOWN_RETRY_DELAY: Duration = Duration::from_secs(300);
/// Breathing room after a trip that did do business.
const TOWN_VISIT_DELAY: Duration = Duration::from_secs(60);

/// A town that cannot help is not worth standing in: nothing spawns inside a
/// no-spawn zone, so a fighter waiting out the retry delay there does nothing
/// at all — which is what "parked at the town boundary" looked like from the
/// outside. The fisher stays put; a town pond is a legitimate fishing hole.
fn leave_a_useless_town(s: &SharedState, cfg: &WorkerConfig) -> Vec<Step> {
    if cfg.kind != WorkerKind::Fighter {
        return vec![Step::Idle];
    }
    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return vec![Step::Idle];
    };
    match fighter::escape_target(&s.no_spawn_zones, me) {
        Some((x, z)) => vec![Step::Walk { x, z }],
        None => vec![Step::Idle],
    }
}

/// Why a trip found nothing to do, in the words of the thing the player can
/// change. A full bag reaching a merchant and coming home just as heavy reads
/// as a broken worker, when what it means is that nothing in the bag carries
/// the app's Sellable mark — the one gate `sell_list` applies.
fn why_nothing(s: &SharedState, cfg: &WorkerConfig, labels: &labels::BagLabels) -> String {
    let load = bag_load_pct(s);
    if load < cfg.bag_full_pct {
        return "nothing to do in town".to_string();
    }
    if labels.sellable.is_empty() {
        format!("bag {load}% full and nothing in it is marked Sellable — mark items in the bag drawer, then Apply labels")
    } else {
        format!(
            "bag {load}% full but nothing marked Sellable is in it (marked: {})",
            labels.sellable.join(", ")
        )
    }
}

/// Everything the town trip does in one turn: sell the marked loot and the
/// surplus supply, drop the marked junk, restock, eat.
/// Empty when the shop has nothing to offer this trip.
pub(crate) fn town_business(
    s: &SharedState,
    cfg: &WorkerConfig,
    labels: &labels::BagLabels,
) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    for id in sell_list(s, labels) {
        steps.push(Step::Sell(id, None));
    }
    for (id, units) in surplus_list(s, cfg) {
        steps.push(Step::Sell(id, units));
    }
    for id in junk_list(s, labels) {
        steps.push(Step::Drop(id));
    }
    if let Some((food, count)) = food_to_buy(s, cfg) {
        for _ in 0..count {
            steps.push(Step::Buy(food.clone()));
        }
    }
    if let Some((potion, count)) = potions_to_buy(s, cfg) {
        for _ in 0..count {
            steps.push(Step::Buy(potion.clone()));
        }
    }
    if let Some((scroll, count)) = scrolls_to_buy(s, cfg) {
        for _ in 0..count {
            steps.push(Step::Buy(scroll.clone()));
        }
    }
    if let Some(food) = should_eat(s) {
        steps.push(Step::Use(food));
    }
    steps
}

/// Run the chosen steps through the LLM driver's own action executor, and
/// mirror the turn to the spectator feed so action captions keep appearing.
/// Returns a monster id when the turn ended in an attack.
async fn run(
    state: &Arc<Mutex<SharedState>>,
    steps: Vec<Step>,
    watch: &Option<Arc<crate::watch::NpcWatch>>,
    last_turn: &mut String,
) -> Option<String> {
    // An idle tick is reported as a turn too — a caption left reading
    // "Attack→orc" while the worker stands in an empty field is a lie — but
    // only when it is news, or a waiting worker would flood the feed twice a
    // second.
    let actions: Vec<Value> = match steps.iter().filter_map(Step::action).collect::<Vec<_>>() {
        actions if actions.is_empty() => vec![json!({"type": "wait"})],
        actions => actions,
    };
    let turn = json!({ "actions": actions }).to_string();
    if let Some(w) = watch {
        if *last_turn != turn {
            w.push("worker", turn.clone());
            last_turn.clone_from(&turn);
        }
    }
    if steps.iter().all(|s| *s == Step::Idle) {
        return None;
    }
    handle_response(state, &turn, &None, &None, false)
        .await
        .map(|(monster_id, _)| monster_id)
}

#[cfg(test)]
mod tests;
