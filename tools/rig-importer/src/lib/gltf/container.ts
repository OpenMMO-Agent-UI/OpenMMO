/**
 * A GLB held open for editing: the parsed glTF JSON plus the single binary
 * chunk it points into.
 *
 * Every geometry, skeleton and material edit in this tool runs against the
 * container rather than against a three.js scene, so the bytes the preview
 * loads are literally the bytes that get written to the repo.
 */

export interface Gltf {
  asset: { version: string; generator?: string }
  scene?: number
  scenes?: { nodes?: number[]; name?: string }[]
  nodes?: GltfNode[]
  meshes?: GltfMesh[]
  skins?: GltfSkin[]
  accessors?: GltfAccessor[]
  bufferViews?: GltfBufferView[]
  buffers?: { byteLength: number; uri?: string }[]
  materials?: GltfMaterial[]
  textures?: { source?: number; sampler?: number }[]
  images?: GltfImage[]
  samplers?: unknown[]
  animations?: GltfAnimation[]
  extensionsUsed?: string[]
  extensionsRequired?: string[]
  [key: string]: unknown
}

export interface GltfNode {
  name?: string
  children?: number[]
  mesh?: number
  skin?: number
  translation?: [number, number, number]
  rotation?: [number, number, number, number]
  scale?: [number, number, number]
  matrix?: number[]
  [key: string]: unknown
}

export interface GltfPrimitive {
  attributes: Record<string, number>
  indices?: number
  material?: number
  mode?: number
  targets?: Record<string, number>[]
}

export interface GltfMesh {
  name?: string
  primitives: GltfPrimitive[]
  weights?: number[]
}

export interface GltfSkin {
  name?: string
  joints: number[]
  skeleton?: number
  inverseBindMatrices?: number
}

export interface GltfAccessor {
  bufferView?: number
  byteOffset?: number
  componentType: number
  normalized?: boolean
  count: number
  type: string
  min?: number[]
  max?: number[]
  sparse?: unknown
  name?: string
}

export interface GltfBufferView {
  buffer: number
  byteOffset?: number
  byteLength: number
  byteStride?: number
  target?: number
  name?: string
}

export interface GltfImage {
  uri?: string
  mimeType?: string
  bufferView?: number
  name?: string
}

export interface GltfTextureRef {
  index: number
  texCoord?: number
}

export interface GltfMaterial {
  name?: string
  pbrMetallicRoughness?: {
    baseColorFactor?: number[]
    baseColorTexture?: GltfTextureRef
    metallicFactor?: number
    roughnessFactor?: number
    metallicRoughnessTexture?: GltfTextureRef
  }
  normalTexture?: GltfTextureRef & { scale?: number }
  occlusionTexture?: GltfTextureRef & { strength?: number }
  emissiveTexture?: GltfTextureRef
  emissiveFactor?: number[]
  alphaMode?: string
  alphaCutoff?: number
  doubleSided?: boolean
  extensions?: Record<string, unknown>
  [key: string]: unknown
}

export interface GltfAnimationSampler {
  input: number
  output: number
  interpolation?: string
}

export interface GltfAnimationChannel {
  sampler: number
  target: { node?: number; path: string }
}

export interface GltfAnimation {
  name?: string
  samplers: GltfAnimationSampler[]
  channels: GltfAnimationChannel[]
}

export interface GlbContainer {
  json: Gltf
  bin: Uint8Array
}

const MAGIC = 0x46546c67 // 'glTF'
const CHUNK_JSON = 0x4e4f534a
const CHUNK_BIN = 0x004e4942

export const COMPONENT_BYTES: Record<number, number> = {
  5120: 1,
  5121: 1,
  5122: 2,
  5123: 2,
  5125: 4,
  5126: 4,
}

export const TYPE_COMPONENTS: Record<string, number> = {
  SCALAR: 1,
  VEC2: 2,
  VEC3: 3,
  VEC4: 4,
  MAT2: 4,
  MAT3: 9,
  MAT4: 16,
}

export function parseGlb(bytes: Uint8Array): GlbContainer {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength)
  if (bytes.byteLength < 12 || view.getUint32(0, true) !== MAGIC) {
    throw new Error('Not a GLB file (bad magic)')
  }
  if (view.getUint32(4, true) !== 2) {
    throw new Error('Only glTF 2.0 binary is supported')
  }

  let offset = 12
  let json: Gltf | null = null
  let bin = new Uint8Array(0)

  while (offset + 8 <= bytes.byteLength) {
    const length = view.getUint32(offset, true)
    const type = view.getUint32(offset + 4, true)
    const start = offset + 8
    const end = start + length
    if (end > bytes.byteLength) throw new Error('Truncated GLB chunk')

    if (type === CHUNK_JSON) {
      json = JSON.parse(new TextDecoder().decode(bytes.subarray(start, end))) as Gltf
    } else if (type === CHUNK_BIN) {
      bin = bytes.slice(start, end)
    }
    offset = end + ((4 - (end % 4)) % 4)
  }

  if (!json) throw new Error('GLB has no JSON chunk')
  if (json.buffers?.some((b) => b.uri)) {
    throw new Error('External or data-URI buffers are not supported — re-export as a self-contained GLB')
  }
  return { json, bin }
}

