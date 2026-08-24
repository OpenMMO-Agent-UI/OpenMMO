import { sveltekit } from '@sveltejs/kit/vite'
import { defineConfig, type Plugin } from 'vite'
import { createReadStream, existsSync, statSync } from 'node:fs'
import { extname, join, normalize, resolve } from 'node:path'

const REPO_ROOT = resolve(import.meta.dirname, '../..')
const PUBLIC_DIR = resolve(REPO_ROOT, 'client/public')

const MIME: Record<string, string> = {
  '.glb': 'model/gltf-binary',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
}

/**
 * The retarget preview calls the game's own loadSharedPackClipsForModel, which
 * fetches the animation packs from absolute `/models/...` URLs. Serve the game's
 * public dir at those paths so the preview loads the exact same files the game
 * does.
 */
function serveGameAssets(): Plugin {
  return {
    name: 'rig-importer:game-assets',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const url = (req.url ?? '').split('?')[0]
        if (!url.startsWith('/models/') && !url.startsWith('/items/')) {
          return next()
        }
        const rel = normalize(decodeURIComponent(url)).replace(/^(\.\.[/\\])+/, '')
        const file = join(PUBLIC_DIR, rel)
        if (!file.startsWith(PUBLIC_DIR) || !existsSync(file) || !statSync(file).isFile()) {
          return next()
        }
        res.setHeader('Content-Type', MIME[extname(file).toLowerCase()] ?? 'application/octet-stream')
        res.setHeader('Content-Length', statSync(file).size)
        createReadStream(file).pipe(res)
      })
    },
  }
}

export default defineConfig({
  plugins: [serveGameAssets(), sveltekit()],
  resolve: {
    alias: {
      // The one narrow door into the game's code: the preview runs the game's
      // own retargeting so what you see here is what the game plays.
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
