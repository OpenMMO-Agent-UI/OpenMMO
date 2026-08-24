import { describe, expect, it } from 'vitest'
import { parseGlb } from '../src/lib/gltf/container'
import { defaultSettings, runPipeline } from '../src/lib/pipeline'
import { guessBoneMapping, type Joint } from '../src/lib/bones/match'
import { nodeParents } from '../src/lib/gltf/measure'
import { restPoseBounds } from '../src/lib/gltf/transform'
import { needsRepair, readMaterials } from '../src/lib/gltf/materials'
import { loadFixture } from './fixtures'

/**
 * The pipeline runs unchanged in node as long as no texture actually needs
 * re-encoding — which is the case for the shipped models, whose textures are
 * already 1024² JPEG. That covers everything except the canvas work.
 */
function joints(container: ReturnType<typeof parseGlb>): Joint[] {
  const parents = nodeParents(container)
  const nodes = new Set<number>()
  for (const skin of container.json.skins ?? []) for (const joint of skin.joints) nodes.add(joint)
  return [...nodes].map((node) => ({ node, name: container.json.nodes![node].name ?? '', parent: parents[node] }))
}

function settingsFor(container: ReturnType<typeof parseGlb>) {
  return { ...defaultSettings(), boneMapping: guessBoneMapping(joints(container)) }
}

/** kobold ships a 2048² texture, which really would need the canvas to resize. */
function keepingTextures(container: ReturnType<typeof parseGlb>) {
  const settings = settingsFor(container)
  return { ...settings, texture: { ...settings.texture, maxSize: 2048 } }
}

describe('pipeline', () => {
  it('produces a GLB that parses back with the model intact', async () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const result = await runPipeline(container, { ...settingsFor(container), targetHeight: 2.05 })

    const reloaded = parseGlb(result.bytes)
    expect(reloaded.json.skins?.[0].joints).toHaveLength(65)
    expect(reloaded.json.animations ?? []).toHaveLength(0)
    expect(restPoseBounds(reloaded).max[1] - restPoseBounds(reloaded).min[1]).toBeCloseTo(2.05, 3)
    expect(restPoseBounds(reloaded).min[1]).toBeCloseTo(0, 4)
  })

  it('leaves the source container untouched, so settings can be revisited', async () => {
    const { container } = loadFixture('monsters/ogre.glb')
    const before = restPoseBounds(container).max[1]

    await runPipeline(container, { ...settingsFor(container), targetHeight: 1.2 })

    expect(restPoseBounds(container).max[1]).toBeCloseTo(before, 6)
  })

  it('lands on the same result whatever order the heights were tried in', async () => {
    const { container } = loadFixture('monsters/troll.glb')
    const settings = settingsFor(container)

    for (const height of [0.9, 3.5, 1.75]) await runPipeline(container, { ...settings, targetHeight: height })
    const direct = await runPipeline(container, { ...settings, targetHeight: 2.4 })

    expect(direct.stats.height).toBeCloseTo(2.4, 4)
    expect(direct.stats.byteLength).toBe((await runPipeline(container, { ...settings, targetHeight: 2.4 })).stats.byteLength)
  })

  it('repairs the material defects the importers leave behind', async () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    container.json.materials![0].pbrMetallicRoughness!.metallicFactor = 1
    container.json.materials![0].emissiveFactor = [1, 1, 1]

    const result = await runPipeline(container, settingsFor(container))
    const material = readMaterials(result.container)[0]

    expect(material.metallicFactor).toBe(0)
    expect(material.roughnessFactor).toBe(0.9)
    expect(material.emissive).toEqual([0, 0, 0])
  })

  it('renames a non-Mixamo rig onto the game bone names', async () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const result = await runPipeline(container, keepingTextures(container))
    const names = new Set((result.container.json.nodes ?? []).map((node) => node.name))

    expect(names.has('LeftUpLeg')).toBe(true)
    expect(names.has('RightForeArm')).toBe(true)
    expect(names.has('UpperLeg_L')).toBe(false)
  })

  it('keeps only the clips it is told to', async () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const all = container.json.animations!.map((clip) => clip.name!)
    const result = await runPipeline(container, { ...keepingTextures(container), keepClips: [all[0], all[1]] })

    expect(result.stats.animations).toEqual([all[0], all[1]])
    expect(all.length).toBeGreaterThan(2)
  })

  it('does not re-encode a texture that is already within budget', async () => {
    const { container } = loadFixture('monsters/troll.glb')
    const result = await runPipeline(container, settingsFor(container))

    expect(result.textureChanges).toEqual([])
    expect(result.stats.images[0].width).toBe(1024)
  })
})

