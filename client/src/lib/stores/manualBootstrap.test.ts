import { describe, expect, it } from 'vitest'

import { decodeManualBootstrap } from './manualBootstrap'

describe('manual desktop bootstrap', () => {
  it('decodes the selected server, short-lived token, and character', () => {
    const payload = {
      serverUrl: 'wss://realm.example/ws',
      googleIdToken: 'short-lived-id-token',
      characterId: 42,
    }
    const encoded = Buffer.from(JSON.stringify(payload)).toString('base64url')

    expect(decodeManualBootstrap(`#manual=${encoded}`)).toEqual(payload)
  })

  it('ignores malformed or incomplete fragments', () => {
    expect(decodeManualBootstrap('#manual=not-base64')).toBeNull()
    expect(
      decodeManualBootstrap(
        `#manual=${Buffer.from(JSON.stringify({ serverUrl: 'wss://realm.example/ws' })).toString('base64url')}`
      )
    ).toBeNull()
  })
})
