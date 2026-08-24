/**
 * Geometry and skeleton edits, applied to the glTF container.
 *
 * Scale is baked into vertex, joint, inverse-bind and animation data rather
 * than left on a node, matching how the existing monsters were authored
 * (`doc/assets/monsters.md`: "mesh/armature data를 직접 스케일했다").
 */
import {
  readAccessor,
  writeFloatAccessor,
  type GlbContainer,
  type GltfNode,
} from './container'
import { fromTrs, multiply, transformPoint, type Mat4, IDENTITY } from './math'

export function localMatrix(node: GltfNode): Mat4 {
  if (node.matrix) return node.matrix.slice()
  return fromTrs(node.translation, node.rotation, node.scale)
}

/** World matrix per node index, walking the scene's root nodes. */
export function nodeWorldMatrices(c: GlbContainer): Mat4[] {
  const nodes = c.json.nodes ?? []
  const world: Mat4[] = nodes.map(() => IDENTITY.slice())
  const roots = sceneRootNodes(c)

  const walk = (index: number, parent: Mat4) => {
    const m = multiply(parent, localMatrix(nodes[index]))
    world[index] = m
    for (const child of nodes[index].children ?? []) walk(child, m)
  }
  for (const root of roots) walk(root, IDENTITY)
  return world
}

export function sceneRootNodes(c: GlbContainer): number[] {
  const sceneIndex = c.json.scene ?? 0
  const scene = c.json.scenes?.[sceneIndex]
  if (scene?.nodes?.length) return scene.nodes
  // Fall back to every node nothing else claims as a child.
  const claimed = new Set<number>()
  for (const node of c.json.nodes ?? []) for (const child of node.children ?? []) claimed.add(child)
  return (c.json.nodes ?? []).map((_, i) => i).filter((i) => !claimed.has(i))
}

function positionAccessors(c: GlbContainer): Set<number> {
  const found = new Set<number>()
  for (const mesh of c.json.meshes ?? []) {
    for (const prim of mesh.primitives) {
      if (prim.attributes.POSITION !== undefined) found.add(prim.attributes.POSITION)
      for (const target of prim.targets ?? []) {
        if (target.POSITION !== undefined) found.add(target.POSITION)
      }
    }
  }
  return found
}

/**
 * Scale the whole model by `s` about the origin. Uniform scale commutes with
 * rotation, so only translations, positions and inverse-bind translations move.
 */
export function bakeUniformScale(c: GlbContainer, s: number): void {
  if (!Number.isFinite(s) || s <= 0) throw new Error(`Bad scale factor: ${s}`)
  if (s === 1) return

  for (const node of c.json.nodes ?? []) {
    if (node.matrix) {
      node.matrix[12] *= s
      node.matrix[13] *= s
      node.matrix[14] *= s
    } else if (node.translation) {
      node.translation = [node.translation[0] * s, node.translation[1] * s, node.translation[2] * s]
    }
  }

  for (const index of positionAccessors(c)) {
    const { values } = readAccessor(c, index)
    const scaled = new Float32Array(values.length)
    for (let i = 0; i < values.length; i++) scaled[i] = values[i] * s
    writeFloatAccessor(c, index, scaled)
  }

  for (const skin of c.json.skins ?? []) {
    if (skin.inverseBindMatrices === undefined) continue
    const { values, count } = readAccessor(c, skin.inverseBindMatrices)
    const scaled = new Float32Array(values.length)
    scaled.set(values)
    // IBM' = S * IBM * S^-1 — for a uniform scale that is the translation column only.
    for (let i = 0; i < count; i++) {
      scaled[i * 16 + 12] *= s
      scaled[i * 16 + 13] *= s
      scaled[i * 16 + 14] *= s
    }
    writeFloatAccessor(c, skin.inverseBindMatrices, scaled)
  }

  for (const animation of c.json.animations ?? []) {
    const outputs = new Set<number>()
    for (const channel of animation.channels) {
      if (channel.target.path === 'translation') outputs.add(animation.samplers[channel.sampler].output)
    }
    for (const output of outputs) {
      const { values } = readAccessor(c, output)
      const scaled = new Float32Array(values.length)
      for (let i = 0; i < values.length; i++) scaled[i] = values[i] * s
      writeFloatAccessor(c, output, scaled)
    }
  }
}

/** Nodes still carrying a scale, which the shipped models never do. */
export function scaledNodes(c: GlbContainer): number[] {
  return (c.json.nodes ?? [])
    .map((node, index) => ({ node, index }))
    .filter(({ node }) => {
      const s = node.matrix ? matrixScale(node.matrix) : node.scale
      return s !== undefined && s.some((axis) => Math.abs(axis - 1) > 1e-4)
    })
    .map(({ index }) => index)
}

function matrixScale(m: number[]): [number, number, number] {
  return [
    Math.hypot(m[0], m[1], m[2]),
    Math.hypot(m[4], m[5], m[6]),
    Math.hypot(m[8], m[9], m[10]),
  ]
}

