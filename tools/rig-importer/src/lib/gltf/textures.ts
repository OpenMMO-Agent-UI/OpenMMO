/**
 * Texture re-encoding. Runs in the browser — it needs a real image decoder.
 *
 * The project standard is 1024² JPEG at quality 0.88 (`doc/assets/monsters.md`).
 * Textures are most of a model's bytes and every player downloads them, so this
 * is where the download budget is actually won.
 */
import { bufferViewBytes, repackBuffer, type GlbContainer } from './container'
import { imageInfos } from './measure'

export interface TextureSettings {
  maxSize: number
  quality: number
  /** 'auto' keeps PNG for anything with real transparency. */
  format: 'auto' | 'jpeg' | 'png'
}

export const DEFAULT_TEXTURE_SETTINGS: TextureSettings = {
  maxSize: 1024,
  quality: 0.88,
  format: 'auto',
}

export interface TextureChange {
  index: number
  name: string
  from: { width: number; height: number; bytes: number; mimeType: string }
  to: { width: number; height: number; bytes: number; mimeType: string }
}

async function decode(bytes: Uint8Array, mimeType: string): Promise<ImageBitmap> {
  return createImageBitmap(new Blob([bytes as BlobPart], { type: mimeType }))
}

function fit(width: number, height: number, maxSize: number): [number, number] {
  const longest = Math.max(width, height)
  if (longest <= maxSize) return [width, height]
  const ratio = maxSize / longest
  return [Math.max(1, Math.round(width * ratio)), Math.max(1, Math.round(height * ratio))]
}

async function encode(canvas: OffscreenCanvas, mimeType: string, quality: number): Promise<Uint8Array> {
  const blob = await canvas.convertToBlob({ type: mimeType, quality })
  return new Uint8Array(await blob.arrayBuffer())
}

function hasTransparency(pixels: Uint8ClampedArray): boolean {
  for (let i = 3; i < pixels.length; i += 4) if (pixels[i] < 250) return true
  return false
}

/**
 * Resize and re-encode every image, returning a fresh container. Images already
 * inside the budget are left byte-identical rather than round-tripped through
 * another lossy encode.
 */
export async function recompressTextures(
  c: GlbContainer,
  settings: TextureSettings
): Promise<{ container: GlbContainer; changes: TextureChange[] }> {
  const infos = imageInfos(c)
  const replacements = new Map<number, Uint8Array>()
  const changes: TextureChange[] = []

  for (const info of infos) {
    const image = c.json.images?.[info.index]
    if (!image || image.bufferView === undefined) continue

    const source = bufferViewBytes(c, image.bufferView)
    const [width, height] = fit(info.width, info.height, settings.maxSize)

    // An in-budget JPEG is already what we would produce, and it has no alpha
    // to preserve — leave its bytes alone rather than decoding and re-encoding
    // it into something slightly worse.
    const sameSize = width === info.width && height === info.height
    const wantsJpeg = settings.format !== 'png'
    if (sameSize && info.mimeType === 'image/jpeg' && wantsJpeg) continue

    const bitmap = await decode(source, info.mimeType)
    const canvas = new OffscreenCanvas(width, height)
    const ctx = canvas.getContext('2d', { willReadFrequently: true })!
    ctx.drawImage(bitmap, 0, 0, width, height)
    bitmap.close()

    const transparent =
      settings.format === 'png' ||
      (settings.format === 'auto' && hasTransparency(ctx.getImageData(0, 0, width, height).data))
    const mimeType = transparent ? 'image/png' : 'image/jpeg'

    const encoded = await encode(canvas, mimeType, settings.quality)
    // Re-encoding a PNG can make it bigger; keep whichever is smaller.
    if (sameSize && mimeType === info.mimeType && encoded.byteLength >= source.byteLength) continue

    replacements.set(image.bufferView, encoded)
    image.mimeType = mimeType
    changes.push({
      index: info.index,
      name: info.name,
      from: { width: info.width, height: info.height, bytes: info.byteLength, mimeType: info.mimeType },
      to: { width, height, bytes: encoded.byteLength, mimeType },
    })
  }

  if (replacements.size === 0) return { container: c, changes }
  return { container: repackBuffer(c, replacements), changes }
}

export interface MetallicRoughnessParams {
  /** Saturation at or below which a pixel may read as bare metal. */
  saturationCeiling: number
  /** Brightness at or below which a desaturated pixel reads as metal. */
  valueCeiling: number
  metallic: number
  metalRoughness: number
  dielectricRoughness: number
}

export const DEFAULT_MR_PARAMS: MetallicRoughnessParams = {
  saturationCeiling: 0.25,
  valueCeiling: 0.55,
  metallic: 0.85,
  metalRoughness: 0.54,
  dielectricRoughness: 0.92,
}

/**
 * Meshy only ships base colour, so the metallic-roughness map is inferred from
 * the albedo's saturation and brightness: dark and colourless reads as plate.
 *
 * It is a guess, and it guesses wrong on dark hair and claws — troll ships with
 * flat factors for exactly that reason. Look at the result before keeping it.
 */
export async function deriveMetallicRoughness(
  albedo: Uint8Array,
  mimeType: string,
  params: MetallicRoughnessParams,
  size: number
): Promise<{ bytes: Uint8Array; metalFraction: number }> {
  const bitmap = await decode(albedo, mimeType)
  const [width, height] = fit(bitmap.width, bitmap.height, size)

  const canvas = new OffscreenCanvas(width, height)
  const ctx = canvas.getContext('2d', { willReadFrequently: true })!
  ctx.drawImage(bitmap, 0, 0, width, height)
  bitmap.close()

  const image = ctx.getImageData(0, 0, width, height)
  const pixels = image.data
  let metalPixels = 0

  for (let i = 0; i < pixels.length; i += 4) {
    const r = pixels[i] / 255
    const g = pixels[i + 1] / 255
    const b = pixels[i + 2] / 255
    const max = Math.max(r, g, b)
    const min = Math.min(r, g, b)
    const saturation = max === 0 ? 0 : (max - min) / max

    const isMetal = saturation <= params.saturationCeiling && max <= params.valueCeiling
    if (isMetal) metalPixels++

    // glTF packs roughness in green and metalness in blue.
    pixels[i] = 255
    pixels[i + 1] = Math.round(255 * (isMetal ? params.metalRoughness : params.dielectricRoughness))
    pixels[i + 2] = Math.round(255 * (isMetal ? params.metallic : 0))
    pixels[i + 3] = 255
  }

  ctx.putImageData(image, 0, 0)
  return {
    bytes: await encode(canvas, 'image/jpeg', 0.9),
    metalFraction: metalPixels / (pixels.length / 4),
  }
}
