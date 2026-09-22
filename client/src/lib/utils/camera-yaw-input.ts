import { isTypingTarget } from './dom'

/** Radians per pixel of middle-button drag: a 400px sweep is a half turn. */
export const DRAG_YAW_PER_PX = Math.PI / 400
/** Radians per pixel of horizontal scroll (a trackpad's two-finger swipe). */
export const SWIPE_YAW_PER_PX = Math.PI / 600
/** Radians per second while a turn key is held. */
export const KEY_YAW_PER_SEC = Math.PI / 2
/** A middle click that moves less than this resets the view instead. */
const CLICK_SLOP_PX = 4
const LINE_HEIGHT_PX = 16

const TURN_KEYS: Record<string, number> = { Comma: 1, Period: -1 }
const RESET_KEY = 'Slash'

/**
 * Yaw for a wheel event that is a sideways swipe, or null for a zoom. A
 * trackpad reports two-finger swipes as wheel deltas and pinches as wheel
 * events with ctrlKey set; mice report vertical deltas, or horizontal ones
 * with Shift held.
 */
export function swipeYaw(event: {
  deltaX: number
  deltaY: number
  deltaMode: number
  ctrlKey: boolean
}): number | null {
  if (event.ctrlKey) return null
  if (Math.abs(event.deltaX) <= Math.abs(event.deltaY)) return null
  const scale = event.deltaMode === 1 ? LINE_HEIGHT_PX : 1
  return event.deltaX * scale * SWIPE_YAW_PER_PX
}

/** Rotate a camera offset about Y; `atan2(x, z)` grows by `yaw`. */
export function rotateOffset<T extends { x: number; z: number }>(
  offset: T,
  yaw: number
): T {
  const c = Math.cos(yaw)
  const s = Math.sin(yaw)
  return {
    ...offset,
    x: offset.x * c + offset.z * s,
    z: -offset.x * s + offset.z * c,
  }
}

export interface CameraYawControls {
  rotate(yaw: number): void
  reset(): void
  /** False while something else owns the camera (editors, a panel). */
  enabled(): boolean
}

/**
 * Turn the camera about the player: middle-button drag, a sideways swipe or
 * Shift+wheel, or the `,` and `.` keys; a middle click or `/` resets.
 * Returns `tick(dtMs)` for held keys and a cleanup.
 *
 * `host` is the element OrbitControls listens on. The wheel is taken in its
 * capture phase so a swipe is claimed before OrbitControls zooms on it,
 * whatever element inside the scene the pointer is over.
 */
export function attachCameraYawInput(
  host: HTMLElement,
  controls: CameraYawControls
): { tick(dtMs: number): void; detach(): void } {
  let dragFrom: number | null = null
  let dragged = 0
  const held = new Set<string>()

  const onPointerDown = (event: PointerEvent) => {
    if (event.button !== 1 || !controls.enabled()) return
    event.preventDefault()
    dragFrom = event.clientX
    dragged = 0
    host.setPointerCapture(event.pointerId)
  }
  const onPointerMove = (event: PointerEvent) => {
    if (dragFrom === null) return
    const dx = event.clientX - dragFrom
    dragFrom = event.clientX
    dragged += Math.abs(dx)
    if (dx !== 0) controls.rotate(-dx * DRAG_YAW_PER_PX)
  }
  const onPointerUp = (event: PointerEvent) => {
    if (dragFrom === null || event.button !== 1) return
    dragFrom = null
    if (host.hasPointerCapture(event.pointerId))
      host.releasePointerCapture(event.pointerId)
    if (dragged < CLICK_SLOP_PX) controls.reset()
  }
  const onWheel = (event: WheelEvent) => {
    if (!controls.enabled()) return
    const yaw = swipeYaw(event)
    if (yaw === null) return
    event.preventDefault()
    event.stopImmediatePropagation()
    controls.rotate(yaw)
  }
  const onKeyDown = (event: KeyboardEvent) => {
    if (isTypingTarget(event.target) || !controls.enabled()) return
    if (event.code === RESET_KEY && !event.repeat) controls.reset()
    else if (event.code in TURN_KEYS) held.add(event.code)
  }
  const onKeyUp = (event: KeyboardEvent) => held.delete(event.code)
  const onBlur = () => held.clear()

  host.addEventListener('pointerdown', onPointerDown)
  host.addEventListener('pointermove', onPointerMove)
  host.addEventListener('pointerup', onPointerUp)
  host.addEventListener('wheel', onWheel, { capture: true, passive: false })
  window.addEventListener('keydown', onKeyDown)
  window.addEventListener('keyup', onKeyUp)
  window.addEventListener('blur', onBlur)

  return {
    tick(dtMs) {
      if (held.size === 0 || !controls.enabled()) return
      let turn = 0
      for (const code of held) turn += TURN_KEYS[code]
      if (turn !== 0) controls.rotate(turn * KEY_YAW_PER_SEC * (dtMs / 1000))
    },
    detach() {
      host.removeEventListener('pointerdown', onPointerDown)
      host.removeEventListener('pointermove', onPointerMove)
      host.removeEventListener('pointerup', onPointerUp)
      host.removeEventListener('wheel', onWheel, { capture: true })
      window.removeEventListener('keydown', onKeyDown)
      window.removeEventListener('keyup', onKeyUp)
      window.removeEventListener('blur', onBlur)
    },
  }
}
