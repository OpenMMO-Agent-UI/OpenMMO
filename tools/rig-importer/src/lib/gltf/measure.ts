/** Read-only facts about a model: what the validator and the data step quote. */
import { bufferViewBytes, readAccessor, type GlbContainer } from './container'
import { nodeWorldMatrices, restPoseBounds, type Bounds } from './transform'

export interface ImageInfo {
  index: number
  name: string
  mimeType: string
  width: number
  height: number
  byteLength: number
}

export interface ModelStats {
  triangles: number
  materials: number
  images: ImageInfo[]
  skins: number
  joints: number
  animations: string[]
  bounds: Bounds
  height: number
  byteLength: number
}

export function modelStats(c: GlbContainer, byteLength: number): ModelStats {
  let triangles = 0
  for (const mesh of c.json.meshes ?? []) {
    for (const prim of mesh.primitives) {
      if (prim.mode !== undefined && prim.mode !== 4) continue
      const count =
        prim.indices !== undefined
          ? c.json.accessors![prim.indices].count
          : (c.json.accessors![prim.attributes.POSITION]?.count ?? 0)
      triangles += Math.floor(count / 3)
    }
  }

  const bounds = restPoseBounds(c)
  return {
    triangles,
    materials: c.json.materials?.length ?? 0,
    images: imageInfos(c),
    skins: c.json.skins?.length ?? 0,
    joints: Math.max(0, ...(c.json.skins ?? []).map((s) => s.joints.length)),
    animations: (c.json.animations ?? []).map((a, i) => a.name ?? `clip_${i}`),
    bounds,
    height: bounds.max[1] - bounds.min[1],
    byteLength,
  }
}

export function imageInfos(c: GlbContainer): ImageInfo[] {
  return (c.json.images ?? []).map((image, index) => {
    const bytes = image.bufferView !== undefined ? bufferViewBytes(c, image.bufferView) : new Uint8Array(0)
    const size = imageSize(bytes)
    return {
      index,
      name: image.name ?? `image_${index}`,
      mimeType: image.mimeType ?? sniffMime(bytes) ?? 'application/octet-stream',
      width: size?.width ?? 0,
      height: size?.height ?? 0,
      byteLength: bytes.byteLength,
    }
  })
}

function sniffMime(bytes: Uint8Array): string | null {
  if (bytes[0] === 0x89 && bytes[1] === 0x50) return 'image/png'
  if (bytes[0] === 0xff && bytes[1] === 0xd8) return 'image/jpeg'
  if (bytes[8] === 0x57 && bytes[9] === 0x45) return 'image/webp'
  return null
}

/** Dimensions straight out of the container headers — no decode needed. */
export function imageSize(bytes: Uint8Array): { width: number; height: number } | null {
  if (bytes.byteLength < 16) return null
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)

  if (bytes[0] === 0x89 && bytes[1] === 0x50) {
    return { width: view.getUint32(16), height: view.getUint32(20) }
  }

  if (bytes[0] === 0xff && bytes[1] === 0xd8) {
    let at = 2
    while (at + 9 < bytes.byteLength) {
      if (bytes[at] !== 0xff) {
        at++
        continue
      }
      const marker = bytes[at + 1]
      const length = view.getUint16(at + 2)
      // SOF0..SOF15, skipping the DHT/DAC/RST/SOS markers that share the range.
      if (marker >= 0xc0 && marker <= 0xcf && marker !== 0xc4 && marker !== 0xc8 && marker !== 0xcc) {
        return { width: view.getUint16(at + 7), height: view.getUint16(at + 5) }
      }
      at += 2 + length
    }
    return null
  }

  if (bytes[8] === 0x57 && bytes[9] === 0x45) {
    const fourCC = String.fromCharCode(bytes[12], bytes[13], bytes[14], bytes[15])
    if (fourCC === 'VP8X') return { width: readUint24(view, 24) + 1, height: readUint24(view, 27) + 1 }
    if (fourCC === 'VP8 ') return { width: view.getUint16(26, true) & 0x3fff, height: view.getUint16(28, true) & 0x3fff }
  }
  return null
}

function readUint24(view: DataView, at: number): number {
  return view.getUint8(at) | (view.getUint8(at + 1) << 8) | (view.getUint8(at + 2) << 16)
}

/** Height of a joint above y=0, used to derive walk/run speed. */
export function jointHeight(c: GlbContainer, node: number): number {
  return nodeWorldMatrices(c)[node]?.[13] ?? 0
}

/** Node indices that any skin uses as a joint. */
export function jointNodes(c: GlbContainer): number[] {
  const joints = new Set<number>()
  for (const skin of c.json.skins ?? []) for (const joint of skin.joints) joints.add(joint)
  return [...joints].sort((a, b) => a - b)
}

/** Parent index per node, or -1 for a root. */
export function nodeParents(c: GlbContainer): number[] {
  const parents = (c.json.nodes ?? []).map(() => -1)
  ;(c.json.nodes ?? []).forEach((node, index) => {
    for (const child of node.children ?? []) parents[child] = index
  })
  return parents
}

/** True when any base-colour image carries meaningful transparency. */
export function usesAlpha(c: GlbContainer): boolean {
  return (c.json.materials ?? []).some((m) => m.alphaMode === 'BLEND' || m.alphaMode === 'MASK')
}

export function accessorIsFloat(c: GlbContainer, index: number | undefined): boolean {
  if (index === undefined) return true
  return c.json.accessors?.[index]?.componentType === 5126
}

export function totalVertices(c: GlbContainer): number {
  let count = 0
  for (const mesh of c.json.meshes ?? []) {
    for (const prim of mesh.primitives) {
      if (prim.attributes.POSITION !== undefined) count += readAccessor(c, prim.attributes.POSITION).count
    }
  }
  return count
}
