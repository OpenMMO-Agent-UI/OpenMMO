/// Spectator mode renders an agent-client mirror instead of owning a player
/// session. The unauthenticated mirror is accepted from loopback only.
import { shortestWrappedDeltaX } from '../terrain/world-wrap'

function readObserveParam(): string | null {
  if (typeof window === 'undefined') return null
  const raw = new URLSearchParams(window.location.search).get('observe')
  if (!raw) return null
  try {
    const url = new URL(raw)
    const local = ['127.0.0.1', 'localhost', '[::1]', '::1']
    if (!/^wss?:$/.test(url.protocol) || !local.includes(url.hostname)) {
      console.warn('Ignoring non-loopback observe target:', raw)
      return null
    }
    return url.toString()
  } catch {
    return null
  }
}

export const observerUrl = readObserveParam()
export const isObserver = observerUrl !== null

let observedId: number | null = null

export function setObservedPlayerId(id: number): void {
  observedId = id
}

export function observedPlayerId(): number | null {
  return observedId
}

const SNAP_DISTANCE = 8

export function farEnoughToSnap(
  from: { x: number; y: number; z: number },
  to: { x: number; y: number; z: number }
): boolean {
  const dx = shortestWrappedDeltaX(from.x, to.x)
  const dz = to.z - from.z
  return dx * dx + dz * dz > SNAP_DISTANCE * SNAP_DISTANCE
}

export function ownedByMe(
  ownerId: number | undefined | null,
  myPlayerId: number | undefined | null
): boolean {
  if (isObserver) return false
  return ownerId != null && ownerId === myPlayerId
}
