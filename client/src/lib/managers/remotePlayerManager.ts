import { steerHorse } from '../utils/horseMovement'
import { SvelteMap } from 'svelte/reactivity'
import { get } from 'svelte/store'
import { hmrSingleton } from '../utils/hmr'
import { gameStore } from '../stores/gameStore'
import {
  calculateMovementStep,
  initMovementState,
  getMovementMode,
  hasTargetChanged,
  DEFAULT_MOVEMENT_CONFIG,
  SPRINT_SPEED_MULT,
  scaleMovementConfig,
  type Position,
  type MovementState,
  type MovementConfig,
  type PlayerState,
} from '../utils/movementUtils'
import { entityGroundY } from './entity-ground'
import { dungeonManager } from './dungeonManager'
import { currentDungeonDepth } from '../stores/dungeonStore'
import { observedPlayerId } from '../stores/observerStore'
import { FishingAnimationName, SitAnimationName } from '../types/animations'
import { shortestWrappedDeltaX } from '../terrain/world-wrap'
import type { TerrainHeightManager } from './terrainHeightManager'
import { mountFloats, mountSpeedMult } from '../utils/mounts'
import type { MountKind } from '../network/networkTypes'
import { floatingSurfaceY } from '../utils/floatingSurface'

// Use the same movement config as local player
const MOVEMENT_CONFIG: MovementConfig = {
  ...DEFAULT_MOVEMENT_CONFIG,
}
const SPRINT_MOVEMENT_CONFIG = scaleMovementConfig(
  MOVEMENT_CONFIG,
  SPRINT_SPEED_MULT
)

/** Built once per mount kind the session actually sees. */
const mountedConfigs = new Map<string, MovementConfig>()

function movementConfigFor(
  mount: MountKind | null | undefined,
  sprinting: boolean
): MovementConfig {
  const base = sprinting ? SPRINT_MOVEMENT_CONFIG : MOVEMENT_CONFIG
  if (!mount) return base
  const key = `${mount}:${sprinting}`
  let config = mountedConfigs.get(key)
  if (!config) {
    config = scaleMovementConfig(base, mountSpeedMult(mount))
    mountedConfigs.set(key, config)
  }
  return config
}

/// Far enough that the player went somewhere, rather than the resting flush
/// that lands on the spot they already stopped at.
const PICKUP_CANCEL_DISTANCE = 0.2

function movedFar(
  from: Position,
  to: Position,
  threshold = PICKUP_CANCEL_DISTANCE
): boolean {
  const dx = shortestWrappedDeltaX(from.x, to.x)
  const dz = to.z - from.z
  return dx * dx + dz * dz > threshold * threshold
}

const FROZEN_STATES = new Set(['attack', 'dead', 'interact'])
/** Faster than the server so a remote replaying its trail catches up. */
const TRAIL_CATCH_UP_MULT = 1.5
/** Server ticks, so a long clip cannot grow the trail without bound. */
const MAX_TRAIL_POINTS = 200

class PlayerStateManager {
  players = new SvelteMap<number, PlayerState>()

  heightManager: TerrainHeightManager | null = null

  isInterpolating(playerId: number): boolean {
    const position = this.players.get(playerId)?.position
    const target = this.targetPositions.get(playerId)
    return !!position && !!target && movedFar(position, target)
  }

  waterSurfaceAt: ((x: number, z: number) => number) | null = null
  hasWaterSurfaceData: ((x: number, z: number) => boolean) | null = null

  floatSurfaceY(
    mount: MountKind | null,
    x: number,
    z: number,
    fallbackY: number
  ): number | null {
    if (!mountFloats(mount)) return null
    return floatingSurfaceY({
      x,
      z,
      fallbackY,
      heightManager: this.heightManager,
      waterSurfaceAt: this.waterSurfaceAt,
      hasWaterSurfaceData: this.hasWaterSurfaceData,
    })
  }

  // Positions a remote passed while frozen in a clip, replayed in order once
  // it ends: heading straight for the latest one cuts corners through walls.
  private trails = new Map<number, Position[]>()

  // Attack animation duration in seconds (updated from actual animation data)
  attackAnimationDuration = 1.0

  // Remote player movement data (for acceleration/deceleration)
  private movementData = new SvelteMap<number, MovementState>()

  // Server-authoritative movement targets for each remote player.
  private targetPositions = new SvelteMap<number, Position>()

