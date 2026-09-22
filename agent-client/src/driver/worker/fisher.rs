//! Fisher: put the rod in hand, get to the fishing spot, find water, cast.
//! Hooking and fighting the catch is the state module's existing reflex, so
//! none of it is repeated here.

use std::sync::Arc;

use onlinerpg_terrain::height::HeightSampler;
use tracing::warn;

use super::{Step, WorkerConfig};
use crate::state::SharedState;

pub(crate) const ROD: &str = "fishing_rod";
/// The rod complaint is a setup hint, not news every tick.
static NO_ROD: std::sync::Once = std::sync::Once::new();
/// The server's own cast limit; stay a little inside it.
const CAST_RANGE: f32 = onlinerpg_shared::fishing::MAX_CAST_DISTANCE_METERS - 1.0;
/// How far out to look for water when none is within casting range.
const SEARCH_RANGE: f32 = crate::state::EVENT_DELIVERY_RADIUS;
/// Sampling grid spacing, in metres. The wider sweep is coarser on purpose:
/// it runs every tick until water is found, and a shoreline is far bigger
/// than one cell.
const STEP_M: f32 = 3.0;
const SEARCH_STEP_M: f32 = 6.0;
/// Where to stand relative to found water: inside cast range, on dry land.
/// Together with `STAND_ARRIVE` it has to stay under `CAST_RANGE`, or a
/// worker that stopped on the far edge of its arrival circle would find the
/// water out of reach and rescan every tick.
const SHORE_GAP: f32 = 4.0;
/// Close enough to the configured spot to start looking for water from it.
const SPOT_ARRIVE: f32 = 10.0;
/// Close enough to the remembered shore to count as standing on it — a walk
/// stops `SCHEDULE_ARRIVAL_RADIUS` (2 m) short, so a tighter check would keep
/// reissuing a leg that has already ended.
const STAND_ARRIVE: f32 = 2.5;

