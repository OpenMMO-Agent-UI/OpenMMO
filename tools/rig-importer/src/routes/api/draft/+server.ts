import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types'
import type { DraftState } from '$lib/plan'
import { deleteDraft, listDrafts, loadState, saveState } from '$lib/server/drafts'

export const GET: RequestHandler = async ({ url }) => {
  const id = url.searchParams.get('id')
  if (!id) return json({ drafts: await listDrafts() })

  const state = await loadState(id)
  if (!state) return new Response('No such draft', { status: 404 })
  return json(state)
}

export const POST: RequestHandler = async ({ request }) => {
  const state = (await request.json()) as DraftState
  state.updatedAt = new Date().toISOString()
  try {
    await saveState(state)
  } catch (error) {
    // A draft id the store will not take is the caller's problem to fix, and
    // saying so beats a 500 with the reason buried in the server log.
    return json({ error: error instanceof Error ? error.message : String(error) }, { status: 400 })
  }
  return json({ ok: true, updatedAt: state.updatedAt })
}

export const DELETE: RequestHandler = async ({ url }) => {
  const id = url.searchParams.get('id')
  if (!id) return new Response('id required', { status: 400 })
  await deleteDraft(id)
  return json({ ok: true })
}
