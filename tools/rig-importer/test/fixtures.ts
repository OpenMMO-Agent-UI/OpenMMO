import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { parseGlb, type GlbContainer } from '../src/lib/gltf/container'

export const MODELS_DIR = resolve(import.meta.dirname, '../../../client/public/models')

export function loadFixture(rel: string): { container: GlbContainer; byteLength: number } {
  const bytes = new Uint8Array(readFileSync(resolve(MODELS_DIR, rel)))
  return { container: parseGlb(bytes), byteLength: bytes.byteLength }
}
