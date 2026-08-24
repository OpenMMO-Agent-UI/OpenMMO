import { svelte } from '@sveltejs/vite-plugin-svelte'
import { defineConfig } from 'vitest/config'

// The core is plain TypeScript over the glTF container, so it tests in node
// against the GLBs the game actually ships. The svelte plugin is here only so
// the `.svelte.ts` modules — the ones using runes — can be imported at all.
export default defineConfig({
  plugins: [svelte({ compilerOptions: { runes: true } })],
  test: {
    include: ['test/**/*.test.ts'],
    environment: 'node',
  },
})
