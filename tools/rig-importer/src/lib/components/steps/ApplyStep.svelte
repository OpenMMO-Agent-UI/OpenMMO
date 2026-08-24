<script lang="ts">
  import { session } from '../../session.svelte'
  import { applyDraft, planApply } from '../../api'
  import type { ApplyPlan, ApplyResult } from '../../plan'

  let plan = $state<ApplyPlan | null>(null)
  let outcome = $state<ApplyResult | null>(null)
  let runGenerators = $state(true)
  let working = $state('')

  let blocked = $derived(session.unresolved.length > 0)

  async function preview() {
    working = 'Planning'
    try {
      // Stop on a failed save: planning would then fail for a second, unrelated
      // reason and replace the message that actually explains it.
      if (!(await session.save())) return
      plan = (await planApply(session.draftId)).plan
      outcome = null
    } catch (error) {
      session.error = error instanceof Error ? error.message : String(error)
    } finally {
      working = ''
    }
  }

  async function write() {
    working = 'Writing'
    try {
      if (!(await session.save())) return
      outcome = await applyDraft(session.draftId, runGenerators)
      plan = outcome.plan
    } catch (error) {
      session.error = error instanceof Error ? error.message : String(error)
    } finally {
      working = ''
    }
  }
</script>

<section class="panel">
  <h3>Before writing</h3>
  {#if blocked}
    <p class="hint">
      <span class="badge badge-red">held</span>
      {session.unresolved.length} finding{session.unresolved.length === 1 ? '' : 's'} on the Validate step still
      needs answering.
    </p>
  {:else}
    <p class="hint"><span class="badge badge-green">ready</span> Nothing outstanding.</p>
  {/if}
  <div class="btn-row" style="margin-top: 10px">
    <button class="btn" onclick={preview} disabled={!!working || !session.result}>
      {working === 'Planning' ? 'Planning…' : 'Preview the diff'}
    </button>
  </div>
</section>

{#if plan}
  <section class="panel">
    <h3>Files</h3>
    <div class="list">
      {#each plan.changes as change (change.path)}
        <div class="change">
          <span class="badge {change.action === 'create' ? 'badge-green' : 'badge-yellow'}">{change.action}</span>
          <code>{change.path}</code>
          {#if change.bytes}<span class="num dim">{(change.bytes / 1024).toFixed(0)} KB</span>{/if}
        </div>
        {#if change.preview}
          <pre class="mono-block">{change.preview}</pre>
        {/if}
      {/each}
    </div>
  </section>

  {#if plan.csvChanges.length > 0}
    <section class="panel">
      <h3>monsters.csv columns</h3>
      <div class="list">
        {#each plan.csvChanges as change (change.column)}
          <div class="diff">
            <span class="col">{change.column}</span>
            <span class="from num">{change.from || '—'}</span>
            <span class="arrow">→</span>
            <span class="to num">{change.to || '—'}</span>
          </div>
        {/each}
      </div>
    </section>
  {/if}

  <section class="panel">
    <h3>Then</h3>
    <label class="field-inline">
      <input type="checkbox" bind:checked={runGenerators} />
      Run the repo's generators afterwards
    </label>
    <div class="list" style="margin-top: 6px">
      {#each plan.commands as command (command)}
        <code class="cmd">{command}</code>
      {/each}
    </div>
    {#each plan.reminders as reminder (reminder)}
      <p class="hint">{reminder}</p>
    {/each}
  </section>

  <section class="panel">
    <div class="btn-row">
      <button class="btn btn-primary" onclick={write} disabled={blocked || !!working}>
        {working === 'Writing' ? 'Writing…' : 'Write it'}
      </button>
    </div>
  </section>
{/if}

{#if outcome}
  <section class="panel">
    <h3>Written</h3>
    <div class="list">
      {#each outcome.written as file (file)}
        <code class="cmd">{file}</code>
      {/each}
    </div>
    {#each outcome.commandResults as result (result.command)}
      <p class="hint">
        <span class="badge {result.ok ? 'badge-green' : 'badge-red'}">{result.ok ? 'ok' : 'failed'}</span>
        <code>{result.command}</code>
      </p>
      {#if result.output}
        <pre class="mono-block">{result.output}</pre>
      {/if}
    {/each}
  </section>

  <section class="panel">
    <h3>git status</h3>
    <pre class="mono-block">{outcome.gitStatus || 'clean'}</pre>
    <p class="hint">Nothing is committed. Read the diff, then commit it yourself.</p>
  </section>
{/if}

<style>
  .change {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .change code {
    word-break: break-all;
  }

  .diff {
    display: grid;
    grid-template-columns: 132px 1fr 14px 1fr;
    gap: 6px;
    align-items: baseline;
    font-size: 11.5px;
  }

  .col {
    color: var(--muted);
  }

  .from {
    color: var(--dim);
    text-decoration: line-through;
  }

  .arrow {
    color: var(--dim);
  }

  .to {
    color: var(--green);
  }

  .cmd {
    display: block;
    font-family: var(--mono);
    font-size: 11.5px;
    color: var(--muted);
  }

  .dim {
    color: var(--dim);
  }
</style>
