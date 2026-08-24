/**
 * Turning a download into something the game's retargeter can read.
 *
 * Two things about a Mixamo file stop it retargeting, and both are silent
 * failures rather than errors:
 *
 * - Bone names arrive as `mixamorig:Hips`. The game matches source to target
 *   bones by exact name (`buildBoneNameMap`), and the rigs in the repo have the
 *   prefix stripped, so nothing matches and the clip is handed back unretargeted
 *   — it plays, and it plays wrong. three's PropertyBinding cannot parse a `:`
 *   either, which is why the repo strips it everywhere.
 * - FBX comes in centimetres. The hip correction works off the bone's local
 *   rest height, so a source rig 100× too big throws the hip track metres into
 *   the air.
 *
 * A take downloaded "Without Skin" has no SkinnedMesh, and the game's
 * retargeter needs one on both sides. That is reported rather than worked
 * around: the fix is to download the take again with skin.
 */
import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { FBXLoader } from 'three/examples/jsm/loaders/FBXLoader.js'

export interface LoadedTake {
  scene: THREE.Object3D
  clips: THREE.AnimationClip[]
  /** Set when the file cannot drive a retarget, and why. */
  problem: string | null
  scaledBy: number
  boneCount: number
}

const gltfLoader = new GLTFLoader()
const fbxLoader = new FBXLoader()
const cache = new Map<string, Promise<LoadedTake>>()

/**
 * Forget a take's parsed form.
 *
 * Removing a take frees its name, so the next file added under that name gets
 * the same URL — and would be served the deleted take's bytes out of this cache.
 */
export function forgetTake(url: string): void {
  cache.delete(url)
}

export function loadTake(url: string): Promise<LoadedTake> {
  let pending = cache.get(url)
  if (!pending) {
    pending = fetchTake(url)
    cache.set(url, pending)
  }
  return pending
}

async function fetchTake(url: string): Promise<LoadedTake> {
  let scene: THREE.Object3D
  let clips: THREE.AnimationClip[]
  if (/\.fbx$/i.test(url)) {
    const group = await fbxLoader.loadAsync(url)
    scene = group
    clips = group.animations ?? []
  } else {
    const gltf = await gltfLoader.loadAsync(url)
    scene = gltf.scene
    clips = gltf.animations ?? []
  }

  stripMixamoPrefix(scene, clips)
  const { root, factor: scaledBy } = normaliseRig(scene, clips)
  scene = root

  let boneCount = 0
  let skinned = false
  scene.traverse((object) => {
    if ((object as THREE.Bone).isBone) boneCount += 1
    if ((object as THREE.SkinnedMesh).isSkinnedMesh) skinned = true
  })

  let problem: string | null = null
  if (clips.length === 0) problem = 'No animation in this file.'
  else if (!skinned) problem = 'No skinned mesh — download this take again with skin.'
  else if (!scene.getObjectByName('Hips')) problem = 'No Hips bone after stripping the Mixamo prefix.'

  return { scene, clips, problem, scaledBy, boneCount }
}

/** `mixamorig:Hips` and `mixamorig1Hips` both become `Hips`, in nodes and tracks. */
export function standardBoneName(name: string): string {
  return name.replace(/^mixamorig[0-9]*[:_]?/i, '')
}

function stripMixamoPrefix(scene: THREE.Object3D, clips: THREE.AnimationClip[]): void {
  scene.traverse((object) => {
    object.name = standardBoneName(object.name)
  })
  for (const clip of clips) {
    for (const track of clip.tracks) {
      const dot = track.name.lastIndexOf('.')
      if (dot < 0) continue
      track.name = `${standardBoneName(track.name.slice(0, dot))}${track.name.slice(dot)}`
    }
  }
}

/**
 * Put a downloaded rig into the shape the shipped packs are in: an identity
 * root, with every bone's local rest position in metres.
 *
 * FBXLoader hands a Mixamo rig back as `root -> Armature -> Hips`, with the
 * centimetre unit parked as `scale: 100` on the Armature group and most of the
 * hip height sitting in that group's own offset rather than in the Hips bone.
 * Left that way the rig *looks* right — world positions come out correct, so
 * it previews fine — but the game does not read world matrices for everything:
 * `getHipBoneRestY` reads the Hips bone's **local** rest position and shifts
 * the retargeted hip track by the difference against the target rig's. A rig
 * whose locals are a hundredth of a metre makes that correction about a metre
 * too large, and every rig it drives stands a metre off the floor.
 *
 * So the unit conversion cannot be applied to the bones' locals while a scale
 * stays on their parent, the way it used to be. The whole rig is scaled, then
 * the group transforms are folded into the bones so nothing is left above
 * them — which is exactly the shape `locomotion.glb` has, and the shape the
 * hip correction assumes.
 */
