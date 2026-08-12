/// Route mirror positions through the spectator's locally loaded passability
/// so the watched character does not interpolate through solid geometry.
import { passability_is_movement_blocked } from '../wasm/onlinerpg_shared'
import { findPath } from './pathfinding'

export interface Leg {
  x: number
  y: number
  z: number
}

const routes = new Map<number, Leg[]>()

export function clearRoute(playerId: number): void {
  routes.delete(playerId)
}

export function routeObserved(
  playerId: number,
  from: Leg,
  to: Leg,
  floorLevel: number
): Leg {
  routes.delete(playerId)

  let blocked = false
  try {
    blocked = passability_is_movement_blocked(
      from.x,
      from.z,
      to.x,
      to.z,
      floorLevel,
      to.y
    )
  } catch {
    return to
  }
  if (!blocked) return to

  const path = findPath(from.x, from.z, floorLevel, to.x, to.z, floorLevel)
  const legs: Leg[] = (path.waypoints ?? []).map((waypoint) => ({
    x: waypoint.x,
    y: to.y,
    z: waypoint.z,
  }))
  if (legs.length === 0) return to
  if (legs.length > 1) routes.set(playerId, legs.slice(1))
  return legs[0]
}

export function nextLeg(playerId: number): Leg | null {
  const legs = routes.get(playerId)
  if (!legs || legs.length === 0) {
    routes.delete(playerId)
    return null
  }
  const leg = legs.shift() as Leg
  if (legs.length === 0) routes.delete(playerId)
  return leg
}
