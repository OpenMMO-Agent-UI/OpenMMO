import * as THREE from 'three'
import * as SkeletonUtils from 'three/examples/jsm/utils/SkeletonUtils.js'
import { AnimationName } from '../types/animations'
import { loadGLB } from './gltfCache'
import { CHARACTER_ANIMATION_PACK_PATHS } from './modelPaths'

type AnimationSource = 'base' | 'locomotion' | 'combat_melee'

export interface OrderedAnimationSelection {
  name: AnimationName
  clip: THREE.AnimationClip
  source: AnimationSource
  fromFallback: boolean
}

export interface RetargetSourceScenes {
  base?: THREE.Object3D | null
  locomotion?: THREE.Object3D | null
  combatMelee?: THREE.Object3D | null
}

export const ANIMATION_SOURCE_BY_NAME: Record<AnimationName, AnimationSource> = {
  [AnimationName.IDLE1]: 'locomotion',
  [AnimationName.IDLE2]: 'locomotion',
  [AnimationName.IDLE3]: 'locomotion',
  [AnimationName.IDLE4]: 'locomotion',
  [AnimationName.IDLE5]: 'locomotion',
  [AnimationName.WALK]: 'locomotion',
  [AnimationName.JOG]: 'locomotion',
  [AnimationName.RUN]: 'locomotion',
  [AnimationName.JUMP]: 'locomotion',
  [AnimationName.SLASH1]: 'combat_melee',
  [AnimationName.SLASH2]: 'combat_melee',
  [AnimationName.SLASH3]: 'combat_melee',
  [AnimationName.SLASH4]: 'combat_melee',
  [AnimationName.ATTACK1]: 'combat_melee',
  [AnimationName.ATTACK2]: 'combat_melee',
  [AnimationName.ATTACK3]: 'combat_melee',
  [AnimationName.ATTACK4]: 'combat_melee',
  [AnimationName.DYING]: 'combat_melee',
  [AnimationName.HIT]: 'combat_melee',
  [AnimationName.COMBAT_IDLE]: 'combat_melee',
}

/** @types/three does not know about SkeletonUtils' `localOffsets` yet. */
type RetargetClipOptions = Parameters<typeof SkeletonUtils.retargetClip>[3] & {
  localOffsets?: Record<string, THREE.Matrix4>
}

const RETARGET_TRACK_NAME_PATTERN = /^\.bones\[(.+?)\]\.(position|quaternion)$/
const HIP_BONE_CANDIDATES = [
  'Hips',
  'hips',
  'Hip',
  'hip',
  'Pelvis',
  'pelvis',
  'mixamorigHips',
] as const
const retargetedClipCache = new Map<string, THREE.AnimationClip>()
/** Top-level cache: source scene UUIDs + clip names → full result array.
 *  Avoids expensive SkeletonUtils.clone() when the same retarget was already done
 *  (e.g. character select → game scene for the same class). */
const retargetBatchCache = new Map<string, THREE.AnimationClip[]>()
const ENABLE_RUNTIME_BONE_RETARGETING = true

export function getGltfAnimations(gltf: unknown): THREE.AnimationClip[] {
  if (!gltf || typeof gltf !== 'object' || !('animations' in gltf)) return []

  const animations = (gltf as { animations?: unknown }).animations
  return Array.isArray(animations) ? (animations as THREE.AnimationClip[]) : []
}

export function createCharacterModelRoot(sourceScene: THREE.Object3D): {
  clonedScene: THREE.Object3D
  modelRoot: THREE.Group
} {
  const clonedScene = SkeletonUtils.clone(sourceScene) as THREE.Object3D
  const modelRoot = new THREE.Group()
  modelRoot.add(clonedScene)

  modelRoot.traverse((child) => {
    if (child instanceof THREE.Mesh) {
      child.castShadow = true
      child.receiveShadow = true
    }
  })

  return { clonedScene, modelRoot }
}

const FOOT_BONE_PATTERN = /foot|toe/i
/** A vertex counts as "on the sole" once foot/toe bones own a majority of it. */
const FOOT_INFLUENCE_THRESHOLD = 0.5
/** Tiny gap (m) kept between the soles and the floor so the contact shadow
 *  doesn't peter-pan where coplanar with the ground. */
