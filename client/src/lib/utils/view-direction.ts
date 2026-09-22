import type { WallDirection } from './house-geo-utils'
import { INITIAL_YAW } from '../components/game-scene/camera-utils'

/**
 * Where the camera looks from, for everything that decides what stands
 * between it and the player. The camera keeps its isometric pitch
 * (atan(1/√2)) and only turns about Y, so the ray from the player toward it
 * rises one metre for every √2 it runs horizontally: R(s) = p + (x, 1, z)·s.
 */
export const viewRay = { x: 0, z: 0 }

/** Bumped whenever the yaw changes, for occluders that cache their verdict. */
export let viewRevision = 0

let currentYaw = Number.NaN

/** Below this a yaw change is float drift from re-deriving the offset. */
const YAW_EPSILON = 1e-4

export function setViewYaw(yaw: number) {
  if (Math.abs(yaw - currentYaw) < YAW_EPSILON) return
  currentYaw = yaw
  viewRay.x = Math.SQRT2 * Math.sin(yaw)
  viewRay.z = Math.SQRT2 * Math.cos(yaw)
  viewRevision++
}

setViewYaw(INITIAL_YAW)

export function viewYaw(): number {
  return currentYaw
}

const OUTWARD: Record<WallDirection, { x: number; z: number }> = {
  north: { x: 0, z: -1 },
  south: { x: 0, z: 1 },
  east: { x: 1, z: 0 },
  west: { x: -1, z: 0 },
}

/** Whether a wall facing `dir` shows its outside to the camera. */
export function facesCamera(dir: WallDirection): boolean {
  const n = OUTWARD[dir]
  return n.x * viewRay.x + n.z * viewRay.z > 1e-6
}

export type ViewQuadrant = 'sw' | 'nw' | 'ne' | 'se'

export const VIEW_QUADRANTS: readonly ViewQuadrant[] = ['sw', 'nw', 'ne', 'se']

/** The pair of wall sides the camera looks at. */
export function viewQuadrant(): ViewQuadrant {
  const west = viewRay.x <= 0
  const south = viewRay.z >= 0
  if (south) return west ? 'sw' : 'se'
  return west ? 'nw' : 'ne'
}

/** The quadrants a wall facing `dir` belongs to. */
export function quadrantsOf(dir: WallDirection): ViewQuadrant[] {
  return VIEW_QUADRANTS.filter((q) => q.includes(dir[0]))
}

/**
 * Length of the view ray's run through an axis-aligned box, in metres of
 * rise; negative when it misses. The run starts no lower than the player.
 */
export function viewRayRun(
  minX: number,
  minY: number,
  minZ: number,
  maxX: number,
  maxY: number,
  maxZ: number,
  px: number,
  py: number,
  pz: number
): number {
  let sMin = Math.max(minY - py, 0)
  let sMax = maxY - py
  if (sMax <= 0) return -1
  ;[sMin, sMax] = clipSlab(sMin, sMax, px, viewRay.x, minX, maxX)
  ;[sMin, sMax] = clipSlab(sMin, sMax, pz, viewRay.z, minZ, maxZ)
  return sMax - sMin
}

function clipSlab(
  sMin: number,
  sMax: number,
  p: number,
  k: number,
  lo: number,
  hi: number
): [number, number] {
  if (Math.abs(k) < 1e-9) return p >= lo && p <= hi ? [sMin, sMax] : [1, 0]
  const a = (lo - p) / k
  const b = (hi - p) / k
  return [Math.max(sMin, Math.min(a, b)), Math.min(sMax, Math.max(a, b))]
}
