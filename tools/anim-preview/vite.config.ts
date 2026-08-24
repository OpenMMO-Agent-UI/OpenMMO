import { sveltekit } from '@sveltejs/kit/vite'
import { defineConfig, type Plugin } from 'vite'
import { createReadStream, existsSync, statSync } from 'node:fs'
import { extname, join, normalize, resolve } from 'node:path'

const REPO_ROOT = resolve(import.meta.dirname, '../..')
const PUBLIC_DIR = resolve(REPO_ROOT, 'client/public')
const TAKES_DIR = resolve(import.meta.dirname, 'takes')

const MIME: Record<string, string> = {
  '.glb': 'model/gltf-binary',
  '.gltf': 'model/gltf+json',
  '.fbx': 'application/octet-stream',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
}

/** Serve a directory read-only at a URL prefix, refusing anything outside it. */
function serveDir(name: string, prefix: string, root: string): Plugin {
  return {
    name,
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const url = (req.url ?? '').split('?')[0]
        if (!url.startsWith(prefix)) return next()
        const rel = normalize(decodeURIComponent(url.slice(prefix.length))).replace(/^(\.\.[/\\])+/, '')
        const file = join(root, rel)
        if (!file.startsWith(root) || !existsSync(file) || !statSync(file).isFile()) return next()
        res.setHeader('Content-Type', MIME[extname(file).toLowerCase()] ?? 'application/octet-stream')
        res.setHeader('Content-Length', statSync(file).size)
        createReadStream(file).pipe(res)
      })
    },
  }
}

export default defineConfig({
  plugins: [
    // The rigs and the shipped packs, at the same absolute URLs the game uses,
    // so the retargeting here reads the exact files the game reads.
    serveDir('anim-preview:game-assets', '/models/', join(PUBLIC_DIR, 'models')),
    // The downloaded takes waiting to be auditioned.
    serveDir('anim-preview:takes', '/takes/', TAKES_DIR),
    sveltekit(),
  ],
  resolve: {
    alias: {
      // The one narrow door into the game's code: the sheet runs the game's own
      // retargeting, so what plays here is what the game plays.
      $game: resolve(REPO_ROOT, 'client/src/lib'),
      // That module imports `three`, which would otherwise resolve to the
      // client's own copy — two three.js instances in one page, and every
      // instanceof check across the boundary silently false.
      three: resolve(import.meta.dirname, 'node_modules/three'),
    },
    dedupe: ['three'],
  },
  server: { fs: { allow: [REPO_ROOT] } },
})
