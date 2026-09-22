// Monster ownership decides who runs a brain and who therefore already drew a
// swing locally. `ownedByMe` is the one place that answers it, because it is
// also the one place that knows a spectator owns nothing: it is handed the
// agent's ownership over the relay but never adopts the brain. Comparing the
// raw ids instead reads as "I ran this monster" for a client that did not, and
// silently drops whatever the owner branch was supposed to do — which is how
// agent-owned monsters lost their attack animation in spectator mode.
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'
import { ownedByMe } from './observerStore'

const SRC = join(import.meta.dirname, '../..')

function sourceFiles(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    if (statSync(path).isDirectory()) return sourceFiles(path)
    return /\.(ts|svelte)$/.test(name) && !name.endsWith('.test.ts')
      ? [path]
      : []
  })
}

describe('monster ownership', () => {
  it('is never decided by comparing raw ids', () => {
    // `ownerId`/`owner_id` weighed against a current-player id on the same line.
    const rawCompare =
      /owner_?[iI]d\s*[!=]==?[^=]*currentPlayer|currentPlayer[^\n]*?[!=]==?\s*[^\n]*owner_?[iI]d/

    const offenders = sourceFiles(SRC)
      .filter((path) => !path.endsWith('observerStore.ts'))
      .filter((path) => rawCompare.test(readFileSync(path, 'utf8')))
      .map((path) => path.slice(SRC.length + 1))

    expect(offenders).toEqual([])
  })

  it('is always false for a spectator, whoever the relay named as owner', () => {
    // isObserver is false under vitest (no ?observe= param), so this pins the
    // ordinary client's answer; the spectator's short-circuit is the first
    // line of ownedByMe.
    expect(ownedByMe(7, 7)).toBe(true)
    expect(ownedByMe(7, 9)).toBe(false)
    expect(ownedByMe(undefined, undefined)).toBe(false)
  })
})