function pad4(n: number): number {
  return (4 - (n % 4)) % 4
}

export function buildGlb({ json, bin }: GlbContainer): Uint8Array {
  const next: Gltf = { ...json }
  next.buffers = bin.byteLength > 0 ? [{ byteLength: bin.byteLength }] : undefined
  if (!next.buffers) delete next.buffers

  const jsonBytes = new TextEncoder().encode(JSON.stringify(next))
  const jsonPad = pad4(jsonBytes.byteLength)
  const binPad = pad4(bin.byteLength)

  const jsonLength = jsonBytes.byteLength + jsonPad
  const binLength = bin.byteLength + binPad
  const total = 12 + 8 + jsonLength + (bin.byteLength > 0 ? 8 + binLength : 0)

  const out = new Uint8Array(total)
  const view = new DataView(out.buffer)
  view.setUint32(0, MAGIC, true)
  view.setUint32(4, 2, true)
  view.setUint32(8, total, true)

  view.setUint32(12, jsonLength, true)
  view.setUint32(16, CHUNK_JSON, true)
  out.set(jsonBytes, 20)
  out.fill(0x20, 20 + jsonBytes.byteLength, 20 + jsonLength) // JSON pads with spaces

  if (bin.byteLength > 0) {
    const binStart = 20 + jsonLength
    view.setUint32(binStart, binLength, true)
    view.setUint32(binStart + 4, CHUNK_BIN, true)
    out.set(bin, binStart + 8)
  }
  return out
}

export function cloneContainer(c: GlbContainer): GlbContainer {
  try {
    return { json: structuredClone(c.json), bin: c.bin.slice() }
  } catch (error) {
    // structuredClone refuses a Proxy, which is what Svelte wraps plain objects
    // in. A container must never be held in $state: besides this, the proxy
    // would sit in front of every one of the accessor reads a measurement does.
    throw new Error(
      `Could not copy the glTF json — it is probably wrapped in a reactive proxy. Hold containers outside $state. (${String(error)})`
    )
  }
}

export function bufferViewBytes(c: GlbContainer, index: number): Uint8Array {
  const bv = c.json.bufferViews?.[index]
  if (!bv) throw new Error(`No bufferView ${index}`)
  const start = bv.byteOffset ?? 0
  return c.bin.subarray(start, start + bv.byteLength)
}

export interface AccessorData {
  /** Tightly packed values, stride removed. */
  values: Float32Array | Int32Array | Uint32Array
  count: number
  components: number
  componentType: number
}

/** Read an accessor into a tightly packed array, undoing any byteStride. */
export function readAccessor(c: GlbContainer, index: number): AccessorData {
  const acc = c.json.accessors?.[index]
  if (!acc) throw new Error(`No accessor ${index}`)
  if (acc.sparse) throw new Error(`Accessor ${index} is sparse — not supported`)

  const components = TYPE_COMPONENTS[acc.type]
  const compBytes = COMPONENT_BYTES[acc.componentType]
  if (!components || !compBytes) throw new Error(`Accessor ${index} has an unknown type`)

  const total = acc.count * components
  const out =
    acc.componentType === 5126
      ? new Float32Array(total)
      : acc.componentType === 5125
        ? new Uint32Array(total)
        : new Int32Array(total)

  if (acc.bufferView === undefined) return { values: out, count: acc.count, components, componentType: acc.componentType }

  const bv = c.json.bufferViews![acc.bufferView]
  const base = (bv.byteOffset ?? 0) + (acc.byteOffset ?? 0)
  const stride = bv.byteStride ?? components * compBytes
  const view = new DataView(c.bin.buffer, c.bin.byteOffset, c.bin.byteLength)

  for (let i = 0; i < acc.count; i++) {
    for (let k = 0; k < components; k++) {
      const at = base + i * stride + k * compBytes
      out[i * components + k] = readComponent(view, at, acc.componentType)
    }
  }
  return { values: out, count: acc.count, components, componentType: acc.componentType }
}

