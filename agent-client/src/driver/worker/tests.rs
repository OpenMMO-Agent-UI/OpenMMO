//! Worker decisions, asserted as behaviour: given a world snapshot and a
//! config, the worker makes the expected call.

use super::*;
use crate::state::tests::{ground_item, test_player, test_state};
use onlinerpg_shared::hunger::HungerState;
use onlinerpg_shared::inventory::ItemInstance;
use onlinerpg_shared::{Monster, MonsterState, NoSpawnZone, PlayerId};

/// Restock is category-driven now, not one fixed id per slot — these just
/// name the canonical item each category's tests bag by default.
const HEALING_POTION: &str = "healing_potion";
const RETURN_SCROLL: &str = "scroll_of_return";

/// The tests work in a local frame around the origin, so the anchor is pinned
/// there: an unset anchor is the world's spawn point thousands of metres away,
/// and every monster placed here would stand outside the patrol circle.
fn cfg() -> WorkerConfig {
    WorkerConfig {
        kind: WorkerKind::Fighter,
        anchor_x: Some(0.0),
        anchor_z: Some(0.0),
        ..WorkerConfig::default()
    }
}

fn state_at(x: f32, z: f32) -> SharedState {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(x, z));
    s.self_player_id = Some(PlayerId::from(1));
    s
}

fn bag(s: &mut SharedState, def_id: &str, quantity: u32) {
    let instance_id = s.self_bag.len() as u64 + 1;
    s.self_bag.push(ItemInstance {
        instance_id,
        item_def_id: def_id.to_string(),
        quantity,
        enchant: 0,
        cape_color: None,
        cape_texture: None,
        locked: false,
    });
}

fn monster(id: &str, kind: &str, x: f32, z: f32) -> Monster {
    Monster {
        id: id.to_string(),
        monster_type: kind.to_string(),
        position: onlinerpg_shared::Position { x, y: 0.0, z },
        rotation: 0.0,
        state: MonsterState::Idle,
        owner_id: None,
        health: 10,
        max_health: 10,
        floor_level: 0,
        level_override: None,
        aggressive: false,
        lifecycle: Default::default(),
        last_attack_at: 0,
        last_move_at: 0,
        move_budget: 0.0,
        owner_since: 0,
    }
}

fn see(s: &mut SharedState, m: Monster) {
    s.nearby_monsters.insert(m.id.clone(), m);
}

fn hurt(s: &mut SharedState, pct: u32) {
    let p = s.self_player.as_mut().unwrap();
    p.max_health = 100;
    p.health = pct;
}

// --- Survival ---

#[test]
fn a_hurt_worker_drinks_while_it_still_has_a_potion() {
    let mut s = state_at(0.0, 0.0);
    hurt(&mut s, 30);
    assert!(!should_drink_potion(&s, &cfg()), "no potion, no drink");
    bag(&mut s, HEALING_POTION, 2);
    assert!(should_drink_potion(&s, &cfg()));
    hurt(&mut s, 90);
    assert!(!should_drink_potion(&s, &cfg()), "healthy again");
}

#[test]
fn the_scroll_is_the_last_resort_only() {
    let mut s = state_at(0.0, 0.0);
    hurt(&mut s, 20);
    bag(&mut s, RETURN_SCROLL, 1);
    assert!(should_use_return_scroll(&s, &cfg()));

    bag(&mut s, HEALING_POTION, 1);
    assert!(
        !should_use_return_scroll(&s, &cfg()),
        "a potion is cheaper than a trip home"
    );
}

/// Rica sells greater_healing_potion; Wick, the night merchant, does not
/// (`merchant_defs.rs`, `a_merchant_stocks_night_essentials`). A worker
/// configured for the greater potion still has to survive the merchant it
/// is actually standing in front of, whichever one carried it home.
#[test]
fn any_potion_in_the_bag_is_drunk_even_if_it_is_not_the_configured_type() {
    let c = WorkerConfig {
        potion_item: Some("greater_healing_potion".into()),
        ..cfg()
    };
    let mut s = state_at(0.0, 0.0);
    hurt(&mut s, 30);
    bag(&mut s, HEALING_POTION, 1);
    assert!(
        should_drink_potion(&s, &c),
        "whatever potion is on hand still saves the fight"
    );
}

#[test]
fn a_configured_restock_item_falls_back_to_whatever_the_nearby_merchant_actually_stocks() {
    let mut s = state_at(0.0, 0.0);
    let mut wick = test_player(3.0, 0.0);
    wick.id = PlayerId::from(2);
    wick.name = "Wick".to_string();
    wick.is_official_npc = true;
    s.nearby_players.insert(wick.id, wick);

    let c = WorkerConfig {
        potion_item: Some("greater_healing_potion".into()),
        ..cfg()
    };
    // Wick does not carry the greater potion: buy what he does stock.
    let (id, _) = potions_to_buy(&s, &c).expect("Wick sells healing potions");
    assert_eq!(id, HEALING_POTION);

    // Swap in Rica, who carries both: the configured potion wins.
    s.nearby_players.clear();
    let mut rica = test_player(3.0, 0.0);
    rica.id = PlayerId::from(3);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    let (id, _) = potions_to_buy(&s, &c).expect("Rica sells healing potions");
    assert_eq!(id, "greater_healing_potion");
}

#[test]
fn eating_waits_for_the_lost_sprint_and_needs_food_in_the_bag() {
    let mut s = state_at(0.0, 0.0);
    bag(&mut s, "apple", 1);
    s.self_hunger = Some((900, HungerState::Normal));
    assert_eq!(should_eat(&s), None);
    // Still the Normal band, but sprinting is already gone — eat now.
    s.self_hunger = Some((onlinerpg_shared::hunger::NORMAL_MIN, HungerState::Normal));
    assert_eq!(should_eat(&s).as_deref(), Some("apple"));
    s.self_hunger = Some((200, HungerState::Hungry));
    assert_eq!(should_eat(&s).as_deref(), Some("apple"));
    s.self_bag.clear();
    s.self_hunger = Some((10, HungerState::Weak));
    assert_eq!(should_eat(&s), None, "nothing to eat");
    // A fisher standing in its own supply should not starve on the way home.
    bag(&mut s, "raw_minnow", 3);
    assert_eq!(should_eat(&s).as_deref(), Some("raw_minnow"));
}

// --- Town trip ---

#[test]
fn a_full_bag_sends_the_worker_to_town() {
    let mut s = state_at(0.0, 0.0);
    s.self_hunger = Some((900, HungerState::Normal));
    assert!(!should_town_trip(&s, &cfg()));
    // 10 STR (test default) → 150 capacity; 140 kg of boots is 93%.
    bag(&mut s, "old_boot", 140);
    assert!(bag_load_pct(&s) >= cfg().bag_full_pct);
    assert!(should_town_trip(&s, &cfg()));
}

#[test]
fn starving_with_no_food_is_a_town_trip_too() {
    let mut s = state_at(0.0, 0.0);
    s.self_hunger = Some((10, HungerState::Weak));
    assert!(should_town_trip(&s, &cfg()));
    bag(&mut s, "apple", 1);
    assert!(!should_town_trip(&s, &cfg()), "it can eat where it stands");
}

#[test]
fn the_town_trip_sells_marked_loot_drops_marked_junk_and_keeps_the_kit() {
    let mut s = state_at(0.0, 0.0);
    let mut wick = test_player(3.0, 0.0);
    wick.id = PlayerId::from(2);
    wick.name = "Wick".to_string();
    wick.is_official_npc = true;
    s.nearby_players.insert(wick.id, wick);
    bag(&mut s, "gold_ring", 1);
    bag(&mut s, "old_boot", 1);
    bag(&mut s, HEALING_POTION, 3);
    bag(&mut s, "apple", 1);
    let labels = labels::BagLabels {
        sellable: vec!["gold_ring".into()],
        dropable: vec!["old_boot".into()],
    };

    assert_eq!(sell_list(&s, &labels), vec!["gold_ring".to_string()]);
    assert_eq!(junk_list(&s, &labels), vec!["old_boot".to_string()]);
    assert_eq!(
        potions_to_buy(&s, &cfg()).map(|(_, n)| n),
        Some(cfg().potion_stock - 3)
    );

    // A purse that covers two potions orders two, not a refused ten.
    let price = crate::item_defs::get(HEALING_POTION)
        .and_then(|d| d.base_price)
        .expect("potions are priced");
    s.self_gold = Some(price * 2);
    assert_eq!(potions_to_buy(&s, &cfg()).map(|(_, n)| n), Some(2));
    s.self_gold = Some(0);
    assert_eq!(potions_to_buy(&s, &cfg()), None);
}

/// Unmarked loot stays in the bag: the app's sell label is what a worker may
/// sell, so a bag of unlabelled drops survives the town trip untouched.
#[test]
fn a_worker_keeps_items_it_was_not_labeled_to_sell() {
    let mut s = state_at(0.0, 0.0);
    bag(&mut s, "gold_ring", 1);
    bag(&mut s, "old_boot", 1);
    let empty = labels::BagLabels::default();

    assert!(
        sell_list(&s, &empty).is_empty(),
        "nothing marked, nothing sold"
    );
    assert!(
        junk_list(&s, &empty).is_empty(),
        "nothing marked, nothing dropped"
    );
}

/// The trip only helps when the shop can help. Starving with nothing to eat
/// and nothing to sell, "go to town" would otherwise repeat forever.
#[test]
fn a_town_trip_with_nothing_to_do_asks_for_nothing() {
    let mut s = state_at(0.0, 0.0);
    s.self_hunger = Some((10, HungerState::Weak));
    s.self_gold = Some(0);
    let labels = labels::BagLabels::default();
    assert!(should_town_trip(&s, &cfg()));
    assert_eq!(town_business(&s, &cfg(), &labels), Vec::new());

    bag(&mut s, "old_boot", 1);
    assert_eq!(
        town_business(&s, &cfg(), &labels),
        Vec::new(),
        "unmarked junk is kept, so the shop has nothing to fix"
    );
}

/// Plenty of items carry no price without being rubbish — a coin pouch pays
/// out when used, a worn starting weapon is the one you fight with.
#[test]
fn only_real_junk_is_dropped() {
    let mut s = state_at(0.0, 0.0);
    bag(&mut s, "sunken_coin_pouch", 1);
    bag(&mut s, "worn_iron_sword", 1);
    bag(&mut s, "clump_of_kelp", 2);
    let labels = labels::BagLabels {
        sellable: vec!["worn_iron_sword".into()],
        dropable: vec!["clump_of_kelp".into()],
    };

    assert_eq!(junk_list(&s, &labels), vec!["clump_of_kelp".to_string()]);
    assert!(
        sell_list(&s, &labels).is_empty(),
        "none of these has a price"
    );
}

/// The town to shop in is the nearest no-spawn zone big enough to hold a
/// merchant, and the search starts at its centre.
#[test]
fn town_is_the_nearest_no_spawn_zone() {
    let mut s = state_at(0.0, 0.0);
    assert_eq!(town_stops(&s).first().copied(), None);
    s.no_spawn_zones = vec![
        NoSpawnZone {
            min_x: 90.0,
            max_x: 110.0,
            min_z: 90.0,
            max_z: 110.0,
        },
        NoSpawnZone {
            min_x: -20.0,
            max_x: 0.0,
            min_z: -20.0,
            max_z: 0.0,
        },
    ];
    assert_eq!(town_stops(&s).first().copied(), Some((-10.0, -10.0)));
    assert_eq!(town_stops(&s).len(), 5, "centre plus four quarters");

    // A map-editor sliver is not a town, however close it sits.
    s.no_spawn_zones.push(NoSpawnZone {
        min_x: -3.0,
        max_x: 4.0,
        min_z: -3.0,
        max_z: 1.0,
    });
    assert_eq!(town_stops(&s).first().copied(), Some((-10.0, -10.0)));
}

// --- Fighter ---

