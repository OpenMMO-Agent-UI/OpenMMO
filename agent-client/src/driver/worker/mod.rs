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

use std::sync::Arc;
use std::time::{Duration, Instant};

use onlinerpg_shared::hunger::HungerState;
use onlinerpg_shared::{Position, ServerMessage};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tracing::info;

use super::combat::{load_attack_cooldown, tick_combat};
use super::execute::handle_response;
use super::movement::{coverage_positions, fetch_furniture_around, fetch_houses_around};
use crate::state::SharedState;

pub const HEALING_POTION: &str = "healing_potion";
pub const RETURN_SCROLL: &str = "scroll_of_return";

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
    /// Fight monsters up to `own level + margin`.
    #[serde(default = "default_level_margin")]
    pub level_margin: u32,
    /// Health percentage below which the worker drinks (then flees).
    #[serde(default = "default_low_health_pct")]
    pub low_health_pct: u32,
    /// How many healing potions a town trip stocks up to.
    #[serde(default = "default_potion_stock")]
    pub potion_stock: u32,
    /// Carry-weight percentage that sends the worker to town.
    #[serde(default = "default_bag_full_pct")]
    pub bag_full_pct: u32,
}

fn default_level_margin() -> u32 {
    2
}
fn default_low_health_pct() -> u32 {
    40
}
fn default_potion_stock() -> u32 {
    10
}
fn default_bag_full_pct() -> u32 {
    80
}

impl Default for WorkerConfig {
    fn default() -> Self {
        Self {
            kind: WorkerKind::None,
            level_margin: default_level_margin(),
            low_health_pct: default_low_health_pct(),
            potion_stock: default_potion_stock(),
            bag_full_pct: default_bag_full_pct(),
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
    Sell(String),
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
            Step::Sell(item) => json!({"type": "sell", "item": item, "qty": "all"}),
            Step::Drop(item) => json!({"type": "drop", "item": item, "qty": "all"}),
            Step::Buy(item) => json!({"type": "buy", "item": item}),
            Step::Fish { x, z } => json!({"type": "fish", "x": x, "z": z}),
            Step::Walk { x, z } => json!({"type": "move", "x": x, "z": z}),
            Step::Idle => return None,
        })
    }
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

fn bag_units(s: &SharedState, def_id: &str) -> u32 {
    s.self_bag
        .iter()
        .filter(|i| i.item_def_id == def_id)
        .map(|i| i.quantity)
        .sum()
}

/// Drink at low health while a potion is left — the fight continues.
pub(crate) fn should_drink_potion(s: &SharedState, cfg: &WorkerConfig) -> bool {
    health_pct(s) < cfg.low_health_pct && bag_units(s, HEALING_POTION) > 0
}

/// Out of potions at low health: read the scroll and take the town trip.
pub(crate) fn should_use_return_scroll(s: &SharedState, cfg: &WorkerConfig) -> bool {
    health_pct(s) < cfg.low_health_pct
        && bag_units(s, HEALING_POTION) == 0
        && bag_units(s, RETURN_SCROLL) > 0
}

fn category(def_id: &str) -> Option<&'static str> {
    crate::item_defs::get(def_id).and_then(|d| d.category.as_deref())
}

/// Kit the worker lives on — never sold, never dropped.
fn is_keeper(def_id: &str) -> bool {
    matches!(
        category(def_id),
        Some("healing_potion") | Some("return_scroll") | Some("food") | Some("fishing_rod")
    )
}

/// Food in the bag, if the hunger band asks for a meal. Passive HP regen is
/// hunger-gated server-side, so an unfed worker never heals out of combat.
/// A raw catch counts: it is worth less nutrition than a grilled one, but a
/// fisher standing in its own supply should not walk to town starving.
pub(crate) fn should_eat(s: &SharedState) -> Option<String> {
    let (_, band) = s.self_hunger?;
    if band == HungerState::Normal {
        return None;
    }
    s.self_bag
        .iter()
        .find(|i| matches!(category(&i.item_def_id), Some("food") | Some("fish")))
        .map(|i| i.item_def_id.clone())
}

