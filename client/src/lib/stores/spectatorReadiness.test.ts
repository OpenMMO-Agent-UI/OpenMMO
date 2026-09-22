import { describe, expect, it } from 'vitest'

import { spectatorHasEnteredWorld } from './spectatorReadiness'

describe('spectator/manual world readiness', () => {
  it('treats a joined character at 0 HP as entered instead of still waiting', () => {
    expect(spectatorHasEnteredWorld(0)).toBe(true)
  })

  it('waits only while no current-player snapshot has arrived', () => {
    expect(spectatorHasEnteredWorld(null)).toBe(false)
    expect(spectatorHasEnteredWorld(12)).toBe(true)
  })
})
