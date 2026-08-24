/**
 * The gate before anything is written.
 *
 * Red findings block the write: they describe a model the game cannot load or a
 * row that would corrupt the CSV. Yellow findings are budget and quality calls —
 * they can be accepted, one acknowledgement each. The budgets come from what
 * the existing assets actually measure: every character and monster in the repo
 * sits at or under 10k triangles with a single material and at most three
 * 1024² textures (`scp939` and `stone_golem` are bought-in exceptions).
 */
import { CSV_UNSAFE } from './game/csv'
import { MAX_ID_LENGTH } from './game/defaults'
import { SHARED_PACK_CLIPS, splitClipList, type AnimColumn } from './game/clips'
import { CORE_BONES, CRITICAL_BONES, type StandardBone } from './bones/skeleton'

export const BUDGET = {
  triangles: 10_000,
  materials: 1,
  images: 3,
  textureSize: 1024,
  hardTextureSize: 2048,
  byteLength: 1_500_000,
  minHeight: 0.3,
  maxHeight: 6,
} as const

export type Severity = 'red' | 'yellow'

export interface Finding {
  /**
   * Unique per finding. `code` is not: one bad clip name, one oversized
   * texture and one unsafe column each raise their own finding under the same
   * code, so the code identifies the *kind* — which is what acknowledgement
   * keys off — while this identifies the instance.
   */
  id: string
  severity: Severity
  code: string
  title: string
  detail: string
}

export interface ValidationInput {
  kind: 'monster' | 'character'
  stats: {
    triangles: number
    materials: number
    images: { width: number; height: number; mimeType: string }[]
    skins: number
    height: number
    byteLength: number
    animations: string[]
  }
  /** Standard bones that ended up mapped to a source joint. */
  mappedBones: StandardBone[]
  /** True when POSITION data is plain FLOAT, so scale could be baked. */
  positionsAreFloat: boolean
  sharedAnims: boolean
  /** Nodes still carrying a scale after the importer flattened what it could. */
  nodesStillScaled: number
  /** Clip name per monsters.csv anim column, "" when the column is left empty. */
  clipAssignments: Partial<Record<AnimColumn, string>>
  csv?: {
    id: string
    idIsValid: boolean
    idTaken: boolean
    replacingExisting: boolean
    values: Record<string, string>
  }
}