#[test]
fn the_fighter_picks_the_nearest_monster_it_can_beat() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().level = 1;
    // kobold is level 1, orc level 5 — the orc is over a +2 margin.
    see(&mut s, monster("orc-1", "orc", 1.0, 0.0));
    see(&mut s, monster("kobold-far", "kobold", 20.0, 0.0));
    see(&mut s, monster("kobold-near", "kobold", 5.0, 0.0));
    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("kobold-near"),
        "same level either way, so the shorter walk wins"
    );
}

/// Surrounded, the fighter takes the pack apart one at a time: everything
/// crowding us is equally close, so the one already bleeding is finished
/// before a fresh one is touched. Spreading the damage leaves every monster
/// in the group alive and swinging.
#[test]
fn a_surrounded_fighter_finishes_the_wounded_one_first() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().level = 5;
    // hobgoblin is level 5, orc 4, kobold 1 — all inside our own level.
    see(&mut s, monster("kobold-underfoot", "kobold", 1.0, 0.0));
    see(&mut s, monster("hobgoblin-close", "hobgoblin", 2.0, 0.0));
    let mut wounded = monster("orc-wounded", "orc", 4.0, 0.0);
    wounded.health = 2;
    see(&mut s, wounded);

    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("orc-wounded")
    );
    assert_eq!(
        fighter::free_kill(&s, &cfg()).as_deref(),
        Some("orc-wounded"),
        "the walk and the swing agree on which one is being killed"
    );

    // Down. The rest of the pack is untouched, so the nearest is next.
    s.nearby_monsters.remove("orc-wounded");
    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("kobold-underfoot")
    );
}

/// Lowest health only decides between monsters standing the same distance
/// away: a wounded one across the clearing is not worth walking past what is
/// already swinging at us.
#[test]
fn a_wounded_monster_further_out_does_not_outrank_what_is_underfoot() {
    let mut s = state_at(0.0, 0.0);
    see(&mut s, monster("kobold-underfoot", "kobold", 1.0, 0.0));
    let mut far = monster("kobold-far", "kobold", 14.0, 0.0);
    far.health = 1;
    see(&mut s, far);

    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("kobold-underfoot")
    );
}

#[test]
fn the_fighter_leaves_someone_elses_dead_and_off_floor_monsters_alone() {
    let mut s = state_at(0.0, 0.0);
    let mut owned = monster("owned", "kobold", 1.0, 0.0);
    owned.owner_id = Some(PlayerId::from(99));
    see(&mut s, owned);
    let mut dead = monster("dead", "kobold", 1.0, 0.0);
    dead.state = MonsterState::Dead;
    dead.health = 0;
    see(&mut s, dead);
    let mut upstairs = monster("upstairs", "kobold", 1.0, 0.0);
    upstairs.floor_level = 1;
    see(&mut s, upstairs);

    assert_eq!(fighter::eligible_target(&s, &cfg()), None);
    // Nothing to fight is not a reason to stand still: since v37 a spawn is
    // rolled per metre walked, so the fighter patrols instead.
    assert!(
        matches!(
            fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice(),
            [Step::Walk { .. }]
        ),
        "expected a patrol leg, got {:?}",
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default())
    );
}

/// The chase gives up past 20 m, so ordering an attack from further out
/// burns the turn on a refusal — ten of them in a row, in the field.
#[test]
fn a_distant_target_is_walked_up_to_before_it_is_attacked() {
    let mut s = state_at(0.0, 0.0);
    see(&mut s, monster("kobold-far", "kobold", 0.0, 25.0));
    let Step::Walk { x, z } = &fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default())[0]
    else {
        panic!("expected a walk toward the distant target");
    };
    assert_eq!(*x, 0.0);
    assert!(
        (17.0..=20.0).contains(z),
        "should stop short of the target, not on it: {z}"
    );

    see(&mut s, monster("kobold-near", "kobold", 0.0, 10.0));
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Attack("kobold-near".into())]
    );
}

/// `owner_id` is which client runs the monster's AI, not whose monster it
/// is: the server hands the ambient monsters around us to our own
/// connection, so refusing owned ones left the fighter standing in a field
/// of 28 monsters with nothing it would touch.
#[test]
fn the_monsters_assigned_to_us_are_the_ones_to_fight() {
    let mut s = state_at(0.0, 0.0);
    let mut assigned = monster("mine", "kobold", 1.0, 0.0);
    assigned.owner_id = s.self_player_id;
    see(&mut s, assigned);

    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("mine")
    );
}

/// Nothing spawns within 30m of a town, so a fighter parked there (drifted in
/// on a chase, or done shopping) has to walk out before it can hunt again.
#[test]
fn a_fighter_with_nothing_to_hunt_walks_out_of_the_towns_dead_zone() {
    let mut s = state_at(5.0, 0.0);
    // With no town known, there is nothing to escape — that is the patrol's
    // case, not a stand-still.
    assert!(matches!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice(),
        [Step::Walk { .. }]
    ));

    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -20.0,
        max_x: 20.0,
        min_z: -100.0,
        max_z: 100.0,
    }];
    // Nearest way out is +x: 20 + 30 margin + 20 slack, z unchanged.
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: 70.0, z: 0.0 }]
    );

    s.self_player.as_mut().unwrap().position.x = 70.0;
    assert_eq!(
        fighter::escape_target(&s.no_spawn_zones, s.self_player.as_ref().unwrap().position),
        None,
        "clear of the margin: the patrol takes it from here"
    );
}

#[test]
fn a_bigger_margin_widens_the_hunt() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().level = 1;
    see(&mut s, monster("orc-1", "orc", 1.0, 0.0));
    assert_eq!(fighter::eligible_target(&s, &cfg()), None);

    let wide = WorkerConfig {
        level_margin: 10,
        ..cfg()
    };
    assert_eq!(
        fighter::eligible_target(&s, &wide).as_deref(),
        Some("orc-1")
    );
}

#[test]
fn whatever_hits_us_becomes_the_target_however_big_it_is() {
    let me = PlayerId::from(1);
    let hit = |player_id| ServerMessage::MonsterAttackedPlayer {
        monster_id: "orc-1".to_string(),
        player_id,
        hit: true,
        roll: 12,
        damage: 3,
        current_health: 5,
    };
    assert_eq!(attacker_in(&[hit(me)], Some(&me)).as_deref(), Some("orc-1"));
    assert_eq!(attacker_in(&[hit(PlayerId::from(2))], Some(&me)), None);
    assert_eq!(attacker_in(&[], Some(&me)), None);
}

#[test]
fn only_drops_beside_the_kill_are_worth_the_detour() {
    let mut s = state_at(0.0, 0.0);
    s.remember_ground_item(ground_item(1, "gold_ring", 2.0, 0.0, 0));
    s.remember_ground_item(ground_item(2, "gold_ring", 25.0, 0.0, 0));
    let kill = onlinerpg_shared::Position {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    assert_eq!(loot_candidates(&s, kill, LOOT_RADIUS), vec![1]);
}

// --- Fisher ---

#[test]
fn water_is_sea_level_or_a_river_bed() {
    assert!(fisher::is_water(None, Some(-1.5)));
    assert!(fisher::is_water(
        Some(crate::splat::PAL_RIVER_BED),
        Some(3.0)
    ));
    assert!(!fisher::is_water(Some(crate::splat::PAL_SAND), Some(3.0)));
    assert!(!fisher::is_water(None, None));
}

#[test]
fn the_fisher_casts_at_the_nearest_water_and_stops_short_of_it() {
    let samples = [(0.0, 12.0, true), (0.0, 6.0, true), (0.0, 3.0, false)];
    let (x, z, dist) = fisher::nearest_water(0.0, 0.0, &samples).expect("water found");
    assert_eq!((x, z), (0.0, 6.0));
    assert_eq!(dist, 6.0);

    let (sx, sz) = fisher::shore_spot(0.0, 0.0, x, z, dist);
    assert_eq!((sx, sz), (0.0, 1.0), "walk to the shore, not into the sea");
    assert_eq!(fisher::nearest_water(0.0, 0.0, &[(1.0, 1.0, false)]), None);
}

// --- Config ---

#[test]
fn the_worker_table_configures_a_character_and_defaults_the_rest() {
    #[derive(serde::Deserialize)]
    struct Config {
        npcs: Vec<crate::orchestrator::NpcConfig>,
    }
    let parsed: Config = toml::from_str(
        r#"
[[npcs]]
account = "npc_x"

[npcs.worker]
kind = "fighter"
level_margin = 5
"#,
    )
    .expect("config parses");
    let worker = &parsed.npcs[0].worker;
    assert_eq!(worker.kind, WorkerKind::Fighter);
    assert_eq!(worker.level_margin, 5);
    assert_eq!(worker.low_health_pct, default_low_health_pct());
    assert_eq!(worker.potion_stock, default_potion_stock());
}

#[test]
fn a_character_without_a_worker_table_runs_the_llm_agent() {
    #[derive(serde::Deserialize)]
    struct Config {
        npcs: Vec<crate::orchestrator::NpcConfig>,
    }
    let parsed: Config = toml::from_str("[[npcs]]\naccount = \"npc_x\"\n").expect("config parses");
    assert_eq!(parsed.npcs[0].worker.kind, WorkerKind::None);
}

// --- Steps to actions ---

#[test]
fn every_step_renders_the_action_json_the_executor_parses() {
    for (step, kind) in [
        (Step::Attack("orc-1".into()), "attack"),
        (Step::Pickup(7), "pickup"),
        (Step::Use(HEALING_POTION.into()), "use"),
        (Step::Sell("gold_ring".into(), None), "sell"),
        (Step::Drop("old_boot".into()), "drop"),
        (Step::Buy(HEALING_POTION.into()), "buy"),
        (Step::Fish { x: 1.0, z: 2.0 }, "fish"),
        (Step::Walk { x: 1.0, z: 2.0 }, "move"),
        (Step::ToGround { x: 1.0, z: 2.0 }, "move"),
        (
            Step::Descend {
                dungeon: "Old Crypt".into(),
                depth: 2,
            },
            "move",
        ),
        (Step::Surface, "move"),
        (Step::OpenChest, "open_chest"),
    ] {
        let action = step.action().expect("an action");
        assert_eq!(action["type"], kind);
        if kind == "move" {
            assert_eq!(action["sprint"], json!(true));
        }
        let turn = json!({ "actions": [action] }).to_string();
        assert!(
            super::super::action::parse_turn_tolerant(&turn)
                .expect("parses")
                .errors
                .is_empty(),
            "{kind} did not parse"
        );
    }
    assert!(Step::Idle.action().is_none());
}

// --- Repro: a worker switched while the character was underground ---

/// Switching the dungeoneer for the fighter leaves the character wherever the
/// descent had got to. From a dungeon floor `resolve_goal_floor` refuses every
/// goal the fighter has — the anchor, the town, the water all lie outside the
/// footprint — before a route is even looked for, so it fought whatever
/// wandered past and never moved toward what it was for. Same wedge upstairs,
/// which is where a respawn puts it. Climbing out comes before everything.
#[tokio::test]
async fn a_surface_worker_left_off_the_surface_climbs_out_first() {
    let stranded = |kind: WorkerKind, floor: i8| async move {
        let (mut s, _rx) = test_state();
        s.self_player = Some(test_player(0.0, 0.0));
        s.self_player_id = Some(PlayerId::from(1));
        s.self_floor_level = floor;
        // A full bag and prey in reach: the errands that would otherwise win
        // the tick, and both of them unwalkable from off the surface.
        bag(&mut s, "iron_helmet", 40);
        see(&mut s, {
            let mut m = monster("m1", "goblin", 2.0, 0.0);
            m.floor_level = floor;
            m
        });
        let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
        next_step(
            &state,
            &WorkerConfig { kind, ..cfg() },
            &mut Errand::Work,
            &mut None,
            &mut None,
            &mut 0usize,
            "test",
            &labels::BagLabels::default(),
            &mut fighter::Patrol::default(),
            &mut dungeoneer::Run::default(),
        )
        .await
    };

    for kind in [WorkerKind::Fighter, WorkerKind::Fisher] {
        assert_eq!(
            stranded(kind, -2).await,
            vec![Step::Surface],
            "{kind:?} underground"
        );
        assert!(
            matches!(stranded(kind, 1).await.as_slice(), [Step::ToGround { .. }]),
            "{kind:?} upstairs"
        );
    }

    // The dungeoneer is exempt: below ground is where its errand is, and it
    // has its own way back up when the night is done.
    assert_ne!(
        stranded(WorkerKind::Dungeoneer, -2).await,
        vec![Step::Surface],
        "the dungeoneer does not get walked out of its own dungeon"
    );
}

// --- Repro: a full bag walks into town and leaves without selling ---

/// The whole town trip, driven through `next_step` the way the loop does:
/// state in, steps out, errand carried across ticks.
#[tokio::test]
async fn a_full_bag_sells_at_the_merchant_instead_of_turning_round() {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(0.0, 0.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    // Standing in town, where a merchant is in sight.
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -50.0,
        max_x: 50.0,
        min_z: -50.0,
        max_z: 50.0,
    }];
    let mut rica = test_player(4.0, 0.0);
    rica.id = PlayerId::from(2);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    // A bag over the 80% mark with something the app marked sellable.
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    assert!(
        should_town_trip(&s, &cfg()),
        "the bag is what sends it to town"
    );

    let labels = labels::BagLabels {
        sellable: vec!["iron_helmet".to_string()],
        dropable: Vec::new(),
    };
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = None;
    let mut stop = 0usize;
    let steps = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut fighter::Patrol::default(),
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert!(
        steps
            .iter()
            .any(|s| matches!(s, Step::Sell(id, _) if id == "iron_helmet")),
        "expected a sale at the merchant, got {steps:?}"
    );
}

