use super::*;
use crate::orchestrator::AI_TICK;

fn armed_state() -> (SharedState, Player, mpsc::Receiver<ClientMessage>) {
    let (mut s, rx) = test_state();
    let me = test_player(10.0, 20.0);
    s.self_player_id = Some(me.id);
    s.self_player = Some(me.clone());
    s.in_game = true;

    s.monster_ai.set_behavior_trees(
        crate::monster_ai::MonsterAiManager::load_behavior_trees_from_json(include_str!(
            "../../../../data-src/behavior_trees.json"
        )),
    );
    let (mapping, movement) = crate::monster_ai::MonsterAiManager::load_monster_data(include_str!(
        "../../../../data/monsters.json"
    ));
    s.monster_ai.set_type_mapping(mapping);
    s.monster_ai.set_movement_speeds(movement);

    let mut orc = monster("m1");
    orc.monster_type = "orc".to_string();
    orc.position = p(10.0, 0.0, 10.0);
    orc.health = 100;
    orc.max_health = 100;
    orc.owner_id = s.self_player_id;
    s.nearby_monsters.insert(orc.id.clone(), orc.clone());
    s.monster_ai.add_monster(&orc);
    (s, me, rx)
}

/// Hit the orc, then run `seconds` of simulated time at `delta_ms` per tick,
/// exactly as the orchestrator's AI task does. Returns the attacks it swung
/// and the largest distance it covered in one tick.
fn fight(delta_ms: f32, seconds: f32) -> (usize, f32) {
    let (mut s, me, _rx) = armed_state();
    let cache = Arc::clone(&s.world_cache);
    {
        let world = cache.read().unwrap();
        s.monster_ai
            .handle_monster_hit("m1", &me.id, true, 5, world.passability_cache());
    }

    let mut attacks = 0;
    let mut max_step = 0.0f32;
    let mut prev = p(10.0, 0.0, 10.0);
    for _ in 0..(seconds * 1000.0 / delta_ms) as usize {
        let world = cache.read().unwrap();
        let self_pass_floor =
            onlinerpg_shared::dungeon::passability_floor_for_level(s.self_floor_level);
        let SharedState {
            ref self_player,
            ref nearby_players,
            ref mut monster_ai,
            ..
        } = s;
        for cmd in monster_ai.tick_all(
            delta_ms,
            nearby_players,
            self_player.as_ref(),
            self_pass_floor,
            world.passability_cache(),
        ) {
            match cmd {
                ClientMessage::MonsterAttack { .. } => attacks += 1,
                ClientMessage::MonsterMove { position, .. } => {
                    max_step = max_step.max(position.dist_xz_sq(&prev).sqrt());
                    prev = position;
                }
                _ => {}
            }
        }
    }
    (attacks, max_step)
}

/// The agent stands next to a monster it owns and hits it. The monster must
/// swing back — the server leaves us out of every player frame it sends us, so
/// without `self_player` threaded in the brain has no target at all and the
/// tree falls through to wander.
#[tokio::test]
async fn an_owned_monster_fights_the_agent_back() {
    let (attacks, _) = fight(AI_TICK.as_secs_f32() * 1000.0, 10.0);
    assert!(
        attacks > 0,
        "an owned monster never attacked the agent over 10s"
    );
}

/// The web client ticks its brains every animation frame. Ours has to be fine
/// enough to match that, or a whole tick of run speed lands in one step: the
/// brain's own 500ms network throttle stops being what spaces the syncs, and a
/// spectator watches the monster teleport between swings.
#[tokio::test]
async fn the_ai_tick_keeps_pace_with_a_web_client() {
    let (web_attacks, web_step) = fight(1000.0 / 60.0, 10.0);
    let (ours, step) = fight(AI_TICK.as_secs_f32() * 1000.0, 10.0);
    assert_eq!(
        ours, web_attacks,
        "AI_TICK drops attacks a web client lands"
    );
    assert!(
        step <= web_step,
        "AI_TICK outruns the brain's own sync throttle: {step:.2}m per sync vs a web client's {web_step:.2}m"
    );
}
