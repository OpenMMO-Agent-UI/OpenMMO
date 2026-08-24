import { json } from '@sveltejs/kit'
import { listMotions, listPacks, listRigs, listTakes } from '$lib/server/library'

export async function GET() {
  const [rigs, motions, takes, packs] = await Promise.all([
    listRigs(),
    listMotions(),
    listTakes(),
    listPacks(),
  ])
  return json({ rigs, motions, takes, packs })
}
