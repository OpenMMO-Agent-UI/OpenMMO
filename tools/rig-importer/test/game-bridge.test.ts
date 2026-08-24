import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/**
 * The preview calls the client's own retargeting so it cannot drift from what
 * the game plays. src/game-bridge.d.ts describes that function locally, which
 * means TypeScript will not notice if the real one changes shape — so check it
 * here, against the actual client source.
 */
const CLIENT_MODULE = resolve(import.meta.dirname, '../../../client/src/lib/utils/characterAnimationUtils.ts')

describe('the game bridge', () => {
  it('still exports loadSharedPackClipsForModel', async () => {
    const module = await import(CLIENT_MODULE)
    expect(typeof module.loadSharedPackClipsForModel).toBe('function')
  })

  it('still takes (modelPath, targetScene, clipNames, grounding)', () => {
    const source = readFileSync(CLIENT_MODULE, 'utf8')
    const signature = source
      .slice(source.indexOf('export function loadSharedPackClipsForModel'))
      .split('):')[0]

    for (const parameter of ['modelPath: string', 'targetScene: THREE.Object3D', 'clipNames: string[]']) {
      expect(signature).toContain(parameter)
    }
    expect(signature).toMatch(/grounding:\s*GroundClipsOptions/)
  })

  it('still grounds clips with restClip and restOffset', () => {
    const source = readFileSync(CLIENT_MODULE, 'utf8')
    expect(source).toMatch(/restClip\?:\s*string/)
    expect(source).toMatch(/restOffset\?:\s*number/)
  })

  it('still loads the packs from the paths the dev server serves', async () => {
    const paths = await import(resolve(import.meta.dirname, '../../../client/src/lib/utils/modelPaths.ts'))
    expect(paths.CHARACTER_ANIMATION_PACK_PATHS.locomotion).toBe('/models/animations/locomotion.glb')
    expect(paths.CHARACTER_ANIMATION_PACK_PATHS.combatMelee).toBe('/models/animations/combat_melee.glb')
  })
})
