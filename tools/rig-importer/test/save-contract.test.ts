import { describe, expect, it } from 'vitest'
import { Session } from '../src/lib/session.svelte'

/**
 * Apply refuses a draft whose model file is missing, so a save that fails has
 * to stop the caller. It used to only record the error on the session, and the
 * apply step carried on and overwrote it with a downstream message — leaving a
 * symptom on screen and no trace of the cause.
 */
describe('save reports failure to its caller', () => {
  it('is false, with a reason, when there is nothing loaded', async () => {
    const session = new Session()
    expect(await session.save()).toBe(false)
    expect(session.error).toMatch(/Nothing to save/)
  })

  it('is false when the work throws, and keeps that error', async () => {
    const session = new Session()
    const ok = await session.guard('Testing', async () => {
      throw new Error('the upload failed')
    })
    expect(ok).toBe(false)
    expect(session.error).toBe('the upload failed')
  })

  it('is true when the work succeeds, and clears any earlier error', async () => {
    const session = new Session()
    session.error = 'something older'
    expect(await session.guard('Testing', async () => {})).toBe(true)
    expect(session.error).toBeNull()
  })
})
