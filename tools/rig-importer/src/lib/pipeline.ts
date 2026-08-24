/**
 * One pass from the imported file to the bytes that would be written.
 *
 * Every setting change re-runs this from the pristine import rather than
 * editing the previous result, so nothing accumulates: dragging the height
 * slider back and forth lands on exactly the model you started with, and the
 * preview is always the artifact.
 */
import {
  appendBufferView,
  buildGlb,
  cloneContainer,
  bufferViewBytes,
  type GlbContainer,
} from './gltf/container'
import {
  bakeUniformScale,
  flattenRootScale,
  keepAnimations,
  originDelta,
  renameAnimations,
  renameNodes,
  restPoseBounds,
  scaledNodes,
  shiftRootNodes,
} from './gltf/transform'
import { imageInfos, modelStats, type ModelStats } from './gltf/measure'
import { stripLightsAndCameras } from './gltf/prune'
import {
  applyMaterialSettings,
  attachMetallicRoughnessTexture,
  baseColorImage,
  DEFAULT_MATERIAL_SETTINGS,
  type MaterialSettings,
} from './gltf/materials'
import {
  DEFAULT_MR_PARAMS,
  DEFAULT_TEXTURE_SETTINGS,
  deriveMetallicRoughness,
  recompressTextures,
  type MetallicRoughnessParams,
  type TextureChange,
  type TextureSettings,
} from './gltf/textures'
import type { BoneGuess } from './bones/match'

export interface PipelineSettings {
  boneMapping: BoneGuess[]
  renameBones: boolean
  /** Metres. Null keeps whatever the file came in at. */
  targetHeight: number | null
  recentre: boolean
  material: MaterialSettings
  deriveMetallicRoughness: boolean
  mrParams: MetallicRoughnessParams
  texture: TextureSettings
  /** Clip names to keep, or null for all of them. */
  keepClips: string[] | null
  clipRenames: Record<string, string>
}

export function defaultSettings(): PipelineSettings {
  return {
    boneMapping: [],
    renameBones: true,
    targetHeight: null,
    recentre: true,
    material: { ...DEFAULT_MATERIAL_SETTINGS },
    deriveMetallicRoughness: false,
    mrParams: { ...DEFAULT_MR_PARAMS },
    texture: { ...DEFAULT_TEXTURE_SETTINGS },
    keepClips: null,
    clipRenames: {},
  }
}

/**
 * The part of a run that is safe to hold in reactive state. The container is
 * deliberately not in here: Svelte deep-proxies plain objects, and a proxied
 * glTF json breaks structuredClone and puts a proxy hop in front of every
 * accessor read.
 */
export interface PipelineOutput {
  bytes: Uint8Array
  stats: ModelStats
  textureChanges: TextureChange[]
  scaleFactor: number
  originShift: [number, number, number]
  metalFraction: number | null
  warnings: string[]
  /** Node scale removed on import, and any that could not be. */
  flattenedScale: number
  nodesStillScaled: number
  /** Scene furniture stripped out: lights and cameras. */
  prunedLights: number
  prunedCameras: number
}

export interface PipelineResult extends PipelineOutput {
  container: GlbContainer
}

