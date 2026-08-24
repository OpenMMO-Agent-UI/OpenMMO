import { json } from '@sveltejs/kit'
import type { RequestHandler } from './$types'
import path from 'node:path'
import { parseCsv, rowToRecord } from '$lib/game/csv'
import { DATA_SRC_DIR, listModels, listWeapons, readText } from '$lib/server/repo'

/** Everything the wizard needs to know about the repo it is writing into. */
export const GET: RequestHandler = async () => {
  const csv = parseCsv(await readText(path.join(DATA_SRC_DIR, 'monsters.csv')))

  return json({
    monsterColumns: csv.header,
    monsters: csv.rows.map((row) => rowToRecord(csv, row)),
    monsterModels: await listModels('monster'),
    characterModels: await listModels('character'),
    weapons: await listWeapons(),
  })
}
