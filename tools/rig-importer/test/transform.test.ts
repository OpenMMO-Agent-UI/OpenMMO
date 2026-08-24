import { describe, expect, it } from 'vitest'
import {
  bakeUniformScale,
  boneReachAlongY,
  flattenRootScale,
  nodeWorldMatrices,
  originDelta,
  restPoseBounds,
  sanitizeNodeName,
  scaledNodes,
  sceneRootNodes,
  shiftRootNodes,
} from '../src/lib/gltf/transform'
import { readAccessor, type GlbContainer } from '../src/lib/gltf/container'
import { walkSpeedFor, runSpeedFor, weaponOffsetFor } from '../src/lib/game/rig'
import { loadFixture } from './fixtures'

function nodeNamed(c: GlbContainer, name: string): number {
  return (c.json.nodes ?? []).findIndex((n) => n.name === name)
}

// Heights and hip heights recorded in doc/assets/monsters.md for the models
// these were authored from — the measurement has a known right answer.
const SHIPPED = [
  { model: 'hobgoblin', height: 1.9, hips: 0.98 },
  { model: 'gnoll', height: 2.15, hips: 1.14 },
  { model: 'bugbear', height: 2.2, hips: 1.25 },
  { model: 'ogre', height: 2.4, hips: 1.17 },
  { model: 'troll', height: 2.7, hips: 1.51 },
]

describe('measurement', () => {
  it.each(SHIPPED)('reads $model at its authored height', ({ model, height, hips }) => {
    const { container } = loadFixture(`monsters/${model}.glb`)
    const bounds = restPoseBounds(container)

    expect(bounds.max[1] - bounds.min[1]).toBeCloseTo(height, 2)
    // Every shipped monster has its origin on the floor.
    expect(bounds.min[1]).toBeCloseTo(0, 3)
    expect(nodeWorldMatrices(container)[nodeNamed(container, 'Hips')][13]).toBeCloseTo(hips, 2)
  })

  // doc/assets/monsters.md: weaponOffset is 80% of the hand's reach along the bone.
  it.each([
    { model: 'hobgoblin', offset: 0.12 },
    { model: 'bugbear', offset: 0.2 },
    { model: 'ogre', offset: 0.24 },
  ])('derives $model weaponOffset within a centimetre', ({ model, offset }) => {
    const { container } = loadFixture(`monsters/${model}.glb`)
    const reach = boneReachAlongY(container, nodeNamed(container, 'RightHand'))
    expect(weaponOffsetFor(reach)).toBeCloseTo(offset, 2)
  })
})

describe('speed derivation', () => {
  // The values monsters.csv actually ships, from the hip heights above.
  it('reproduces the shipped walk speeds', () => {
    expect(walkSpeedFor(0.982)).toBeCloseTo(1.52, 2)
    expect(walkSpeedFor(1.173)).toBeCloseTo(1.81, 2)
    expect(walkSpeedFor(1.507)).toBeCloseTo(2.33, 2)
  })

  it('reproduces the shipped run speeds', () => {
    expect(runSpeedFor(1.173)).toBeCloseTo(5.08, 2)
    expect(runSpeedFor(1.248)).toBeCloseTo(5.41, 2)
    expect(runSpeedFor(1.507)).toBeCloseTo(6.53, 2)
  })
})

describe('scale bake', () => {
  it('scales height, hips and vertex data together', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const before = restPoseBounds(container)
    const hipsNode = nodeNamed(container, 'Hips')
    const hipsBefore = nodeWorldMatrices(container)[hipsNode][13]

    bakeUniformScale(container, 2)

    const after = restPoseBounds(container)
    expect(after.max[1] - after.min[1]).toBeCloseTo((before.max[1] - before.min[1]) * 2, 4)
    expect(nodeWorldMatrices(container)[hipsNode][13]).toBeCloseTo(hipsBefore * 2, 4)
  })

  it('keeps skinning consistent by scaling the inverse bind matrices', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const skin = container.json.skins![0]
    const before = readAccessor(container, skin.inverseBindMatrices!).values.slice(0, 16)

    bakeUniformScale(container, 3)

    const after = readAccessor(container, skin.inverseBindMatrices!).values.slice(0, 16)
    // Rotation block untouched, translation column tripled.
    for (let i = 0; i < 12; i++) expect(after[i]).toBeCloseTo(before[i], 5)
    for (let i = 12; i < 15; i++) expect(after[i]).toBeCloseTo(before[i] * 3, 4)
    expect(after[15]).toBeCloseTo(1, 6)
  })

  it('scales animation translation tracks so clips still reach', () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const animation = container.json.animations![0]
    const channel = animation.channels.find((ch) => ch.target.path === 'translation')!
    const output = animation.samplers[channel.sampler].output
    const before = readAccessor(container, output).values.slice(0, 9)

    bakeUniformScale(container, 0.5)

    const after = readAccessor(container, output).values.slice(0, 9)
    for (let i = 0; i < 9; i++) expect(after[i]).toBeCloseTo(before[i] * 0.5, 5)
  })

  it('leaves rotation tracks alone', () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const animation = container.json.animations![0]
    const channel = animation.channels.find((ch) => ch.target.path === 'rotation')!
    const output = animation.samplers[channel.sampler].output
    const before = Array.from(readAccessor(container, output).values.slice(0, 8))

    bakeUniformScale(container, 4)

    expect(Array.from(readAccessor(container, output).values.slice(0, 8))).toEqual(before)
  })

  it('rejects a nonsense factor', () => {
    const { container } = loadFixture('monsters/troll.glb')
    expect(() => bakeUniformScale(container, 0)).toThrow(/Bad scale/)
    expect(() => bakeUniformScale(container, NaN)).toThrow(/Bad scale/)
  })
})

