import { describe, expect, it } from 'vitest'
import { removeNodes, stripLightsAndCameras } from '../src/lib/gltf/prune'
import { buildGlb, parseGlb, type GlbContainer } from '../src/lib/gltf/container'
import { restPoseBounds } from '../src/lib/gltf/transform'
import { modelStats } from '../src/lib/gltf/measure'
import { loadFixture } from './fixtures'

/** A GLB exported out of Blender with the scene's lamp and camera still in it. */
function addSceneFurniture(c: GlbContainer): void {
  const json = c.json as typeof c.json & {
    extensions?: Record<string, unknown>
    cameras?: unknown[]
  }
  json.extensions = { KHR_lights_punctual: { lights: [{ type: 'point', intensity: 1000 }] } }
  json.extensionsUsed = [...(json.extensionsUsed ?? []), 'KHR_lights_punctual']
  json.cameras = [{ type: 'perspective', perspective: { yfov: 0.8, znear: 0.1 } }]

  json.nodes!.push({ name: 'Point', translation: [0, 3, 0], extensions: { KHR_lights_punctual: { light: 0 } } })
  json.nodes!.push({ name: 'Camera', translation: [4, 2, 4], camera: 0 })
  json.scenes![0].nodes = [...(json.scenes![0].nodes ?? []), json.nodes!.length - 2, json.nodes!.length - 1]
}

describe('stripLightsAndCameras', () => {
  it('finds nothing to do on the models the repo ships', () => {
    for (const model of ['monsters/ogre.glb', 'monsters/kobold.glb', 'characters/knight.glb']) {
      const { container } = loadFixture(model)
      const before = JSON.stringify(container.json)

      expect(stripLightsAndCameras(container)).toEqual({ lights: 0, cameras: 0, removedNodes: [] })
      expect(JSON.stringify(container.json)).toBe(before)
    }
  })

  it('removes a lamp and a camera, and the nodes that only held them', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    addSceneFurniture(container)

    const report = stripLightsAndCameras(container)

    expect(report).toEqual({ lights: 1, cameras: 1, removedNodes: ['Point', 'Camera'] })
    expect(container.json.extensionsUsed ?? []).not.toContain('KHR_lights_punctual')
    expect((container.json as { cameras?: unknown[] }).cameras).toBeUndefined()
    expect((container.json.nodes ?? []).some((n) => n.name === 'Point' || n.name === 'Camera')).toBe(false)
  })

  it('leaves the model itself untouched', () => {
    const { container, byteLength } = loadFixture('monsters/ogre.glb')
    const bounds = restPoseBounds(container)
    const stats = modelStats(container, byteLength)

    addSceneFurniture(container)
    stripLightsAndCameras(container)

    const reloaded = parseGlb(buildGlb(container))
    expect(modelStats(reloaded, byteLength).triangles).toBe(stats.triangles)
    expect(modelStats(reloaded, byteLength).joints).toBe(stats.joints)
    expect(restPoseBounds(reloaded).max[1]).toBeCloseTo(bounds.max[1], 6)
  })

  it('keeps a light node that something else hangs off', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    addSceneFurniture(container)
    const lightNode = container.json.nodes!.findIndex((n) => n.name === 'Point')
    container.json.nodes![lightNode].children = [0]

    const report = stripLightsAndCameras(container)

    expect(report.lights).toBe(1)
    expect(report.removedNodes).toEqual(['Camera'])
    const kept = container.json.nodes!.find((n) => n.name === 'Point')
    expect(kept).toBeDefined()
    expect(kept?.extensions).toBeUndefined()
  })

  it('never strips a light node that is also a joint', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    const joint = container.json.skins![0].joints[3]
    container.json.nodes![joint].extensions = { KHR_lights_punctual: { light: 0 } }
    ;(container.json as { extensions?: unknown }).extensions = {
      KHR_lights_punctual: { lights: [{ type: 'point' }] },
    }

    stripLightsAndCameras(container)

    expect(container.json.skins![0].joints[3]).toBe(joint)
    expect(container.json.nodes![joint].extensions).toBeUndefined()
  })
})

describe('removeNodes', () => {
  it('renumbers children, scene roots and skin joints', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    const jointNames = container.json.skins![0].joints.map((j) => container.json.nodes![j].name)

    // Two spare nodes at the front, so every surviving index shifts.
    container.json.nodes!.unshift({ name: 'spare_a' }, { name: 'spare_b' })
    for (const node of container.json.nodes!.slice(2)) {
      if (node.children) node.children = node.children.map((c) => c + 2)
    }
    container.json.scenes![0].nodes = container.json.scenes![0].nodes!.map((n) => n + 2)
    container.json.skins![0].joints = container.json.skins![0].joints.map((j) => j + 2)
    if (container.json.skins![0].skeleton !== undefined) container.json.skins![0].skeleton += 2
    for (const animation of container.json.animations ?? []) {
      for (const channel of animation.channels) {
        if (channel.target.node !== undefined) channel.target.node += 2
      }
    }

    removeNodes(container, new Set([0, 1]))

    expect(container.json.skins![0].joints.map((j) => container.json.nodes![j].name)).toEqual(jointNames)
    expect(container.json.nodes!.some((n) => n.name?.startsWith('spare'))).toBe(false)
    const reloaded = parseGlb(buildGlb(container))
    expect(reloaded.json.skins![0].joints).toHaveLength(jointNames.length)
  })

  it('refuses to remove a skin joint', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    const joint = container.json.skins![0].joints[0]
    expect(() => removeNodes(container, new Set([joint]))).toThrow(/skin joint/)
  })

  it('drops animation channels aimed at a node that is gone, and their samplers', () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const spare = container.json.nodes!.push({ name: 'spare' }) - 1
    const animation = container.json.animations![0]
    const before = animation.channels.length

    animation.samplers.push({ ...animation.samplers[0] })
    animation.channels.push({ sampler: animation.samplers.length - 1, target: { node: spare, path: 'translation' } })

    removeNodes(container, new Set([spare]))

    const after = container.json.animations![0]
    expect(after.channels).toHaveLength(before)
    expect(after.samplers).toHaveLength(new Set(after.channels.map((c) => c.sampler)).size)
    for (const channel of after.channels) expect(after.samplers[channel.sampler]).toBeDefined()
  })

  it('does nothing when asked to remove nothing', () => {
    const { container } = loadFixture('monsters/troll.glb')
    const before = JSON.stringify(container.json)
    removeNodes(container, new Set())
    expect(JSON.stringify(container.json)).toBe(before)
  })
})
