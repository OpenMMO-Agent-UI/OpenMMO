/**
 * Everything the tool is allowed to touch in the repo, in one place.
 *
 * Paths are resolved from this file rather than from cwd, and every write goes
 * through a helper that refuses to escape the directory it belongs to.
 */
import { execFile } from 'node:child_process'
import { existsSync } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import { promisify } from 'node:util'
import { docAssetsFileName, modelFolder, type ModelKind } from '../game/paths'

const run = promisify(execFile)

/** The tool runs from tools/rig-importer, the way glb-editor resolves its own. */
export const TOOL_ROOT = process.cwd()
export const REPO_ROOT = path.resolve(TOOL_ROOT, '../..')

if (!existsSync(path.join(REPO_ROOT, 'data-src/monsters.csv'))) {
  throw new Error(
    `rig-importer expects to run from tools/rig-importer — no data-src/monsters.csv under ${REPO_ROOT}`
  )
}
export const MODELS_DIR = path.join(REPO_ROOT, 'client/public/models')
export const DATA_SRC_DIR = path.join(REPO_ROOT, 'data-src')
export const DOC_ASSETS_DIR = path.join(REPO_ROOT, 'doc/assets')
export const DOC_IMAGES_DIR = path.join(REPO_ROOT, 'doc/images')
export const RAW_ASSETS_DIR = path.join(REPO_ROOT, 'assets')

export function modelDir(kind: ModelKind): string {
  return path.join(MODELS_DIR, modelFolder(kind))
}

export function docAssetsFile(kind: ModelKind): string {
  return path.join(DOC_ASSETS_DIR, docAssetsFileName(kind))
}

export function conceptImagePath(kind: ModelKind, id: string): string {
  return path.join(DOC_IMAGES_DIR, modelFolder(kind), `${id}-concept.png`)
}

/** Reject anything that would write outside the directory it claims to be in. */
export function within(base: string, candidate: string): string {
  const resolved = path.resolve(base, candidate)
  if (resolved !== base && !resolved.startsWith(base + path.sep)) {
    throw new Error(`Refusing to touch ${candidate} — outside ${path.relative(REPO_ROOT, base)}`)
  }
  return resolved
}

export function repoRelative(absolute: string): string {
  return path.relative(REPO_ROOT, absolute)
}

export async function readText(file: string): Promise<string> {
  return fs.readFile(file, 'utf8')
}

export async function writeText(file: string, content: string): Promise<void> {
  await fs.mkdir(path.dirname(file), { recursive: true })
  await fs.writeFile(file, content, 'utf8')
}

export async function writeBinary(file: string, bytes: Uint8Array): Promise<void> {
  await fs.mkdir(path.dirname(file), { recursive: true })
  await fs.writeFile(file, bytes)
}

export async function listModels(kind: ModelKind): Promise<string[]> {
  const dir = modelDir(kind)
  if (!existsSync(dir)) return []
  const entries = await fs.readdir(dir)
  return entries.filter((name) => name.endsWith('.glb')).sort()
}

export interface WeaponOption {
  id: string
  name: string
  model: string
}

/** main-hand weapons from items.csv, for the weapon-fit step. */
export async function listWeapons(): Promise<WeaponOption[]> {
  const text = await readText(path.join(DATA_SRC_DIR, 'items.csv'))
  const [headerLine, ...lines] = text.trim().split('\n')
  const header = headerLine.split(',')
  const at = (row: string[], column: string) => row[header.indexOf(column)] ?? ''

  return lines
    .map((line) => line.split(','))
    .filter((row) => at(row, 'category') === 'weapon' && at(row, 'worldModel'))
    .map((row) => ({ id: row[0], name: at(row, 'name'), model: at(row, 'worldModel') }))
    .sort((a, b) => a.name.localeCompare(b.name))
}

export interface CommandResult {
  command: string
  ok: boolean
  output: string
}

/** Run one of the repo's own generators. Failure is reported, never thrown. */
export async function runRepoCommand(command: string, args: string[], cwd = REPO_ROOT): Promise<CommandResult> {
  try {
    const { stdout, stderr } = await run(command, args, { cwd, timeout: 180_000, maxBuffer: 8 << 20 })
    return { command: `${command} ${args.join(' ')}`, ok: true, output: (stdout + stderr).trim() }
  } catch (error) {
    const shell = error as { stdout?: string; stderr?: string; message?: string }
    return {
      command: `${command} ${args.join(' ')}`,
      ok: false,
      output: (shell.stdout ?? '') + (shell.stderr ?? shell.message ?? ''),
    }
  }
}

export async function gitStatus(): Promise<string> {
  const result = await runRepoCommand('git', ['status', '--short'])
  return result.output
}
