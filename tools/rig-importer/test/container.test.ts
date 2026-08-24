import { describe, expect, it } from 'vitest'
import { buildGlb, parseGlb, readAccessor, repackBuffer, writeFloatAccessor } from '../src/lib/gltf/container'
import { loadFixture } from './fixtures'

describe('GLB container', () => {
  it('survives a parse/build/parse round trip', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const rebuilt = parseGlb(buildGlb(container))

    expect(rebuilt.json.nodes?.length).toBe(container.json.nodes?.length)
    expect(rebuilt.json.accessors?.length).toBe(container.json.accessors?.length)

    const before = readAccessor(container, 0).values
    const after = readAccessor(rebuilt, 0).values
    expect(Array.from(after.slice(0, 32))).toEqual(Array.from(before.slice(0, 32)))
  })

  it('keeps every accessor readable after a repack', () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const packed = repackBuffer(container)

    for (let i = 0; i < (container.json.accessors?.length ?? 0); i++) {
      if (container.json.accessors![i].sparse) continue
      const before = readAccessor(container, i)
      const after = readAccessor(packed, i)
      expect(after.count).toBe(before.count)
      expect(Array.from(after.values.slice(0, 12))).toEqual(Array.from(before.values.slice(0, 12)))
    }
  })

  it('swaps image bytes without disturbing the other views', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const imageView = container.json.images![0].bufferView!
    const replacement = new Uint8Array(1234).fill(7)

    const packed = repackBuffer(container, new Map([[imageView, replacement]]))
    expect(packed.json.bufferViews![imageView].byteLength).toBe(1234)

    const positions = container.json.meshes![0].primitives[0].attributes.POSITION
    expect(Array.from(readAccessor(packed, positions).values.slice(0, 9))).toEqual(
      Array.from(readAccessor(container, positions).values.slice(0, 9))
    )
  })

  it('rejects a non-GLB file', () => {
    expect(() => parseGlb(new Uint8Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]))).toThrow(/bad magic/)
  })

  it('refuses to write into a quantized accessor', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const indices = container.json.meshes![0].primitives[0].indices!
    expect(() => writeFloatAccessor(container, indices, [0])).toThrow(/not FLOAT/)
  })
})
