import { afterEach, describe, expect, it } from 'vitest'
import { INITIAL_YAW } from '../components/game-scene/camera-utils'
import {
  facesCamera,
  quadrantsOf,
  setViewYaw,
  viewQuadrant,
  viewRayRun,
  viewRevision,
} from './view-direction'

/** The fixed south-west camera every occluder was written against. */
function legacyRun(
  min: [number, number, number],
  max: [number, number, number],
  p: [number, number, number]
) {
  const sHigh = max[1] - p[1]
  if (sHigh <= 0) return -1
  const sLow = Math.max(min[1] - p[1], 0)
  const sMin = Math.max(p[0] - max[0], min[2] - p[2], sLow)
  const sMax = Math.min(p[0] - min[0], max[2] - p[2], sHigh)
  return sMax - sMin
}

describe('view direction', () => {
  afterEach(() => setViewYaw(INITIAL_YAW))

  it('matches the fixed south-west camera at the default yaw', () => {
    let seed = 7
    const rand = () => {
      seed = (seed * 16807) % 2147483647
      return (seed / 2147483647) * 20 - 10
    }
    for (let i = 0; i < 500; i++) {
      const a: [number, number, number] = [rand(), rand() / 4, rand()]
      const size: [number, number, number] = [
        Math.abs(rand()) / 2,
        Math.abs(rand()) / 2,
        Math.abs(rand()) / 2,
      ]
      const max: [number, number, number] = [
        a[0] + size[0],
        a[1] + size[1],
        a[2] + size[2],
      ]
      const p: [number, number, number] = [rand() / 2, 0, rand() / 2]
      const legacy = legacyRun(a, max, p)
      const run = viewRayRun(...a, ...max, ...p)
      expect(run > 0.05).toBe(legacy > 0.05)
      if (legacy > 0) expect(run).toBeCloseTo(legacy, 6)
    }
  })

  it('shows the south and west walls by default', () => {
    expect(facesCamera('south')).toBe(true)
    expect(facesCamera('west')).toBe(true)
    expect(facesCamera('north')).toBe(false)
    expect(facesCamera('east')).toBe(false)
    expect(viewQuadrant()).toBe('sw')
  })

  it('turns the occluders with the camera', () => {
    const before = viewRevision
    setViewYaw(INITIAL_YAW + Math.PI)

    expect(viewRevision).toBeGreaterThan(before)
    expect(facesCamera('north')).toBe(true)
    expect(facesCamera('east')).toBe(true)
    expect(facesCamera('south')).toBe(false)
    expect(viewQuadrant()).toBe('ne')
    const wallNorthOfPlayer = viewRayRun(-2, 0, -1.2, 2, 3, -0.8, 0, 0, 0)
    const wallSouthOfPlayer = viewRayRun(-2, 0, 0.8, 2, 3, 1.2, 0, 0, 0)
    expect(wallNorthOfPlayer).toBeGreaterThan(0)
    expect(wallSouthOfPlayer).toBeLessThan(0)
  })

  it('shows a single side when looking straight down an axis', () => {
    setViewYaw(0)
    expect(facesCamera('south')).toBe(true)
    expect(facesCamera('east')).toBe(false)
    expect(facesCamera('west')).toBe(false)
  })

  it('files each side under the two quadrants that see it', () => {
    expect(quadrantsOf('south').sort()).toEqual(['se', 'sw'])
    expect(quadrantsOf('north').sort()).toEqual(['ne', 'nw'])
    expect(quadrantsOf('west').sort()).toEqual(['nw', 'sw'])
  })
})
