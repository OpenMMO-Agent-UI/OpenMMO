<script lang="ts">
  import { session } from '../../session.svelte'
  import { removeDraft } from '../../api'
  import ModelDrop from '../ModelDrop.svelte'
  import type { ModelKind } from '../../game/paths'

  // The kind only decides which directory the GLB lands in and whether there is
  // a CSV row at the end, so it can be picked here and changed later.
  let kind = $state<ModelKind>('monster')

  $effect(() => {
    if (session.draftId === '') session.startNew(kind)
  })

  function choose(next: ModelKind) {
    kind = next
    session.kind = next
  }

  async function discard(id: string) {
    await removeDraft(id)
    session.drafts = session.drafts.filter((draft) => draft.id !== id)
  }
</script>

<section class="panel">
  <h3>New import</h3>
  <div class="segmented">
    <button aria-pressed={kind === 'monster'} onclick={() => choose('monster')}>Monster</button>
    <button aria-pressed={kind === 'character'} onclick={() => choose('character')}>Character</button>
  </div>
  <div style="margin-top: 10px">
    <ModelDrop idle="Drop the rigged .glb or .fbx here to begin" />
  </div>
  <p class="hint">
    {#if kind === 'monster'}
      A monster ends as a row in <code>data-src/monsters.csv</code> and can spawn straight away.
    {:else}
      A character gets as far as the file and the checklist — the game wires those up in TypeScript.
    {/if}
  </p>
  {#if session.loaded}
    <div class="btn-row" style="margin-top: 10px">
      <button class="btn btn-primary" onclick={() => (session.step = 'source')}>
        Continue to Source →
      </button>
    </div>
  {/if}
</section>

{#if session.drafts.length > 0}
  <section class="panel">
    <h3>Unfinished drafts</h3>
    <div class="list">
      {#each session.drafts as draft (draft.id)}
        <div class="draft">
          <button class="card" onclick={() => session.openDraft(draft.id)}>
            <div class="card-title">{draft.displayName || draft.id}</div>
            <div class="card-sub">
              {draft.kind} · stopped at {draft.step} · {new Date(draft.updatedAt).toLocaleString()}
            </div>
          </button>
          <button class="btn btn-danger" onclick={() => discard(draft.id)} title="Delete this draft">✕</button>
        </div>
      {/each}
    </div>
  </section>
{/if}

<section class="panel">
  <h3>Rework something shipped</h3>
  <p class="hint">
    Loads the GLB the game is using now, with its CSV row filled in. Height, weapon offset and texture
    quality are the settings that get revisited most.
  </p>
  <div class="field">
    <label for="rework-monster">Monster</label>
    <select
      id="rework-monster"
      onchange={(event) => {
        const file = event.currentTarget.value
        if (file) session.openExisting('monster', file)
        event.currentTarget.value = ''
      }}
    >
      <option value="">Pick a model…</option>
      {#each session.repo?.monsterModels ?? [] as file (file)}
        <option value={file}>{file}</option>
      {/each}
    </select>
  </div>
  <div class="field">
    <label for="rework-character">Character</label>
    <select
      id="rework-character"
      onchange={(event) => {
        const file = event.currentTarget.value
        if (file) session.openExisting('character', file)
        event.currentTarget.value = ''
      }}
    >
      <option value="">Pick a model…</option>
      {#each session.repo?.characterModels ?? [] as file (file)}
        <option value={file}>{file}</option>
      {/each}
    </select>
  </div>
</section>

<style>
  .draft {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 6px;
  }
</style>
