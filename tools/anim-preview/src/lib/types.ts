/** Shapes shared by the server routes and the page. */

export interface Rig {
  id: string
  name: string
  /** Path in the form the game refers to it, e.g. "monsters/ogre.glb". */
  model: string
  url: string
  kind: 'monster' | 'character'
}

export interface Motion {
  name: string
  pack: string
}

export interface Take {
  path: string
  name: string
  filedUnder: string | null
  bytes: number
}

export interface Pack {
  name: string
  url: string
  clips: string[]
  shipped: boolean
}

export interface Library {
  rigs: Rig[]
  motions: Motion[]
  takes: Take[]
  packs: Pack[]
}
