<script lang="ts">
  import { onMount } from 'svelte'
  import { ContactSheet } from '$lib/viewer/sheet'
  import { checkCompatibility, readSkeleton } from '$lib/bones/compat'
  import { retargetPack, retargetTake } from '$lib/retarget'
  import { forgetTake, loadTake } from '$lib/takes'
  import {
    currentOverride,
    currentPack,
    hiddenRigs,
    isIncompatible,
    motionsByPack,
    overrideCount,
    overrideKey,
    session,
    slotsOverriding,
  } from '$lib/session.svelte'
  import type { Library, Rig } from '$lib/types'
  import MotionLadder from '$lib/components/MotionLadder.svelte'
  import TakeStrip from '$lib/components/TakeStrip.svelte'
  import RigCell from '$lib/components/RigCell.svelte'
  import HiddenRigs from '$lib/components/HiddenRigs.svelte'
  import ExportDialog from '$lib/components/ExportDialog.svelte'

  let canvas = $state<HTMLCanvasElement | null>(null)
  let grid = $state<HTMLElement | null>(null)
  let sheet: ContactSheet | null = null
  let exporting = $state(false)
  let cellSize = $state(240)
  /** Focus mode trades columns for detail, so it allows much larger cells. */
  const FOCUS_MAX_CELL = 700

  /**
   * Bumped every time the selection changes. A run that finds its generation
   * stale drops what it was doing — switching motions while 28 rigs are still
   * retargeting must not leave half the sheet playing the previous one.
   */
  let generation = 0
  const clips = new Map<string, import('three').AnimationClip>()

  let groups = $derived(motionsByPack())
  let monsters = $derived(session.rigs.filter((rig) => rig.kind === 'monster'))
  let characters = $derived(session.rigs.filter((rig) => rig.kind === 'character'))
  let shown = $derived(session.rigs.filter((rig) => session.shown[rig.id]))
  let hidden = $derived(hiddenRigs())

  onMount(() => {
    if (!canvas) return
    sheet = new ContactSheet(canvas)
    sheet.setClip(grid)
    const resize = () => sheet?.resize()
    window.addEventListener('resize', resize)
    load()
    return () => {
      window.removeEventListener('resize', resize)
      sheet?.dispose()
      sheet = null
    }
  })

  async function load() {
    try {
      const response = await fetch('/api/library')
      const library = (await response.json()) as Library
      session.rigs = library.rigs
      session.motions = library.motions
      session.takes = library.takes
      session.packs = library.packs
      session.motion = library.motions[0]?.name ?? ''
      session.pack = library.motions[0]?.pack ?? ''
      for (const rig of library.rigs) session.shown[rig.id] = true
    } catch (error) {
      session.error = error instanceof Error ? error.message : String(error)
    } finally {
      session.loading = false
    }
  }

  /** A cell has mounted its box; give it a scene and pull its rig in. */
  async function mountCell(id: string, element: HTMLElement) {
    if (!sheet) return
    sheet.register(id, element)
    const rig = session.rigs.find((entry) => entry.id === id)
    if (!rig) return
    session.status[id] = {
      loaded: false,
      boneCount: 0,
      hipsHeight: 0,
      missing: [],
      nearMisses: [],
      error: null,
    }
    try {
      const model = await sheet.load(id, rig.url)
      const skeleton = readSkeleton(model.scene)
      const { missing, nearMisses } = checkCompatibility(skeleton)
      session.status[id] = {
        loaded: true,
        boneCount: skeleton.boneCount,
        hipsHeight: skeleton.hipsHeight,
        missing,
        nearMisses,
        error: null,
      }
      // A rig the packs cannot drive is hidden rather than shown empty — it
      // moves to the strip below, with the bone it lacks named there.
      if (missing.length > 0) session.shown[id] = false
      if (session.shown[id]) void apply([rig], generation)
    } catch (error) {
      session.status[id] = {
        loaded: false,
        boneCount: 0,
        hipsHeight: 0,
        missing: [],
        nearMisses: [],
        error: error instanceof Error ? error.message : 'load failed',
      }
    }
  }

  /**
   * Put the current motion onto a set of rigs, one at a time.
   *
   * Sequential on purpose: each retarget clones two skeletons and walks every
   * keyframe, and firing 28 of those at once locks the tab for as long as it
   * takes. One at a time keeps the sheet drawing, and the rigs fill in visibly.
   */
  async function apply(rigs: Rig[], run: number) {
    for (const rig of rigs) {
      if (run !== generation) return
      if (!session.shown[rig.id]) continue
      const scene = sheet?.sceneFor(rig.id)
      if (!scene) continue

      const source = currentOverride() ?? `pack:${session.pack}`
      const key = `${rig.id}::${source}::${session.motion}`
      const cached = clips.get(key)
      if (cached) {
        sheet?.play(rig.id, cached)
        continue
      }

      session.working.add(rig.id)
      session.working = new Set(session.working)
      try {
        const clip = await build(rig, scene)
        if (run !== generation) return
        if (clip) {
          clips.set(key, clip)
          sheet?.play(rig.id, clip)
        } else {
          sheet?.play(rig.id, null)
        }
      } catch (error) {
        session.takeProblem = error instanceof Error ? error.message : String(error)
        sheet?.play(rig.id, null)
      } finally {
        session.working.delete(rig.id)
        session.working = new Set(session.working)
      }
    }
  }

  /**
   * The current motion's clip for one rig: the overriding take if one was
   * uploaded, otherwise the motion's own pack, unretouched.
   */
  async function build(rig: Rig, scene: import('three').Object3D) {
    // dying is the one motion grounded against its own rest pose — a body that
    // has fallen is held up by its thickness, not by its hips.
    const grounding = session.motion === 'dying' ? { restClip: session.motion } : {}

    const overridePath = currentOverride()
    if (!overridePath) {
      const pack = currentPack()
      if (!pack) return null
      const built = await retargetPack(rig.model, scene, [session.motion], [pack.url], grounding)
      return built.find((clip) => clip.name === session.motion) ?? null
    }

    const take = await loadTake(`/takes/${overridePath}`)
    if (take.problem) {
      session.takeProblem = `${overridePath}: ${take.problem}`
      return null
    }
    session.takeProblem = ''
    const source = take.clips[0]
    if (!source) return null
    const built = await retargetTake(scene, take.scene, [source], grounding)
    const clip = built[0]?.clone()
    if (clip) clip.name = session.motion
    return clip ?? null
  }

  /** Restart the whole sheet on the current motion. */
  function refresh() {
    generation += 1
    const run = generation
    for (const rig of session.rigs) {
      if (!session.shown[rig.id]) continue
      sheet?.play(rig.id, null)
    }
    void apply(session.rigs, run)
  }

  function pickMotion(name: string, pack: string) {
    session.motion = name
    session.pack = pack
    session.takeProblem = ''
    refresh()
  }

  /**
   * Give the sheet the whole window.
   *
   * Judging a retarget is looking closely at 28 small pictures, and the rail,
   * the strip and the two footers are all apparatus for choosing what to look
   * at rather than for looking. Folding them away roughly doubles the area the
   * cells get. The browser's own fullscreen is asked for on top of that, but
   * it is a bonus: if the request is refused the layout still collapses, so
   * the button always does something.
   */
  let focused = $state(false)
  /** The sheet size to come back to; focus mode's enlargement is a mode, not an edit. */
  let sizeBeforeFocus = 240

  async function toggleFocus() {
    focused = !focused
    if (focused) {
      // Reclaiming the chrome only fits *more* cells at the same size, which is
      // the opposite of looking closely. Grow them to spend the new room on
      // detail instead; the slider is right there if the guess is wrong.
      sizeBeforeFocus = cellSize
      cellSize = Math.min(FOCUS_MAX_CELL, Math.round(cellSize * 1.6))
    } else {
      cellSize = sizeBeforeFocus
    }
    try {
      if (focused && !document.fullscreenElement) await document.documentElement.requestFullscreen()
      else if (!focused && document.fullscreenElement) await document.exitFullscreen()
    } catch {
      // Refused (or unsupported). The collapsed layout is the part that matters.
    }
  }

  /** Esc out of the browser's fullscreen should leave focus mode too. */
  function syncFullscreen() {
    if (!document.fullscreenElement && focused) focused = false
  }

  /** Walk the ladder without the ladder on screen. */
  function stepMotion(delta: number) {
    const at = session.motions.findIndex((m) => m.name === session.motion && m.pack === session.pack)
    if (at === -1) return
    const next = session.motions[(at + delta + session.motions.length) % session.motions.length]
    if (next) pickMotion(next.name, next.pack)
  }

  /**
   * Make a take the source for the current (pack, motion) slot, or hand it
   * back to the pack default when given null. Immediate — there is no
   * separate audition step, because the sheet is already showing whatever
   * plays right now.
   *
   * Keyed on the pack too, not just the motion: `locomotion`'s `walk` and
   * `locomotion2`'s `walk` are two different candidates being judged, and
   * deciding one must not silently decide the other just because they share
   * a name.
   */
  function activate(takePath: string | null) {
    const key = overrideKey(session.pack, session.motion)
    if (takePath) {
      session.overrides = { ...session.overrides, [key]: takePath }
    } else {
      const { [key]: _dropped, ...rest } = session.overrides
      session.overrides = rest
    }
    session.takeProblem = ''
    refresh()
  }

  function toggleRig(id: string) {
    const on = !session.shown[id]
    session.shown[id] = on
    sheet?.setVisible(id, on)
    if (!on) {
      sheet?.play(id, null)
      return
    }
    const rig = session.rigs.find((entry) => entry.id === id)
    if (rig) void apply([rig], generation)
  }

  /**
   * Turning rigs on never reaches for one the packs cannot drive — "All" means
   * every rig worth looking at, not literally every rig on the roster. Turning
   * them off has no such exception: hiding is always exactly what it says.
   */
  function setGroup(rigs: Rig[], on: boolean) {
    const targets = on ? rigs.filter((rig) => !isIncompatible(rig.id)) : rigs
    for (const rig of targets) {
      session.shown[rig.id] = on
      sheet?.setVisible(rig.id, on)
      if (!on) sheet?.play(rig.id, null)
    }
    if (on) void apply(targets, generation)
  }

  /** Monsters / Characters: show this group, and only this group. */
  function showOnly(rigs: Rig[]) {
    const keep = new Set(rigs.map((rig) => rig.id))
    setGroup(session.rigs.filter((rig) => !keep.has(rig.id)), false)
    setGroup(rigs, true)
  }

  /** Re-read the library, and put the just-added take straight to work. */
  async function afterAdd(written: string[]) {
    const response = await fetch('/api/library')
    session.takes = ((await response.json()) as Library).takes
    if (written[0]) activate(written[0])
  }

  /**
   * Delete a take, and unpick it from everywhere it is still named.
   *
   * A take can override several motions, and be the thing a held rig is
   * playing, all at once. Removing the file without clearing those leaves an
   * override pointing at nothing — which would survive all the way into the
   * export and fail there instead of here.
   */
  async function removeTake(takePath: string) {
    const response = await fetch('/api/takes', {
      method: 'DELETE',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ path: takePath }),
    })
    const result = await response.json()
    if (!response.ok) {
      session.takeProblem = result.error ?? 'The take could not be removed.'
      return
    }

    forgetTake(`/takes/${takePath}`)
    for (const key of [...clips.keys()]) {
      if (key.includes(`::${takePath}::`)) clips.delete(key)
    }

    // Every (pack, motion) slot this take was overriding falls back to that
    // pack's own default.
    const orphaned = slotsOverriding(takePath)
    if (orphaned.length > 0) {
      const kept = { ...session.overrides }
      for (const slot of orphaned) delete kept[overrideKey(slot.pack, slot.motion)]
      session.overrides = kept
    }

    const library = (await (await fetch('/api/library')).json()) as Library
    session.takes = library.takes
    session.takeProblem = ''
    refresh()
  }

  async function afterExport() {
    const response = await fetch('/api/library')
    session.packs = ((await response.json()) as Library).packs
  }

  // Dragging anywhere on the sheet turns every rig together — comparing a
  // retarget across rigs means comparing them from one angle.
  let dragging = false
  let last = { x: 0, y: 0 }

  function down(event: PointerEvent) {
    dragging = true
    last = { x: event.clientX, y: event.clientY }
    ;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId)
  }

  function move(event: PointerEvent) {
    if (!dragging || !sheet) return
    const orbit = sheet.orbitState
    sheet.setOrbit({
      azimuth: orbit.azimuth - (event.clientX - last.x) * 0.008,
      elevation: orbit.elevation + (event.clientY - last.y) * 0.005,
    })
    last = { x: event.clientX, y: event.clientY }
  }

  function up(event: PointerEvent) {
    dragging = false
    ;(event.currentTarget as HTMLElement).releasePointerCapture(event.pointerId)
  }

  function wheel(event: WheelEvent) {
    if (!event.altKey || !sheet) return
    event.preventDefault()
    sheet.setOrbit({ dolly: sheet.orbitState.dolly * (1 + Math.sign(event.deltaY) * 0.1) })
  }
