<script lang="ts">
  /**
   * What plays for the current motion, and what else could.
   *
   * There is no separate "audition, then decide" step. Every tile here is
   * immediately playable — click one and it is what the sheet shows, upload
   * one and it takes over the same way. The pack tile is not a fallback
   * bolted onto the strip; it is just the take that ships with the game,
   * shown the same way the others are.
   *
   * A written pack gets none of this. It already is a decision — the take you
   * chose, baked in — so offering the take library here would mean the same
   * physical file, `takes/walk/Walk.fbx`, showing up as a live "swap this in"
   * option under a result you already finished judging. Wanting to try a
   * different take for `walk` means going back to `walk`'s own pack; a
   * written pack is a fixed point to compare everything else against, not
   * another place to keep deciding from.
   */
  import { currentOverride, currentPack, session, slotsOverriding, takesFor } from '../session.svelte'

  let {
    onactivate,
    onadded,
    onremove,
  }: {
    onactivate: (takePath: string | null) => void
    onadded: (paths: string[]) => void
    onremove: (path: string) => void
  } = $props()

  /** The take whose tile is asking to be confirmed before it goes. */
  let confirming = $state('')

  let picker = $state<HTMLInputElement | null>(null)
  let fileUnderMotion = $state(true)
  let dropping = $state(false)
  let adding = $state(false)
  let addError = $state('')

  let pack = $derived(currentPack())
  /** A written candidate is a finished comparison point, not another place to
   *  keep deciding from — no take library, no upload, on that pack alone. */
  let readOnly = $derived(!!pack && !pack.shipped)
  let takes = $derived(session.motion && !readOnly ? takesFor(session.motion) : [])
  let active = $derived(currentOverride() ?? null)
  let destination = $derived(fileUnderMotion && session.motion ? `takes/${session.motion}/` : 'takes/')

  function kb(bytes: number): string {
    return bytes > 1048576 ? `${(bytes / 1048576).toFixed(1)} MB` : `${Math.round(bytes / 1024)} KB`
  }

  async function add(files: FileList | File[] | null) {
    const chosen = [...(files ?? [])]
    if (chosen.length === 0) return
    adding = true
    addError = ''
    try {
      const body = new FormData()
      body.set('filedUnder', fileUnderMotion ? session.motion : '')
      for (const file of chosen) body.append('files', file)
      const response = await fetch('/api/takes', { method: 'POST', body })
      const result = await response.json()
      if (!response.ok) throw new Error(result.error ?? 'The copy failed.')
      if (result.skipped.length > 0) {
        addError = `Not a model file, skipped: ${result.skipped.join(', ')}`
      }
      onadded(result.written)
    } catch (thrown) {
      addError = thrown instanceof Error ? thrown.message : String(thrown)
    } finally {
      adding = false
      if (picker) picker.value = ''
    }
  }

  /**
   * Removal asks first, on the tile itself.
   *
   * A take can override more than one motion, so the confirm says which —
   * undoing that by accident costs a choice you already made and would have
   * no way of knowing you had lost.
   */
  function askRemove(path: string, event: MouseEvent) {
    event.stopPropagation()
    confirming = confirming === path ? '' : path
  }

  function confirmRemove(path: string, event: MouseEvent) {
    event.stopPropagation()
    confirming = ''
    onremove(path)
  }

  function drop(event: DragEvent) {
    event.preventDefault()
    dropping = false
    if (readOnly) return
    void add(event.dataTransfer?.files ?? null)
  }
</script>

<section
  class="strip"
  class:dropping
  ondragover={(event) => {
    if (readOnly) return
    event.preventDefault()
    dropping = true
  }}
  ondragleave={() => (dropping = false)}
  ondrop={drop}
  role="presentation"
