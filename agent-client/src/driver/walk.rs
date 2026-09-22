//! Server-approved walking and door handling for fixed and moving targets.
//! [`WalkTo`] and [`Tuning`] define arrival and retry behavior.

use std::sync::Arc;

const WALK_SEGMENT_DISTANCE: f32 = 27.0;
use std::time::{Duration, Instant};

use onlinerpg_shared::housing::{WallDirection, WallVariant};
use onlinerpg_shared::messages::MoveStatus;
use onlinerpg_shared::pathfinding;
use onlinerpg_shared::pathfinding::PathWaypoint;
use onlinerpg_shared::{ClientMessage, PlayerId, Position};
use tokio::sync::Mutex;
use tracing::error;

use crate::dungeon::{DoorApproach, Dungeon};
use crate::geom::PlanarDelta;
use crate::state::SharedState;

const ATTACK_RANGE: f32 = 2.0;

const APPROACH_RANGE: f32 = 1.0;

const PICKUP_ARRIVE_RANGE: f32 = 2.0;

const MAX_CHASE_DISTANCE: f32 = 20.0;

const REROUTE_THRESHOLD: f32 = 1.5;

const MAX_CHASE_SECS: f32 = 15.0;

const MAX_APPROACH_SECS: f32 = 30.0;

const MAX_PICKUP_WALK_SECS: f32 = 12.0;

const MAX_POINT_WALK_SECS: f32 = 15.0;

const MAX_DOORS_PER_WALK: usize = 6;

const MAX_CHASE_DOORS: usize = 2;

const DOOR_TOGGLE_WAIT: Duration = Duration::from_millis(400);

const MAX_DOOR_PROBES: usize = 6;

const MAX_DOOR_SEARCH_DIST: f32 = 40.0;

/// The same underground, where the "map" is one `GRID`-metre floor: the
/// surface radius is shorter than the floor, so the door that is the only way
/// on can sit outside it and never be probed. `MAX_DOOR_PROBES` still bounds
/// the cost, and candidates are still tried nearest-first.
pub(crate) fn door_search_dist(underground: bool) -> f32 {
    if underground {
        onlinerpg_shared::dungeon::GRID as f32 * std::f32::consts::SQRT_2
    } else {
        MAX_DOOR_SEARCH_DIST
    }
}

pub(super) enum WalkTo<'a> {
    Monster(&'a str),
    Character(&'a PlayerId),
    GroundItem(u64),

    Point {
        pos: Position,
        floor_level: i8,
        arrive_range: f32,
    },

    Place {
        x: f32,
        z: f32,
        floor: u8,
    },
}

struct Tuning {
    arrive_range: f32,

    max_distance: f32,
    max_secs: f32,

    needs_clear_line: bool,
    max_doors: usize,
}

impl WalkTo<'_> {
    fn position(&self, s: &SharedState) -> Option<Position> {
        match self {
            Self::Monster(id) => s.nearby_monsters.get(*id).map(|m| m.position),
            Self::Character(id) => s.nearby_players.get(*id).map(|p| p.position),
            Self::GroundItem(id) => s.ground_item(*id).map(|i| i.position),
            Self::Point { pos, .. } => Some(*pos),
            Self::Place { x, z, .. } => Some(Position {
                x: *x,
                y: 0.0,
                z: *z,
            }),
        }
    }

    fn floor(&self, s: &SharedState) -> u8 {
        let level = match self {
            Self::Monster(id) => s.nearby_monsters.get(*id).map(|m| m.floor_level),
            Self::Character(id) => s.nearby_players.get(*id).map(|p| p.floor_level),
            Self::GroundItem(id) => s.ground_item(*id).map(|i| i.floor_level),
            Self::Point { floor_level, .. } => Some(*floor_level),
            Self::Place { floor, .. } => return *floor,
        };
        onlinerpg_shared::dungeon::passability_floor_for_level(level.unwrap_or(0))
    }

    fn tuning(&self) -> Tuning {
        match self {
            // Characters carry a little arrive slack over APPROACH_RANGE so
            // reaching the pulled-back path goal always counts as in range.
            Self::Character(_) => Tuning {
                arrive_range: APPROACH_RANGE + 0.2,
                max_distance: WALK_SEGMENT_DISTANCE,
                max_secs: MAX_APPROACH_SECS,
                needs_clear_line: false,
                max_doors: MAX_CHASE_DOORS,
            },
            Self::Monster(_) => Tuning {
                arrive_range: ATTACK_RANGE,
                max_distance: MAX_CHASE_DISTANCE,
                max_secs: MAX_CHASE_SECS,
                needs_clear_line: true,
                max_doors: MAX_CHASE_DOORS,
            },
            Self::GroundItem(_) => Tuning {
                arrive_range: PICKUP_ARRIVE_RANGE,
                max_distance: WALK_SEGMENT_DISTANCE,
                max_secs: MAX_PICKUP_WALK_SECS,
                needs_clear_line: false,
                max_doors: MAX_CHASE_DOORS,
            },
            // A fixed point is only ever offered once we share its room, so
            // the sight radius is slack, not a leash.
            Self::Point { arrive_range, .. } => Tuning {
                arrive_range: *arrive_range,
                max_distance: WALK_SEGMENT_DISTANCE,
                max_secs: MAX_POINT_WALK_SECS,
                needs_clear_line: false,
                max_doors: MAX_CHASE_DOORS,
            },
            // Arrival is the goal cell itself: A* smooths its last waypoint
            // onto it, so anything short of standing there is still walking.
            Self::Place { .. } => Tuning {
                arrive_range: 0.1,
                max_distance: f32::INFINITY,
                max_secs: f32::INFINITY,
                needs_clear_line: false,
                max_doors: MAX_DOORS_PER_WALK,
            },
        }
    }
}