</script>

<svelte:window
  onfullscreenchange={syncFullscreen}
  onkeydown={(event) => {
    if (event.target instanceof HTMLInputElement) return
    if (event.key === 'Escape' && focused) toggleFocus()
    if (event.key === 'ArrowLeft') stepMotion(-1)
    if (event.key === 'ArrowRight') stepMotion(1)
  }}
/>

<canvas bind:this={canvas} class="stage-canvas"></canvas>

<div class="app" class:focused>
  <header class="bar">
    <div class="brand">
      <span class="mark" aria-hidden="true"></span>
      <h1>anim<span>preview</span></h1>
    </div>

    <div class="tally">
      <span class="eyebrow">Replaced</span>
      <span class="score num">{overrideCount()}<i>/{session.motions.length}</i></span>
      <button class="btn" onclick={toggleFocus}>Full screen</button>
      <button class="btn btn-key" disabled={overrideCount() === 0} onclick={() => (exporting = true)}>
        Write pack
      </button>
    </div>
  </header>

  <div class="body">
    <aside class="rail">
      <MotionLadder {groups} onpick={pickMotion} />
    </aside>

    <main>
      <TakeStrip onactivate={activate} onadded={afterAdd} onremove={removeTake} />

      <div class="sheet-head">
        <div class="filters">
          <span class="eyebrow">Rigs</span>
          <button class="btn" onclick={() => setGroup(session.rigs, true)}>All</button>
          <button class="btn" onclick={() => setGroup(session.rigs, false)}>None</button>
          <button class="btn" onclick={() => showOnly(monsters)}>Monsters</button>
          <button class="btn" onclick={() => showOnly(characters)}>Characters</button>
        </div>
        <div class="readout">
          <label class="size">
            <span class="eyebrow">Size</span>
            <input type="range" min="150" max="420" step="10" bind:value={cellSize} />
          </label>
          <span class="eyebrow num">{shown.length}/{session.rigs.length} shown</span>
        </div>
      </div>

      <div
        class="sheet"
        bind:this={grid}
        style={`--cell:${cellSize}px`}
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointercancel={up}
        onwheel={wheel}
        role="presentation"
      >
        {#if session.loading}
          <p class="note">Reading the roster…</p>
        {:else if session.error}
          <p class="note bad">{session.error}</p>
        {:else}
          {#each session.rigs as rig (rig.id)}
            <RigCell {rig} onmount={mountCell} ontoggle={toggleRig} />
          {/each}
        {/if}
      </div>

      <HiddenRigs rigs={hidden} onshow={toggleRig} />

      <p class="hint">Drag to turn every rig together. Alt-scroll to pull back.</p>
    </main>
  </div>

  <!-- Everything folded away above still has to be reachable, or focus mode is
       a dead end: which motion is playing, how to get to the next one, how big
       the cells are, and the way out. -->
  {#if focused}
    <div class="focus-bar">
      <button class="step" onclick={() => stepMotion(-1)} aria-label="Previous motion">&lsaquo;</button>
      <span class="focus-motion">{session.motion}</span>
      <button class="step" onclick={() => stepMotion(1)} aria-label="Next motion">&rsaquo;</button>
      <span class="focus-pack">{session.pack}</span>
      <input
        class="focus-size"
        type="range"
        min="150"
        max={FOCUS_MAX_CELL}
        step="10"
        bind:value={cellSize}
        aria-label="Cell size"
      />
      <span class="eyebrow num">{shown.length}/{session.rigs.length}</span>
      <button class="step out" onclick={toggleFocus} aria-label="Leave full screen">&times;</button>
    </div>
  {/if}
</div>

{#if exporting}
  <ExportDialog onclose={() => (exporting = false)} onwritten={afterExport} />
{/if}

<style>
  /* One context, stretched over the page; every cell is scissored out of it. */
  .stage-canvas {
    position: fixed;
    inset: 0;
    width: 100%;
    height: 100%;
    z-index: 0;
    pointer-events: none;
  }

  .app {
    position: relative;
    z-index: 1;
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 20px;
    height: 44px;
    padding: 0 14px;
    background: var(--panel);
    border-bottom: 1px solid var(--line);
    flex: 0 0 auto;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 9px;
  }

  .brand .mark {
    width: 7px;
    height: 14px;
    background: var(--signal);
  }

  h1 {
    margin: 0;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.2em;
    text-transform: uppercase;
    color: var(--chalk);
  }

  h1 span {
    color: var(--faint);
  }

  .tally {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-left: auto;
  }

  .score {
    font-size: 14px;
    font-weight: 600;
    color: var(--signal);
  }

  .score i {
    font-style: normal;
    font-size: 11px;
    color: var(--faint);
  }

  .body {
    display: flex;
    flex: 1 1 auto;
    min-height: 0;
  }

  .rail {
    flex: 0 0 var(--rail);
    display: flex;
    flex-direction: column;
    padding-top: 12px;
    background: var(--panel);
    border-right: 1px solid var(--line);
    overflow: hidden;
  }

  main {
    display: flex;
    flex-direction: column;
    flex: 1 1 auto;
    min-width: 0;
  }

  .sheet-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 8px 16px;
    border-bottom: 1px solid var(--line);
    flex: 0 0 auto;
  }

  .filters,
  .readout {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .size {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .size input {
    width: 88px;
    accent-color: var(--signal);
  }

  /* The grid has no gaps and the cells carry hairlines, so the whole thing
     reads as one sheet rather than a tray of floating cards. */
  .sheet {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(var(--cell), 1fr));
    /* Rows need a definite height. Sized to content, a cell is only as tall as
       its caption, the stage above it collapses to nothing, and the renderer is
       handed a rectangle with no height to scissor into. */
    grid-auto-rows: var(--cell);
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    align-content: start;
    border-top: 1px solid transparent;
    touch-action: none;
    cursor: grab;
  }

  .sheet:active {
    cursor: grabbing;
  }

  .note {
    grid-column: 1 / -1;
    margin: 0;
    padding: 24px 16px;
    color: var(--dim);
  }

  .bad {
    color: var(--halt);
  }

  .hint {
    margin: 0;
    padding: 6px 16px 7px;
    font-size: 10.5px;
    color: var(--faint);
    border-top: 1px solid var(--line);
    background: var(--panel);
    flex: 0 0 auto;
  }

  /* Focus mode: everything that exists to choose what to look at folds away,
     and only the looking is left. The grid is the one thing that stays. */
  .app.focused .bar,
  .app.focused .rail,
  .app.focused .sheet-head,
  .app.focused .hint {
    display: none;
  }

  /* The take strip and the off-the-sheet strip both. Their markup lives in
     their own components, so this has to reach through :global. */
  .app.focused :global(.strip) {
    display: none;
  }

  /* One floating strip carries what the folded chrome still owed: what is
     playing, how to step off it, how big the cells are, and the way out. */
  .focus-bar {
    position: fixed;
    z-index: 30;
    top: 10px;
    right: 12px;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 5px 8px;
    background: rgba(22, 32, 45, 0.92);
    border: 1px solid var(--line);
  }

  .focus-motion {
    min-width: 84px;
    text-align: center;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--signal);
  }

  .focus-pack {
    font-size: 10px;
    color: var(--faint);
    max-width: 110px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .step {
    padding: 1px 7px;
    font-size: 14px;
    line-height: 1.2;
    background: none;
    border: 1px solid transparent;
    color: var(--dim);
    cursor: pointer;
  }

  .step:hover {
    color: var(--chalk);
    border-color: var(--line);
  }

  .step.out:hover {
    color: var(--halt);
    border-color: var(--halt);
  }

  .focus-size {
    width: 96px;
    accent-color: var(--signal);
  }
</style>
