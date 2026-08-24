/**
 * The narrow door, checked against the real module.
 *
 * src/game-bridge.d.ts describes the client's functions locally so this program
 * does not compile the client's tree. That means TypeScript cannot catch the
 * client changing their shape — a renamed export or a reordered parameter would
 * only show up as the sheet playing something the game does not. So this loads
 * the actual module and asserts the shape the tool relies on.
 */
import { describe, expect, it } from 'vitest'
import * as game from '$game/utils/characterAnimationUtils'

describe('the game module this tool borrows', () => {
  it('still exports the three functions the sheet runs on', () => {
    expect(typeof game.retargetAnimationsForCharacterModel).toBe('function')
    expect(typeof game.groundRetargetedClips).toBe('function')
    expect(typeof game.loadSharedPackClipsForModel).toBe('function')
  })

  it('takes the pack override this tool added, after the four the game passes', () => {
    // (modelPath, targetScene, clipNames, grounding, packPaths) — a parameter
    // with a default is not counted in `length`, so the four before it are.
    expect(game.loadSharedPackClipsForModel.length).toBe(3)
    const source = game.loadSharedPackClipsForModel.toString()
    expect(source).toContain('packPaths')
  })

  it('still says which pack each motion comes from', () => {
    expect(game.ANIMATION_SOURCE_BY_NAME.walk).toBe('locomotion')
    expect(game.ANIMATION_SOURCE_BY_NAME.dying).toBe('combat_melee')
  })
})