/**
 * Move a scale sitting on the scene root into the data underneath it.
 *
 * Blender and Mixamo hand back armatures scaled on the node — stone_golem's is
 * 0.0132 — and the shipped models never keep one: `doc/assets/monsters.md`
 * records scaling mesh and armature data directly instead. Leaving it costs
 * more than tidiness: anything parented to a bone, a weapon above all, inherits
 * that scale and renders 76x too small, and `weaponOffset` stops being metres.
 *
 * Returns the factor removed, or 1 if there was nothing to do. Only a uniform
 * scale shared by every root can be flattened this way; anything else is left
 * alone for the validator to report.
 */
export function flattenRootScale(c: GlbContainer): number {
  const roots = sceneRootNodes(c)
  if (roots.length === 0) return 1

  const factors = roots.map((index) => {
    const node = c.json.nodes![index]
    const scale = node.matrix ? matrixScale(node.matrix) : node.scale
    if (!scale) return 1
    const uniform = Math.abs(scale[0] - scale[1]) < 1e-6 && Math.abs(scale[1] - scale[2]) < 1e-6
    return uniform ? scale[0] : NaN
  })

  const factor = factors[0]
  if (!Number.isFinite(factor) || factor <= 0) return 1
  if (factors.some((entry) => Math.abs(entry - factor) > 1e-6)) return 1
  if (Math.abs(factor - 1) < 1e-6) return 1

  // Scaling every translation, position and inverse-bind by s reproduces
  // exactly what the root's scale was doing, so it can then be dropped.
  bakeUniformScale(c, factor)
  for (const index of roots) {
    const node = c.json.nodes![index]
    if (node.matrix) {
      for (const [col, axis] of [[0, 0], [4, 1], [8, 2]] as const) {
        void axis
        for (let row = 0; row < 3; row++) node.matrix[col + row] /= factor
      }
    } else {
      node.scale = [1, 1, 1]
    }
  }
  return factor
}

export interface Bounds {
  min: [number, number, number]
  max: [number, number, number]
}

/**
 * Rest-pose bounds.
 *
 * A skinned vertex lands at `jointWorld * IBM * v`, and in the bind pose that
 * product is the same matrix for every joint — the transform sitting above the
 * skeleton. Take it from one joint and apply it to the raw positions, so moving
 * the model's root actually moves the bounds. Unskinned meshes use their own
 * node's world matrix.
 */
export function restPoseBounds(c: GlbContainer): Bounds {
  const world = nodeWorldMatrices(c)
  const min: [number, number, number] = [Infinity, Infinity, Infinity]
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity]

  const absorb = (p: readonly [number, number, number]) => {
    for (let k = 0; k < 3; k++) {
      if (p[k] < min[k]) min[k] = p[k]
      if (p[k] > max[k]) max[k] = p[k]
    }
  }

  ;(c.json.nodes ?? []).forEach((node, index) => {
    if (node.mesh === undefined) return
    const placement = node.skin !== undefined ? skinPlacement(c, node.skin, world) : world[index]
    for (const prim of c.json.meshes![node.mesh].primitives) {
      const accessorIndex = prim.attributes.POSITION
      if (accessorIndex === undefined) continue
      const acc = c.json.accessors![accessorIndex]
      const corners: [number, number, number][] =
        acc.min && acc.max
          ? cornersOf(acc.min as [number, number, number], acc.max as [number, number, number])
          : boundsFromValues(readAccessor(c, accessorIndex).values)
      for (const corner of corners) absorb(transformPoint(placement, corner))
    }
  })

  if (!Number.isFinite(min[0])) throw new Error('Model has no mesh geometry')
  return { min, max }
}

/** Where a skin's bind pose sits in the scene: jointWorld * inverseBind. */
function skinPlacement(c: GlbContainer, skinIndex: number, world: Mat4[]): Mat4 {
  const skin = c.json.skins![skinIndex]
  if (skin.inverseBindMatrices === undefined || skin.joints.length === 0) return IDENTITY.slice()
  const ibm = readAccessor(c, skin.inverseBindMatrices).values
  return multiply(world[skin.joints[0]], Array.from(ibm.slice(0, 16)))
}

function cornersOf(min: [number, number, number], max: [number, number, number]) {
  const out: [number, number, number][] = []
  for (const x of [min[0], max[0]]) for (const y of [min[1], max[1]]) for (const z of [min[2], max[2]]) out.push([x, y, z])
  return out
}

function boundsFromValues(values: ArrayLike<number>): [number, number, number][] {
  const min: [number, number, number] = [Infinity, Infinity, Infinity]
  const max: [number, number, number] = [-Infinity, -Infinity, -Infinity]
  for (let i = 0; i < values.length; i += 3) {
    for (let k = 0; k < 3; k++) {
      if (values[i + k] < min[k]) min[k] = values[i + k]
      if (values[i + k] > max[k]) max[k] = values[i + k]
    }
  }
  return cornersOf(min, max)
}

