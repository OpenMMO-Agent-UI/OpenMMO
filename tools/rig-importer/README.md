# rig-importer

A step-by-step importer for rigged monster and character models. Drop the GLB or
FBX that came back from Mixamo, and walk out the other end with the model in
`client/public/models/`, a spawnable row in `data-src/monsters.csv`, and the
provenance entry `CLAUDE.md` asks for.

```sh
cd tools/rig-importer
npm install
npm run dev        # http://localhost:5173
npm test           # the core, against the models the game ships
```

It writes into the working tree. It never commits, and it never uploads.

## The steps

| Step | What it settles |
|---|---|
| Source | The file, the id, and where the model came from |
| Skeleton | Which source joint plays each of the game's bone names |
| Size & origin | Height in metres, baked; origin at the floor centre |
| Material | Undo importer damage, fit the texture budget |
| Animation | Its own clips, or the shared packs retargeted |
| Weapon | Where a weapon sits in the hand |
| Game data | The `monsters.csv` row |
| Validate | What blocks, and what needs a decision |
| Review & apply | The diff, then the write |

## How it is built

**Edits happen on the glTF container, not on a three.js scene.** `src/lib/gltf/`
parses the GLB into `{ json, bin }` and edits that directly — scale baked into
vertex, joint, inverse-bind and animation data; bones renamed; materials
repaired; textures re-encoded. The preview then loads the bytes that edit
produced, so what is on screen is what would be written. Nothing can drift
between them.

That also means most of the tool is plain TypeScript over a data structure, and
`npm test` checks it in node against `hobgoblin.glb`, `ogre.glb`, `kobold.glb`
and friends — models whose right answers are already recorded in
`doc/assets/monsters.md`.

**The pipeline is re-run from the pristine import on every change**
(`src/lib/pipeline.ts`). Dragging the height slider back and forth lands exactly
where it started; steps can be revisited in any order.

**One narrow door into the game's code.** The shared-pack preview calls the
client's own `loadSharedPackClipsForModel` through the `$game` alias, so the
retargeting and the per-clip grounding lift are the game's, not a second
implementation that would drift from it within a release. `three` is deduped to
this tool's copy so there is only ever one three.js instance in the page.
`src/game-bridge.d.ts` describes that function locally rather than dragging the
client's tsconfig into this one, and `test/game-bridge.test.ts` loads the real
module to catch a change of shape.

## Numbers it derives, and where they come from

All of these are recorded in `doc/assets/monsters.md`, and the tests reproduce
the shipped values from the shipped models.

- **walkSpeed** = `1.8 × hips / 1.165`. The shared pack's rig has its hips
  1.165 m up and walks 1.8 m/s. Retargeting moves rotations only, so stride —
  and the speed that does not skate — scales with hip height.
- **runSpeed** = `5.05 × hips / 1.165`, back-derived the same way.
- **weaponOffset** = 80% of how far the vertices majority-weighted to
  `RightHand` reach along the bone. The hand bone sits at the wrist. The other
  five grip axes — `weaponOffsetX`, `weaponOffsetZ` and `weaponRotation`
  (`rx|ry|rz` degrees) — are fitted by eye against the real weapon model, and
  the client applies all six as the position and XYZ Euler of the weapon
  parented to the bone.
- **Origin** = lowest vertex on y=0, bounding box centred on x/z.

## Budgets

Measured off the repo: every character and monster sits at or under 10k
triangles, one material, at most three 1024² textures. Past that is a yellow
finding — deliberate, not forbidden. Over 2048² is a red one.

## What it deliberately does not do

- **Reduce polygons.** Browser-side simplification wrecks skin weights and UV
  seams, and Meshy and Tripo both remesh to 10k on the way out. It reports the
  number and points upstream.
- **Upload anything.** `tools/push-assets.sh` syncs the whole working tree to a
  public Hugging Face dataset with `--delete`. That stays a maintainer action.
- **Edit the client's TypeScript.** Characters are wired up in `modelPaths.ts`,
  `CLASS_GENDER_MODELS`, `CharacterClass` and on the server. The apply step
  lists them; it does not touch them.
- **Commit.** It prints `git status` and stops.

## Drafts

Work in progress lives in `.drafts/<id>/` (git-ignored): the original upload, the
processed GLB, the concept art, and every decision made about them. Close the
browser and pick it up from the start screen.