const FOOT_GROUND_CLEARANCE = 0.005

/**
 * Y offset (metres) to add to a freshly-cloned model so its shoe soles rest
 * just above the floor plane (y = 0 at the model-root origin), with a hair of
 * ground clearance. Measured in the skeleton's current pose, so callers MUST
 * call this right after createCharacterModelRoot — i.e. in the deterministic
 * bind/rest pose, where both soles are planted — before any animation is
 * played. This replaces sampling an arbitrary first animation frame, which
 * (with a randomly-picked idle clip) produced a different lift every session
 * and left the character floating on flat ground.
 *
 * Only foot/toe-skinned vertices are considered, so skirts, capes and
 * not-yet-attached weapons can't pull the contact point below the soles.
 * Returns 0 when the model has no skinned foot geometry (left unshifted).
 */
export function computeSoleGroundOffset(modelRoot: THREE.Object3D): number {
  modelRoot.updateMatrixWorld(true)
  let lowest = Infinity
  const v = new THREE.Vector3()

  modelRoot.traverse((child) => {
    if (!(child instanceof THREE.SkinnedMesh) || !child.skeleton) return

    const footBones = new Set<number>()
    child.skeleton.bones.forEach((bone, i) => {
      if (FOOT_BONE_PATTERN.test(bone.name)) footBones.add(i)
    })
    if (footBones.size === 0) return

    const position = child.geometry.getAttribute('position')
    const skinIndex = child.geometry.getAttribute('skinIndex')
    const skinWeight = child.geometry.getAttribute('skinWeight')
    if (!position || !skinIndex || !skinWeight) return

    for (let i = 0; i < position.count; i++) {
      let footInfluence = 0
      for (let k = 0; k < 4; k++) {
        if (footBones.has(skinIndex.getComponent(i, k))) {
          footInfluence += skinWeight.getComponent(i, k)
        }
      }
      if (footInfluence < FOOT_INFLUENCE_THRESHOLD) continue
      v.fromBufferAttribute(position, i)
      child.applyBoneTransform(i, v) // bind-pose skinned position, mesh-local
      child.localToWorld(v) // → model-root space (root is at origin here)
      if (v.y < lowest) lowest = v.y
    }
  })

  return Number.isFinite(lowest) ? -lowest + FOOT_GROUND_CLEARANCE : 0
}

/** Tiny gap (m) kept between a settled corpse and the floor. */
const CORPSE_GROUND_CLEARANCE = 0.01

/**
 * Y offset (metres) to add to `model` so the body of its CURRENT animated pose
 * rests on the floor. Call once the death clip has clamped on its final frame.
 *
 * Unlike computeSoleGroundOffset this scans every skinned vertex, not just
 * soles: a fallen corpse touches down with its back or flank, and the monster
 * rigs have no foot/toe bones to key off. A monster whose lowest vertex is a
 * dangling appendage ends up hovering on its body — nudge it with
 * corpseGroundOffset in monsters.csv. Returns 0 with no skinned geometry.
 *
 * `stride` samples every nth vertex, for callers measuring many poses.
 */
export function computeCorpseGroundOffset(
  model: THREE.Object3D,
  stride = 1
): number {
  model.updateMatrixWorld(true)
  let lowest = Infinity
  const v = new THREE.Vector3()

  model.traverse((child) => {
    if (!(child instanceof THREE.SkinnedMesh) || !child.skeleton) return
    const position = child.geometry.getAttribute('position')
    if (!position) return

    for (let i = 0; i < position.count; i += stride) {
      v.fromBufferAttribute(position, i)
      child.applyBoneTransform(i, v) // skinned position in the current pose
      child.localToWorld(v)
      model.worldToLocal(v) // → model-local, independent of model.position
      if (v.y < lowest) lowest = v.y
    }
  })

  return Number.isFinite(lowest) ? -lowest + CORPSE_GROUND_CLEARANCE : 0
}

