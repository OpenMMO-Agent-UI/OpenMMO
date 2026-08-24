/**
 * Guess which source joint plays each standard bone.
 *
 * Meshy and Tripo rig with their own naming, so an import that only stripped
 * `mixamorig:` would leave most of the skeleton unmapped. The guess is always
 * reviewable — this only decides what the mapping table opens with.
 */
import {
  BONE_PARENT,
  FINGER_ALIASES,
  LIMB_ALIASES,
  STANDARD_BONES,
  type StandardBone,
} from './skeleton'

export interface Joint {
  node: number
  name: string
  /** Node index of the parent joint, or -1. */
  parent: number
}

export type MatchKind = 'exact' | 'alias' | 'similar' | 'none'

export interface BoneGuess {
  standard: StandardBone
  node: number | null
  confidence: number
  how: MatchKind
}

export function normalize(name: string): string {
  return name
    .toLowerCase()
    .replace(/^mixamorig\d*:?/, '')
    .replace(/[^a-z0-9]/g, '')
}

const SIDE_PATTERNS: { re: RegExp; group: number; side: 'Left' | 'Right' }[] = [
  { re: /(^|[._\- ])(left)([._\- ]|$)/, group: 2, side: 'Left' },
  { re: /(^|[._\- ])(right)([._\- ]|$)/, group: 2, side: 'Right' },
  { re: /(^|[._\- ])(l)([._\- ]|$)/, group: 2, side: 'Left' },
  { re: /(^|[._\- ])(r)([._\- ]|$)/, group: 2, side: 'Right' },
  { re: /^(left)/, group: 1, side: 'Left' },
  { re: /^(right)/, group: 1, side: 'Right' },
  { re: /(left)$/, group: 1, side: 'Left' },
  { re: /(right)$/, group: 1, side: 'Right' },
]

export function splitSide(name: string): { side: 'Left' | 'Right' | null; stem: string } {
  const raw = name.toLowerCase().replace(/^mixamorig\d*:?/, '')
  for (const { re, group, side } of SIDE_PATTERNS) {
    const match = raw.match(re)
    if (!match) continue
    const token = match[group]
    const at = raw.indexOf(token, match.index ?? 0)
    return { side, stem: normalize(raw.slice(0, at) + raw.slice(at + token.length)) }
  }
  return { side: null, stem: normalize(raw) }
}

/** Best-effort translation of one source name into a standard bone name. */
export function canonicalize(name: string): StandardBone | null {
  const direct = STANDARD_BONES.find((bone) => normalize(bone) === normalize(name))
  if (direct) return direct

  const { side, stem } = splitSide(name)

  const finger = Object.keys(FINGER_ALIASES).find((key) => stem.includes(key))
  if (finger && side) {
    const joint = Number((stem.match(/(\d+)\s*$/)?.[1] ?? '1').slice(-2)) || 1
    const candidate = `${side}Hand${FINGER_ALIASES[finger]}${Math.min(joint, 4)}`
    return STANDARD_BONES.find((bone) => bone === candidate) ?? null
  }

  const limb = LIMB_ALIASES[stem] ?? LIMB_ALIASES[stem.replace(/\d+$/, '')]
  if (!limb) return null
  const candidate = STANDARD_BONES.find((bone) => bone === limb)
  if (candidate) return candidate
  return side ? (STANDARD_BONES.find((bone) => bone === `${side}${limb}`) ?? null) : null
}

function bigrams(value: string): Set<string> {
  const out = new Set<string>()
  for (let i = 0; i < value.length - 1; i++) out.add(value.slice(i, i + 2))
  return out
}

export function similarity(a: string, b: string): number {
  if (a === b) return 1
  const left = bigrams(a)
  const right = bigrams(b)
  if (left.size === 0 || right.size === 0) return 0
  let shared = 0
  for (const gram of left) if (right.has(gram)) shared++
  return (2 * shared) / (left.size + right.size)
}

const SIMILARITY_FLOOR = 0.62

function sideOf(bone: string): 'Left' | 'Right' | null {
  if (bone.startsWith('Left')) return 'Left'
  if (bone.startsWith('Right')) return 'Right'
  return null
}

