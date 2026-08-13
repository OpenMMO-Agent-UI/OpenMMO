//! Monster fighter: hunt the best-matched eligible monster, loot the kill, and
//! stand still when nothing is worth attacking.

use std::collections::HashMap;
use std::sync::OnceLock;

use onlinerpg_shared::{Monster, MonsterState, NoSpawnZone, Position};
use serde::Deserialize;

use super::{Step, WorkerConfig};
use crate::state::{SharedState, TOWN_MARGIN};

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

/// How far past the no-spawn margin an escape walks, so the next kill's drift
/// does not put us straight back inside it.
const ESCAPE_SLACK: f32 = 20.0;

/// Where to walk to get monsters spawning again, or `None` when we are already
/// clear. Nothing spawns within [`TOWN_MARGIN`] of a town, so a fighter that
/// drifted in — or just finished its shopping — must leave before it can hunt.
pub(crate) fn escape_target(zones: &[NoSpawnZone], me: Position) -> Option<(f32, f32)> {
    let zone = zones
        .iter()
        .find(|z| z.contains_with_margin(me.x, me.z, TOWN_MARGIN))?;
    let (min_x, max_x) = (zone.min_x - TOWN_MARGIN, zone.max_x + TOWN_MARGIN);
    let (min_z, max_z) = (zone.min_z - TOWN_MARGIN, zone.max_z + TOWN_MARGIN);
    let pushes = [
        (min_x - ESCAPE_SLACK, me.z, me.x - min_x),
        (max_x + ESCAPE_SLACK, me.z, max_x - me.x),
        (me.x, min_z - ESCAPE_SLACK, me.z - min_z),
        (me.x, max_z + ESCAPE_SLACK, max_z - me.z),
    ];
    pushes
        .iter()
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|&(x, z, _)| (x, z))
}

/// How close the target must be before we swing. The chase itself gives up
/// past 20 m (`MAX_CHASE_DISTANCE` in combat.rs), so an attack ordered from
/// further out is refused before a single step is taken — walk first.
const STRIKE_RANGE: f32 = 15.0;
/// Where to stop when closing on a distant target: inside striking range,
/// not on top of it.
const CLOSE_TO: f32 = 8.0;

/// The monster worth attacking: the one whose level sits closest to ours, and
/// among equals the nearest. The level margin says which fights are allowed;
/// this says which of the allowed ones is worth walking to — a level-matched
/// kill pays the XP a trivial one does not, so it outranks a shorter walk.
pub(crate) fn eligible_target(s: &SharedState, cfg: &WorkerConfig) -> Option<String> {
    best_eligible(s, cfg).map(|m| m.id.clone())
}

fn best_eligible<'a>(s: &'a SharedState, cfg: &WorkerConfig) -> Option<&'a Monster> {
    let player = s.self_player.as_ref()?;
    let (me, my_level) = (player.position, player.level);
    let level_gap = |m: &Monster| my_level.abs_diff(monster_level(m));
    s.nearby_monsters
        .values()
        .filter(|m| is_eligible(s, cfg, m))
        .min_by(|a, b| {
            level_gap(a).cmp(&level_gap(b)).then_with(|| {
                a.position
                    .dist_xz_sq(&me)
                    .total_cmp(&b.position.dist_xz_sq(&me))
            })
        })
}

/// Attack the best eligible monster, closing the gap first when it stands
/// beyond the chase's reach. With nothing to hunt, leave the town's no-spawn
/// area if we are in it and otherwise idle — wandering only walks into trouble.
///
/// `town_bound` says a town trip is wanted, which outranks hunting: a full bag
/// cannot hold loot anyway, and a trip on its retry clock leaves the errand
/// reading `Work` for a minute or more. Escaping through that window marched
/// the worker back out of the town it was trying to reach, over and over.
pub(crate) fn step(s: &SharedState, cfg: &WorkerConfig, town_bound: bool) -> Vec<Step> {
    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return vec![Step::Idle];
    };
    let Some(target) = eligible_target(s, cfg).and_then(|id| s.nearby_monsters.get(&id)) else {
        if town_bound {
            return vec![Step::Idle];
        }
        return match escape_target(&s.no_spawn_zones, me) {
            Some((x, z)) => vec![Step::Walk { x, z }],
            None => vec![Step::Idle],
        };
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
