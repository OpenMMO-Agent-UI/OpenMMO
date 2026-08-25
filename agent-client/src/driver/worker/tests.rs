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
        cape_color: None,
        cape_texture: None,
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

/// Level match outranks distance: a kill at our own level is what pays, and
/// the level margin has already ruled out the fights we would lose.
#[test]
fn a_level_matched_fight_is_worth_the_longer_walk() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().level = 5;
    // hobgoblin is level 5, orc 4, kobold 1 — all inside a +2 margin.
    see(&mut s, monster("kobold-underfoot", "kobold", 1.0, 0.0));
    see(&mut s, monster("orc-close", "orc", 6.0, 0.0));
    see(&mut s, monster("hobgoblin-far", "hobgoblin", 24.0, 0.0));
    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("hobgoblin-far")
    );

    // With the exact match gone, the next-closest level wins — still not the
    // kobold at our feet.
    s.nearby_monsters.remove("hobgoblin-far");
    assert_eq!(
        fighter::eligible_target(&s, &cfg()).as_deref(),
        Some("orc-close")
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
    )
    .await;
    assert!(
        steps
            .iter()
            .any(|s| matches!(s, Step::Sell(id, _) if id == "iron_helmet")),
        "expected a sale at the merchant, got {steps:?}"
    );
}

/// The same trip with nothing marked: the marks are the gate, so the shop has
/// nothing it may do and the trip is written off for five minutes. The worker
/// leaves rather than standing in a town that cannot help it — with the reason
/// named in the log, since the bag stays full either way.
#[tokio::test]
async fn an_unmarked_full_bag_writes_the_town_trip_off_and_leaves() {
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
            )
            .await
        };
    }
    assert!(
        matches!(tick!().as_slice(), [Step::Walk { .. }]),
        "trip written off, so leave the town it cannot use"
    );
    assert!(
        blocked.is_some_and(|t| t > Instant::now()),
        "and do not retry it for a while"
    );
    let s = state.lock().await;
    assert!(should_town_trip(&s, &cfg()), "with the bag still full");
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
    let mut blocked = Some(Instant::now() + Duration::from_secs(30));
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

// --- The hunting ring ---

/// A player `metres` out from the spawn point, on the +x bearing.
fn state_out_from_spawn(metres: f32) -> SharedState {
    let (sx, sz) = fighter::spawn_point();
    state_at(sx + metres, sz)
}

fn from_spawn(x: f32, z: f32) -> f32 {
    let (sx, sz) = fighter::spawn_point();
    (x - sx).hypot(z - sz)
}

#[test]
fn the_ring_is_where_our_own_level_spawns_and_stops_at_the_strongest() {
    assert_eq!(fighter::hunt_radius(1), 0.0, "level 1 hunts anywhere");
    assert_eq!(fighter::hunt_radius(2), 70.0);
    assert_eq!(fighter::hunt_radius(5), 280.0);
    let strongest = fighter::hunt_radius(u32::MAX);
    assert_eq!(
        fighter::hunt_radius(100),
        strongest,
        "past the strongest type the walk buys nothing"
    );
}

#[test]
fn the_fighter_walks_out_to_its_ring_past_fodder_it_would_have_to_chase() {
    let mut s = state_out_from_spawn(10.0);
    s.self_player.as_mut().unwrap().level = 5;
    // Eligible but out of reach, and exactly the trap: chasing this is what
    // keeps a worker pinned to the weakest ring.
    let (sx, sz) = fighter::spawn_point();
    see(
        &mut s,
        monster(
            "kobold-1",
            "kobold",
            sx + 10.0 + fighter::STRIKE_RANGE + 5.0,
            sz,
        ),
    );

    let [Step::Walk { x, z }] =
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice()[..]
    else {
        panic!(
            "expected a walk out, got {:?}",
            fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default())
        );
    };
    assert!(
        (from_spawn(x, z) - (280.0 + 20.0)).abs() < 0.5,
        "walked to {} m from spawn, wanted the level-5 ring plus slack",
        from_spawn(x, z)
    );

    // Standing on the ring, the same kobold is finally worth swinging at.
    let mut there = state_out_from_spawn(300.0);
    there.self_player.as_mut().unwrap().level = 5;
    see(&mut there, monster("kobold-1", "kobold", sx + 301.0, sz));
    assert_eq!(
        fighter::step(&there, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Attack("kobold-1".to_string())]
    );
}