function findPrimarySkinnedMesh(
  root: THREE.Object3D
): THREE.SkinnedMesh | null {
  let bestMatch: THREE.SkinnedMesh | null = null

  root.traverse((child) => {
    if (!(child instanceof THREE.SkinnedMesh) || !child.skeleton) return
    if (
      !bestMatch ||
      child.skeleton.bones.length > bestMatch.skeleton.bones.length
    ) {
      bestMatch = child
    }
  })

  return bestMatch
}

/** Named bone on the rig a character model is skinned to. */
export function findBoneByName(
  root: THREE.Object3D,
  name: string
): THREE.Bone | undefined {
  const skinnedMesh = findPrimarySkinnedMesh(root)
  return skinnedMesh?.skeleton.bones.find((bone) => bone.name === name)
}

function quaternionDistance(a: THREE.Quaternion, b: THREE.Quaternion): number {
  const direct = Math.hypot(a.x - b.x, a.y - b.y, a.z - b.z, a.w - b.w)
  const negated = Math.hypot(a.x + b.x, a.y + b.y, a.z + b.z, a.w + b.w)
  return Math.min(direct, negated)
}

function roundForProfile(value: number): number {
  return Math.round(value * 1000) / 1000
}

function buildSkeletonProfileKey(skinnedMesh: THREE.SkinnedMesh): string {
  const sortedBones = [...skinnedMesh.skeleton.bones].sort((a, b) =>
    a.name.localeCompare(b.name)
  )
  return sortedBones
    .map((bone) =>
      [
        bone.name,
        roundForProfile(bone.position.x),
        roundForProfile(bone.position.y),
        roundForProfile(bone.position.z),
        roundForProfile(bone.quaternion.x),
        roundForProfile(bone.quaternion.y),
        roundForProfile(bone.quaternion.z),
        roundForProfile(bone.quaternion.w),
        roundForProfile(bone.scale.x),
        roundForProfile(bone.scale.y),
        roundForProfile(bone.scale.z),
      ].join(':')
    )
    .join('|')
}

function hasEquivalentSkeletonRestPose(
  targetSkinnedMesh: THREE.SkinnedMesh,
  sourceSkinnedMesh: THREE.SkinnedMesh
): boolean {
  const targetBones = targetSkinnedMesh.skeleton.bones.filter(
    (bone) => bone.name.length > 0
  )
  const sourceBoneByName = new Map(
    sourceSkinnedMesh.skeleton.bones
      .filter((bone) => bone.name.length > 0)
      .map((bone) => [bone.name, bone])
  )

  const commonBones = targetBones.filter((bone) =>
    sourceBoneByName.has(bone.name)
  )
  const coverage =
    commonBones.length / Math.max(targetBones.length, sourceBoneByName.size)
  if (coverage < 0.95) return false

  for (const targetBone of commonBones) {
    const sourceBone = sourceBoneByName.get(targetBone.name)
    if (!sourceBone) return false

    if (targetBone.position.distanceTo(sourceBone.position) > 0.001)
      return false
    if (targetBone.scale.distanceTo(sourceBone.scale) > 0.001) return false
    if (
      quaternionDistance(targetBone.quaternion, sourceBone.quaternion) > 0.001
    ) {
      return false
    }
  }

  return true
}

function normalizeRetargetedClipTrackNames(
  retargetedClip: THREE.AnimationClip,
  originalClipName: string
): THREE.AnimationClip {
  let renamedTrackFound = false
  const convertedTracks: THREE.KeyframeTrack[] = []

  for (const track of retargetedClip.tracks) {
    const match = RETARGET_TRACK_NAME_PATTERN.exec(track.name)
    if (!match) {
      convertedTracks.push(track)
      continue
    }

    const [, boneName, property] = match

    const renamedTrack = track.clone()
    renamedTrack.name = `${boneName}.${property}`
    renamedTrackFound = true
    convertedTracks.push(renamedTrack)
  }

  if (!renamedTrackFound) return retargetedClip

  return new THREE.AnimationClip(
    originalClipName,
    retargetedClip.duration,
    convertedTracks
  )
}

