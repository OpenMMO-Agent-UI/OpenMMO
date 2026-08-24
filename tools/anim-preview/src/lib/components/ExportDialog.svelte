<script lang="ts">
  import { currentPack, packDecisions, session } from '../session.svelte'
  import { buildPack } from '../export-pack'

  let { onclose, onwritten }: { onclose: () => void; onwritten: () => void } = $props()

  let fileName = $state('')
  let building = $state(false)
  let error = $state('')
  let written = $state('')

  /**
   * Only the pack currently on screen, not the whole ladder.
   *
   * A written pack has one clip per name, same as any other — so if
   * `locomotion` and `locomotion2` each have their own `walk` override, there
   * is no single sensible file that could hold both. Scoping to `session.pack`
   * sidesteps the ambiguity entirely: within one pack's own motions, names are
   * unique by construction, and "write" means "bundle what I have decided for
   * this one".
   */
  let decisions = $derived(packDecisions(session.pack))
  let replaced = $derived(decisions.filter((decision) => decision.takePath))
  let unreplaced = $derived(decisions.filter((decision) => !decision.takePath))

  async function write() {
    building = true
    error = ''
    written = ''
    try {
      const pack = await buildPack(decisions, currentPack()?.url ?? null)
      const body = new FormData()
      body.set('fileName', fileName)
      body.set('glb', new Blob([pack.bytes as BlobPart], { type: 'model/gltf-binary' }), 'pack.glb')
      const response = await fetch('/api/export', { method: 'POST', body })
      const result = await response.json()
      if (!response.ok) throw new Error(result.error ?? 'The write failed.')
      written = result.file
      onwritten()
    } catch (thrown) {
      error = thrown instanceof Error ? thrown.message : String(thrown)
    } finally {
      building = false
    }
  }
</script>

<div class="scrim" role="presentation" onclick={onclose}></div>
<div class="sheet" role="dialog" aria-modal="true" aria-label="Write pack">
  <span class="eyebrow">Write pack</span>
  <p class="lede">
    From <strong>{session.pack}</strong>: all {decisions.length} motion{decisions.length === 1 ? '' : 's'},
    {replaced.length} replaced, on the rig from <strong>{replaced[0]?.takePath ?? '—'}</strong>.
  </p>
  <p class="note">
    Only {session.pack}'s own motions go in this file — switch to a different group on the ladder to write that
    one separately.
  </p>

  {#if unreplaced.length > 0}
    <p class="note">
      Carried over unchanged from {session.pack}: {unreplaced.map((d) => d.motion).join(', ')}
    </p>
  {/if}

  <label class="field">
    <span class="eyebrow">File name</span>
    <div class="input-row">
      <span class="prefix">client/public/models/animations/</span>
      <input
        bind:value={fileName}
        placeholder="mixamo_pass1"
        spellcheck="false"
        autocomplete="off"
        onkeydown={(e) => e.key === 'Enter' && fileName && !building && write()}
      />
      <span class="suffix">.glb</span>
    </div>
  </label>
  <p class="note">
    The packs the game loads are off limits here. Adopting this one means renaming it over
    <code>locomotion.glb</code> yourself.
  </p>

  {#if error}<p class="bad">{error}</p>{/if}
  {#if written}<p class="good">Wrote {written}.</p>{/if}

  <div class="row">
    <button class="btn" onclick={onclose}>{written ? 'Close' : 'Cancel'}</button>
    <button class="btn btn-key" disabled={building || !fileName || replaced.length === 0} onclick={write}>
      {building ? 'Building…' : 'Write'}
    </button>
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    background: rgba(6, 11, 17, 0.72);
    z-index: 40;
  }

  .sheet {
    position: fixed;
    z-index: 41;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(560px, calc(100vw - 32px));
    padding: 20px;
    background: var(--panel);
    border: 1px solid var(--line);
  }

  .lede {
    margin: 8px 0 4px;
    color: var(--chalk);
  }

  .lede strong {
    font-weight: 600;
    color: var(--signal);
  }

  .note {
    margin: 6px 0 0;
    font-size: 11px;
    color: var(--dim);
  }

  .field {
    display: block;
    margin-top: 16px;
  }

  .input-row {
    display: flex;
    align-items: center;
    margin-top: 5px;
    border: 1px solid var(--line);
    background: var(--ink);
  }

  .prefix,
  .suffix {
    padding: 6px 0 6px 8px;
    font-size: 11px;
    color: var(--faint);
    white-space: nowrap;
  }

  .suffix {
    padding: 6px 8px 6px 0;
  }

  .input-row input {
    flex: 1 1 auto;
    min-width: 0;
    padding: 6px 2px;
    background: none;
    border: 0;
    outline: none;
    color: var(--signal);
  }

  .row {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 18px;
  }

  .bad {
    margin: 12px 0 0;
    color: var(--halt);
  }

  .good {
    margin: 12px 0 0;
    color: var(--signal);
  }

  code {
    color: var(--chalk);
  }
</style>