describe('specular repair', () => {
  // doc/assets/monsters.md names the Mixamo defect as "metallic=1 / specular 2배".
  // Only metallic was being fixed, so cyclop shipped with specularColorFactor
  // [2,2,2] and rendered pale and glaring.
  it('strips the boost the importer applied', async () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    container.json.materials![0].extensions = {
      KHR_materials_specular: { specularColorFactor: [2, 2, 2] },
    }
    container.json.extensionsUsed = [...(container.json.extensionsUsed ?? []), 'KHR_materials_specular']

    const result = await runPipeline(container, settingsFor(container))

    expect(readMaterials(result.container)[0].specular).toBeNull()
    expect(result.container.json.extensionsUsed ?? []).not.toContain('KHR_materials_specular')
  })

  it('keeps it when asked to', async () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    container.json.materials![0].extensions = { KHR_materials_specular: { specularFactor: 0.3 } }

    const settings = settingsFor(container)
    const result = await runPipeline(container, {
      ...settings,
      material: { ...settings.material, specularMode: 'keep' },
    })

    expect(readMaterials(result.container)[0].specular).toBeCloseTo(0.3, 6)
  })

  it('sets a value of its own, without the colour tint the importer used', async () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    container.json.materials![0].extensions = {
      KHR_materials_specular: { specularColorFactor: [2, 2, 2] },
    }

    const settings = settingsFor(container)
    const result = await runPipeline(container, {
      ...settings,
      material: { ...settings.material, specularMode: 'set', specularFactor: 0.42 },
    })

    const specular = result.container.json.materials![0].extensions
      ?.KHR_materials_specular as { specularFactor?: number; specularColorFactor?: number[] }
    expect(specular.specularFactor).toBeCloseTo(0.42, 6)
    expect(specular.specularColorFactor).toBeUndefined()
    expect(result.container.json.extensionsUsed).toContain('KHR_materials_specular')
  })

  it('adds the extension to a model that had none when a value is set', async () => {
    const { container } = loadFixture('monsters/troll.glb')
    expect(readMaterials(container)[0].specular).toBeNull()

    const settings = settingsFor(container)
    const result = await runPipeline(container, {
      ...settings,
      material: { ...settings.material, specularMode: 'set', specularFactor: 0.25 },
    })

    expect(readMaterials(result.container)[0].specular).toBeCloseTo(0.25, 6)
    expect(result.container.json.extensionsUsed).toContain('KHR_materials_specular')
  })

  it('counts a boosted specular as damage worth repairing', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    expect(readMaterials(container).some(needsRepair)).toBe(false)

    container.json.materials![0].extensions = {
      KHR_materials_specular: { specularColorFactor: [2, 2, 2] },
    }
    expect(readMaterials(container).some(needsRepair)).toBe(true)
  })

  // stone_golem ships specularFactor 0.3 on purpose; that is not a defect.
  it('leaves a deliberate low specular unflagged', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    container.json.materials![0].extensions = { KHR_materials_specular: { specularFactor: 0.3 } }
    expect(readMaterials(container).some(needsRepair)).toBe(false)
  })
})
