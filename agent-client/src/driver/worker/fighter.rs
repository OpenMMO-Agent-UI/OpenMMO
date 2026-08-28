//! Monster fighter: hunt the best-matched eligible monster around an anchor
//! point, loot the kill, and patrol the ground nearby when nothing is worth
//! attacking.

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

/// The world's spawn point: where the return scroll lands, and the anchor a
/// fighter falls back to when the player has not picked one.
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

/// Where this fighter works: the configured anchor, else the spawn point.
pub(crate) fn anchor(cfg: &WorkerConfig) -> (f32, f32) {
    match (cfg.anchor_x, cfg.anchor_z) {
        (Some(x), Some(z)) => (x, z),
        _ => spawn_point(),
    }
}

/// Whether we stand outside the circle, which is to say on the commute back
/// to the anchor: the one leg that is neither hunted along nor interrupted.
pub(crate) fn beyond_circle(s: &SharedState, cfg: &WorkerConfig) -> bool {
    s.self_player
        .as_ref()
        .is_some_and(|p| dist_from(anchor(cfg), p.position.x, p.position.z) > patrol_radius(cfg))
}

/// How far from the anchor the fighter may roam. Floored at one metre so a config
/// of zero cannot make every candidate leg illegal.
pub(crate) fn patrol_radius(cfg: &WorkerConfig) -> f32 {
    (cfg.patrol_radius as f32).max(1.0)
}

fn dist_from(anchor: (f32, f32), x: f32, z: f32) -> f32 {
    (x - anchor.0).hypot(z - anchor.1)
}

const BEARING_TRIES: u32 = 8;

/// Somewhere a walk can end: clear of a town's dead zone, not inside a
/// building.
///
/// ponytail: no water or height test — `sample_height` is async and every
/// decision here is pure and sync, so a spot can still land in the sea.
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
///
/// Deliberately no anchor test: this is what `free_kill` asks, and something
/// standing close enough to hit is worth hitting wherever we are. The leash
/// belongs to `eligible_target`, which is what decides where to *walk*.
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

/// How far one patrol leg walks. The move is executed blocking, so this
/// doubles as how long the fighter goes without looking at what spawned
/// behind it — and at `SPAWN_CHANCE_PER_METER` (0.08, about one monster per
/// 12 m) a leg this long is worth roughly two rolls.
pub(crate) const PATROL_LEG: f32 = 28.0;

/// A leg that ends this close to where it began did not happen.
const STALLED_WITHIN: f32 = 1.0;

/// What the patrol carries between ticks: where the last leg was issued from,
/// the heading it walked, and how far round the compass it has had to reach
/// for a point.
#[derive(Debug, Default)]
pub(crate) struct Patrol {
    from: Option<(f32, f32)>,
    heading: Option<f32>,
    turn: u32,
}

impl Patrol {
    /// A leg that moved us keeps its heading, and the next leg carries on the
    /// same way — spawns land in a cone off the heading, so a straight run
    /// puts them in front of us. A leg that left us where we were did not
    /// happen: the target was standable but unreachable (across a river, up a
    /// cliff), and turning further round the compass is the only way past it.
    fn advance(&mut self, me: Position) {
        match self.from {
            Some((x, z)) if (me.x - x).hypot(me.z - z) <= STALLED_WITHIN => {
                self.turn = self.turn.wrapping_add(1)
            }
            Some((x, z)) => {
                self.heading = Some((me.z - z).atan2(me.x - x));
                self.turn = 0;
            }
            None => {}
        }
        self.from = Some((me.x, me.z));
    }
}

/// Where to walk with nothing to fight: one leg on, ending inside the patrol
/// circle.
///
/// The circle is what turns the walk around — a leg that would leave it is
/// declined, so the fighter reflects off the boundary instead of drifting
/// away from the ground the player picked. Since v37 the server rolls a spawn
/// per metre walked and none at all for standing still, so this walk is the
/// whole reason monsters keep coming.
pub(crate) fn patrol_target(
    s: &SharedState,
    me: Position,
    anchor: (f32, f32),
    radius: f32,
    heading: Option<f32>,
    turn: u32,
) -> Option<(f32, f32)> {
    // No heading yet — the first leg, or one that never moved us. Head out
    // from the anchor; standing on it reads as due +x.
    let bearing = heading.unwrap_or_else(|| {
        let (dx, dz) = (me.x - anchor.0, me.z - anchor.1);
        if dx.hypot(dz) > f32::EPSILON {
            dz.atan2(dx)
        } else {
            0.0
        }
    });
    // A leg longer than the circle leaves nowhere legal to put it: from the
    // centre of a 20 m circle every 28 m leg would end outside.
    let leg = PATROL_LEG.min(radius);
    let step = std::f32::consts::TAU / BEARING_TRIES as f32;
    (0..BEARING_TRIES).find_map(|n| {
        let angle = bearing + ((n + turn) % BEARING_TRIES) as f32 * step;
        let (x, z) = (me.x + angle.cos() * leg, me.z + angle.sin() * leg);
        (dist_from(anchor, x, z) <= radius && is_standable(s, x, z)).then_some((x, z))
    })
}