/** Move every scene root so the model sits on y=0, centred on `centerXZ`. */
export function shiftRootNodes(c: GlbContainer, delta: readonly [number, number, number]): void {
  for (const index of sceneRootNodes(c)) {
    const node = c.json.nodes![index]
    if (node.matrix) {
      node.matrix[12] += delta[0]
      node.matrix[13] += delta[1]
      node.matrix[14] += delta[2]
    } else {
      const t = node.translation ?? [0, 0, 0]
      node.translation = [t[0] + delta[0], t[1] + delta[1], t[2] + delta[2]]
    }
  }
}

/**
 * Origin at floor centre: y=0 on the lowest vertex, x/z on the bounding-box
 * centre. That is the convention every shipped monster follows — measured, all
 * five land on 0 horizontally, while their hips sit up to 8 cm off in z.
 */
export function originDelta(c: GlbContainer): [number, number, number] {
  const bounds = restPoseBounds(c)
  return [-(bounds.min[0] + bounds.max[0]) / 2, -bounds.min[1], -(bounds.min[2] + bounds.max[2]) / 2]
}

/**
 * Node names reach three.js as PropertyBinding paths, where "." and ":" are
 * separators — a bone called `mixamorig:Hips` silently loses its animation.
 */
export function sanitizeNodeName(name: string): string {
  return name.replace(/^mixamorig\d*:?/i, '').replace(/[.:[\]/]/g, '_').trim()
}

export function renameNodes(c: GlbContainer, names: Map<number, string>): void {
  const used = new Set<string>()
  ;(c.json.nodes ?? []).forEach((node, index) => {
    const wanted = sanitizeNodeName(names.get(index) ?? node.name ?? `node_${index}`)
    let unique = wanted || `node_${index}`
    let n = 2
    while (used.has(unique)) unique = `${wanted}_${n++}`
    used.add(unique)
    node.name = unique
  })
}

export function renameAnimations(c: GlbContainer, names: Map<number, string>): void {
  ;(c.json.animations ?? []).forEach((animation, index) => {
    const wanted = names.get(index)
    if (wanted) animation.name = wanted
  })
}

export function keepAnimations(c: GlbContainer, keep: Set<string>): void {
  if (!c.json.animations) return
  c.json.animations = c.json.animations.filter((a) => keep.has(a.name ?? ''))
  if (c.json.animations.length === 0) delete c.json.animations
}

/**
 * How far the vertices skinned to `joint` reach along the bone's local +Y.
 * `doc/assets/monsters.md` sets weaponOffset to 80% of this — the hand bone
 * sits at the wrist, so a weapon parented to it needs pushing to the fingers.
 *
 * A vertex counts once the joint owns a majority of it, the same rule the
 * client's sole detection uses. Measured that way this reproduces the shipped
 * offsets: hobgoblin 0.118 (0.12), bugbear 0.197 (0.20), ogre 0.240 (0.24).
 */
export function boneReachAlongY(c: GlbContainer, joint: number, weightThreshold = 0.5): number {
  const world = nodeWorldMatrices(c)
  let reach = 0

  ;(c.json.nodes ?? []).forEach((node, index) => {
    if (node.mesh === undefined || node.skin === undefined) return
    const skin = c.json.skins![node.skin]
    const jointSlot = skin.joints.indexOf(joint)
    if (jointSlot < 0) return

    const ibm = skin.inverseBindMatrices !== undefined ? readAccessor(c, skin.inverseBindMatrices).values : null
    const boneSpace: Mat4 = ibm
      ? Array.from(ibm.slice(jointSlot * 16, jointSlot * 16 + 16))
      : invertRigid(world[index])

    for (const prim of c.json.meshes![node.mesh].primitives) {
      const pos = prim.attributes.POSITION
      const joints = prim.attributes.JOINTS_0
      const weights = prim.attributes.WEIGHTS_0
      if (pos === undefined || joints === undefined || weights === undefined) continue

      const p = readAccessor(c, pos).values
      const j = readAccessor(c, joints).values
      const w = readAccessor(c, weights).values

      for (let v = 0; v < p.length / 3; v++) {
        let weight = 0
        for (let k = 0; k < 4; k++) if (j[v * 4 + k] === jointSlot) weight += w[v * 4 + k]
        if (weight < weightThreshold) continue
        const local = transformPoint(boneSpace, [p[v * 3], p[v * 3 + 1], p[v * 3 + 2]])
        if (local[1] > reach) reach = local[1]
      }
    }
  })

  return reach
}

function invertRigid(m: Mat4): Mat4 {
  const r: Mat4 = [
    m[0], m[4], m[8], 0,
    m[1], m[5], m[9], 0,
    m[2], m[6], m[10], 0,
    0, 0, 0, 1,
  ]
  const t = transformPoint(r, [m[12], m[13], m[14]])
  r[12] = -t[0]
  r[13] = -t[1]
  r[14] = -t[2]
  return r
}
