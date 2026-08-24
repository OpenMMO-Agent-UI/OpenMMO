import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { findRow, parseCsv, rowDiff, rowToRecord, serializeCsv, upsertRow } from '../src/lib/game/csv'

const MONSTERS_CSV = resolve(import.meta.dirname, '../../../data-src/monsters.csv')
const source = readFileSync(MONSTERS_CSV, 'utf8')

describe('monsters.csv', () => {
  it('round-trips the checked-in file byte for byte', () => {
    expect(serializeCsv(parseCsv(source))).toBe(source)
  })

  it('reads a shipped row back with its documented values', () => {
    const table = parseCsv(source)
    const ogre = rowToRecord(table, table.rows[findRow(table, 'ogre')])
    expect(ogre.model).toBe('monsters/ogre.glb')
    expect(ogre.sharedAnims).toBe('true')
    expect(ogre.weaponOffset).toBe('0.24')
    expect(ogre.animAttack).toBe('slash1')
  })

  it('appends a new row and leaves every other line untouched', () => {
    const table = parseCsv(source)
    const next = upsertRow(table, 'wyvern', {
      name: 'Wyvern',
      model: 'monsters/wyvern.glb',
      level: '7',
      sharedAnims: 'true',
    })

    expect(next.rows).toHaveLength(table.rows.length + 1)
    expect(serializeCsv(next).startsWith(source.trimEnd())).toBe(true)

    const added = rowToRecord(next, next.rows.at(-1)!)
    expect(added.id).toBe('wyvern')
    expect(added.name).toBe('Wyvern')
    expect(added.animIdle).toBe('')
    expect(next.rows.at(-1)).toHaveLength(table.header.length)
  })

  it('updates an existing row without clearing the columns it was not given', () => {
    const table = parseCsv(source)
    const before = rowToRecord(table, table.rows[findRow(table, 'ogre')])

    const next = upsertRow(table, 'ogre', { walkSpeed: '1.9' })
    const after = rowToRecord(next, next.rows[findRow(next, 'ogre')])

    expect(next.rows).toHaveLength(table.rows.length)
    expect(after.walkSpeed).toBe('1.9')
    expect(after.weapon).toBe(before.weapon)
    expect(after.animAttack).toBe(before.animAttack)
  })

  it('reports exactly what a write would change', () => {
    const table = parseCsv(source)
    const next = upsertRow(table, 'ogre', { walkSpeed: '1.9', guard: '15' })
    const changes = rowDiff(table, 'ogre', next.rows[findRow(next, 'ogre')])
    expect(changes).toEqual([{ column: 'walkSpeed', from: '1.8', to: '1.9' }])
  })

  // tools/convert.mjs parses these files with a plain split(','), so there is
  // no escaping available — the value has to be refused, not quoted.
  it('refuses a value containing a comma', () => {
    const table = parseCsv(source)
    expect(() => upsertRow(table, 'wyvern', { name: 'Wyvern, the Green' })).toThrow(/comma/)
  })

  it('refuses a column the table does not have', () => {
    const table = parseCsv(source)
    expect(() => upsertRow(table, 'wyvern', { breathWeapon: 'acid' })).toThrow(/No such column/)
  })
})
