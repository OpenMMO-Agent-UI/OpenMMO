import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types'
import { applyDraft, planApply } from '$lib/server/apply'
import { loadState } from '$lib/server/drafts'

export const POST: RequestHandler = async ({ request }) => {
  const { id, dryRun, runGenerators } = (await request.json()) as {
    id: string
    dryRun?: boolean
    runGenerators?: boolean
  }

  let draft
  try {
    draft = await loadState(id)
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : String(error) }, { status: 400 })
  }
  if (!draft) return new Response('No such draft', { status: 404 })

  try {
    if (dryRun) return json({ plan: await planApply(draft) })
    return json(await applyDraft(draft, runGenerators !== false))
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : String(error) }, { status: 400 })
  }
}