function buildBoneNameMap(
  targetSkinnedMesh: THREE.SkinnedMesh,
  sourceSkinnedMesh: THREE.SkinnedMesh
): Record<string, string> {
  const sourceBoneNames = new Set(
    sourceSkinnedMesh.skeleton.bones
      .map((bone) => bone.name)
      .filter((name) => name.length > 0)
  )
  const nameMap: Record<string, string> = {}

  for (const targetBone of targetSkinnedMesh.skeleton.bones) {
    if (!targetBone.name || !sourceBoneNames.has(targetBone.name)) continue
    nameMap[targetBone.name] = targetBone.name
  }

  return nameMap
}

/**
 * Past this, a bone's rest orientation does not differ by a bind pose — it
 * differs by a rigging convention. cyclop, lizardfolk and stone_golem came off a
 * rigger that rolls the leg bones ~180° from Mixamo's, so those bones sit that
 * far from the pack's while still pointing down the limb.
 *
 * The shipped rigs leave a wide gap to put this in: 86° is the worst any of them
 * reaches against any pack (gnoll against combat_melee), and 170° the least a
 * misrolled leg bone reaches against any.
 */
const REST_POSE_CONVENTION_LIMIT = THREE.MathUtils.degToRad(120)

/**
 * Bones a rig is judged on before any of it is corrected. Fingers are left out:
 * every rig curls them differently in bind (up to 140° on ogre) and that is a
 * bind pose, not a convention — judged on all bones, half the shipped rigs would
 * trip the limit.
 */
const SILHOUETTE_BONES = [
  'Hips',
  'Spine',
  'Neck',
  'Head',
  'LeftArm',
  'LeftForeArm',
  'LeftHand',
  'RightArm',
  'RightForeArm',
  'RightHand',
  'LeftUpLeg',
  'LeftLeg',
  'LeftFoot',
  'RightUpLeg',
  'RightLeg',
  'RightFoot',
] as const

/**
 * Per-bone rotation that turns the pack's rest orientation into this rig's, for
 * the bones that are rolled off the pack's convention.
 *
 * SkeletonUtils copies the source bone's *absolute* world rotation onto the
 * target, which is right only while both rigs roll their bones the same way. A
 * bone that does not gets its mesh wrapped around the bone axis instead — 180°
 * on cyclop's legs, which reads as knees bending backwards. An offset makes
 * `retargetClip` drive that bone by the source's motion relative to each rest
 * pose (`R_source(t) · R_source_rest⁻¹ · R_target_rest`) rather than by the
 * source's orientation outright.
 *
 * Only the bones over the limit get one. Everything else — including every bone
 * of a rig with none — keeps taking the pack's orientation, because that is what
 * puts all 28 rigs in the same pose and the whole point of the sheet is
 * comparing them. The rest of a misrolled rig's gap is the ordinary bind-pose
 * difference every rig has (cyclop's arm sits 54° off combat_melee's, troll's
 * 67°), and correcting it on two rigs and no others is what made cyclop and
 * lizardfolk hold a sword unlike anything else on the sheet.
 */
function buildRestPoseOffsets(
  targetSkinnedMesh: THREE.SkinnedMesh,
  sourceSkinnedMesh: THREE.SkinnedMesh,
  boneNameMap: Record<string, string>
): Record<string, THREE.Matrix4> | undefined {
  const sourceBones = new Map(
    sourceSkinnedMesh.skeleton.bones.map((bone) => [bone.name, bone])
  )
  const sourceRest = new THREE.Quaternion()
  const targetRest = new THREE.Quaternion()
  const gaps = new Map<string, THREE.Quaternion>()
  const silhouette = new Set<string>(SILHOUETTE_BONES)
  let misrolled = false

  for (const targetBone of targetSkinnedMesh.skeleton.bones) {
    const sourceBone = sourceBones.get(boneNameMap[targetBone.name])
    if (!sourceBone) continue

    targetBone.getWorldQuaternion(targetRest)
    sourceBone.getWorldQuaternion(sourceRest)
    const gap = sourceRest.clone().invert().multiply(targetRest)
    gaps.set(targetBone.name, gap)

    if (
      silhouette.has(targetBone.name) &&
      angleOf(gap) > REST_POSE_CONVENTION_LIMIT
    ) {
      misrolled = true
    }
  }
  if (!misrolled) return undefined

  const offsets: Record<string, THREE.Matrix4> = {}
  for (const [boneName, gap] of gaps) {
    if (angleOf(gap) <= REST_POSE_CONVENTION_LIMIT) continue
    offsets[boneName] = new THREE.Matrix4().makeRotationFromQuaternion(gap)
  }
  return offsets
}

