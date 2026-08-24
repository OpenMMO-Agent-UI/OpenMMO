# anim-preview

A contact sheet for animation takes. Every humanoid rig in the game on one
screen, all playing the same motion, retargeted the way the game retargets it —
so you can tell which downloaded take is the good one before it goes into a
pack.

```sh
cd tools/anim-preview
npm install
npm run dev        # http://localhost:5173
npm test           # the core, against the models and packs the game ships
```

It writes one file, only when you ask, only into
`client/public/models/animations/`, and never over a pack the game loads.

## The loop

The ladder on the left is the only place you choose what to look at. There is
no second control for where it comes from — every motion always has something
playing (its own pack's own clip, until a take replaces it), so picking the
motion is the whole action.

1. Click a motion. It plays, on all 28 rigs, immediately — the pack's own clip,
   the first time.
2. **Upload to replace** on the strip below opens a file picker, and dropping a
   file anywhere on the strip does the same. It takes over the moment it
   lands — no separate "use this one" step. Download from Mixamo **with
   skin**.
3. Not right? Upload another. The new one replaces it, immediately, same as
   the first did. Click the dashed pack tile to go back to the shipped clip.
4. Every take you have tried for this motion stays on the strip — click any of
   them, including one you have moved away from, to make it the one playing
   again.
5. When enough motions are replaced, **Write pack** builds a GLB out of exactly
   those replacements and names it what you tell it.

## Where takes live

Picked files are copied into `takes/`, under a folder named for the motion when
"file under" is ticked, and at the root when it is not. A take at the root is
offered for every motion, so a download you have not decided the use of yet can
be tried anywhere.

They are copied rather than held as object URLs for the session. A replacement
that evaporated on reload would be a trap when the job is thirty-two motions
long and takes more than one sitting — and everything downstream already
addresses a take by its path under `takes/`, so a copy keeps one way of naming a
take instead of two. Nothing in `takes/` is committed.

A file already there is never overwritten; a second `walk.fbx` lands as
`walk-2.fbx`.

## What is on the sheet

28 rigs: 12 monsters and 16 player characters.

The monsters come from `data-src/monsters.csv`, one entry per distinct GLB —
`orc_boss` is `orc.glb` at 1.4×, so it is the same rig and appears once. The
characters are every GLB in `client/public/models/characters/`. Add a monster to
the CSV and it turns up here without touching this tool.

`scp939` is left out entirely. It is a quadruped, and a human walk cycle on it
is not a defect anyone can judge.

A rig missing a **core bone** — hips, spine, neck, head, or any of the twelve
limb bones — does not get a cell at all. It drops to the **Off the sheet** strip
below the grid instead, named, with the bone it lacks written next to it, and a
`+` to bring it back if you want to look closer. A cell that never plays
anything does not belong in a grid whose whole point is "everything here is
worth comparing."

Fingers are deliberately not part of that test: `ogre` is rigged with 33 bones
and `gnoll` with 57, and dropping those two would hide how their walk and their
swing actually retarget, which is the thing being judged. Their fingers will not
move, and no amount of algorithm work will change that — that needs a re-rig.

When every missing bone turns out to be present under another spelling, the
strip says **rename x → y** in amber instead of naming a count in red. The rig
still will not retarget — the game matches bone names exactly — but a rename is
not a re-rig, and the two should not read the same. As the repo stands:

| rig | state |
|---|---|
| `stone_golem` | has `neck`, needs `Neck` — one rename away |
| `kobold`, `goblin`, `orc`, `orc_female` | 13 of 16 core bones absent; only `Hips`, `Spine` and `RightHand` line up |

The four on the bottom row are the verse8.io rigs, and they carry their own
baked clips (`Death01_Rig` and friends) rather than using the packs.

A rig you hide by hand (the checkbox — now a click on its own name) lands in
the same strip, unlabelled beyond its name. One place to look for "where did
that go," whether the sheet excluded it or you did.

## The 32 motions

Read out of all five packs themselves — `locomotion`, `combat_melee`, `fishing`,
`offhand`, `social` — not from the `AnimationName` enum. The enum and the packs
disagree, and the packs are what the shared-animation path actually loads:

- in the packs, not in the enum — `combat_idle`, `claw1`, `claw2`, which the
  monsters do use
- in the enum, in no pack — `attack1`–`attack4`, which come off base models

## One narrow door

The sheet does not have its own retargeting. It calls the client's
`characterAnimationUtils` through the `$game` alias, and `three` is deduped to
this tool's copy so there is only ever one three.js instance in the page. A
second implementation would drift from the game's within a release, and this
tool would then be showing you takes the game plays differently — which is the
one thing it must not do.

`src/game-bridge.d.ts` describes those functions locally rather than dragging
the client's tsconfig into this one. `test/game-bridge.test.ts` loads the real
module to catch a change of shape, and `test/retarget.test.ts` runs an actual
retarget through it and checks the clip comes back rebound and changed — a
passing name-and-duration check would not notice the client quietly handing
clips back untouched.

### The one change to the client

`loadSharedPackClipsForModel` grew a fifth argument, `packPaths`, defaulting to
the two packs it always loaded. The game passes nothing and behaves exactly as
before. This tool passes a candidate pack, which is how a freshly written one
gets played against the sheet without overwriting the one the game ships.

## Mixamo names every export the same thing

A default Mixamo download's clip is named `Armature|mixamo.com|Layer0` —
checked with Blender against a real download in `takes/`. That collides with
the game's own retarget cache: `retargetAnimationsForCharacterModel` keys a
result on `<skeleton pair>::<clip name>`, not on which file the clip came from,
and two takes of the same Mixamo character share both halves of that key. Left
alone, auditioning a second take of the same character would silently replay
the first take's cached result — which looks exactly like "the old take won't
play" once you have moved on to a third.

`retargetTake` renames every clip to something take-unique before handing it
through the narrow door, so this can't happen. `test/retarget-collision.test.ts`
reproduces the exact collision and fails without the rename.

## What it does to a download first

Two things about a Mixamo file stop it retargeting, and both fail silently:

- **`mixamorig:` prefixes.** The game matches source bones to target bones by
  exact name, and the repo's rigs have the prefix stripped. Left on, nothing
  matches, and the clip is handed back unretargeted — it plays, and it plays
  wrong.
- **Centimetres.** FBX arrives 100× too big. The hip correction works off the
  bone's local rest height, so an un-scaled source throws the hip track metres
  into the air.

Both are fixed on load, bones and hip track together. A take with no skinned
mesh is reported rather than worked around: download it again with skin.

## The pack it writes

One rig plus named clips — that is all the game reads out of `locomotion.glb`.
The rig comes from the first replaced motion's take; every clip is put onto it through the
game's own retargeter, so a take from somewhere else is corrected instead of
quietly writing a pack with two skeletons in it. Materials and textures are
dropped, because nothing ever renders the pack rig.

One file, not two. The `locomotion` / `combat_melee` split exists because those
were exported from different armatures — 33 bones and 69. A pack built here has
one rig throughout, and `packPaths` takes any number of files.

## What it deliberately does not do

- **Touch mixamo.com.** No API exists, and a click-automation script against a
  third-party site would rot. Downloading is a one-time job per take; this tool
  starts where the download lands.
- **Overwrite a pack the game loads.** Adopting a candidate means renaming it
  over `locomotion.glb` yourself, with the game stopped.
- **Fix the retargeting.** It shows you what the retargeting does. Whether the
  stretching on `hobgoblin` is worth fixing in `characterAnimationUtils.ts` is
  the next job, and this is the instrument for judging it.
- **Touch `monsters.csv`.** Wiring a monster to different clips is rig-importer's
  job.
- **Commit.**

## A number worth knowing

`walkSpeed` = `1.8 × hips / 1.165`. The pack rig's hips sit 1.165 m up and it
walks 1.8 m/s; retargeting moves rotations only, so stride scales with hip
height. Each cell prints its rig's hip height for that reason. Changing the pack
rig changes every monster's walk speed — `test/compat.test.ts` holds the
formula against the values in `doc/assets/monsters.md`.
