import { describe, expect, it } from 'vitest'
import { formatRotation, gripFromCsv, gripRadians, gripToCsv, isRotated, parseRotation } from '../src/lib/game/grip'

describe('grip', () => {
  it('reads a shipped row as an along-bone offset only', () => {
    // ogre ships weaponOffset 0.24 and nothing else.
    expect(gripFromCsv({ weaponOffset: '0.24' })).toEqual({ x: 0, y: 0.24, z: 0, rx: 0, ry: 0, rz: 0 })
  })

  it('reads all six axes back', () => {
    const grip = gripFromCsv({
      weaponOffset: '0.24',
      weaponOffsetX: '-0.03',
      weaponOffsetZ: '0.015',
      weaponRotation: '90|0|-45',
    })
    expect(grip).toEqual({ x: -0.03, y: 0.24, z: 0.015, rx: 90, ry: 0, rz: -45 })
  })

  it('leaves unused columns blank rather than writing zeros', () => {
    const written = gripToCsv({ x: 0, y: 0.24, z: 0, rx: 0, ry: 0, rz: 0 })
    expect(written).toEqual({
      weaponOffset: '0.24',
      weaponOffsetX: '',
      weaponOffsetZ: '',
      weaponRotation: '',
    })
  })

  it('round-trips a fitted grip', () => {
    const grip = { x: -0.032, y: 0.241, z: 0.016, rx: 90, ry: 12.5, rz: -45 }
    expect(gripFromCsv(gripToCsv(grip))).toEqual(grip)
  })

  it('never writes a comma, which these CSVs cannot carry', () => {
    const written = gripToCsv({ x: -0.5, y: 1.25, z: 0.75, rx: -180, ry: 90, rz: 33.3 })
    for (const value of Object.values(written)) expect(value).not.toMatch(/[,\n"]/)
  })

  it('converts to the radians the client builds its Euler from', () => {
    const [rx, ry, rz] = gripRadians({ x: 0, y: 0, z: 0, rx: 180, ry: -90, rz: 0 })
    expect(rx).toBeCloseTo(Math.PI, 10)
    expect(ry).toBeCloseTo(-Math.PI / 2, 10)
    expect(rz).toBe(0)
  })

  it('treats a component it cannot read as zero, as the client does', () => {
    expect(parseRotation('90|banana|0')).toEqual([90, 0, 0])
    expect(parseRotation('')).toEqual([0, 0, 0])
    expect(parseRotation(undefined)).toEqual([0, 0, 0])
  })

  it('knows when a grip is turned', () => {
    expect(isRotated({ x: 1, y: 2, z: 3, rx: 0, ry: 0, rz: 0 })).toBe(false)
    expect(isRotated({ x: 0, y: 0, z: 0, rx: 0, ry: 0.5, rz: 0 })).toBe(true)
    expect(formatRotation(0, 0, 0)).toBe('')
    expect(formatRotation(90, 0, -45)).toBe('90|0|-45')
  })
})