impl std::fmt::Display for WalkTo<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Monster(id) => f.write_str(id),
            Self::Character(id) => write!(f, "{id}"),
            Self::GroundItem(id) => write!(f, "item {id}"),
            Self::Point { pos, .. } => write!(f, "point ({:.1}, {:.1})", pos.x, pos.z),
            Self::Place { x, z, .. } => write!(f, "({x:.1}, {z:.1})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum LostReason {
    TargetGone,

    PlayerDied,

    TooFar(f32),

    Timeout,

    NoPath,

    LockedDoor,

    Desynced,
    /// Given up part-way because something worth fighting turned up. Only a
    /// worker asks for this (`SharedState::abandon_leg_for`); the caller is
    /// expected to re-decide rather than treat it as a failure.
    PreyInReach,
}

impl LostReason {
    pub(super) fn clause(&self) -> String {
        match self {
            Self::TargetGone => "your target is no longer there".to_string(),
            Self::PlayerDied => "you died on the way".to_string(),
            Self::TooFar(d) => format!("your target is {d:.0}m away, beyond your reach"),
            Self::Timeout => "you ran out of time before arriving".to_string(),
            Self::NoPath => "no route leads there from here".to_string(),
            Self::LockedDoor => {
                "the way on is a locked door and you hold no key for it".to_string()
            }
            Self::Desynced => "the ground kept refusing your steps".to_string(),
            Self::PreyInReach => "something worth fighting is here".to_string(),
        }
    }
}

/// Whether this leg is one a worker is willing to give up for something worth
/// fighting.
///
/// Only a walk to a *place* — a patrol leg, the commute back to the anchor.
/// Never a walk that is already aimed at something: `chase_monster` is the
/// approach an attack makes, and the monster it is closing on is inside
/// `STRIKE_RANGE` by construction, because that is how it got picked. Asking
/// `prey_in_reach` there answers yes on the first pass through this loop, so
/// arming the interrupt for it aborted every attack before it landed and the
/// fighter could not hit anything at all.
fn interruptible(to: &WalkTo<'_>) -> bool {
    matches!(to, WalkTo::Place { .. })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Walked {
    Arrived,

    Lost(LostReason),
    Error,
}

/// Re-plans after learning a route's water cells, at most this many times.
const DRY_REPLANS: usize = 3;
/// Sample spacing along a planned leg when looking for water.
const WATER_PROBE_M: f32 = 1.0;

/// A surface route that keeps out of water: plan, probe the legs for water,
/// and re-plan around what was found. Where no dry way exists, the plain
/// route is taken so the walk is never refused for it.
async fn plan_route(
    state: &Arc<Mutex<SharedState>>,
    goal: (f32, f32),
    floor: u8,
) -> pathfinding::PathResult {
    for _ in 0..DRY_REPLANS {
        let (plan, start, height, splat) = {
            let s = state.lock().await;
            if floor != 0 || s.self_floor_level != 0 {
                return s.find_path_to(goal.0, goal.1, floor);
            }
            let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
                return s.find_path_to(goal.0, goal.1, floor);
            };
            (
                s.find_dry_path_to(goal.0, goal.1, floor),
                (me.x, me.z),
                Arc::clone(&s.height_sampler),
                Arc::clone(&s.splat_sampler),
            )
        };
        if !plan.found {
            break;
        }
        let wet = wet_cells_along(&height, &splat, start, &plan.waypoints).await;
        let mut s = state.lock().await;
        let known = s.wet_cells.len();
        s.wet_cells.extend(wet);
        if s.wet_cells.len() == known {
            return plan;
        }
    }
    let s = state.lock().await;
    let dry = s.find_dry_path_to(goal.0, goal.1, floor);
    if dry.found {
        dry
    } else {
        s.find_path_to(goal.0, goal.1, floor)
    }
}

