/**
 * Where to put the camera for a model of a given height.
 *
 * Kept apart from the viewport so the relationship that actually matters can be
 * checked: the camera has to end up further from the model than the near plane.
 * A fixed 0.05 near plane and a distance derived from the height put the camera
 * inside a 2 cm import — which is what a raw Meshy export measures — and the
 * viewport rendered nothing at all.
 */
export interface CameraFraming {
  target: [number, number, number]
  position: [number, number, number]
  near: number
  far: number
}

/** Below this the framing stops shrinking, so a degenerate model still shows. */
export const MIN_FRAMED_HEIGHT = 0.02

export function frameFor(height: number): CameraFraming {
  const size = Math.max(Number.isFinite(height) ? height : 1, MIN_FRAMED_HEIGHT)
  const focus = size * 0.5
  const distance = size * 2.2

  return {
    target: [0, focus, 0],
    position: [distance * 0.45, focus + size * 0.35, distance],
    near: Math.max(0.0005, size / 200),
    far: Math.max(60, size * 400),
  }
}

export function distanceToTarget(framing: CameraFraming): number {
  return Math.hypot(
    framing.position[0] - framing.target[0],
    framing.position[1] - framing.target[1],
    framing.position[2] - framing.target[2]
  )
}
