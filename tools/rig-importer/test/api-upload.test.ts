import { afterEach, describe, expect, it, vi } from 'vitest'
import { putDraftFile } from '../src/lib/api'

/**
 * A binary PUT with no Content-Type reaches the SvelteKit dev server with an
 * empty body — the byte count is right on the client, the request succeeds, and
 * the handler sees zero bytes. It cost a draft that saved its settings but not
 * its model, and reported the failure two steps downstream.
 */
describe('putDraftFile', () => {
  afterEach(() => vi.unstubAllGlobals())

  function captureRequest() {
    const calls: { url: string; init: RequestInit }[] = []
    vi.stubGlobal('fetch', (url: string, init: RequestInit) => {
      calls.push({ url, init })
      return Promise.resolve(new Response(JSON.stringify({ ok: true }), { status: 200 }))
    })
    return calls
  }

  it('always sends an explicit Content-Type', async () => {
    const calls = captureRequest()
    await putDraftFile('ogre', 'model.glb', new Uint8Array([1, 2, 3]))

    const headers = calls[0].init.headers as Record<string, string>
    expect(headers['Content-Type']).toBe('application/octet-stream')
  })

  it('sends the bytes it was given, untouched', async () => {
    const calls = captureRequest()
    const bytes = new Uint8Array([9, 8, 7, 6])
    await putDraftFile('ogre', 'original.glb', bytes)

    expect(calls[0].init.method).toBe('PUT')
    expect(calls[0].init.body).toBe(bytes)
  })

  it('addresses the right draft and file', async () => {
    const calls = captureRequest()
    await putDraftFile('stone golem', 'concept.png', new Uint8Array([1]))

    expect(calls[0].url).toBe('/api/draft/file?id=stone%20golem&name=concept.png')
  })

  it('rejects when the server refuses, so the caller can stop', async () => {
    vi.stubGlobal('fetch', () => Promise.resolve(new Response('Empty body', { status: 400 })))
    await expect(putDraftFile('ogre', 'model.glb', new Uint8Array([1]))).rejects.toThrow(/Empty body/)
  })
})