/// The same trip with nothing marked and no money to buy anything: the marks
/// are the gate, so the shop has nothing it may do, and an empty purse means
/// even restocking is not on the table. The worker stays in town rather than
/// walking out to hunt gold it cannot spend — the reason named in the log.
#[tokio::test]
async fn an_unmarked_full_bag_with_no_money_stays_in_town() {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(0.0, 0.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    s.self_gold = Some(0);
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -50.0,
        max_x: 50.0,
        min_z: -50.0,
        max_z: 50.0,
    }];
    let mut rica = test_player(4.0, 0.0);
    rica.id = PlayerId::from(2);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    bag(&mut s, HEALING_POTION, 10);
    assert!(should_town_trip(&s, &cfg()));

    let labels = labels::BagLabels::default();
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = None;
    let mut stop = 0usize;
    macro_rules! tick {
        () => {
            next_step(
                &state,
                &cfg(),
                &mut errand,
                &mut loot_at,
                &mut blocked,
                &mut stop,
                "test",
                &labels,
                &mut fighter::Patrol::default(),
                &mut dungeoneer::Run::default(),
            )
            .await
        };
    }
    assert!(
        matches!(tick!().as_slice(), [Step::Idle]),
        "trip written off, so stay in town: no money to spend anywhere else"
    );
    assert!(
        blocked.is_some_and(|p| p.broke),
        "and mark the pause as broke, so the fighter stays town-bound through it"
    );
    assert!(
        blocked.is_some_and(|p| p.until > Instant::now()),
        "and do not retry it for a while"
    );
    let s = state.lock().await;
    assert!(should_town_trip(&s, &cfg()), "with the bag still full");
}

/// The same trip with a purse that could buy something: the shop still has
/// nothing to do (nothing marked, everything stocked), but the fighter is not
/// broke — so it leaves town and goes hunting, which is how it earns the gold
/// a later trip will actually be able to spend.
#[tokio::test]
async fn an_unmarked_full_bag_with_money_leaves_a_town_that_cannot_help() {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(0.0, 0.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    s.self_gold = Some(500);
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -50.0,
        max_x: 50.0,
        min_z: -50.0,
        max_z: 50.0,
    }];
    let mut rica = test_player(4.0, 0.0);
    rica.id = PlayerId::from(2);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    // Everything the trip would restock is already on hand (no scrolls wanted,
    // so the trip is not routed through the return-scroll escape either), which
    // means a full purse changes nothing: the shop still has no business.
    bag(&mut s, HEALING_POTION, cfg().potion_stock);
    bag(&mut s, "apple", cfg().food_stock);
    let c = WorkerConfig {
        scroll_stock: 0,
        ..cfg()
    };
    assert!(should_town_trip(&s, &c), "the bag wants a town trip");
    assert_eq!(
        town_business(&s, &c, &labels::BagLabels::default()),
        Vec::new()
    );

    let labels = labels::BagLabels::default();
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = None;
    let mut stop = 0usize;
    let steps = next_step(
        &state,
        &c,
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut fighter::Patrol::default(),
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert!(
        matches!(steps.as_slice(), [Step::Walk { .. }]),
        "not broke, so leave the town it cannot use: {steps:?}"
    );
    assert!(
        blocked.is_some_and(|p| p.until > Instant::now() && !p.broke),
        "and pause the retry without keeping the fighter town-bound"
    );
}

/// Through a broke pause the fighter stays town-bound: the field is not the
/// fix for an empty purse, so even with a town trip still wanted it stands
/// put rather than walking out to hunt.
#[tokio::test]
async fn a_broke_fighter_waits_out_its_verdict_in_town() {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(0.0, 0.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((10, HungerState::Weak));
    s.self_gold = Some(0);
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -50.0,
        max_x: 50.0,
        min_z: -50.0,
        max_z: 50.0,
    }];
    let labels = labels::BagLabels::default();
    assert!(should_town_trip(&s, &cfg()), "hungry with nothing to eat");

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = Some(TownPause {
        until: Instant::now() + Duration::from_secs(30),
        useless: true,
        broke: true,
    });
    let mut stop = 0usize;
    let steps = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut fighter::Patrol::default(),
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert!(
        matches!(steps.as_slice(), [Step::Idle]),
        "a broke fighter waits the verdict out in town, got {steps:?}"
    );
}

/// The reported symptom: a full bag, a town trip still on the clock from an
/// earlier visit, and the worker standing just inside the no-spawn margin on
/// its way in. The trip cannot start while blocked, so the errand is still
/// Work — and the fighter's escape rule turns it round at the boundary
/// instead of letting it reach the merchant.
#[tokio::test]
async fn a_blocked_trip_must_not_turn_a_full_bag_round_at_the_boundary() {
    let (mut s, _rx) = test_state();
    // Just inside the 30m margin of a town that starts at x = 0.
    s.self_player = Some(test_player(-20.0, 0.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: 0.0,
        max_x: 60.0,
        min_z: -30.0,
        max_z: 30.0,
    }];
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    let labels = labels::BagLabels {
        sellable: vec!["iron_helmet".to_string()],
        dropable: Vec::new(),
    };
    assert!(should_town_trip(&s, &cfg()), "the bag wants a town trip");

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    // A visit a moment ago (TOWN_VISIT_DELAY) is still on the clock.
    let mut blocked = Some(TownPause {
        until: Instant::now() + Duration::from_secs(30),
        useless: false,
        broke: false,
    });
    let mut stop = 0usize;
    let steps = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut fighter::Patrol::default(),
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert!(
        !matches!(steps.as_slice(), [Step::Walk { x, .. }] if *x < -20.0),
        "walked away from the town it needs: {steps:?}"
    );
}

/// Reported live: a full bag, and the worker parked at the town boundary doing
/// nothing. Driven here through `next_step` from the moment the trip starts,
/// with no merchant anywhere in sight — which is what an unattended town (the
/// merchant NPCs' own sessions are not running) looks like to the worker.
#[tokio::test]
async fn a_full_bag_must_not_park_forever_when_town_cannot_help() {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(-60.0, 0.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -50.0,
        max_x: 50.0,
        min_z: -50.0,
        max_z: 50.0,
    }];
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    let labels = labels::BagLabels {
        sellable: vec!["iron_helmet".to_string()],
        dropable: Vec::new(),
    };
    assert!(should_town_trip(&s, &cfg()));

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = None;
    let mut stop = 0usize;
    macro_rules! tick {
        () => {
            next_step(
                &state,
                &cfg(),
                &mut errand,
                &mut loot_at,
                &mut blocked,
                &mut stop,
                "test",
                &labels,
                &mut fighter::Patrol::default(),
                &mut dungeoneer::Run::default(),
            )
            .await
        };
    }

    // The trip starts: walk to the town anchor.
    assert_eq!(
        tick!(),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "heads for town"
    );

    // Nobody in sight from the centre, so the search walks the four quarters
    // before the town counts as empty — one look from the middle misses a
    // merchant standing further out than NPC_SIGHT_RADIUS.
    macro_rules! stand_at {
        ($x:expr, $z:expr) => {
            state.lock().await.self_player.as_mut().unwrap().position = onlinerpg_shared::Position {
                x: $x,
                y: 0.0,
                z: $z,
            }
        };
    }
    stand_at!(0.0, 0.0);
    for quarter in 0..4 {
        let out = tick!();
        let [Step::Walk { x, z }] = out.as_slice() else {
            panic!("quarter {quarter}: expected a walk to the next stop, got {out:?}");
        };
        assert_eq!(
            (x.abs(), z.abs()),
            (25.0, 25.0),
            "the quarters of a 100x100 town"
        );
        stand_at!(*x, *z);
    }

    // Every stop looked at, still nobody: only now is the trip written off,
    // and writing it off has to get us out of the dead zone rather than park
    // us in it — that wait was what "standing at the town boundary" was.
    let out = tick!();
    let [Step::Walk { x, z }] = out.as_slice() else {
        panic!("expected a walk out of town, got {out:?}");
    };
    // 50 to the edge + 30 margin + 20 slack, on whichever side is nearest;
    // the other coordinate is left where the search ended.
    assert_eq!(x.abs(), 100.0);

    // And out there the fighter has spawns to wait for, not a town to stand in.
    let mut s = state.lock().await;
    s.self_player.as_mut().unwrap().position = onlinerpg_shared::Position {
        x: *x,
        y: 0.0,
        z: *z,
    };
    assert_eq!(
        fighter::escape_target(&s.no_spawn_zones, s.self_player.as_ref().unwrap().position),
        None,
        "clear of the no-spawn margin, where spawns reach us"
    );
}

/// Reported live: a full bag, and the worker never steps into town. Driven
/// from the field, with a merchant standing in the town centre but out of
/// sight until we get there — the ordinary trip, tick by tick.
#[tokio::test]
async fn a_full_bag_walks_into_town_and_sells() {
    let (mut s, _rx) = test_state();
    // Out in the field, well clear of the no-spawn margin.
    s.self_player = Some(test_player(0.0, 300.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -50.0,
        max_x: 50.0,
        min_z: -50.0,
        max_z: 50.0,
    }];
    let mut rica = test_player(0.0, 0.0);
    rica.id = PlayerId::from(2);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    let labels = labels::BagLabels {
        sellable: vec!["iron_helmet".to_string()],
        dropable: Vec::new(),
    };
    assert!(should_town_trip(&s, &cfg()), "the bag wants a town trip");

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = None;
    let mut stop = 0usize;
    macro_rules! tick {
        () => {
            next_step(
                &state,
                &cfg(),
                &mut errand,
                &mut loot_at,
                &mut blocked,
                &mut stop,
                "test",
                &labels,
                &mut fighter::Patrol::default(),
                &mut dungeoneer::Run::default(),
            )
            .await
        };
    }

    // Out of the merchant's sight, so the trip is a walk to the town anchor.
    assert_eq!(
        tick!(),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "a full bag must set off for town"
    );

    // Walk done: the merchant is in sight, so the sale is the next turn.
    state.lock().await.self_player.as_mut().unwrap().position = onlinerpg_shared::Position {
        x: 0.0,
        y: 0.0,
        z: 0.0,
    };
    let steps = tick!();
    assert!(
        steps
            .iter()
            .any(|s| matches!(s, Step::Sell(id, _) if id == "iron_helmet")),
        "expected the sale that empties the bag, got {steps:?}"
    );
}

