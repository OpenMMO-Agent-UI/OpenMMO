/**
 * The contact sheet: every rig on screen at once, in one WebGL context.
 *
 * A renderer per cell would be the obvious build and it does not survive
 * contact with 28 rigs — browsers cap live WebGL contexts around 16 and start
 * dropping the oldest. So there is one canvas stretched over the page and one
 * renderer, and each frame walks the visible cells, scissors the renderer to
 * that cell's rectangle on screen, and draws that cell's scene into it. A cell
 * scrolled out of view costs nothing.
 *
 * The camera is deliberately shared. Judging whether a retarget stretches a
 * limb means seeing every rig from the same angle, so one azimuth, elevation
 * and dolly drive all of them; only the distance differs, scaled by each
 * model's height so a kobold and a troll are framed the same.
 */
import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'

export interface CellModel {
  scene: THREE.Group
  height: number
  clips: THREE.AnimationClip[]
}

interface Cell {
  id: string
  element: HTMLElement
  scene: THREE.Scene
  camera: THREE.PerspectiveCamera
  model: CellModel | null
  mixer: THREE.AnimationMixer | null
  action: THREE.AnimationAction | null
  visible: boolean
}

export interface OrbitState {
  azimuth: number
  elevation: number
  /** Multiplier on the framing distance. 1 fits the model. */
  dolly: number
}

const BACKGROUND = 0x0e1621
const GROUND = 0x141f2b

export class ContactSheet {
  readonly renderer: THREE.WebGLRenderer
  private readonly loader = new GLTFLoader()
  private readonly cells = new Map<string, Cell>()
  private readonly clock = new THREE.Clock()
  private frame = 0
  private orbit: OrbitState = { azimuth: 0.42, elevation: 0.18, dolly: 1 }
  /** The scrolling grid. Cells are clipped to it, not just to the viewport. */
  private clip: HTMLElement | null = null
  /** Loads in flight or done, so a rig is fetched and parsed once. */
  private readonly models = new Map<string, Promise<CellModel>>()

