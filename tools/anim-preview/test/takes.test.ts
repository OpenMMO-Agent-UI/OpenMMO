import { describe, expect, it } from 'vitest'
import { standardBoneName } from '../src/lib/takes'

describe('Mixamo bone names', () => {
  it('strips the prefix the repo strips, in both spellings', () => {
    expect(standardBoneName('mixamorig:Hips')).toBe('Hips')
    expect(standardBoneName('mixamorig1:LeftHand')).toBe('LeftHand')
    expect(standardBoneName('mixamorig_Spine2')).toBe('Spine2')
  })

  it('leaves a name that is already standard alone', () => {
    expect(standardBoneName('Hips')).toBe('Hips')
    expect(standardBoneName('RightHandIndex1')).toBe('RightHandIndex1')
  })
})
