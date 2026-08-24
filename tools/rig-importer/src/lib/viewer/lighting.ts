/**
 * Lighting presets.
 *
 * Studio is for judging the material honestly. The other two mirror what the
 * game actually renders under, with the constants read out of GameScene:
 * ambient 0.35 and a 10.0 sun at #fff8f0, blending 85% toward #ff6b35 at
 * twilight.
 *
 * These reproduce the light rig, not the game's post-processing — the client
 * renders through its own WebGPU pipeline.
 */
import * as THREE from 'three'

export interface LightingPreset {
  id: string
  label: string
  hint: string
  background: number
  ground: number
  build: () => THREE.Object3D
}

function directional(color: string, intensity: number, position: [number, number, number]): THREE.DirectionalLight {
  const light = new THREE.DirectionalLight(new THREE.Color(color), intensity)
  light.position.set(...position)
  return light
}

function group(...lights: THREE.Object3D[]): THREE.Group {
  const holder = new THREE.Group()
  for (const light of lights) holder.add(light)
  return holder
}

function twilight(): string {
  return `#${new THREE.Color('#fff8f0').lerp(new THREE.Color('#ff6b35'), 0.85).getHexString()}`
}

export const LIGHTING_PRESETS: LightingPreset[] = [
  {
    id: 'studio',
    label: 'Studio',
    hint: 'Neutral three-point. Judge the material here — it flatters nothing.',
    background: 0x14161c,
    ground: 0x23262f,
    build: () =>
      group(
        new THREE.HemisphereLight(0xffffff, 0x404040, 1.1),
        directional('#ffffff', 2.2, [4, 6, 5]),
        directional('#ffffff', 0.9, [-5, 3, -4]),
        directional('#ffffff', 0.7, [0, 2, -6])
      ),
  },
  {
    id: 'noon',
    label: 'Game · noon',
    hint: 'Full sun. Where a too-metallic material turns into a mirror.',
    background: 0x8fb4d8,
    ground: 0x4a5a3c,
    build: () => group(new THREE.AmbientLight(0xffffff, 0.35), directional('#fff8f0', 10, [10, 10, 10])),
  },
  {
    id: 'dusk',
    label: 'Game · dusk',
    hint: 'Twilight sun, 85% blended to orange. Dark models vanish here.',
    background: 0x2b2438,
    ground: 0x33302e,
    build: () => group(new THREE.AmbientLight(0xffffff, 0.32), directional(twilight(), 3.2, [8, 2.2, 6])),
  },
]

export function presetById(id: string): LightingPreset {
  return LIGHTING_PRESETS.find((preset) => preset.id === id) ?? LIGHTING_PRESETS[0]
}