async fn wet_cells_along(
    height: &onlinerpg_terrain::height::HeightSampler,
    splat: &crate::splat::SplatSampler,
    start: (f32, f32),
    route: &[PathWaypoint],
) -> Vec<(i32, i32)> {
    let mut wet = Vec::new();
    let mut from = start;
    for wp in route {
        let (dx, dz) = (wp.x - from.0, wp.z - from.1);
        let n = (dx.hypot(dz) / WATER_PROBE_M).ceil().max(1.0) as u32;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let (x, z) = (from.0 + dx * t, from.1 + dz * t);
            let h = height.sample_height(x, z).await.ok();
            let surface = splat.dominant_at(x, z).await.ok();
            let cell = (
                onlinerpg_shared::wrap_world_x(x).floor() as i32,
                z.floor() as i32,
            );
            if super::worker::fisher::is_water(surface, h) && !wet.contains(&cell) {
                wet.push(cell);
            }
        }
        from = (wp.x, wp.z);
    }
    wet
}

/// The goal to hand the server, kept out of water. The server routes through
/// anything its passability grid calls walkable, and a lake is walkable — only
/// we know where the shallows are. Where our own dry route bends round one,
/// hand over the bend instead of the far shore; everywhere else the goal
/// stands, so a walk with no water in it is still one request.
async fn dry_goal(state: &Arc<Mutex<SharedState>>, goal: (f32, f32)) -> (f32, f32) {
    let route = plan_route(state, goal, 0).await;
    let s = state.lock().await;
    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return goal;
    };
    let wet: Vec<(i32, i32)> = s.wet_cells.iter().copied().collect();
    if wet.is_empty() || !pathfinding::segment_enters_cells(me.x, me.z, goal.0, goal.1, &wet) {
        return goal;
    }
    route
        .waypoints
        .iter()
        .find(|wp| PlanarDelta::to_xz(&me, wp.x, wp.z).dist > REROUTE_THRESHOLD)
        .map(|wp| (wp.x, wp.z))
        .unwrap_or(goal)
}

async fn eat_on_the_move(state: &Arc<Mutex<SharedState>>) {
    let mut s = state.lock().await;
    let Some(instance_id) = super::worker::snack(&s) else {
        return;
    };
    s.snacked_at = s.self_hunger.map(|(satiation, _)| satiation);
    if let Err(e) = s
        .send_background_command(ClientMessage::UseItem { instance_id })
        .await
    {
        error!("Failed to eat on the move: {e}");
    }
}

pub(super) async fn walk(
    state: &Arc<Mutex<SharedState>>,
    to: &WalkTo<'_>,
    background: bool,
    sprint: Option<bool>,
) -> Walked {
    let mut owned_request = None;
    let result = walk_inner(state, to, background, sprint, &mut owned_request).await;
    let mut s = state.lock().await;
    if owned_request == Some(s.move_request_id) {
        let _ = s
            .send_flagged_command(ClientMessage::PlayerMoveStop { request_id: 0 }, background)
            .await;
    }
    result
}

