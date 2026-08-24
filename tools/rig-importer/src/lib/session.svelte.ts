/**
 * The wizard's state.
 *
 * The imported file is kept pristine and every setting change re-runs the whole
 * pipeline against it, so steps can be revisited in any order without the
 * result depending on the path taken to get there.
 */
import { guessBoneMapping, type BoneGuess, type Joint } from './bones/match'
import { CORE_BONES } from './bones/skeleton'
import { parseGlb, type GlbContainer } from './gltf/container'
import { accessorIsFloat, jointHeight, nodeParents } from './gltf/measure'
import { boneReachAlongY, restPoseBounds } from './gltf/transform'
import { needsRepair, readMaterials } from './gltf/materials'
import { importModelFile } from './io/import'
import { defaultSettings, runPipeline, type PipelineOutput, type PipelineSettings } from './pipeline'
import { gameplayDefaults, isValidId, suggestId } from './game/defaults'
import { ANIM_COLUMNS, sharedAnimDefaults, type AnimColumn, type AttackStyle } from './game/clips'
import { runSpeedFor, walkSpeedFor, weaponOffsetFor } from './game/rig'
import { gripFromCsv, gripToCsv } from './game/grip'
import { validate, type Finding } from './validate'
import type { DraftFile, DraftState, ImportSummary, SourceInfo } from './plan'
import type { ModelKind } from './game/paths'
import * as api from './api'

export const STEPS = [
  { id: 'start', label: 'Start', hint: 'New import, a saved draft, or an existing model' },
  { id: 'source', label: 'Source', hint: 'The file, and where it came from' },
  { id: 'skeleton', label: 'Skeleton', hint: 'Map the rig onto the game bone names' },
  { id: 'size', label: 'Size & origin', hint: 'Height in metres, feet on the floor' },
  { id: 'material', label: 'Material', hint: 'Undo importer damage, fit the texture budget' },
  { id: 'animation', label: 'Animation', hint: 'Own clips, or the shared packs retargeted' },
  { id: 'weapon', label: 'Weapon', hint: 'Where a weapon sits in the hand' },
  { id: 'data', label: 'Game data', hint: 'The monsters.csv row' },
  { id: 'validate', label: 'Validate', hint: 'What blocks, and what needs a decision' },
  { id: 'apply', label: 'Review & apply', hint: 'See the diff, then write it' },
] as const

export type StepId = (typeof STEPS)[number]['id']

function emptySource(): SourceInfo {
  return {
    generator: '',
    tier: '',
    generatedOn: new Date().toISOString().slice(0, 10),
    sourceName: '',
    rigger: 'mixamo.com',
    conceptSource: '',
    license: '',
    notes: '',
    sourceFileName: '',
  }
}

export class Session {
  repo = $state<api.RepoInfo | null>(null)
  drafts = $state<DraftState[]>([])

  draftId = $state('')
  kind = $state<ModelKind>('monster')
  id = $state('')
  displayName = $state('')
  replacingExisting = $state(false)
  step = $state<StepId>('start')

  source = $state<SourceInfo>(emptySource())
  settings = $state<PipelineSettings>(defaultSettings())
  csvValues = $state<Record<string, string>>({})
  acknowledged = $state<string[]>([])

  attackStyle = $state<AttackStyle>('weapon')
  sharedAnims = $state(true)
  weaponId = $state('')
  lighting = $state('studio')
  showReferences = $state(true)

  /**
   * The file exactly as it came in, and the container the last run produced.
   *
   * Neither is `$state`. Svelte deep-proxies plain objects, and a proxied glTF
   * json cannot be structuredClone'd — it also puts a proxy hop in front of
   * every one of the millions of accessor reads a measurement does. Reactivity
   * comes from `loaded`, `subjectGeneration` and `result` instead.
   */
  original: GlbContainer | null = null
  container: GlbContainer | null = null
  originalBytes: Uint8Array | null = null

  /** True once a model is in. Reactive stand-in for `original !== null`. */
  loaded = $state(false)
  convertedFromFbx = $state(false)
  importNotes = $state<string[]>([])
  sourceFileBytes: Uint8Array | null = null
  conceptBytes = $state<Uint8Array | null>(null)

  result = $state<PipelineOutput | null>(null)
  /** Bumped when a different model is loaded, so the preview reframes on it. */
  subjectGeneration = $state(0)
  busy = $state<string | null>(null)
  error = $state<string | null>(null)
  savedAt = $state('')

  #token = 0
  #pending: ReturnType<typeof setTimeout> | null = null

  get joints(): Joint[] {
    void this.subjectGeneration // the container is not reactive; this is
    const container = this.original
    if (!container) return []
    const parents = nodeParents(container)
    const nodes = new Set<number>()
    for (const skin of container.json.skins ?? []) for (const joint of skin.joints) nodes.add(joint)
    return [...nodes].map((node) => ({
      node,
      name: container.json.nodes![node].name ?? `node_${node}`,
      parent: parents[node],
    }))
  }

