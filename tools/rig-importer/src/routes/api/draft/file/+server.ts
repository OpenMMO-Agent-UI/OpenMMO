import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types'
import type { DraftFile } from '$lib/plan'
import { loadFile, saveFile } from '$lib/server/drafts'

const ALLOWED: DraftFile[] = ['model.glb', 'original.glb', 'concept.png', 'source.bin']

const MIME: Record<DraftFile, string> = {
  'model.glb': 'model/gltf-binary',
  'original.glb': 'model/gltf-binary',
  'concept.png': 'image/png',
  'source.bin': 'application/octet-stream',
}

function params(url: URL): { id: string; name: DraftFile } | null {
  const id = url.searchParams.get('id')
  const name = url.searchParams.get('name') as DraftFile | null
  if (!id || !name || !ALLOWED.includes(name)) return null
  return { id, name }
}

export const GET: RequestHandler = async ({ url }) => {
  const target = params(url)
  if (!target) return new Response('id and name required', { status: 400 })

  const bytes = await loadFile(target.id, target.name)
  if (!bytes) return new Response('Not found', { status: 404 })

  return new Response(bytes as BodyInit, {
    headers: { 'Content-Type': MIME[target.name], 'Cache-Control': 'no-store' },
  })
}

export const PUT: RequestHandler = async ({ url, request }) => {
  const target = params(url)
  if (!target) return new Response('id and name required', { status: 400 })

  const bytes = new Uint8Array(await request.arrayBuffer())
  if (bytes.byteLength === 0) {
    // Most often this is a binary PUT sent without a Content-Type, which
    // arrives here with nothing in it however many bytes the caller passed.
    return new Response('Empty body — a binary PUT needs an explicit Content-Type', { status: 400 })
  }

  await saveFile(target.id, target.name, bytes)
  return json({ ok: true, bytes: bytes.byteLength })
}
