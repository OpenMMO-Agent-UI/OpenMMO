import { describe, expect, it } from 'vitest'
import { distanceToTarget, frameFor, MIN_FRAMED_HEIGHT } from '../src/lib/viewer/framing'

// Heights the tool actually sees: a raw Meshy export at 1cm scale through to a
// troll, plus the degenerate values a broken file can produce.
const HEIGHTS = [0, 0.004, 0.02, 0.3, 1.09, 1.8, 2.4, 2.7, 6, 40]

describe('camera framing', () => {
  it.each(HEIGHTS)('keeps the camera outside the near plane at %s m', (height) => {
    const framing = frameFor(height)
    // The bug this guards: distance 0.04 with a near plane of 0.05 clipped the
    // whole scene and the viewport rendered black.
    expect(distanceToTarget(framing)).toBeGreaterThan(framing.near * 10)
  })

  it.each(HEIGHTS)('keeps the model inside the far plane at %s m', (height) => {
    const framing = frameFor(height)
    expect(framing.far).toBeGreaterThan(distanceToTarget(framing) + Math.max(height, 0))
  })

  it('looks at the middle of the model', () => {
    expect(frameFor(2.4).target[1]).toBeCloseTo(1.2, 6)
  })

  it('backs off further for a taller model', () => {
    expect(distanceToTarget(frameFor(2.7))).toBeGreaterThan(distanceToTarget(frameFor(1.8)))
  })

  it('stops shrinking below the floor, so nothing degenerate gets through', () => {
    expect(frameFor(0)).toEqual(frameFor(MIN_FRAMED_HEIGHT))
    expect(frameFor(Number.NaN).near).toBeGreaterThan(0)
  })
})
