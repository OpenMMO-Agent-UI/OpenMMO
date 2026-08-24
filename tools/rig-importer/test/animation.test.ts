import { describe, expect, it } from 'vitest'
import { appendAnimation, sanitizeClipName, uniqueClipName, type ClipData } from '../src/lib/gltf/animation'
import { buildGlb, parseGlb, readAccessor } from '../src/lib/gltf/container'
import { bakeUniformScale, restPoseBounds } from '../src/lib/gltf/transform'
import { modelStats } from '../src/lib/gltf/measure'
import { loadFixture } from './fixtures'

function clip(name: string, bones: string[]): ClipData {
  return {
    name,
    duration: 1,
    tracks: bones.flatMap((bone) => [
      {
        bone,
        path: 'rotation' as const,
        times: new Float32Array([0, 0.5, 1]),
        values: new Float32Array([0, 0, 0, 1, 0, 0.1, 0, 0.99, 0, 0, 0, 1]),
        interpolation: 'LINEAR' as const,
      },
      {
        bone,
        path: 'translation' as const,
        times: new Float32Array([0, 1]),
        values: new Float32Array([0, 1, 0, 0, 2, 0]),
        interpolation: 'LINEAR' as const,
      },
    ]),
  }
}

function nodeResolver(container: ReturnType<typeof parseGlb>) {
  return (bone: string) => {
    const index = (container.json.nodes ?? []).findIndex((node) => node.name === bone)
    return index < 0 ? null : index
  }
}

describe('appendAnimation', () => {
  it('adds a clip the model can be reloaded with', () => {
    const { container, byteLength } = loadFixture('monsters/hobgoblin.glb')
    expect(modelStats(container, byteLength).animations).toEqual([])

    const report = appendAnimation(container, clip('swing', ['Hips', 'RightArm']), nodeResolver(container))
    expect(report.boundTracks).toBe(4)
    expect(report.unboundBones).toEqual([])

    const reloaded = parseGlb(buildGlb(container))
    expect(reloaded.json.animations?.map((a) => a.name)).toEqual(['swing'])
    expect(reloaded.json.animations![0].channels).toHaveLength(4)
  })

  it('writes keyframes back exactly', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    appendAnimation(container, clip('swing', ['Hips']), nodeResolver(container))

    const reloaded = parseGlb(buildGlb(container))
    const animation = reloaded.json.animations![0]
    const rotation = animation.channels.find((ch) => ch.target.path === 'rotation')!
    const sampler = animation.samplers[rotation.sampler]

    expect(Array.from(readAccessor(reloaded, sampler.input).values)).toEqual([0, 0.5, 1])
    expect(readAccessor(reloaded, sampler.output).count).toBe(3)
    expect(reloaded.json.accessors![sampler.input].min).toEqual([0])
    expect(reloaded.json.accessors![sampler.input].max).toEqual([1])
  })

  it('points each channel at the right node', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    appendAnimation(container, clip('swing', ['RightHand']), nodeResolver(container))

    const animation = container.json.animations![0]
    for (const channel of animation.channels) {
      expect(container.json.nodes![channel.target.node!].name).toBe('RightHand')
    }
  })

  it('drops tracks for bones the model has not got, and says which', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const report = appendAnimation(container, clip('swing', ['Hips', 'Tail', 'Wing_L']), nodeResolver(container))

    expect(report.boundTracks).toBe(2)
    expect(report.unboundBones).toEqual(['Tail', 'Wing_L'])
    expect(container.json.animations![0].channels).toHaveLength(2)
  })

  it('adds nothing at all when no track binds', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const report = appendAnimation(container, clip('swing', ['Tail']), nodeResolver(container))

    expect(report.boundTracks).toBe(0)
    expect(container.json.animations).toBeUndefined()
  })

  it('leaves the rest of the model readable', () => {
    const { container, byteLength } = loadFixture('monsters/hobgoblin.glb')
    const before = restPoseBounds(container)
    appendAnimation(container, clip('swing', ['Hips', 'Spine', 'RightArm']), nodeResolver(container))

    const reloaded = parseGlb(buildGlb(container))
    const after = restPoseBounds(reloaded)
    expect(after.max[1] - after.min[1]).toBeCloseTo(before.max[1] - before.min[1], 5)
    expect(modelStats(reloaded, byteLength).triangles).toBe(10594)
  })

  // The reason clips are merged before the scale bake rather than after.
  it('lets the scale bake reach a merged clip, so the hips do not drift', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    appendAnimation(container, clip('swing', ['Hips']), nodeResolver(container))

    bakeUniformScale(container, 3)

    const animation = container.json.animations![0]
    const translation = animation.channels.find((ch) => ch.target.path === 'translation')!
    const values = readAccessor(container, animation.samplers[translation.sampler].output).values
    expect(Array.from(values)).toEqual([0, 3, 0, 0, 6, 0])
  })

  it('appends alongside clips the model already had', () => {
    const { container, byteLength } = loadFixture('monsters/kobold.glb')
    const before = modelStats(container, byteLength).animations

    appendAnimation(container, clip('swing', ['Hips']), nodeResolver(container))

    const after = modelStats(container, byteLength).animations
    expect(after).toEqual([...before, 'swing'])
  })
})

describe('clip names', () => {
  it('strips what Mixamo and Blender prepend, and the separators three.js reserves', () => {
    expect(sanitizeClipName('mixamo.com')).toBe('mixamo_com')
    expect(sanitizeClipName('Armature|mixamo.com')).toBe('mixamo_com')
    expect(sanitizeClipName('mixamorig:Attack')).toBe('Attack')
    expect(sanitizeClipName('  ')).toBe('clip')
  })

  it('makes room for a second import of the same name', () => {
    expect(uniqueClipName('attack', [])).toBe('attack')
    expect(uniqueClipName('attack', ['attack'])).toBe('attack_2')
    expect(uniqueClipName('attack', ['attack', 'attack_2'])).toBe('attack_3')
  })
})