function angleOf(rotation: THREE.Quaternion): number {
  return 2 * Math.acos(Math.min(1, Math.abs(rotation.w)))
}

function resolveHipBoneName(sourceSkinnedMesh: THREE.SkinnedMesh): string {
  const sourceBoneNames = new Set(
    sourceSkinnedMesh.skeleton.bones.map((bone) => bone.name)
  )
  return (
    HIP_BONE_CANDIDATES.find((boneName) => sourceBoneNames.has(boneName)) ??
    sourceSkinnedMesh.skeleton.bones[0]?.name ??
    'Hips'
  )
}

function getHipBoneRestY(
  skinnedMesh: THREE.SkinnedMesh,
  hipBoneName: string
): number | null {
  const hipBone = skinnedMesh.skeleton.bones.find((b) => b.name === hipBoneName)
  return hipBone ? hipBone.position.y : null
}

/**
 * The hip correction assumes the clip stands on its legs, so it scales with the
 * gap between the two rigs' hip heights. A body that ends up lying down is held
 * off the floor by its own thickness instead, and the correction only floats it
 * — a tall character's corpse hovered 10cm up while the hobgoblin's sank.
 */
const CLIPS_KEEPING_SOURCE_HIP_HEIGHT = new Set<string>([AnimationName.DYING])

function correctHipHeightInClip(
  clip: THREE.AnimationClip,
  hipBoneName: string,
  yDelta: number
): void {
  const track = clip.tracks.find((t) => t.name === `${hipBoneName}.position`)
  if (!track) return

  // Position values are stored as [x, y, z, x, y, z, ...]
  const values = track.values
  for (let i = 1; i < values.length; i += 3) {
    values[i] += yDelta
  }
}