>
  <header>
    <div class="title">
      <span class="eyebrow">Playing</span>
      <h2>{session.motion || '—'}</h2>
    </div>
    {#if !readOnly}
      <label class="filing" title={`New takes are copied into ${destination}`}>
        <input type="checkbox" bind:checked={fileUnderMotion} disabled={!session.motion} />
        <span>file under {session.motion || 'motion'}</span>
      </label>
    {/if}
  </header>

  <input
    bind:this={picker}
    type="file"
    multiple
    accept=".glb,.gltf,.fbx,model/gltf-binary"
    onchange={(event) => add(event.currentTarget.files)}
    hidden
  />

  <div class="rail" role="listbox" aria-label="Sources for this motion">
    {#if pack}
      <button
        role="option"
        aria-selected={active === null}
        class="take pack"
        class:active={active === null}
        class:still={readOnly}
        onclick={() => !readOnly && onactivate(null)}
      >
        <span class="take-name">{pack.name}</span>
        <span class="take-meta">{pack.shipped ? 'shipped' : 'written'}</span>
      </button>
    {/if}

    {#each takes as take (take.path)}
      {@const overriding = slotsOverriding(take.path)}
      <div class="slot" class:confirming={confirming === take.path}>
        <button
          role="option"
          aria-selected={active === take.path}
          class="take"
          class:active={active === take.path}
          onclick={() => onactivate(take.path)}
        >
          <span class="take-name">{take.name}</span>
          <span class="take-meta num">
            {take.filedUnder ? take.filedUnder : 'unfiled'} · {kb(take.bytes)}
          </span>
        </button>

        <button
          class="remove"
          aria-label={`Remove ${take.name}`}
          title={`Remove ${take.path}`}
          onclick={(event) => askRemove(take.path, event)}
        >×</button>

        {#if confirming === take.path}
          <div class="confirm">
            <span class="confirm-text">
              {overriding.length > 0
                ? `Also reverts ${overriding.map((slot) => `${slot.motion} (${slot.pack})`).join(', ')} to their pack default.`
                : 'Delete this file?'}
            </span>
            <div class="confirm-row">
              <button class="mini" onclick={(event) => confirmRemove(take.path, event)}>Remove</button>
              <button class="mini ghost" onclick={(event) => askRemove(take.path, event)}>Keep</button>
            </div>
          </div>
        {/if}
      </div>
    {/each}

    {#if !readOnly}
      <button class="add" onclick={() => picker?.click()} disabled={adding}>
        <span class="plus" aria-hidden="true">+</span>
        <span class="add-label">{adding ? 'Copying…' : 'Upload to replace'}</span>
        <span class="add-meta">from your computer &rarr; {destination}</span>
      </button>
    {/if}
  </div>

  {#if readOnly}
    <p class="empty">
      <strong>{pack?.name}</strong> is a written pack — a fixed comparison point, not a place to keep
      deciding from. To try a different take for <code>{session.motion || 'walk'}</code>, decide it from
      {session.motion || 'walk'}'s own pack instead.
    </p>
  {:else if takes.length === 0}
    <p class="empty">
      Playing <strong>{pack?.name ?? 'nothing'}</strong>'s own clip — nothing has been uploaded for
      <code>{session.motion || 'walk'}</code> yet. Pick a file, or drop one anywhere on this strip, and it
      takes over immediately. Download from Mixamo <strong>with skin</strong>.
    </p>
  {/if}

  {#if addError}<p class="problem">{addError}</p>{/if}
  {#if session.takeProblem}<p class="problem">{session.takeProblem}</p>{/if}
</section>

<style>
  .strip {
    position: relative;
    border-bottom: 1px solid var(--line);
    padding: 12px 16px 14px;
    flex: 0 0 auto;
  }

  /* The whole strip is the drop target, so there is nothing to aim at. */
  .strip.dropping::after {
    content: 'Drop to replace';
    position: absolute;
    inset: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    font-size: 10px;
    color: var(--signal);
    background: rgba(14, 22, 33, 0.9);
    border: 1px dashed var(--signal);
    pointer-events: none;
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    margin-bottom: 10px;
  }

  .title {
    display: flex;
    align-items: baseline;
    gap: 9px;
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: 0.06em;
    color: var(--chalk);
  }

  .filing {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10.5px;
    color: var(--faint);
    cursor: pointer;
  }

  .filing input {
    width: 11px;
    height: 11px;
    margin: 0;
    accent-color: var(--signal);
  }

  .rail {
    display: flex;
    gap: 8px;
    overflow-x: auto;
    padding-bottom: 4px;
  }

  .slot {
    position: relative;
    flex: 0 0 auto;
    display: flex;
  }

  /* The × only appears on the tile you are pointing at, so the strip stays a
     row of takes rather than a row of controls. */
  .remove {
    position: absolute;
    top: 0;
    right: 0;
    width: 17px;
    height: 17px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: var(--panel);
    border: 0;
    border-left: 1px solid var(--line);
    border-bottom: 1px solid var(--line);
    color: var(--faint);
    font-size: 13px;
    line-height: 1;
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms, color 120ms;
  }

  .slot:hover .remove,
  .remove:focus-visible {
    opacity: 1;
  }

  .remove:hover {
    color: var(--halt);
  }

  .confirm {
    position: absolute;
    inset: 0;
    z-index: 2;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 5px;
    padding: 6px 8px;
    background: var(--ink);
    border: 1px solid var(--halt);
  }

  .confirm-text {
    font-size: 9.5px;
    line-height: 1.3;
    color: var(--dim);
  }

  .confirm-row {
    display: flex;
    gap: 5px;
  }

  .mini {
    padding: 2px 7px;
    font-size: 10px;
    background: none;
    border: 1px solid var(--halt);
    color: var(--halt);
    cursor: pointer;
  }

  .mini.ghost {
    border-color: var(--line);
    color: var(--dim);
  }

  .take,
  .add {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 0 0 auto;
    min-width: 132px;
    max-width: 210px;
    padding: 7px 10px;
    border-radius: 0;
    text-align: left;
    cursor: pointer;
    transition: border-color 120ms, background 120ms;
  }

  .take {
    width: 100%;
    background: var(--panel);
    border: 1px solid var(--line);
  }

  .take:hover {
    border-color: var(--faint);
  }

  .take-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--chalk);
  }

  .take-meta {
    font-size: 10px;
    color: var(--faint);
  }

  /* The one thing actually playing right now — one visual state, not two. */
  .take.active {
    border-color: var(--signal);
    background: var(--raised);
  }

  .take.active .take-name {
    color: var(--signal);
  }

  /* The pack tile sits first and reads as the resting state — dashed, not
     solid, until it is the one active. */
  .take.pack {
    border-style: dashed;
  }

  .take.pack .take-meta {
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  /* A written pack's tile: still shows what is playing, but there is nothing
     to switch to, so it does not invite a click. */
  .take.still {
    cursor: default;
  }

  .take.still:hover {
    border-color: var(--signal);
  }

  /* An empty frame at the end of the strip — the same size as a take, waiting
     for one. */
  .add {
    position: relative;
    background: transparent;
    border: 1px dashed var(--line);
    color: var(--dim);
    padding-left: 26px;
  }

  .add:hover:not(:disabled) {
    border-color: var(--faint);
    color: var(--chalk);
  }

  .add:disabled {
    opacity: 0.5;
    cursor: progress;
  }

  .plus {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    font-size: 15px;
    line-height: 1;
    color: var(--faint);
  }

  .add-label {
    white-space: nowrap;
  }

  .add-meta {
    font-size: 10px;
    color: var(--faint);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .empty,
  .problem {
    margin: 8px 0 0;
    font-size: 11px;
    color: var(--dim);
    max-width: 76ch;
  }

  .problem {
    color: var(--halt);
  }

  .empty strong {
    font-weight: 600;
    color: var(--chalk);
  }

  code {
    color: var(--chalk);
  }
</style>