export function validate(input: ValidationInput): Finding[] {
  const findings: Finding[] = []
  const seen = new Map<string, number>()
  const add = (severity: Severity, code: string, title: string, detail: string) => {
    const nth = (seen.get(code) ?? 0) + 1
    seen.set(code, nth)
    findings.push({ id: `${code}#${nth}`, severity, code, title, detail })
  }
  const red = (code: string, title: string, detail: string) => add('red', code, title, detail)
  const yellow = (code: string, title: string, detail: string) => add('yellow', code, title, detail)

  const { stats } = input
  const mapped = new Set(input.mappedBones)

  if (stats.skins === 0) {
    red('no-skin', 'No skinned mesh', 'This file has no skin, so it is not a rigged model. Rig it first.')
  }
  if (!mapped.has('Hips')) {
    red(
      'no-hips',
      'No Hips bone',
      'Grounding, corpse offset and retargeting all key off Hips. Map it on the Skeleton step.'
    )
  }
  if (!input.positionsAreFloat) {
    red(
      'quantized-positions',
      'Quantized vertex positions',
      'POSITION is not FLOAT, so the height cannot be baked. Re-export without mesh quantization.'
    )
  }

  const duplicateClips = stats.animations.filter((name, i) => stats.animations.indexOf(name) !== i)
  if (duplicateClips.length > 0) {
    red(
      'duplicate-clips',
      'Duplicate clip names',
      `${[...new Set(duplicateClips)].join(', ')} appears more than once — the client picks clips by name.`
    )
  }
  const badClipNames = stats.animations.filter((name) => /[.:[\]]/.test(name))
  if (badClipNames.length > 0) {
    red(
      'clip-name-separator',
      'Clip name contains "." or ":"',
      `${badClipNames.join(', ')} — three.js reads those as property-path separators and drops the track.`
    )
  }

  for (const image of stats.images) {
    if (Math.max(image.width, image.height) > BUDGET.hardTextureSize) {
      red(
        'texture-too-large',
        'Texture over 2048px',
        `${image.width}×${image.height} is past what this project ships. Downsize it on the Material step.`
      )
    }
  }

  for (const [column, clip] of Object.entries(input.clipAssignments) as [AnimColumn, string][]) {
    for (const name of splitClipList(clip)) {
      const known = input.sharedAnims
        ? (SHARED_PACK_CLIPS as readonly string[]).includes(name)
        : stats.animations.includes(name)
      if (known) continue
      red(
        'unknown-clip',
        `${column} names a clip that does not exist`,
        input.sharedAnims
          ? `"${name}" is not in locomotion.glb or combat_melee.glb.`
          : `"${name}" is not in this model.`
      )
    }
  }

  if (input.csv) {
    const { csv } = input
    if (!csv.idIsValid) {
      const tooLong = csv.id.length > MAX_ID_LENGTH
      red(
        'bad-id',
        tooLong ? `Id is ${csv.id.length} characters` : 'Invalid id',
        tooLong
          ? `"${csv.id}" — this becomes a filename and a CSV key, so keep it under ${MAX_ID_LENGTH}. Generator filenames carry a lot of packaging worth dropping.`
          : `"${csv.id}" — use lower case letters, digits and underscores, starting with a letter.`
      )
    }
    if (csv.idTaken && !csv.replacingExisting) {
      red('id-taken', 'Id already used', `monsters.csv already has a row for "${csv.id}".`)
    }
    const rotation = csv.values.weaponRotation ?? ''
    if (rotation !== '' && !/^-?[\d.]+(\|-?[\d.]+){0,2}$/.test(rotation.trim())) {
      red(
        'bad-weapon-rotation',
        'weaponRotation is not readable',
        `"${rotation}" — the client reads this as up to three degree values separated by "|", and silently treats anything it cannot parse as zero.`
      )
    }

    const unsafe = Object.entries(csv.values).filter(([, value]) => CSV_UNSAFE.test(value))
    for (const [column, value] of unsafe) {
      red(
        'csv-unsafe-value',
        `${column} contains a comma or quote`,
        `"${value}" would break the row — tools/convert.mjs splits these files on commas with no quoting.`
      )
    }
  }

  if (stats.triangles > BUDGET.triangles) {
    yellow(
      'triangle-budget',
      `${stats.triangles.toLocaleString()} triangles`,
      `Everything else in the repo sits at or under ${BUDGET.triangles.toLocaleString()}. Remesh upstream in Meshy or Tripo rather than simplifying here.`
    )
  }
  if (stats.materials > BUDGET.materials) {
    yellow(
      'material-budget',
      `${stats.materials} materials`,
      'Every shipped monster uses one. Each extra material is another draw call per instance on screen.'
    )
  }
  if (stats.images.length > BUDGET.images) {
    yellow('image-budget', `${stats.images.length} textures`, `The shipped models carry at most ${BUDGET.images}.`)
  }
  for (const image of stats.images) {
    const size = Math.max(image.width, image.height)
    if (size > BUDGET.textureSize && size <= BUDGET.hardTextureSize) {
      yellow(
        'texture-size',
        `${image.width}×${image.height} texture`,
        `The project standard is ${BUDGET.textureSize}² JPEG. Downsize it on the Material step.`
      )
    }
  }
  if (stats.byteLength > BUDGET.byteLength) {
    yellow(
      'file-size',
      `${(stats.byteLength / 1_000_000).toFixed(2)} MB`,
      `Past the ${(BUDGET.byteLength / 1_000_000).toFixed(1)} MB soft budget — this downloads once per player.`
    )
  }

  const missingCritical = CRITICAL_BONES.filter((bone) => bone !== 'Hips' && !mapped.has(bone))
  if (missingCritical.length > 0) {
    yellow(
      'missing-critical-bones',
      `Unmapped: ${missingCritical.join(', ')}`,
      'Feet decide where the model meets the ground; RightHand is where a weapon hangs.'
    )
  }
  const missingCore = CORE_BONES.filter((bone) => !mapped.has(bone) && !missingCritical.includes(bone) && bone !== 'Hips')
  if (missingCore.length > 0 && input.sharedAnims) {
    yellow(
      'missing-core-bones',
      `${missingCore.length} core bones unmapped`,
      `${missingCore.join(', ')} — shared pack clips will not drive these joints.`
    )
  }

  if (stats.height < BUDGET.minHeight || stats.height > BUDGET.maxHeight) {
    yellow(
      'odd-height',
      `${stats.height.toFixed(2)} m tall`,
      'Outside the range anything in this game occupies. Check the target height on the Size step.'
    )
  }

  if (input.nodesStillScaled > 0) {
    yellow(
      'node-scale',
      `${input.nodesStillScaled} node${input.nodesStillScaled === 1 ? '' : 's'} still scaled`,
      'No shipped model keeps a scale on a node. Anything parented to a bone below one — a weapon above all — inherits it and renders at the wrong size, and weaponOffset stops being metres.'
    )
  }

  if (input.csv?.values.weapon && !mapped.has('RightHand')) {
    yellow('weapon-without-hand', 'Weapon set but no RightHand', 'The weapon will hang off the model root.')
  }

  return findings
}

export function blocking(findings: Finding[]): Finding[] {
  return findings.filter((f) => f.severity === 'red')
}

export function acknowledgeable(findings: Finding[]): Finding[] {
  return findings.filter((f) => f.severity === 'yellow')
}