export async function runPipeline(
  original: GlbContainer,
  settings: PipelineSettings
): Promise<PipelineResult> {
  const warnings: string[] = []
  let container = cloneContainer(original)

  // Before anything is measured or renamed: a light in the file is parented to
  // the model and would light the world wherever the monster walks, and both
  // removals renumber nodes.
  const pruned = stripLightsAndCameras(container)
  if (pruned.lights > 0 || pruned.cameras > 0) {
    const parts = [
      pruned.lights > 0 && `${pruned.lights} light${pruned.lights === 1 ? '' : 's'}`,
      pruned.cameras > 0 && `${pruned.cameras} camera${pruned.cameras === 1 ? '' : 's'}`,
    ].filter(Boolean)
    warnings.push(
      `Removed ${parts.join(' and ')} the file was carrying${
        pruned.removedNodes.length > 0 ? `, along with the nodes holding them (${pruned.removedNodes.join(', ')})` : ''
      }. A light parented to the model travels with it.`
    )
  }

  // A scale left on the armature makes every bone a scaled space, so weapon
  // offsets stop being metres and anything parented to a bone is sized wrong.
  const flattenedScale = flattenRootScale(container)
  if (flattenedScale !== 1) {
    warnings.push(
      `The armature came in scaled ${flattenedScale.toPrecision(4)}×. Baked into the mesh and skeleton data, the way the shipped models are authored.`
    )
  }

  if (settings.renameBones) {
    const names = new Map<number, string>()
    for (const guess of settings.boneMapping) {
      if (guess.node !== null) names.set(guess.node, guess.standard)
    }
    renameNodes(container, names)
  }

  const sourceHeight = restPoseBounds(container).max[1] - restPoseBounds(container).min[1]
  let scaleFactor = 1
  if (settings.targetHeight && settings.targetHeight > 0 && sourceHeight > 0) {
    scaleFactor = settings.targetHeight / sourceHeight
    if (scaleFactor !== 1) bakeUniformScale(container, scaleFactor)
  }

  let originShift: [number, number, number] = [0, 0, 0]
  if (settings.recentre) {
    originShift = originDelta(container)
    shiftRootNodes(container, originShift)
  }

  if (settings.keepClips) {
    keepAnimations(container, new Set(settings.keepClips))
  }
  if (Object.keys(settings.clipRenames).length > 0) {
    const renames = new Map<number, string>()
    ;(container.json.animations ?? []).forEach((clip, index) => {
      const next = settings.clipRenames[clip.name ?? '']
      if (next) renames.set(index, next)
    })
    renameAnimations(container, renames)
  }

  applyMaterialSettings(container, settings.material)

  let metalFraction: number | null = null
  if (settings.deriveMetallicRoughness) {
    const derived = await addMetallicRoughness(container, settings)
    metalFraction = derived.metalFraction
    if (derived.skipped.length > 0) {
      warnings.push(`No base-colour texture on ${derived.skipped.join(', ')} — nothing to derive from.`)
    }
  }

  const recompressed = await recompressTextures(container, settings.texture)
  container = recompressed.container

  const stillScaled = scaledNodes(container)
  if (stillScaled.length > 0) {
    warnings.push(
      `${stillScaled.length} node${stillScaled.length === 1 ? '' : 's'} still carry a scale this tool cannot flatten — a weapon parented to a bone below one will render at the wrong size.`
    )
  }

  const bytes = buildGlb(container)
  return {
    container,
    bytes,
    stats: modelStats(container, bytes.byteLength),
    textureChanges: recompressed.changes,
    scaleFactor,
    originShift,
    metalFraction,
    warnings,
    flattenedScale,
    nodesStillScaled: stillScaled.length,
    prunedLights: pruned.lights,
    prunedCameras: pruned.cameras,
  }
}

async function addMetallicRoughness(
  container: GlbContainer,
  settings: PipelineSettings
): Promise<{ metalFraction: number | null; skipped: string[] }> {
  const infos = imageInfos(container)
  const skipped: string[] = []
  let metalFraction: number | null = null

  for (let materialIndex = 0; materialIndex < (container.json.materials?.length ?? 0); materialIndex++) {
    const albedoIndex = baseColorImage(container, materialIndex)
    const albedo = albedoIndex === null ? null : infos[albedoIndex]
    if (!albedo || albedo.width === 0) {
      skipped.push(container.json.materials![materialIndex].name ?? `material_${materialIndex}`)
      continue
    }

    const source = bufferViewBytes(container, container.json.images![albedoIndex!].bufferView!)
    const derived = await deriveMetallicRoughness(source, albedo.mimeType, settings.mrParams, settings.texture.maxSize)
    metalFraction = derived.metalFraction

    const bufferView = appendBufferView(container, derived.bytes)
    container.json.images ??= []
    const imageIndex = container.json.images.push({ bufferView, mimeType: 'image/jpeg', name: `mr_${materialIndex}` }) - 1
    attachMetallicRoughnessTexture(container, materialIndex, imageIndex)
  }

  return { metalFraction, skipped }
}
