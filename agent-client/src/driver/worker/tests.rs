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
    assert_eq!(fighter::step(&s, &cfg(), false), vec![Step::Idle]);
}

/// The chase gives up past 20 m, so ordering an attack from further out
/// burns the turn on a refusal — ten of them in a row, in the field.
#[test]
fn a_distant_target_is_walked_up_to_before_it_is_attacked() {
    let mut s = state_at(0.0, 0.0);
    see(&mut s, monster("kobold-far", "kobold", 0.0, 25.0));
    let Step::Walk { x, z } = &fighter::step(&s, &cfg(), false)[0] else {
        panic!("expected a walk toward the distant target");
    };
    assert_eq!(*x, 0.0);
    assert!(
        (17.0..=20.0).contains(z),
        "should stop short of the target, not on it: {z}"
    );

    see(&mut s, monster("kobold-near", "kobold", 0.0, 10.0));
    assert_eq!(
        fighter::step(&s, &cfg(), false),
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
    assert_eq!(fighter::step(&s, &cfg(), false), vec![Step::Idle]);

    s.no_spawn_zones = vec![NoSpawnZone {
        min_x: -20.0,
        max_x: 20.0,
        min_z: -100.0,
        max_z: 100.0,
    }];
    // Nearest way out is +x: 20 + 30 margin + 20 slack, z unchanged.
    assert_eq!(
        fighter::step(&s, &cfg(), false),
        vec![Step::Walk { x: 70.0, z: 0.0 }]
    );

    s.self_player.as_mut().unwrap().position.x = 70.0;
    assert_eq!(
        fighter::escape_target(&s.no_spawn_zones, s.self_player.as_ref().unwrap().position),
        None,
        "clear of the margin: stand still and wait for a spawn"
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
fn the_fighter_walks_out_to_its_ring_past_the_fodder_underfoot() {
    let mut s = state_out_from_spawn(10.0);
    s.self_player.as_mut().unwrap().level = 5;
    // Eligible, adjacent, and exactly the trap: stopping for it is what keeps
    // a worker in the weakest ring.
    let (sx, sz) = fighter::spawn_point();
    see(&mut s, monster("kobold-1", "kobold", sx + 11.0, sz));

    let [Step::Walk { x, z }] = fighter::step(&s, &cfg(), false).as_slice()[..] else {
        panic!(
            "expected a walk out, got {:?}",
            fighter::step(&s, &cfg(), false)
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
        fighter::step(&there, &cfg(), false),
        vec![Step::Attack("kobold-1".to_string())]
    );
}

#[test]
fn a_town_trip_outranks_the_walk_out() {
    let mut s = state_out_from_spawn(10.0);
    s.self_player.as_mut().unwrap().level = 5;
    assert_eq!(fighter::step(&s, &cfg(), true), vec![Step::Idle]);
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
