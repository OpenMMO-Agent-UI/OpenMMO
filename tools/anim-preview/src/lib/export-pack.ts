/**
 * Build a pack out of the decided takes.
 *
 * A pack is one rig plus named clips — that is all the game reads out of
 * locomotion.glb. So the export takes the rig from one designated take, puts
 * every decided clip onto it through the game's own retargeter, and writes the
 * lot as a single GLB.
 *
 * Routing the clips through the retargeter is not busywork even when every take
 * came from Mixamo on the same skeleton: in that case the game's own
 * `hasEquivalentSkeletonRestPose` check short-circuits and hands the clips back
 * untouched, and when a take does come from somewhere else it is corrected
 * instead of quietly writing a pack with two skeletons in it.
 *
 * One file, not two. The split between locomotion.glb and combat_melee.glb
 * exists because those two were exported from different armatures — 33 bones
 * and 69. A pack built here has one rig throughout, and the `packPaths`
 * argument on the client's loader takes any number of files, so there is no
 * reason to cut it in half.
 */
import * as THREE from 'three'
import { GLTFExporter } from 'three/examples/jsm/exporters/GLTFExporter.js'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import * as SkeletonUtils from 'three/examples/jsm/utils/SkeletonUtils.js'
import { retargetTake } from './retarget'
import { loadTake } from './takes'

export interface PackDecision {
  motion: string
  /** The take to use, or null to carry the pack's own clip through. */
  takePath: string | null
}

export interface PackBuild {
  bytes: Uint8Array
  clipNames: string[]
  /** Motions taken unchanged from the pack being replaced. */
  carriedOver: string[]
  /** Take whose rig the pack is built on. */
  baseTakePath: string
}

/**
 * @param sourcePackUrl the pack being replaced. Motions with no take of their
 *   own are carried across from it, so the file is a whole pack rather than
 *   only the parts that changed — a pack missing `jump` because `jump` was
 *   left alone would break every rig that asks for it.
 * @param baseTakePath which take supplies the rig. Defaults to the first
 *   decision that has one, which is right whenever the takes share a skeleton.
 */
export async function buildPack(
  decisions: PackDecision[],
  sourcePackUrl: string | null = null,
  baseTakePath = decisions.find((decision) => decision.takePath)?.takePath ?? undefined
): Promise<PackBuild> {
  if (decisions.length === 0) throw new Error('Nothing decided yet.')
  if (!baseTakePath) throw new Error('No take to build the rig from.')

  const base = await loadTake(`/takes/${baseTakePath}`)
  if (base.problem) throw new Error(`The base take cannot supply a rig: ${base.problem}`)

  // SkeletonUtils, not Object3D.clone: a plain clone of a SkinnedMesh keeps
  // pointing at the original's skeleton, and stubbing its mesh would reach
  // back into the cached take.
  const rig = stubMesh(SkeletonUtils.clone(base.scene))
  const clips: THREE.AnimationClip[] = []

  // Loaded once, and only if something actually falls back to it.
  let sourcePack: { scene: THREE.Object3D; animations: THREE.AnimationClip[] } | null = null
  const carriedOver: string[] = []

  for (const { motion, takePath } of decisions) {
    let source: THREE.AnimationClip | undefined
    let sourceScene: THREE.Object3D

    if (takePath) {
      const take = await loadTake(`/takes/${takePath}`)
      if (take.problem) throw new Error(`${motion}: ${take.problem}`)
      source = take.clips[0]
      sourceScene = take.scene
      if (!source) throw new Error(`${motion}: no clip in ${takePath}.`)
    } else {
      if (!sourcePackUrl) continue
      sourcePack ??= await loadPack(sourcePackUrl)
      source = sourcePack.animations.find((clip) => clip.name === motion)
      sourceScene = sourcePack.scene
      // A motion the source pack does not actually have is not an error: the
      // ladder lists what is in the file, so this only happens if it changed
      // underfoot.
      if (!source) continue
      carriedOver.push(motion)
    }

    // Retargeted onto the rig this pack is built on, whoever supplied the
    // clip — a pack with two skeletons in it is not a pack.
    const onto = takePath === baseTakePath ? [source] : await retargetTake(rig, sourceScene, [source])
    const clip = onto[0].clone()
    clip.name = motion
    clips.push(clip)
  }

  const exporter = new GLTFExporter()
  const glb = (await exporter.parseAsync(rig, {
    binary: true,
    animations: clips,
    onlyVisible: false,
    embedImages: false,
  })) as ArrayBuffer

  return { bytes: new Uint8Array(glb), clipNames: clips.map((clip) => clip.name), carriedOver, baseTakePath }
}

/**
 * Read a pack straight, with none of `loadTake`'s Mixamo repairs.
 *
 * Those repairs — stripping the `mixamorig:` prefix, converting centimetres,
 * flattening the FBX's root transform — exist for a download. A pack already
 * has an identity root and standard bone names, so running them over it would
 * be re-normalising something already normal, and `loadTake` would cache the
 * mutated result under the pack's own URL.
 */
async function loadPack(url: string) {
  const gltf = await new GLTFLoader().loadAsync(url)
  return { scene: gltf.scene as THREE.Object3D, animations: gltf.animations ?? [] }
}

/**
 * Throw the character body away and leave a stub in its place.
 *
 * A pack is a skeleton and some curves; nothing ever renders the pack rig, and
 * the only reason a mesh is here at all is that the game finds the skeleton
 * through `findPrimarySkinnedMesh`. The shipped packs are already built this
 * way — every one of them carries a three-vertex mesh with no material and no
 * indices — whereas a take is a Mixamo download *with skin*, so exporting its
 * rig as-is drags 30,000 vertices of character along: 1.6 MB of a body that is
 * never drawn.
 *
 * Matched to what the shipped packs contain, down to the attribute set, so a
 * written pack and a shipped one are the same kind of file.
 */
function stubMesh(root: THREE.Object3D): THREE.Object3D {
  root.traverse((object) => {
    const skinned = object as THREE.SkinnedMesh
    if (!skinned.isSkinnedMesh) return

    const geometry = new THREE.BufferGeometry()
    geometry.setAttribute('position', new THREE.Float32BufferAttribute(new Float32Array(9), 3))
    geometry.setAttribute('skinIndex', new THREE.Uint8BufferAttribute(new Uint8Array(12), 4))
    geometry.setAttribute('skinWeight', new THREE.Float32BufferAttribute(
      new Float32Array([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]), 4
    ))
    skinned.geometry = geometry
    skinned.material = new THREE.MeshBasicMaterial()
  })
  return root
}
