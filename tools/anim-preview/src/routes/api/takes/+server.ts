import { json } from '@sveltejs/kit'
import fs from 'node:fs/promises'
import path from 'node:path'
import {
  freeTakeName,
  pruneTakeFolder,
  resolveTakeFile,
  resolveTakeFolder,
  TAKES_DIR,
} from '$lib/server/library'

const MODEL_FILE = /\.(glb|gltf|fbx)$/i

/**
 * Copy picked files into takes/.
 *
 * The alternative — holding them as object URLs for the session — would make a
 * decision evaporate on reload, which is a trap when the job is eighteen
 * decisions long and takes more than one sitting. Everything downstream already
 * addresses a take by its path under takes/, so putting the file there keeps
 * one way of naming a take instead of two.
 */
export async function POST({ request }) {
  const form = await request.formData()
  const files = form.getAll('files').filter((entry): entry is File => entry instanceof File)
  if (files.length === 0) return json({ error: 'No files in the request.' }, { status: 400 })

  let dir: string
  try {
    dir = resolveTakeFolder(String(form.get('filedUnder') ?? ''))
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : String(error) }, { status: 400 })
  }
  await fs.mkdir(dir, { recursive: true })

  const written: string[] = []
  const skipped: string[] = []
  for (const file of files) {
    // Browsers send a bare name, but a folder drop carries a relative path.
    const name = path.basename(file.name)
    if (!MODEL_FILE.test(name)) {
      skipped.push(name)
      continue
    }
    const target = await freeTakeName(dir, name)
    await fs.writeFile(target, new Uint8Array(await file.arrayBuffer()))
    written.push(path.relative(TAKES_DIR, target).split(path.sep).join('/'))
  }

  return json({ written, skipped })
}

/** Remove a take. The file was copied in here, so this is the way back out. */
export async function DELETE({ request }) {
  const { path: relative } = (await request.json()) as { path?: string }

  let file: string
  try {
    file = resolveTakeFile(String(relative ?? ''))
  } catch (error) {
    return json({ error: error instanceof Error ? error.message : String(error) }, { status: 400 })
  }

  try {
    await fs.unlink(file)
  } catch {
    return json({ error: `${relative} is not there.` }, { status: 404 })
  }
  await pruneTakeFolder(path.dirname(file))
  return json({ removed: relative })
}
