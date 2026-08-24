<script lang="ts">
  /**
   * Everything off the sheet, named and why.
   *
   * A rig the packs cannot drive lands here on its own — the grid is a claim
   * "every cell here is worth looking at," and an empty box that never plays
   * anything does not belong in it. This is also where a rig you hid by hand
   * ends up, so there is exactly one place to look for "where did that go."
   */
  import { isIncompatible, session } from '../session.svelte'
  import type { Rig } from '../types'

  let { rigs, onshow }: { rigs: Rig[]; onshow: (id: string) => void } = $props()

  type Reason = { text: string; tone: 'bad' | 'near' | 'dim' }

  function reasonFor(rig: Rig): Reason {
    const status = session.status[rig.id]
    if (!isIncompatible(rig.id) || !status) return { text: 'hidden', tone: 'dim' }
    if (status.nearMisses.length === status.missing.length) {
      return { text: `rename ${status.nearMisses[0].have} → ${status.nearMisses[0].want}`, tone: 'near' }
    }
    return { text: `missing ${status.missing.length} bone${status.missing.length === 1 ? '' : 's'}`, tone: 'bad' }
  }
</script>

{#if rigs.length > 0}
  <div class="strip">
    <span class="eyebrow">Off the sheet</span>
    <div class="chips">
      {#each rigs as rig (rig.id)}
        {@const reason = reasonFor(rig)}
        <button class="chip" onclick={() => onshow(rig.id)} title={`Show ${rig.name}`}>
          <span class="plus" aria-hidden="true">+</span>
          <span class="name">{rig.name}</span>
          <span class="reason {reason.tone}">{reason.text}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .strip {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 16px;
    border-top: 1px solid var(--line);
    background: var(--panel);
    flex: 0 0 auto;
    overflow: hidden;
  }

  .chips {
    display: flex;
    gap: 6px;
    overflow-x: auto;
  }

  .chip {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 0 0 auto;
    padding: 3px 8px 3px 6px;
    background: var(--ink);
    border: 1px solid var(--line);
    color: var(--dim);
    font-size: 10.5px;
    cursor: pointer;
    white-space: nowrap;
    transition: border-color 120ms, color 120ms;
  }

  .chip:hover {
    border-color: var(--faint);
    color: var(--chalk);
  }

  .plus {
    color: var(--faint);
  }

  .name {
    color: var(--chalk);
  }

  .reason {
    font-size: 9.5px;
  }

  .reason.dim {
    color: var(--faint);
  }

  .reason.near {
    color: var(--signal);
  }

  .reason.bad {
    color: var(--halt);
  }
</style>
