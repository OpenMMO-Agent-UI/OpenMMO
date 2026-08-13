//! Fisher: put the rod in hand, find water, cast. Hooking and fighting the
//! catch is the state module's existing reflex, so none of it is repeated
//! here.

use std::sync::Arc;

use onlinerpg_terrain::height::HeightSampler;
use tracing::warn;

use super::Step;
use crate::state::SharedState;

const ROD: &str = "fishing_rod";
/// The rod complaint is a setup hint, not news every half-second.
static NO_ROD: std::sync::Once = std::sync::Once::new();
/// The server's own cast limit; stay a little inside it.
const CAST_RANGE: f32 = onlinerpg_shared::fishing::MAX_CAST_DISTANCE_METERS - 1.0;
/// How far out to look for water when none is within casting range.
const SEARCH_RANGE: f32 = crate::state::NPC_SIGHT_RADIUS;
/// Sampling grid spacing, in metres. The wider sweep is coarser on purpose:
/// it runs every tick until water is found, and a shoreline is far bigger
/// than one cell.
const STEP_M: f32 = 3.0;
const SEARCH_STEP_M: f32 = 6.0;
/// Where to stand relative to found water: inside cast range, on dry land.
const SHORE_GAP: f32 = 5.0;

/// What the fisher needs off the state before it starts sampling terrain —
/// tile reads must not run under the state lock.
pub(crate) struct WaterJob {
    px: f32,
    pz: f32,
    fishing: bool,
    rod_in_hand: bool,
    rod_in_bag: bool,
    height: Arc<HeightSampler>,
    splat: Arc<crate::splat::SplatSampler>,
}

pub(crate) fn water_job(s: &SharedState) -> Option<WaterJob> {
    let me = s.self_player.as_ref()?;
    Some(WaterJob {
        px: me.position.x,
        pz: me.position.z,
        fishing: s.self_fishing,
        // Our own inventory snapshot, not the broadcast `main_hand` — that
        // one is only ever read for other players' models.
        rod_in_hand: s
            .self_equipped
            .get(&onlinerpg_shared::inventory::EquipSlot::MainHand)
            .is_some_and(|i| i.item_def_id == ROD),
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

async fn sample_grid(job: &WaterJob, range: f32, step: f32) -> Vec<(f32, f32, bool)> {
    let cells = (range / step) as i32;
    let mut out = Vec::new();
    for r in -cells..=cells {
        for c in -cells..=cells {
            let x = job.px + c as f32 * step;
            let z = job.pz + r as f32 * step;
            if (x - job.px).powi(2) + (z - job.pz).powi(2) > range * range {
                continue;
            }
            let height = job.height.sample_height(x, z).await.ok();
            let surface = job.splat.primary_at(x, z).await.ok();
            out.push((x, z, is_water(surface, height)));
        }
    }
    out
}

/// Rod in hand, then cast at the nearest water — or walk to its shore first.
pub(crate) async fn step(job: Option<WaterJob>) -> Vec<Step> {
    let Some(job) = job else {
        return vec![Step::Idle];
    };
    if !job.rod_in_hand {
        if job.rod_in_bag {
            return vec![Step::Use(ROD.to_string())];
        }
        // Buying gear is not this worker's business, so say why it is
        // standing still rather than look broken. Once: the tick is fast.
        NO_ROD.call_once(|| warn!("Fisher has no fishing rod — put one in its bag"));
        return vec![Step::Idle];
    }
    // The bite, the hook and the fight are all reflexes already.
    if job.fishing {
        return vec![Step::Idle];
    }

    let near = sample_grid(&job, CAST_RANGE, STEP_M).await;
    if let Some((x, z, _)) = nearest_water(job.px, job.pz, &near) {
        return vec![Step::Fish { x, z }];
    }
    let far = sample_grid(&job, SEARCH_RANGE, SEARCH_STEP_M).await;
    match nearest_water(job.px, job.pz, &far) {
        Some((wx, wz, dist)) => {
            let (x, z) = shore_spot(job.px, job.pz, wx, wz, dist);
            vec![Step::Walk { x, z }]
        }
        None => vec![Step::Idle],
    }
}
