/** The one mounted viewport, so any step can drive it without prop-drilling. */
import type * as THREE from 'three'
import type { Viewport } from './viewport'

export const stage = $state({
  viewport: null as Viewport | null,
  /** Retargeted shared-pack clips, once the animation step has built them. */
  sharedClips: [] as THREE.AnimationClip[],
  playing: '' as string,
})
