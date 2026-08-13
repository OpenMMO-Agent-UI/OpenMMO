//! Monster fighter: hunt the best-matched eligible monster, loot the kill, and
//! stand still when nothing is worth attacking.

use std::collections::HashMap;
use std::sync::OnceLock;

use onlinerpg_shared::{pathfinding, Monster, MonsterState, NoSpawnZone, Position};
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

/// The world's spawn point: what the server measures its ambient-spawn
/// distance gates from, and where the return scroll lands.
pub(crate) fn spawn_point() -> (f32, f32) {
    #[derive(Deserialize)]
    struct World {
        #[serde(rename = "spawnPosition")]
        spawn_position: Spawn,
    }
    #[derive(Deserialize)]
    struct Spawn {
        x: f32,
        z: f32,
    }
    static CACHE: OnceLock<(f32, f32)> = OnceLock::new();
    *CACHE.get_or_init(|| {
        serde_json::from_str::<World>(include_str!("../../../../data-src/world.json"))
            .map(|w| (w.spawn_position.x, w.spawn_position.z))
            .unwrap_or((0.0, 0.0))
    })
}

/// Mirrors the server's `AMBIENT_SPAWN_METERS_PER_LEVEL`: a level-N monster
/// is offered only at `(N - 1)` times this far from the spawn point.
const METERS_PER_LEVEL: f32 = 70.0;

/// How far out our own level spawns, capped by the strongest type there is.
/// Not widened by `level_margin`: [`best_eligible`] prefers our own level, so
/// the extra walk would unlock what it then declines to pick.
pub(crate) fn hunt_radius(my_level: u32) -> f32 {
    let strongest = monster_levels().values().copied().max().unwrap_or(1) as u32;
    my_level.min(strongest).saturating_sub(1) as f32 * METERS_PER_LEVEL
}

const BEARING_TRIES: u32 = 8;

/// Somewhere a walk can end: clear of a town's dead zone, not inside a
/// building.
///
/// ponytail: no water test — `sample_height` is async and every decision here
/// is pure and sync. A spot in the sea is walked to and stalled on; sample it
/// in the driver loop if that ever strands one.
fn is_standable(s: &SharedState, x: f32, z: f32) -> bool {
    if s.no_spawn_zones
        .iter()
        .any(|zone| zone.contains_with_margin(x, z, TOWN_MARGIN))
    {
        return false;
    }
    let Ok(world) = s.world_cache.read() else {
        return true;
    };
    !pathfinding::is_movement_blocked(world.passability_cache(), x, z, x, z, 0, None)
}

/// Where to stand to hunt, or `None` when already far enough out — the radius
/// is a floor, not a band, so a chase that drifted further is left alone.
/// Keeps our current bearing from the spawn point, turning it when the spot
/// it lands on is not standable.
pub(crate) fn hunt_target(s: &SharedState, me: Position, my_level: u32) -> Option<(f32, f32)> {
    let (sx, sz) = spawn_point();
    let radius = hunt_radius(my_level);
    let (dx, dz) = (me.x - sx, me.z - sz);
    if dx.hypot(dz) >= radius {
        return None;
    }
    // On the spawn point itself there is no bearing to keep; the retry picks one.
    let bearing = if dx.hypot(dz) > f32::EPSILON {
        dz.atan2(dx)
    } else {
        0.0
    };
    // Slack for the same reason an escape walks it: land on the gate exactly
    // and the next chase's drift drops us back under it.
    let want = radius + ESCAPE_SLACK;
    let step = std::f32::consts::TAU / BEARING_TRIES as f32;
    (0..BEARING_TRIES).find_map(|turn| {
        let angle = bearing + turn as f32 * step;
        let (x, z) = (sx + angle.cos() * want, sz + angle.sin() * want);
        is_standable(s, x, z).then_some((x, z))
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
    let Some(player) = s.self_player.as_ref() else {
        return vec![Step::Idle];
    };
    let me = player.position;
    // Getting to the ring outranks the fodder underfoot — a kobold is eligible
    // at every level, and stopping for each one pins the worker to the weakest
    // ring. Ahead of target selection, or a level-up would never widen the
    // radius: the old ring always has something eligible standing in it.
    // Anything that hits us is still fought — retaliation runs before this.
    if !town_bound {
        if let Some((x, z)) = hunt_target(s, me, player.level) {
            return vec![Step::Walk { x, z }];
        }
    }
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
