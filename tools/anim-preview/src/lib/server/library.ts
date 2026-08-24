/**
 * What there is to look at: the rigs, the motions, and the downloaded takes.
 *
 * All three are read off the repo rather than listed here, so a monster added
 * to monsters.csv or a clip added to a pack shows up without editing the tool.
 */
import { existsSync } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import { CHARACTER_ANIMATION_PACK_PATHS } from '$game/utils/modelPaths'

export const TOOL_ROOT = process.cwd()
export const REPO_ROOT = path.resolve(TOOL_ROOT, '../..')

if (!existsSync(path.join(REPO_ROOT, 'data-src/monsters.csv'))) {
  throw new Error(
    `anim-preview expects to run from tools/anim-preview — no data-src/monsters.csv under ${REPO_ROOT}`
  )
}

export const MODELS_DIR = path.join(REPO_ROOT, 'client/public/models')
export const ANIMATIONS_DIR = path.join(MODELS_DIR, 'animations')
export const TAKES_DIR = path.join(TOOL_ROOT, 'takes')

/**
 * Rigs the shared packs were never meant to drive. scp939 is a quadruped that
 * walks on all fours — a human walk cycle on it is not a defect to judge.
 */
const NON_HUMANOID = new Set(['scp939'])

export interface Rig {
  id: string
  name: string
  /** Path in the form the game refers to it, e.g. "monsters/ogre.glb". */
  model: string
  url: string
  kind: 'monster' | 'character'
}

function titleCase(fileStem: string): string {
  return fileStem
    .split('_')
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(' ')
}

/**
 * One entry per distinct GLB. The boss rows in monsters.csv point at the model
 * their base monster already uses — orc_boss is orc.glb at 1.4× — so listing
 * them again would put the same rig on the sheet three times.
 */
export async function listRigs(): Promise<Rig[]> {
  const csv = await fs.readFile(path.join(REPO_ROOT, 'data-src/monsters.csv'), 'utf8')
  const [headerLine, ...lines] = csv.trim().split('\n')
  const header = headerLine.split(',')
  const idAt = header.indexOf('id')
  const nameAt = header.indexOf('name')
  const modelAt = header.indexOf('model')

  const monsters: Rig[] = []
  const seen = new Set<string>()
  for (const line of lines) {
    const row = line.split(',')
    const id = row[idAt]
    const model = row[modelAt]
    if (!id || !model || NON_HUMANOID.has(id) || seen.has(model)) continue
    seen.add(model)
    monsters.push({ id, name: row[nameAt] || titleCase(id), model, url: `/models/${model}`, kind: 'monster' })
  }

  const charactersDir = path.join(MODELS_DIR, 'characters')
  const characters: Rig[] = (await fs.readdir(charactersDir))
    .filter((file) => file.endsWith('.glb'))
    .sort()
    .map((file) => {
      const id = file.replace(/\.glb$/, '')
      return {
        id,
        name: titleCase(id),
        model: `characters/${file}`,
        url: `/models/characters/${file}`,
        kind: 'character' as const,
      }
    })

  return [...monsters, ...characters]
}

export interface Motion {
  name: string
  /** The pack file the game currently gets this motion from. */
  pack: string
}

/**
 * The motions a pack has to supply, read from the packs themselves.
 *
 * Not from `AnimationName`: that enum carries attack1–attack4, which no pack
 * has (they come off base models), and leaves out combat_idle, claw1 and claw2,
 * which the packs do have and the monsters do use.
 */
/**
 * `locomotion` and `combat_melee` first: idle and walk are what a retarget is
 * judged on before anything else. Everything else on disk follows, sorted —
 * the other four shipped packs, and any candidate a write has produced.
 * Reading the directory rather than a fixed list means a freshly written
 * candidate pack shows up as its own group without touching this file.
 */
const PRIORITY_PACKS = ['locomotion.glb', 'combat_melee.glb']

export async function listMotions(): Promise<Motion[]> {
  const files = (await fs.readdir(ANIMATIONS_DIR)).filter((file) => file.endsWith('.glb')).sort()
  const ordered = [
    ...PRIORITY_PACKS.filter((file) => files.includes(file)),
    ...files.filter((file) => !PRIORITY_PACKS.includes(file)),
  ]

  const motions: Motion[] = []
  for (const file of ordered) {
    const pack = file.replace(/\.glb$/, '')
    for (const name of await readClipNames(path.join(ANIMATIONS_DIR, file))) {
      motions.push({ name, pack })
    }
  }
  return motions
}

/** Clip names out of a GLB's JSON chunk, without parsing the buffers. */
export async function readClipNames(file: string): Promise<string[]> {
  const bytes = await fs.readFile(file)
  const jsonLength = bytes.readUInt32LE(12)
  const json = JSON.parse(bytes.subarray(20, 20 + jsonLength).toString('utf8')) as {
    animations?: { name?: string }[]
  }
  return (json.animations ?? []).map((clip, index) => clip.name ?? `clip${index}`)
}

export interface Take {
  /** Path under takes/, also the id and the URL suffix. */
  path: string
  name: string
  /** Sub-folder it sits in, when that folder names a motion. Else null. */
  filedUnder: string | null
  bytes: number
}

