/**
 * The door actually opens.
 *
 * game-bridge.test.ts checks the shapes; this runs a real retarget through the
 * client's own function, source pack onto a different rig, and asserts the
 * result is bound to the target's bones. If the client's retargeting stops
 * working — or stops being reachable from here — the sheet would still render,
 * just showing clips it never actually retargeted, and only this notices.
 *
 * Both packs are used because they carry no textures, so GLTFLoader.parse gets
 * through in node without an ImageBitmap.
 */
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { retargetAnimationsForCharacterModel } from '$game/utils/characterAnimationUtils'
import { ANIMATIONS_DIR } from '../src/lib/server/library'

async function loadPack(name: string) {
  const bytes = await readFile(path.join(ANIMATIONS_DIR, name))
  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength)
  return new Promise<{ scene: THREE.Group; animations: THREE.AnimationClip[] }>((resolve, reject) =>
    new GLTFLoader().parse(buffer as ArrayBuffer, '', (gltf) => resolve(gltf as never), reject)
  )
}

describe('retargeting through the game', () => {
  it('rebinds a locomotion clip onto a different rig', async () => {
    const source = await loadPack('locomotion.glb')
    const target = await loadPack('combat_melee.glb')
    const walk = source.animations.find((clip) => clip.name === 'walk')
    expect(walk).toBeDefined()

    const [retargeted] = await retargetAnimationsForCharacterModel(target.scene, source.scene, [walk!])

    expect(retargeted.name).toBe('walk')
    expect(retargeted.duration).toBeCloseTo(walk!.duration, 3)

    // Not the identity. The client hands clips straight back when the two rigs
    // share a rest pose, and an assertion that only checked the name and the
    // duration would pass on that. These two rigs do not share one — 33 bones
    // against 69 — so real work has to have happened.
    expect(retargeted).not.toBe(walk)
    const hipsBefore = walk!.tracks.find((track) => track.name.endsWith('Hips.position'))!
    const hipsAfter = retargeted.tracks.find((track) => track.name === 'Hips.position')!
    expect(hipsAfter).toBeDefined()
    expect(hipsAfter.values[1]).not.toBeCloseTo(hipsBefore.values[1], 3)

    // Track names come back as `Bone.property`, and every bone they name has to
    // exist on the target — a track bound to a bone the rig lacks is exactly the
    // silent failure this tool is for.
    const targetBones = new Set<string>()
    target.scene.traverse((object) => {
      if ((object as THREE.Bone).isBone) targetBones.add(object.name)
    })
    const named = retargeted.tracks.map((track) => track.name.split('.')[0])
    expect(named.length).toBeGreaterThan(10)
    for (const bone of named) expect(targetBones.has(bone)).toBe(true)
  }, 30_000)
})
