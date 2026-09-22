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
use tracing::{error, info, warn};

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

/// A walk that has not carried us this far in [`STALL_AFTER`] has stalled.
const STALL_DIST: f32 = 1.0;

const STALL_AFTER: Duration = Duration::from_secs(10);

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
    /// The server kept the walk alive but we did not move.
    Stalled,
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
            Self::Stalled => "you stopped moving and could not get going again".to_string(),
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
        if !state.lock().await.learn_wet_cells(wet) {
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
    let wet: Vec<(i32, i32)> = s.wet_cells().copied().collect();
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
    state.lock().await.walk_trace.clear();
    let result = walk_inner(state, to, background, sprint, &mut owned_request).await;
    let mut s = state.lock().await;
    if let Walked::Lost(
        reason @ (LostReason::NoPath
        | LostReason::LockedDoor
        | LostReason::Timeout
        | LostReason::Desynced
        | LostReason::Stalled
        | LostReason::TooFar(_)),
    ) = result
    {
        let target = to.position(&s).map(|p| (p.x, p.z));
        let floor = to.floor(&s);
        let report = format!(
            "Walk to {to} lost ({reason:?}): {}\ntrace:\n{}",
            describe_blockage(&s, target, floor),
            s.walk_trace.join("\n")
        );
        warn!("{report}");
        s.walk_diagnostic = Some(report);
    }
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
    let mut last_aim: Option<(f32, f32)> = None;
    let mut sent_goal: Option<(f32, f32)> = None;
    let mut request_id = None;
    let mut sent_at = Instant::now();
    let mut doors_opened = 0;
    let relocations = state.lock().await.relocations;
    let mut progress: Option<(Position, Instant)> = None;
    let mut recentred_in: Option<(i32, i32)> = None;
    let mut replans = 0;
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
        let Some(me) = s.self_player.clone().filter(|p| p.health > 0) else {
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
        match progress {
            Some((at, since)) if PlanarDelta::between(&at, &me.position).dist <= STALL_DIST => {
                if since.elapsed() >= STALL_AFTER {
                    let cell = cell_of(&me.position);
                    if recentred_in != Some(cell) {
                        recentred_in = Some(cell);
                        drop(s);
                        if recentre(state, background, sprint, owned_request).await {
                            progress = None;
                        }
                        request_id = None;
                        last_aim = None;
                        continue;
                    }
                    let status = (request_id == Some(s.move_request_id))
                        .then_some(s.move_status)
                        .flatten();
                    warn!(
                        "Walk to {to} stalled at ({:.1}, {:.1}): last goal {sent_goal:?}, \
                         request {request_id:?}, server status {status:?}",
                        me.position.x, me.position.z
                    );
                    return Walked::Lost(LostReason::Stalled);
                }
            }
            _ => progress = Some((me.position, Instant::now())),
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
        let mut aim = (target.x, target.z);
        if s.passability_floor() != target_floor {
            match stair_aim(&s, (target.x, target.z), target_floor) {
                Ok(point) => aim = point,
                Err(termination) => {
                    trace(
                        &mut s,
                        format!("no local route to floor {target_floor} ({termination:?})"),
                    );
                    drop(s);
                    if doors_opened < tuning.max_doors
                        && open_blocking_door(state, background, sprint, owned_request).await
                    {
                        doors_opened += 1;
                        request_id = None;
                        last_aim = None;
                        continue;
                    }
                    let s = state.lock().await;
                    return Walked::Lost(if locked_door_without_key(&s) {
                        LostReason::LockedDoor
                    } else {
                        LostReason::NoPath
                    });
                }
            }
        }
        // Only the aim moving is a reason to re-plan. The leg handed to the
        // server is derived from where we stand, so it drifts with every step;
        // re-sending on that drift re-planned from each new spot and flipped
        // between near-equal routes, walking us back and forth.
        let changed = last_aim
            .is_none_or(|(x, z)| PlanarDelta::xz(x, z, aim.0, aim.1).dist > REROUTE_THRESHOLD);
        let status = (request_id == Some(s.move_request_id))
            .then_some(s.move_status)
            .flatten();
        if request_id.is_some() && request_id != Some(s.move_request_id) {
            return Walked::Lost(LostReason::Desynced);
        }
        let terminal = status
            .is_some_and(|status| !matches!(status, MoveStatus::Moving | MoveStatus::Searching));
        if terminal && status != Some(MoveStatus::Arrived) && !changed {
            let line = format!(
                "move {request_id:?} to {sent_goal:?} ended {status:?} at ({:.1}, {:.1})",
                me.position.x, me.position.z
            );
            trace(&mut s, line);
            if let Some(leg) = sent_goal.filter(|leg| {
                target_floor == 0
                    && s.passability_floor() == 0
                    && matches!(status, Some(MoveStatus::NodeLimit | MoveStatus::Partial))
                    && PlanarDelta::xz(leg.0, leg.1, aim.0, aim.1).dist > REROUTE_THRESHOLD
                    && PlanarDelta::to_xz(&me.position, leg.0, leg.1).dist > LEG_DEAD_END_RADIUS
            }) {
                if replans < MAX_REPLANS {
                    if replans == 0 {
                        let view = describe_blockage(&s, Some(leg), target_floor);
                        trace(&mut s, format!("where the server stopped: {view}"));
                    }
                    replans += 1;
                    mark_dead_end(&mut s, leg);
                    trace(
                        &mut s,
                        format!(
                            "leg ({:.1}, {:.1}) is a dead end, re-planning",
                            leg.0, leg.1
                        ),
                    );
                    request_id = None;
                    last_aim = None;
                    drop(s);
                    continue;
                }
            }
            let cell = cell_of(&me.position);
            if status == Some(MoveStatus::Blocked) && recentred_in != Some(cell) {
                recentred_in = Some(cell);
                drop(s);
                recentre(state, background, sprint, owned_request).await;
                request_id = None;
                last_aim = None;
                continue;
            }
            drop(s);
            if doors_opened < tuning.max_doors
                && open_blocking_door(state, background, sprint, owned_request).await
            {
                doors_opened += 1;
                request_id = None;
                last_aim = None;
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
            let from = me.position;
            let mut goal = aim;
            let to_goal = PlanarDelta::to_xz(&me.position, goal.0, goal.1);
            let mut planned = String::new();
            if to_goal.dist > MAX_LEG {
                let route = if target_floor == 0 && s.passability_floor() == 0 {
                    s.find_long_path_to(aim.0, aim.1, target_floor)
                } else {
                    s.find_path_to(aim.0, aim.1, target_floor)
                };
                planned = format!(
                    ", local {:?} via {}",
                    route.termination,
                    route
                        .waypoints
                        .iter()
                        .take(6)
                        .map(|w| format!("({:.1}, {:.1})", w.x, w.z))
                        .collect::<Vec<_>>()
                        .join(" ")
                );
                goal = leg_along(&route.waypoints, (from.x, from.z), MAX_LEG).unwrap_or((
                    from.x + to_goal.dx * MAX_LEG / to_goal.dist,
                    from.z + to_goal.dz * MAX_LEG / to_goal.dist,
                ));
            }
            if target_floor == 0 && s.passability_floor() == 0 {
                drop(s);
                goal = dry_goal(state, goal).await;
                s = state.lock().await;
            }
            match s.request_move(goal.0, goal.1, background, sprint).await {
                Ok(id) => {
                    let line = format!(
                        "move {id} to ({:.1}, {:.1}) from ({:.2}, {:.2}), previous {status:?}\
                         {planned}",
                        goal.0, goal.1, from.x, from.z
                    );
                    trace(&mut s, line);
                    request_id = Some(id);
                    *owned_request = Some(id);
                }
                Err(_) => return Walked::Error,
            }
            last_aim = Some(aim);
            sent_goal = Some(goal);
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

/// Where on this floor to aim for a target on another: the first point of our
/// route that leaves this floor.
///
/// Only from a route that reached the target. One that did not ends wherever
/// the search gave up, and its first stair can lead the wrong way — up and
/// out of the dungeon when the way down is behind a closed door.
fn stair_aim(
    s: &SharedState,
    target: (f32, f32),
    target_floor: u8,
) -> Result<(f32, f32), pathfinding::PathTermination> {
    let route = s.find_path_to(target.0, target.1, target_floor);
    if route.termination != pathfinding::PathTermination::Reached {
        return Err(route.termination);
    }
    let here = s.passability_floor();
    Ok(route
        .waypoints
        .iter()
        .find(|p| p.floor != here)
        .map_or(target, |p| (p.x, p.z)))
}

/// The longest leg handed to the server in one request.
const MAX_LEG: f32 = 48.0;

/// How far round a leg goal the server could not reach is kept out of the
/// next plan.
const LEG_DEAD_END_RADIUS: f32 = 3.0;

/// Dead-end legs re-planned round before a walk gives up.
const MAX_REPLANS: usize = 6;

fn mark_dead_end(s: &mut SharedState, leg: (f32, f32)) {
    let r = LEG_DEAD_END_RADIUS as i32;
    let (cx, cz) = (leg.0.floor() as i32, leg.1.floor() as i32);
    let cells = (-r..=r)
        .flat_map(|dz| (-r..=r).map(move |dx| (dx, dz)))
        .filter(|(dx, dz)| dx * dx + dz * dz <= r * r)
        .map(|(dx, dz)| (cx + dx, cz + dz));
    s.mark_unreachable(cells, Instant::now());
}

/// The point `max` metres along a planned route. A straight-line point that
/// far out can sit inside a fenced yard or a house, and the server then walks
/// us to the nearest spot to it and stops; a point on our own route is one we
/// know is reachable.
fn leg_along(route: &[PathWaypoint], from: (f32, f32), max: f32) -> Option<(f32, f32)> {
    let mut left = max;
    let mut at = from;
    for wp in route {
        let d = PlanarDelta::xz(at.0, at.1, wp.x, wp.z);
        if d.dist >= left {
            let t = left / d.dist;
            return Some((at.0 + d.dx * t, at.1 + d.dz * t));
        }
        left -= d.dist;
        at = (wp.x, wp.z);
    }
    route.last().map(|wp| (wp.x, wp.z))
}

fn trace(s: &mut SharedState, line: String) {
    info!("{line}");
    s.push_walk_trace(line);
}

fn cell_of(p: &Position) -> (i32, i32) {
    (p.x.floor() as i32, p.z.floor() as i32)
}

/// Step to the centre of the cell we stand in.
///
/// The server plans from wherever we stand, but refuses a first leg whose
/// body sweep grazes an obstacle. Off-centre beside a one-cell gap that is
/// every replan, and we never move again. From the centre the same route
/// clears.
async fn recentre(
    state: &Arc<Mutex<SharedState>>,
    background: bool,
    sprint: Option<bool>,
    owned_request: &mut Option<u32>,
) -> bool {
    let (id, from) = {
        let mut s = state.lock().await;
        let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
            return false;
        };
        let (cx, cz) = cell_of(&me);
        let centre = (cx as f32 + 0.5, cz as f32 + 0.5);
        if PlanarDelta::to_xz(&me, centre.0, centre.1).dist < 0.05 {
            return false;
        }
        let line = format!(
            "recentre from ({:.2}, {:.2}) to ({:.1}, {:.1})",
            me.x, me.z, centre.0, centre.1
        );
        trace(&mut s, line);
        let Ok(id) = s.request_move(centre.0, centre.1, background, sprint).await else {
            return false;
        };
        *owned_request = Some(id);
        (id, me)
    };
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let s = state.lock().await;
        if s.move_request_id != id {
            break;
        }
        if !matches!(
            s.move_status,
            Some(MoveStatus::Moving | MoveStatus::Searching) | None
        ) {
            break;
        }
    }
    let s = state.lock().await;
    s.self_player
        .as_ref()
        .is_some_and(|p| PlanarDelta::between(&from, &p.position).dist > 0.05)
}

async fn open_blocking_door(
    state: &Arc<Mutex<SharedState>>,
    background: bool,
    sprint: Option<bool>,
    owned_request: &mut Option<u32>,
) -> bool {
    let mut candidates = {
        let s = state.lock().await;
        let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
            return false;
        };
        let cap = door_search_dist(s.self_floor_level < 0);
        let floor = s.passability_floor();
        // The side we can walk to, not the one nearest as the crow flies: a
        // corridor that winds round reaches a door from its far side.
        let mut doors: Vec<_> = closed_doors_on_our_floor(&s)
            .into_iter()
            .filter(|door| {
                door.sides
                    .iter()
                    .any(|side| PlanarDelta::to_xz(&me, side.0, side.1).dist <= cap)
            })
            .filter_map(|door| {
                let (length, side) = door
                    .sides
                    .into_iter()
                    .filter_map(|side| {
                        let route = s.find_path_to(side.0, side.1, floor);
                        (route.termination == pathfinding::PathTermination::Reached)
                            .then(|| (route_length((me.x, me.z), &route.waypoints), side))
                    })
                    .min_by(|a, b| a.0.total_cmp(&b.0))?;
                Some((length, door, side))
            })
            .collect();
        doors.sort_by(|a, b| a.0.total_cmp(&b.0));
        doors
    };
    for (_, door, side) in candidates.drain(..).take(MAX_DOOR_PROBES) {
        state
            .lock()
            .await
            .push_walk_trace(format!("door probe at ({:.1}, {:.1})", side.0, side.1));
        if !walk_to_door_side(state, side, background, sprint, owned_request).await {
            continue;
        }
        let mut s = state.lock().await;
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
    false
}

fn route_length(from: (f32, f32), route: &[PathWaypoint]) -> f32 {
    let mut at = from;
    let mut length = 0.0;
    for wp in route {
        length += PlanarDelta::xz(at.0, at.1, wp.x, wp.z).dist;
        at = (wp.x, wp.z);
    }
    length
}

/// Walk to a door side in legs the server accepts: it refuses any goal
/// further than `MAX_MOVE_TARGET_DISTANCE`, and a door across a dungeon floor
/// is often further than that.
async fn walk_to_door_side(
    state: &Arc<Mutex<SharedState>>,
    side: (f32, f32),
    background: bool,
    sprint: Option<bool>,
    owned_request: &mut Option<u32>,
) -> bool {
    for _ in 0..10 {
        let (id, final_leg) = {
            let mut s = state.lock().await;
            let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
                return false;
            };
            let to_side = PlanarDelta::to_xz(&me, side.0, side.1);
            let leg = if to_side.dist > MAX_LEG {
                let floor = s.passability_floor();
                let route = s.find_path_to(side.0, side.1, floor);
                leg_along(&route.waypoints, (me.x, me.z), MAX_LEG).unwrap_or(side)
            } else {
                side
            };
            let Ok(id) = s.request_move(leg.0, leg.1, background, sprint).await else {
                return false;
            };
            *owned_request = Some(id);
            (id, leg == side)
        };
        let mut arrived = false;
        for _ in 0..300 {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let s = state.lock().await;
            if s.move_request_id != id {
                return false;
            }
            match s.move_status {
                Some(MoveStatus::Arrived) => {
                    arrived = true;
                    break;
                }
                Some(MoveStatus::Moving | MoveStatus::Searching) | None => {}
                status => {
                    drop(s);
                    state
                        .lock()
                        .await
                        .push_walk_trace(format!("door walk ended {status:?}"));
                    return false;
                }
            }
        }
        if !arrived {
            return false;
        }
        if final_leg {
            return true;
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

/// Cells drawn on each side of us in [`describe_blockage`].
const DIAG_RADIUS: i32 = 4;

/// What our own passability says about a failed walk: the local A* verdict,
/// which obstacles close our cell, and a map of the cells around us.
///
/// Map legend: `@` us, `G` goal, `.` open, `1`-`3` edges blocked, `#` sealed.
/// North (+z) is up.
pub(super) fn describe_blockage(
    s: &SharedState,
    goal: Option<(f32, f32)>,
    goal_floor: u8,
) -> String {
    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return "no self position".to_string();
    };
    let floor = s.passability_floor();
    let world = s.world_cache.read().unwrap();
    let cache = world.passability_cache();
    const SIDES: [(&str, i32, i32); 4] = [("N", 0, 1), ("S", 0, -1), ("E", 1, 0), ("W", -1, 0)];
    let edge = |cx: i32, cz: i32, dx: i32, dz: i32| {
        let (x, z) = (cx as f32 + 0.5, cz as f32 + 0.5);
        (x, z, x + dx as f32, z + dz as f32)
    };
    let closed_sides = |cx: i32, cz: i32| {
        SIDES
            .iter()
            .filter(|&&(_, dx, dz)| {
                let (x0, z0, x1, z1) = edge(cx, cz, dx, dz);
                pathfinding::is_movement_blocked(cache, x0, z0, x1, z1, floor, Some(me.y))
            })
            .count()
    };
    let (mx, mz) = (me.x.floor() as i32, me.z.floor() as i32);
    let goal_cell = goal.map(|(x, z)| (x.floor() as i32, z.floor() as i32));
    let mut map = String::new();
    for dz in (-DIAG_RADIUS..=DIAG_RADIUS).rev() {
        map.push('\n');
        for dx in -DIAG_RADIUS..=DIAG_RADIUS {
            let cell = (mx + dx, mz + dz);
            map.push(if (dx, dz) == (0, 0) {
                '@'
            } else if goal_cell == Some(cell) {
                'G'
            } else {
                match closed_sides(cell.0, cell.1) {
                    0 => '.',
                    4 => '#',
                    n => char::from(b'0' + n as u8),
                }
            });
        }
    }
    let here = SIDES
        .iter()
        .filter_map(|&(side, dx, dz)| {
            let (x0, z0, x1, z1) = edge(mx, mz, dx, dz);
            if !pathfinding::is_movement_blocked(cache, x0, z0, x1, z1, floor, Some(me.y)) {
                return None;
            }
            let by =
                pathfinding::blocking_entry_for_mover(cache, x0, z0, x1, z1, floor, Some(me.y))
                    .map_or("waived".to_string(), |info| info.key.to_string());
            Some(format!("{side}={by}"))
        })
        .collect::<Vec<_>>()
        .join(" ");
    let sealed = pathfinding::is_cell_sealed(cache, me.x, me.z, floor, Some(me.y));
    let circle = pathfinding::is_circle_blocked_on_floor(cache, me.x, me.z, 0.3, floor, Some(me.y));
    drop(world);
    let plan = goal.map(|(x, z)| {
        let route = s.find_path_to(x, z, goal_floor);
        let end = route
            .waypoints
            .last()
            .map_or("-".to_string(), |w| format!("({:.1}, {:.1})", w.x, w.z));
        format!(
            "local A* {:?}, {} waypoints, ends {end}",
            route.termination,
            route.waypoints.len()
        )
    });
    format!(
        "at ({:.2}, {:.2}, y {:.2}) floor {floor}, goal {}, {}; body overlaps obstacle: {circle}; \
         cell sealed: {sealed}; blocked here: [{here}]{map}",
        me.x,
        me.z,
        me.y,
        goal.map_or("-".to_string(), |(x, z)| format!("({x:.1}, {z:.1})")),
        plan.unwrap_or_else(|| "no goal".to_string()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fence line along z = 0 with its one gap plugged by a barrel we stand
    /// on — the shape a worker was found wedged in.
    fn wedged_in_a_plugged_gate() -> SharedState {
        use onlinerpg_shared::fence::{Fence, FenceAxis, FenceEdge};
        let (mut s, _rx) = crate::state::tests::test_state();
        s.self_player = Some(crate::state::tests::test_player(0.5, 0.5));
        let fences: Vec<Fence> = (-6..=6)
            .filter(|&x| x != 0)
            .map(|x| Fence {
                edge: FenceEdge {
                    x,
                    z: 0,
                    axis: FenceAxis::X,
                },
                y: 0.0,
                owner_id: 1,
            })
            .collect();
        let mut world = s.world_cache.write().unwrap();
        world.update_fences(1.into(), &fences, &[]);
        world.sync_furniture(
            0,
            0,
            vec![onlinerpg_shared::furniture::FurniturePlacement {
                id: 0,
                type_id: "barrel".to_string(),
                x: 0.5,
                y: 0.0,
                z: 0.5,
                rotation_deg: 0.0,
                floor_level: 0,
            }],
        );
        drop(world);
        s
    }

    /// Skeleton Crypt floor 2: the shaft down sits behind a closed door. The
    /// search gives up, and the first stair of what it did find is the one
    /// back up to the surface — which is where the worker used to walk.
    #[test]
    fn a_descent_behind_a_closed_door_opens_it_instead_of_climbing_out() {
        let (mut s, _rx) = crate::state::tests::test_state();
        s.world_cache.write().unwrap().register_dungeons();
        let crypt = s
            .world_cache
            .read()
            .unwrap()
            .all_dungeons()
            .iter()
            .find(|d| d.name == "Skeleton Crypt")
            .cloned()
            .unwrap();
        let start = crypt.arrival_position(2).unwrap();
        let goal = crypt.arrival_position(3).unwrap();
        let mut me = crate::state::tests::test_player(start.x, start.z);
        me.position.y = start.y;
        s.self_player = Some(me);
        s.adopt_floor_level(-2);

        assert!(stair_aim(&s, (goal.x, goal.z), crypt.passability_floor(3)).is_err());

        let open: Vec<(u8, u32)> = crypt
            .closed_doors(2, &Default::default())
            .iter()
            .map(|d| (2, d.door_id))
            .collect();
        s.world_cache
            .write()
            .unwrap()
            .set_dungeon_doors(&crypt.id, &open);
        assert_eq!(
            stair_aim(&s, (goal.x, goal.z), crypt.passability_floor(3)),
            Ok((goal.x, goal.z))
        );
    }

    /// Every door side is a cell centre on the right side of the wall, so
    /// the server reads the walk to it as a walk to that cell. Skeleton Crypt
    /// floor 6 has a two-cell door whose midpoint sat on a cell edge, the
    /// server refused the walk to it, and the descent stalled there.
    #[test]
    fn dungeon_door_sides_are_standable_cell_centres() {
        let (s, _rx) = crate::state::tests::test_state();
        s.world_cache.write().unwrap().register_dungeons();
        let world = s.world_cache.read().unwrap();
        for dungeon in world.all_dungeons() {
            for depth in 1..=dungeon.max_depth() {
                let floor = dungeon.passability_floor(depth);
                for door in dungeon.closed_doors(depth, &Default::default()) {
                    for (x, z) in door.sides {
                        assert_eq!((x.fract().abs(), z.fract().abs()), (0.5, 0.5));
                        assert!(
                            world.is_walkable(x, z, floor),
                            "{} floor {depth} door {} side ({x}, {z})",
                            dungeon.name,
                            door.door_id
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_long_leg_follows_the_route_not_the_straight_line() {
        let wp = |x, z| PathWaypoint { x, z, floor: 0 };
        let route = [wp(0.0, 30.0), wp(40.0, 30.0), wp(40.0, -100.0)];

        assert_eq!(leg_along(&route, (0.0, 0.0), 48.0), Some((18.0, 30.0)));
        assert_eq!(leg_along(&route, (0.0, 0.0), 500.0), Some((40.0, -100.0)));
        assert_eq!(leg_along(&[], (0.0, 0.0), 48.0), None);
    }

    #[test]
    fn a_dead_end_leg_is_routed_round_next_time() {
        let (mut s, _rx) = crate::state::tests::test_state();
        s.self_player = Some(crate::state::tests::test_player(0.5, 0.5));
        let crosses = |s: &SharedState| {
            s.find_long_path_to(0.5, 60.5, 0)
                .waypoints
                .iter()
                .any(|w| (w.x - 0.5).abs() < 3.0 && (w.z - 30.5).abs() < 3.0)
        };
        let straight = s.find_long_path_to(0.5, 60.5, 0);
        assert!(straight.found);

        mark_dead_end(&mut s, (0.5, 30.5));
        let round = s.find_long_path_to(0.5, 60.5, 0);

        assert!(round.found, "{round:?}");
        assert!(!crosses(&s), "{round:?}");
        assert!(round.waypoints.len() > straight.waypoints.len());
    }

    #[test]
    fn a_dead_end_is_tried_again_once_it_lapses() {
        let (mut s, _rx) = crate::state::tests::test_state();
        s.self_player = Some(crate::state::tests::test_player(0.5, 0.5));
        let straight = s.find_long_path_to(0.5, 60.5, 0).waypoints.len();
        let long_ago = Instant::now()
            .checked_sub(crate::state::DEAD_END_FOR)
            .unwrap();

        s.mark_unreachable((-3..=3).map(|x| (x, 30)), long_ago);

        assert_eq!(s.find_long_path_to(0.5, 60.5, 0).waypoints.len(), straight);
    }

    #[test]
    fn known_water_starts_over_rather_than_growing_without_bound() {
        let (mut s, _rx) = crate::state::tests::test_state();
        let cells = |from: i32, n: i32| (from..from + n).map(|x| (x, 0)).collect::<Vec<_>>();

        assert!(s.learn_wet_cells(cells(0, 15_000)));
        assert!(!s.learn_wet_cells(cells(0, 10)));
        assert!(s.learn_wet_cells(cells(15_000, 6_000)));

        assert_eq!(s.wet_cells().count(), 6_000);
    }

    #[test]
    fn a_failed_walk_describes_what_closes_our_cell() {
        let s = wedged_in_a_plugged_gate();
        let report = describe_blockage(&s, Some((0.5, -8.0)), 0);
        assert!(report.contains("goal (0.5, -8.0)"), "{report}");
        assert!(report.contains("local A* Reached"), "{report}");
        assert!(report.contains("cell sealed: true"), "{report}");
        assert!(report.contains("N=waived"), "{report}");
        let rows: Vec<&str> = report.lines().skip(1).collect();
        assert_eq!(rows.len(), 9, "{report}");
        assert_eq!(rows[4].chars().nth(4), Some('@'), "{report}");
    }

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