/// The live failure, in the shape the world had it: Aldermark is 114x118m and
/// sight reaches 43m, so a merchant standing off-centre is invisible from the
/// town's middle. One look from there wrote every trip off and the worker was
/// shoved back out — for hours, with a bag over the threshold the whole time.
#[tokio::test]
async fn a_merchant_across_town_is_found_instead_of_written_off() {
    let (mut s, _rx) = test_state();
    s.self_player = Some(test_player(-1604.0, 4763.0));
    s.self_player_id = Some(PlayerId::from(1));
    s.self_hunger = Some((900, HungerState::Normal));
    // Aldermark, and the 7x4m editor sliver that sits beside it.
    s.no_spawn_zones = vec![
        NoSpawnZone {
            min_x: -1554.419,
            max_x: -1440.459,
            min_z: 4704.431,
            max_z: 4822.621,
        },
        NoSpawnZone {
            min_x: -1447.023,
            max_x: -1439.604,
            min_z: 4770.36,
            max_z: 4774.627,
        },
    ];
    // Rica stands in the town's north-west quarter, 60m from the centre —
    // inside the town, outside NPC_SIGHT_RADIUS of the anchor.
    let mut rica = test_player(-1540.0, 4720.0);
    rica.id = PlayerId::from(2);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    let labels = labels::BagLabels {
        sellable: vec!["iron_helmet".to_string()],
        dropable: Vec::new(),
    };

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let mut loot_at = None;
    let mut blocked = None;
    let mut stop = 0usize;
    macro_rules! tick {
        () => {
            next_step(
                &state,
                &cfg(),
                &mut errand,
                &mut loot_at,
                &mut blocked,
                &mut stop,
                "test",
                &labels,
                &mut fighter::Patrol::default(),
                &mut dungeoneer::Run::default(),
            )
            .await
        };
    }

    // Walk the stops the search hands us, standing on each in turn, until the
    // merchant comes into sight. The sliver zone must not be mistaken for the
    // town: its centre is nowhere near Rica.
    let mut sold = false;
    for _ in 0..6 {
        let steps = tick!();
        if steps.iter().any(|s| matches!(s, Step::Sell(..))) {
            sold = true;
            break;
        }
        let [Step::Walk { x, z }] = steps.as_slice() else {
            panic!("expected a search step or a sale, got {steps:?}");
        };
        assert!(
            *x >= -1554.5 && *x <= -1440.4,
            "the search must stay inside Aldermark, not walk to the sliver: {x}"
        );
        state.lock().await.self_player.as_mut().unwrap().position = onlinerpg_shared::Position {
            x: *x,
            y: 0.0,
            z: *z,
        };
    }
    assert!(
        sold,
        "the trip must reach the merchant it walked to town for"
    );
}

/// Food is bought from the shop in front of us, not from a hardcoded id — the
/// two merchants stock different larders (Wick opens with bread, Rica with
/// apples), and ordering what a shop does not sell spends the turn on a
/// refusal.
#[test]
fn the_food_restock_orders_what_this_merchant_actually_sells() {
    let mut s = state_at(0.0, 0.0);
    let mut wick = test_player(3.0, 0.0);
    wick.id = PlayerId::from(2);
    wick.name = "Wick".to_string();
    wick.is_official_npc = true;
    s.nearby_players.insert(wick.id, wick);

    let (id, count) = food_to_buy(&s, &cfg()).expect("Wick sells food");
    assert_eq!(count, cfg().food_stock, "an empty larder tops right up");
    assert_eq!(
        crate::item_defs::get(&id).and_then(|d| d.category.clone()),
        Some("food".to_string())
    );

    // Meals already carried count against the stock.
    bag(&mut s, &id, 4);
    assert_eq!(
        food_to_buy(&s, &cfg()).map(|(_, n)| n),
        Some(cfg().food_stock - 4)
    );

    // And the purse bounds the order.
    let price = crate::item_defs::get(&id)
        .and_then(|d| d.base_price)
        .expect("food is priced");
    s.self_gold = Some(price * 2);
    assert_eq!(food_to_buy(&s, &cfg()).map(|(_, n)| n), Some(2));

    // A different shop, a different first meal on the shelf.
    s.self_gold = None;
    s.self_bag.clear();
    s.nearby_players.clear();
    let mut rica = test_player(3.0, 0.0);
    rica.id = PlayerId::from(3);
    rica.name = "Rica".to_string();
    rica.is_official_npc = true;
    s.nearby_players.insert(rica.id, rica);
    let (rica_food, _) = food_to_buy(&s, &cfg()).expect("Rica sells food too");
    assert_ne!(rica_food, id, "each merchant's own catalog decides");

    // Nobody to buy from at all: nothing to order.
    s.nearby_players.clear();
    assert_eq!(food_to_buy(&s, &cfg()), None);
}

#[test]
fn a_configured_food_item_is_preferred_when_the_merchant_carries_it() {
    let mut s = state_at(0.0, 0.0);
    let mut wick = test_player(3.0, 0.0);
    wick.id = PlayerId::from(2);
    wick.name = "Wick".to_string();
    wick.is_official_npc = true;
    s.nearby_players.insert(wick.id, wick);

    // Wick stocks bread and jerky; jerky is not the catalog's first meal.
    let c = WorkerConfig {
        food_item: Some("jerky".into()),
        ..cfg()
    };
    let (id, _) = food_to_buy(&s, &c).expect("Wick sells food");
    assert_eq!(id, "jerky");

    // Wick does not carry cheese: fall back to whatever this shop sells.
    let c = WorkerConfig {
        food_item: Some("cheese".into()),
        ..cfg()
    };
    let (id, _) = food_to_buy(&s, &c).expect("Wick sells food");
    assert_ne!(id, "cheese");
}

// --- The patrol circle ---

/// Far enough from the anchor at the origin to be outside the default circle.
fn state_out_of_circle() -> SharedState {
    state_at(fighter::patrol_radius(&cfg()) + 50.0, 0.0)
}

#[test]
fn an_unset_anchor_is_the_worlds_spawn_point() {
    assert_eq!(
        fighter::anchor(&WorkerConfig::default()),
        fighter::spawn_point()
    );
    assert_eq!(fighter::anchor(&cfg()), (0.0, 0.0));
}

/// The circle is the ground the player picked. A monster outside it is not
/// walked to however good the fight looks — that is how a fighter ends up
/// three valleys away from where it was posted.
#[test]
fn a_monster_outside_the_circle_is_not_walked_to() {
    let mut s = state_out_of_circle();
    let me = s.self_player.as_ref().unwrap().position;
    see(
        &mut s,
        monster(
            "kobold-away",
            "kobold",
            me.x + fighter::STRIKE_RANGE + 5.0,
            me.z,
        ),
    );

    assert_eq!(fighter::eligible_target(&s, &cfg()), None);
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "out of the circle with nothing to fight: walk back to the anchor"
    );
}

/// The commute back is not a hunt: a monster standing on the route is walked
/// past, however free the swing would be. Only something that strikes first
/// is fought, and that is the driver loop's retaliation, ahead of any of this.
#[test]
fn nothing_is_fought_on_the_way_back_to_the_anchor() {
    let mut s = state_out_of_circle();
    let me = s.self_player.as_ref().unwrap().position;
    see(&mut s, monster("underfoot", "kobold", me.x + 1.0, me.z));

    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "walk past it, not into it"
    );
    // And the leg must not be armed against what it is walking past, or it
    // would be abandoned and reissued every tick without ever moving.
    assert!(fighter::beyond_circle(&s, &cfg()));
}

/// The interrupt asks whether anything eligible is within reach; target
/// selection asks the same of the patrol circle alone. Something in reach but
/// outside the circle is those two questions with different answers — the
/// chase leg was abandoned the instant it was issued, re-decided unchanged,
/// and abandoned again, 87 times in three minutes with the fighter standing
/// still throughout. Whatever stops a leg has to be what the next decision
/// swings at.
#[test]
fn what_stops_a_leg_is_what_the_next_decision_swings_at() {
    let narrow = WorkerConfig {
        patrol_radius: 10,
        ..cfg()
    };
    let mut s = state_at(9.0, 0.0);
    s.self_player.as_mut().unwrap().level = 18;
    see(&mut s, monster("kobold-in-circle", "kobold", 2.0, 0.0));
    see(&mut s, monster("ogre-outside", "ogre", 13.0, 0.0));

    assert_eq!(
        fighter::eligible_target(&s, &narrow).as_deref(),
        Some("kobold-in-circle"),
        "only what stands in the circle is walked to"
    );
    assert!(
        prey_in_reach(&s, narrow.level_margin),
        "and the ogre at our feet is what would stop that walk"
    );
    assert_eq!(
        fighter::step(&s, &narrow, false, &mut fighter::Patrol::default()),
        vec![Step::Attack("ogre-outside".to_string())],
        "a leg the interrupt would abandon must not be issued at all"
    );
}

/// `town_bound` suppresses the walk back, not the swing: a fighter waiting
/// out a town-trip retry still kills what is standing on it, wherever that
/// is. The walk interrupt is what stays disarmed for a town run — a leg
/// walked to reach a merchant must not be abandoned every time something
/// wanders past.
#[test]
fn a_town_bound_fighter_still_swings_at_what_is_on_top_of_it() {
    let mut s = state_out_of_circle();
    let me = s.self_player.as_ref().unwrap().position;
    see(&mut s, monster("underfoot", "kobold", me.x + 1.0, me.z));

    assert_eq!(
        fighter::step(&s, &cfg(), true, &mut fighter::Patrol::default()),
        vec![Step::Attack("underfoot".to_string())],
        "the errand stops the walk back, not the fight in front of it"
    );
}

#[test]
fn a_town_trip_outranks_the_walk_back() {
    let s = state_out_of_circle();
    assert_eq!(
        fighter::step(&s, &cfg(), true, &mut fighter::Patrol::default()),
        vec![Step::Idle]
    );
}

// --- The ride home ---

#[tokio::test]
async fn a_full_bag_reads_a_scroll_home_and_keeps_the_last_one() {
    let trip = |scrolls: u32| async move {
        let (mut s, _rx) = test_state();
        s.self_player = Some(test_player(0.0, 0.0));
        s.self_player_id = Some(PlayerId::from(1));
        s.self_hunger = Some((900, HungerState::Normal));
        for _ in 0..40 {
            bag(&mut s, "iron_helmet", 1);
        }
        if scrolls > 0 {
            bag(&mut s, RETURN_SCROLL, scrolls);
        }
        assert!(should_town_trip(&s, &cfg()));
        let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
        let (mut errand, mut loot_at, mut blocked, mut stop) = (Errand::Work, None, None, 0usize);
        let labels = labels::BagLabels {
            sellable: vec!["iron_helmet".to_string()],
            dropable: Vec::new(),
        };
        next_step(
            &state,
            &cfg(),
            &mut errand,
            &mut loot_at,
            &mut blocked,
            &mut stop,
            "test",
            &labels,
            &mut fighter::Patrol::default(),
            &mut dungeoneer::Run::default(),
        )
        .await
    };

    assert_eq!(
        trip(3).await,
        vec![Step::Use(RETURN_SCROLL.to_string())],
        "a spare scroll is the ride home"
    );
    assert!(
        !trip(1)
            .await
            .contains(&Step::Use(RETURN_SCROLL.to_string())),
        "the last scroll stays for the low-health escape"
    );
    assert!(
        !trip(0)
            .await
            .contains(&Step::Use(RETURN_SCROLL.to_string())),
        "no scroll, walk"
    );
}

// --- The horse ---

const HORSE_REINS: &str = "horse_reins";