  // Server-authoritative target rotation for each remote player.
  private targetRotations = new SvelteMap<number, number>()
  // Only read inside the update loop — no reactivity needed.
  private targetSprinting = new Map<number, boolean>()

  // Queue for pending attacks when player is still moving
  private attackQueue = new SvelteMap<number, string[]>()

  /** Flinch counters, kept apart from the player entries the movement loop
   *  rebuilds each frame. */
  hitCounters = new SvelteMap<number, number>()

  // Timestamp (performance.now()) when each player's attack animation started
  private attackStartTimes = new Map<number, number>()

  // Move remote players toward their target positions with acceleration/deceleration
  update(deltaTime: number) {
    const dt = deltaTime / 1000 // Convert to seconds
    const now = performance.now()

    // Check for attack animations that have finished
    this.attackStartTimes.forEach((startTime, playerId) => {
      const elapsed = (now - startTime) / 1000
      if (elapsed < this.attackAnimationDuration) return

      this.attackStartTimes.delete(playerId)

      const p = this.players.get(playerId)
      if (p?.state === 'attack')
        this.players.set(playerId, { ...p, state: 'idle' })
    })

    // Snapshot the player stores once for the frame (mount/torch lookups
    // below).
    const store = get(gameStore)
    const otherPlayers = store.otherPlayers
    /// Who a drawn body's state lives on. The watched character in observer
    /// mode is never in `otherPlayers` — it is `currentPlayer` (the same
    /// singleton trap the floorLevel lookup below documents) — so reading its
    /// mount there always missed, and a rider was interpolated at footpace
    /// while the server carried it off at `HORSE_MOVE_MULT`. The mirror fell
    /// behind until the desync guard snapped it forward, over and over.
    const drawnState = (playerId: number) =>
      playerId === observedPlayerId()
        ? (store.currentPlayer ?? undefined)
        : otherPlayers.get(playerId)

    // Update players
    this.targetPositions.forEach((targetPos, playerId) => {
      // Get current interpolated position or initialize from player position
      const currentPlayer = this.players.get(playerId)
      if (!currentPlayer) return

      if (FROZEN_STATES.has(currentPlayer.state)) return

      const currentPos = currentPlayer.position
      const sprinting = this.targetSprinting.get(playerId) ?? false
      const drawn = drawnState(playerId)
      const mount = drawn?.mount ?? null
      // dungeonManager's live depth is the only place the observed
      // character's real floor still is — it's never in otherPlayers.
      const floor =
        playerId === observedPlayerId() && dungeonManager.active
          ? -get(currentDungeonDepth)
          : (otherPlayers.get(playerId)?.floorLevel ?? 0)
      const hasTorch = drawn?.torchOn ?? false
      const movementConfig = movementConfigFor(mount, sprinting)

      const trail = this.trails.get(playerId)
      if (trail) {
        const speed = movementConfig.maxSpeed * TRAIL_CATCH_UP_MULT
        const pos = { ...currentPos }
        let rotation = this.advanceAlongTrail(playerId, trail, pos, speed * dt)
        rotation ??= currentPlayer.rotation
        pos.y = this.groundY(mount, floor, pos, currentPos.y)
        if (mount !== null)
          rotation = steerHorse(currentPlayer.rotation, rotation, dt).rotation
        // handleAttack reads the speed to queue swings until the remote stops.
        const running = this.movementData.get(playerId)
        if (running && this.trails.has(playerId)) running.currentSpeed = speed
        else
          this.movementData.set(
            playerId,
            initMovementState(pos, targetPos, speed)
          )
        this.players.set(playerId, {
          position: pos,
          state: 'moving',
          speed,
          rotation,
          movementMode: getMovementMode(undefined, hasTorch, sprinting),
        })
        return
      }

      // Get or initialize movement data
      let movement = this.movementData.get(playerId)
      const targetChanged = hasTargetChanged(movement, targetPos)

      if (targetChanged) {
        // New target - initialize movement from current position
        movement = initMovementState(
          currentPos,
          targetPos,
          movement?.currentSpeed ?? 0
        )
        this.movementData.set(playerId, movement)
      }

      // movement is guaranteed to be defined after above block
      if (!movement) return

      const mounted = mount !== null
      const result = calculateMovementStep(
        currentPos,
        movement,
        movementConfig,
        dt
      )

      if (mounted) {
        result.rotation = steerHorse(
          currentPlayer.rotation,
          this.targetRotations.get(playerId) ?? result.rotation,
          dt
        ).rotation
      }

      result.newPos.y = this.groundY(mount, floor, result.newPos, currentPos.y)

      // Update movement state
      movement.currentSpeed = result.newSpeed
      this.movementData.set(playerId, movement)

      // Update player
      // Since we skip movement update if player is already attacking,
      // currentState will just be based on whether they arrived
      const currentState = result.arrived ? 'idle' : 'moving'

      if (result.arrived) {
        // Use server-authoritative rotation on arrival (handles face-only packets)
        const targetRotation = this.targetRotations.get(playerId)
        this.players.set(playerId, {
          position: result.newPos,
          state: currentState,
          speed: 0,
          rotation: mounted
            ? result.rotation
            : (targetRotation ?? currentPlayer?.rotation ?? result.rotation),
          movementMode: undefined,
        })

        // Check for queued attacks upon arrival
        const queue = this.attackQueue.get(playerId)
        if (queue && queue.length > 0) {
          console.log(
            `[RemotePlayerManager] Executing queued attack for ${playerId} upon arrival`
          )
          queue.shift() // Consume one attack
          if (queue.length === 0) {
            this.attackQueue.delete(playerId)
          } else {
            this.attackQueue.set(playerId, queue)
          }
          this.executeAttack(playerId)
        }
      } else {
        const movementMode = getMovementMode(
          movement.totalDistance,
          hasTorch,
          sprinting
        )

        this.players.set(playerId, {
          position: result.newPos,
          state: currentState,
          speed: result.newSpeed,
          rotation: result.rotation,
          movementMode,
        })
      }
    })
  }