export async function retargetAnimationsForCharacterModel(
  targetScene: THREE.Object3D,
  retargetSourceScene: THREE.Object3D | null | undefined,
  clips: THREE.AnimationClip[]
): Promise<THREE.AnimationClip[]> {
  if (!ENABLE_RUNTIME_BONE_RETARGETING) return clips
  if (clips.length === 0 || !retargetSourceScene) return clips

  // Fast path: check the batch cache before the expensive cloning. Clones share
  // their source's geometry, so the two uuids identify the skeleton pair across
  // every clone of the same GLB — another model gets its own entry.
  const batchKey = [
    findPrimarySkinnedMesh(targetScene)?.geometry.uuid,
    findPrimarySkinnedMesh(retargetSourceScene)?.geometry.uuid,
    clips.map((c) => c.name).join(','),
  ].join('::')
  const cachedBatch = retargetBatchCache.get(batchKey)
  if (cachedBatch) return cachedBatch

  // Operate on clones only. Both target and source scenes can come from shared
  // loader instances, and retarget internals mutate skeleton transforms.
  const targetSceneClone = SkeletonUtils.clone(targetScene) as THREE.Object3D

  // `retargetSourceScene` comes from a shared loader cache. Retargeting mutates
  // skeleton state (`pose`, matrix updates), so work on a clone to avoid
  // leaking transforms back into female_knight.glb previews.
  const retargetSourceClone = SkeletonUtils.clone(
    retargetSourceScene
  ) as THREE.Object3D

  const targetSkinnedMesh = findPrimarySkinnedMesh(targetSceneClone)
  const sourceSkinnedMesh = findPrimarySkinnedMesh(retargetSourceClone)
  if (!targetSkinnedMesh || !sourceSkinnedMesh) return clips

  targetSkinnedMesh.skeleton.pose()
  sourceSkinnedMesh.skeleton.pose()
  targetSkinnedMesh.updateMatrixWorld(true)
  sourceSkinnedMesh.updateMatrixWorld(true)

  if (hasEquivalentSkeletonRestPose(targetSkinnedMesh, sourceSkinnedMesh)) {
    return clips
  }

  const boneNameMap = buildBoneNameMap(targetSkinnedMesh, sourceSkinnedMesh)
  if (Object.keys(boneNameMap).length === 0) return clips

  const restOffsets = buildRestPoseOffsets(
    targetSkinnedMesh,
    sourceSkinnedMesh,
    boneNameMap
  )

  const targetProfileKey = buildSkeletonProfileKey(targetSkinnedMesh)
  const sourceProfileKey = buildSkeletonProfileKey(sourceSkinnedMesh)
  const hipBoneName = resolveHipBoneName(sourceSkinnedMesh)

  const targetHipY = getHipBoneRestY(targetSkinnedMesh, hipBoneName)
  const sourceHipY = getHipBoneRestY(sourceSkinnedMesh, hipBoneName)
  const hipYDelta =
    targetHipY !== null && sourceHipY !== null ? targetHipY - sourceHipY : 0

  const retargetedClips: THREE.AnimationClip[] = []
  for (const clip of clips) {
    const cacheKey = `${targetProfileKey}::${sourceProfileKey}::${clip.name}`
    const cachedClip = retargetedClipCache.get(cacheKey)
    if (cachedClip) {
      retargetedClips.push(cachedClip)
      continue
    }

    try {
      targetSkinnedMesh.skeleton.pose()
      targetSkinnedMesh.updateMatrixWorld(true)

      const retargetedClip = SkeletonUtils.retargetClip(
        targetSkinnedMesh,
        sourceSkinnedMesh,
        clip,
        {
          names: boneNameMap,
          hip: hipBoneName,
          preserveBoneMatrix: true,
          useTargetMatrix: false,
          useFirstFramePosition: false,
          localOffsets: restOffsets,
        } as RetargetClipOptions
      )
      const normalizedClip = normalizeRetargetedClipTrackNames(
        retargetedClip,
        clip.name
      )
      if (normalizedClip.tracks.length === 0) {
        retargetedClips.push(clip)
        continue
      }
      if (
        Math.abs(hipYDelta) > 0.001 &&
        !CLIPS_KEEPING_SOURCE_HIP_HEIGHT.has(clip.name)
      ) {
        correctHipHeightInClip(normalizedClip, hipBoneName, hipYDelta)
      }
      retargetedClipCache.set(cacheKey, normalizedClip)
      retargetedClips.push(normalizedClip)
    } catch (error) {
      console.warn(`Failed to retarget animation clip "${clip.name}"`, error)
      retargetedClips.push(clip)
    }

    // Yield to the browser after each clip so the render loop keeps running
    await new Promise((r) => setTimeout(r, 0))
  }

  retargetBatchCache.set(batchKey, retargetedClips)
  return retargetedClips
}

export async function retargetOrderedCharacterAnimationsForModel(
  targetScene: THREE.Object3D,
  orderedSelections: OrderedAnimationSelection[],
  sourceScenes: RetargetSourceScenes
): Promise<THREE.AnimationClip[]> {
  if (!ENABLE_RUNTIME_BONE_RETARGETING) {
    return orderedSelections.map((selection) => selection.clip)
  }

  const bySource = {
    base: orderedSelections.filter((selection) => selection.source === 'base'),
    locomotion: orderedSelections.filter(
      (selection) => selection.source === 'locomotion'
    ),
    combat_melee: orderedSelections.filter(
      (selection) => selection.source === 'combat_melee'
    ),
  }

  // Each call yields per-clip internally so the browser can render frames
  const retargetedBase = await retargetAnimationsForCharacterModel(
    targetScene,
    sourceScenes.base,
    bySource.base.map((selection) => selection.clip)
  )

  const retargetedLocomotion = await retargetAnimationsForCharacterModel(
    targetScene,
    sourceScenes.locomotion,
    bySource.locomotion.map((selection) => selection.clip)
  )

  const retargetedCombatMelee = await retargetAnimationsForCharacterModel(
    targetScene,
    sourceScenes.combatMelee ?? sourceScenes.locomotion,
    bySource.combat_melee.map((selection) => selection.clip)
  )

  const retargetedBySource = {
    base: retargetedBase,
    locomotion: retargetedLocomotion,
    combat_melee: retargetedCombatMelee,
  }

  let baseIndex = 0
  let locomotionIndex = 0
  let combatMeleeIndex = 0

  return orderedSelections.map((selection) => {
    if (selection.source === 'base') {
      const clip = retargetedBySource.base[baseIndex]
      baseIndex += 1
      return clip ?? selection.clip
    }

    if (selection.source === 'locomotion') {
      const clip = retargetedBySource.locomotion[locomotionIndex]
      locomotionIndex += 1
      return clip ?? selection.clip
    }

    const clip = retargetedBySource.combat_melee[combatMeleeIndex]
    combatMeleeIndex += 1
    return clip ?? selection.clip
  })
}

