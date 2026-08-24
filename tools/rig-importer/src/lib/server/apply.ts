/**
 * Turning a finished draft into repo changes.
 *
 * Everything is planned first and shown as a diff; the same plan is what gets
 * executed. Nothing here reaches outside the repo — syncing the raw source to
 * Hugging Face is a maintainer action with its own script, so it is only ever
 * mentioned, never run.
 */
import path from 'node:path'
import { existsSync } from 'node:fs'
import { findRow, parseCsv, rowDiff, serializeCsv, upsertRow } from '../game/csv'
import type { ApplyPlan, ApplyResult, DraftState, FileChange } from '../plan'
import { modelPathFor, sourceAssetName } from '../game/paths'
import { buildDocEntry, upsertDocEntry } from './doc-entry'
import { loadFile } from './drafts'
import {
  DATA_SRC_DIR,
  RAW_ASSETS_DIR,
  conceptImagePath,
  docAssetsFile,
  gitStatus,
  modelDir,
  readText,
  repoRelative,
  runRepoCommand,
  within,
  writeBinary,
  writeText,
} from './repo'

const MONSTERS_CSV = path.join(DATA_SRC_DIR, 'monsters.csv')

const DOC_HEADING = {
  monster: '## Monster',
  character: '## Imported with rig-importer',
} as const

interface Prepared {
  plan: ApplyPlan
  writes: (() => Promise<void>)[]
}

export async function planApply(draft: DraftState): Promise<ApplyPlan> {
  return (await prepare(draft)).plan
}

export async function applyDraft(draft: DraftState, runGenerators: boolean): Promise<ApplyResult> {
  const { plan, writes } = await prepare(draft)

  const written: string[] = []
  for (let i = 0; i < writes.length; i++) {
    await writes[i]()
    written.push(plan.changes[i].path)
  }

  const commandResults = runGenerators
    ? await Promise.all(plan.commands.map((line) => runOne(line)))
    : []

  return { plan, written, commandResults, gitStatus: await gitStatus() }
}

function runOne(line: string) {
  const [command, ...args] = line.split(' ')
  return runRepoCommand(command, args)
}

async function prepare(draft: DraftState): Promise<Prepared> {
  const changes: FileChange[] = []
  const writes: (() => Promise<void>)[] = []
  const reminders: string[] = []
  let csvChanges: ApplyPlan['csvChanges'] = []

  const model = await loadFile(draft.id, 'model.glb')
  if (!model) {
    throw new Error(
      `Draft "${draft.id}" has its settings on disk but not its processed model, so the save did not finish. ` +
        'Go back a step and press Save draft — the error it reports is the real one.'
    )
  }

  const modelFile = within(modelDir(draft.kind), `${draft.id}.glb`)
  changes.push({
    path: repoRelative(modelFile),
    action: existsSync(modelFile) ? 'overwrite' : 'create',
    bytes: model.byteLength,
  })
  writes.push(() => writeBinary(modelFile, model))

  if (draft.kind === 'monster') {
    const source = await readText(MONSTERS_CSV)
    const table = parseCsv(source)
    const values = { ...draft.csvValues, model: modelPathFor(draft.kind, draft.id) }
    const next = upsertRow(table, draft.id, values)
    csvChanges = rowDiff(table, draft.id, next.rows[findRow(next, draft.id)])

    changes.push({
      path: repoRelative(MONSTERS_CSV),
      action: findRow(table, draft.id) >= 0 ? 'overwrite' : 'append',
      preview: next.rows[findRow(next, draft.id)].join(','),
    })
    writes.push(() => writeText(MONSTERS_CSV, serializeCsv(next)))
  }

  const concept = await loadFile(draft.id, 'concept.png')
  if (concept) {
    const conceptFile = conceptImagePath(draft.kind, draft.id)
    changes.push({
      path: repoRelative(conceptFile),
      action: existsSync(conceptFile) ? 'overwrite' : 'create',
      bytes: concept.byteLength,
    })
    writes.push(() => writeBinary(conceptFile, concept))
  }

  const docFile = docAssetsFile(draft.kind)
  const entry = buildDocEntry(draft)
  const markdown = await readText(docFile)

  const replacingEntry = new RegExp(`^- ${draft.id}(\\s|$)`, 'm').test(markdown)
  changes.push({
    path: repoRelative(docFile),
    action: replacingEntry ? 'overwrite' : 'append',
    preview: entry,
  })
  writes.push(() => writeText(docFile, upsertDocEntry(markdown, entry, draft.id, DOC_HEADING[draft.kind])))

  const sourceFile = await loadFile(draft.id, 'source.bin')
  if (sourceFile && draft.source.sourceFileName) {
    const target = within(RAW_ASSETS_DIR, sourceAssetName(draft.id, draft.source.sourceFileName))
    changes.push({
      path: repoRelative(target),
      action: existsSync(target) ? 'overwrite' : 'create',
      bytes: sourceFile.byteLength,
    })
    writes.push(() => writeBinary(target, sourceFile))
    reminders.push(
      'The raw source is now in assets/, which is git-ignored. Run tools/push-assets.sh to sync it to Hugging Face — that uploads the whole working tree and deletes what is missing, so it stays a deliberate, separate step.'
    )
  } else {
    reminders.push('No raw source file was attached. Keep the FBX somewhere — without it the model cannot be rebuilt.')
  }

  const commands =
    draft.kind === 'monster'
      ? ['node tools/convert.mjs', 'node tools/measure-monster-attack-clips.mjs']
      : ['node tools/convert.mjs']

  if (draft.kind === 'character') {
    reminders.push(
      `Characters are wired up in TypeScript, not CSV. Still to do by hand: add a path constant for ${draft.id}.glb in client/src/lib/utils/modelPaths.ts, map it in CLASS_GENDER_MODELS (or NPC_MODEL_OVERRIDES), add the class to CharacterClass in networkTypes, and mirror it on the server.`
    )
  }
  reminders.push('Nothing is committed. Review the diff and commit it yourself.')

  return { plan: { changes, csvChanges, commands, reminders }, writes }
}
