<script lang="ts">
  import { session } from '../../session.svelte'
  import ModelDrop from '../ModelDrop.svelte'
  import { modelPathFor, sourceAssetName } from '../../game/paths'

  async function takeConcept(files: FileList | null) {
    const file = files?.[0]
    if (!file) return
    session.conceptBytes = new Uint8Array(await file.arrayBuffer())
  }
</script>

<section class="panel">
  <h3>Model file</h3>
  <p class="hint">Dropping another file here replaces the model and re-runs every step against it.</p>
  <ModelDrop idle="Drop a .glb or .fbx, or click to browse" />

  {#each session.importNotes as note, i (i)}
    <p class="hint">{note}</p>
  {/each}
  {#if session.convertedFromFbx}
    <p class="hint">Converted from FBX. Mixamo bone prefixes were stripped so the clips stay bound.</p>
  {/if}
</section>

<section class="panel">
  <h3>Identity</h3>
  <div class="grid2">
    <div class="field">
      <label for="src-id">Id</label>
      <input id="src-id" bind:value={session.id} placeholder="bugbear" spellcheck="false" />
    </div>
    <div class="field">
      <label for="src-name">Display name</label>
      <input id="src-name" bind:value={session.displayName} placeholder="Bugbear" />
    </div>
  </div>
  <p class="hint">
    The id names everything this writes, and the CSV row. Lower case, digits and underscores.
  </p>
  {#if session.id}
    <dl class="stats">
      <dt>Model</dt>
      <dd>client/public/models/{modelPathFor(session.kind, session.id)}</dd>
      {#if session.source.sourceFileName}
        <dt>Source kept as</dt>
        <dd>assets/{sourceAssetName(session.id, session.source.sourceFileName)}</dd>
      {/if}
    </dl>
  {/if}
</section>

<section class="panel">
  <h3>Where it came from</h3>
  <p class="hint">
    CLAUDE.md requires this on every new asset, with the tier and date for anything AI or paid. It goes
    into <code>doc/assets/{session.kind === 'monster' ? 'monsters' : 'characters'}.md</code> when you apply.
  </p>
  <div class="grid2">
    <div class="field">
      <label for="src-gen">Generator</label>
      <input id="src-gen" bind:value={session.source.generator} placeholder="Meshy.ai" />
    </div>
    <div class="field">
      <label for="src-tier">Tier</label>
      <input id="src-tier" bind:value={session.source.tier} placeholder="유료 생성" />
    </div>
    <div class="field">
      <label for="src-date">Generated on</label>
      <input id="src-date" type="date" bind:value={session.source.generatedOn} />
    </div>
    <div class="field">
      <label for="src-model">Source name</label>
      <input id="src-model" bind:value={session.source.sourceName} placeholder="Fanghide Warlord" />
    </div>
    <div class="field">
      <label for="src-rig">Rigged by</label>
      <input id="src-rig" bind:value={session.source.rigger} placeholder="mixamo.com" />
    </div>
    <div class="field">
      <label for="src-concept-src">Concept art from</label>
      <input id="src-concept-src" bind:value={session.source.conceptSource} placeholder="chatgpt.com" />
    </div>
  </div>
  <div class="field">
    <label for="src-license">Licence</label>
    <input id="src-license" bind:value={session.source.license} placeholder="CC-BY 4.0, or blank if generated" />
  </div>
  <div class="field">
    <label for="src-notes">Notes (one per line)</label>
    <textarea id="src-notes" rows="3" bind:value={session.source.notes}></textarea>
  </div>
</section>

<section class="panel">
  <h3>Concept art</h3>
  <label class="dropzone">
    <input type="file" accept="image/png,image/jpeg" hidden onchange={(e) => takeConcept(e.currentTarget.files)} />
    {#if session.conceptBytes}
      Attached — {(session.conceptBytes.byteLength / 1024).toFixed(0)} KB
    {:else}
      Drop the concept image (optional)
    {/if}
  </label>
  <p class="hint">
    Saved as <code
      >doc/images/{session.kind === 'monster' ? 'monsters' : 'characters'}/{session.id || 'id'}-concept.png</code
    > and linked from the doc entry.
  </p>
</section>
