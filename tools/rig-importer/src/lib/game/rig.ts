/**
 * Numbers the model itself decides.
 *
 * The speed constants come from `doc/assets/monsters.md`: the shared pack's rig
 * has its hips 1.165 m up and its walk cycle covers 1.72 m in 0.958 s (1.8 m/s).
 * Retargeting moves rotations only, so stride — and therefore the speed that
 * does not skate — scales with how high the target's hips sit.
 */
export const PACK_HIPS_HEIGHT = 1.165
export const PACK_WALK_SPEED = 1.8
export const PACK_RUN_SPEED = 5.05

/** 80% of the hand's reach along the bone, per the same doc. */
export const WEAPON_OFFSET_RATIO = 0.8

export function walkSpeedFor(hipsHeight: number): number {
  return round2((PACK_WALK_SPEED * hipsHeight) / PACK_HIPS_HEIGHT)
}

export function runSpeedFor(hipsHeight: number): number {
  return round2((PACK_RUN_SPEED * hipsHeight) / PACK_HIPS_HEIGHT)
}

export function weaponOffsetFor(handReach: number): number {
  return round2(handReach * WEAPON_OFFSET_RATIO)
}

function round2(value: number): number {
  return Math.round(value * 100) / 100
}

export function formatNumber(value: number, digits = 2): string {
  if (!Number.isFinite(value)) return ''
  const fixed = value.toFixed(digits)
  return fixed.replace(/\.?0+$/, '') || '0'
}
