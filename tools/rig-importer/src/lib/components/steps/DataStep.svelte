<script lang="ts">
  import { session } from '../../session.svelte'
  import { BEHAVIORS, MATERIALS, MODEL_DERIVED_COLUMNS } from '../../game/defaults'
  import { ANIM_COLUMNS } from '../../game/clips'

  const DERIVED = new Set<string>(MODEL_DERIVED_COLUMNS)
  const ANIM = new Set<string>(ANIM_COLUMNS)
  const CHOICES: Record<string, readonly string[]> = {
    behavior: BEHAVIORS,
    material: MATERIALS,
    dungeonAggressive: ['true', 'false', ''],
    deathPlaysHit: ['true', 'false', ''],
    boss: ['', 'true'],
    sharedAnims: ['true', ''],
  }

  let columns = $derived((session.repo?.monsterColumns ?? []).filter((column) => column !== 'id'))

  function set(column: string, value: string) {
    session.csvValues = { ...session.csvValues, [column]: value }
  }
</script>

{#if session.kind === 'character'}
  <section class="panel">
    <h3>Characters are wired up in code</h3>
    <p class="hint">
      There is no CSV row for a character. Applying writes the GLB and the provenance entry; the rest is a
      short list of edits in the client and the server, which the review step spells out with the exact
      symbols to add.
    </p>
  </section>
{:else}
  <section class="panel">
    <h3>Derived from the model</h3>
    <dl class="stats">
      {#each MODEL_DERIVED_COLUMNS as column (column)}
        <dt>{column}</dt>
        <dd>{session.csvValues[column] || '—'}</dd>
      {/each}
    </dl>
    <div class="btn-row" style="margin-top: 8px">
      <button class="btn" onclick={() => session.applyDerivedValues()}>Recalculate</button>
    </div>
    <p class="hint">These follow from the height, the hips and the hand. Edit them below if you disagree.</p>
  </section>

  <section class="panel">
    <h3>monsters.csv row</h3>
    <p class="hint">
      Combat numbers start from the shallow end of the existing table so the row spawns and can be fought
      the moment it is written. Balancing them is a separate job.
    </p>
    {#each columns as column (column)}
      <div class="csv-row" data-kind={DERIVED.has(column) ? 'derived' : ANIM.has(column) ? 'anim' : 'plain'}>
        <label for={`csv-${column}`}>{column}</label>
        {#if CHOICES[column]}
          <select id={`csv-${column}`} value={session.csvValues[column] ?? ''} onchange={(e) => set(column, e.currentTarget.value)}>
            {#each CHOICES[column] as choice (choice)}
              <option value={choice}>{choice || '—'}</option>
            {/each}
          </select>
        {:else}
          <input
            id={`csv-${column}`}
            value={session.csvValues[column] ?? ''}
            oninput={(e) => set(column, e.currentTarget.value)}
            spellcheck="false"
          />
        {/if}
      </div>
    {/each}
  </section>
{/if}

<style>
  .csv-row {
    display: grid;
    grid-template-columns: 152px 1fr;
    align-items: center;
    gap: 6px;
    margin-bottom: 3px;
  }

  .csv-row label {
    font-size: 11.5px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .csv-row[data-kind='derived'] label {
    color: var(--accent);
  }

  .csv-row[data-kind='anim'] label {
    color: var(--blue);
  }

  .csv-row input,
  .csv-row select {
    padding: 3px 6px;
    font-size: 11.5px;
  }
</style>
