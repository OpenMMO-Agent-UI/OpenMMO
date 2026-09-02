import { beforeEach, describe, expect, it } from 'vitest'
import { gameStore } from '../stores/gameStore'
import { setObservedPlayerId } from '../stores/observerStore'
import { remotePlayerManager } from './remotePlayerManager'

const ID = 11

function watch(mounted: boolean) {
  setObservedPlayerId(ID)
  gameStore.update((state) => ({
    ...state,
    currentPlayer: {
      id: ID,
      name: 'Mirror',
      level: 1,
      health: 10,
      maxHealth: 10,
      characterClass: 'knight',
      gender: 'male',
      mounted,
      position: { x: 0, y: 0, z: 0 },
      rotation: 0,
    } as never,
  }))
  remotePlayerManager.removePlayer(ID)
  remotePlayerManager.initPlayer(ID, { x: 0, y: 0, z: 0 }, 0)
  remotePlayerManager.setTargetPosition(ID, { x: 40, y: 0, z: 0 }, 0, true)
  remotePlayerManager.update(1000)
  return remotePlayerManager.players.get(ID)?.position.x ?? 0
}

/// The watched character lives in `currentPlayer`, never in `otherPlayers`, so
/// the mount lookup used to miss and the mirror walked a rider at footpace —
/// the body fell behind its own frames until the desync guard snapped it on.
describe('the watched character’s mirror', () => {
  beforeEach(() => {
    remotePlayerManager.removePlayer(ID)
  })

  it('rides at horse speed when the character is mounted', () => {
    expect(watch(true)).toBeGreaterThan(watch(false) * 2)
  })

  it('sprints at sprint speed when it is not', () => {
    expect(watch(false)).toBeCloseTo(4.5, 1)
  })

  /// A route around an obstacle is handed leg by leg (GameScene), and the
  /// server is running the whole of it at one speed.
  it('keeps sprinting through a handed-over leg', () => {
    watch(false)
    remotePlayerManager.setTargetPosition(ID, { x: 80, y: 0, z: 0 }, 0)
    remotePlayerManager.update(1000)

    const walked = remotePlayerManager.players.get(ID)?.position.x ?? 0
    expect(walked).toBeGreaterThan(4.5 * 2 - 0.5)
  })
})
