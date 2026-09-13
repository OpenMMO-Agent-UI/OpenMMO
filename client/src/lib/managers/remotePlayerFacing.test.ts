/// A swing is preceded by a face-only move carrying the rotation toward the
/// target — agent-client's tick_combat sends one, and so does the web client.
/// Both arrive in the same batch as the attack, and once the swing starts the
/// interpolator skips the attacker entirely, so the facing has to be taken at
/// the moment the clip does.
import { beforeEach, describe, expect, it } from 'vitest'
import { remotePlayerManager } from './remotePlayerManager'

const ID = 21
const FACING = 1.5

function faceThenSwing() {
  remotePlayerManager.setTargetPosition(ID, { x: 0, y: 0, z: 0 }, FACING)
  remotePlayerManager.handleAttack(ID)
  return remotePlayerManager.players.get(ID)
}

describe('a remote swing faces what it is swinging at', () => {
  beforeEach(() => {
    remotePlayerManager.removePlayer(ID)
    remotePlayerManager.initPlayer(ID, { x: 0, y: 0, z: 0 }, 0)
  })

  it('takes the facing from the move that preceded it, not the drawn body', () => {
    const player = faceThenSwing()
    expect(player?.state).toBe('attack')
    expect(player?.rotation).toBe(FACING)
  })

  it('turns to a new target between swings', () => {
    faceThenSwing()
    // The next face-only move lands mid-clip, where the buffer used to hold it
    // until the swing was over — so every repeat pointed at the first target.
    remotePlayerManager.setTargetPosition(ID, { x: 0, y: 0, z: 0 }, -FACING)
    expect(remotePlayerManager.players.get(ID)?.rotation).toBe(-FACING)
    expect(remotePlayerManager.players.get(ID)?.state).toBe('attack')
  })
})
