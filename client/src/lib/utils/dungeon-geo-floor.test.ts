import { afterEach, describe, expect, it, vi } from 'vitest'
import { Mesh, Raycaster, Vector3, type Group } from 'three'
import type { DungeonRoom } from '../managers/dungeonManager'
import { buildDungeonFloorGroup, type WallRun } from './dungeon-geo-floor'
import { isoCameraOccludesPlayer } from './iso-occlusion'
import { dungeonCaveTheme } from './dungeon-cave-themes'
import {
  DUNGEON_FLOOR_TEXTURE_IDX,
  DUNGEON_WALL_TEXTURE_IDX,
} from './dungeon-geo-constants'

vi.mock('./dungeon-geo-doors', () => ({ buildInteriorDoor: vi.fn() }))

const ctx = {
  grid: 20,
  wallHeight: 3,
  floorHeight: 4,
  shaftW: 2,
  shaftLen: 8,
}
let group: Group
afterEach(() => {
  group?.traverse((object) => {
    if (object instanceof Mesh) object.geometry.dispose()
  })
})

function buildWalls(corridors: DungeonRoom[], rooms: DungeonRoom[] = []) {
  const carved = Array<boolean>(ctx.grid ** 2).fill(false)
  for (const rect of [...rooms, ...corridors])
    for (let z = rect.z; z < rect.z + rect.d; z++)
      for (let x = rect.x; x < rect.x + rect.w; x++)
        carved[x + z * ctx.grid] = true
  const floor = buildDungeonFloorGroup(
    {
      depth: 1,
      rooms,
      carved,
      upShaft: { x: 16, z: 12, alongZ: true, reversed: false },
      spawns: [],
      props: [],
    },
    ctx,
    []
  )
  group = floor.group
  return floor.wallRuns
}

function wallAt(runs: WallRun[], x: number, z: number) {
  const run = runs.find((r) => r.localAABB.containsPoint(new Vector3(x, 1, z)))
  expect(run).toBeDefined()
  return run!
}

describe('dungeon wall fade groups', () => {
  it('textures corridor ground separately and keeps cave walls out of ground picking', () => {
    const runs = buildWalls(
      [{ x: 4, z: 6, w: 2, d: 4 }],
      [{ x: 2, z: 2, w: 6, d: 4 }]
    )
    const cave = dungeonCaveTheme('', 1)
    const groundAt = (x: number, z: number) =>
      new Raycaster(
        new Vector3(x, 5, z),
        new Vector3(0, -1, 0)
      ).intersectObject(group)[0]
    expect(groundAt(4.02, 8).point.y).toBe(0)
    expect(groundAt(4.02, 8).object.userData.textureIndex).toBe(
      cave.floorTexture
    )
    expect(groundAt(4, 4).object.userData.textureIndex).toBe(
      DUNGEON_FLOOR_TEXTURE_IDX
    )
    expect(wallAt(runs, 3.95, 8).mesh.userData.textureIndex).toBe(
      cave.wallTexture
    )
    expect(wallAt(runs, 1.95, 4).mesh.userData.textureIndex).toBe(
      DUNGEON_WALL_TEXTURE_IDX
    )
  })

  it('pairs an L-shaped corridor corner when only its south wall occludes', () => {
    const runs = buildWalls([
      { x: 2, z: 2, w: 2, d: 8 },
      { x: 2, z: 8, w: 8, d: 2 },
      { x: 12, z: 2, w: 2, d: 4 },
    ])
    const south = wallAt(runs, 6, 10.05)
    const west = wallAt(runs, 1.95, 5)

    expect(isoCameraOccludesPlayer(south.localAABB, 6, 1, 9.5, 0.05)).toBe(true)
    expect(isoCameraOccludesPlayer(west.localAABB, 6, 1, 9.5, 0.05)).toBe(false)
    expect(south.fadeGroups.sw).toBeGreaterThanOrEqual(0)
    expect(west.fadeGroups.sw).toBe(south.fadeGroups.sw)
    expect(wallAt(runs, 13, 6.05).fadeGroups.sw).not.toBe(south.fadeGroups.sw)
    expect(wallAt(runs, 3, 1.95).fadeGroups.sw).toBe(-1)
    expect(wallAt(runs, 4.05, 5).fadeGroups.sw).toBe(-1)
  })

  it('joins both south runs connected by the west wall of an inner bend', () => {
    const runs = buildWalls([
      { x: 2, z: 2, w: 8, d: 2 },
      { x: 8, z: 2, w: 2, d: 8 },
    ])
    const innerSouth = wallAt(runs, 5, 4.05)
    const innerWest = wallAt(runs, 7.95, 7)
    const outerSouth = wallAt(runs, 9, 10.05)

    expect(innerSouth.fadeGroups.sw).toBeGreaterThanOrEqual(0)
    expect(innerWest.fadeGroups.sw).toBe(innerSouth.fadeGroups.sw)
    expect(outerSouth.fadeGroups.sw).toBe(innerSouth.fadeGroups.sw)
  })

  it('groups the north and east walls when the camera looks from the north-east', () => {
    const runs = buildWalls([
      { x: 2, z: 2, w: 2, d: 8 },
      { x: 2, z: 2, w: 8, d: 2 },
    ])
    const north = wallAt(runs, 6, 1.95)
    const eastEnd = wallAt(runs, 10.05, 3)
    const innerEast = wallAt(runs, 4.05, 7)
    const south = wallAt(runs, 6, 4.05)

    expect(north.fadeGroups.ne).toBeGreaterThanOrEqual(0)
    expect(eastEnd.fadeGroups.ne).toBe(north.fadeGroups.ne)
    expect(innerEast.fadeGroups.ne).not.toBe(north.fadeGroups.ne)
    expect(north.fadeGroups.sw).toBe(-1)
    expect(south.fadeGroups.ne).toBe(-1)
    expect(south.fadeGroups.sw).toBeGreaterThanOrEqual(0)
  })

  it('preserves room groups across doorway gaps and separates corridor walls', () => {
    const runs = buildWalls(
      [{ x: 4, z: 6, w: 2, d: 4 }],
      [{ x: 2, z: 2, w: 6, d: 4 }]
    )
    const roomSouth = wallAt(runs, 3, 6.05)
    const corridorSouth = wallAt(runs, 5, 10.05)

    expect(roomSouth.fadeGroups.sw).toBe(0)
    expect(wallAt(runs, 7, 6.05).fadeGroups.sw).toBe(roomSouth.fadeGroups.sw)
    expect(wallAt(runs, 1.95, 4).fadeGroups.sw).toBe(roomSouth.fadeGroups.sw)
    expect(corridorSouth.fadeGroups.sw).not.toBe(roomSouth.fadeGroups.sw)
    expect(wallAt(runs, 3.95, 8).fadeGroups.sw).toBe(
      corridorSouth.fadeGroups.sw
    )
  })
})
