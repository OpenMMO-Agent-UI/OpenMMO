import {
  afterEach,
  beforeEach,
  describe,
  expect,
  it,
  vi,
  type Mock,
} from 'vitest'
import {
  attachCameraYawInput,
  DRAG_YAW_PER_PX,
  KEY_YAW_PER_SEC,
  rotateOffset,
  swipeYaw,
} from './camera-yaw-input'

function event<T extends object>(type: string, fields: T) {
  return Object.assign(new Event(type, { cancelable: true }), fields)
}

describe('swipeYaw', () => {
  it('turns on a sideways swipe and leaves zoom to vertical scrolls', () => {
    const swipe = { deltaY: 1, deltaMode: 0, ctrlKey: false }
    expect(swipeYaw({ ...swipe, deltaX: 30 })).toBeGreaterThan(0)
    expect(swipeYaw({ ...swipe, deltaX: -30 })).toBeLessThan(0)
    expect(swipeYaw({ ...swipe, deltaX: 0, deltaY: 30 })).toBeNull()
  })

  it('leaves a pinch to zoom', () => {
    expect(
      swipeYaw({ deltaX: 30, deltaY: 0, deltaMode: 0, ctrlKey: true })
    ).toBeNull()
  })

  it('scales line-mode deltas to pixels', () => {
    const lines = swipeYaw({
      deltaX: 3,
      deltaY: 0,
      deltaMode: 1,
      ctrlKey: false,
    })
    const pixels = swipeYaw({
      deltaX: 48,
      deltaY: 0,
      deltaMode: 0,
      ctrlKey: false,
    })
    expect(lines).toBeCloseTo(pixels!)
  })
})

describe('rotateOffset', () => {
  it('turns the offset about Y without changing its length or height', () => {
    const offset = { x: -10, y: 8, z: 10 }
    const turned = rotateOffset(offset, Math.PI / 2)
    expect(Math.atan2(turned.x, turned.z)).toBeCloseTo(
      Math.atan2(offset.x, offset.z) + Math.PI / 2
    )
    expect(Math.hypot(turned.x, turned.z)).toBeCloseTo(Math.hypot(10, 10))
    expect(turned.y).toBe(8)
  })
})

describe('attachCameraYawInput', () => {
  let host: HTMLElement
  let rotate: Mock<(yaw: number) => void>
  let reset: Mock<() => void>
  let enabled: boolean
  let detach: () => void
  let tick: (dtMs: number) => void

  beforeEach(() => {
    vi.stubGlobal('window', new EventTarget())
    vi.stubGlobal('HTMLElement', class {})
    host = Object.assign(new EventTarget(), {
      setPointerCapture: vi.fn(),
      releasePointerCapture: vi.fn(),
      hasPointerCapture: () => true,
    }) as unknown as HTMLElement
    rotate = vi.fn<(yaw: number) => void>()
    reset = vi.fn<() => void>()
    enabled = true
    ;({ tick, detach } = attachCameraYawInput(host, {
      rotate,
      reset,
      enabled: () => enabled,
    }))
  })

  afterEach(() => {
    detach()
    vi.unstubAllGlobals()
  })

  const pointer = (type: string, clientX: number) =>
    host.dispatchEvent(event(type, { button: 1, clientX, pointerId: 1 }))

  it('turns with a middle-button drag', () => {
    pointer('pointerdown', 100)
    pointer('pointermove', 140)
    pointer('pointerup', 140)

    expect(rotate).toHaveBeenCalledWith(-40 * DRAG_YAW_PER_PX)
    expect(reset).not.toHaveBeenCalled()
  })

  it('resets on a middle click that does not drag', () => {
    pointer('pointerdown', 100)
    pointer('pointerup', 101)

    expect(reset).toHaveBeenCalledOnce()
    expect(rotate).not.toHaveBeenCalled()
  })

  it('keeps a sideways swipe from also zooming', () => {
    const zoom = vi.fn()
    host.addEventListener('wheel', zoom)
    const swipe = event('wheel', {
      deltaX: 40,
      deltaY: 2,
      deltaMode: 0,
      ctrlKey: false,
    })
    host.dispatchEvent(swipe)

    expect(rotate).toHaveBeenCalledOnce()
    expect(swipe.defaultPrevented).toBe(true)
    expect(zoom).not.toHaveBeenCalled()
  })

  it('turns while a key is held and resets on slash', () => {
    const key = (type: string, code: string) =>
      window.dispatchEvent(event(type, { code, repeat: false }))
    key('keydown', 'Comma')
    tick(500)
    key('keyup', 'Comma')
    tick(500)
    key('keydown', 'Slash')

    expect(rotate).toHaveBeenCalledOnce()
    expect(rotate).toHaveBeenCalledWith(KEY_YAW_PER_SEC / 2)
    expect(reset).toHaveBeenCalledOnce()
  })

  it('does nothing while something else owns the camera', () => {
    enabled = false
    pointer('pointerdown', 100)
    pointer('pointermove', 140)
    host.dispatchEvent(
      event('wheel', { deltaX: 40, deltaY: 0, deltaMode: 0, ctrlKey: false })
    )

    expect(rotate).not.toHaveBeenCalled()
  })
})
