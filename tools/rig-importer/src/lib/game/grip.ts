/**
 * The weapon grip transform, as monsters.csv carries it.
 *
 * `weaponOffset` is the metres along the bone's local +Y that the shipped
 * monsters were tuned with; `weaponOffsetX`/`weaponOffsetZ` settle the weapon
 * sideways into the palm, and `weaponRotation` turns it. Rotation lives in the
 * CSV as `rx|ry|rz` degrees — these files cannot carry a comma, and `|` is
 * already how `animAttack` lists alternatives.
 *
 * The client reads the same columns in `Monster.svelte`, applying them as a
 * plain position and an XYZ Euler on the weapon parented to the bone.
 */
export interface Grip {
  x: number
  y: number
  z: number
  /** Degrees, about the bone's local axes. */
  rx: number
  ry: number
  rz: number
}

export const NO_GRIP: Grip = { x: 0, y: 0, z: 0, rx: 0, ry: 0, rz: 0 }

export const GRIP_COLUMNS = ['weaponOffset', 'weaponOffsetX', 'weaponOffsetZ', 'weaponRotation'] as const

export function parseRotation(value: string | undefined): [number, number, number] {
  if (!value) return [0, 0, 0]
  const parts = value.split('|').map((part) => Number(part.trim()))
  return [0, 1, 2].map((axis) => (Number.isFinite(parts[axis]) ? parts[axis] : 0)) as [number, number, number]
}

/** Empty when the grip is unrotated, so unused columns stay blank. */
export function formatRotation(rx: number, ry: number, rz: number): string {
  if ([rx, ry, rz].every((angle) => Math.abs(angle) < 1e-6)) return ''
  return [rx, ry, rz].map((angle) => round(angle, 1)).join('|')
}

export function gripFromCsv(values: Record<string, string>): Grip {
  const [rx, ry, rz] = parseRotation(values.weaponRotation)
  return {
    x: number(values.weaponOffsetX),
    y: number(values.weaponOffset),
    z: number(values.weaponOffsetZ),
    rx,
    ry,
    rz,
  }
}

export function gripToCsv(grip: Grip): Record<string, string> {
  return {
    weaponOffset: text(grip.y),
    weaponOffsetX: text(grip.x),
    weaponOffsetZ: text(grip.z),
    weaponRotation: formatRotation(grip.rx, grip.ry, grip.rz),
  }
}

/** Radians in XYZ order, matching the Euler the client builds. */
export function gripRadians(grip: Grip): [number, number, number] {
  const toRadians = Math.PI / 180
  return [grip.rx * toRadians, grip.ry * toRadians, grip.rz * toRadians]
}

export function isRotated(grip: Grip): boolean {
  return [grip.rx, grip.ry, grip.rz].some((angle) => Math.abs(angle) > 1e-6)
}

function number(value: string | undefined): number {
  const parsed = Number(value ?? '')
  return Number.isFinite(parsed) ? parsed : 0
}

/** Blank rather than "0", so an unused column reads as unused. */
function text(value: number): string {
  return Math.abs(value) < 1e-6 ? '' : String(round(value, 3))
}

function round(value: number, digits: number): number {
  const factor = 10 ** digits
  return Math.round(value * factor) / factor
}
