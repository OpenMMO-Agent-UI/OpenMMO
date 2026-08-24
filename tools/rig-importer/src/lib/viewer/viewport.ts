/**
 * The 3D pane. It is mounted once and lives for the whole session — every step
 * talks to this, nothing remounts it.
 *
 * The subject is always loaded from the container's current bytes, so what is on
 * screen is what would be written to disk.
 */
import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js'
import { frameFor } from './framing'
import { LIGHTING_PRESETS, presetById, type LightingPreset } from './lighting'

/** Position in metres and rotation in radians, both in the bone's local space. */
export interface WeaponGrip {
  position: [number, number, number]
  rotation: [number, number, number]
}

export interface LoadedModel {
  scene: THREE.Group
  clips: THREE.AnimationClip[]
  height: number
}

export class Viewport {
  readonly renderer: THREE.WebGLRenderer
  readonly scene = new THREE.Scene()
  readonly camera: THREE.PerspectiveCamera
  readonly controls: OrbitControls

  private readonly loader = new GLTFLoader()
  private readonly lightHolder = new THREE.Group()
  private readonly subjectHolder = new THREE.Group()
  private readonly referenceHolder = new THREE.Group()
  private readonly rulerHolder = new THREE.Group()
  private readonly ground: THREE.Mesh
  private readonly grid: THREE.GridHelper

  private mixer: THREE.AnimationMixer | null = null
  private action: THREE.AnimationAction | null = null
  private subject: LoadedModel | null = null
  private weapon: THREE.Object3D | null = null
  private frame = 0
  private readonly clock = new THREE.Clock()
  private preset: LightingPreset = LIGHTING_PRESETS[0]
  private observer: ResizeObserver | null = null

