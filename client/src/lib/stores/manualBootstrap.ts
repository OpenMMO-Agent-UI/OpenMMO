export type ManualBootstrap = {
  serverUrl: string
  googleIdToken: string
  characterId: number
}

export function decodeManualBootstrap(hash: string): ManualBootstrap | null {
  if (!hash.startsWith('#manual=')) return null
  try {
    const encoded = hash.slice('#manual='.length)
    const base64 = encoded.replace(/-/g, '+').replace(/_/g, '/')
    const padded = base64.padEnd(Math.ceil(base64.length / 4) * 4, '=')
    const bytes = Uint8Array.from(atob(padded), (character) =>
      character.charCodeAt(0)
    )
    const value = JSON.parse(new TextDecoder().decode(bytes)) as Partial<ManualBootstrap>
    const server = new URL(value.serverUrl ?? '')
    if (server.protocol !== 'ws:' && server.protocol !== 'wss:') return null
    if (
      !value.googleIdToken ||
      typeof value.characterId !== 'number' ||
      !Number.isInteger(value.characterId)
    ) {
      return null
    }
    return {
      serverUrl: server.toString(),
      googleIdToken: value.googleIdToken,
      characterId: value.characterId,
    }
  } catch {
    return null
  }
}