export function selectOrderedCharacterAnimations(
  baseAnimations: THREE.AnimationClip[],
  locomotionAnimations: THREE.AnimationClip[],
  combatMeleeAnimations: THREE.AnimationClip[]
): OrderedAnimationSelection[] {
  const baseClipByName = new Map(
    baseAnimations.map((clip) => [clip.name, clip])
  )
  const locomotionClipByName = new Map(
    locomotionAnimations.map((clip) => [clip.name, clip])
  )
  const combatMeleeClipByName = new Map(
    combatMeleeAnimations.map((clip) => [clip.name, clip])
  )
  const firstBaseClip = baseAnimations[0]
  const firstLocomotionClip = locomotionAnimations[0]
  const firstCombatMeleeClip = combatMeleeAnimations[0]
  const orderedSelections: OrderedAnimationSelection[] = []

  for (const name of Object.values(AnimationName)) {
    const source = ANIMATION_SOURCE_BY_NAME[name]
    if (!source) {
      console.error(
        `No animation source mapping defined for "${name}"; update animation source map.`
      )
      return []
    }

    const selectedClip =
      source === 'locomotion'
        ? locomotionClipByName.get(name)
        : source === 'combat_melee'
          ? combatMeleeClipByName.get(name)
          : baseClipByName.get(name)

    let clip = selectedClip
    let fromFallback = false

    if (!clip) {
      const fallbackClip =
        source === 'locomotion'
          ? firstLocomotionClip
          : source === 'combat_melee'
            ? firstCombatMeleeClip
            : firstBaseClip

      if (fallbackClip) {
        clip = fallbackClip
        fromFallback = true
        console.warn(
          `Missing animation "${name}" in ${source}.glb; using first ${source} clip "${fallbackClip.name}" as fallback.`
        )
      }
    }

    if (!clip) {
      console.error(
        `Missing animation "${name}" in ${source}.glb and no fallback clip is available.`
      )
      return []
    }

    orderedSelections.push({
      name,
      clip,
      source,
      fromFallback,
    })
  }

  return orderedSelections
}

/** Poses sampled per clip, and the vertex stride each pose is measured at. */
const GROUND_SAMPLE_POSES = 24
const GROUND_SAMPLE_STRIDE = 8

/**
 * Ease the hip track from a flush start to `restOffset` at the last key, then
 * lift any key still below the floor. The ease keeps the standing frames where
 * they are; the clamp stops the body passing through the floor as it falls.
 *
 * The floor eases along with the body — held at 0 it would push every key but
 * the last back up, leaving the whole rest offset to drop in the final key.
 */
function groundClipToRest(
  hipTrack: THREE.KeyframeTrack,
  lowestAt: (time: number) => number,
  restOffset: number
): void {
  const times = hipTrack.times
  const last = times.length - 1
  const startLift = -lowestAt(times[0])
  const endLift = -lowestAt(times[last]) + restOffset
  const smoothstep = (x: number) => x * x * (3 - 2 * x)

  for (let i = 0; i <= last; i++) {
    const weight = smoothstep(times[i] / times[last])
    hipTrack.values[i * 3 + 1] += startLift + weight * (endLift - startLift)
  }
  for (let i = 0; i <= last; i++) {
    const floor = smoothstep(times[i] / times[last]) * restOffset
    hipTrack.values[i * 3 + 1] += Math.max(0, floor - lowestAt(times[i]))
  }
}

