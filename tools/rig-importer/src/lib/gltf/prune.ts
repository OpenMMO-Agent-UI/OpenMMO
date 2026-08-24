/**
 * Stripping scene furniture a game model has no business carrying.
 *
 * Meshy and Mixamo hand back bare meshes, but a GLB exported straight out of
 * Blender or pulled off Sketchfab can bring the whole scene with it — lights
 * and a camera included. A light left in the file is parented to the model, so
 * it rides around the world lighting things wherever the monster walks.
 *
 * Removing a node means reindexing: children, scene roots, skin joints and
 * animation channel targets all address nodes by position in the array.
 */
import type { GlbContainer, GltfNode } from './container'

const LIGHTS_EXTENSION = 'KHR_lights_punctual'

export interface PruneReport {
  lights: number
  cameras: number
  /** Names of the now-purposeless nodes that were removed with them. */
  removedNodes: string[]
}

function nodeExtensions(node: GltfNode): Record<string, unknown> | undefined {
  return node.extensions as Record<string, unknown> | undefined
}

/** Nodes that exist only to hold a light or a camera. */
function isEmptyAfterStripping(node: GltfNode, isJoint: boolean): boolean {
  if (isJoint) return false
  if (node.mesh !== undefined || node.skin !== undefined) return false
  if (node.children && node.children.length > 0) return false
  const extensions = nodeExtensions(node)
  return extensions === undefined || Object.keys(extensions).length === 0
}

export function stripLightsAndCameras(c: GlbContainer): PruneReport {
  const json = c.json as typeof c.json & {
    extensions?: Record<string, { lights?: unknown[] }>
    cameras?: unknown[]
  }

  const lights = json.extensions?.[LIGHTS_EXTENSION]?.lights?.length ?? 0
  const cameras = json.cameras?.length ?? 0

  if (json.extensions) {
    delete json.extensions[LIGHTS_EXTENSION]
    if (Object.keys(json.extensions).length === 0) delete json.extensions
  }
  delete json.cameras

  const joints = new Set<number>()
  for (const skin of json.skins ?? []) {
    for (const joint of skin.joints) joints.add(joint)
    if (skin.skeleton !== undefined) joints.add(skin.skeleton)
  }

  const orphaned = new Set<number>()
  const removedNodes: string[] = []

  ;(json.nodes ?? []).forEach((node, index) => {
    let touched = false
    if (node.camera !== undefined) {
      delete node.camera
      touched = true
    }
    const extensions = nodeExtensions(node)
    if (extensions?.[LIGHTS_EXTENSION] !== undefined) {
      delete extensions[LIGHTS_EXTENSION]
      if (Object.keys(extensions).length === 0) delete node.extensions
      touched = true
    }
    if (touched && isEmptyAfterStripping(node, joints.has(index))) {
      orphaned.add(index)
      removedNodes.push(node.name ?? `node_${index}`)
    }
  })

  if (orphaned.size > 0) removeNodes(c, orphaned)

  if (lights > 0) {
    json.extensionsUsed = json.extensionsUsed?.filter((e) => e !== LIGHTS_EXTENSION)
    json.extensionsRequired = json.extensionsRequired?.filter((e) => e !== LIGHTS_EXTENSION)
    if (json.extensionsUsed?.length === 0) delete json.extensionsUsed
    if (json.extensionsRequired?.length === 0) delete json.extensionsRequired
  }

  return { lights, cameras, removedNodes }
}

/**
 * Drop nodes and renumber every reference to the survivors.
 *
 * Refuses to remove a skin joint: the inverse-bind matrices are positional, so
 * losing one silently reshapes the skeleton.
 */
export function removeNodes(c: GlbContainer, remove: Set<number>): void {
  const nodes = c.json.nodes ?? []
  if (remove.size === 0) return

  for (const skin of c.json.skins ?? []) {
    for (const joint of skin.joints) {
      if (remove.has(joint)) throw new Error(`Refusing to remove node ${joint}, which is a skin joint`)
    }
  }

  const remap = new Array<number>(nodes.length).fill(-1)
  let next = 0
  nodes.forEach((_, index) => {
    if (!remove.has(index)) remap[index] = next++
  })

  const survivors = nodes.filter((_, index) => !remove.has(index))
  for (const node of survivors) {
    if (!node.children) continue
    const children = node.children.map((child) => remap[child]).filter((child) => child >= 0)
    if (children.length > 0) node.children = children
    else delete node.children
  }
  c.json.nodes = survivors

  for (const scene of c.json.scenes ?? []) {
    if (!scene.nodes) continue
    scene.nodes = scene.nodes.map((node) => remap[node]).filter((node) => node >= 0)
  }

  for (const skin of c.json.skins ?? []) {
    skin.joints = skin.joints.map((joint) => remap[joint])
    if (skin.skeleton !== undefined) {
      const skeleton = remap[skin.skeleton]
      if (skeleton >= 0) skin.skeleton = skeleton
      else delete skin.skeleton
    }
  }

  for (const animation of c.json.animations ?? []) {
    const channels = animation.channels.filter(
      (channel) => channel.target.node === undefined || remap[channel.target.node] >= 0
    )
    for (const channel of channels) {
      if (channel.target.node !== undefined) channel.target.node = remap[channel.target.node]
    }
    // Samplers are addressed by position too, so rebuild the ones still used.
    const used = new Map<number, number>()
    const samplers = []
    for (const channel of channels) {
      if (!used.has(channel.sampler)) {
        used.set(channel.sampler, samplers.length)
        samplers.push(animation.samplers[channel.sampler])
      }
      channel.sampler = used.get(channel.sampler)!
    }
    animation.channels = channels
    animation.samplers = samplers
  }

  if (c.json.animations) {
    c.json.animations = c.json.animations.filter((animation) => animation.channels.length > 0)
    if (c.json.animations.length === 0) delete c.json.animations
  }
}
