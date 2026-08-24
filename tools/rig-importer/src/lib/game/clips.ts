/**
 * Clip names the client binds by string (`doc/ANIMATION.md` §2). A monster on
 * `sharedAnims` plays these out of locomotion.glb / combat_melee.glb; one with
 * its own clips names whatever it shipped with.
 */
export const LOCOMOTION_CLIPS = ['idle1', 'idle2', 'idle3', 'idle4', 'idle5', 'walk', 'jog', 'run', 'jump'] as const
export const COMBAT_CLIPS = [
  'slash1', 'slash2', 'slash3', 'slash4', 'slash5',
  'attack1', 'attack2', 'attack3', 'attack4',
  'claw1', 'claw2',
  'combat_idle',
  'dying',
] as const

export const SHARED_PACK_CLIPS = [...LOCOMOTION_CLIPS, ...COMBAT_CLIPS]

/** monsters.csv columns that name an animation clip. */
export const ANIM_COLUMNS = [
  'animIdle',
  'animWalk',
  'animRun',
  'animAttack',
  'animAttackIdle',
  'animHit',
  'animDie',
  'animDead',
] as const

export type AnimColumn = (typeof ANIM_COLUMNS)[number]

/** `animAttack` may list alternatives with "|"; the client picks one per swing. */
export const MULTI_CLIP_COLUMNS: AnimColumn[] = ['animAttack']

export type AttackStyle = 'weapon' | 'claw'

/**
 * What the shared packs can fill in. There is no hit reaction in them, so
 * `animHit` stays empty — matching hobgoblin and gnoll.
 */
export function sharedAnimDefaults(style: AttackStyle): Record<AnimColumn, string> {
  return {
    animIdle: 'idle1',
    animWalk: 'walk',
    animRun: 'run',
    animAttack: style === 'claw' ? 'claw1|claw2' : 'slash1',
    animAttackIdle: 'combat_idle',
    animHit: '',
    animDie: 'dying',
    animDead: 'dying',
  }
}

/** Which pack a shared clip has to be pulled from, for the preview. */
export function packOf(clip: string): 'locomotion' | 'combat_melee' | null {
  if ((LOCOMOTION_CLIPS as readonly string[]).includes(clip)) return 'locomotion'
  if ((COMBAT_CLIPS as readonly string[]).includes(clip)) return 'combat_melee'
  return null
}

export function splitClipList(value: string): string[] {
  return value.split('|').map((clip) => clip.trim()).filter(Boolean)
}
