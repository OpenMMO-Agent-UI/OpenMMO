import { describe, expect, it } from 'vitest'
import { isValidId, MAX_ID_LENGTH, suggestId } from '../src/lib/game/defaults'
import { sourceAssetName } from '../src/lib/game/paths'

describe('suggestId', () => {
  // Real filenames out of assets/, plus the one that produced a 77-character
  // id and a 500 from the draft store.
  it.each([
    ['Meshy_AI_Hyena_Warlord_0815114431_texture_obj.fbx', 'hyena_warlord'],
    ['Meshy_AI_Ironclad_Warlord_0814065431_texture_obj.fbx', 'ironclad_warlord'],
    ['Meshy_AI_Ironthorn_Crusher_0816083411_texture_obj.zip', 'ironthorn_crusher'],
    ['Meshy_AI_The_Jolly_Buccaneer_0808104220_texture_obj.zip', 'jolly_buccaneer'],
    ['Meshy_AI_Silent_Oath_0817092607_texture_obj.zip', 'silent_oath'],
    ['Meshy_AI_The_One_Eyed_Colossus_Biped_Meshy_AI_Meshy_Merged_Animations.fbx', 'one_eyed_colossus'],
    ['ogre.fbx', 'ogre'],
    ['troll.fbx', 'troll'],
    ['stone_golem.fbx', 'stone_golem'],
  ])('reads %s as %s', (fileName, expected) => {
    expect(suggestId(fileName)).toBe(expected)
  })

  it('always produces something the CSV and the draft store accept', () => {
    const awkward = [
      'Meshy_AI_The_One_Eyed_Colossus_Biped_Meshy_AI_Meshy_Merged_Animations.fbx',
      '2048_dragon.glb',
      '   .glb',
      'A.glb',
      'x'.repeat(200) + '.fbx',
      'Meshy_AI_0815114431_texture_obj.fbx',
    ]
    for (const fileName of awkward) {
      const id = suggestId(fileName)
      expect(id.length).toBeLessThanOrEqual(MAX_ID_LENGTH)
      expect(isValidId(id), `${fileName} -> "${id}"`).toBe(true)
    }
  })

  it('never cuts a word in half when it truncates', () => {
    const id = suggestId(`${'alpha_bravo_charlie_delta_echo_foxtrot_golf_hotel_india'}.fbx`)
    expect(id.length).toBeLessThanOrEqual(MAX_ID_LENGTH)
    expect(id.endsWith('_')).toBe(false)
  })
})

describe('isValidId', () => {
  it('accepts the ids the game ships', () => {
    for (const id of ['ogre', 'stone_golem', 'orc_female', 'scp939', 'ogre_boss']) {
      expect(isValidId(id)).toBe(true)
    }
  })

  it('rejects what the CSV and the filesystem cannot carry', () => {
    expect(isValidId('')).toBe(false)
    expect(isValidId('2headed')).toBe(false)
    expect(isValidId('Ogre')).toBe(false)
    expect(isValidId('ogre boss')).toBe(false)
    expect(isValidId('ogre-boss')).toBe(false)
    expect(isValidId('a'.repeat(MAX_ID_LENGTH + 1))).toBe(false)
  })
})

describe('sourceAssetName', () => {
  it.each([
    ['bugbear', 'Meshy_AI_Fanghide_Warlord_0816070254_texture_obj.fbx', 'bugbear.fbx'],
    ['ogre', 'Meshy_AI_Ironhide_Brute_0816083411_texture_obj.glb', 'ogre.glb'],
    ['troll', 'troll.FBX', 'troll.fbx'],
    ['stone_golem', 'whatever', 'stone_golem.glb'],
  ])('files %s as %s', (id, original, expected) => {
    expect(sourceAssetName(id, original)).toBe(expected)
  })

  it('matches how the shipped monster sources are already named', () => {
    for (const id of ['bugbear', 'ogre', 'troll', 'stone_golem']) {
      expect(sourceAssetName(id, `Meshy_AI_Something_012345_texture_obj.fbx`)).toBe(`${id}.fbx`)
    }
  })
})
