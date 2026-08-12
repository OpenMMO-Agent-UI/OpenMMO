import { derived, writable } from 'svelte/store'
import type {
  EquipSlot,
  ItemInstance,
  PlayerInventory,
} from '../network/networkTypes'
import { getItemDef } from '../data/itemDefs'
import { isUnderground } from './dungeonStore'
import { serverGameTime } from './timeStore'

export type { EquipSlot, ItemInstance, PlayerInventory }

const initialState: PlayerInventory = {
  bag: [],
  equipped: {},
}

export const inventoryStore = writable<PlayerInventory>({ ...initialState })

/** The local player's gold in the smallest currency unit (copper). */
export const playerGold = writable(0)

/** The local player's effective guard (base attribute + equipped-gear bonuses),
 *  computed server-side and pushed on join and after each equipment change.
 *  `null` until the first GuardUpdated arrives. */
export const playerGuard = writable<number | null>(null)

/** Item defs that act as a carried light source (mirrors shared TORCH_ITEM_IDS). */
const TORCH_ITEM_IDS = ['torch', 'worn_torch']

export function isTorchItemDefId(id: string | null | undefined): boolean {
  return id != null && TORCH_ITEM_IDS.includes(id)
}

/** True when the local player has a torch equipped in the off-hand slot. */
export const localTorchEquipped = derived(inventoryStore, (inv) => {
  const id = inv.equipped.off_hand?.item_def_id
  return isTorchItemDefId(id)
})

/** Item defs that light the way when a torch would, but are not torches. */
const SHIELD_ITEM_IDS = ['wooden_shield', 'raven_shield']

export function isShieldItemDefId(id: string | null | undefined): boolean {
  return id != null && SHIELD_ITEM_IDS.includes(id)
}

/** Whether the local player's equipped shield should be burning like a torch:
 *  once the sun is down, or anywhere underground — a shield that went dark in
 *  a pitch-black dungeon would read as a bug. Client-side only, and the wire
 *  has no off-hand field, so this can never light a remote player's shield. */
export const shieldGlowLit = derived(
  [inventoryStore, serverGameTime, isUnderground],
  ([inv, time, underground]) =>
    isShieldItemDefId(inv.equipped.off_hand?.item_def_id) &&
    (underground || time?.isNight === true)
)

/** The local player's first revive item (phoenix talisman), offered on the
 *  death dialog together with its def. */
export const reviveItem = derived(inventoryStore, (inv) => {
  for (const item of inv.bag) {
    const def = getItemDef(item.item_def_id)
    if (def?.reviveHpPercent != null) return { item, def }
  }
  return null
})

export function setInventory(inventory: PlayerInventory) {
  inventoryStore.set(inventory)
}

export function resetInventoryStore() {
  inventoryStore.set({ bag: [], equipped: {} })
  playerGold.set(0)
  playerGuard.set(null)
}
