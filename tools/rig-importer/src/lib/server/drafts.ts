/**
 * Work in progress, kept on disk.
 *
 * Skeleton mapping and material tuning are not quick, and a browser reload
 * should not cost the 30 MB FBX along with the decisions made about it. Drafts
 * live in a git-ignored directory beside the tool.
 */
import { existsSync } from 'node:fs'
import fs from 'node:fs/promises'
import path from 'node:path'
import type { DraftFile, DraftState } from '../plan'
import { TOOL_ROOT, within } from './repo'

export const DRAFTS_DIR = path.join(TOOL_ROOT, '.drafts')

const DRAFT_ID = /^[a-z0-9][a-z0-9_-]{0,63}$/

export function draftDir(id: string): string {
  if (!DRAFT_ID.test(id)) throw new Error(`Bad draft id: ${id}`)
  return within(DRAFTS_DIR, id)
}

export async function saveState(state: DraftState): Promise<void> {
  const dir = draftDir(state.id)
  await fs.mkdir(dir, { recursive: true })
  await fs.writeFile(path.join(dir, 'state.json'), JSON.stringify(state, null, 2), 'utf8')
}

export async function loadState(id: string): Promise<DraftState | null> {
  const file = path.join(draftDir(id), 'state.json')
  if (!existsSync(file)) return null
  return JSON.parse(await fs.readFile(file, 'utf8')) as DraftState
}

export async function saveFile(id: string, name: DraftFile, bytes: Uint8Array): Promise<void> {
  const dir = draftDir(id)
  await fs.mkdir(dir, { recursive: true })
  await fs.writeFile(within(dir, name), bytes)
}

export async function loadFile(id: string, name: DraftFile): Promise<Uint8Array | null> {
  const file = within(draftDir(id), name)
  if (!existsSync(file)) return null
  return new Uint8Array(await fs.readFile(file))
}

export async function listDrafts(): Promise<DraftState[]> {
  if (!existsSync(DRAFTS_DIR)) return []
  const entries = await fs.readdir(DRAFTS_DIR, { withFileTypes: true })
  const states = await Promise.all(
    entries.filter((entry) => entry.isDirectory()).map((entry) => loadState(entry.name).catch(() => null))
  )
  return states
    .filter((state): state is DraftState => state !== null)
    .sort((a, b) => b.updatedAt.localeCompare(a.updatedAt))
}

export async function deleteDraft(id: string): Promise<void> {
  await fs.rm(draftDir(id), { recursive: true, force: true })
}
