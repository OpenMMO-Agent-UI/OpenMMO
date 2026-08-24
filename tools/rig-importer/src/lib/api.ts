/** Thin wrappers over the tool's own dev-server routes. */
import type { ApplyPlan, ApplyResult, DraftFile, DraftState } from './plan'

export interface RepoInfo {
  monsterColumns: string[]
  monsters: Record<string, string>[]
  monsterModels: string[]
  characterModels: string[]
  weapons: { id: string; name: string; model: string }[]
}

async function ok<T>(response: Response): Promise<T> {
  if (!response.ok) throw new Error((await response.text()) || response.statusText)
  return (await response.json()) as T
}

export function fetchRepo(): Promise<RepoInfo> {
  return fetch('/api/repo').then(ok<RepoInfo>)
}

export function listDrafts(): Promise<{ drafts: DraftState[] }> {
  return fetch('/api/draft').then(ok<{ drafts: DraftState[] }>)
}

export function fetchDraft(id: string): Promise<DraftState> {
  return fetch(`/api/draft?id=${encodeURIComponent(id)}`).then(ok<DraftState>)
}

export function saveDraft(state: DraftState): Promise<{ updatedAt: string }> {
  return fetch('/api/draft', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(state),
  }).then(ok<{ updatedAt: string }>)
}

export function removeDraft(id: string): Promise<unknown> {
  return fetch(`/api/draft?id=${encodeURIComponent(id)}`, { method: 'DELETE' }).then(ok)
}

/**
 * The Content-Type is not optional here. A binary PUT sent without one reaches
 * the SvelteKit dev server with an empty body — the request goes through, the
 * byte count is right on this side, and the handler sees zero bytes. It is not
 * size-dependent: a 1 KB body vanishes the same way a 5 MB one does.
 */
export function putDraftFile(id: string, name: DraftFile, bytes: Uint8Array): Promise<unknown> {
  return fetch(`/api/draft/file?id=${encodeURIComponent(id)}&name=${name}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/octet-stream' },
    body: bytes as BodyInit,
  }).then(ok)
}

export async function getDraftFile(id: string, name: DraftFile): Promise<Uint8Array | null> {
  const response = await fetch(`/api/draft/file?id=${encodeURIComponent(id)}&name=${name}`)
  if (response.status === 404) return null
  if (!response.ok) throw new Error(await response.text())
  return new Uint8Array(await response.arrayBuffer())
}

export function planApply(id: string): Promise<{ plan: ApplyPlan }> {
  return fetch('/api/apply', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id, dryRun: true }),
  }).then(ok<{ plan: ApplyPlan }>)
}

export function applyDraft(id: string, runGenerators: boolean): Promise<ApplyResult> {
  return fetch('/api/apply', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ id, runGenerators }),
  }).then(ok<ApplyResult>)
}

/** Load a GLB the game already ships, e.g. "monsters/ogre.glb". */
export async function fetchGameModel(modelPath: string): Promise<Uint8Array> {
  const response = await fetch(`/models/${modelPath}`)
  if (!response.ok) throw new Error(`Could not read /models/${modelPath}`)
  return new Uint8Array(await response.arrayBuffer())
}
