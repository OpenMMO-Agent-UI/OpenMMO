/**
 * The bug: choosing a take for `locomotion`'s `walk` also replaced
 * `locomotion2`'s `walk`, because overrides were keyed on the motion name
 * alone and two different packs can share a name. These pin the fix: an
 * override belongs to a (pack, motion) pair.
 */
import { beforeEach, describe, expect, it } from 'vitest'
import {
  currentOverride,
  overrideCount,
  overrideKey,
  packDecisions,
  session,
  slotsOverriding,
  splitOverrideKey,
} from '../src/lib/session.svelte'

beforeEach(() => {
  session.overrides = {}
  session.motion = ''
  session.pack = ''
})

describe('overrideKey / splitOverrideKey', () => {
  it('round-trips', () => {
    expect(splitOverrideKey(overrideKey('locomotion', 'walk'))).toEqual({ pack: 'locomotion', motion: 'walk' })
  })

  it('keeps two packs sharing a motion name distinct', () => {
    expect(overrideKey('locomotion', 'walk')).not.toBe(overrideKey('locomotion2', 'walk'))
  })
})

describe('deciding one pack does not touch another with the same motion name', () => {
  it('setting locomotion/walk leaves locomotion2/walk on its own default', () => {
    session.overrides[overrideKey('locomotion', 'walk')] = 'takes/walk/heavy.fbx'

    session.pack = 'locomotion'
    session.motion = 'walk'
    expect(currentOverride()).toBe('takes/walk/heavy.fbx')

    session.pack = 'locomotion2'
    session.motion = 'walk'
    expect(currentOverride()).toBeUndefined()
  })

  it('counts each pack’s decision separately even though the name repeats', () => {
    session.overrides[overrideKey('locomotion', 'walk')] = 'takes/walk/a.fbx'
    session.overrides[overrideKey('locomotion2', 'walk')] = 'takes/walk/b.fbx'
    expect(overrideCount()).toBe(2)
  })
})

describe('slotsOverriding', () => {
  it('finds every (pack, motion) a take is currently used for', () => {
    session.overrides[overrideKey('locomotion', 'walk')] = 'takes/walk/shared.fbx'
    session.overrides[overrideKey('locomotion2', 'walk')] = 'takes/walk/shared.fbx'
    session.overrides[overrideKey('locomotion', 'idle1')] = 'takes/idle1/other.fbx'

    const slots = slotsOverriding('takes/walk/shared.fbx')
    expect(slots).toHaveLength(2)
    expect(slots).toEqual(
      expect.arrayContaining([
        { pack: 'locomotion', motion: 'walk' },
        { pack: 'locomotion2', motion: 'walk' },
      ])
    )
  })
})

describe('what a written pack is built from', () => {
  beforeEach(() => {
    session.motions = [
      { name: 'walk', pack: 'locomotion' },
      { name: 'jog', pack: 'locomotion' },
      { name: 'jump', pack: 'locomotion' },
      { name: 'slash1', pack: 'combat_melee' },
    ]
  })

  it('lists every motion in the pack, not just the replaced ones', () => {
    session.overrides[overrideKey('locomotion', 'walk')] = 'takes/walk/heavy.fbx'

    const decisions = packDecisions('locomotion')
    // The bug: `jump` and `jog` were dropped, and the written pack had no
    // `jump` clip at all.
    expect(decisions.map((d) => d.motion)).toEqual(['walk', 'jog', 'jump'])
    expect(decisions.find((d) => d.motion === 'walk')?.takePath).toBe('takes/walk/heavy.fbx')
    expect(decisions.find((d) => d.motion === 'jump')?.takePath).toBeNull()
    expect(decisions.find((d) => d.motion === 'jog')?.takePath).toBeNull()
  })

  it('stays inside the pack being written', () => {
    expect(packDecisions('locomotion').some((d) => d.motion === 'slash1')).toBe(false)
    expect(packDecisions('combat_melee').map((d) => d.motion)).toEqual(['slash1'])
  })

  it('carries everything over when nothing was replaced', () => {
    const decisions = packDecisions('locomotion')
    expect(decisions).toHaveLength(3)
    expect(decisions.every((d) => d.takePath === null)).toBe(true)
  })
})