/// The ring outranks *chasing* the fodder, not swinging at it. Something
/// already inside striking range costs no walking, so walking past it buys
/// nothing — and it is what keeps the walk interrupt from stuttering: the
/// interrupt stops a leg the moment prey is in reach, so a fighter that then
/// walked again rather than attacking would grind in place beside it.
#[test]
fn fodder_already_in_reach_is_killed_on_the_way_out() {
    let mut s = state_out_from_spawn(10.0);
    s.self_player.as_mut().unwrap().level = 5;
    let (sx, sz) = fighter::spawn_point();

    // Still inside the ring, so the walk out is what would otherwise happen.
    assert!(fighter::hunt_target(&s, s.self_player.as_ref().unwrap().position, 5, 0).is_some());

    see(&mut s, monster("underfoot", "kobold", sx + 11.0, sz));
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Attack("underfoot".to_string())]
    );
    // Which is exactly what the walk interrupt stops the leg for.
    assert!(prey_in_reach(&s, cfg().level_margin));
}

/// `town_bound` suppresses the walk out to the ring, not the swing: a
/// fighter waiting out a town-trip retry still kills what is standing on it.
/// The walk interrupt is what stays disarmed for a town run — a leg walked to
/// reach a merchant must not be abandoned every time something wanders past.
#[test]
fn a_town_bound_fighter_still_swings_at_what_is_on_top_of_it() {
    let mut s = state_out_from_spawn(10.0);
    s.self_player.as_mut().unwrap().level = 5;
    let (sx, sz) = fighter::spawn_point();
    see(&mut s, monster("underfoot", "kobold", sx + 11.0, sz));

    assert_eq!(
        fighter::step(&s, &cfg(), true, &mut fighter::Patrol::default()),
        vec![Step::Attack("underfoot".to_string())],
        "the errand stops the walk out, not the fight in front of it"
    );
}

#[test]
fn a_town_trip_outranks_the_walk_out() {
    let mut s = state_out_from_spawn(10.0);
    s.self_player.as_mut().unwrap().level = 5;
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

/// The high ground is not worth the climb: a monster up there is skipped, and
/// a fighter that ended up there walks back down instead of idling on it.
#[test]
fn nothing_above_the_height_cap_is_hunted_or_stood_on() {
    let mut s = state_at(0.0, 0.0);
    let mut high = monster("kobold-peak", "kobold", 5.0, 0.0);
    high.position.y = fighter::MAX_WALK_Y + 1.0;
    see(&mut s, high);
    assert_eq!(fighter::eligible_target(&s, &cfg()), None);

    s.self_player.as_mut().unwrap().position.y = fighter::MAX_WALK_Y + 1.0;
    let (sx, sz) = fighter::spawn_point();
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: sx, z: sz }],
        "should head back down to the spawn point"
    );
}

/// Since v37 the server rolls a spawn per metre walked (`SPAWN_CHANCE_PER_METER`)
/// and none whatsoever for standing still, so "nothing nearby" has to mean
/// walk, not wait. The leg holds its distance from the spawn point on
/// purpose: the monster table is gated on that distance
/// (`min_ambient_town_distance`), so drifting inward would quietly drop the
/// types this fighter walked out here for.
#[test]
fn a_fighter_with_nothing_to_fight_patrols_the_ring_instead_of_waiting() {
    let s = state_at(0.0, 0.0);
    let me = s.self_player.as_ref().unwrap().position;
    let (sx, sz) = fighter::spawn_point();
    let before = (me.x - sx).hypot(me.z - sz);

    let [Step::Walk { x, z }] =
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice()[..]
    else {
        panic!(
            "expected a patrol leg, got {:?}",
            fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default())
        );
    };

    let after = (x - sx).hypot(z - sz);
    assert!(
        (after - before).abs() < 1.0,
        "the ring radius is what holds the monster table: {before} -> {after}"
    );
    let walked = (x - me.x).hypot(z - me.z);
    assert!(
        (fighter::PATROL_LEG - 2.0..=fighter::PATROL_LEG + 2.0).contains(&walked),
        "one leg, not a march across the map: {walked}"
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
    let (x, z) = fighter::patrol_target(&s, me, 1, 0).expect("a clear arc to start with");

    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: x - 1.0,
        max_x: x + 1.0,
        min_z: z - 1.0,
        max_z: z + 1.0,
    }];

    let (nx, nz) = fighter::patrol_target(&s, me, 1, 0).expect("another way round");
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