/// The commute back to the anchor is the longest leg a fighter walks, and the
/// reins turn it into a third of one.
#[test]
fn a_long_leg_is_ridden_when_the_bag_holds_reins() {
    let mut s = state_out_of_circle();
    bag(&mut s, HORSE_REINS, 1);

    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![
            Step::Use(HORSE_REINS.to_string()),
            Step::Walk { x: 0.0, z: 0.0 }
        ],
        "mount, then ride home"
    );
}

/// The reins toggle, so asking for them in the saddle climbs off — and a
/// patrol leg is too short to be worth mounting for in the first place.
#[test]
fn a_rider_stays_up_and_a_patrol_leg_is_walked() {
    let mut s = state_out_of_circle();
    bag(&mut s, HORSE_REINS, 1);
    s.self_player.as_mut().unwrap().mounted = true;
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "already riding: just ride"
    );

    let mut s = state_at(0.0, 0.0);
    bag(&mut s, HORSE_REINS, 1);
    let steps = fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default());
    assert!(
        !steps.contains(&Step::Use(HORSE_REINS.to_string())),
        "a patrol leg is shorter than the mount is worth: {steps:?}"
    );
}

/// Every condition the server mounts on, checked here first. A refused
/// toggle spends the turn and answers with a system message, and these are
/// states a worker sits in for seconds at a time — the ten seconds after a
/// kill, a stretch of walk through a house — so asking each tick would walk
/// the whole leg on foot anyway.
#[test]
fn the_mount_is_only_asked_for_where_the_server_would_allow_it() {
    let ride = |s: &SharedState| {
        fighter::step(s, &cfg(), false, &mut fighter::Patrol::default())
            .contains(&Step::Use(HORSE_REINS.to_string()))
    };
    let saddled = || {
        let mut s = state_out_of_circle();
        bag(&mut s, HORSE_REINS, 1);
        s
    };
    assert!(ride(&saddled()), "nothing in the way: mount");

    let mut dead = saddled();
    dead.self_player.as_mut().unwrap().health = 0;
    assert!(!ride(&dead), "a corpse does not climb on");

    let mut below = saddled();
    below.self_floor_level = -1;
    assert!(!ride(&below), "underground");

    let mut seated = saddled();
    seated.self_player.as_mut().unwrap().object_type =
        Some(crate::state::SIT_OBJECT_TYPE.to_string());
    assert!(!ride(&seated), "a pose has to be left first");

    let mut fighting = saddled();
    fighting.note_combat();
    assert!(
        !ride(&fighting),
        "the server's window outlasts the last blow"
    );

    let mut wading = saddled();
    wading.self_player.as_mut().unwrap().position.y = -1.0;
    assert!(
        !ride(&wading),
        "standing in water deeper than the mount allows"
    );
}

/// The out-of-combat window is the server's, and nothing on the wire reports
/// it — so the clock is kept from the two messages that carry a blow.
#[test]
fn a_blow_either_way_starts_the_out_of_combat_clock() {
    let mut s = state_at(0.0, 0.0);
    assert!(!s.in_combat(), "a fresh session has never fought");

    s.self_last_combat_at = Some(Instant::now() - crate::state::OUT_OF_COMBAT);
    assert!(!s.in_combat(), "the window has run out");

    s.note_combat();
    assert!(s.in_combat());
}

/// A refused step taken on horseback is not something to try again: mounted
/// movement is an arc with no wall slide, so the graze a walker slides past
/// stops the horse and drops the whole queue. The worker used to re-decide
/// the identical leg and the horse took the same arc into the same corner,
/// forever. Now the correction takes the rider off and keeps them off.
#[tokio::test]
async fn a_refused_step_on_horseback_ends_the_ride() {
    let mut s = state_out_of_circle();
    bag(&mut s, HORSE_REINS, 1);
    s.self_player.as_mut().unwrap().mounted = true;

    // The leg the fighter would take, ridden, before anything goes wrong.
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "already up: ride it"
    );

    let me = s.self_player.as_ref().unwrap().position;
    s.push_event(ServerMessage::PositionCorrected {
        position: me,
        rotation: 0.0,
        floor_level: 0,
    });
    assert!(s.horse_held(), "a correction taken on horseback holds it");

    // Climbing off outranks the leg: one decided up there is one that will be
    // refused again.
    let instance_id = s.self_bag[0].instance_id;
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    assert_eq!(
        next_step(
            &state,
            &cfg(),
            &mut Errand::Work,
            &mut None,
            &mut None,
            &mut 0usize,
            "test",
            &labels::BagLabels::default(),
            &mut fighter::Patrol::default(),
            &mut dungeoneer::Run::default(),
        )
        .await,
        vec![Step::Use(HORSE_REINS.to_string())]
    );

    // Sending the toggle mirrors it, so the tick after is not still asking.
    let mut s = state.lock().await;
    let _ = s
        .send_command(onlinerpg_shared::ClientMessage::UseItem { instance_id })
        .await;
    assert!(
        !s.self_player.as_ref().unwrap().mounted,
        "a dismount is never refused, so it does not wait for the echo"
    );

    // And the leg that follows is walked, not ridden — the whole point.
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: 0.0, z: 0.0 }],
        "the hold is what stops the very next long leg climbing back up"
    );
}

/// A correction on foot says nothing about the horse — it is the ordinary
/// "the ground refused that step", and the mover already re-paths for it.
#[test]
fn a_correction_on_foot_does_not_hold_the_horse() {
    let mut s = state_out_of_circle();
    bag(&mut s, HORSE_REINS, 1);
    let me = s.self_player.as_ref().unwrap().position;

    s.push_event(ServerMessage::PositionCorrected {
        position: me,
        rotation: 0.0,
        floor_level: 0,
    });

    assert!(!s.horse_held());
    assert!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default())
            .contains(&Step::Use(HORSE_REINS.to_string()))
    );
}

/// The horse is the commute. A Sellable mark on the dearest thing in the bag
/// is a slip of the mouse, not an order to sell the ride.
#[test]
fn the_reins_are_never_sold() {
    let mut s = state_at(0.0, 0.0);
    bag(&mut s, HORSE_REINS, 1);
    let labels = labels::BagLabels {
        sellable: vec![HORSE_REINS.to_string()],
        dropable: Vec::new(),
    };

    assert_eq!(sell_list(&s, &labels), Vec::<String>::new());
}

// --- Surplus supply ---

#[test]
fn supply_past_its_cap_is_sold_without_waiting_for_a_label() {
    let mut s = state_at(0.0, 0.0);
    bag(&mut s, HEALING_POTION, 15);
    bag(&mut s, "bread", 4);
    bag(&mut s, RETURN_SCROLL, 8);
    let c = WorkerConfig {
        potion_stock: 10,
        food_stock: 10,
        scroll_stock: 5,
        ..cfg()
    };

    assert_eq!(
        surplus_list(&s, &c),
        vec![
            (HEALING_POTION.to_string(), Some(5)),
            (RETURN_SCROLL.to_string(), Some(3)),
        ],
        "sell what is over each cap, keep the caps, leave the under-stocked bread"
    );

    // The kit itself is never surplus, however the labels read.
    let empty = labels::BagLabels {
        sellable: Vec::new(),
        dropable: Vec::new(),
    };
    assert!(
        town_business(&s, &c, &empty).contains(&Step::Sell(HEALING_POTION.to_string(), Some(5)))
    );
    assert!(
        sell_list(&s, &empty).is_empty(),
        "loot still needs its mark"
    );
}

/// Since v37 the server rolls a spawn per metre walked (`SPAWN_CHANCE_PER_METER`)
/// and none whatsoever for standing still, so "nothing nearby" has to mean
/// walk, not wait. The leg stays inside the circle: that is the whole promise
/// of an anchor, and the boundary is what turns the walk around.
#[test]
fn a_fighter_with_nothing_to_fight_patrols_its_circle_instead_of_waiting() {
    let s = state_at(0.0, 0.0);
    let me = s.self_player.as_ref().unwrap().position;

    let [Step::Walk { x, z }] =
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice()[..]
    else {
        panic!(
            "expected a patrol leg, got {:?}",
            fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default())
        );
    };

    assert!(
        x.hypot(z) <= fighter::patrol_radius(&cfg()),
        "a leg must not leave the circle: landed {} m out",
        x.hypot(z)
    );
    let walked = (x - me.x).hypot(z - me.z);
    assert!(
        (fighter::PATROL_LEG - 2.0..=fighter::PATROL_LEG + 2.0).contains(&walked),
        "one leg, not a march across the map: {walked}"
    );
}

/// The boundary is what makes it a patrol rather than a departure: standing
/// on the edge, every leg that would leave the circle is declined and the
/// walk reflects back inside.
#[test]
fn a_patrol_leg_from_the_edge_turns_back_inside() {
    let radius = fighter::patrol_radius(&cfg());
    let s = state_at(radius - 1.0, 0.0);
    let me = s.self_player.as_ref().unwrap().position;

    // Heading straight out, which is the leg the circle has to refuse.
    let (x, z) = fighter::patrol_target(&s, me, (0.0, 0.0), radius, Some(0.0), 0)
        .expect("a way back inside");
    assert!(
        x.hypot(z) <= radius,
        "the leg left the circle: {} m out",
        x.hypot(z)
    );
}

/// The patrol is the last resort, not a competitor: anything eligible is
/// fought where it stands.
#[test]
fn a_monster_outranks_the_patrol() {
    let mut s = state_at(0.0, 0.0);
    see(&mut s, monster("kobold", "kobold", 0.0, 5.0));

    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Attack("kobold".into())]
    );
}

/// `is_standable` is what keeps a leg out of a town or a building, and the
/// sweep behind it is what stops a blocked arc becoming a stall.
#[test]
fn the_patrol_turns_off_a_point_it_cannot_stand_on() {
    let mut s = state_at(0.0, 0.0);
    let me = s.self_player.as_ref().unwrap().position;
    let (x, z) = fighter::patrol_target(&s, me, (0.0, 0.0), 100.0, None, 0)
        .expect("a clear heading to start with");

    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: x - 1.0,
        max_x: x + 1.0,
        min_z: z - 1.0,
        max_z: z + 1.0,
    }];

    let (nx, nz) =
        fighter::patrol_target(&s, me, (0.0, 0.0), 100.0, None, 0).expect("another way round");
    assert!(
        (nx - x).hypot(nz - z) > 1.0,
        "expected a different point once the first sits in a town"
    );
}

/// A patrol that is working walks the same leg every time. The first cut of
/// this advanced the arc offset on every leg, so successive legs stretched
/// 28 m, 56 m, 84 m … up to 224 m before wrapping — and since the move
/// executes blocking, a longer leg is a longer stretch of not looking at
/// what spawned behind it.
#[test]
fn consecutive_patrol_legs_are_the_same_length() {
    let mut s = state_at(0.0, 0.0);
    let mut patrol = fighter::Patrol::default();

    for leg in 0..4 {
        let me = s.self_player.as_ref().unwrap().position;
        let [Step::Walk { x, z }] = fighter::step(&s, &cfg(), false, &mut patrol).as_slice()[..]
        else {
            panic!("expected a patrol leg on {leg}");
        };

        let walked = (x - me.x).hypot(z - me.z);
        assert!(
            (fighter::PATROL_LEG - 2.0..=fighter::PATROL_LEG + 2.0).contains(&walked),
            "leg {leg} walked {walked}, not one leg"
        );

        // The walk lands, which is what makes the next one a fresh leg.
        s.self_player.as_mut().unwrap().position.x = x;
        s.self_player.as_mut().unwrap().position.z = z;
    }
}

/// A target that is standable but unreachable — across a river, up a cliff —
/// leaves us exactly where we were. Reissuing it unchanged is how a fighter
/// grinds against the same rock forever, so a leg that did not move us
/// reaches further round the arc.
#[test]
fn a_leg_that_did_not_move_us_reaches_further_round_the_arc() {
    let s = state_at(0.0, 0.0);
    let mut patrol = fighter::Patrol::default();

    let first = fighter::step(&s, &cfg(), false, &mut patrol);
    // Same position on the next tick: the leg never landed.
    let second = fighter::step(&s, &cfg(), false, &mut patrol);

    assert_ne!(
        first, second,
        "a stalled leg must not be reissued unchanged"
    );
}