export interface GroundClipsOptions {
  /** Clip that ends with the body at rest — grounded key by key so it lands on
   *  the floor instead of being corrected after it clamps. */
  restClip?: string
  /** Where that clip's last pose belongs, relative to its lowest vertex. A body
   *  lying on an outstretched limb has to sink for its torso to touch. */
  restOffset?: number
}

/**
 * Retargeting anchors the hips from the source rig, so a model built to other
 * proportions plays buried in the floor — the death clip ended underground and
 * the corpse popped up once it was settled. Shift each clip's hip track so it
 * plays on the floor, leaving the motion itself alone.
 */
export async function groundRetargetedClips(
  targetScene: THREE.Object3D,
  clips: THREE.AnimationClip[],
  options: GroundClipsOptions = {}
): Promise<THREE.AnimationClip[]> {
  const scene = SkeletonUtils.clone(targetScene) as THREE.Object3D
  const mixer = new THREE.AnimationMixer(scene)
  const grounded: THREE.AnimationClip[] = []

  for (const clip of clips) {
    const shifted = clip.clone()
    const hipTrack = shifted.tracks.find((track) =>
      HIP_BONE_CANDIDATES.some((hip) => track.name === `${hip}.position`)
    )
    if (!hipTrack || shifted.duration <= 0) {
      grounded.push(clip)
      continue
    }

    const action = mixer.clipAction(shifted)
    action.play()
    const lowestAt = (time: number) => {
      mixer.setTime(Math.min(time, shifted.duration - 1e-4))
      return (
        CORPSE_GROUND_CLEARANCE -
        computeCorpseGroundOffset(scene, GROUND_SAMPLE_STRIDE)
      )
    }

    if (clip.name === options.restClip) {
      groundClipToRest(hipTrack, lowestAt, options.restOffset ?? 0)
    } else {
      let lift = 0
      for (let i = 0; i <= GROUND_SAMPLE_POSES; i++) {
        lift = Math.max(
          lift,
          -lowestAt((i * shifted.duration) / GROUND_SAMPLE_POSES)
        )
      }
      for (let i = 1; i < hipTrack.values.length; i += 3) {
        hipTrack.values[i] += lift
      }
    }
    action.stop()
    mixer.uncacheClip(shifted)

    grounded.push(shifted)
    await new Promise((r) => setTimeout(r, 0))
  }

  return grounded
}

const sharedPackClipsByModel = new Map<string, Promise<THREE.AnimationClip[]>>()

/**
 * The named locomotion + melee clips retargeted onto a model rigged on the
 * character bone names but with its own bone offsets (monsters with
 * `sharedAnims`) — played unretargeted, combat_melee's clips stretch it.
 * Cached per model, so a crowd of one monster type retargets once.
 *
 * `packPaths` overrides which pack files supply the clips. The game never
 * passes it; tools/anim-preview does, to audition a candidate pack against
 * every rig without overwriting the one the game ships.
 */
export function loadSharedPackClipsForModel(
  modelPath: string,
  targetScene: THREE.Object3D,
  clipNames: string[],
  grounding: GroundClipsOptions = {},
  packPaths: string[] = [
    CHARACTER_ANIMATION_PACK_PATHS.locomotion,
    CHARACTER_ANIMATION_PACK_PATHS.combatMelee,
  ]
): Promise<THREE.AnimationClip[]> {
  const wanted = new Set(clipNames)
  const cacheKey = `${modelPath}::${[...wanted].sort().join(',')}::${grounding.restClip ?? ''}:${grounding.restOffset ?? 0}::${packPaths.join('|')}`
  const cached = sharedPackClipsByModel.get(cacheKey)
  if (cached) return cached

  const clips = Promise.all(packPaths.map((path) => loadGLB(path)))
    .then((packs) =>
      Promise.all(
        packs.map((pack) =>
          retargetAnimationsForCharacterModel(
            targetScene,
            pack.scene,
            pack.animations.filter((clip) => wanted.has(clip.name))
          )
        )
      )
    )
    .then((packs) =>
      groundRetargetedClips(targetScene, packs.flat(), grounding)
    )
  sharedPackClipsByModel.set(cacheKey, clips)
  return clips
}
