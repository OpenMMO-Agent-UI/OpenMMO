import { describe, expect, it } from 'vitest'
import { canonicalize, guessBoneMapping, normalize, splitSide, type Joint } from '../src/lib/bones/match'
import { nodeParents } from '../src/lib/gltf/measure'
import { loadFixture } from './fixtures'
import type { GlbContainer } from '../src/lib/gltf/container'

function jointsOf(c: GlbContainer): Joint[] {
  const parents = nodeParents(c)
  const nodes = new Set<number>()
  for (const skin of c.json.skins ?? []) for (const joint of skin.joints) nodes.add(joint)
  return [...nodes].map((node) => ({ node, name: c.json.nodes![node].name ?? '', parent: parents[node] }))
}

function mappingOf(c: GlbContainer): Record<string, string> {
  const out: Record<string, string> = {}
  for (const guess of guessBoneMapping(jointsOf(c))) {
    if (guess.node !== null) out[guess.standard] = c.json.nodes![guess.node].name ?? ''
  }
  return out
}

function chain(names: string[]): Joint[] {
  return names.map((name, i) => ({ node: i, name, parent: i === 0 ? -1 : i - 1 }))
}

describe('name normalisation', () => {
  it('drops the mixamo prefix and every separator', () => {
    expect(normalize('mixamorig:LeftHand')).toBe('lefthand')
    expect(normalize('mixamorig1:Hips')).toBe('hips')
    expect(normalize('Upper_Arm.L')).toBe('upperarml')
  })

  it('finds the side token wherever the rigger put it', () => {
    expect(splitSide('UpperArm_L')).toEqual({ side: 'Left', stem: 'upperarm' })
    expect(splitSide('thigh.R')).toEqual({ side: 'Right', stem: 'thigh' })
    expect(splitSide('LeftFoot')).toEqual({ side: 'Left', stem: 'foot' })
    expect(splitSide('l_hand')).toEqual({ side: 'Left', stem: 'hand' })
    expect(splitSide('Hips')).toEqual({ side: null, stem: 'hips' })
  })
})

describe('canonicalize', () => {
  it.each([
    ['mixamorig:RightForeArm', 'RightForeArm'],
    ['UpperArm_L', 'LeftArm'],
    ['LowerArm_R', 'RightForeArm'],
    ['clavicle_l', 'LeftShoulder'],
    ['thigh_r', 'RightUpLeg'],
    ['calf_L', 'LeftLeg'],
    ['ball_r', 'RightToeBase'],
    ['pelvis', 'Hips'],
    ['spine_01', 'Spine'],
    ['spine_03', 'Spine2'],
    ['Chest', 'Spine1'],
    ['index_02_l', 'LeftHandIndex2'],
    ['thumb_01_r', 'RightHandThumb1'],
    ['neck_01', 'Neck'],
  ])('reads %s as %s', (source, expected) => {
    expect(canonicalize(source)).toBe(expected)
  })

  it('gives up rather than guessing wildly', () => {
    expect(canonicalize('IK_target_04')).toBeNull()
    expect(canonicalize('cape_physics')).toBeNull()
  })
})

describe('guessBoneMapping', () => {
  it('maps a Mixamo rig exactly, all 65 bones', () => {
    const { container } = loadFixture('monsters/hobgoblin.glb')
    const guesses = guessBoneMapping(jointsOf(container))
    const mapped = guesses.filter((g) => g.node !== null)

    expect(mapped).toHaveLength(65)
    expect(mapped.every((g) => g.how === 'exact')).toBe(true)
  })

  // kobold and goblin ship a hand-authored 14-bone rig with none of Mixamo's names.
  it('maps a non-Mixamo rig through the alias table', () => {
    const { container } = loadFixture('monsters/kobold.glb')
    expect(mappingOf(container)).toMatchObject({
      Hips: 'Hips',
      Spine: 'Spine',
      Spine1: 'Chest',
      LeftUpLeg: 'UpperLeg_L',
      LeftLeg: 'LowerLeg_L',
      RightUpLeg: 'UpperLeg_R',
      RightLeg: 'LowerLeg_R',
      LeftShoulder: 'Shoulder_L',
      LeftArm: 'UpperArm_L',
      LeftForeArm: 'LowerArm_L',
      RightArm: 'UpperArm_R',
      RightForeArm: 'LowerArm_R',
      RightHand: 'RightHand',
    })
  })

  it('reports the bones a sparse rig simply does not have', () => {
    const { container } = loadFixture('monsters/kobold.glb')
    const mapping = mappingOf(container)
    expect(mapping.LeftFoot).toBeUndefined()
    expect(mapping.RightFoot).toBeUndefined()
    expect(mapping.Head).toBeUndefined()
  })

  // stone_golem is rigged Hips -> Spine02 -> Spine01 -> Spine, so its bone
  // named "Spine" is the one nearest the neck. Position has to win here.
  it('orders a numbered spine by the rig, even when the digits run backwards', () => {
    const { container } = loadFixture('monsters/stone_golem.glb')
    const mapping = mappingOf(container)
    expect(mapping.Spine).toBe('Spine02')
    expect(mapping.Spine1).toBe('Spine01')
    expect(mapping.Spine2).toBe('Spine')
  })

  it('orders an Unreal-style spine the same way', () => {
    const joints = chain(['pelvis', 'spine_01', 'spine_02', 'spine_03', 'neck_01', 'head'])
    const mapping = new Map(guessBoneMapping(joints).filter((g) => g.node !== null).map((g) => [g.standard, joints[g.node!].name]))
    expect(mapping.get('Hips')).toBe('pelvis')
    expect(mapping.get('Spine')).toBe('spine_01')
    expect(mapping.get('Spine1')).toBe('spine_02')
    expect(mapping.get('Spine2')).toBe('spine_03')
    expect(mapping.get('Neck')).toBe('neck_01')
  })

  it('never swaps a left bone onto a right slot', () => {
    const joints = chain(['Hips', 'thigh_l', 'calf_l', 'foot_l'])
    for (const guess of guessBoneMapping(joints)) {
      if (guess.node === null || !guess.standard.startsWith('Right')) continue
      throw new Error(`${joints[guess.node].name} was mapped onto ${guess.standard}`)
    }
    expect(true).toBe(true)
  })

  it('gives each source joint at most one standard bone', () => {
    const { container } = loadFixture('monsters/stone_golem.glb')
    const used = guessBoneMapping(jointsOf(container)).filter((g) => g.node !== null).map((g) => g.node)
    expect(new Set(used).size).toBe(used.length)
  })
})
