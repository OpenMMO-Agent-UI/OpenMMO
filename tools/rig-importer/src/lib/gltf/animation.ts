/**
 * Merging an animation clip into the model's own GLB.
 *
 * Mixamo hands animations back as separate downloads, so a monster with its own
 * moves arrives as one rigged model plus a pile of clip files. This writes them
 * into the container as glTF animations, which means the pipeline then treats
 * them like any clip the model shipped with — the scale bake reaches their
 * translation tracks, so the hips do not drift when the model is resized.
 *
 * Deliberately three.js-free: the conversion from a THREE.AnimationClip happens
 * on the way in, so everything here is testable without a browser.
 */
import { appendBufferViews, type GlbContainer, type GltfAnimation } from './container'

export type TrackPath = 'translation' | 'rotation' | 'scale'
export type Interpolation = 'LINEAR' | 'STEP'

export interface ClipTrack {
  /** Bone name as the source file spells it. */
  bone: string
  path: TrackPath
  /** Keyframe times in seconds, ascending. */
  times: Float32Array
  /** Flattened values: 3 per key for translation/scale, 4 for rotation. */
  values: Float32Array
  interpolation: Interpolation
}

export interface ClipData {
  name: string
  duration: number
  tracks: ClipTrack[]
}

export interface MergeReport {
  name: string
  boundTracks: number
  /** Bones in the clip that the model does not have. */
  unboundBones: string[]
}

const COMPONENTS: Record<TrackPath, number> = { translation: 3, rotation: 4, scale: 3 }

/**
 * Write `clip` into the container. `resolveNode` turns a bone name from the
 * clip into a node index, or null when the model has no such bone — those
 * tracks are dropped and reported rather than silently bound to the wrong one.
 */
export function appendAnimation(
  c: GlbContainer,
  clip: ClipData,
  resolveNode: (bone: string) => number | null
): MergeReport {
  const unbound = new Set<string>()
  const usable: { track: ClipTrack; node: number }[] = []

  for (const track of clip.tracks) {
    if (track.times.length === 0) continue
    if (track.values.length !== track.times.length * COMPONENTS[track.path]) continue
    const node = resolveNode(track.bone)
    if (node === null) {
      unbound.add(track.bone)
      continue
    }
    usable.push({ track, node })
  }

  if (usable.length === 0) {
    return { name: clip.name, boundTracks: 0, unboundBones: [...unbound] }
  }

  // One append for every buffer this clip needs, times then values per track.
  const blobs = usable.flatMap(({ track }) => [bytesOf(track.times), bytesOf(track.values)])
  const views = appendBufferViews(c, blobs)

  c.json.accessors ??= []
  const samplers: GltfAnimation['samplers'] = []
  const channels: GltfAnimation['channels'] = []

  usable.forEach(({ track, node }, i) => {
    const times = track.times
    const input = pushAccessor(c, views[i * 2], times.length, 'SCALAR', [min(times)], [max(times)])
    const output = pushAccessor(c, views[i * 2 + 1], times.length, track.path === 'rotation' ? 'VEC4' : 'VEC3')

    samplers.push({ input, output, interpolation: track.interpolation })
    channels.push({ sampler: samplers.length - 1, target: { node, path: track.path } })
  })

  c.json.animations ??= []
  c.json.animations.push({ name: clip.name, samplers, channels })

  return { name: clip.name, boundTracks: usable.length, unboundBones: [...unbound] }
}

function pushAccessor(
  c: GlbContainer,
  bufferView: number,
  count: number,
  type: string,
  minimum?: number[],
  maximum?: number[]
): number {
  c.json.accessors!.push({
    bufferView,
    componentType: 5126,
    count,
    type,
    ...(minimum ? { min: minimum, max: maximum } : {}),
  })
  return c.json.accessors!.length - 1
}

function bytesOf(values: Float32Array): Uint8Array {
  const copy = new Float32Array(values)
  return new Uint8Array(copy.buffer, copy.byteOffset, copy.byteLength)
}

function min(values: Float32Array): number {
  let lowest = Infinity
  for (const value of values) if (value < lowest) lowest = value
  return lowest
}

function max(values: Float32Array): number {
  let highest = -Infinity
  for (const value of values) if (value > highest) highest = value
  return highest
}

/** Clip names reach three.js as strings it splits on "." and ":". */
export function sanitizeClipName(name: string): string {
  const cleaned = name
    .replace(/^Armature\|/i, '')
    .replace(/^mixamorig\d*:?/i, '')
    .replace(/[.:[\]]/g, '_')
    .trim()
  return cleaned || 'clip'
}

/** Make `name` unique against `taken`, the way a second import needs. */
export function uniqueClipName(name: string, taken: Iterable<string>): string {
  const used = new Set(taken)
  if (!used.has(name)) return name
  for (let n = 2; n < 1000; n++) {
    const candidate = `${name}_${n}`
    if (!used.has(candidate)) return candidate
  }
  return `${name}_${Date.now()}`
}