  // calculateMovementStep only advances XZ and carries Y over, and the move
  // protocol has no per-waypoint Y, so the ground has to be resampled. Without
  // it a remote keeps the Y it entered the floor with, which reads as sinking
  // through dungeon and house stairs.
  private groundY(
    mount: MountKind | null,
    floor: number,
    pos: Position,
    fallbackY: number
  ): number {
    return (
      (floor === 0
        ? this.floatSurfaceY(mount, pos.x, pos.z, fallbackY)
        : null) ??
      entityGroundY(this.heightManager, floor, pos.x, pos.z, fallbackY)
    )
  }

  /** Walks `budget` metres of `trail` from `pos` (mutated in place), dropping
   *  the points it reaches; returns the heading of the last leg walked. */
  private advanceAlongTrail(
    playerId: number,
    trail: Position[],
    pos: Position,
    budget: number
  ): number | undefined {
    let rotation: number | undefined
    while (budget > 0 && trail.length > 0) {
      const next = trail[0]
      const dx = shortestWrappedDeltaX(pos.x, next.x)
      const dz = next.z - pos.z
      const distance = Math.sqrt(dx * dx + dz * dz)
      if (distance > 0) rotation = Math.atan2(dx, dz)
      const t = distance <= budget ? 1 : budget / distance
      pos.x += dx * t
      pos.z += dz * t
      budget -= distance * t
      if (t === 1) trail.shift()
    }
    if (trail.length === 0) this.trails.delete(playerId)
    return rotation
  }

  private extendTrail(playerId: number, from: Position, position: Position) {
    let trail = this.trails.get(playerId)
    const last = trail?.[trail.length - 1] ?? from
    if (!movedFar(last, position, 0.01)) return
    if (!trail) this.trails.set(playerId, (trail = []))
    trail.push({ ...position })
    if (trail.length > MAX_TRAIL_POINTS) trail.shift()
  }

  // Initialize remote player state with position and rotation
  initPlayer(playerId: number, position: Position, rotation: number) {
    this.trails.delete(playerId)
    this.targetPositions.set(playerId, { ...position })
    this.targetSprinting.delete(playerId)
    this.seedHitCounter(playerId)
    this.players.set(playerId, {
      position: { ...position },
      state: 'idle',
      speed: 0,
      rotation,
    })
  }

  // Clean up data for players that have left
  removePlayer(playerId: number) {
    this.players.delete(playerId)
    this.movementData.delete(playerId)
    this.targetPositions.delete(playerId)
    this.targetRotations.delete(playerId)
    this.targetSprinting.delete(playerId)
    this.attackQueue.delete(playerId)
    this.trails.delete(playerId)
    this.attackStartTimes.delete(playerId)
    this.hitCounters.delete(playerId)
  }