/// Standing on the anchor itself there is no bearing to carry over, and no
/// level to derive one from either. Without a heading of its own the patrol
/// would leave the fighter idling where it logged in — which is what "it just
/// stands in the village" looks like from the outside.
#[test]
fn a_fighter_standing_on_its_anchor_is_given_somewhere_to_walk() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().level = 1;

    assert!(
        matches!(
            fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice(),
            [Step::Walk { .. }]
        ),
        "a fighter on its anchor must still be given a direction"
    );
}

/// A walk runs to its waypoint no matter what appears, and the server drops
/// ambient spawns about 20 m ahead of a walker inside a ±30° cone off the
/// heading — so the monster worth fighting lands squarely in the stretch the
/// fighter is not looking at. `prey_in_reach` is what lets a leg give way.
#[test]
fn a_walk_gives_way_only_to_something_actually_worth_swinging_at() {
    let mut s = state_at(0.0, 0.0);
    assert!(!prey_in_reach(&s, 0), "empty ground stops nothing");

    // Beyond the strike range the chase would be refused anyway, so it is not
    // worth throwing a leg away for.
    see(
        &mut s,
        monster("far", "kobold", 0.0, fighter::STRIKE_RANGE + 5.0),
    );
    assert!(!prey_in_reach(&s, 0));

    see(&mut s, monster("near", "kobold", 0.0, 5.0));
    assert!(prey_in_reach(&s, 0));
}

/// The margin is the whole point of the flag carrying a number: a monster the
/// fighter would decline to fight must not keep stopping its legs.
#[test]
fn a_walk_does_not_give_way_to_a_monster_out_of_our_league() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().level = 1;
    let mut ogre = monster("ogre", "ogre", 0.0, 5.0);
    ogre.level_override = Some(9);
    see(&mut s, ogre);

    assert!(!prey_in_reach(&s, 0), "out of our league at margin 0");
    assert!(prey_in_reach(&s, 8), "in range once the margin allows it");
}

/// Armed for a leg walked to find a fight, disarmed for one walked to reach a
/// merchant: abandoning the town run every time something wanders past is how
/// a town trip never finishes.
#[tokio::test]
async fn the_walk_interrupt_is_armed_for_hunting_and_not_for_shopping() {
    let mut s = state_at(0.0, 0.0);
    for _ in 0..40 {
        bag(&mut s, "iron_helmet", 1);
    }
    assert!(
        should_town_trip(&s, &cfg()),
        "the bag is what sends it to town"
    );

    let labels = labels::BagLabels {
        sellable: vec!["iron_helmet".to_string()],
        dropable: Vec::new(),
    };
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let (mut loot_at, mut blocked, mut stop) = (None, None, 0usize);
    let mut patrol = fighter::Patrol::default();

    let _ = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut patrol,
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert_eq!(
        state.lock().await.abandon_leg_for,
        None,
        "a town-bound leg must not be abandoned for a passing monster"
    );

    // Empty the bag: nothing sends it to town any more, so it is hunting.
    state.lock().await.self_bag.clear();
    errand = Errand::Work;
    blocked = None;
    let _ = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut patrol,
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert_eq!(state.lock().await.abandon_leg_for, Some(cfg().level_margin));
}

/// The interrupt and the decision have to agree about the commute too: the
/// walk back declines the free kill, so a leg armed against one would be
/// abandoned the moment it started and reissued unchanged for as long as the
/// monster stood there.
#[tokio::test]
async fn the_walk_interrupt_is_disarmed_for_the_commute_back() {
    let mut s = state_out_of_circle();
    let me = s.self_player.as_ref().unwrap().position;
    see(&mut s, monster("underfoot", "kobold", me.x + 1.0, me.z));
    s.self_hunger = Some((900, HungerState::Normal));
    assert!(
        prey_in_reach(&s, cfg().level_margin),
        "in reach, and declined"
    );

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let mut errand = Errand::Work;
    let (mut loot_at, mut blocked, mut stop) = (None, None, 0usize);
    let labels = labels::BagLabels {
        sellable: Vec::new(),
        dropable: Vec::new(),
    };

    let step = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut fighter::Patrol::default(),
        &mut dungeoneer::Run::default(),
    )
    .await;

    assert_eq!(step, vec![Step::Walk { x: 0.0, z: 0.0 }]);
    assert_eq!(state.lock().await.abandon_leg_for, None);
}

/// `next_step` can return before it ever reaches the worker's own decision —
/// the town errand and the loot sweep both do. The arming has to be cleared
/// up front, or a leg walked to reach a merchant inherits it from the tick
/// that was hunting and gets abandoned for the first monster that wanders by.
#[tokio::test]
async fn a_town_errand_never_inherits_the_hunting_arm() {
    let mut s = state_at(0.0, 0.0);
    // Armed by an earlier hunting tick.
    s.abandon_leg_for = Some(0);

    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    // Mid-errand with no town and no merchant to find: an early return.
    let mut errand = Errand::ToTown;
    let (mut loot_at, mut blocked, mut stop) = (None, None, 0usize);
    let labels = labels::BagLabels {
        sellable: Vec::new(),
        dropable: Vec::new(),
    };

    let _ = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut fighter::Patrol::default(),
        &mut dungeoneer::Run::default(),
    )
    .await;

    assert_eq!(
        state.lock().await.abandon_leg_for,
        None,
        "the errand's own walk must not be interruptible"
    );
}

/// Out of food, the trip home fires when the sprint goes — not two thirds
/// further down at `Weak`. Waiting for `Weak` meant starting the walk from as
/// far out as the ring goes at `WEAK_MOVE_MULT`, and `WEAK_CARRY_MULT` shrinks
/// the bag on the way, so the trip that did fire often read as a full-bag one.
#[test]
fn an_empty_larder_sends_the_worker_home_when_the_sprint_goes() {
    let mut s = state_at(0.0, 0.0);
    s.self_hunger = Some((900, HungerState::Normal));
    assert!(!should_town_trip(&s, &cfg()), "well fed, nothing to do");

    // One point past the sprint threshold, which is where `should_eat` acts.
    s.self_hunger = Some((onlinerpg_shared::hunger::NORMAL_MIN, HungerState::Hungry));
    assert!(
        should_town_trip(&s, &cfg()),
        "hungry with an empty bag is the trip, without waiting for Weak"
    );

    // Carrying food, it eats instead of walking home.
    bag(&mut s, "bread", 1);
    assert!(should_eat(&s).is_some());
    assert!(
        !should_town_trip(&s, &cfg()),
        "food in the bag answers hunger without a trip"
    );
}

/// Spawns land in a cone off the heading, so a leg that landed carries its
/// direction into the next one: a fighter that repicked a bearing every tick
/// would scatter its own spawns behind it.
#[test]
fn a_leg_that_landed_carries_its_heading_into_the_next_one() {
    let mut s = state_at(0.0, 0.0);
    let mut patrol = fighter::Patrol::default();

    let [Step::Walk { x, z }] = fighter::step(&s, &cfg(), false, &mut patrol).as_slice()[..] else {
        panic!("expected a patrol leg");
    };
    let first = (z - 0.0).atan2(x - 0.0);

    // The walk landed, so the next leg is a fresh one from there.
    s.self_player.as_mut().unwrap().position.x = x;
    s.self_player.as_mut().unwrap().position.z = z;
    let [Step::Walk { x: nx, z: nz }] =
        fighter::step(&s, &cfg(), false, &mut patrol).as_slice()[..]
    else {
        panic!("expected a second patrol leg");
    };
    let second = (nz - z).atan2(nx - x);

    assert!(
        (first - second).abs() < 0.01,
        "the heading changed on a leg that landed: {first} -> {second}"
    );
}

/// A town that could not help has already answered. Staying town-bound
/// through its retry clock stops the fighter for five minutes, then another
/// five, for as long as the thing it could not fix stays true — and hunger
/// reaching that trigger at `NORMAL_MIN` rather than `Weak` made it somewhere
/// a fighter actually ends up. The verdict pause has to read differently from
/// the breather after a visit that did some business.
#[tokio::test]
async fn a_town_that_cannot_help_does_not_keep_the_fighter_standing() {
    let mut s = state_out_of_circle();
    // Hungry with an empty larder: exactly what a town cannot fix without gold.
    s.self_hunger = Some((onlinerpg_shared::hunger::NORMAL_MIN, HungerState::Hungry));
    assert!(should_town_trip(&s, &cfg()), "the hunger wants a town trip");

    let labels = labels::BagLabels {
        sellable: Vec::new(),
        dropable: Vec::new(),
    };
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(s));
    let (mut errand, mut loot_at, mut stop) = (Errand::Work, None, 0usize);
    let mut patrol = fighter::Patrol::default();

    // The town said no a moment ago and is on its retry clock.
    let mut blocked = Some(TownPause {
        until: Instant::now() + Duration::from_secs(300),
        useless: true,
        broke: false,
    });
    let steps = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut blocked,
        &mut stop,
        "test",
        &labels,
        &mut patrol,
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert!(
        matches!(steps.as_slice(), [Step::Walk { .. }]),
        "should be back at work, not waiting the town out: {steps:?}"
    );

    // A breather after a trip that *did* do business still holds it.
    let mut breather = Some(TownPause {
        until: Instant::now() + Duration::from_secs(30),
        useless: false,
        broke: false,
    });
    let held = next_step(
        &state,
        &cfg(),
        &mut errand,
        &mut loot_at,
        &mut breather,
        &mut stop,
        "test",
        &labels,
        &mut patrol,
        &mut dungeoneer::Run::default(),
    )
    .await;
    assert_eq!(
        held,
        vec![Step::Idle],
        "a productive visit still earns its pause"
    );
}

// --- Dungeoneer ---

mod dungeon {
    use super::*;
    use crate::driver::worker::dungeoneer::{
        current_depth, cycle_done, key_owed, step, Run, CHEST_LOOT_TRIES,
    };
    use crate::dungeon::Dungeon;
    use crate::state::tests::{dungeon_state_at, stand_at};
    use onlinerpg_shared::dungeon::{key_drop_floors, last_locked_depth};
    use onlinerpg_shared::ServerMessage;
    use std::sync::Arc;

    /// Old Crypt: the shallowest dungeon, and the only one whose whole run
    /// fits in a test — one locked floor, whose key is also the chest's.
    fn crypt() -> (SharedState, Arc<Dungeon>) {
        let (s, d, _rx) = dungeon_state_at(-1450.0, 4720.0);
        (s, d)
    }

    /// A margin wide enough that nothing underground is ruled out on level —
    /// these tests are about the descent, not about picking fights.
    /// Nothing marked sellable or dropable, which is what a character with no
    /// labels set in the app carries.
    fn marks() -> labels::BagLabels {
        labels::BagLabels::default()
    }

    /// Burn the chest's loot grace: with nothing on the floor the sweep waits
    /// out its budget before the errand behind it starts.
    fn past_the_sweep(s: &mut SharedState, d: &Dungeon, run: &mut Run) {
        for _ in 0..CHEST_LOOT_TRIES {
            if !matches!(step(s, &cfg(d), run, &marks()).as_slice(), [Step::Idle]) {
                break;
            }
        }
    }

    fn cfg(d: &Dungeon) -> WorkerConfig {
        WorkerConfig {
            kind: WorkerKind::Dungeoneer,
            dungeon_id: Some(d.id.clone()),
            level_margin: 50,
            ..WorkerConfig::default()
        }
    }

    /// Put the character on the surface, away from the entrance stairs.
    fn on_the_surface(s: &mut SharedState) {
        s.self_floor_level = 0;
        s.self_player = Some(test_player(0.0, 0.0));
    }

