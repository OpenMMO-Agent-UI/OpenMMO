/**
 * The bug: two different takes sharing a clip name silently returned the
 * same retargeted result.
 *
 * Mixamo names almost every export's action `Armature|mixamo.com|Layer0` (see
 * takes/idle2/Idle-2.fbx, a real download, checked with Blender). The game's
 * `retargetAnimationsForCharacterModel` caches a retarget on
 * `<skeleton pair>::<clip name>` — nothing about which file the clip came
 * from — so two takes of the same Mixamo character (same rest skeleton, same
 * generic name) collided, and the second one's audition silently played back
 * the first one's cached result. `retargetTake` in `src/lib/retarget.ts` now
 * renames every clip to something take-unique before handing it through the
 * narrow door.
 */
import { readFile } from 'node:fs/promises'
import path from 'node:path'
import { describe, expect, it } from 'vitest'
import * as THREE from 'three'
import { GLTFLoader } from 'three/examples/jsm/loaders/GLTFLoader.js'
import { retargetTake } from '../src/lib/retarget'
import { ANIMATIONS_DIR } from '../src/lib/server/library'

async function loadPack(name: string) {
  const bytes = await readFile(path.join(ANIMATIONS_DIR, name))
  const buffer = bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength)
  return new Promise<{ scene: THREE.Group; animations: THREE.AnimationClip[] }>((resolve, reject) =>
    new GLTFLoader().parse(buffer as ArrayBuffer, '', (gltf) => resolve(gltf as never), reject)
  )
}

/** Two "takes" sharing Mixamo's generic name, with genuinely different motion. */
function fakeMixamoTakes(source: { scene: THREE.Group; animations: THREE.AnimationClip[] }) {
  const walk = source.animations.find((clip) => clip.name === 'walk')!
  const run = source.animations.find((clip) => clip.name === 'run')!
  const genericName = 'Armature|mixamo.com|Layer0'
  const takeA = { scene: source.scene, clip: walk.clone() }
  const takeB = { scene: source.scene, clip: run.clone() }
  takeA.clip.name = genericName
  takeB.clip.name = genericName
  return { takeA, takeB }
}

describe('retargeting two takes that share Mixamo\'s default clip name', () => {
  it('does not let the second take come back as the first take\'s cached result', async () => {
    const source = await loadPack('locomotion.glb')
    const target = await loadPack('combat_melee.glb')
    const { takeA, takeB } = fakeMixamoTakes(source)

    const [resultA] = await retargetTake(target.scene, takeA.scene, [takeA.clip])
    const [resultB] = await retargetTake(target.scene, takeB.scene, [takeB.clip])

    // walk and run have different durations and different hip-position curves.
    // If the collision were still live, resultB would be byte-for-byte resultA.
    expect(resultB.duration).not.toBeCloseTo(resultA.duration, 2)
    const hipsA = resultA.tracks.find((t) => t.name === 'Hips.position')!
    const hipsB = resultB.tracks.find((t) => t.name === 'Hips.position')!
    expect(hipsB.values).not.toEqual(hipsA.values)
  }, 30_000)
})