/// How close the target must be before we swing. The chase itself gives up
/// past 20 m (`MAX_CHASE_DISTANCE` in combat.rs), so an attack ordered from
/// further out is refused before a single step is taken — walk first.
pub(super) const STRIKE_RANGE: f32 = 15.0;
/// Where to stop when closing on a distant target: inside striking range,
/// not on top of it.
const CLOSE_TO: f32 = 8.0;

/// The monster worth walking to: the one whose level sits closest to ours,
/// and among equals the nearest — but only among those standing inside the
/// patrol circle. What lies outside it is somebody else's problem; chasing it
/// is how a fighter wanders off the ground the player picked.
pub(crate) fn eligible_target(s: &SharedState, cfg: &WorkerConfig) -> Option<String> {
    best_eligible_within(s, cfg, f32::INFINITY, true).map(|m| m.id.clone())
}

/// The one already inside striking range, if any — a kill that costs no
/// walking at all.
///
/// Not leashed, on purpose: what the circle governs is where the fighter
/// *walks*, and something already in reach costs no walking. It is also what
/// keeps the walk interrupt honest — `abandon_leg_for` stops a leg the moment
/// prey is in reach, and a fighter that then walked again rather than swinging
/// would stutter in place beside a monster it refused to fight. The commute
/// back to the anchor is the one leg neither of them applies to: it is not
/// armed, and nothing on it is fought.
pub(crate) fn free_kill(s: &SharedState, cfg: &WorkerConfig) -> Option<String> {
    best_eligible_within(s, cfg, STRIKE_RANGE, false).map(|m| m.id.clone())
}

/// The best eligible monster within `range` of us, optionally restricted to
/// what stands inside the patrol circle.
fn best_eligible_within<'a>(
    s: &'a SharedState,
    cfg: &WorkerConfig,
    range: f32,
    leashed: bool,
) -> Option<&'a Monster> {
    let player = s.self_player.as_ref()?;
    let (me, my_level) = (player.position, player.level);
    let (anchor, radius) = (anchor(cfg), patrol_radius(cfg));
    let level_gap = |m: &Monster| my_level.abs_diff(monster_level(m));
    s.nearby_monsters
        .values()
        .filter(|m| m.position.dist_xz_sq(&me) <= range * range && is_eligible(s, cfg, m))
        .filter(|m| !leashed || dist_from(anchor, m.position.x, m.position.z) <= radius)
        .min_by(|a, b| {
            level_gap(a).cmp(&level_gap(b)).then_with(|| {
                a.position
                    .dist_xz_sq(&me)
                    .total_cmp(&b.position.dist_xz_sq(&me))
            })
        })
}

/// Walk back if a fight or a town trip took us out of the circle, else kill
/// what is in reach, else walk up to the best target in the circle, else one
/// patrol leg.
///
/// `town_bound` says a town trip is wanted, which outranks every walk here: a
/// full bag cannot hold loot anyway, and a trip on its retry clock leaves the
/// errand reading `Work` for a minute or more. Walking back to the anchor
/// through that window marched the worker out of the town it was reaching for.
pub(crate) fn step(
    s: &SharedState,
    cfg: &WorkerConfig,
    town_bound: bool,
    patrol: &mut Patrol,
) -> Vec<Step> {
    let Some(player) = s.self_player.as_ref() else {
        return vec![Step::Idle];
    };
    let me = player.position;
    let (anchor, radius) = (anchor(cfg), patrol_radius(cfg));
    // Too far out: a chase that ran, or the walk back from a town trip. The
    // commute is not a hunt — whatever stands along it is walked past, and
    // only something that strikes first is fought (retaliation is the driver
    // loop's, ahead of this). `beyond_circle` disarms the walk interrupt over
    // the same leg, so the two agree about it.
    if !town_bound && dist_from(anchor, me.x, me.z) > radius {
        return vec![Step::Walk {
            x: anchor.0,
            z: anchor.1,
        }];
    }
    // Before every walk that remains, because this is the question the walk
    // interrupt asks: `abandon_leg_for` drops a leg the moment anything
    // eligible is in reach, so unless the very next decision is that swing,
    // the abandoned leg is re-decided unchanged and abandoned again. A kobold
    // underfoot with an ogre 18 m out was exactly that — target selection
    // wants the level match, the interrupt wants what is close — and the
    // fighter stood still through 87 abandoned legs in three minutes.
    if let Some(id) = free_kill(s, cfg) {
        return vec![Step::Attack(id)];
    }
    let Some(target) = eligible_target(s, cfg).and_then(|id| s.nearby_monsters.get(&id)) else {
        if town_bound {
            return vec![Step::Idle];
        }
        return match escape_target(&s.no_spawn_zones, me) {
            Some((x, z)) => vec![Step::Walk { x, z }],
            None => {
                patrol.advance(me);
                match patrol_target(s, me, anchor, radius, patrol.heading, patrol.turn) {
                    Some((x, z)) => vec![Step::Walk { x, z }],
                    None => vec![Step::Idle],
                }
            }
        };
    };
    // Everything within striking range was answered above, so this one is
    // beyond it: walk most of the way and let the next tick's chase finish it.
    let (dx, dz) = (target.position.x - me.x, target.position.z - me.z);
    let dist = dx.hypot(dz);
    let ratio = (dist - CLOSE_TO) / dist;
    vec![Step::Walk {
        x: me.x + dx * ratio,
        z: me.z + dz * ratio,
    }]
}