  constructor(private readonly canvas: HTMLCanvasElement) {
    this.renderer = new THREE.WebGLRenderer({ canvas, antialias: true })
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2))
    this.renderer.outputColorSpace = THREE.SRGBColorSpace
    this.renderer.setClearColor(BACKGROUND, 1)
    this.renderer.autoClear = false
    this.resize()
    this.tick()
  }

  resize(): void {
    const width = this.canvas.clientWidth
    const height = this.canvas.clientHeight
    if (width === 0 || height === 0) return
    this.renderer.setSize(width, height, false)
  }

  /**
   * Confine drawing to the scroll container.
   *
   * A cell scrolled up out of the grid still has a rectangle on screen, and
   * without this the renderer would happily draw it over the take strip and the
   * header — they are ordinary DOM above a canvas that does not know about them.
   */
  setClip(element: HTMLElement | null): void {
    this.clip = element
  }

  setOrbit(orbit: Partial<OrbitState>): void {
    this.orbit = { ...this.orbit, ...orbit }
    this.orbit.elevation = clamp(this.orbit.elevation, -0.45, 1.2)
    this.orbit.dolly = clamp(this.orbit.dolly, 0.35, 3)
  }

  get orbitState(): OrbitState {
    return { ...this.orbit }
  }

  register(id: string, element: HTMLElement): void {
    const existing = this.cells.get(id)
    if (existing) {
      existing.element = element
      return
    }
    const scene = new THREE.Scene()
    scene.add(buildLights(), buildGround())
    const camera = new THREE.PerspectiveCamera(34, 1, 0.05, 60)
    this.cells.set(id, { id, element, scene, camera, model: null, mixer: null, action: null, visible: true })
  }

  unregister(id: string): void {
    const cell = this.cells.get(id)
    if (!cell) return
    cell.action?.stop()
    cell.mixer?.stopAllAction()
    this.cells.delete(id)
  }

  setVisible(id: string, visible: boolean): void {
    const cell = this.cells.get(id)
    if (cell) cell.visible = visible
  }

  /** Fetch and parse a rig once, however many cells ask for it. */
  async load(id: string, url: string): Promise<CellModel> {
    let pending = this.models.get(url)
    if (!pending) {
      pending = this.loader.loadAsync(url).then((gltf) => {
        const scene = gltf.scene as THREE.Group
        const box = new THREE.Box3().setFromObject(scene)
        return { scene, height: box.max.y - box.min.y, clips: gltf.animations ?? [] }
      })
      this.models.set(url, pending)
    }
    const model = await pending
    const cell = this.cells.get(id)
    if (cell) {
      cell.model = model
      cell.scene.add(model.scene)
      cell.mixer = new THREE.AnimationMixer(model.scene)
    }
    return model
  }

  /**
   * Play a clip, or stop and return to the rest pose when given null.
   *
   * Every cell is restarted from zero by the caller in the same tick, which is
   * what keeps the sheet in step: the mixers all advance on one clock, so equal
   * start times stay equal.
   */
  play(id: string, clip: THREE.AnimationClip | null): void {
    const cell = this.cells.get(id)
    if (!cell?.mixer) return
    cell.action?.stop()
    cell.mixer.stopAllAction()
    cell.action = null
    if (!clip) {
      cell.mixer.setTime(0)
      return
    }
    cell.action = cell.mixer.clipAction(clip)
    cell.action.reset().play()
  }

  /** The rig's own scene, for retargeting a take onto it. */
  sceneFor(id: string): THREE.Object3D | null {
    return this.cells.get(id)?.model?.scene ?? null
  }

  private tick = (): void => {
    this.frame = requestAnimationFrame(this.tick)
    const delta = this.clock.getDelta()
    const canvasHeight = this.canvas.clientHeight
    const canvasWidth = this.canvas.clientWidth

    this.renderer.setScissorTest(false)
    this.renderer.clear()

    const bounds = this.clip?.getBoundingClientRect()
    const limitTop = Math.max(0, bounds?.top ?? 0)
    const limitBottom = Math.min(canvasHeight, bounds?.bottom ?? canvasHeight)
    const limitLeft = Math.max(0, bounds?.left ?? 0)
    const limitRight = Math.min(canvasWidth, bounds?.right ?? canvasWidth)

    for (const cell of this.cells.values()) {
      cell.mixer?.update(delta)
      if (!cell.visible || !cell.model) continue

      const rect = cell.element.getBoundingClientRect()
      if (rect.width < 2 || rect.height < 2) continue

      // The camera frames the cell's full rectangle; the scissor is the part of
      // it the grid actually shows. Framing off the clipped height instead would
      // make a half-scrolled rig squash as it goes.
      const left = Math.max(rect.left, limitLeft)
      const right = Math.min(rect.right, limitRight)
      const top = Math.max(rect.top, limitTop)
      const foot = Math.min(rect.bottom, limitBottom)
      if (right - left < 1 || foot - top < 1) continue

      this.renderer.setViewport(rect.left, canvasHeight - rect.bottom, rect.width, rect.height)
      this.renderer.setScissor(left, canvasHeight - foot, right - left, foot - top)
      this.renderer.setScissorTest(true)

      this.aim(cell, rect.width / rect.height)
      this.renderer.render(cell.scene, cell.camera)
    }
  }

  /** Same angle on every rig; distance scaled so each one fills its cell. */
  private aim(cell: Cell, aspect: number): void {
    const height = cell.model?.height ?? 1.8
    const { azimuth, elevation, dolly } = this.orbit
    const centre = height * 0.55
    // Fit the model's height into the vertical field of view, then widen the
    // pull-back for a narrow cell so a tall rig is not cropped side to side.
    const fov = (cell.camera.fov * Math.PI) / 180
    const fit = height / 2 / Math.tan(fov / 2)
    const distance = (fit / Math.min(1, aspect * 0.9)) * 1.5 * dolly

    cell.camera.aspect = aspect
    cell.camera.position.set(
      Math.sin(azimuth) * Math.cos(elevation) * distance,
      centre + Math.sin(elevation) * distance,
      Math.cos(azimuth) * Math.cos(elevation) * distance
    )
    cell.camera.lookAt(0, centre, 0)
    cell.camera.near = Math.max(0.02, distance * 0.02)
    cell.camera.far = distance * 8
    cell.camera.updateProjectionMatrix()
  }

  dispose(): void {
    cancelAnimationFrame(this.frame)
    for (const cell of this.cells.values()) {
      cell.action?.stop()
      cell.mixer?.stopAllAction()
    }
    this.cells.clear()
    this.renderer.dispose()
  }
}

function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value))
}

function buildLights(): THREE.Group {
  const group = new THREE.Group()
  const key = new THREE.DirectionalLight(0xfff4e6, 2.1)
  key.position.set(2.5, 4, 3)
  const fill = new THREE.DirectionalLight(0x9dc4ff, 0.8)
  fill.position.set(-3, 1.5, -2)
  group.add(key, fill, new THREE.HemisphereLight(0xdce6f0, 0x1a2635, 1.1))
  return group
}

function buildGround(): THREE.Mesh {
  const ground = new THREE.Mesh(
    new THREE.CircleGeometry(4, 48).rotateX(-Math.PI / 2),
    new THREE.MeshStandardMaterial({ color: GROUND, roughness: 1, metalness: 0 })
  )
  ground.position.y = -0.002
  return ground
}
