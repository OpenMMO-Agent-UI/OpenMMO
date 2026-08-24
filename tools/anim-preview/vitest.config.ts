import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'
import { resolve } from 'node:path'

const REPO_ROOT = resolve(import.meta.dirname, '../..')

export default defineConfig({
  plugins: [svelte({ compilerOptions: { runes: true } })],
  resolve: {
    alias: {
      $game: resolve(REPO_ROOT, 'client/src/lib'),
      three: resolve(import.meta.dirname, 'node_modules/three'),
    },
    dedupe: ['three'],
  },
  test: {
    include: ['test/**/*.test.ts'],
    environment: 'node',
  },
})
