import { describe, expect, it } from 'vitest'
import { CORE_BONES, checkCompatibility, walkSpeedFor } from '../src/lib/bones/compat'

const full = { boneNames: [...CORE_BONES] as string[], boneCount: 16, hipsHeight: 1 }

describe('core-bone compatibility', () => {
  it('passes a rig with the silhouette', () => {
    expect(checkCompatibility(full).compatible).toBe(true)
  })

  it('passes a rig with no fingers — ogre has 33 bones and still has to be judged', () => {
    expect(checkCompatibility({ ...full, boneCount: 33 }).compatible).toBe(true)
  })

  it('names what a rig missing an arm lacks', () => {
    const without = full.boneNames.filter((bone) => bone !== 'RightForeArm')
    const result = checkCompatibility({ ...full, boneNames: without })
    expect(result.compatible).toBe(false)
    expect(result.missing).toEqual(['RightForeArm'])
  })
})

describe('walk speed', () => {
  it('reproduces the values recorded for the shipped monsters', () => {
    // doc/assets/monsters.md: hobgoblin 0.98 -> 1.52, ogre 1.17 -> 1.81.
    expect(walkSpeedFor(0.98)).toBeCloseTo(1.51, 2)
    expect(walkSpeedFor(1.17)).toBeCloseTo(1.81, 2)
    expect(walkSpeedFor(1.165)).toBeCloseTo(1.8, 3)
  })
})

describe('near misses', () => {
  it('spots a bone that is there under another spelling', () => {
    // stone_golem: has `neck`, needs `Neck`. One rename from being driveable.
    const golem = full.boneNames.filter((bone) => bone !== 'Neck').concat('neck')
    const result = checkCompatibility({ ...full, boneNames: golem })
    expect(result.compatible).toBe(false)
    expect(result.missing).toEqual(['Neck'])
    expect(result.nearMisses).toEqual([{ want: 'Neck', have: 'neck' }])
  })

  it('sees through a Mixamo prefix and a separator', () => {
    const prefixed = full.boneNames.filter((b) => b !== 'LeftUpLeg').concat('mixamorig:Left_Up_Leg')
    expect(checkCompatibility({ ...full, boneNames: prefixed }).nearMisses).toEqual([
      { want: 'LeftUpLeg', have: 'mixamorig:Left_Up_Leg' },
    ])
  })

  it('reports nothing to rename when the joint is genuinely absent', () => {
    const armless = full.boneNames.filter((bone) => bone !== 'RightForeArm')
    const result = checkCompatibility({ ...full, boneNames: armless })
    expect(result.missing).toEqual(['RightForeArm'])
    expect(result.nearMisses).toEqual([])
  })
})