export function guessBoneMapping(joints: Joint[]): BoneGuess[] {
  const scores: { standard: StandardBone; node: number; score: number; how: MatchKind }[] = []

  for (const joint of joints) {
    const normalized = normalize(joint.name)
    const canonical = canonicalize(joint.name)
    const jointSide = splitSide(joint.name).side

    for (const standard of STANDARD_BONES) {
      if (normalize(standard) === normalized) {
        scores.push({ standard, node: joint.node, score: 1, how: 'exact' })
        continue
      }
      if (canonical === standard) {
        scores.push({ standard, node: joint.node, score: 0.9, how: 'alias' })
        continue
      }
      // Never let a left bone stand in for a right one, however alike the names.
      const standardSide = sideOf(standard)
      if (standardSide && jointSide && standardSide !== jointSide) continue
      if (standardSide && !jointSide) continue

      const score = similarity(normalized, normalize(standard))
      if (score >= SIMILARITY_FLOOR) {
        scores.push({ standard, node: joint.node, score: score * 0.8, how: 'similar' })
      }
    }
  }

  scores.sort((a, b) => b.score - a.score)

  const byStandard = new Map<StandardBone, { node: number; score: number; how: MatchKind }>()
  const takenNodes = new Set<number>()
  for (const entry of scores) {
    if (byStandard.has(entry.standard) || takenNodes.has(entry.node)) continue
    byStandard.set(entry.standard, entry)
    takenNodes.add(entry.node)
  }

  resolveSpineChain(byStandard, joints)
  dropHierarchyViolations(byStandard, joints)

  return STANDARD_BONES.map((standard) => {
    const hit = byStandard.get(standard)
    return {
      standard,
      node: hit?.node ?? null,
      confidence: hit?.score ?? 0,
      how: hit?.how ?? 'none',
    }
  })
}

const SPINE_CHAIN: StandardBone[] = ['Spine', 'Spine1', 'Spine2']

/**
 * Spine bones are numbered inconsistently — `Spine/Spine01/Spine02`,
 * `spine_01/02/03`, `Spine/Chest/UpperChest` — and the numbers do not even
 * always run the same way: stone_golem is rigged Hips -> Spine02 -> Spine01 ->
 * Spine, so its bone named `Spine` is the topmost one. Position is the signal
 * that survives all of that, so when exactly as many joints sit between the
 * hips and the neck as the standard has, they take the slots in order.
 */
function resolveSpineChain(
  byStandard: Map<StandardBone, { node: number; score: number; how: MatchKind }>,
  joints: Joint[]
): void {
  const hips = byStandard.get('Hips')
  const top =
    byStandard.get('Neck') ??
    byStandard.get('LeftShoulder') ??
    byStandard.get('RightShoulder') ??
    byStandard.get('Head')
  if (!hips || !top) return

  const parentOf = new Map(joints.map((j) => [j.node, j.parent]))
  const chain: number[] = []
  let at = parentOf.get(top.node) ?? -1
  for (let guard = 0; at !== -1 && at !== hips.node && guard < 64; guard++) {
    chain.unshift(at)
    at = parentOf.get(at) ?? -1
  }
  if (at !== hips.node || chain.length === 0) return

  // A longer chain means extra joints this vocabulary has no slot for; leave
  // those rigs to the names rather than guessing which links to drop.
  if (chain.length > SPINE_CHAIN.length) return
  const picked = chain

  const wanted = new Map<StandardBone, number>()
  picked.forEach((node, i) => wanted.set(SPINE_CHAIN[i], node))

  // A joint that exactly matched some bone outside the spine is not up for
  // grabs — that would mean the chain walked through something else entirely.
  for (const [standard, hit] of byStandard) {
    if (hit.how !== 'exact') continue
    if ((SPINE_CHAIN as string[]).includes(standard)) continue
    if (picked.includes(hit.node)) return
  }

  for (const bone of SPINE_CHAIN) {
    const keep = byStandard.get(bone)
    const node = wanted.get(bone)
    if (node === undefined) {
      byStandard.delete(bone)
    } else if (keep?.node !== node) {
      byStandard.set(bone, { node, score: 0.85, how: 'alias' })
    }
  }
}

/**
 * A name can look right while sitting in the wrong place — a "hand" parented
 * under the pelvis is not the hand. Drop the weaker half of any pair whose
 * parent relationship the rig contradicts.
 */
function dropHierarchyViolations(
  byStandard: Map<StandardBone, { node: number; score: number; how: MatchKind }>,
  joints: Joint[]
): void {
  const parentOf = new Map(joints.map((j) => [j.node, j.parent]))

  const isDescendant = (node: number, ancestor: number): boolean => {
    let at = parentOf.get(node) ?? -1
    for (let guard = 0; at !== -1 && guard < 256; guard++) {
      if (at === ancestor) return true
      at = parentOf.get(at) ?? -1
    }
    return false
  }

  for (const [standard, hit] of [...byStandard]) {
    const parentStandard = BONE_PARENT[standard]
    if (!parentStandard) continue
    const parentHit = byStandard.get(parentStandard)
    if (!parentHit) continue
    if (isDescendant(hit.node, parentHit.node)) continue
    if (hit.how === 'exact' && parentHit.how === 'exact') continue
    const loser = hit.score <= parentHit.score ? standard : parentStandard
    byStandard.delete(loser)
  }
}

/** The rename map to hand to `renameNodes`, from a reviewed mapping. */
export function renameMapFrom(guesses: BoneGuess[]): Map<number, string> {
  const map = new Map<number, string>()
  for (const guess of guesses) if (guess.node !== null) map.set(guess.node, guess.standard)
  return map
}