async fn walk_inner(
    state: &Arc<Mutex<SharedState>>,
    to: &WalkTo<'_>,
    background: bool,
    sprint: Option<bool>,
    owned_request: &mut Option<u32>,
) -> Walked {
    let tuning = to.tuning();
    let started = Instant::now();
    let mut last_goal: Option<(f32, f32)> = None;
    let mut request_id = None;
    let mut sent_at = Instant::now();
    let mut doors_opened = 0;
    let relocations = state.lock().await.relocations;
    loop {
        eat_on_the_move(state).await;
        if started.elapsed().as_secs_f32() > tuning.max_secs {
            return Walked::Lost(LostReason::Timeout);
        }
        let mut s = state.lock().await;
        if s.relocations != relocations {
            return Walked::Lost(LostReason::Desynced);
        }
        let Some(target) = to.position(&s) else {
            return Walked::Lost(LostReason::TargetGone);
        };
        let Some(me) = s.self_player.as_ref().filter(|p| p.health > 0) else {
            return Walked::Lost(LostReason::PlayerDied);
        };
        // Checked here, mid-walk, because this loop is the only place a long
        // walk is interruptible at all: the server otherwise walks the goal
        // out however good the thing that spawned in front of it, and it
        // drops ambient spawns about 20m ahead of a walker. The lock is
        // already held and the check is a scan of what is nearby.
        if interruptible(to) {
            if let Some(margin) = s.abandon_leg_for {
                if super::worker::prey_in_reach(&s, margin) {
                    return Walked::Lost(LostReason::PreyInReach);
                }
            }
        }
        let delta = PlanarDelta::between(&me.position, &target);
        let target_floor = to.floor(&s);
        if delta.dist <= tuning.arrive_range
            && s.passability_floor() == target_floor
            && !(tuning.needs_clear_line && s.attack_line_blocked(target.x, target.z))
        {
            return Walked::Arrived;
        }
        if delta.dist > tuning.max_distance {
            return Walked::Lost(LostReason::TooFar(delta.dist));
        }
        let mut goal = (target.x, target.z);
        if s.passability_floor() != target_floor {
            let route = s.find_path_to(target.x, target.z, target_floor);
            if let Some(point) = route
                .waypoints
                .iter()
                .find(|p| p.floor != s.passability_floor())
            {
                goal = (point.x, point.z);
            }
        }
        let to_goal = PlanarDelta::to_xz(&me.position, goal.0, goal.1);
        if to_goal.dist > 48.0 {
            goal = (
                me.position.x + to_goal.dx * 48.0 / to_goal.dist,
                me.position.z + to_goal.dz * 48.0 / to_goal.dist,
            );
        }
        let changed = last_goal
            .is_none_or(|(x, z)| PlanarDelta::xz(x, z, goal.0, goal.1).dist > REROUTE_THRESHOLD);
        let status = (request_id == Some(s.move_request_id))
            .then_some(s.move_status)
            .flatten();
        if request_id.is_some() && request_id != Some(s.move_request_id) {
            return Walked::Lost(LostReason::Desynced);
        }
        let terminal = status
            .is_some_and(|status| !matches!(status, MoveStatus::Moving | MoveStatus::Searching));
        if terminal && status != Some(MoveStatus::Arrived) && !changed {
            drop(s);
            if doors_opened < tuning.max_doors
                && open_blocking_door(state, background, sprint, owned_request).await
            {
                doors_opened += 1;
                request_id = None;
                last_goal = None;
                continue;
            }
            let s = state.lock().await;
            return Walked::Lost(if locked_door_without_key(&s) {
                LostReason::LockedDoor
            } else {
                LostReason::NoPath
            });
        }
        if request_id.is_none()
            || (changed && sent_at.elapsed() >= Duration::from_millis(200))
            || terminal
        {
            if target_floor == 0 && s.passability_floor() == 0 {
                drop(s);
                goal = dry_goal(state, goal).await;
                s = state.lock().await;
            }
            match s.request_move(goal.0, goal.1, background, sprint).await {
                Ok(id) => {
                    request_id = Some(id);
                    *owned_request = Some(id);
                }
                Err(_) => return Walked::Error,
            }
            last_goal = Some(goal);
            sent_at = Instant::now();
        } else if sent_at.elapsed() > Duration::from_secs(60) {
            return Walked::Lost(LostReason::Timeout);
        }
        drop(s);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

struct DoorCandidate {
    toggle: ClientMessage,
    sides: [(f32, f32); 2],
}

async fn open_blocking_door(
    state: &Arc<Mutex<SharedState>>,
    background: bool,
    sprint: Option<bool>,
    owned_request: &mut Option<u32>,
) -> bool {
    let mut candidates = {
        let s = state.lock().await;
        let Some(player) = s.self_player.as_ref() else {
            return false;
        };
        let cap = door_search_dist(s.self_floor_level < 0);
        let mut doors: Vec<_> = closed_doors_on_our_floor(&s)
            .into_iter()
            .map(|door| {
                let side = door
                    .sides
                    .into_iter()
                    .min_by(|a, b| {
                        player
                            .position
                            .dist_xz_sq(&Position {
                                x: a.0,
                                y: 0.0,
                                z: a.1,
                            })
                            .total_cmp(&player.position.dist_xz_sq(&Position {
                                x: b.0,
                                y: 0.0,
                                z: b.1,
                            }))
                    })
                    .unwrap();
                (
                    PlanarDelta::to_xz(&player.position, side.0, side.1).dist,
                    door,
                    side,
                )
            })
            .filter(|(distance, _, _)| *distance <= cap)
            .collect();
        doors.sort_by(|a, b| a.0.total_cmp(&b.0));
        doors
    };
    for (_, door, side) in candidates.drain(..).take(MAX_DOOR_PROBES) {
        let id = {
            let mut s = state.lock().await;
            let Ok(id) = s.request_move(side.0, side.1, background, sprint).await else {
                return false;
            };
            *owned_request = Some(id);
            id
        };
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let mut s = state.lock().await;
            if s.move_request_id != id {
                return false;
            }
            match s.move_status {
                Some(MoveStatus::Arrived) => {
                    if s.send_flagged_command(door.toggle, background)
                        .await
                        .is_err()
                    {
                        return false;
                    }
                    drop(s);
                    tokio::time::sleep(DOOR_TOGGLE_WAIT).await;
                    return true;
                }
                Some(MoveStatus::Moving | MoveStatus::Searching) | None => {}
                _ => break,
            }
        }
    }
    false
}

fn dungeon_doors_here(s: &SharedState) -> Option<(Arc<Dungeon>, u8, Vec<DoorApproach>, bool)> {
    let dungeon = s.dungeon_here()?;
    let depth = s.self_floor_level.unsigned_abs();
    let open = s
        .world_cache
        .read()
        .unwrap()
        .open_dungeon_doors(&dungeon.id, depth);
    let doors = dungeon.closed_doors(depth, &open);
    let has_key = s.holds_item(&dungeon.key_item_id(depth));
    Some((dungeon, depth, doors, has_key))
}

fn locked_door_without_key(s: &SharedState) -> bool {
    dungeon_doors_here(s)
        .is_some_and(|(_, _, doors, has_key)| !has_key && doors.iter().any(|d| d.locked))
}

fn closed_doors_on_our_floor(s: &SharedState) -> Vec<DoorCandidate> {
    if s.self_floor_level < 0 {
        let Some((dungeon, depth, doors, has_key)) = dungeon_doors_here(s) else {
            return Vec::new();
        };
        return doors
            .into_iter()
            .filter(|d| !d.locked || has_key)
            .map(|d| DoorCandidate {
                toggle: ClientMessage::ToggleDungeonDoor {
                    entrance_id: dungeon.id.clone(),
                    depth,
                    door_id: d.door_id,
                },
                sides: d.sides,
            })
            .collect();
    }

    let floor = s.self_floor_level as u8;
    let world = s.world_cache.read().unwrap();
    let mut out = Vec::new();
    for house in world.houses_for(
        s.self_player_id
            .unwrap_or_else(|| onlinerpg_shared::PlayerId::from(0)),
    ) {
        // Cells are indexed from the house origin; the floor grid's own origin
        // cancels out (see `pathfinding::update_door_edge`).
        let ox = house.origin.x.floor() as i32;
        let oz = house.origin.z.floor() as i32;
        for (room_index, room) in house.rooms.iter().enumerate() {
            if room.floor_level != floor {
                continue;
            }
            for dir in [
                WallDirection::North,
                WallDirection::South,
                WallDirection::East,
                WallDirection::West,
            ] {
                for (seg, wall) in room.wall(dir).iter().enumerate() {
                    // Windows are openable too, but they are not a way through.
                    if wall.variant != WallVariant::WithDoor || wall.is_open {
                        continue;
                    }
                    let ((dx, dz, _), (adx, adz, _)) = pathfinding::door_cells(room, dir, seg);
                    let (rx, rz) = (ox + room.local_x, oz + room.local_z);
                    out.push(DoorCandidate {
                        toggle: ClientMessage::ToggleDoor {
                            house_id: house.id.clone(),
                            room_index: room_index as u32,
                            wall_dir: dir,
                            segment_index: seg as u32,
                        },
                        sides: [
                            ((rx + dx) as f32 + 0.5, (rz + dz) as f32 + 0.5),
                            ((rx + adx) as f32 + 0.5, (rz + adz) as f32 + 0.5),
                        ],
                    });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The interrupt exists for a leg walked to *find* a fight. A walk that is
    /// already aimed at a monster is the approach an attack makes, and the
    /// monster is inside `STRIKE_RANGE` by construction — that is how it got
    /// picked — so `prey_in_reach` answers yes on the first pass and the
    /// chase aborts before it lands. Arming it there meant the fighter could
    /// not hit anything at all.
    #[test]
    fn only_a_walk_to_a_place_may_be_given_up_for_prey() {
        assert!(interruptible(&WalkTo::Place {
            x: 0.0,
            z: 0.0,
            floor: 0
        }));

        let id = PlayerId::from(1);
        for aimed in [
            WalkTo::Monster("kobold"),
            WalkTo::Character(&id),
            WalkTo::GroundItem(7),
        ] {
            assert!(
                !interruptible(&aimed),
                "a walk already aimed at something must run to it"
            );
        }
    }
}
