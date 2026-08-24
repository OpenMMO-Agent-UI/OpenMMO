/**
 * What plays for each (pack, motion) slot, and what is on screen while
 * judging it.
 *
 * Every slot always has something to play: that pack's own clip, until an
 * override says otherwise. `overrides` is the whole point — a take chosen in
 * place of the pack default, per slot. Everything else here is apparatus.
 */
import type { Motion, Pack, Rig, Take } from './types'

export interface RigStatus {
  loaded: boolean
  boneCount: number
  hipsHeight: number
  /** Core bones the rig lacks. Non-empty means the packs cannot drive it. */
  missing: string[]
  /** Of those, the ones it has under another spelling. */
  nearMisses: { want: string; have: string }[]
  error: string | null
}

export const session = $state({
  rigs: [] as Rig[],
  motions: [] as Motion[],
  takes: [] as Take[],
  packs: [] as Pack[],
  loading: true,
  error: '',

  /** The motion on screen right now. */
  motion: '',
  /**
   * Which pack group that motion is being viewed from. Motion names are not
   * unique across packs — `walk` exists in every pack that has one — and
   * `locomotion` and `locomotion2` are meant to be judged independently, so
   * this is not just for picking the right rung: it is half of the key that
   * decides which take overrides which pack's `walk`.
   */
  pack: '',
  /** overrideKey(pack, motion) -> take path, where a take has replaced that
   *  one pack's clip for that one motion. Deciding `locomotion`'s `walk`
   *  must not touch `locomotion2`'s `walk` — they are different candidates
   *  being judged separately, even though they share a name. */
  overrides: {} as Record<string, string>,

  shown: {} as Record<string, boolean>,
  status: {} as Record<string, RigStatus>,

  /** Rigs still being retargeted for the current motion. */
  working: new Set<string>(),
  takeProblem: '' as string,
})

export function motionsByPack(): { pack: string; motions: Motion[] }[] {
  const groups = new Map<string, Motion[]>()
  for (const motion of session.motions) {
    const list = groups.get(motion.pack) ?? []
    list.push(motion)
    groups.set(motion.pack, list)
  }
  return [...groups].map(([pack, motions]) => ({ pack, motions }))
}

/**
 * The takes offered for a motion: the ones filed in a folder of that name,
 * then everything sitting unfiled at the root of takes/.
 */
export function takesFor(motion: string): Take[] {
  const filed = session.takes.filter((take) => take.filedUnder === motion)
  const unfiled = session.takes.filter((take) => take.filedUnder === null)
  return [...filed, ...unfiled]
}

/** The pack backing the motion currently on screen — `session.pack`, resolved. */
export function currentPack(): Pack | undefined {
  return session.packs.find((entry) => entry.name === session.pack)
}

/** The `overrides` key for one pack's one motion. Centralised so every
 *  read and write agrees on the format — a hand-rolled template string
 *  drifting out of sync here is exactly how the old by-name-only bug hid. */
export function overrideKey(pack: string, motion: string): string {
  return `${pack}::${motion}`
}

/** Undo `overrideKey` for display — "walk" in the "locomotion" group. */
export function splitOverrideKey(key: string): { pack: string; motion: string } {
  const at = key.indexOf('::')
  return { pack: key.slice(0, at), motion: key.slice(at + 2) }
}

/** The override for the (pack, motion) currently on screen, if any. */
export function currentOverride(): string | undefined {
  return session.overrides[overrideKey(session.pack, session.motion)]
}

/** How many (pack, motion) slots have an override — every group's rungs
 *  count on their own, since `locomotion`'s `walk` and `locomotion2`'s `walk`
 *  are judged independently. */
export function overrideCount(): number {
  return Object.keys(session.overrides).length
}

/** Rigs the packs cannot drive — hidden, with the bone they lack named. */
export function isIncompatible(rigId: string): boolean {
  const status = session.status[rigId]
  return !!status?.loaded && status.missing.length > 0
}

export function visibleRigs(): Rig[] {
  return session.rigs.filter((rig) => session.shown[rig.id])
}

/** Rigs off the sheet right now — hidden by hand, or by their own bones. */
export function hiddenRigs(): Rig[] {
  return session.rigs.filter((rig) => !session.shown[rig.id])
}

/** The (pack, motion) slots whose override points at this take. */
export function slotsOverriding(takePath: string): { pack: string; motion: string }[] {
  return Object.keys(session.overrides)
    .filter((key) => session.overrides[key] === takePath)
    .map(splitOverrideKey)
}

/**
 * What the export should write for one pack: every motion it has, each
 * pointing at its take or at nothing.
 *
 * A motion with no take still belongs in the list. Sending only the replaced
 * ones produced a file missing every motion left alone — a "locomotion" pack
 * with no `jump` in it, which any rig asking for `jump` would come up empty on.
 */
export function packDecisions(pack: string): { motion: string; takePath: string | null }[] {
  return session.motions
    .filter((motion) => motion.pack === pack)
    .map((motion) => ({
      motion: motion.name,
      takePath: session.overrides[overrideKey(pack, motion.name)] ?? null,
    }))
}