  get mappedBones(): string[] {
    return this.settings.boneMapping.filter((g) => g.node !== null).map((g) => g.standard)
  }

  get missingCore(): string[] {
    const mapped = new Set(this.mappedBones)
    return CORE_BONES.filter((bone) => !mapped.has(bone))
  }

  get sourceHeight(): number {
    void this.subjectGeneration
    if (!this.original) return 0
    const bounds = restPoseBounds(this.original)
    return bounds.max[1] - bounds.min[1]
  }

  get hipsHeight(): number {
    const container = this.result ? this.container : null
    if (!container) return 0
    const hips = (container.json.nodes ?? []).findIndex((node) => node.name === 'Hips')
    return hips < 0 ? 0 : jointHeight(container, hips)
  }

  get handReach(): number {
    const container = this.result ? this.container : null
    if (!container) return 0
    const hand = (container.json.nodes ?? []).findIndex((node) => node.name === 'RightHand')
    return hand < 0 ? 0 : boneReachAlongY(container, hand)
  }

  get materialsNeedRepair(): boolean {
    void this.subjectGeneration
    return this.original ? readMaterials(this.original).some(needsRepair) : false
  }

  get clipAssignments(): Partial<Record<AnimColumn, string>> {
    const out: Partial<Record<AnimColumn, string>> = {}
    for (const column of ANIM_COLUMNS) {
      const value = this.csvValues[column]
      if (value) out[column] = value
    }
    return out
  }

  get findings(): Finding[] {
    const stats = this.result?.stats
    if (!stats) return []
    return validate({
      kind: this.kind,
      stats: {
        triangles: stats.triangles,
        materials: stats.materials,
        images: stats.images,
        skins: stats.skins,
        height: stats.height,
        byteLength: stats.byteLength,
        animations: stats.animations,
      },
      mappedBones: this.mappedBones as never[],
      positionsAreFloat: this.positionsAreFloat,
      sharedAnims: this.sharedAnims,
      nodesStillScaled: this.result?.nodesStillScaled ?? 0,
      clipAssignments: this.clipAssignments,
      csv:
        this.kind === 'monster'
          ? {
              id: this.id,
              idIsValid: isValidId(this.id),
              idTaken: (this.repo?.monsters ?? []).some((row) => row.id === this.id),
              replacingExisting: this.replacingExisting,
              values: this.csvValues,
            }
          : undefined,
    })
  }

  get positionsAreFloat(): boolean {
    void this.subjectGeneration
    const container = this.original
    if (!container) return true
    return (container.json.meshes ?? []).every((mesh) =>
      mesh.primitives.every((prim) => accessorIsFloat(container, prim.attributes.POSITION))
    )
  }

  get unresolved(): Finding[] {
    return this.findings.filter((f) => f.severity === 'red' || !this.acknowledged.includes(f.code))
  }

  async loadRepo(): Promise<void> {
    this.repo = await api.fetchRepo()
    this.drafts = (await api.listDrafts()).drafts
  }

  /** Clear everything and begin a fresh draft. Does not change the step. */
  startNew(kind: ModelKind): void {
    this.draftId = `draft-${Date.now().toString(36)}`
    this.kind = kind
    this.id = ''
    this.displayName = ''
    this.replacingExisting = false
    this.source = emptySource()
    this.settings = defaultSettings()
    this.csvValues = {}
    this.acknowledged = []
    this.original = null
    this.container = null
    this.originalBytes = null
    this.loaded = false
    this.result = null
    this.conceptBytes = null
    this.sourceFileBytes = null
    this.importNotes = []
    this.subjectGeneration++
    // Navigation is the caller's: the start step prepares a draft in place and
    // stays put until there is something to move on to.
  }

  async importFile(file: File): Promise<void> {
    await this.guard('Reading the model', async () => {
      const imported = await importModelFile(file)
      this.original = imported.container
      this.originalBytes = imported.bytes
      this.loaded = true
      this.subjectGeneration++
      this.convertedFromFbx = imported.converted
      this.importNotes = imported.notes
      this.source.sourceFileName = file.name
      this.sourceFileBytes = new Uint8Array(await file.arrayBuffer())

      this.settings.boneMapping = guessBoneMapping(this.joints)
      this.settings.targetHeight = this.openingHeight()
      this.settings.material.metallicFactor = 0
      this.settings.material.roughnessFactor = 0.9
      if (!this.id) this.id = suggestId(file.name)
      if (!this.displayName) this.displayName = titleCase(this.id)

      await this.recompute()
      this.applyDerivedValues()
    })
  }

