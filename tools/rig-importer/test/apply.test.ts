import { afterAll, describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import fs from 'node:fs/promises'
import { resolve } from 'node:path'
import { planApply } from '../src/lib/server/apply'
import { buildDocEntry, upsertDocEntry } from '../src/lib/server/doc-entry'
import { deleteDraft, saveFile, saveState } from '../src/lib/server/drafts'
import type { DraftState } from '../src/lib/plan'
import { MODELS_DIR } from './fixtures'

const DRAFT_ID = 'test-wyvern'

function draft(overrides: Partial<DraftState> = {}): DraftState {
  return {
    id: DRAFT_ID,
    kind: 'monster',
    displayName: 'Wyvern',
    createdAt: '2026-08-24T00:00:00.000Z',
    updatedAt: '2026-08-24T00:00:00.000Z',
    step: 'apply',
    source: {
      generator: 'Meshy.ai',
      tier: '유료 생성',
      generatedOn: '2026-08-24',
      sourceName: 'Emerald Wyrm',
      rigger: 'mixamo.com',
      conceptSource: 'chatgpt.com',
      license: '',
      notes: 'Wings are a separate material — merged before export.',
      sourceFileName: 'Meshy_AI_Emerald_Wyrm_0824101500_texture_obj.fbx',
    },
    summary: {
      height: 2.6,
      joints: 65,
      mappedBones: 65,
      triangles: 9800,
      textureSize: 1024,
      textureQuality: 0.88,
      textureCount: 2,
      sharedAnims: true,
      weapon: 'greatclub',
      weaponOffset: 0.24,
      walkSpeed: 1.9,
      runSpeed: 5.4,
      hipsHeight: 1.23,
      convertedFromFbx: true,
    },
    csvValues: { name: 'Wyvern', level: '7', animIdle: 'idle1', animDie: 'dying', sharedAnims: 'true' },
    acknowledged: [],
    hasModel: true,
    hasConcept: false,
    hasSourceFile: true,
    replacingExisting: false,
    ...overrides,
  }
}

afterAll(() => deleteDraft(DRAFT_ID))

describe('apply plan', () => {
  it('names every file it would touch, and touches nothing yet', async () => {
    const model = new Uint8Array(readFileSync(resolve(MODELS_DIR, 'monsters/troll.glb')))
    await saveState(draft())
    await saveFile(DRAFT_ID, 'model.glb', model)
    await saveFile(DRAFT_ID, 'source.bin', new Uint8Array([1, 2, 3]))

    const plan = await planApply(draft())
    const paths = plan.changes.map((change) => change.path)

    expect(paths).toContain('client/public/models/monsters/test-wyvern.glb')
    expect(paths).toContain('data-src/monsters.csv')
    expect(paths).toContain('doc/assets/monsters.md')
    // Named for the monster, the way every source already in assets/ is —
    // bugbear.fbx, ogre.fbx, stone_golem.fbx — not for the generator.
    expect(paths).toContain('assets/test-wyvern.fbx')

    expect(plan.commands).toEqual(['node tools/convert.mjs', 'node tools/measure-monster-attack-clips.mjs'])
    expect(plan.reminders.join(' ')).toMatch(/push-assets\.sh/)
    expect(plan.reminders.join(' ')).toMatch(/Nothing is committed/)

    // Planning is read-only.
    await expect(fs.access(resolve(MODELS_DIR, 'monsters/test-wyvern.glb'))).rejects.toThrow()
  })

  it('shows the CSV row it would add, column by column', async () => {
    const plan = await planApply(draft())
    const byColumn = Object.fromEntries(plan.csvChanges.map((change) => [change.column, change.to]))

    expect(byColumn.name).toBe('Wyvern')
    expect(byColumn.model).toBe('monsters/test-wyvern.glb')
    expect(byColumn.level).toBe('7')
    expect(byColumn.sharedAnims).toBe('true')
    expect(plan.changes.find((change) => change.path === 'data-src/monsters.csv')?.preview).toContain('test-wyvern')
  })

  it('sends a character down a different path, with the wiring it still needs', async () => {
    const plan = await planApply(draft({ kind: 'character' }))
    const paths = plan.changes.map((change) => change.path)

    expect(paths).toContain('client/public/models/characters/test-wyvern.glb')
    expect(paths).not.toContain('data-src/monsters.csv')
    expect(paths).toContain('doc/assets/characters.md')
    expect(plan.reminders.join(' ')).toMatch(/modelPaths\.ts/)
    expect(plan.reminders.join(' ')).toMatch(/CLASS_GENDER_MODELS/)
  })
})

describe('doc entry', () => {
  it('records everything CLAUDE.md asks for', () => {
    const entry = buildDocEntry(draft())

    expect(entry).toContain('Meshy.ai')
    expect(entry).toContain('유료 생성')
    expect(entry).toContain('2026-08-24')
    expect(entry).toContain('"Emerald Wyrm"')
    expect(entry).toContain('mixamo.com')
    expect(entry).toContain('65본')
    expect(entry).toContain('높이 2.60m')
    expect(entry).toContain('1024²·JPEG q88')
    expect(entry).toContain('assets/test-wyvern.fbx')
    expect(entry).toContain('원본 파일명')
    expect(entry).toContain('sharedAnims')
    expect(entry).toContain('greatclub')
    expect(entry).toContain('Wings are a separate material')
  })

  it('links the concept art only when there is one', () => {
    expect(buildDocEntry(draft({ hasConcept: true }))).toContain('![원화](../images/monsters/test-wyvern-concept.png)')
    expect(buildDocEntry(draft())).not.toContain('![원화]')
  })

  it('files the entry under its heading instead of at the bottom of the file', () => {
    const markdown = '# Monster Assets\n\n## Monster\n\n- orc\n- goblin\n\n## Boss\n\n- ogre_boss\n'
    const next = upsertDocEntry(markdown, '- wyvern', 'wyvern', '## Monster')

    expect(next).toBe('# Monster Assets\n\n## Monster\n\n- orc\n- goblin\n- wyvern\n\n## Boss\n\n- ogre_boss\n')
  })

  it('appends the heading when the file has not got one', () => {
    const next = upsertDocEntry('# Character Assets\n\n## Human\n\n- knight\n', '- wyvern', 'wyvern', '## New')
    expect(next).toContain('## New\n\n- wyvern')
  })

  // Reworking a shipped model applies again, and it used to stack a second and
  // a third copy of the entry underneath the first.
  it('replaces the entry it already wrote instead of stacking another', () => {
    const first = upsertDocEntry('## Monster\n\n- orc\n', '- wyvern (Wyvern)\n  - first pass', 'wyvern', '## Monster')
    const second = upsertDocEntry(first, '- wyvern (Wyvern)\n  - second pass', 'wyvern', '## Monster')

    expect(second.match(/^- wyvern/gm)).toHaveLength(1)
    expect(second).toContain('second pass')
    expect(second).not.toContain('first pass')
    expect(second).toContain('- orc')
  })

  it('replaces the whole entry, sub-bullets and all', () => {
    const markdown = [
      '## Monster',
      '',
      '- kobold (Kobold) generated somewhere',
      '  - 원화는 chatgpt.com에서 생성',
      '',
      '    > a prompt indented under the entry',
      '',
      '    ![원화](../images/monsters/kobold-concept.png)',
      '- orc (Orc)',
      '',
    ].join('\n')

    const next = upsertDocEntry(markdown, '- kobold (Kobold) redone', 'kobold', '## Monster')

    expect(next).not.toContain('a prompt indented under the entry')
    expect(next).not.toContain('kobold-concept.png')
    expect(next).toContain('- kobold (Kobold) redone')
    expect(next).toContain('- orc (Orc)')
  })

  it('does not mistake a longer id that starts the same way', () => {
    const markdown = '## Monster\n\n- ogre_boss (Ogre Warlord)\n  - detail\n- ogre (Ogre)\n  - detail\n'
    const next = upsertDocEntry(markdown, '- ogre (Ogre) redone', 'ogre', '## Monster')

    expect(next).toContain('- ogre_boss (Ogre Warlord)')
    expect(next).toContain('- ogre (Ogre) redone')
    expect(next.match(/^- ogre_boss/gm)).toHaveLength(1)
  })
})

describe('doc entry wording', () => {
  it('does not leave a dangling clause when the generator is unknown', () => {
    const entry = buildDocEntry(
      draft({ source: { ...draft().source, generator: '', tier: '', sourceName: '' } })
    )
    const first = entry.split('\n')[0]

    expect(first).not.toContain('  ')
    expect(first).not.toMatch(/\)\s+에서 3d 생성/)
    expect(first).toContain('mixamo.com에서 auto-rig (65본)')
  })

  it('reads as one sentence when everything is known', () => {
    const first = buildDocEntry(draft()).split('\n')[0]
    expect(first).toBe(
      '- test-wyvern (Wyvern) Meshy.ai (유료 생성, 2026-08-24, "Emerald Wyrm")에서 3d 생성 후 mixamo.com에서 auto-rig (65본)'
    )
  })
})
