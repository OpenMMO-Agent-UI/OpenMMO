import type * as THREE from 'three'
import { viewRayRun } from './view-direction'

/**
 * Whether `box` stands between the player and the camera: the view ray from
 * the player runs through it for more than `minDepth`. `margin` widens the
 * box in XZ. Shared by the housing/tree/dungeon occluders so the camera
 * model lives in one place.
 */
export function isoCameraOccludesPlayer(
  box: THREE.Box3,
  px: number,
  py: number,
  pz: number,
  minDepth: number,
  margin = 0
): boolean {
  return (
    viewRayRun(
      box.min.x - margin,
      box.min.y,
      box.min.z - margin,
      box.max.x + margin,
      box.max.y,
      box.max.z + margin,
      px,
      py,
      pz
    ) > minDepth
  )
}

/**
 * `isoCameraOccludesPlayer` with about `margin` metres of screen slack, so walls
 * that clip the figure's edge count too. Boxes north or east of the player sit behind them and never count.
 */
export function isoCameraOccludesNearPlayer(
  box: THREE.Box3,
  px: number,
  py: number,
  pz: number,
  minDepth: number,
  margin: number
): boolean {
  if (box.max.z <= pz || box.min.x >= px) return false
  return isoCameraOccludesPlayer(
    box,
    px,
    py,
    pz,
    minDepth,
    margin * Math.SQRT1_2
  )
}
