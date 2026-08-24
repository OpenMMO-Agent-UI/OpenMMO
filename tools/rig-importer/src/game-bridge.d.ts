/**
 * Types for the one module this tool borrows from the game.
 *
 * Declared here rather than letting TypeScript follow the import into
 * client/src: that tree compiles under the client's own tsconfig, and dragging
 * it into this program pulls in a second copy of three.js's types along with
 * every unrelated error in the client.
 *
 * The runtime import is the real thing — Vite resolves `$game` at
 * client/src/lib, so a rename or a move breaks dev and build immediately. What
 * this file cannot catch is a change to the signature, so
 * test/game-bridge.test.ts loads the actual module and checks it.
 */
declare module '$game/utils/characterAnimationUtils' {
  import type * as THREE from 'three'

  export interface GroundClipsOptions {
    restClip?: string
    restOffset?: number
  }

  export function loadSharedPackClipsForModel(
    modelPath: string,
    targetScene: THREE.Object3D,
    clipNames: string[],
    grounding?: GroundClipsOptions
  ): Promise<THREE.AnimationClip[]>
}