  constructor(private readonly host: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: false })
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, 2))
    this.renderer.outputColorSpace = THREE.SRGBColorSpace
    host.appendChild(this.renderer.domElement)

    this.camera = new THREE.PerspectiveCamera(38, 1, 0.05, 200)
    this.camera.position.set(2.4, 1.6, 3.2)

    this.controls = new OrbitControls(this.camera, this.renderer.domElement)
    this.controls.enableDamping = true
    this.controls.target.set(0, 1, 0)

    this.ground = new THREE.Mesh(
      new THREE.CircleGeometry(6, 64).rotateX(-Math.PI / 2),
      new THREE.MeshStandardMaterial({ roughness: 0.95, metalness: 0 })
    )
    this.ground.position.y = -0.001
    this.grid = new THREE.GridHelper(12, 24, 0x4a5568, 0x2c3340)
    ;(this.grid.material as THREE.Material).transparent = true
    ;(this.grid.material as THREE.Material).opacity = 0.35

    this.scene.add(this.lightHolder, this.subjectHolder, this.referenceHolder, this.rulerHolder, this.ground, this.grid)
    this.setLighting('studio')
    this.observe()
    this.tick()
  }

  private observe(): void {
    const resize = () => {
      const { clientWidth, clientHeight } = this.host
      if (clientWidth === 0 || clientHeight === 0) return
      this.renderer.setSize(clientWidth, clientHeight, false)
      this.camera.aspect = clientWidth / clientHeight
      this.camera.updateProjectionMatrix()
    }
    this.observer = new ResizeObserver(resize)
    this.observer.observe(this.host)
    resize()
  }

  private tick = (): void => {
    this.frame = requestAnimationFrame(this.tick)
    const delta = this.clock.getDelta()
    this.mixer?.update(delta)
    this.controls.update()
    this.renderer.render(this.scene, this.camera)
  }

  setLighting(id: string): void {
    this.preset = presetById(id)
    this.lightHolder.clear()
    this.lightHolder.add(this.preset.build())
    this.scene.background = new THREE.Color(this.preset.background)
    ;(this.ground.material as THREE.MeshStandardMaterial).color.setHex(this.preset.ground)
  }

  get lightingId(): string {
    return this.preset.id
  }

  /**
   * Swap in a new build of the model. The camera is only reframed for a subject
   * the viewport has not seen before — every settings change rebuilds the GLB,
   * and snapping the view back each time would make the sliders unusable.
   */
  async loadSubject(bytes: Uint8Array, reframe = this.subject === null): Promise<LoadedModel> {
    const model = await this.parse(bytes)
    this.stopClip()
    disposeTree(this.subjectHolder)
    this.subjectHolder.clear()
    this.subjectHolder.add(model.scene)
    this.subject = model
    this.mixer = new THREE.AnimationMixer(model.scene)
    if (reframe) this.frameCamera(model.height)
    this.drawRuler(model.height)
    return model
  }

  /** Forget the current subject, so the next load reframes on it. */
  clearSubject(): void {
    this.stopClip()
    disposeTree(this.subjectHolder)
    this.subjectHolder.clear()
    this.subject = null
    this.mixer = null
  }

  private async parse(bytes: Uint8Array): Promise<LoadedModel> {
    const copy = bytes.slice().buffer as ArrayBuffer
    const gltf = await this.loader.parseAsync(copy, '')
    const scene = gltf.scene as THREE.Group
    const box = new THREE.Box3().setFromObject(scene)
    return { scene, clips: gltf.animations ?? [], height: box.max.y - box.min.y }
  }

  /** Put the whole model on screen, whatever size it came in at. */
  frameCamera(height = this.subject?.height ?? 1): void {
    const framing = frameFor(height)
    this.controls.target.set(...framing.target)
    this.camera.position.set(...framing.position)
    this.camera.near = framing.near
    this.camera.far = framing.far
    this.camera.updateProjectionMatrix()
    this.controls.update()
  }

  /** Ticks every half metre, plus a marker at the subject's crown. */
  private drawRuler(height: number): void {
    disposeTree(this.rulerHolder)
    this.rulerHolder.clear()

    const points: number[] = []
    const top = Math.ceil(Math.max(height, 1) * 2) / 2
    for (let y = 0; y <= top + 0.001; y += 0.5) {
      points.push(-1.35, y, 0, -1.15, y, 0)
      this.rulerHolder.add(label(`${y.toFixed(1)} m`, new THREE.Vector3(-1.62, y, 0), 0x7c8698))
    }
    points.push(-1.35, 0, 0, -1.35, top, 0)
    points.push(-1.35, height, 0, 0.9, height, 0)

    const line = new THREE.LineSegments(
      new THREE.BufferGeometry().setAttribute('position', new THREE.Float32BufferAttribute(points, 3)),
      new THREE.LineBasicMaterial({ color: 0x5c6675 })
    )
    this.rulerHolder.add(line)
    this.rulerHolder.add(label(`${height.toFixed(2)} m`, new THREE.Vector3(1.35, height, 0), 0xe6b25c))
  }

  /** Flat silhouettes of shipped models, for judging scale against the game. */
  async setReferences(paths: { url: string; x: number }[]): Promise<void> {
    disposeTree(this.referenceHolder)
    this.referenceHolder.clear()

    for (const { url, x } of paths) {
      try {
        const gltf = await this.loader.loadAsync(url)
        const silhouette = gltf.scene
        silhouette.traverse((object) => {
          const mesh = object as THREE.Mesh
          if (!mesh.isMesh) return
          mesh.material = new THREE.MeshBasicMaterial({
            color: 0x1b2029,
            transparent: true,
            opacity: 0.55,
            depthWrite: false,
          })
        })
        silhouette.position.x = x
        this.referenceHolder.add(silhouette)
      } catch {
        // A missing reference model is not worth failing the step over.
      }
    }
  }

  setReferencesVisible(visible: boolean): void {
    this.referenceHolder.visible = visible
  }

  playClip(clip: THREE.AnimationClip | null): void {
    this.stopClip()
    if (!clip || !this.mixer) return
    this.action = this.mixer.clipAction(clip)
    this.action.reset().play()
  }

  stopClip(): void {
    this.action?.stop()
    this.action = null
    this.mixer?.stopAllAction()
  }

  get subjectScene(): THREE.Object3D | null {
    return this.subject?.scene ?? null
  }

  get subjectClips(): THREE.AnimationClip[] {
    return this.subject?.clips ?? []
  }

  /**
   * Hang a weapon off a bone with the grip transform the game applies: a plain
   * position and an XYZ Euler on a child of the bone (see Monster.svelte).
   */
  async setWeapon(url: string | null, boneName: string, grip: WeaponGrip): Promise<boolean> {
    this.weapon?.removeFromParent()
    this.weapon = null
    if (!url || !this.subject) return false

    const bone = this.subject.scene.getObjectByName(boneName)
    if (!bone) return false

    const gltf = await this.loader.loadAsync(url)
    bone.add(gltf.scene)
    this.weapon = gltf.scene
    this.moveWeapon(grip)
    return true
  }

  moveWeapon(grip: WeaponGrip): void {
    if (!this.weapon) return
    this.weapon.position.set(grip.position[0], grip.position[1], grip.position[2])
    this.weapon.rotation.set(grip.rotation[0], grip.rotation[1], grip.rotation[2])
  }

  /** World scale of a bone — 1 unless the rig kept a scale on a node. */
  boneScale(boneName: string): number {
    const bone = this.subject?.scene.getObjectByName(boneName)
    if (!bone) return 1
    bone.updateWorldMatrix(true, false)
    return bone.getWorldScale(new THREE.Vector3()).x
  }

  screenshot(): string {
    this.renderer.render(this.scene, this.camera)
    return this.renderer.domElement.toDataURL('image/png')
  }

  dispose(): void {
    cancelAnimationFrame(this.frame)
    this.observer?.disconnect()
    this.controls.dispose()
    disposeTree(this.scene)
    this.renderer.dispose()
    this.renderer.domElement.remove()
  }
}

function label(text: string, position: THREE.Vector3, color: number): THREE.Sprite {
  const canvas = document.createElement('canvas')
  canvas.width = 256
  canvas.height = 64
  const ctx = canvas.getContext('2d')!
  ctx.font = '600 34px ui-monospace, SFMono-Regular, monospace'
  ctx.fillStyle = `#${color.toString(16).padStart(6, '0')}`
  ctx.textAlign = 'center'
  ctx.textBaseline = 'middle'
  ctx.fillText(text, 128, 34)

  const texture = new THREE.CanvasTexture(canvas)
  texture.colorSpace = THREE.SRGBColorSpace
  const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: texture, depthTest: false, transparent: true }))
  sprite.position.copy(position)
  sprite.scale.set(0.5, 0.125, 1)
  sprite.renderOrder = 10
  return sprite
}

function disposeTree(root: THREE.Object3D): void {
  root.traverse((object) => {
    const mesh = object as THREE.Mesh
    if (mesh.geometry) mesh.geometry.dispose()
    const material = (mesh as THREE.Mesh).material
    for (const entry of Array.isArray(material) ? material : [material]) {
      if (!entry) continue
      for (const value of Object.values(entry)) {
        if (value instanceof THREE.Texture) value.dispose()
      }
      entry.dispose()
    }
  })
}