  /** Load a shipped model back in, to adjust it and write it out again. */
  async openExisting(kind: ModelKind, fileName: string): Promise<void> {
    await this.guard('Loading the shipped model', async () => {
      this.startNew(kind)
      const id = fileName.replace(/\.glb$/i, '')
      const bytes = await api.fetchGameModel(`${kind === 'monster' ? 'monsters' : 'characters'}/${fileName}`)

      this.original = parseGlb(bytes)
      this.originalBytes = bytes
      this.loaded = true
      this.subjectGeneration++
      this.id = id
      this.replacingExisting = true
      this.displayName = titleCase(id)

      const row = (this.repo?.monsters ?? []).find((entry) => entry.id === id)
      if (row) {
        this.csvValues = { ...row }
        this.displayName = row.name || this.displayName
        this.sharedAnims = row.sharedAnims === 'true'
        this.weaponId = row.weapon ?? ''
        this.attackStyle = (row.animAttack ?? '').startsWith('claw') ? 'claw' : 'weapon'
      }

      this.settings.boneMapping = guessBoneMapping(this.joints)
      this.settings.targetHeight = round(this.sourceHeight, 2)
      // A shipped model is already repaired; do not undo its material by default.
      this.settings.material.metallicFactor = 0
      this.settings.material.roughnessFactor = 0.9
      this.settings.texture.maxSize = 1024

      await this.recompute()
      // Some shipped models have no row yet — stone_golem is in the models
      // directory but not in monsters.csv. Seed one so the form is usable.
      if (!row) this.applyDerivedValues()
      this.step = 'skeleton'
    })
  }

  async openDraft(id: string): Promise<void> {
    await this.guard('Opening the draft', async () => {
      const state = await api.fetchDraft(id)
      const original = await api.getDraftFile(id, 'original.glb')
      if (!original) throw new Error('This draft lost its model file')

      this.draftId = state.id
      this.kind = state.kind
      this.id = state.id.replace(/^draft-/, '')
      this.displayName = state.displayName
      this.source = state.source
      this.csvValues = state.csvValues
      this.acknowledged = state.acknowledged
      this.replacingExisting = state.replacingExisting
      this.sharedAnims = state.summary.sharedAnims
      this.weaponId = state.summary.weapon
      this.original = parseGlb(original)
      this.originalBytes = original
      this.loaded = true
      this.subjectGeneration++
      this.conceptBytes = await api.getDraftFile(id, 'concept.png')

      this.settings.boneMapping = guessBoneMapping(this.joints)
      this.settings.targetHeight = state.summary.height
      this.settings.texture.maxSize = state.summary.textureSize
      this.settings.texture.quality = state.summary.textureQuality

      await this.recompute()
      this.step = (state.step as StepId) ?? 'skeleton'
    })
  }

  /**
   * What the height slider opens on.
   *
   * Keeping whatever the file measures is right for anything already authored
   * at game scale, but a raw Meshy export arrives at 1 cm — `doc/assets/
   * monsters.md` records rescaling every one of them — and opening on 0.02 m
   * leaves an invisible model and a slider nowhere near the useful range.
   */
  openingHeight(): number {
    const measured = this.sourceHeight
    if (measured >= PLAUSIBLE_HEIGHT.min && measured <= PLAUSIBLE_HEIGHT.max) {
      return round(measured, 2)
    }
    const fallback = this.kind === 'character' ? 1.8 : 2
    this.importNotes = [
      ...this.importNotes,
      `Imported at ${measured.toPrecision(3)} m, which is not a size anything in this game is. Opening at ${fallback} m — set the real height on the Size step.`,
    ]
    return fallback
  }

