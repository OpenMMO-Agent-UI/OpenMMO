import type * as THREE from 'three'
import { viewRayRun } from './view-direction'

/**
 * Whether `box` stands between the player and the camera: the view ray from
 * the player runs through it for more than `minDepth`. Shared by the
 * housing/tree/dungeon occluders so the camera model lives in one place.
 */
export function isoCameraOccludesPlayer(
  box: THREE.Box3,
  px: number,
  py: number,
  pz: number,
  minDepth: number
): boolean {
  return (
    viewRayRun(
      box.min.x,
      box.min.y,
      box.min.z,
      box.max.x,
      box.max.y,
      box.max.z,
      px,
      py,
      pz
    ) > minDepth
  )
}
