<script lang="ts">
  import { session } from '../../session.svelte'
  import { BUDGET } from '../../validate'

  let reds = $derived(session.findings.filter((finding) => finding.severity === 'red'))
  let yellows = $derived(session.findings.filter((finding) => finding.severity === 'yellow'))
</script>

<section class="panel">
  <h3>Result</h3>
  {#if session.findings.length === 0}
    <p class="hint"><span class="badge badge-green">clear</span> Nothing to answer for. This one is ready to write.</p>
  {:else}
    <dl class="stats">
      <dt>Blocking</dt>
      <dd class:bad={reds.length > 0}>{reds.length}</dd>
      <dt>Needs a decision</dt>
      <dd>{yellows.filter((f) => !session.acknowledged.includes(f.code)).length} of {yellows.length}</dd>
    </dl>
  {/if}
</section>

{#if reds.length > 0}
  <section class="panel">
    <h3>Blocking</h3>
    <div class="list">
      {#each reds as finding (finding.id)}
        <div class="finding red">
          <div class="finding-head">
            <span class="badge badge-red">block</span>
            <strong>{finding.title}</strong>
          </div>
          <p class="hint">{finding.detail}</p>
        </div>
      {/each}
    </div>
  </section>
{/if}

{#if yellows.length > 0}
  <section class="panel">
    <h3>Your call</h3>
    <div class="list">
      {#each yellows as finding (finding.id)}
        <div class="finding yellow" class:accepted={session.acknowledged.includes(finding.code)}>
          <div class="finding-head">
            <span class="badge badge-yellow">check</span>
            <strong>{finding.title}</strong>
          </div>
          <p class="hint">{finding.detail}</p>
          <label class="field-inline">
            <input
              type="checkbox"
              checked={session.acknowledged.includes(finding.code)}
              onchange={(e) => session.acknowledge(finding.code, e.currentTarget.checked)}
            />
            I know, go ahead
          </label>
        </div>
      {/each}
    </div>
  </section>
{/if}

<section class="panel">
  <h3>Budgets</h3>
  <dl class="stats">
    <dt>Triangles</dt>
    <dd>{(session.result?.stats.triangles ?? 0).toLocaleString()} / {BUDGET.triangles.toLocaleString()}</dd>
    <dt>Materials</dt>
    <dd>{session.result?.stats.materials ?? 0} / {BUDGET.materials}</dd>
    <dt>Textures</dt>
    <dd>{session.result?.stats.images.length ?? 0} / {BUDGET.images}</dd>
    <dt>File size</dt>
    <dd>
      {((session.result?.stats.byteLength ?? 0) / 1_000_000).toFixed(2)} /
      {(BUDGET.byteLength / 1_000_000).toFixed(1)} MB
    </dd>
  </dl>
  <p class="hint">
    Measured off what the repo already ships: every character and monster sits at or under 10k triangles
    with one material and at most three 1024² textures. Past that is not forbidden, just deliberate.
  </p>
</section>

<style>
  .finding {
    border: 1px solid var(--line);
    border-left-width: 3px;
    border-radius: var(--radius);
    background: var(--raised);
    padding: 9px 11px;
  }

  .finding.red {
    border-left-color: var(--red);
  }

  .finding.yellow {
    border-left-color: var(--yellow);
  }

  .finding.accepted {
    opacity: 0.55;
  }

  .finding-head {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .bad {
    color: var(--red);
  }
</style>