/// `hunt_radius(1)` is zero, so a level-1 fighter's ring is the spawn point
/// it is standing on and `hunt_target` never asks it to move anywhere. Before
/// the patrol that left it idling where it logged in — which is what "it just
/// stands in the village" looks like from the outside.
#[test]
fn a_level_one_fighter_is_given_somewhere_to_walk() {
    let (sx, sz) = fighter::spawn_point();
    let mut s = state_at(sx, sz);
    s.self_player.as_mut().unwrap().level = 1;
    let me = s.self_player.as_ref().unwrap().position;

    assert_eq!(fighter::hunt_radius(1), 0.0);
    assert_eq!(fighter::hunt_target(&s, me, 1, 0), None);

    assert!(
        matches!(
            fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()).as_slice(),
            [Step::Walk { .. }]
        ),
        "a level-1 fighter must still be given a direction"
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
    )
    .await;
    assert_eq!(state.lock().await.abandon_leg_for, Some(cfg().level_margin));
}

/// Stranded above `MAX_WALK_Y` the fighter walks back down to the spawn
/// point — and that leg is armed like any other, so an eligible monster in
/// reach would interrupt it on every tick and it would never get down. Free
/// kills come first here for the same reason they do on the way to the ring.
#[test]
fn a_stranded_fighter_kills_what_is_in_reach_before_climbing_down() {
    let mut s = state_at(0.0, 0.0);
    s.self_player.as_mut().unwrap().position.y = fighter::MAX_WALK_Y + 5.0;

    let (sx, sz) = fighter::spawn_point();
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Walk { x: sx, z: sz }],
        "nothing in reach: head back down"
    );

    // Down at a height it would fight at, and within reach.
    see(&mut s, monster("underfoot", "kobold", 3.0, 0.0));
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default()),
        vec![Step::Attack("underfoot".to_string())]
    );
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

/// Standing on the spawn point reads as bearing zero — due +x — and the walk
/// back down from a peak lands close enough to read the same way. With one
/// bearing the fighter marched at the same mountain, was sent home by
/// `MAX_WALK_Y`, and set off at it again. Nothing here can see how high a
/// spot is, so fanning the bearing out is what breaks the loop.
#[test]
fn the_ring_walk_does_not_set_off_in_the_same_direction_every_time() {
    let (sx, sz) = fighter::spawn_point();
    let mut s = state_at(sx, sz);
    s.self_player.as_mut().unwrap().level = 5;
    let me = s.self_player.as_ref().unwrap().position;

    let first = fighter::hunt_target(&s, me, 5, 0).expect("a ring point");
    let second = fighter::hunt_target(&s, me, 5, 1).expect("another ring point");

    assert_ne!(first, second, "one bearing forever is the mountain loop");
    // Both still land on the ring; only the direction changed.
    for (x, z) in [first, second] {
        assert!((from_spawn(x, z) - (280.0 + 20.0)).abs() < 0.5);
    }
}

/// Getting stranded is the signal that the last bearing walked into a
/// mountain, so it counts as a failed attempt: the ring walk after the climb
/// down sets off somewhere else.
#[test]
fn a_strand_turns_the_next_ring_walk_away_from_the_mountain() {
    let (sx, sz) = fighter::spawn_point();
    let mut s = state_at(sx, sz);
    s.self_player.as_mut().unwrap().level = 5;
    s.self_player.as_mut().unwrap().position.y = fighter::MAX_WALK_Y + 5.0;

    let mut patrol = fighter::Patrol::default();
    assert_eq!(
        fighter::step(&s, &cfg(), false, &mut patrol),
        vec![Step::Walk { x: sx, z: sz }],
        "stranded: head back down"
    );

    // Back down at the spawn point, the ring walk must not retrace it.
    s.self_player.as_mut().unwrap().position.y = 0.0;
    let after = fighter::step(&s, &cfg(), false, &mut patrol);
    let virgin = fighter::step(&s, &cfg(), false, &mut fighter::Patrol::default());
    assert_ne!(after, virgin, "the strand has to change where it sets off");
}
