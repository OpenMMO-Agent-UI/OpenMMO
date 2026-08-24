/**
 * The roster and the motion list, against the repo as it actually stands.
 *
 * Both are derived rather than listed, so these assert the derivation rules —
 * not the exact contents, which change whenever a monster is added.
 */
import { describe, expect, it } from 'vitest'
import { CHARACTER_ANIMATION_PACK_PATHS } from '$game/utils/modelPaths'
import fs from 'node:fs/promises'
import path from 'node:path'
import {
  ANIMATIONS_DIR,
  listMotions,
  listPacks,
  listRigs,
  resolveTakeFile,
  resolveTakeFolder,
} from '../src/lib/server/library'

describe('the roster', () => {
  it('lists each distinct GLB once, so a boss does not repeat its base monster', async () => {
    const rigs = await listRigs()
    const models = rigs.map((rig) => rig.model)
    expect(new Set(models).size).toBe(models.length)
    expect(models).toContain('monsters/ogre.glb')
    // ogre_boss is ogre.glb at 1.4x — the same rig, and only on the sheet once.
    expect(rigs.filter((rig) => rig.model === 'monsters/ogre.glb')).toHaveLength(1)
  })

  it('leaves out the quadruped, which no humanoid clip applies to', async () => {
    const rigs = await listRigs()
    expect(rigs.map((rig) => rig.id)).not.toContain('scp939')
  })

  it('carries the three rigs whose animation is the standard to match', async () => {
    const ids = (await listRigs()).map((rig) => rig.id)
    expect(ids).toEqual(expect.arrayContaining(['stone_golem', 'cyclop', 'lizardfolk']))
  })

  it('includes the player character models alongside the monsters', async () => {
    const rigs = await listRigs()
    expect(rigs.filter((rig) => rig.kind === 'character').map((rig) => rig.id)).toEqual(
      expect.arrayContaining(['knight', 'female_knight', 'barbarian', 'valkyrie'])
    )
  })
})

describe('the motions', () => {
  it('comes from the packs, not from the AnimationName enum', async () => {
    const names = (await listMotions()).map((motion) => motion.name)
    // In the packs and used by monsters, but absent from the enum.
    expect(names).toEqual(expect.arrayContaining(['combat_idle', 'claw1', 'claw2']))
    // In the enum, but in no pack — these come off the base models.
    expect(names).not.toContain('attack1')
  })

  it('says which pack supplies each one', async () => {
    const motions = await listMotions()
    expect(motions.find((motion) => motion.name === 'walk')?.pack).toBe('locomotion')
    expect(motions.find((motion) => motion.name === 'dying')?.pack).toBe('combat_melee')
  })
})

describe('the packs', () => {
  /**
   * `shipped` decides whether the strip lets you upload over a pack, so a pack
   * of the game's own that is not marked as one becomes read-only for no
   * reason — which is what happened to social, offhand and fishing when this
   * list was kept by hand alongside the game's.
   */
  it('marks every pack the game loads, not just the two the shared path defaults to', async () => {
    const packs = await listPacks()
    const shipped = packs.filter((pack) => pack.shipped).map((pack) => pack.name).sort()
    expect(shipped).toEqual(['combat_melee', 'fishing', 'locomotion', 'offhand', 'social'])
  })

  it('agrees with the game about which files those are', async () => {
    const fromGame = Object.values(CHARACTER_ANIMATION_PACK_PATHS)
      .map((url) => url.split('/').pop()!.replace(/\.glb$/, ''))
      .sort()
    const shipped = (await listPacks()).filter((pack) => pack.shipped).map((pack) => pack.name).sort()
    expect(shipped).toEqual(fromGame)
  })

  it('does not mark a written candidate as shipped', async () => {
    const candidate = path.join(ANIMATIONS_DIR, 'zz_written_candidate.glb')
    await fs.copyFile(path.join(ANIMATIONS_DIR, 'locomotion.glb'), candidate)
    try {
      const written = (await listPacks()).find((pack) => pack.name === 'zz_written_candidate')
      expect(written?.shipped).toBe(false)
    } finally {
      await fs.unlink(candidate)
    }
  })
})

describe('where an added take lands', () => {
  it('files it under a motion, or at the root when unfiled', () => {
    expect(resolveTakeFolder('walk').endsWith('/takes/walk')).toBe(true)
    expect(resolveTakeFolder('').endsWith('/takes')).toBe(true)
    expect(resolveTakeFolder('  ').endsWith('/takes')).toBe(true)
  })

  it('refuses anything that would climb out of takes/', () => {
    expect(() => resolveTakeFolder('../../../etc')).toThrow()
    expect(() => resolveTakeFolder('walk/../..')).toThrow()
    expect(() => resolveTakeFolder('/etc')).toThrow()
    expect(() => resolveTakeFolder('a/b')).toThrow()
    expect(() => resolveTakeFolder('..')).toThrow()
  })
})

describe('removing a take', () => {
  it('resolves a take inside takes/', () => {
    expect(resolveTakeFile('walk/heavy.fbx').endsWith('/takes/walk/heavy.fbx')).toBe(true)
    expect(resolveTakeFile('loose.glb').endsWith('/takes/loose.glb')).toBe(true)
  })

  it('refuses anything that is not a take, so the folder keeps its README', () => {
    expect(() => resolveTakeFile('README.md')).toThrow()
    expect(() => resolveTakeFile('')).toThrow()
  })

  it('refuses anything outside takes/', () => {
    expect(() => resolveTakeFile('../../client/public/models/animations/locomotion.glb')).toThrow()
    expect(() => resolveTakeFile('/etc/hosts.glb')).toThrow()
    expect(() => resolveTakeFile('walk/../../../secret.glb')).toThrow()
  })
})

describe('multiple packs sharing the same motion names', () => {
  it('gives a candidate pack its own group, duplicating names locomotion already uses', async () => {
    const candidate = path.join(ANIMATIONS_DIR, 'zz_test_candidate.glb')
    await fs.copyFile(path.join(ANIMATIONS_DIR, 'locomotion.glb'), candidate)
    try {
      const motions = await listMotions()
      const fromLocomotion = motions.filter((m) => m.pack === 'locomotion' && m.name === 'walk')
      const fromCandidate = motions.filter((m) => m.pack === 'zz_test_candidate' && m.name === 'walk')
      expect(fromLocomotion).toHaveLength(1)
      expect(fromCandidate).toHaveLength(1)
      // Same name, two distinct (name, pack) rungs — not merged, not deduped.
      expect(motions.filter((m) => m.name === 'walk').length).toBeGreaterThanOrEqual(2)
    } finally {
      await fs.unlink(candidate)
    }
  })

  it('still puts locomotion and combat_melee first regardless of what else is on disk', async () => {
    const candidate = path.join(ANIMATIONS_DIR, 'aaa_test_first_alphabetically.glb')
    await fs.copyFile(path.join(ANIMATIONS_DIR, 'locomotion.glb'), candidate)
    try {
      const motions = await listMotions()
      expect(motions[0].pack).toBe('locomotion')
    } finally {
      await fs.unlink(candidate)
    }
  })
})