/// Everything in the bag a merchant pays for.
pub(crate) fn sell_list(s: &SharedState) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for item in &s.self_bag {
        let id = &item.item_def_id;
        let sellable = crate::item_defs::get(id).is_some_and(|d| d.base_price.unwrap_or(0) > 0);
        if sellable && !is_keeper(id) && !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// Dead weight, dropped in town. Only what the game itself calls junk: plenty
/// of unpriced items are worth carrying (a coin pouch pays out when used, a
/// worn starting weapon is the one you are holding).
pub(crate) fn junk_list(s: &SharedState) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for item in &s.self_bag {
        let id = &item.item_def_id;
        if category(id) == Some("junk") && !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    ids
}

/// How many healing potions the restock buys: the gap to the stock cap, but
/// never more than the purse can plausibly cover. The server prices the sale
/// (a merchant's markup is its own), so this is a bound, not a quote — it
/// only keeps a broke worker from firing refused purchases every trip.
pub(crate) fn potions_to_buy(s: &SharedState, cfg: &WorkerConfig) -> u32 {
    let wanted = cfg
        .potion_stock
        .saturating_sub(bag_units(s, HEALING_POTION));
    let Some(gold) = s.self_gold else {
        return wanted;
    };
    let price = crate::item_defs::get(HEALING_POTION)
        .and_then(|d| d.base_price)
        .unwrap_or(1)
        .max(1);
    wanted.min((gold / price).max(0) as u32)
}

/// The server's carry cap: STR × 15, scaled by the hunger band.
fn carry_capacity(s: &SharedState) -> f32 {
    let strength = s
        .characters
        .first()
        .map(|c| c.attributes.r#str as f32)
        .unwrap_or(10.0);
    let band = s.self_hunger.map_or(HungerState::Normal, |(_, b)| b);
    strength * 15.0 * onlinerpg_shared::hunger::state_multipliers(band).2
}

fn item_weight(def_id: &str) -> f32 {
    crate::item_defs::get(def_id).map_or(0.0, |d| d.weight)
}

/// How full the bag is, as a percentage of the carry cap.
pub(crate) fn bag_load_pct(s: &SharedState) -> u32 {
    let carried: f32 = s
        .self_bag
        .iter()
        .map(|i| item_weight(&i.item_def_id) * i.quantity as f32)
        .chain(
            s.self_equipped
                .values()
                .map(|i| item_weight(&i.item_def_id)),
        )
        .sum();
    let cap = carry_capacity(s);
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
    let starving = s.self_hunger.is_some_and(|(_, b)| b == HungerState::Weak);
    starving && should_eat(s).is_none()
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
pub(crate) fn town_anchor(s: &SharedState) -> Option<(f32, f32)> {
    let me = s.self_player.as_ref()?.position;
    s.no_spawn_zones
        .iter()
        .map(|z| ((z.min_x + z.max_x) / 2.0, (z.min_z + z.max_z) / 2.0))
        .min_by(|a, b| {
            let da = (a.0 - me.x).powi(2) + (a.1 - me.z).powi(2);
            let db = (b.0 - me.x).powi(2) + (b.1 - me.z).powi(2);
            da.total_cmp(&db)
        })
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
        );
        around.map(|p| (p.x, p.z))
    };

    let attack_cooldown = load_attack_cooldown();
    let mut attack_target: Option<String> = None;
    let mut last_attack_at = Instant::now() - attack_cooldown;
    let mut dead_since: Option<Instant> = None;
    let mut errand = Errand::Work;
    // Where the last kill fell (and how many pickups we have tried there), so
    // its drops are the only loot we detour for.
    let mut loot_at: Option<(Position, u32)> = None;
    let mut town_blocked_until: Option<Instant> = None;
    // Where the current target stood the last time we could see it.
    let mut target_last_seen: Option<Position> = None;
    // The last turn mirrored to the spectator feed, so a repeated decision is
    // reported once instead of twice a second.
    let mut last_turn = String::new();

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
                attack_target = tick_combat(&state, target).await;
                last_attack_at = Instant::now();
                if attack_target.is_none() {
                    loot_at = target_last_seen.take().map(|p| (p, 0));
                }
            }
            continue;
        }

        let step = next_step(
            &state,
            &cfg,
            &mut errand,
            &mut loot_at,
            &mut town_blocked_until,
            &label,
        )
        .await;
        if let Some(target) = run(&state, step, &watch, &mut last_turn).await {
            attack_target = Some(target);
            last_attack_at = Instant::now() - attack_cooldown;
        }
    }
}

