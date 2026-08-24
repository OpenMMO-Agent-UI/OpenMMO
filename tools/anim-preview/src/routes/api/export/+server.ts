import { json } from '@sveltejs/kit'
import fs from 'node:fs/promises'
import path from 'node:path'
import { ANIMATIONS_DIR } from '$lib/server/library'

/**
 * Write a pack the browser built. It never overwrites a pack the game loads —
 * naming the output locomotion.glb would change what the running game plays on
 * the next reload, from a tool whose whole job is to decide whether the change
 * is any good. Renaming it over the shipped pack stays a deliberate step.
 */
const RESERVED = new Set(['locomotion.glb', 'combat_melee.glb', 'social.glb', 'offhand.glb', 'fishing.glb'])

export async function POST({ request }) {
  const form = await request.formData()
  const requested = String(form.get('fileName') ?? '').trim()
  const file = form.get('glb')

  if (!(file instanceof File)) return json({ error: 'No GLB in the request.' }, { status: 400 })
  if (!/^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(requested)) {
    return json({ error: 'Use letters, numbers, dot, dash and underscore.' }, { status: 400 })
  }

  const name = requested.endsWith('.glb') ? requested : `${requested}.glb`
  if (RESERVED.has(name)) {
    return json({ error: `${name} is a pack the game loads. Choose another name.` }, { status: 400 })
  }

  const target = path.join(ANIMATIONS_DIR, name)
  if (path.dirname(target) !== ANIMATIONS_DIR) {
    return json({ error: 'Refusing to write outside the animations folder.' }, { status: 400 })
  }

  const bytes = new Uint8Array(await file.arrayBuffer())
  await fs.writeFile(target, bytes)
  return json({ file: `client/public/models/animations/${name}`, url: `/models/animations/${name}`, bytes: bytes.length })
}
