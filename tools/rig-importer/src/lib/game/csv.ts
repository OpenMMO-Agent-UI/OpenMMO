/**
 * data-src CSV editing.
 *
 * `tools/convert.mjs` reads these files with a plain `split(',')` and the
 * checked-in files contain no quotes at all, so quoting is not a thing here:
 * a value with a comma or a newline in it would corrupt the table, and the
 * validator rejects it rather than inventing an escaping the build cannot read.
 */
export interface CsvTable {
  header: string[]
  rows: string[][]
  /** Trailing newline of the original file, preserved on write. */
  trailingNewline: boolean
}

export function parseCsv(text: string): CsvTable {
  const trailingNewline = text.endsWith('\n')
  const lines = text.replace(/\r\n/g, '\n').split('\n')
  if (trailingNewline) lines.pop()
  const [headerLine, ...rest] = lines
  return {
    header: headerLine.split(','),
    rows: rest.filter((line) => line.length > 0).map((line) => line.split(',')),
    trailingNewline,
  }
}

export function serializeCsv(table: CsvTable): string {
  const lines = [table.header.join(','), ...table.rows.map((row) => row.join(','))]
  return lines.join('\n') + (table.trailingNewline ? '\n' : '')
}

export function rowToRecord(table: CsvTable, row: string[]): Record<string, string> {
  const record: Record<string, string> = {}
  table.header.forEach((column, i) => {
    record[column] = row[i] ?? ''
  })
  return record
}

export function findRow(table: CsvTable, id: string): number {
  return table.rows.findIndex((row) => row[0] === id)
}

export const CSV_UNSAFE = /[,\n\r"]/

export function unsafeFields(values: Record<string, string>): string[] {
  return Object.entries(values)
    .filter(([, value]) => CSV_UNSAFE.test(value))
    .map(([column]) => column)
}

/**
 * Insert or replace one row, leaving every other row byte-identical and every
 * column the caller did not name untouched.
 */
export function upsertRow(table: CsvTable, id: string, values: Record<string, string>): CsvTable {
  const bad = unsafeFields(values)
  if (bad.length > 0) {
    throw new Error(`Values for ${bad.join(', ')} contain a comma, quote or newline, which this CSV cannot carry`)
  }

  const unknown = Object.keys(values).filter((column) => !table.header.includes(column))
  if (unknown.length > 0) throw new Error(`No such column: ${unknown.join(', ')}`)

  const index = findRow(table, id)
  const existing = index >= 0 ? table.rows[index] : []
  const row = table.header.map((column, i) => {
    if (column in values) return values[column]
    return existing[i] ?? ''
  })
  row[0] = id

  const rows = table.rows.slice()
  if (index >= 0) rows[index] = row
  else rows.push(row)
  return { ...table, rows }
}

/** Unified-diff style summary of what an upsert would change. */
export function rowDiff(table: CsvTable, id: string, next: string[]): { column: string; from: string; to: string }[] {
  const index = findRow(table, id)
  const before = index >= 0 ? table.rows[index] : table.header.map(() => '')
  return table.header
    .map((column, i) => ({ column, from: before[i] ?? '', to: next[i] ?? '' }))
    .filter((change) => change.from !== change.to)
}