  /**
   * Coalesce the rebuilds a dragged slider would otherwise fire. Rebuilding
   * means re-encoding textures and reparsing the GLB, which is far too much to
   * do per input event.
   */
  scheduleRecompute(after: () => void = () => {}): void {
    if (this.#pending) clearTimeout(this.#pending)
    this.#pending = setTimeout(() => {
      this.#pending = null
      this.recompute().then(after)
    }, 140)
  }

  /** Re-run everything from the pristine import. Late results are discarded. */
  async recompute(): Promise<void> {
    if (!this.original) return
    const token = ++this.#token
    const { container, ...output } = await runPipeline(
      this.original,
      $state.snapshot(this.settings) as PipelineSettings
    )
    if (token !== this.#token) return
    this.container = container
    this.result = output
  }

  /** Fill in every column the model itself decides. */
  applyDerivedValues(): void {
    if (this.kind !== 'monster') return
    const defaults = gameplayDefaults(this.displayName)
    const derived: Record<string, string> = {
      ...defaults,
      ...this.csvValues,
      name: this.csvValues.name || defaults.name,
      sharedAnims: this.sharedAnims ? 'true' : '',
      walkSpeed: String(walkSpeedFor(this.hipsHeight)),
      runSpeed: String(runSpeedFor(this.hipsHeight)),
      weapon: this.weaponId,
      weaponBone: this.weaponId ? 'RightHand' : '',
      // The along-bone offset follows from the hand, so it is recomputed; the
      // other five axes are fitted by eye and are left as they were found.
      ...gripToCsv(
        this.weaponId
          ? { ...gripFromCsv(this.csvValues), y: weaponOffsetFor(this.handReach) }
          : { x: 0, y: 0, z: 0, rx: 0, ry: 0, rz: 0 }
      ),
    }
    if (this.sharedAnims) Object.assign(derived, sharedAnimDefaults(this.attackStyle))
    this.csvValues = derived
  }

  get summary(): ImportSummary {
    const stats = this.result?.stats
    return {
      height: stats?.height ?? 0,
      joints: stats?.joints ?? 0,
      mappedBones: this.mappedBones.length,
      triangles: stats?.triangles ?? 0,
      textureSize: this.settings.texture.maxSize,
      textureQuality: this.settings.texture.quality,
      textureCount: stats?.images.length ?? 0,
      sharedAnims: this.sharedAnims,
      weapon: this.weaponId,
      weaponOffset: Number(this.csvValues.weaponOffset ?? 0),
      walkSpeed: Number(this.csvValues.walkSpeed ?? 0),
      runSpeed: Number(this.csvValues.runSpeed ?? 0),
      hipsHeight: this.hipsHeight,
      convertedFromFbx: this.convertedFromFbx,
    }
  }

  get draftState(): DraftState {
    return {
      id: this.draftId,
      kind: this.kind,
      displayName: this.displayName,
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      step: this.step,
      source: $state.snapshot(this.source) as SourceInfo,
      summary: this.summary,
      csvValues: $state.snapshot(this.csvValues) as Record<string, string>,
      acknowledged: $state.snapshot(this.acknowledged) as string[],
      hasModel: this.result !== null,
      hasConcept: this.conceptBytes !== null,
      hasSourceFile: this.sourceFileBytes !== null,
      replacingExisting: this.replacingExisting,
    }
  }

  /** Persist the draft, and — for apply — the files it will write from. */
  async save(): Promise<boolean> {
    if (!this.draftId || !this.result || !this.originalBytes) {
      this.error = 'Nothing to save yet — load a model on the Start step first.'
      return false
    }
    return this.guard('Saving the draft', async () => {
      const state = this.draftState
      // Apply reads the row by id, so the draft is stored under the final id.
      state.id = this.id || this.draftId
      this.draftId = state.id

      await api.saveDraft(state)
      // Named individually: apply refuses without model.glb, and knowing which
      // upload failed is the difference between a cause and a symptom.
      await this.put(state.id, 'original.glb', this.originalBytes!)
      await this.put(state.id, 'model.glb', this.result!.bytes)
      if (this.conceptBytes) await this.put(state.id, 'concept.png', this.conceptBytes)
      if (this.sourceFileBytes) await this.put(state.id, 'source.bin', this.sourceFileBytes)
      this.savedAt = new Date().toLocaleTimeString()
      this.drafts = (await api.listDrafts()).drafts
    })
  }

  async put(id: string, name: DraftFile, bytes: Uint8Array): Promise<void> {
    try {
      await api.putDraftFile(id, name, bytes)
    } catch (error) {
      throw new Error(
        `Could not save ${name} (${(bytes.byteLength / 1024).toFixed(0)} KB) into the draft: ${
          error instanceof Error ? error.message : String(error)
        }`
      )
    }
  }

  acknowledge(code: string, on: boolean): void {
    this.acknowledged = on
      ? [...new Set([...this.acknowledged, code])]
      : this.acknowledged.filter((entry) => entry !== code)
  }

  /**
   * Run a step's work, reporting failure both on screen and to the caller.
   *
   * The return value matters: a caller that carries on after a failed save
   * hits a second, downstream error which overwrites the first — and the
   * message left on screen then describes a symptom rather than the cause.
   */
  async guard(label: string, work: () => Promise<void>): Promise<boolean> {
    this.busy = label
    this.error = null
    try {
      await work()
      return true
    } catch (error) {
      this.error = error instanceof Error ? error.message : String(error)
      return false
    } finally {
      this.busy = null
    }
  }
}

/** The range anything in this game occupies, matching the validator's. */
const PLAUSIBLE_HEIGHT = { min: 0.3, max: 6 }

function round(value: number, digits: number): number {
  const factor = 10 ** digits
  return Math.round(value * factor) / factor
}

function titleCase(id: string): string {
  return id
    .split('_')
    .filter(Boolean)
    .map((word) => word[0].toUpperCase() + word.slice(1))
    .join(' ')
}

export const session = new Session()
