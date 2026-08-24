/** Types shared by the wizard and the server that writes its result. */
import type { ModelKind } from './game/paths'

export type { ModelKind }

export interface SourceInfo {
  /** Where the mesh came from: "Meshy.ai", "Tripo", "Sketchfab", … */
  generator: string
  /** Free/paid tier — CLAUDE.md requires recording this for AI and paid tools. */
  tier: string
  /** ISO date the model was generated. */
  generatedOn: string
  /** The name the generator gave it, e.g. "Ironhide Brute". */
  sourceName: string
  /** Who rigged it, usually mixamo.com. */
  rigger: string
  /** Where the concept art came from. */
  conceptSource: string
  license: string
  notes: string
  /** File name kept under assets/, e.g. "bugbear.fbx". */
  sourceFileName: string
}

export interface ImportSummary {
  height: number
  joints: number
  mappedBones: number
  triangles: number
  textureSize: number
  textureQuality: number
  textureCount: number
  sharedAnims: boolean
  weapon: string
  weaponOffset: number
  walkSpeed: number
  runSpeed: number
  hipsHeight: number
  convertedFromFbx: boolean
}

export interface DraftState {
  id: string
  kind: ModelKind
  displayName: string
  createdAt: string
  updatedAt: string
  step: string
  source: SourceInfo
  summary: ImportSummary
  csvValues: Record<string, string>
  acknowledged: string[]
  hasModel: boolean
  hasConcept: boolean
  hasSourceFile: boolean
  replacingExisting: boolean
}

/** The files a draft keeps on disk while it is being worked on. */
export type DraftFile = 'model.glb' | 'original.glb' | 'concept.png' | 'source.bin'

export interface FileChange {
  /** Repo-relative path. */
  path: string
  action: 'create' | 'overwrite' | 'append'
  bytes?: number
  /** Text preview for markdown and CSV changes. */
  preview?: string
}

export interface ApplyPlan {
  changes: FileChange[]
  csvChanges: { column: string; from: string; to: string }[]
  commands: string[]
  reminders: string[]
}

export interface ApplyResult {
  plan: ApplyPlan
  written: string[]
  commandResults: { command: string; ok: boolean; output: string }[]
  gitStatus: string
}