function readComponent(view: DataView, at: number, componentType: number): number {
  switch (componentType) {
    case 5126:
      return view.getFloat32(at, true)
    case 5125:
      return view.getUint32(at, true)
    case 5123:
      return view.getUint16(at, true)
    case 5122:
      return view.getInt16(at, true)
    case 5121:
      return view.getUint8(at)
    default:
      return view.getInt8(at)
  }
}

/** Write packed float values back into a FLOAT accessor, in place. */
export function writeFloatAccessor(c: GlbContainer, index: number, values: ArrayLike<number>): void {
  const acc = c.json.accessors?.[index]
  if (!acc) throw new Error(`No accessor ${index}`)
  if (acc.componentType !== 5126) {
    throw new Error(`Accessor ${index} is not FLOAT — quantized meshes are not supported`)
  }
  if (acc.bufferView === undefined) throw new Error(`Accessor ${index} has no bufferView`)

  const components = TYPE_COMPONENTS[acc.type]
  const bv = c.json.bufferViews![acc.bufferView]
  const base = (bv.byteOffset ?? 0) + (acc.byteOffset ?? 0)
  const stride = bv.byteStride ?? components * 4
  const view = new DataView(c.bin.buffer, c.bin.byteOffset, c.bin.byteLength)

  for (let i = 0; i < acc.count; i++) {
    for (let k = 0; k < components; k++) {
      view.setFloat32(base + i * stride + k * 4, values[i * components + k], true)
    }
  }

  if (acc.min && acc.max) {
    const min = new Array(components).fill(Infinity)
    const max = new Array(components).fill(-Infinity)
    for (let i = 0; i < acc.count; i++) {
      for (let k = 0; k < components; k++) {
        const v = values[i * components + k]
        if (v < min[k]) min[k] = v
        if (v > max[k]) max[k] = v
      }
    }
    acc.min = min
    acc.max = max
  }
}

/**
 * Rebuild the binary chunk so every bufferView is contiguous and 4-byte
 * aligned, optionally swapping the bytes behind some of them. Used when an
 * image is re-encoded to a different length.
 */
export function repackBuffer(c: GlbContainer, replacements: Map<number, Uint8Array> = new Map()): GlbContainer {
  const views = c.json.bufferViews ?? []
  const chunks = views.map((bv, i) => replacements.get(i) ?? bufferViewBytes(c, i).slice())

  let total = 0
  for (const chunk of chunks) total += chunk.byteLength + pad4(chunk.byteLength)

  const bin = new Uint8Array(total)
  const nextViews: GltfBufferView[] = []
  let at = 0
  for (let i = 0; i < chunks.length; i++) {
    bin.set(chunks[i], at)
    nextViews.push({ ...views[i], byteOffset: at, byteLength: chunks[i].byteLength })
    at += chunks[i].byteLength + pad4(chunks[i].byteLength)
  }

  const json = structuredClone(c.json)
  json.bufferViews = nextViews
  json.buffers = [{ byteLength: total }]
  return { json, bin }
}

/**
 * Append several blobs in one pass, returning their bufferView indices.
 *
 * Merging a clip adds two accessors per track — a 60-bone rig is a few hundred
 * appends — and growing the binary chunk once per blob would copy the whole
 * buffer each time.
 */
export function appendBufferViews(c: GlbContainer, blobs: Uint8Array[]): number[] {
  if (blobs.length === 0) return []

  const starts: number[] = []
  let total = c.bin.byteLength + pad4(c.bin.byteLength)
  for (const blob of blobs) {
    starts.push(total)
    total += blob.byteLength + pad4(blob.byteLength)
  }

  const bin = new Uint8Array(total)
  bin.set(c.bin, 0)
  blobs.forEach((blob, i) => bin.set(blob, starts[i]))

  c.json.bufferViews ??= []
  const indices = blobs.map((blob, i) => {
    c.json.bufferViews!.push({ buffer: 0, byteOffset: starts[i], byteLength: blob.byteLength })
    return c.json.bufferViews!.length - 1
  })

  c.bin = bin
  c.json.buffers = [{ byteLength: total }]
  return indices
}

/** Append raw bytes as a new bufferView, returning its index. */
export function appendBufferView(c: GlbContainer, bytes: Uint8Array): number {
  const padding = pad4(c.bin.byteLength)
  const bin = new Uint8Array(c.bin.byteLength + padding + bytes.byteLength)
  bin.set(c.bin, 0)
  bin.set(bytes, c.bin.byteLength + padding)

  c.bin = bin
  c.json.bufferViews = c.json.bufferViews ?? []
  c.json.bufferViews.push({
    buffer: 0,
    byteOffset: c.bin.byteLength - bytes.byteLength,
    byteLength: bytes.byteLength,
  })
  c.json.buffers = [{ byteLength: bin.byteLength }]
  return c.json.bufferViews.length - 1
}