function normaliseRig(scene: THREE.Object3D, clips: THREE.AnimationClip[]): { root: THREE.Object3D; factor: number } {
  scene.updateMatrixWorld(true)

  const bones: THREE.Bone[] = []
  scene.traverse((object) => {
    if ((object as THREE.Bone).isBone) bones.push(object as THREE.Bone)
  })
  if (bones.length === 0) return { root: scene, factor: 1 }

  const box = new THREE.Box3()
  const point = new THREE.Vector3()
  for (const bone of bones) box.expandByPoint(bone.getWorldPosition(point))
  // A humanoid is around 2 m and never 20. Anything that big came in as
  // centimetres; anything already in metres is left alone.
  const factor = box.max.y - box.min.y > 10 ? 0.01 : 1

  scene.scale.multiplyScalar(factor)
  scene.updateMatrixWorld(true)

  const root = new THREE.Group()
  root.name = 'Armature'

  // Only the bones at the top of the skeleton move: folding a parent's world
  // transform into them preserves every descendant's world transform, because
  // the descendants' locals are relative to a parent whose world matrix has
  // not changed.
  const parentWorld = new THREE.Matrix4()
  const parentSpin = new THREE.Quaternion()
  const spin = new THREE.Quaternion()
  const scratch = new THREE.Vector3()
  for (const bone of bones) {
    if ((bone.parent as THREE.Bone | null)?.isBone) continue
    parentWorld.copy(bone.parent?.matrixWorld ?? new THREE.Matrix4())
    parentWorld.decompose(scratch, parentSpin, scratch)

    // Every keyframe of this bone is expressed in its old parent's space, so
    // the whole track moves with the rest pose: position through the full
    // matrix, rotation through the parent's rotation alone. Moving one and not
    // the other is worse than moving neither — the rest pose ends up carrying
    // the FBX's Z-up correction while the clip still expects it, and the rig
    // plays folded up.
    for (const clip of clips) {
      for (const track of clip.tracks) {
        if (track.name === `${bone.name}.position`) {
          for (let i = 0; i + 2 < track.values.length; i += 3) {
            point.set(track.values[i], track.values[i + 1], track.values[i + 2]).applyMatrix4(parentWorld)
            track.values[i] = point.x
            track.values[i + 1] = point.y
            track.values[i + 2] = point.z
          }
        } else if (track.name === `${bone.name}.quaternion`) {
          for (let i = 0; i + 3 < track.values.length; i += 4) {
            spin
              .set(track.values[i], track.values[i + 1], track.values[i + 2], track.values[i + 3])
              .premultiply(parentSpin)
            track.values[i] = spin.x
            track.values[i + 1] = spin.y
            track.values[i + 2] = spin.z
            track.values[i + 3] = spin.w
          }
        }
      }
    }

    bone.matrixWorld.decompose(bone.position, bone.quaternion, bone.scale)
    root.add(bone)
  }

  // The mesh only rides along as something for the retargeter to find a
  // skeleton on, but it cannot keep a transform of its own.
  //
  // `GLTFExporter.processSkin` writes `boneInverse × bindMatrix` into the
  // file's inverseBindMatrices, and `Skeleton.pose()` — which the game runs
  // before every retarget — rebuilds each bone's local rest position from
  // exactly those. A mesh bound with anything but the identity therefore
  // smuggles its own transform into the rest pose the game reads back, and
  // the hip-height correction is computed against the wrong number. So the
  // mesh's world transform goes into its geometry instead, and it sits at
  // the origin bound to the identity.
  let mesh: THREE.SkinnedMesh | null = null
  scene.traverse((object) => {
    if ((object as THREE.SkinnedMesh).isSkinnedMesh && !mesh) mesh = object as THREE.SkinnedMesh
  })
  if (mesh) {
    const skinned = mesh as THREE.SkinnedMesh
    skinned.geometry = skinned.geometry.clone()
    skinned.geometry.applyMatrix4(skinned.matrixWorld)
    skinned.position.set(0, 0, 0)
    skinned.quaternion.identity()
    skinned.scale.set(1, 1, 1)
    root.add(skinned)
  }

  root.updateMatrixWorld(true)
  if (mesh) {
    const skinned = mesh as THREE.SkinnedMesh
    // The rest pose we just built is the bind pose.
    skinned.skeleton.calculateInverses()
    skinned.bind(skinned.skeleton, new THREE.Matrix4())
  }

  // A pack animates bones and nothing else. Mixamo's clips carry a track for
  // the Armature group they came under, which no longer exists here and which
  // the shipped packs do not have either.
  const boneNames = new Set(bones.map((bone) => bone.name))
  for (const clip of clips) {
    clip.tracks = clip.tracks.filter((track) => boneNames.has(track.name.split('.')[0]))
  }

  return { root, factor }
}
