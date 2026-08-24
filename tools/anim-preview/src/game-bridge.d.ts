/**
 * Types for the modules this tool borrows from the game.
 *
 * Declared here rather than letting TypeScript follow the import into
 * client/src: that tree compiles under the client's own tsconfig, and dragging
 * it into this program pulls in a second copy of three.js's types along with
 * every unrelated error in the client.
 *
 * The runtime import is the real thing — Vite resolves `$game` at
 * client/src/lib, so a rename or a move breaks dev and build immediately. What
 * this file cannot catch is a change to a signature, so
 * test/game-bridge.test.ts loads the actual modules and checks them.
 */
declare module '$game/utils/characterAnimationUtils' {
  import type * as THREE from 'three'

  export interface GroundClipsOptions {
    restClip?: string
    restOffset?: number
  }

  export function retargetAnimationsForCharacterModel(
    targetScene: THREE.Object3D,
    sourceScene: THREE.Object3D,
    clips: THREE.AnimationClip[]
  ): Promise<THREE.AnimationClip[]>

  export function groundRetargetedClips(
    targetScene: THREE.Object3D,
    clips: THREE.AnimationClip[],
    options?: GroundClipsOptions
  ): Promise<THREE.AnimationClip[]>

  export function loadSharedPackClipsForModel(
    modelPath: string,
    targetScene: THREE.Object3D,
    clipNames: string[],
    grounding?: GroundClipsOptions,
    packPaths?: string[]
  ): Promise<THREE.AnimationClip[]>

  export const ANIMATION_SOURCE_BY_NAME: Record<string, 'base' | 'locomotion' | 'combat_melee'>
}

declare module '$game/utils/modelPaths' {
  /** The animation packs the game loads, keyed by role. */
  export const CHARACTER_ANIMATION_PACK_PATHS: Record<string, string>
}
