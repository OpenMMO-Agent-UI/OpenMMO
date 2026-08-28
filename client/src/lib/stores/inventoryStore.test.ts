import { beforeEach, describe, expect, it } from 'vitest'
import {
  inventoryStore,
  isTorchItemDefId,
  resetInventoryStore,
  shieldGlowLit,
} from './inventoryStore'
import type { ItemInstance } from './inventoryStore'
import { currentDungeonDepth } from './dungeonStore'
import { serverGameTime } from './timeStore'

describe('isTorchItemDefId', () => {
  it('recognizes every carried torch variant', () => {
    expect(isTorchItemDefId('torch')).toBe(true)
    expect(isTorchItemDefId('worn_torch')).toBe(true)
  })

  it('rejects missing and unrelated item definitions', () => {
    expect(isTorchItemDefId(undefined)).toBe(false)
    expect(isTorchItemDefId(null)).toBe(false)
    expect(isTorchItemDefId('dagger')).toBe(false)
  })
})

describe('shieldGlowLit', () => {
  const shield: ItemInstance = {
    instance_id: 1,
    item_def_id: 'wooden_shield',
    quantity: 1,
    enchant: 0,
  }
  const night = {
    year: 1,
    month: 1,
    day: 1,
    hour: 22,
    minute: 0,
    isNight: true,
  }
  const day = { ...night, hour: 12, isNight: false }

  function lit(): boolean {
    let value = false
    shieldGlowLit.subscribe((v) => (value = v))()
    return value
  }

  beforeEach(() => {
    resetInventoryStore()
    serverGameTime.set(day)
    currentDungeonDepth.set(0)
  })

  it('stays dark with a shield in broad daylight', () => {
    inventoryStore.set({ bag: [], equipped: { off_hand: shield } })
    expect(lit()).toBe(false)
  })

  it('burns once the sun is down', () => {
    inventoryStore.set({ bag: [], equipped: { off_hand: shield } })
    serverGameTime.set(night)
    expect(lit()).toBe(true)
  })

  it('burns underground whatever the hour', () => {
    inventoryStore.set({ bag: [], equipped: { off_hand: shield } })
    currentDungeonDepth.set(1)
    expect(lit()).toBe(true)
  })

  it('leaves an empty or torch-lit off-hand to the torch path', () => {
    serverGameTime.set(night)
    expect(lit()).toBe(false)
    inventoryStore.set({
      bag: [],
      equipped: { off_hand: { ...shield, item_def_id: 'torch' } },
    })
    expect(lit()).toBe(false)
  })
})
