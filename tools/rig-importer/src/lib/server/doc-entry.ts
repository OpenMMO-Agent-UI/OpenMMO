/**
 * The `doc/assets/` provenance entry.
 *
 * CLAUDE.md requires a record of where every asset came from, with the licence
 * and — for AI or paid tools — the tier and generation date. Written in Korean
 * to match the entries already in these files rather than leaving one paragraph
 * in a different language halfway down the list.
 */
import { sourceAssetName } from '../game/paths'
import type { DraftState } from '../plan'

export function buildDocEntry(draft: DraftState): string {
  const { source, summary } = draft
  const name = draft.displayName || draft.id
  const conceptDir = draft.kind === 'monster' ? 'monsters' : 'characters'

  // Every clause is optional. Stitching them unconditionally left entries like
  // "- cyclop (Cyclop)  (2026-08-24) 에서 3d 생성" — a double space and a
  // postposition with nothing in front of it.
  const provenance = [source.tier, source.generatedOn, source.sourceName && `"${source.sourceName}"`]
    .filter(Boolean)
    .join(', ')

  const clauses: string[] = []
  if (source.generator) {
    clauses.push(`${source.generator}${provenance ? ` (${provenance})` : ''}에서 3d 생성`)
  } else if (provenance) {
    clauses.push(`${provenance} 임포트`)
  }
  if (source.rigger) {
    clauses.push(`${source.rigger}에서 auto-rig (${summary.joints}본)`)
  } else {
    clauses.push(`${summary.joints}본 리그`)
  }

  const lines = [`- ${draft.id} (${name}) ${clauses.join(' 후 ')}`.replace(/\s+/g, ' ')]

  if (source.conceptSource) {
    lines.push(
      `  - 원화는 ${source.conceptSource}에서 생성` +
        (draft.hasConcept ? ` ![원화](../images/${conceptDir}/${draft.id}-concept.png)` : '')
    )
  }

  lines.push(
    `  - tools/rig-importer로 임포트. 높이 ${summary.height.toFixed(2)}m, 원점=바닥 중심, ` +
      `본 이름 표준화(${summary.mappedBones}/${summary.joints}본 매핑), ` +
      `텍스처 ${summary.textureSize}²·JPEG q${Math.round(summary.textureQuality * 100)} ${summary.textureCount}장, ` +
      `${summary.triangles.toLocaleString()} tri` +
      (summary.convertedFromFbx ? ', FBX에서 변환' : '')
  )

  if (source.sourceFileName) {
    const stored = sourceAssetName(draft.id, source.sourceFileName)
    const renamed = stored === source.sourceFileName ? '' : ` (원본 파일명 \`${source.sourceFileName}\`)`
    lines.push(`  - 소스는 \`assets/${stored}\` 하나만 보관 (HF 동기화)${renamed}`)
  }

  if (summary.sharedAnims) {
    lines.push(
      `  - 애니메이션 클립 미탑재 — 캐릭터 공용 팩(locomotion/combat_melee)을 런타임에 리타게팅해서 쓴다 ` +
        `(\`sharedAnims\`). Hips 높이 ${summary.hipsHeight.toFixed(2)}m 기준으로 ` +
        `walkSpeed ${summary.walkSpeed}, runSpeed ${summary.runSpeed}. 공용 팩에는 hit 리액션이 없어 \`animHit\`은 비워 둠`
    )
  } else {
    lines.push('  - 모델에 포함된 클립을 그대로 쓴다 (`sharedAnims` 미사용)')
  }

  if (summary.weapon) {
    const fitted = [
      draft.csvValues.weaponOffsetX && `X ${draft.csvValues.weaponOffsetX}`,
      draft.csvValues.weaponOffsetZ && `Z ${draft.csvValues.weaponOffsetZ}`,
      draft.csvValues.weaponRotation && `회전 ${draft.csvValues.weaponRotation}(도)`,
    ].filter(Boolean)

    lines.push(
      `  - 무기는 \`RightHand\`에 ${summary.weapon}. 손 본이 손목에 있어 \`weaponOffset\` ${summary.weaponOffset}로 ` +
        `손가락 밑동까지 밀었다 (RightHand 가중치 정점이 본 축으로 뻗은 길이의 80%)` +
        (fitted.length > 0 ? `. 손바닥에 맞추려고 ${fitted.join(', ')} 추가 조정` : '')
    )
  }

  if (source.license) lines.push(`  - 라이선스: ${source.license}`)
  if (source.notes) {
    for (const note of source.notes.split('\n').filter(Boolean)) lines.push(`  - ${note}`)
  }

  return lines.join('\n')
}

/**
 * Put the entry in the file, replacing the one already there for this id.
 *
 * Reworking a shipped model is a first-class path, so applying twice has to
 * update the record rather than stack another copy of it underneath — which is
 * what it did, three times over, before this looked for the id first.
 */
export function upsertDocEntry(markdown: string, entry: string, id: string, heading: string): string {
  const lines = markdown.split('\n')
  const existing = findEntry(lines, id)

  if (existing) {
    return [...lines.slice(0, existing.start), entry, ...lines.slice(existing.end)].join('\n')
  }

  const headingAt = lines.findIndex((line) => line.trim().toLowerCase() === heading.trim().toLowerCase())
  if (headingAt < 0) {
    const trimmed = markdown.replace(/\s+$/, '')
    return `${trimmed}\n\n${heading}\n\n${entry}\n`
  }

  let insertAt = lines.length
  for (let i = headingAt + 1; i < lines.length; i++) {
    if (lines[i].startsWith('#')) {
      insertAt = i
      break
    }
  }
  while (insertAt > headingAt + 1 && lines[insertAt - 1].trim() === '') insertAt--

  return [...lines.slice(0, insertAt), entry, ...lines.slice(insertAt)].join('\n')
}

/**
 * The span an entry occupies: its own line plus everything indented under it.
 * Blank lines only belong to the entry when indented content follows them —
 * the kobold entry has a blockquote and an image separated that way.
 */
function findEntry(lines: string[], id: string): { start: number; end: number } | null {
  const opener = new RegExp(`^- ${id.replace(/[.*+?^\${}()|[\]\\]/g, '\\$&')}(\\s|$)`)
  const start = lines.findIndex((line) => opener.test(line))
  if (start < 0) return null

  let end = start + 1
  let pendingBlanks = 0
  for (let i = start + 1; i < lines.length; i++) {
    const line = lines[i]
    if (line.trim() === '') {
      pendingBlanks++
      continue
    }
    if (/^\s/.test(line)) {
      end = i + 1
      pendingBlanks = 0
      continue
    }
    break
  }
  void pendingBlanks
  return { start, end }
}
