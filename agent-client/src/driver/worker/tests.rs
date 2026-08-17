//! Worker decisions, asserted as behaviour: given a world snapshot and a
//! config, the worker makes the expected call.

use super::*;
use crate::state::tests::{ground_item, test_player, test_state};
use onlinerpg_shared::hunger::HungerState;
use onlinerpg_shared::inventory::ItemInstance;
use onlinerpg_shared::{Monster, MonsterState, NoSpawnZone, PlayerId};

fn cfg() -> WorkerConfig {
    WorkerConfig {
        kind: WorkerKind::Fighter,
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

#[test]
fn eating_waits_for_the_hunger_band_and_needs_food_in_the_bag() {
    let mut s = state_at(0.0, 0.0);
    bag(&mut s, "apple", 1);
    s.self_hunger = Some((900, HungerState::Normal));
    assert_eq!(should_eat(&s), None);
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
    // 10 STR (test default) → 150 capacity; 130 kg of boots is 87%.
    bag(&mut s, "old_boot", 130);
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
    assert_eq!(potions_to_buy(&s, &cfg()), cfg().potion_stock - 3);

    // A purse that covers two potions orders two, not a refused ten.
    let price = crate::item_defs::get(HEALING_POTION)
        .and_then(|d| d.base_price)
        .expect("potions are priced");
    s.self_gold = Some(price * 2);
    assert_eq!(potions_to_buy(&s, &cfg()), 2);
    s.self_gold = Some(0);
    assert_eq!(potions_to_buy(&s, &cfg()), 0);
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

#[test]
fn town_is_the_nearest_no_spawn_zone() {
    let mut s = state_at(0.0, 0.0);
    assert_eq!(town_anchor(&s), None);
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
    assert_eq!(town_anchor(&s), Some((-10.0, -10.0)));
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
        Some("kobold-near")
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
    assert_eq!(fighter::step(&s, &cfg()), vec![Step::Idle]);
}

/// The chase gives up past 20 m, so ordering an attack from further out
/// burns the turn on a refusal — ten of them in a row, in the field.
#[test]
fn a_distant_target_is_walked_up_to_before_it_is_attacked() {
    let mut s = state_at(0.0, 0.0);
    see(&mut s, monster("kobold-far", "kobold", 0.0, 25.0));
    let Step::Walk { x, z } = &fighter::step(&s, &cfg())[0] else {
        panic!("expected a walk toward the distant target");
    };
    assert_eq!(*x, 0.0);
    assert!(
        (17.0..=20.0).contains(z),
        "should stop short of the target, not on it: {z}"
    );

    see(&mut s, monster("kobold-near", "kobold", 0.0, 10.0));
    assert_eq!(
        fighter::step(&s, &cfg()),
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
        (Step::Sell("gold_ring".into()), "sell"),
        (Step::Drop("old_boot".into()), "drop"),
        (Step::Buy(HEALING_POTION.into()), "buy"),
        (Step::Fish { x: 1.0, z: 2.0 }, "fish"),
        (Step::Walk { x: 1.0, z: 2.0 }, "move"),
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
