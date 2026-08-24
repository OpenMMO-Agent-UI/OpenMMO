/**
 * Whether a rig can be driven by the shared packs at all.
 *
 * The packs are keyed on Mixamo's bone names with the `mixamorig:` prefix
 * stripped (doc/ANIMATION.md). Retargeting moves rotations bone by bone, so a
 * name the rig does not have is a channel that lands nowhere.
 *
 * The test is the silhouette, not the whole skeleton: the torso and the four
 * limbs decide whether a walk reads as a walk. Fingers are excluded on purpose
 * — ogre is rigged with 33 bones and gnoll with 57, and holding those two off
 * the sheet would hide how their walk and swing actually retarget, which is the
 * thing being judged.
 */
import * as THREE from 'three'

/** The silhouette. A rig missing one of these cannot play a shared clip. */
export const CORE_BONES = [
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

export interface RigSkeleton {
  boneNames: string[]
  boneCount: number
  /** Height of the hip bone above the floor — what stride length scales with. */
  hipsHeight: number
}

/** A core bone the rig almost has — same joint, spelled differently. */
export interface NearMiss {
  want: string
  have: string
}

export interface Compatibility {
  /** Core bones the rig does not have, in CORE_BONES order. */
  missing: string[]
  /**
   * Missing bones the rig does have under another spelling.
   *
   * The game matches bones by exact name, so these really do not retarget and
   * the rig really does stay switched off — naming them anyway is the
   * difference between "this rig cannot be driven" and "this rig is one rename
   * away", which is a different piece of work. stone_golem is the case that
   * makes it worth reporting: it has `neck`, and needs `Neck`.
   */
  nearMisses: NearMiss[]
  compatible: boolean
}

export function readSkeleton(scene: THREE.Object3D): RigSkeleton {
  const boneNames: string[] = []
  const bones = new Map<string, THREE.Object3D>()
  scene.traverse((object) => {
    if (!(object as THREE.Bone).isBone) return
    boneNames.push(object.name)
    bones.set(object.name, object)
  })
  scene.updateWorldMatrix(true, true)
  const hips = bones.get('Hips')
  const hipsHeight = hips ? hips.getWorldPosition(new THREE.Vector3()).y : 0
  return { boneNames, boneCount: boneNames.length, hipsHeight }
}

export function checkCompatibility(skeleton: RigSkeleton): Compatibility {
  const present = new Set(skeleton.boneNames)
  const missing = CORE_BONES.filter((bone) => !present.has(bone))

  const loose = new Map<string, string>()
  for (const name of skeleton.boneNames) {
    const key = looseKey(name)
    if (!loose.has(key)) loose.set(key, name)
  }
  const nearMisses: NearMiss[] = []
  for (const want of missing) {
    const have = loose.get(looseKey(want))
    if (have) nearMisses.push({ want, have })
  }

  return { missing, nearMisses, compatible: missing.length === 0 }
}

/** Case, separators and a Mixamo prefix are all a rename can differ by. */
function looseKey(name: string): string {
  return name
    .toLowerCase()
    .replace(/^mixamorig[0-9]*[:_]?/, '')
    .replace(/[^a-z0-9]/g, '')
}

/**
 * The pack rig walks 1.8 m/s with its hips 1.165 m up. Retargeting moves
 * rotations only, so stride — and the speed that does not skate — scales with
 * hip height (doc/assets/monsters.md).
 */
export const PACK_HIPS_HEIGHT = 1.165

export function walkSpeedFor(hipsHeight: number): number {
  return (1.8 * hipsHeight) / PACK_HIPS_HEIGHT
}
