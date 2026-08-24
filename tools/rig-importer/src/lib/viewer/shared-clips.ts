/**
 * The narrow door.
 *
 * Retargeting the shared packs onto a monster rig — and lifting each clip so the
 * body does not play buried in the floor — is fiddly enough that a second
 * implementation would drift from the game's within a release. So the preview
 * calls the game's own function, aliased in as `$game`. If the client moves or
 * renames it, this fails to build, which is the point: a silent divergence here
 * would mean the tool showing something the game never plays.
 *
 * The animation packs are served from the client's public dir by a dev-server
 * middleware (see vite.config.ts), so the same GLBs are loaded either way.
 */
import type * as THREE from 'three'
import { loadSharedPackClipsForModel } from '$game/utils/characterAnimationUtils'

export interface SharedClipRequest {
  /** monsters.csv `model`, e.g. "monsters/ogre.glb" — used as the cache key. */
  modelPath: string
  scene: THREE.Object3D
  clipNames: string[]
  dieClip?: string
  corpseGroundOffset?: number
}

export async function retargetSharedClips(request: SharedClipRequest): Promise<THREE.AnimationClip[]> {
  return loadSharedPackClipsForModel(request.modelPath, request.scene, request.clipNames, {
    restClip: request.dieClip,
    restOffset: request.corpseGroundOffset ?? 0,
  })
}
