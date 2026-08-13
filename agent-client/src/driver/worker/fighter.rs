//! Monster fighter: hunt the nearest eligible monster, loot the kill, and
//! stand still when nothing is worth attacking.

use std::collections::HashMap;
use std::sync::OnceLock;

use onlinerpg_shared::{Monster, MonsterState};
use serde::Deserialize;

use super::{Step, WorkerConfig};
use crate::state::SharedState;

/// Combat level per monster type, from the same game data the server uses.
fn monster_levels() -> &'static HashMap<String, u8> {
    #[derive(Deserialize)]
    struct Row {
        #[serde(default = "one")]
        level: u8,
    }
    fn one() -> u8 {
        1
    }
    static CACHE: OnceLock<HashMap<String, u8>> = OnceLock::new();
    CACHE.get_or_init(|| {
        serde_json::from_str::<HashMap<String, Row>>(include_str!("../../../../data/monsters.json"))
            .map(|rows| rows.into_iter().map(|(id, r)| (id, r.level)).collect())
            .unwrap_or_default()
    })
}

/// A monster's level: the dungeon depth override, else its type's level.
pub(crate) fn monster_level(m: &Monster) -> u32 {
    m.level_override
        .or_else(|| monster_levels().get(&m.monster_type).copied())
        .unwrap_or(1) as u32
}

/// Whether the fighter may start a fight with this monster: on our floor,
/// not another player's, alive, and inside the level margin.
///
/// `owner_id` says which client simulates the monster's AI, not who it
/// belongs to — the server assigns the ambient monsters around us to our own
/// connection, so those are exactly the ones there are to fight. Only
/// someone else's assignment is off limits.
pub(crate) fn is_eligible(s: &SharedState, cfg: &WorkerConfig, m: &Monster) -> bool {
    let my_level = s.self_player.as_ref().map_or(1, |p| p.level);
    let mine_or_nobodys = m
        .owner_id
        .is_none_or(|owner| Some(owner) == s.self_player_id);
    m.floor_level == s.self_floor_level
        && mine_or_nobodys
        && m.state != MonsterState::Dead
        && m.health > 0
        && monster_level(m) <= my_level + cfg.level_margin
}

/// How close the target must be before we swing. The chase itself gives up
/// past 20 m (`MAX_CHASE_DISTANCE` in combat.rs), so an attack ordered from
/// further out is refused before a single step is taken — walk first.
const STRIKE_RANGE: f32 = 15.0;
/// Where to stop when closing on a distant target: inside striking range,
/// not on top of it.
const CLOSE_TO: f32 = 8.0;

/// The nearest monster worth attacking, wherever it stands.
pub(crate) fn eligible_target(s: &SharedState, cfg: &WorkerConfig) -> Option<String> {
    nearest_eligible(s, cfg).map(|m| m.id.clone())
}

fn nearest_eligible<'a>(s: &'a SharedState, cfg: &WorkerConfig) -> Option<&'a Monster> {
    let me = s.self_player.as_ref()?.position;
    s.nearby_monsters
        .values()
        .filter(|m| is_eligible(s, cfg, m))
        .min_by(|a, b| {
            a.position
                .dist_xz_sq(&me)
                .total_cmp(&b.position.dist_xz_sq(&me))
        })
}

/// Attack the nearest eligible monster, closing the gap first when it stands
/// beyond the chase's reach. Idle when there is nothing to hunt — wandering
/// only walks into trouble.
pub(crate) fn step(s: &SharedState, cfg: &WorkerConfig) -> Vec<Step> {
    let Some(target) = eligible_target(s, cfg).and_then(|id| s.nearby_monsters.get(&id)) else {
        return vec![Step::Idle];
    };
    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return vec![Step::Idle];
    };
    let (dx, dz) = (target.position.x - me.x, target.position.z - me.z);
    let dist = dx.hypot(dz);
    if dist <= STRIKE_RANGE {
        return vec![Step::Attack(target.id.clone())];
    }
    // Walk most of the way, then let the next tick's chase finish it.
    let ratio = (dist - CLOSE_TO) / dist;
    vec![Step::Walk {
        x: me.x + dx * ratio,
        z: me.z + dz * ratio,
    }]
}
