<script lang="ts">
  import { STEPS, type StepId } from '../session.svelte'

  type Status = 'todo' | 'done' | 'warn' | 'blocked'

  interface Props {
    current: StepId
    status: Record<string, Status>
    enabled: (id: StepId) => boolean
    onSelect: (id: StepId) => void
  }

  let { current, status, enabled, onSelect }: Props = $props()

  const MARK: Record<Status, string> = { todo: '○', done: '●', warn: '▲', blocked: '■' }
</script>

<nav class="rail">
  {#each STEPS as step, i (step.id)}
    {@const state = status[step.id] ?? 'todo'}
    <button
      class="step"
      class:active={current === step.id}
      data-status={state}
      disabled={!enabled(step.id)}
      onclick={() => onSelect(step.id)}
      title={enabled(step.id) ? step.hint : 'Load a model on the Start step first'}
    >
      <span class="index num">{String(i).padStart(2, '0')}</span>
      <span class="label">{step.label}</span>
      <span class="mark">{MARK[state]}</span>
    </button>
  {/each}
</nav>

<style>
  .step {
    display: grid;
    grid-template-columns: 22px 1fr 14px;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 7px 14px;
    color: var(--muted);
    border-left: 2px solid transparent;
    text-align: left;
  }

  .step:hover:not(:disabled) {
    background: var(--raised);
    color: var(--text);
  }

  .step:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  .step.active {
    background: var(--raised);
    border-left-color: var(--accent);
    color: var(--text);
  }

  .index {
    font-size: 10px;
    color: var(--dim);
  }

  .label {
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mark {
    font-size: 10px;
    text-align: center;
  }

  .step[data-status='done'] .mark {
    color: var(--green);
  }
  .step[data-status='warn'] .mark {
    color: var(--yellow);
  }
  .step[data-status='blocked'] .mark {
    color: var(--red);
  }
  .step[data-status='todo'] .mark {
    color: var(--dim);
  }
</style>