  // Reset all data
  reset() {
    this.players.clear()
    this.movementData.clear()
    this.targetPositions.clear()
    this.targetRotations.clear()
    this.targetSprinting.clear()
    this.attackQueue.clear()
    this.trails.clear()
    this.attackStartTimes.clear()
    this.hitCounters.clear()
  }

  /** Seed the key so per-player reads never fall back to the map-wide signal.
   *  Never overwrites: a reset count would read as a hit and fire the flinch. */
  private seedHitCounter(playerId: number) {
    if (!this.hitCounters.has(playerId)) this.hitCounters.set(playerId, 0)
  }

  /** Bump the flinch counter for a remote taking a blow. */
  handleHit(playerId: number) {
    if (!this.players.has(playerId)) return
    this.hitCounters.set(playerId, (this.hitCounters.get(playerId) ?? 0) + 1)
  }

  handleDead(playerId: number) {
    const player = this.players.get(playerId)
    if (!player) return

    this.attackStartTimes.delete(playerId)
    this.players.set(playerId, {
      ...player,
      state: 'dead',
      speed: 0,
    })
  }

  handleRespawn(playerId: number, position: Position, rotation: number) {
    this.trails.delete(playerId)
    this.movementData.delete(playerId)
    this.attackQueue.delete(playerId)
    this.attackStartTimes.delete(playerId)
    this.targetPositions.set(playerId, { ...position })
    this.targetSprinting.delete(playerId)
    this.players.set(playerId, {
      position: { ...position },
      state: 'idle',
      speed: 0,
      rotation,
    })
  }

  /// The spectator mirror fell too far behind to interpolate back, so catch
  /// the drawn body up to where the server says it is. Not a teleport: nothing
  /// happened to the character, the watcher just lost the race. Snapping
  /// through `teleportPlayer` reset the state to idle and forgot the sprint
  /// flag, so the idle clip played over a body still crossing the ground, and
  /// the next leg was interpolated at footpace while the server carried it off
  /// at sprint or horse speed — which put it behind again, and snapped again.
  catchUpPlayer(
    playerId: number,
    position: Position,
    rotation: number,
    sprinting: boolean
  ) {
    const player = this.players.get(playerId)
    if (!player) {
      this.teleportPlayer(playerId, position, rotation)
      return
    }
    this.targetPositions.set(playerId, { ...position })
    this.targetRotations.set(playerId, rotation)
    this.targetSprinting.set(playerId, sprinting)
    this.movementData.delete(playerId)
    this.players.set(playerId, {
      ...player,
      position: { ...position },
      rotation,
    })
  }

  teleportPlayer(playerId: number, position: Position, rotation: number) {
    this.trails.delete(playerId)
    this.targetPositions.set(playerId, { ...position })
    this.seedHitCounter(playerId)
    this.targetSprinting.delete(playerId)
    this.movementData.delete(playerId)
    this.players.set(playerId, {
      position: { ...position },
      state: 'idle',
      speed: 0,
      rotation,
    })
  }

  handleInteraction(
    playerId: number,
    anim: string,
    offsetY: number,
    rotation?: number
  ) {
    const player = this.players.get(playerId)
    if (!player) return

    const newState: PlayerState = {
      ...player,
      state: 'interact',
      speed: 0,
      interactionAnim: anim,
      interactionCounter: (player.interactionCounter ?? 0) + 1,
      interactOffsetY: offsetY,
    }
    if (rotation !== undefined) {
      newState.rotation = rotation
      this.targetRotations.set(playerId, rotation)
    }
    this.players.set(playerId, newState)
  }

  setAuthoritativePose(playerId: number, position: Position, rotation: number) {
    const player = this.players.get(playerId)
    if (!player) return
    this.trails.delete(playerId)
    this.movementData.delete(playerId)
    this.targetPositions.set(playerId, { ...position })
    this.targetRotations.set(playerId, rotation)
    this.players.set(playerId, {
      ...player,
      position: { ...position },
      rotation,
      speed: 0,
    })
  }