    /// Stand on `depth`'s arrival landing, the floor's one guaranteed cell.
    fn on_floor(s: &mut SharedState, d: &Dungeon, depth: u8) {
        let landing = d.arrival_position(depth).expect("floor exists");
        let cell = onlinerpg_shared::dungeon::world_to_cell(&d.entrance, landing.x, landing.z);
        stand_at(s, d, depth, cell);
    }

    fn saw(s: &mut SharedState, id: &str, x: f32, z: f32) {
        let mut m = monster(id, "goblin", x, z);
        m.floor_level = s.self_floor_level;
        see(s, m);
    }

    fn chest_emptied(s: &mut SharedState, d: &Dungeon) {
        let player_id = s.self_player_id.unwrap_or_else(|| PlayerId::from(1));
        s.self_player_id = Some(player_id);
        s.push_event(ServerMessage::DungeonChestOpened {
            entrance_id: d.id.clone(),
            player_id,
            item_def_ids: Vec::new(),
            gold: 0,
        });
    }

    #[test]
    fn the_crypt_locks_its_deepest_floor() {
        let (_s, d) = crypt();
        let last = last_locked_depth(d.max_depth()).expect("the crypt locks a floor");
        assert_eq!(
            last,
            d.max_depth(),
            "the crypt's one lock is its boss floor, which is what makes it the short run"
        );
        assert_eq!(key_drop_floors(last), 1..=last - 1);
    }

    /// Depth is read against *this* dungeon: standing underground somewhere
    /// else is not progress in the one we work.
    #[test]
    fn depth_only_counts_inside_our_own_dungeon() {
        let (mut s, d) = crypt();
        on_the_surface(&mut s);
        assert_eq!(current_depth(&s, &d), 0);
        on_floor(&mut s, &d, 2);
        assert_eq!(current_depth(&s, &d), 2);
        // Underground by floor level, but standing outside the footprint.
        s.self_player = Some(test_player(0.0, 0.0));
        assert_eq!(current_depth(&s, &d), 0, "another dungeon is not ours");
    }

    #[test]
    fn the_lock_below_names_the_key_and_whether_we_hold_it() {
        let (mut s, d) = crypt();
        let lock = last_locked_depth(d.max_depth()).unwrap();
        on_floor(&mut s, &d, 1);

        let (need, key, held) = key_owed(&s, &d, 1).expect("a lock stands below floor 1");
        assert_eq!(need, lock);
        assert_eq!(key, d.key_item_id(lock));
        assert!(!held, "nothing in the bag yet");

        bag(&mut s, &key, 1);
        assert!(key_owed(&s, &d, 1).unwrap().2, "the key is in the bag");
    }