describe('origin', () => {
  it.each(SHIPPED)('finds $model already floor-centred', ({ model }) => {
    const { container } = loadFixture(`monsters/${model}.glb`)
    for (const axis of originDelta(container)) expect(Math.abs(axis)).toBeLessThan(0.001)
  })

  it('puts a lifted, shoved model back on the floor', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    shiftRootNodes(container, [0.4, 1.3, -0.25])

    const lifted = restPoseBounds(container)
    expect(lifted.min[1]).toBeCloseTo(1.3, 3)
    expect((lifted.min[0] + lifted.max[0]) / 2).toBeCloseTo(0.4, 3)

    shiftRootNodes(container, originDelta(container))
    const bounds = restPoseBounds(container)
    expect(bounds.min[1]).toBeCloseTo(0, 4)
    expect((bounds.min[0] + bounds.max[0]) / 2).toBeCloseTo(0, 4)
    expect((bounds.min[2] + bounds.max[2]) / 2).toBeCloseTo(0, 4)
  })
})

describe('node names', () => {
  it('strips the mixamo prefix and the separators three.js reserves', () => {
    expect(sanitizeNodeName('mixamorig:LeftHand')).toBe('LeftHand')
    expect(sanitizeNodeName('mixamorig1:Hips')).toBe('Hips')
    expect(sanitizeNodeName('spine.001')).toBe('spine_001')
    expect(sanitizeNodeName('arm[L]')).toBe('arm_L_')
  })
})

describe('root scale', () => {
  /** Blender and Mixamo leave a scale on the armature node; stone_golem shipped
   *  with 0.0132 until it was reworked, and its bind box came out 3 cm wide. */
  function withArmatureScale(container: GlbContainer, factor: number): void {
    const root = sceneRootNodes(container)[0]
    container.json.nodes![root].scale = [factor, factor, factor]
  }

  // The invariant worth protecting, and it holds across the whole set — the one
  // model that used to break it was fixed upstream rather than tolerated.
  it.each(['monsters/ogre.glb', 'monsters/stone_golem.glb', 'monsters/kobold.glb', 'characters/knight.glb'])(
    'finds no node scale left on %s',
    (model) => {
      expect(scaledNodes(loadFixture(model).container)).toEqual([])
    }
  )

  it('spots one when a rig brings it in', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    withArmatureScale(container, 0.01317)
    expect(scaledNodes(container)).toHaveLength(1)
  })

  // What flattening must never do is change how the model looks. Bounds are
  // taken after the scale is injected, because injecting it is itself a resize.
  it('bakes it away without moving the model', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    withArmatureScale(container, 0.01317)
    const before = restPoseBounds(container)

    const factor = flattenRootScale(container)

    expect(factor).toBeCloseTo(0.01317, 5)
    expect(scaledNodes(container)).toEqual([])

    const after = restPoseBounds(container)
    expect(after.max[1] - after.min[1]).toBeCloseTo(before.max[1] - before.min[1], 6)
    expect(after.min[1]).toBeCloseTo(before.min[1], 6)
  })

  /**
   * A node scale leaves the hand's reach measured in a bone space that is not
   * the world's, so weaponOffset stops being metres. Flattening puts the two
   * back in step — the reach keeps its proportion to the model.
   */
  it('leaves weaponOffset meaning the same fraction of the model', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    const hand = nodeNamed(container, 'RightHand')
    const bounds = restPoseBounds(container)
    const ratio = boneReachAlongY(container, hand) / (bounds.max[1] - bounds.min[1])

    withArmatureScale(container, 0.01317)
    flattenRootScale(container)

    const after = restPoseBounds(container)
    expect(boneReachAlongY(container, hand) / (after.max[1] - after.min[1])).toBeCloseTo(ratio, 6)
  })

  it('leaves an already-flat rig exactly as it was', () => {
    const { container } = loadFixture('monsters/ogre.glb')
    const before = boneReachAlongY(container, nodeNamed(container, 'RightHand'))

    expect(flattenRootScale(container)).toBe(1)
    expect(boneReachAlongY(container, nodeNamed(container, 'RightHand'))).toBeCloseTo(before, 6)
  })
})
