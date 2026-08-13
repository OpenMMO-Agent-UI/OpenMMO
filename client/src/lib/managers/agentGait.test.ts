// An agent-client session is drawn by the same remote-player interpolation as
// a human's, so its PlayerMove stream has to read the same way. It does not
// send a smoothed waypoint whole — it subdivides into MAX_STEP_DIST hops — and
// from standstill that hop is the entire distance the watching client is given,
// which is what picks the walk or jog clip.
import { describe, expect, it } from 'vitest'
import {
  DEFAULT_MOVEMENT_CONFIG,
  calculateMovementStep,
  getMovementMode,
  hasTargetChanged,
  initMovementState,
  type MovementState,
  type Position,
} from '../utils/movementUtils'

/** shared/src/world.rs PLAYER_MOVE_SPEED */
const MOVE_SPEED = 3
/** agent-client/src/driver/movement.rs MAX_STEP_DIST */
const MAX_STEP_DIST = 4.0
const FRAME = 1 / 60

interface Wire {
  atMs: number
  target: Position
}

/** agent-client's walk_waypoints: hop, then sleep the hop's travel time. */
function agentStream(distance: number): Wire[] {
  const out: Wire[] = []
  let walked = 0
  let atMs = 0
  while (walked < distance - 0.1) {
    const step = Math.min(MAX_STEP_DIST, distance - walked)
    walked += step
    out.push({ atMs, target: { x: 0, y: 0, z: walked } })
    atMs += Math.max(50, (step / MOVE_SPEED) * 1000)
  }
  return out
}

/** A human's click: one smoothed waypoint. */
function humanStream(distance: number): Wire[] {
  return [{ atMs: 0, target: { x: 0, y: 0, z: distance } }]
}

/** Replay a wire stream through remotePlayerManager's per-frame math. */
function render(wire: Wire[], seconds: number) {
  let pos: Position = { x: 0, y: 0, z: 0 }
  let movement: MovementState | undefined
  let target: Position | undefined
  let next = 0
  const legs: string[] = []
  const frames: string[] = []

  for (let t = 0; t < seconds; t += FRAME) {
    while (next < wire.length && wire[next].atMs <= t * 1000) {
      target = wire[next].target
      next++
    }
    if (!target) continue

    if (hasTargetChanged(movement, target)) {
      movement = initMovementState(pos, target, movement?.currentSpeed ?? 0)
      legs.push(getMovementMode(movement.totalDistance))
    }
    if (!movement) continue

    const r = calculateMovementStep(
      pos,
      movement,
      DEFAULT_MOVEMENT_CONFIG,
      FRAME
    )
    movement.currentSpeed = r.newSpeed
    pos = r.newPos
    frames.push(r.arrived ? 'idle' : getMovementMode(movement.totalDistance))
  }
  const walking = frames.filter((m) => m === 'walk').length / frames.length
  return { legs, travelled: pos.z, walking }
}

describe('agent gait', () => {
  it('sets off across open ground at the same gait a human does', () => {
    const agent = render(agentStream(30), 14)
    const human = render(humanStream(30), 14)

    expect(agent.travelled).toBeCloseTo(30, 1)
    // The regression: subdividing at the client's own walk cutoff opened every
    // journey with a walk clip under a body already gliding at jog speed.
    expect(agent.legs[0]).toBe(human.legs[0])
    expect(agent.legs[0]).toBe('jog')
    // Only the last leg — whatever distance is left over — may still walk.
    expect(agent.legs.slice(0, -1).every((m) => m === 'jog')).toBe(true)
    expect(agent.walking).toBeLessThan(0.15)
  })

  it('still walks a short hop, exactly as a human clicking two metres away', () => {
    expect(render(agentStream(2), 4).legs).toEqual(
      render(humanStream(2), 4).legs
    )
    expect(render(agentStream(2), 4).legs).toContain('walk')
  })
})