/**
 * Everything under takes/, recursively.
 *
 * A file in a sub-folder named after a motion is filed under that motion; a
 * file at the root is unfiled and offered for every motion. Sorting downloads
 * into folders is a convenience, not a requirement — a Mixamo download can be
 * dropped straight in and auditioned against any motion.
 */
export async function listTakes(): Promise<Take[]> {
  if (!existsSync(TAKES_DIR)) return []
  const takes: Take[] = []

  async function walk(dir: string, prefix: string): Promise<void> {
    for (const entry of await fs.readdir(dir, { withFileTypes: true })) {
      if (entry.name.startsWith('.')) continue
      const rel = prefix ? `${prefix}/${entry.name}` : entry.name
      if (entry.isDirectory()) {
        await walk(path.join(dir, entry.name), rel)
        continue
      }
      if (!/\.(glb|gltf|fbx)$/i.test(entry.name)) continue
      const { size } = await fs.stat(path.join(dir, entry.name))
      takes.push({
        path: rel,
        name: entry.name.replace(/\.(glb|gltf|fbx)$/i, ''),
        filedUnder: prefix.split('/')[0] || null,
        bytes: size,
      })
    }
  }

  await walk(TAKES_DIR, '')
  return takes.sort((a, b) => a.path.localeCompare(b.path))
}

export interface Pack {
  name: string
  url: string
  clips: string[]
  /** One of the packs the game itself loads. */
  shipped: boolean
}

/**
 * The packs the game itself loads, taken from the game's own list rather than
 * restated here.
 *
 * A hand-kept copy is exactly how this went wrong once already: the tool held
 * two filenames while `CHARACTER_ANIMATION_PACK_PATHS` held five, so social,
 * offhand and fishing looked like candidate files somebody had written, and
 * the strip refused uploads for them.
 */
const SHIPPED_PACKS = new Set(
  Object.values(CHARACTER_ANIMATION_PACK_PATHS).map((url) => url.split('/').pop() as string)
)

/**
 * Every pack GLB on disk, so a pack written by the export step can be played
 * back against the whole sheet. That is the check the export is for: the take
 * looked right one at a time, and the question is whether it still does once
 * it has been through a round trip into a file.
 */
export async function listPacks(): Promise<Pack[]> {
  const files = (await fs.readdir(ANIMATIONS_DIR)).filter((file) => file.endsWith('.glb')).sort()
  return Promise.all(
    files.map(async (file) => ({
      name: file.replace(/\.glb$/, ''),
      url: `/models/animations/${file}`,
      clips: await readClipNames(path.join(ANIMATIONS_DIR, file)),
      shipped: SHIPPED_PACKS.has(file),
    }))
  )
}

/** A motion name, and nothing that could climb out of takes/. */
const TAKE_FOLDER = /^[A-Za-z0-9_-]+$/

/**
 * Where a newly added take goes. Empty files it at the root, where it is
 * offered for every motion.
 *
 * Both checks matter: the pattern rejects a name with a separator in it, and
 * the dirname check catches anything that still resolved somewhere else.
 */
export function resolveTakeFolder(filedUnder: string): string {
  const name = filedUnder.trim()
  if (!name) return TAKES_DIR
  if (!TAKE_FOLDER.test(name)) throw new Error(`${filedUnder} is not a motion name.`)
  const dir = path.join(TAKES_DIR, name)
  if (path.dirname(dir) !== TAKES_DIR) throw new Error('Refusing to write outside takes/.')
  return dir
}

/** Never overwrite a take already sitting there — add a counter instead. */
export async function freeTakeName(dir: string, fileName: string): Promise<string> {
  const extension = path.extname(fileName)
  const stem = fileName.slice(0, -extension.length)
  for (let n = 1; ; n += 1) {
    const candidate = path.join(dir, n === 1 ? fileName : `${stem}-${n}${extension}`)
    try {
      await fs.access(candidate)
    } catch {
      return candidate
    }
  }
}

/**
 * Resolve a take's path under takes/ for removal.
 *
 * Only a model file, and only inside takes/ — the pattern check rejects the
 * obvious climb and the resolved-prefix check catches anything that got past
 * it. README.md is not a model file, so the folder keeps its instructions.
 */
export function resolveTakeFile(relative: string): string {
  const rel = relative.trim()
  if (!rel || rel.includes('\0')) throw new Error('No take named.')
  if (!/\.(glb|gltf|fbx)$/i.test(rel)) throw new Error(`${relative} is not a take.`)
  const file = path.resolve(TAKES_DIR, rel)
  if (file !== TAKES_DIR && !file.startsWith(TAKES_DIR + path.sep)) {
    throw new Error('Refusing to touch anything outside takes/.')
  }
  return file
}

/**
 * Drop a motion folder once its last take leaves, so the strip is not offering
 * to file things into folders that no longer hold anything.
 */
export async function pruneTakeFolder(dir: string): Promise<void> {
  if (path.dirname(dir) !== TAKES_DIR) return
  try {
    if ((await fs.readdir(dir)).length === 0) await fs.rmdir(dir)
  } catch {
    // Already gone, or not empty. Either way there is nothing to prune.
  }
}
