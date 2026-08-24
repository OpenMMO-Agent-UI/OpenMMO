<script lang="ts">
  import { session } from '../../session.svelte'
  import { stage } from '../../viewer/current.svelte'
  import { retargetSharedClips } from '../../viewer/shared-clips'
  import { ANIM_COLUMNS, SHARED_PACK_CLIPS, sharedAnimDefaults, splitClipList } from '../../game/clips'
  import { modelPathFor } from '../../game/paths'

  let retargeting = $state(false)
  let retargetError = $state('')

  let ownClips = $derived(session.result?.stats.animations ?? [])
  let options = $derived(session.sharedAnims ? SHARED_PACK_CLIPS : ownClips)

  function setShared(on: boolean) {
    session.sharedAnims = on
    stage.sharedClips = []
    if (on) session.csvValues = { ...session.csvValues, ...sharedAnimDefaults(session.attackStyle) }
    session.applyDerivedValues()
  }

  function setStyle(style: 'weapon' | 'claw') {
    session.attackStyle = style
    if (session.sharedAnims) session.csvValues = { ...session.csvValues, ...sharedAnimDefaults(style) }
  }

  /** Build the clips through the game's own retargeting, then play one. */
  async function retarget() {
    const scene = stage.viewport?.subjectScene
    if (!scene) return
    retargeting = true
    retargetError = ''
    try {
      const wanted = new Set<string>()
      for (const column of ANIM_COLUMNS) {
        for (const clip of splitClipList(session.csvValues[column] ?? '')) wanted.add(clip)
      }
      stage.sharedClips = await retargetSharedClips({
        modelPath: modelPathFor(session.kind, session.id || 'preview'),
        scene,
        clipNames: [...wanted],
        dieClip: session.csvValues.animDie,
        corpseGroundOffset: Number(session.csvValues.corpseGroundOffset ?? 0),
      })
      play(session.csvValues.animIdle ?? 'idle1')
    } catch (error) {
      retargetError = error instanceof Error ? error.message : String(error)
    } finally {
      retargeting = false
    }
  }

  function play(name: string) {
    const pool = session.sharedAnims ? stage.sharedClips : (stage.viewport?.subjectClips ?? [])
    const clip = pool.find((entry) => entry.name === name)
    stage.playing = clip ? name : ''
    stage.viewport?.playClip(clip ?? null)
  }
</script>

<section class="panel">
  <h3>Where the animation comes from</h3>
  <div class="segmented">
    <button aria-pressed={session.sharedAnims} onclick={() => setShared(true)}>Shared packs</button>
    <button aria-pressed={!session.sharedAnims} onclick={() => setShared(false)}>
      Its own clips ({ownClips.length})
    </button>
  </div>
  <p class="hint">
    {#if session.sharedAnims}
      The rig plays <code>locomotion.glb</code> and <code>combat_melee.glb</code> retargeted at runtime,
      cached once per model. It needs the character bone names — {session.missingCore.length} core bones are
      still unmapped. There is no hit reaction in those packs, so <code>animHit</code> stays empty.
    {:else}
      The clips shipped inside the model are used as they are. The rig can be named anything.
    {/if}
  </p>
</section>

{#if session.sharedAnims}
  <section class="panel">
    <h3>Attack style</h3>
    <div class="segmented">
      <button aria-pressed={session.attackStyle === 'weapon'} onclick={() => setStyle('weapon')}>Weapon (slash1)</button>
      <button aria-pressed={session.attackStyle === 'claw'} onclick={() => setStyle('claw')}>Claws (claw1|claw2)</button>
    </div>
    <p class="hint">
      <code>animAttack</code> can list alternatives with <code>|</code> — the client picks one per swing and
      the server holds for the longest.
    </p>
  </section>
{/if}

<section class="panel">
  <h3>Clip per column</h3>
  {#each ANIM_COLUMNS as column (column)}
    <div class="clip-row">
      <label for={`clip-${column}`}>{column}</label>
      <input
        id={`clip-${column}`}
        list="clip-options"
        value={session.csvValues[column] ?? ''}
        oninput={(e) => (session.csvValues = { ...session.csvValues, [column]: e.currentTarget.value })}
      />
      <button
        class="btn tiny"
        disabled={!session.csvValues[column]}
        onclick={() => play(splitClipList(session.csvValues[column] ?? '')[0])}
      >
        ▶
      </button>
    </div>
  {/each}
  <datalist id="clip-options">
    {#each options as option, i (i)}
      <option value={option}></option>
    {/each}
  </datalist>
</section>

<section class="panel">
  <h3>Preview</h3>
  {#if session.sharedAnims}
    <div class="btn-row">
      <button class="btn btn-primary" onclick={retarget} disabled={retargeting || !stage.viewport}>
        {retargeting ? 'Retargeting…' : 'Retarget and play'}
      </button>
      <button class="btn" onclick={() => play('')} disabled={!stage.playing}>Stop</button>
    </div>
    <p class="hint">
      This runs the client's own <code>loadSharedPackClipsForModel</code> — the same retargeting and the same
      per-clip grounding lift the game applies, against the same pack files. If the limbs stretch or the body
      sinks here, they will in game too.
    </p>
    {#if stage.sharedClips.length > 0}
      <div class="btn-row">
        {#each stage.sharedClips as clip, i (i)}
          <button class="btn tiny" aria-pressed={stage.playing === clip.name} onclick={() => play(clip.name)}>
            {clip.name}
            <span class="num dim">{clip.duration.toFixed(2)}s</span>
          </button>
        {/each}
      </div>
    {/if}
  {:else}
    <div class="btn-row">
      {#each ownClips as clip, i (i)}
        <button class="btn tiny" aria-pressed={stage.playing === clip} onclick={() => play(clip)}>{clip}</button>
      {/each}
    </div>
  {/if}
  {#if retargetError}
    <p class="error" style="margin-top: 10px">{retargetError}</p>
  {/if}
</section>

<style>
  .clip-row {
    display: grid;
    grid-template-columns: 118px 1fr 30px;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }

  .clip-row label {
    font-size: 11.5px;
    color: var(--muted);
  }

  .clip-row input {
    padding: 3px 6px;
    font-size: 11.5px;
  }

  .tiny {
    padding: 3px 8px;
    font-size: 11px;
  }

  .tiny[aria-pressed='true'] {
    border-color: var(--accent);
    color: var(--accent);
  }

  .dim {
    color: var(--dim);
  }
</style>
