/**
 * Material repair.
 *
 * Two providers, three reliable defects. A Mixamo FBX arrives with metallic 1
 * and `KHR_materials_specular` boosted to 2x, which reads as black chrome or,
 * with a light albedo, a washed-out pale sheen. Meshy ships an emissive that
 * makes the model self-lit and immune to the scene's lighting. All of them are
 * just numbers on the material, so all of them are fixed on the container.
 *
 * The reference point is what the hand-processed monsters carry: hobgoblin,
 * gnoll, ogre and troll have no specular extension at all.
 */
import type { GlbContainer, GltfMaterial } from './container'

export interface MaterialSettings {
  metallicFactor: number
  roughnessFactor: number
  clearEmissive: boolean
  /**
   * What to do with KHR_materials_specular.
   *
   * `remove` matches the hand-processed monsters, which carry no specular
   * extension at all. `set` writes a single specularFactor — stone_golem ships
   * a deliberate 0.3 that way. `keep` leaves whatever the importer produced,
   * including the 2x boost, which is only ever what you want for comparison.
   */
  specularMode: 'remove' | 'set' | 'keep'
  /** Used when specularMode is 'set'. */
  specularFactor: number
  /** Drop the metallic-roughness texture and rely on the factors alone. */
  dropMetallicRoughnessTexture: boolean
}

export const DEFAULT_MATERIAL_SETTINGS: MaterialSettings = {
  metallicFactor: 0,
  roughnessFactor: 0.9,
  clearEmissive: true,
  specularMode: 'remove',
  specularFactor: 0.3,
  dropMetallicRoughnessTexture: false,
}

export interface MaterialReport {
  index: number
  name: string
  metallicFactor: number
  roughnessFactor: number
  emissive: [number, number, number]
  hasBaseColorTexture: boolean
  hasMetallicRoughnessTexture: boolean
  hasEmissiveTexture: boolean
  alphaMode: string
  /** Strongest channel of KHR_materials_specular, or null when absent. */
  specular: number | null
}

export function readMaterials(c: GlbContainer): MaterialReport[] {
  return (c.json.materials ?? []).map((material, index) => {
    const pbr = material.pbrMetallicRoughness ?? {}
    const emissive = (material.emissiveFactor ?? [0, 0, 0]) as [number, number, number]
    return {
      index,
      name: material.name ?? `material_${index}`,
      metallicFactor: pbr.metallicFactor ?? 1,
      roughnessFactor: pbr.roughnessFactor ?? 1,
      emissive,
      hasBaseColorTexture: pbr.baseColorTexture !== undefined,
      hasMetallicRoughnessTexture: pbr.metallicRoughnessTexture !== undefined,
      hasEmissiveTexture: material.emissiveTexture !== undefined,
      alphaMode: material.alphaMode ?? 'OPAQUE',
      specular: specularOf(material),
    }
  })
}

const SPECULAR_EXTENSION = 'KHR_materials_specular'

interface SpecularExtension {
  specularFactor?: number
  specularColorFactor?: number[]
}

function specularOf(material: GltfMaterial): number | null {
  const specular = material.extensions?.[SPECULAR_EXTENSION] as SpecularExtension | undefined
  if (!specular) return null

  // Only what the file actually sets. Folding in the glTF default of 1 for the
  // absent half would report a deliberate 0.3 as a 1.
  const authored = [
    ...(specular.specularFactor !== undefined ? [specular.specularFactor] : []),
    ...(specular.specularColorFactor ?? []),
  ]
  return authored.length > 0 ? Math.max(...authored) : 1
}

/** True when a material still carries the defects the importers leave behind. */
export function needsRepair(report: MaterialReport): boolean {
  const emissive = report.emissive.some((channel) => channel > 0.01) || report.hasEmissiveTexture
  const chrome = report.metallicFactor > 0.5 && !report.hasMetallicRoughnessTexture
  // Anything above physical white is a boost the importer applied, not a choice.
  const glare = report.specular !== null && report.specular > 1.001
  return emissive || chrome || glare
}

export function applyMaterialSettings(c: GlbContainer, settings: MaterialSettings): void {
  for (const material of c.json.materials ?? []) {
    const pbr = (material.pbrMetallicRoughness ??= {})
    pbr.metallicFactor = settings.metallicFactor
    pbr.roughnessFactor = settings.roughnessFactor

    if (settings.clearEmissive) {
      material.emissiveFactor = [0, 0, 0]
      delete material.emissiveTexture
      delete (material.extensions ?? {})['KHR_materials_emissive_strength']
    }
    applySpecular(material, settings)
    if (settings.dropMetallicRoughnessTexture) delete pbr.metallicRoughnessTexture
  }
  if (settings.clearEmissive) dropUnusedExtension(c, 'KHR_materials_emissive_strength')
  if (settings.specularMode === 'remove') dropUnusedExtension(c, SPECULAR_EXTENSION)
  if (settings.specularMode === 'set') declareExtension(c, SPECULAR_EXTENSION)
}

function applySpecular(material: GltfMaterial, settings: MaterialSettings): void {
  if (settings.specularMode === 'keep') return

  if (settings.specularMode === 'remove') {
    if (material.extensions?.[SPECULAR_EXTENSION] === undefined) return
    delete material.extensions[SPECULAR_EXTENSION]
    if (Object.keys(material.extensions).length === 0) delete material.extensions
    return
  }

  // A single factor, and no colour tint: the 2x the importers apply lives in
  // specularColorFactor, so leaving it would undo the point of setting this.
  material.extensions ??= {}
  material.extensions[SPECULAR_EXTENSION] = { specularFactor: settings.specularFactor }
}

function declareExtension(c: GlbContainer, name: string): void {
  c.json.extensionsUsed ??= []
  if (!c.json.extensionsUsed.includes(name)) c.json.extensionsUsed.push(name)
}

function dropUnusedExtension(c: GlbContainer, name: string): void {
  const stillUsed = (c.json.materials ?? []).some((m) => m.extensions?.[name] !== undefined)
  if (stillUsed) return
  c.json.extensionsUsed = c.json.extensionsUsed?.filter((e) => e !== name)
  c.json.extensionsRequired = c.json.extensionsRequired?.filter((e) => e !== name)
  if (c.json.extensionsUsed?.length === 0) delete c.json.extensionsUsed
  if (c.json.extensionsRequired?.length === 0) delete c.json.extensionsRequired
}

/** Point a material at a metallic-roughness image, wiring up texture + sampler. */
export function attachMetallicRoughnessTexture(c: GlbContainer, materialIndex: number, imageIndex: number): void {
  c.json.textures ??= []
  const existing = c.json.textures.findIndex((t) => t.source === imageIndex)
  const textureIndex = existing >= 0 ? existing : c.json.textures.push({ source: imageIndex }) - 1

  const material: GltfMaterial = c.json.materials![materialIndex]
  const pbr = (material.pbrMetallicRoughness ??= {})
  pbr.metallicRoughnessTexture = { index: textureIndex }
  // The texture supplies both channels; the factors become multipliers.
  pbr.metallicFactor = 1
  pbr.roughnessFactor = 1
}

/** Base-colour image index for a material, if it has one. */
export function baseColorImage(c: GlbContainer, materialIndex: number): number | null {
  const ref = c.json.materials?.[materialIndex]?.pbrMetallicRoughness?.baseColorTexture
  if (!ref) return null
  return c.json.textures?.[ref.index]?.source ?? null
}