    #[test]
    fn without_the_key_it_works_the_floors_that_drop_it() {
        let (mut s, d) = crypt();
        on_the_surface(&mut s);
        let section = key_drop_floors(last_locked_depth(d.max_depth()).unwrap());

        // From the surface: step onto the section's shallowest floor.
        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: *section.start(),
            }]
        );

        // On a section floor with prey in sight: fight, not descend.
        on_floor(&mut s, &d, *section.start());
        let me = s.self_player.as_ref().unwrap().position;
        saw(&mut s, "m1", me.x + 2.0, me.z);
        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Attack("m1".to_string())]
        );
    }

    /// The walk to the entrance is the longest leg of the night, and it is
    /// outdoor ground the whole way — exactly what the horse is for.
    #[test]
    fn the_ride_to_the_entrance_is_taken_when_the_bag_holds_reins() {
        let (mut s, d) = crypt();
        on_the_surface(&mut s);
        bag(&mut s, HORSE_REINS, 1);
        let section = key_drop_floors(last_locked_depth(d.max_depth()).unwrap());

        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![
                Step::Use(HORSE_REINS.to_string()),
                Step::Descend {
                    dungeon: d.name.clone(),
                    depth: *section.start(),
                }
            ],
            "mount, then ride to the doorstep"
        );
    }

    /// Underground the reins buy nothing: the server dismounts anyone below
    /// ground, so asking for them there spends the turn on a refusal — and
    /// asking while already up would climb straight back off.
    #[test]
    fn the_stair_legs_are_never_ridden() {
        let (mut s, d) = crypt();
        let lock = last_locked_depth(d.max_depth()).unwrap();
        on_floor(&mut s, &d, 1);
        bag(&mut s, &d.key_item_id(lock), 1);
        bag(&mut s, HORSE_REINS, 1);

        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 2,
            }],
            "a floor-to-floor leg starts below ground"
        );

        on_the_surface(&mut s);
        s.self_player.as_mut().unwrap().mounted = true;
        let steps = step(&mut s, &cfg(&d), &mut Run::default(), &marks());
        assert!(
            !steps.contains(&Step::Use(HORSE_REINS.to_string())),
            "already riding: {steps:?}"
        );
    }

    /// Put the character on a spot on the floor it already stands on — where
    /// a fight's chase would have left it.
    fn put_at(s: &mut SharedState, at: Position) {
        s.self_player.as_mut().unwrap().position = at;
    }

    /// The sweep leg's target, on the plane the tour is laid out on — a
    /// `Walk` carries no height.
    fn walked_to(steps: &[Step]) -> (f32, f32) {
        match steps {
            [Step::Walk { x, z }] => (*x, *z),
            other => panic!("expected a sweep leg, got {other:?}"),
        }
    }

    fn xz(at: Position) -> (f32, f32) {
        (at.x, at.z)
    }

    fn nearest_to(at: Position, stops: &[Position], skip: &[(f32, f32)]) -> (f32, f32) {
        stops
            .iter()
            .map(|stop| xz(*stop))
            .filter(|stop| !skip.contains(stop))
            .min_by(|a, b| {
                (at.x - a.0)
                    .hypot(at.z - a.1)
                    .total_cmp(&(at.x - b.0).hypot(at.z - b.1))
            })
            .expect("a stop left to sweep")
    }

    /// The fight is a blocking chase and the worker's own tick does not run
    /// while it happens, so the character comes out of one wherever the
    /// monster led it. The tour used to hold a cursor into the layout's own
    /// order and resume at the stop it had been walking to — which, after a
    /// fight had dragged it to the far side of the floor, sent it straight
    /// back over the ground it had just been pulled across.
    #[test]
    fn the_tour_resumes_from_where_the_fight_left_us() {
        let (mut s, d) = crypt();
        let depth = *key_drop_floors(last_locked_depth(d.max_depth()).unwrap()).start();
        on_floor(&mut s, &d, depth);
        let stops = d.sweep_stops(depth);
        assert!(stops.len() >= 3, "a floor with a tour worth walking");

        let mut run = Run::default();
        let heading_for = walked_to(&step(&mut s, &cfg(&d), &mut run, &marks()));

        // The chase ends on the stop furthest from the one we were walking to.
        let dragged_to = *stops
            .iter()
            .max_by(|a, b| {
                (heading_for.0 - a.x)
                    .hypot(heading_for.1 - a.z)
                    .total_cmp(&(heading_for.0 - b.x).hypot(heading_for.1 - b.z))
            })
            .unwrap();
        put_at(&mut s, dragged_to);

        let next = walked_to(&step(&mut s, &cfg(&d), &mut run, &marks()));
        assert_ne!(
            next,
            xz(dragged_to),
            "the stop under our feet is already swept"
        );
        assert_eq!(
            next,
            nearest_to(dragged_to, &stops, &[xz(dragged_to)]),
            "the nearest stop still owed, not the one the interrupted leg was for"
        );
    }

    /// The tour is a set of cells to stand at, not a queue to work through in
    /// whatever order the layout happens to list them. Every leg goes to the
    /// nearest one still owed, so the walk covers the floor instead of
    /// crossing it to reach a cell it stood beside two legs ago — and each
    /// stop is asked for once.
    #[test]
    fn every_sweep_leg_goes_to_the_nearest_stop_still_owed() {
        let (mut s, d) = crypt();
        let depth = *key_drop_floors(last_locked_depth(d.max_depth()).unwrap()).start();
        on_floor(&mut s, &d, depth);
        let stops = d.sweep_stops(depth);

        let mut run = Run::default();
        let mut swept: Vec<(f32, f32)> = Vec::new();
        for _ in 0..stops.len() {
            let me = s.self_player.as_ref().unwrap().position;
            let next = walked_to(&step(&mut s, &cfg(&d), &mut run, &marks()));
            assert_eq!(next, nearest_to(me, &stops, &swept), "from {me:?}");
            swept.push(next);
            put_at(
                &mut s,
                Position {
                    x: next.0,
                    y: 0.0,
                    z: next.1,
                },
            );
        }
        assert_eq!(swept.len(), stops.len(), "every stop, each of them once");
    }

    /// Going deeper outranks a fight. Arming the walk interrupt on a descent
    /// leg made the two trade places forever: the leg was dropped the moment
    /// something came within reach, the swing that followed lost it, and the
    /// descent started over — the character shuttling between the stairs and
    /// a monster it never caught.
    ///
    /// `abandon_leg_for` and `free_kill` have to answer the same question, so
    /// both are off here. Retaliation still fights whatever actually hits us.
    #[test]
    fn a_descent_is_not_dropped_for_a_monster_that_wanders_past() {
        let (mut s, d) = crypt();
        let lock = last_locked_depth(d.max_depth()).unwrap();
        on_floor(&mut s, &d, 1);
        bag(&mut s, &d.key_item_id(lock), 1);
        let me = s.self_player.as_ref().unwrap().position;
        saw(&mut s, "m1", me.x + 2.0, me.z);

        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 2,
            }],
            "prey in reach does not outrank the way down"
        );
        assert_eq!(
            s.abandon_leg_for, None,
            "and the walk it just issued must not be abandoned for that prey"
        );
    }

    /// The same monster, on the ground its key drops from: here kills *are*
    /// the errand, so the leg is worth dropping and the swing is taken.
    #[test]
    fn a_hunting_leg_is_dropped_for_prey() {
        let (mut s, d) = crypt();
        on_floor(&mut s, &d, 1);
        let me = s.self_player.as_ref().unwrap().position;
        saw(&mut s, "m1", me.x + 2.0, me.z);

        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Attack("m1".to_string())]
        );
        assert_eq!(s.abandon_leg_for, Some(cfg(&d).level_margin));
    }

    #[test]
    fn the_key_in_the_bag_is_what_opens_the_way_down() {
        let (mut s, d) = crypt();
        let lock = last_locked_depth(d.max_depth()).unwrap();
        on_floor(&mut s, &d, 1);
        bag(&mut s, &d.key_item_id(lock), 1);

        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 2,
            }],
            "holding the key, the next floor is the whole decision"
        );
    }

    /// The chest is the errand, so the boss floor asks for it however the
    /// guardian is doing — the retaliation rule fights that fight.
    #[test]
    fn the_boss_floor_walks_to_the_chest_and_opens_it() {
        let (mut s, d) = crypt();
        let depth = d.max_depth();
        on_floor(&mut s, &d, depth);

        let chest = d.treasure_position().expect("the crypt has a chest");
        let spot = d.treasure_approach().expect("and a cell beside it");
        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::Walk {
                x: spot.x,
                z: spot.z
            }],
            "the landing is not the chest room — walk to the cell beside the chest"
        );

        // In the chest room, the sighting is what turns the walk into an open.
        let cell = onlinerpg_shared::dungeon::world_to_cell(&d.entrance, chest.x, chest.z);
        let layout = d.layouts().last().unwrap();
        let room = layout.room_at(cell.0, cell.1).unwrap();
        stand_at(&mut s, &d, depth, layout.stand_cell(room.center()));
        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::OpenChest]
        );
    }

    /// The chest bursts its haul onto the floor. Nothing else collects it —
    /// the driver loop's sweep only follows kills — and the way out comes
    /// after it, not before: a merchant cannot be walked to from a floor
    /// underground.
    #[test]
    fn an_emptied_chest_is_swept_up_before_the_climb_out() {
        // Standing where the chest was opened, with it already emptied.
        let at_the_chest = || {
            let (mut s, d) = crypt();
            let depth = d.max_depth();
            let chest = d.treasure_position().unwrap();
            let cell = onlinerpg_shared::dungeon::world_to_cell(&d.entrance, chest.x, chest.z);
            let layout = d.layouts().last().unwrap();
            let room = layout.room_at(cell.0, cell.1).unwrap();
            stand_at(&mut s, &d, depth, layout.stand_cell(room.center()));
            chest_emptied(&mut s, &d);
            (s, d, chest, depth)
        };

        let mut run = Run::default();

        // The drops are broadcast a moment after the open is answered, so the
        // first look comes back empty. Giving up there emptied every chest
        // into thin air.
        let (mut bare, d, chest, depth) = at_the_chest();
        assert_eq!(
            step(&mut bare, &cfg(&d), &mut run, &marks()),
            vec![Step::Idle],
            "an empty first look is the message in flight, not a bare floor"
        );

        let (mut s, d, _, _) = at_the_chest();
        s.remember_ground_item(crate::state::tests::ground_item(
            77,
            "leather_helmet",
            chest.x + 1.5,
            chest.z,
            -(depth as i8),
        ));
        assert_eq!(
            step(&mut s, &cfg(&d), &mut run, &marks()),
            vec![Step::Pickup(77)],
            "the haul comes up once it lands"
        );
        assert!(!run.resupply_due(), "no shopping from underground");

        // Swept clean: out of the dungeon, and only then is the trip due. The
        // grace runs out rather than being cut short by one empty look.
        let (mut swept, d, _, _) = at_the_chest();
        for _ in 0..CHEST_LOOT_TRIES {
            step(&mut swept, &cfg(&d), &mut run, &marks());
        }
        assert_eq!(
            step(&mut swept, &cfg(&d), &mut run, &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 0
            }],
            "out of the dungeon before the shop"
        );
        on_the_surface(&mut swept);
        assert_eq!(
            step(&mut swept, &cfg(&d), &mut run, &marks()),
            vec![Step::Idle]
        );
        assert!(run.resupply_due(), "above ground the haul is worth selling");
    }

    /// Emptying the chest spends the keys, which puts the very same rule back
    /// at the start of the section — once the errand it started is done.
    #[test]
    fn a_spent_chest_sends_it_back_to_bank_the_next_keys() {
        let (mut s, d) = crypt();
        let section = key_drop_floors(last_locked_depth(d.max_depth()).unwrap());
        on_floor(&mut s, &d, d.max_depth());
        chest_emptied(&mut s, &d);

        let mut run = Run::default();
        past_the_sweep(&mut s, &d, &mut run);
        assert!(!cycle_done(&s, &d), "the next key is not banked yet");

        // Nothing on the floor to sweep, so straight out; the town trip is
        // the driver loop's, and after it the section rule takes over again.
        assert_eq!(
            step(&mut s, &cfg(&d), &mut run, &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 0
            }]
        );
        on_the_surface(&mut s);
        step(&mut s, &cfg(&d), &mut run, &marks());
        run.town_trip_started();

        assert_eq!(
            step(&mut s, &cfg(&d), &mut run, &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: *section.start(),
            }],
            "back into the section the key drops in"
        );
    }

    /// A full bag underground refuses pickups server-side, and a key or the
    /// chest's haul refused is the whole errand lost. No merchant down there,
    /// so the marked junk goes on the floor.
    #[test]
    fn a_full_bag_underground_drops_its_marked_junk_where_it_stands() {
        let (mut s, d) = crypt();
        on_floor(&mut s, &d, 1);
        let cfg = WorkerConfig {
            bag_full_pct: 1,
            ..cfg(&d)
        };
        bag(&mut s, "old_boot", 30);
        let marked = labels::BagLabels {
            sellable: Vec::new(),
            dropable: vec!["old_boot".to_string()],
        };

        assert_eq!(
            step(&mut s, &cfg, &mut Run::default(), &marked),
            vec![Step::Drop("old_boot".to_string())]
        );
        // Unmarked, it stays: what the app has not released is not ours to bin.
        assert!(!matches!(
            step(&mut s, &cfg, &mut Run::default(), &marks()).as_slice(),
            [Step::Drop(_)]
        ));
    }

    /// Chest spent and the key already back in the bag: surplus keys buy
    /// nothing, so the only thing left is to wait out of harm's way.
    #[test]
    fn a_finished_cycle_climbs_out_and_waits() {
        let (mut s, d) = crypt();
        let lock = last_locked_depth(d.max_depth()).unwrap();
        on_floor(&mut s, &d, d.max_depth());
        chest_emptied(&mut s, &d);
        bag(&mut s, &d.key_item_id(lock), 1);
        let mut run = Run::default();
        past_the_sweep(&mut s, &d, &mut run);

        assert!(cycle_done(&s, &d));
        assert_eq!(
            step(&mut s, &cfg(&d), &mut run, &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 0
            }],
            "depth 0 is the way back to the surface"
        );

        on_the_surface(&mut s);
        assert_eq!(step(&mut s, &cfg(&d), &mut run, &marks()), vec![Step::Idle]);
    }

    #[test]
    fn dying_past_the_limit_stops_the_run_and_says_so() {
        let (mut s, d) = crypt();
        on_floor(&mut s, &d, 1);
        let cfg = WorkerConfig {
            death_limit: 2,
            ..cfg(&d)
        };
        let mut run = Run::default();

        run.died();
        assert!(
            !matches!(
                step(&mut s, &cfg, &mut run, &marks()).as_slice(),
                [Step::Idle]
            ),
            "one death is not the limit"
        );

        run.died();
        assert_eq!(
            step(&mut s, &cfg, &mut run, &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 0
            }],
            "at the limit it climbs out"
        );
        on_the_surface(&mut s);
        assert_eq!(step(&mut s, &cfg, &mut run, &marks()), vec![Step::Idle]);
        assert!(
            run.take_note().is_some_and(|n| n.contains("died")),
            "the panel is told why it stopped"
        );
    }

    /// The world's respawn point is a bed on a building's first storey, and
    /// it sits inside the Old Crypt's own footprint — so every death lands
    /// the character somewhere no walk and no descent can leave.
    #[test]
    fn a_respawn_upstairs_takes_the_stairs_down_first() {
        let (mut s, d) = crypt();
        on_the_surface(&mut s);
        let bed = onlinerpg_shared::Position {
            x: -1446.7,
            y: 4.4,
            z: 4754.9,
        };
        assert!(
            d.footprint_contains(bed.x, bed.z),
            "the respawn bed stands over the crypt, which is what hides the surface leg"
        );
        s.self_floor_level = 1;
        s.self_player = Some(crate::state::tests::test_player(bed.x, bed.z));
        let spawn = fighter::spawn_point();

        assert_eq!(
            step(&mut s, &cfg(&d), &mut Run::default(), &marks()),
            vec![Step::ToGround {
                x: spawn.0,
                z: spawn.1
            }],
            "ground level first; from a storey a descent has no route at all"
        );
    }

    /// The section tour walks one floor at a time and turns around at the
    /// ends. Jumping back across the section crossed whole floors of
    /// monsters, and hunting is exactly the phase a leg is abandoned for
    /// prey — so the leg was dropped partway, the tour restarted wherever it
    /// stopped, and Ogre Stronghold's 11-14 sweep shuttled over 13 and 14
    /// instead of covering all four.
    #[test]
    fn the_section_tour_turns_around_instead_of_jumping_back() {
        use crate::driver::worker::dungeoneer::next_floor;

        let (first, last) = (11u8, 14u8);
        let mut run = Run::default();
        let mut at = first;
        let mut walked = vec![at];
        for _ in 0..8 {
            let next = next_floor(at, first, last, &mut run);
            assert_eq!(
                next.abs_diff(at),
                1,
                "every hop is a single floor, {at} -> {next}"
            );
            assert!(
                (first..=last).contains(&next),
                "{next} is outside the section"
            );
            at = next;
            walked.push(at);
        }
        assert_eq!(walked, vec![11, 12, 13, 14, 13, 12, 11, 12, 13]);
    }

    /// A descent that never lands is the one stall the level margin and the
    /// death count cannot see: the worker looks busy and reports nothing new.
    #[test]
    fn a_descent_that_never_lands_stops_the_run() {
        let (mut s, d) = crypt();
        on_the_surface(&mut s);
        let mut run = Run::default();

        // The same descent, asked for over and over — what a wedged entrance
        // looks like from in here.
        let mut asked = 0;
        for _ in 0..40 {
            match step(&mut s, &cfg(&d), &mut run, &marks()).as_slice() {
                [Step::Descend { .. }] => asked += 1,
                [Step::Idle] => break,
                other => panic!("unexpected step {other:?}"),
            }
        }
        assert!(asked > 1, "it must try more than once before giving up");
        assert_eq!(
            step(&mut s, &cfg(&d), &mut run, &marks()),
            vec![Step::Idle],
            "a wedged descent stops being reissued"
        );
        assert!(
            run.take_note()
                .is_some_and(|n| n.contains("cannot get where it is going")),
            "the panel is told the leg is not landing"
        );

        // A floor reached is a descent that worked: the count starts over.
        on_floor(&mut s, &d, 1);
        run.observe_epoch(Some(1));
        run.observe_epoch(Some(2));
        let me = s.self_player.as_ref().unwrap().position;
        saw(&mut s, "m1", me.x + 2.0, me.z);
        assert_eq!(
            step(&mut s, &cfg(&d), &mut run, &marks()),
            vec![Step::Attack("m1".to_string())]
        );
    }

    /// A nightfall voids every verdict this cycle reached: the chest owes its
    /// once-a-night again and the death count starts over.
    #[test]
    fn nightfall_starts_a_fresh_cycle() {
        let (mut s, d) = crypt();
        on_floor(&mut s, &d, 1);
        let cfg = WorkerConfig {
            death_limit: 1,
            ..cfg(&d)
        };
        let mut run = Run::default();
        run.observe_epoch(Some(10));
        run.died();
        assert_eq!(
            step(&mut s, &cfg, &mut run, &marks()),
            vec![Step::Descend {
                dungeon: d.name.clone(),
                depth: 0
            }],
            "stopped on the death limit"
        );

        run.observe_epoch(Some(11));
        assert!(
            !matches!(
                step(&mut s, &cfg, &mut run, &marks()).as_slice(),
                [Step::Idle]
            ),
            "a new night is a new run"
        );
    }

    /// The haul is worth a town trip once, and once only — a trip already
    /// taken must not be asked for again.
    #[test]
    fn an_emptied_chest_asks_for_one_town_trip() {
        let (mut s, d) = crypt();
        on_floor(&mut s, &d, d.max_depth());
        let mut run = Run::default();

        step(&mut s, &cfg(&d), &mut run, &marks());
        assert!(
            !run.resupply_due(),
            "nothing sold before the chest is opened"
        );

        chest_emptied(&mut s, &d);
        past_the_sweep(&mut s, &d, &mut run);
        step(&mut s, &cfg(&d), &mut run, &marks());
        on_the_surface(&mut s);
        step(&mut s, &cfg(&d), &mut run, &marks());
        assert!(run.resupply_due());

        run.town_trip_started();
        step(&mut s, &cfg(&d), &mut run, &marks());
        assert!(!run.resupply_due(), "the trip is not asked for twice");
    }

    /// A full bag underground is not a town trip: there is no merchant down
    /// there and no way to reach one that does not throw the descent away.
    #[test]
    fn a_full_bag_underground_is_not_a_town_trip() {
        let (mut s, d) = crypt();
        let cfg = WorkerConfig {
            bag_full_pct: 1,
            ..cfg(&d)
        };
        on_the_surface(&mut s);
        bag(&mut s, "iron_sword", 40);
        assert!(should_town_trip(&s, &cfg), "on the surface it is a trip");

        on_floor(&mut s, &d, 1);
        assert!(!should_town_trip(&s, &cfg));
    }
}