/// Refetch houses/furniture once we have walked a chunk away, the same rule
/// the LLM driver uses — otherwise buildings vanish from pathfinding.
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
        return Some(vec![Step::Use(HEALING_POTION.to_string())]);
    }
    // Only ever one scroll: reading it teleports but does not heal, so the
    // trigger is still true on the next tick and a whole stack would burn.
    // The town errand is what says the escape already happened.
    if *errand == Errand::Work && should_use_return_scroll(&s, cfg) {
        info!("[{label}] Worker: reading a return scroll to escape");
        *errand = Errand::InTown;
        return Some(vec![Step::Use(RETURN_SCROLL.to_string())]);
    }
    should_eat(&s).map(|food| vec![Step::Use(food)])
}

async fn next_step(
    state: &Arc<Mutex<SharedState>>,
    cfg: &WorkerConfig,
    errand: &mut Errand,
    loot_at: &mut Option<(Position, u32)>,
    town_blocked_until: &mut Option<Instant>,
    label: &str,
) -> Vec<Step> {
    let s = state.lock().await;
    let waiting_out_a_failed_trip = town_blocked_until.is_some_and(|t| Instant::now() < t);
    if *errand == Errand::Work && !waiting_out_a_failed_trip && should_town_trip(&s, cfg) {
        info!(
            "[{label}] Worker: heading to town (bag {}%)",
            bag_load_pct(&s)
        );
        *errand = Errand::ToTown;
    }

    if *errand != Errand::Work {
        // A merchant in sight is what "in town" means — sell/buy walk the
        // last stretch themselves.
        if s.nearest_merchant().is_some() {
            *errand = Errand::Work;
            let business = town_business(&s, cfg);
            if business.is_empty() {
                // Nothing the shop can fix — starving with no gold for food,
                // say. Coming straight back would be an endless commute.
                info!("[{label}] Worker: nothing to do in town, back to work");
                *town_blocked_until = Some(Instant::now() + TOWN_RETRY_DELAY);
                return vec![Step::Idle];
            }
            // Even a productive trip earns a pause: whatever sent us here may
            // still read as unfixed for a tick or two, and the answer to that
            // is work, not a second lap of the same shop.
            *town_blocked_until = Some(Instant::now() + TOWN_VISIT_DELAY);
            return business;
        }
        // Teleported home but no merchant in view: walk to the anchor.
        *errand = Errand::ToTown;
        let arrived = |x: f32, z: f32| {
            s.self_player
                .as_ref()
                .is_some_and(|p| (p.position.x - x).hypot(p.position.z - z) <= TOWN_ARRIVE_RANGE)
        };
        match town_anchor(&s) {
            // Standing in town with no merchant to be seen: stop walking in
            // circles and get back to work for a while.
            Some((x, z)) if !arrived(x, z) => return vec![Step::Walk { x, z }],
            _ => {
                info!("[{label}] Worker: no merchant found in town, back to work");
                *errand = Errand::Work;
                *town_blocked_until = Some(Instant::now() + TOWN_RETRY_DELAY);
                return vec![Step::Idle];
            }
        }
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
        WorkerKind::Fighter => fighter::step(&s, cfg),
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

/// Everything the town trip does in one turn: empty the bag, restock, eat.
/// Empty when the shop has nothing to offer this trip.
pub(crate) fn town_business(s: &SharedState, cfg: &WorkerConfig) -> Vec<Step> {
    let mut steps: Vec<Step> = Vec::new();
    for id in sell_list(s) {
        steps.push(Step::Sell(id));
    }
    for id in junk_list(s) {
        steps.push(Step::Drop(id));
    }
    for _ in 0..potions_to_buy(s, cfg) {
        steps.push(Step::Buy(HEALING_POTION.to_string()));
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
    handle_response(state, &turn, &None, &None, false).await
}

#[cfg(test)]
mod tests;