/// Where the last scan found water and where it chose to stand: a cast is
/// repeated from here every tick rather than re-sampling the terrain grid,
/// and a worker sent away (to town, into a fight) walks back to it.
#[derive(Debug, Default)]
pub(crate) struct Fishing {
    pub(crate) cast: Option<Cast>,
    /// The dry-spot complaint, once per run: a spot with no water is a
    /// setting to change, not news every tick.
    pub(crate) warned_dry: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cast {
    pub(crate) stand: (f32, f32),
    pub(crate) water: (f32, f32),
}

/// What the fisher needs off the state before it starts sampling terrain —
/// tile reads must not run under the state lock.
pub(crate) struct WaterJob {
    px: f32,
    pz: f32,
    /// The configured fishing spot, when there is one.
    spot: Option<(f32, f32)>,
    fishing: bool,
    rod_in_hand: bool,
    rod_in_bag: bool,
    height: Arc<HeightSampler>,
    splat: Arc<crate::splat::SplatSampler>,
}

/// A rod in hand or in the bag. Our own inventory snapshot, not the broadcast
/// `main_hand` — that one is only ever read for other players' models.
pub(crate) fn has_rod(s: &SharedState) -> bool {
    rod_in_hand(s) || s.self_bag.iter().any(|i| i.item_def_id == ROD)
}

fn rod_in_hand(s: &SharedState) -> bool {
    s.self_equipped
        .get(&onlinerpg_shared::inventory::EquipSlot::MainHand)
        .is_some_and(|i| i.item_def_id == ROD)
}

pub(crate) fn spot(cfg: &WorkerConfig) -> Option<(f32, f32)> {
    match (cfg.fishing_x, cfg.fishing_z) {
        (Some(x), Some(z)) => Some((x, z)),
        _ => None,
    }
}

pub(crate) fn water_job(s: &SharedState, cfg: &WorkerConfig) -> Option<WaterJob> {
    let me = s.self_player.as_ref()?;
    Some(WaterJob {
        px: me.position.x,
        pz: me.position.z,
        spot: spot(cfg),
        fishing: s.self_fishing,
        rod_in_hand: rod_in_hand(s),
        rod_in_bag: s.self_bag.iter().any(|i| i.item_def_id == ROD),
        height: Arc::clone(&s.height_sampler),
        splat: Arc::clone(&s.splat_sampler),
    })
}

/// Sea reads from the heightmap (below sea level), rivers from the splat's
/// river-bed palette entry — the same test the terrain summary uses.
pub(crate) fn is_water(surface: Option<u8>, height: Option<f32>) -> bool {
    height.is_some_and(|h| h < 0.0) || surface == Some(crate::splat::PAL_RIVER_BED)
}

/// The nearest water cell in a sampled grid, as (x, z, distance).
pub(crate) fn nearest_water(
    px: f32,
    pz: f32,
    samples: &[(f32, f32, bool)],
) -> Option<(f32, f32, f32)> {
    samples
        .iter()
        .filter(|(_, _, wet)| *wet)
        .map(|(x, z, _)| (*x, *z, ((x - px).powi(2) + (z - pz).powi(2)).sqrt()))
        .min_by(|a, b| a.2.total_cmp(&b.2))
}

/// Where to stand to reach water `dist` away: short of it, on the shore.
pub(crate) fn shore_spot(px: f32, pz: f32, wx: f32, wz: f32, dist: f32) -> (f32, f32) {
    if dist <= SHORE_GAP {
        return (px, pz);
    }
    let ratio = (dist - SHORE_GAP) / dist;
    (px + (wx - px) * ratio, pz + (wz - pz) * ratio)
}

fn dist(a: (f32, f32), b: (f32, f32)) -> f32 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

/// The leg to the configured spot, or `None` once we stand near enough to
/// look for water from it.
pub(crate) fn spot_leg(spot: Option<(f32, f32)>, px: f32, pz: f32) -> Option<Step> {
    let (x, z) = spot?;
    (dist((px, pz), (x, z)) > SPOT_ARRIVE).then_some(Step::Walk { x, z })
}

/// A cast planned from a scan around `centre`: the nearest water, and the
/// shore to stand on to reach it.
pub(crate) fn plan_cast(centre: (f32, f32), samples: &[(f32, f32, bool)]) -> Option<Cast> {
    let (wx, wz, d) = nearest_water(centre.0, centre.1, samples)?;
    Some(Cast {
        stand: shore_spot(centre.0, centre.1, wx, wz, d),
        water: (wx, wz),
    })
}

/// The next move from a remembered cast: cast when the water is in range,
/// walk back to the shore when it is not, and `None` — forgetting the cast —
/// when the shore has been reached and the water is still out of reach, which
/// means the scan is worth redoing.
pub(crate) fn resume(mem: &mut Fishing, px: f32, pz: f32) -> Option<Step> {
    let cast = mem.cast?;
    if dist((px, pz), cast.water) <= CAST_RANGE {
        let (x, z) = cast.water;
        return Some(Step::Fish { x, z });
    }
    if dist((px, pz), cast.stand) > STAND_ARRIVE {
        let (x, z) = cast.stand;
        return Some(Step::Walk { x, z });
    }
    mem.cast = None;
    None
}

async fn sample_grid(
    job: &WaterJob,
    centre: (f32, f32),
    range: f32,
    step: f32,
) -> Vec<(f32, f32, bool)> {
    let cells = (range / step) as i32;
    let mut out = Vec::new();
    for r in -cells..=cells {
        for c in -cells..=cells {
            let x = centre.0 + c as f32 * step;
            let z = centre.1 + r as f32 * step;
            if (x - centre.0).powi(2) + (z - centre.1).powi(2) > range * range {
                continue;
            }
            let height = job.height.sample_height(x, z).await.ok();
            let surface = job.splat.dominant_at(x, z).await.ok();
            out.push((x, z, is_water(surface, height)));
        }
    }
    out
}

/// Rod in hand, then at the spot, then cast at the nearest water — or walk
/// to its shore first.
pub(crate) async fn step(job: Option<WaterJob>, mem: &mut Fishing) -> Vec<Step> {
    let Some(job) = job else {
        return vec![Step::Idle];
    };
    if !job.rod_in_hand {
        if job.rod_in_bag {
            return vec![Step::Use(ROD.to_string())];
        }
        // The town trip buys one; reaching here means that trip is paused or
        // came home empty-handed, so say why it is standing still rather than
        // look broken. Once: the tick is fast.
        NO_ROD.call_once(|| {
            warn!("Fisher has no fishing rod and could not buy one — put one in its bag")
        });
        return vec![Step::Idle];
    }
    // The bite, the hook and the fight are all reflexes already.
    if job.fishing {
        return vec![Step::Idle];
    }
    if let Some(step) = resume(mem, job.px, job.pz) {
        return vec![step];
    }
    if let Some(leg) = spot_leg(job.spot, job.px, job.pz) {
        return vec![leg];
    }

    let centre = job.spot.unwrap_or((job.px, job.pz));
    let near = sample_grid(&job, centre, CAST_RANGE, STEP_M).await;
    let cast = match plan_cast(centre, &near) {
        Some(cast) => Some(cast),
        None => {
            let far = sample_grid(&job, centre, SEARCH_RANGE, SEARCH_STEP_M).await;
            plan_cast(centre, &far)
        }
    };
    let Some(cast) = cast else {
        if !mem.warned_dry {
            mem.warned_dry = true;
            let (x, z) = centre;
            warn!(
                "Fisher found no water within {SEARCH_RANGE} m of {} ({x:.0}, {z:.0}) — {}",
                if job.spot.is_some() {
                    "its fishing spot"
                } else {
                    "where it stands"
                },
                if job.spot.is_some() {
                    "pick another spot"
                } else {
                    "move it nearer the shore"
                },
            );
        }
        return vec![Step::Idle];
    };
    mem.cast = Some(cast);
    resume(mem, job.px, job.pz).map_or_else(|| vec![Step::Idle], |s| vec![s])
}
