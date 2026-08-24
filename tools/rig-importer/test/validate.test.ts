import { describe, expect, it } from 'vitest'
import { BUDGET, blocking, validate, type ValidationInput } from '../src/lib/validate'

function input(overrides: Partial<ValidationInput> = {}): ValidationInput {
  return {
    kind: 'monster',
    stats: {
      triangles: 9_500,
      materials: 1,
      images: [{ width: 1024, height: 1024, mimeType: 'image/jpeg' }],
      skins: 1,
      height: 2.1,
      byteLength: 800_000,
      animations: [],
    },
    mappedBones: ['Hips', 'RightHand', 'LeftFoot', 'RightFoot', 'Spine', 'Neck', 'Head',
      'LeftArm', 'LeftForeArm', 'LeftHand', 'RightArm', 'RightForeArm', 'LeftUpLeg',
      'LeftLeg', 'RightUpLeg', 'RightLeg'],
    positionsAreFloat: true,
    sharedAnims: true,
    nodesStillScaled: 0,
    clipAssignments: { animIdle: 'idle1', animAttack: 'claw1|claw2', animDie: 'dying' },
    csv: {
      id: 'wyvern',
      idIsValid: true,
      idTaken: false,
      replacingExisting: false,
      values: { name: 'Wyvern', weapon: '' },
    },
    ...overrides,
  }
}

const codes = (i: ValidationInput) => validate(i).map((f) => f.code)

describe('validate', () => {
  it('passes a model that matches the shipped conventions', () => {
    expect(validate(input())).toEqual([])
  })

  // The UI keys its lists on these. `code` is not unique — one finding is
  // raised per bad clip, per oversized texture, per unsafe column — so keying
  // on it crashed the Validate step with each_key_duplicate.
  it('gives every finding a unique id even when they share a code', () => {
    const stats = {
      ...input().stats,
      images: [
        { width: 4096, height: 4096, mimeType: 'image/png' },
        { width: 4096, height: 4096, mimeType: 'image/png' },
        { width: 2048, height: 2048, mimeType: 'image/jpeg' },
      ],
    }
    const findings = validate(
      input({ stats, clipAssignments: { animIdle: 'nope', animAttack: 'also_nope|still_nope' } })
    )

    const ids = findings.map((f) => f.id)
    expect(new Set(ids).size).toBe(ids.length)

    // ...and the codes really do repeat, which is the point.
    expect(findings.filter((f) => f.code === 'texture-too-large')).toHaveLength(2)
    expect(findings.filter((f) => f.code === 'unknown-clip')).toHaveLength(3)
  })

  it('blocks a model with no skin', () => {
    expect(codes(input({ stats: { ...input().stats, skins: 0 } }))).toContain('no-skin')
  })

  it('blocks a model with no Hips', () => {
    const findings = validate(input({ mappedBones: ['RightHand'] }))
    expect(blocking(findings).map((f) => f.code)).toContain('no-hips')
  })

  it('blocks quantized positions, which cannot be rescaled', () => {
    expect(codes(input({ positionsAreFloat: false }))).toContain('quantized-positions')
  })

  it('blocks duplicate and separator-bearing clip names', () => {
    const stats = { ...input().stats, animations: ['walk', 'walk', 'mixamo.com'] }
    const found = codes(input({ stats, sharedAnims: true }))
    expect(found).toContain('duplicate-clips')
    expect(found).toContain('clip-name-separator')
  })

  it('blocks a clip the shared packs do not contain', () => {
    expect(codes(input({ clipAssignments: { animIdle: 'loiter' } }))).toContain('unknown-clip')
  })

  it('checks a self-animated model against its own clips instead', () => {
    const stats = { ...input().stats, animations: ['Idle_Loop_Rig'] }
    expect(codes(input({ sharedAnims: false, stats, clipAssignments: { animIdle: 'Idle_Loop_Rig' } }))).toEqual([])
    expect(codes(input({ sharedAnims: false, stats, clipAssignments: { animIdle: 'idle1' } }))).toContain('unknown-clip')
  })

  it('blocks a texture past 2048 but only warns between 1024 and 2048', () => {
    const big = { ...input().stats, images: [{ width: 4096, height: 4096, mimeType: 'image/png' }] }
    expect(blocking(validate(input({ stats: big }))).map((f) => f.code)).toContain('texture-too-large')

    const middling = { ...input().stats, images: [{ width: 2048, height: 2048, mimeType: 'image/jpeg' }] }
    const findings = validate(input({ stats: middling }))
    expect(blocking(findings)).toEqual([])
    expect(findings.map((f) => f.code)).toContain('texture-size')
  })

  it('blocks a duplicate id unless the run is deliberately replacing it', () => {
    const taken = { ...input().csv!, idTaken: true }
    expect(codes(input({ csv: taken }))).toContain('id-taken')
    expect(codes(input({ csv: { ...taken, replacingExisting: true } }))).not.toContain('id-taken')
  })

  it('blocks a CSV value the build could not parse back', () => {
    const csv = { ...input().csv!, values: { name: 'Wyvern, the Green' } }
    expect(codes(input({ csv }))).toContain('csv-unsafe-value')
  })

  it('warns on the budgets without blocking', () => {
    const stats = {
      ...input().stats,
      triangles: BUDGET.triangles + 1,
      materials: 3,
      byteLength: 4_000_000,
      images: new Array(5).fill({ width: 1024, height: 1024, mimeType: 'image/jpeg' }),
    }
    const findings = validate(input({ stats }))
    expect(blocking(findings)).toEqual([])
    expect(findings.map((f) => f.code)).toEqual(
      expect.arrayContaining(['triangle-budget', 'material-budget', 'image-budget', 'file-size'])
    )
  })

  it('warns when a rig has no feet to stand on', () => {
    expect(codes(input({ mappedBones: ['Hips', 'RightHand'] }))).toContain('missing-critical-bones')
  })

  it('blocks an id too long to be a filename', () => {
    const long = 'meshy_ai_the_one_eyed_colossus_biped_meshy_ai_meshy_merged_animations'
    const csv = { ...input().csv!, id: long, idIsValid: false }
    const findings = validate(input({ csv }))

    const finding = blocking(findings).find((f) => f.code === 'bad-id')
    expect(finding?.title).toContain(String(long.length))
  })

  it('blocks a weaponRotation the client could not read', () => {
    const bad = { ...input().csv!, values: { weaponRotation: '90|banana|0' } }
    expect(blocking(validate(input({ csv: bad }))).map((f) => f.code)).toContain('bad-weapon-rotation')

    const good = { ...input().csv!, values: { weaponRotation: '90|0|-45' } }
    expect(codes(input({ csv: good }))).not.toContain('bad-weapon-rotation')

    const blank = { ...input().csv!, values: { weaponRotation: '' } }
    expect(codes(input({ csv: blank }))).not.toContain('bad-weapon-rotation')
  })

  it('warns when a node keeps a scale the importer could not flatten', () => {
    const findings = validate(input({ nodesStillScaled: 2 }))
    expect(blocking(findings)).toEqual([])
    expect(findings.map((f) => f.code)).toContain('node-scale')
  })

  it('warns when a weapon has nowhere to hang', () => {
    const csv = { ...input().csv!, values: { weapon: 'greatclub' } }
    expect(codes(input({ csv, mappedBones: ['Hips', 'LeftFoot', 'RightFoot'] }))).toContain('weapon-without-hand')
  })
})
