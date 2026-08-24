/** Where a model lives, in the form the game refers to it. Shared by both sides. */
export type ModelKind = 'monster' | 'character'

export function modelFolder(kind: ModelKind): string {
  return kind === 'monster' ? 'monsters' : 'characters'
}

/** The value that goes in monsters.csv `model`, e.g. "monsters/ogre.glb". */
export function modelPathFor(kind: ModelKind, id: string): string {
  return `${modelFolder(kind)}/${id}.glb`
}

export function docAssetsFileName(kind: ModelKind): string {
  return kind === 'monster' ? 'monsters.md' : 'characters.md'
}

/**
 * What the raw source is filed under in `assets/`.
 *
 * The generators name their downloads after themselves —
 * `Meshy_AI_Hyena_Warlord_0815114431_texture_obj.fbx` — while every monster
 * source already in the repo is named for the monster: `bugbear.fbx`,
 * `ogre.fbx`, `stone_golem.fbx`. The extension follows the file that came in,
 * because an FBX source stays an FBX.
 */
export function sourceAssetName(id: string, originalFileName: string): string {
  const match = /\.([A-Za-z0-9]+)$/.exec(originalFileName)
  const extension = (match?.[1] ?? 'glb').toLowerCase()
  return `${id}.${extension}`
}
