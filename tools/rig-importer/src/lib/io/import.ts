/**
 * Getting a dropped file into the container the rest of the tool edits.
 *
 * GLB goes straight in. FBX — what Mixamo hands back after auto-rigging — has
 * to be converted, and that is the one place three.js touches the asset data.
 */
import * as THREE from 'three'
import { FBXLoader } from 'three/examples/jsm/loaders/FBXLoader.js'
import { GLTFExporter } from 'three/examples/jsm/exporters/GLTFExporter.js'
import { parseGlb, type GlbContainer } from '../gltf/container'

export interface ImportedModel {
  container: GlbContainer
  bytes: Uint8Array
  fileName: string
  /** True when the file had to be converted from FBX on the way in. */
  converted: boolean
  notes: string[]
}

export async function importModelFile(file: File): Promise<ImportedModel> {
  const bytes = new Uint8Array(await file.arrayBuffer())
  const extension = file.name.toLowerCase().split('.').pop() ?? ''

  if (extension === 'glb') {
    return { container: parseGlb(bytes), bytes, fileName: file.name, converted: false, notes: [] }
  }
  if (extension === 'fbx') {
    return convertFbx(bytes, file.name)
  }
  throw new Error(`Unsupported file type ".${extension}" — drop a .glb or a .fbx`)
}

async function convertFbx(bytes: Uint8Array, fileName: string): Promise<ImportedModel> {
  const notes: string[] = []
  const scene = new FBXLoader().parse(bytes.buffer as ArrayBuffer, '')

  // FBXLoader hands back the scene immediately but loads its embedded textures
  // through TextureLoader off blob URLs, so they are still decoding. Exporting
  // now fails with "No valid image data found".
  const unresolved = await settleTextures(scene)
  if (unresolved.length > 0) {
    notes.push(
      `${unresolved.length} texture${unresolved.length === 1 ? '' : 's'} never arrived and ${unresolved.length === 1 ? 'was' : 'were'} dropped — this FBX references files it does not embed. The model imports untextured.`
    )
  }

  // three.js reads "." and ":" in a node name as property-path separators, and
  // GLTFExporter silently drops every track bound to one. Mixamo names every
  // bone "mixamorig:Something", so this has to happen before the export.
  let renamed = 0
  scene.traverse((object) => {
    const clean = object.name.replace(/^mixamorig\d*:?/i, '').replace(/[.:[\]]/g, '_')
    if (clean !== object.name && clean.length > 0) {
      renamed++
      object.name = clean
    }
  })
  if (renamed > 0) notes.push(`Renamed ${renamed} nodes so three.js can bind animation tracks to them.`)

  const animations = scene.animations ?? []
  for (const clip of animations) {
    for (const track of clip.tracks) {
      track.name = track.name.replace(/^mixamorig\d*:?/i, '')
    }
  }
  const exporter = new GLTFExporter()
  const glb = (await exporter.parseAsync(scene, {
    binary: true,
    animations,
    onlyVisible: false,
    // Downsizing happens on the Material step, where it can be seen.
    maxTextureSize: 4096,
  })) as ArrayBuffer

  const converted = new Uint8Array(glb)
  return { container: parseGlb(converted), bytes: converted, fileName, converted: true, notes }
}

type TextureSlot = { material: THREE.Material; key: string; texture: THREE.Texture }

function textureSlots(scene: THREE.Object3D): TextureSlot[] {
  const slots: TextureSlot[] = []
  const seen = new Set<THREE.Material>()

  scene.traverse((object) => {
    const mesh = object as THREE.Mesh
    if (!mesh.isMesh) return
    for (const material of Array.isArray(mesh.material) ? mesh.material : [mesh.material]) {
      if (!material || seen.has(material)) continue
      seen.add(material)
      for (const [key, value] of Object.entries(material)) {
        if (value instanceof THREE.Texture) slots.push({ material, key, texture: value })
      }
    }
  })
  return slots
}

function decoded(texture: THREE.Texture): boolean {
  const image = texture.image as { width?: number; height?: number; data?: unknown } | undefined
  if (!image) return false
  if (image.data) return true
  return (image.width ?? 0) > 0 && (image.height ?? 0) > 0
}

/**
 * Wait for every texture to finish decoding, and unhook the ones that never
 * do. A missing texture should cost the model its colour, not the whole import.
 */
async function settleTextures(scene: THREE.Object3D, timeoutMs = 20_000): Promise<string[]> {
  const slots = textureSlots(scene)
  const deadline = Date.now() + timeoutMs

  while (slots.some((slot) => !decoded(slot.texture)) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 25))
  }

  const dropped: string[] = []
  for (const slot of slots) {
    if (decoded(slot.texture)) continue
    dropped.push(slot.texture.name || slot.key)
    ;(slot.material as unknown as Record<string, unknown>)[slot.key] = null
    slot.material.needsUpdate = true
  }
  return dropped
}