  /** A remote's one-shot interact clip ended. A finished fishing cast hands
   *  over to the looping fishing idle; anything else drops back to idle. */
  handleInteractionFinished(playerId: number) {
    const player = this.players.get(playerId)
    if (player?.state !== 'interact') return
    if (player.interactionAnim === FishingAnimationName.CAST) {
      this.handleInteraction(playerId, FishingAnimationName.IDLE, 0)
    } else {
      this.handleStopInteraction(playerId)
    }
  }

  handleStopInteraction(playerId: number) {
    const player = this.players.get(playerId)
    if (!player || player.state !== 'interact') return

    // A seated player stands up first; that clip's finish lands back here.
    if (player.interactionAnim === SitAnimationName.SIT) {
      this.handleInteraction(
        playerId,
        SitAnimationName.SIT_TO_STAND,
        player.interactOffsetY ?? 0
      )
      return
    }

    this.players.set(playerId, {
      ...player,
      state: 'idle',
      interactionAnim: undefined,
      interactOffsetY: undefined,
    })
  }

  handleAttack(playerId: number) {
    const player = this.players.get(playerId)
    if (!player) return

    const movement = this.movementData.get(playerId)
    const isMoving = movement && movement.currentSpeed > 0.01

    if (isMoving) {
      // Still moving - queue the attack
      console.log(`[RemotePlayerManager] Queueing attack for ${playerId}`)
      const queue = this.attackQueue.get(playerId) || []
      queue.push('attack') // Currently monsterId isn't stored in PlayerState, so just queue an 'attack' event
      this.attackQueue.set(playerId, queue)
    } else {
      // Not moving - execute immediately
      this.executeAttack(playerId)
    }
  }

  /// `sprinting` left out means "however this body was already travelling" —
  /// the observer's route handover (GameScene) continues a leg the server is
  /// still running, and defaulting that to a walk dropped a sprinting mirror
  /// to footpace halfway round an obstacle.
  setTargetPosition(
    playerId: number,
    targetPosition: Position,
    rotation: number,
    sprinting = this.targetSprinting.get(playerId) ?? false
  ) {
    const player = this.players.get(playerId)

    // The observed character's agent walks off the moment its target drops;
    // waiting for the attack clip to finish and replaying a trail is how the
    // mirror fell behind, so cancel the attack immediately instead of
    // trailing it like any other frozen player.
    if (
      player?.state === 'attack' &&
      playerId === observedPlayerId() &&
      movedFar(player.position, targetPosition)
    ) {
      this.attackStartTimes.delete(playerId)
      this.trails.delete(playerId)
      this.players.set(playerId, { ...player, state: 'idle' })
    } else if (
      player &&
      (FROZEN_STATES.has(player.state) || this.trails.has(playerId))
    ) {
      this.extendTrail(playerId, player.position, targetPosition)
      // A face-only move precedes a swing and arrives mid-clip; the trail
      // only replays position once unfrozen, so the facing has to be applied
      // now or every repeat swing points at the first target.
      if (player.state === 'attack') {
        this.players.set(playerId, { ...player, rotation })
      }
    }

    // The picker's own client drops the crouch the moment it moves, so a move
    // that actually goes somewhere ends it here too — otherwise the remote
    // stays frozen for the rest of a clip the picker already abandoned. Held
    // poses (bench, forge) are left to their StopInteraction. The distance
    // gate ignores the resting flush sent when the walk to the item ends.
    if (
      player?.state === 'interact' &&
      player.interactionAnim === 'pickup' &&
      movedFar(player.position, targetPosition)
    ) {
      this.handleStopInteraction(playerId)
    }

    this.targetPositions.set(playerId, { ...targetPosition })
    this.targetRotations.set(playerId, rotation)
    this.targetSprinting.set(playerId, sprinting)
  }

  private executeAttack(playerId: number) {
    const player = this.players.get(playerId)
    if (!player) return

    // Swing facing where the server last said, not where the interpolator had
    // got to. The face-only move that precedes an attack arrives in the same
    // batch as the attack itself, so its rotation may not have reached the
    // drawn body through update() yet — and once the swing starts, update()
    // skips this player entirely.
    this.players.set(playerId, {
      ...player,
      state: 'attack',
      rotation: this.targetRotations.get(playerId) ?? player.rotation,
      attackCounter: (player.attackCounter ?? 0) + 1,
    })
    this.attackStartTimes.set(playerId, performance.now())
  }
}

export const remotePlayerManager = hmrSingleton(
  'remotePlayerManager',
  () => new PlayerStateManager()
)
