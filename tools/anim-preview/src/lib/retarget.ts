/**
 * The narrow door.
 *
 * Retargeting a clip onto a rig with different proportions — and lifting it so
 * the body does not play buried in the floor — is fiddly enough that a second
 * implementation would drift from the game's within a release. The whole point
 * of this tool is to judge how that retargeting looks, so it has to be the
 * game's own, aliased in as `$game`. If the client moves or renames it, this
 * fails to build, which is what should happen: a silent divergence here would
 * mean approving a take that the game plays differently.
 */
import type * as THREE from 'three'
import {
  groundRetargetedClips,
  loadSharedPackClipsForModel,
  retargetAnimationsForCharacterModel,
} from '$game/utils/characterAnimationUtils'

export interface Grounding {
  /** Clip whose rest pose sets the floor — the death clip, for a corpse. */
  restClip?: string
  restOffset?: number
}

/**
 * Put one downloaded take's clips onto a rig, exactly as the game would if the
 * take were shipped in a pack.
 */
export async function retargetTake(
  targetScene: THREE.Object3D,
  sourceScene: THREE.Object3D,
  clips: THREE.AnimationClip[],
  grounding: Grounding = {}
): Promise<THREE.AnimationClip[]> {
  const retargeted = await retargetAnimationsForCharacterModel(targetScene, sourceScene, uniquelyNamed(clips))
  return groundRetargetedClips(targetScene, retargeted, grounding)
}

/**
 * Give each clip a name nothing else will ever produce.
 *
 * The game's retarget cache keys a result on `<skeleton pair>::<clip name>` —
 * fine for its own two packs, whose clip names are hand-picked and unique. This
 * tool hands it clips out of arbitrary downloads instead, and Mixamo names
 * almost every export identically (`Armature|mixamo.com|Layer0`). Two
 * different takes of the same Mixamo character share both halves of that key
 * — same rest skeleton, same clip name — so the second take's retarget would
 * silently come back as the first take's cached result. Renaming before the
 * call sidesteps the collision; `build()` in +page.svelte renames the result
 * to the motion name afterward regardless, so the synthetic name never leaks
 * out.
 */
function uniquelyNamed(clips: THREE.AnimationClip[]): THREE.AnimationClip[] {
  return clips.map((clip) => {
    const renamed = clip.clone()
    renamed.name = `${clip.name}::${crypto.randomUUID()}`
    return renamed
  })
}

/**
 * Play a whole pack against a rig — the shipped one, or a candidate written by
 * the export step. `packPaths` is the parameter this tool added to the client's
 * function; passing nothing is the game's own behaviour.
 */
export async function retargetPack(
  modelPath: string,
  targetScene: THREE.Object3D,
  clipNames: string[],
  packPaths?: string[],
  grounding: Grounding = {}
): Promise<THREE.AnimationClip[]> {
  return loadSharedPackClipsForModel(modelPath, targetScene, clipNames, grounding, packPaths)
}
