/**
 * A new monster's non-model columns. These are placeholders picked from the
 * shallow end of the existing table so the row spawns and can be fought the
 * moment it is written — balancing them is a separate job.
 */
export interface GameplayDefaults {
  name: string
  level: string
  guard: string
  health: string
  damageRoll: string
  behavior: string
  material: string
  attackRange: string
  chaseRange: string
  attackCooldown: string
  attackImpactDelay: string
  attackDamageTextDelay: string
  dungeonMinDepth: string
  dungeonMaxDepth: string
  dungeonWeight: string
  dungeonAggressive: string
  deathPlaysHit: string
  corpseGroundOffset: string
  weapon: string
  weaponDropChance: string
  weaponBone: string
  boss: string
  hitDebuff: string
}

export function gameplayDefaults(displayName: string): GameplayDefaults {
  return {
    name: displayName,
    level: '1',
    guard: '8',
    health: '',
    damageRoll: '',
    behavior: 'brave',
    material: 'flesh',
    attackRange: '2.0',
    chaseRange: '22.0',
    attackCooldown: '1800',
    attackImpactDelay: '500',
    attackDamageTextDelay: '500',
    dungeonMinDepth: '1',
    dungeonMaxDepth: '5',
    dungeonWeight: '2',
    dungeonAggressive: 'true',
    deathPlaysHit: 'false',
    corpseGroundOffset: '-0.05',
    weapon: '',
    weaponDropChance: '0.1',
    weaponBone: 'RightHand',
    boss: '',
    hitDebuff: '',
  }
}

export const BEHAVIORS = ['brave', 'timid'] as const
export const MATERIALS = ['flesh', 'leather', 'metal', 'stone', 'wood'] as const

/** Columns the tool derives from the model rather than asking about. */
export const MODEL_DERIVED_COLUMNS = [
  'model',
  'walkSpeed',
  'runSpeed',
  'weaponOffset',
  'sharedAnims',
  'scale',
] as const

/** Long enough for anything readable, short enough to be a filename and a key. */
export const MAX_ID_LENGTH = 48

export function isValidId(id: string): boolean {
  return id.length <= MAX_ID_LENGTH && /^[a-z][a-z0-9_]*$/.test(id)
}

/**
 * A usable id from a generator's filename.
 *
 * Meshy names its downloads things like
 * `Meshy_AI_Hyena_Warlord_0815114431_texture_obj.fbx`, and re-exporting through
 * it again stacks another round on: one real file was called
 * `Meshy_AI_The_One_Eyed_Colossus_Biped_Meshy_AI_Meshy_Merged_Animations.fbx`.
 * Taken literally that becomes a 77-character monster id, a GLB filename and a
 * CSV key. Strip the packaging and keep the name.
 */
const LEADING_NOISE = ['meshy_ai', 'meshy', 'tripo', 'mixamo', 'sketchfab', 'the', 'a', 'an']
const TRAILING_NOISE = [
  'texture', 'textures', 'obj', 'fbx', 'glb', 'gltf', 'zip',
  'rig', 'rigged', 'rigging', 'skinned', 'biped',
  'animation', 'animations', 'anim', 'anims', 'merged',
  'retopo', 'remesh', 'remeshed', 'lowpoly', 'low_poly',
  'meshy_ai', 'meshy', 'mixamo', 'tripo', 'ai', 'com',
]

export function suggestId(fileName: string): string {
  const base = fileName
    .replace(/\.[^.]+$/, '')
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '_')
    .replace(/^_+|_+$/g, '')

  let words = base.split('_').filter(Boolean)

  // Generator timestamps: a long run of digits carries no meaning.
  words = words.filter((word) => !/^\d{6,}$/.test(word))

  let changed = true
  while (changed && words.length > 1) {
    changed = false
    for (const noise of LEADING_NOISE) {
      const parts = noise.split('_')
      if (words.length > parts.length && parts.every((part, i) => words[i] === part)) {
        words = words.slice(parts.length)
        changed = true
      }
    }
    for (const noise of TRAILING_NOISE) {
      const parts = noise.split('_')
      const tail = words.slice(-parts.length)
      if (words.length > parts.length && parts.every((part, i) => tail[i] === part)) {
        words = words.slice(0, -parts.length)
        changed = true
      }
    }
  }

  let id = words.join('_') || base
  if (id.length > MAX_ID_LENGTH) {
    id = id.slice(0, MAX_ID_LENGTH).replace(/_[^_]*$/, '')
  }
  return /^[a-z]/.test(id) ? id : `m_${id}`.slice(0, MAX_ID_LENGTH)
}
