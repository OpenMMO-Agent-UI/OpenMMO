<script lang="ts">
  /**
   * One frame of the contact sheet.
   *
   * The box is empty on purpose — the shared WebGL canvas draws into this
   * rectangle from behind. Everything here is the slate around it.
   */
  import { onMount } from 'svelte'
  import { session } from '../session.svelte'
  import type { Rig } from '../types'

  let {
    rig,
    onmount,
    ontoggle,
  }: {
    rig: Rig
    onmount: (id: string, element: HTMLElement) => void
    ontoggle: (id: string) => void
  } = $props()

  let stage = $state<HTMLElement | null>(null)
  let status = $derived(session.status[rig.id])
  let busy = $derived(session.working.has(rig.id))

  onMount(() => {
    if (stage) onmount(rig.id, stage)
  })
</script>

<figure class="cell" class:off={!session.shown[rig.id]}>
  <div class="stage" bind:this={stage}>
    {#if busy}<span class="working" aria-label="Retargeting"></span>{/if}
  </div>

  <figcaption>
    <button class="pick" onclick={() => ontoggle(rig.id)} title="Hide this rig" aria-label={`Hide ${rig.name}`}>
      <span class="name">{rig.name}</span>
    </button>

    <div class="facts num">
      {#if status?.error}
        <span class="bad">{status.error}</span>
      {:else if !status?.loaded}
        <span class="dim">loading</span>
      {:else if status.missing.length > 0}
        {#if status.nearMisses.length === status.missing.length}
          <!-- Every missing bone is there under another name: a rename, not a re-rig. -->
          <span
            class="near"
            title={`The game matches bone names exactly. Rename ${status.nearMisses
              .map((miss) => `${miss.have} to ${miss.want}`)
              .join(', ')}.`}
          >
            rename {status.nearMisses[0].have} &rarr; {status.nearMisses[0].want}{status.nearMisses.length > 1
              ? ` +${status.nearMisses.length - 1}`
              : ''}
          </span>
        {:else}
          <span class="bad" title={`Missing: ${status.missing.join(', ')}`}>
            no {status.missing[0]}{status.missing.length > 1 ? ` +${status.missing.length - 1}` : ''}
          </span>
        {/if}
      {:else}
        <span class="dim">{status.boneCount} bones · hips {status.hipsHeight.toFixed(2)}m</span>
      {/if}
    </div>
  </figcaption>
</figure>

<style>
  /* No background. The shared WebGL canvas is behind the whole page, and this
     frame is a window onto it — painting the cell would hide the render. */
  .cell {
    display: flex;
    flex-direction: column;
    margin: 0;
    min-height: 0;
    position: relative;
    background: transparent;
    border-right: 1px solid var(--line-soft);
    border-bottom: 1px solid var(--line-soft);
  }

  /* A hidden rig leaves the grid entirely — it moves to the strip below rather
     than sitting here empty. The component stays mounted regardless, so the
     contact sheet keeps its registration and its mixer's position. */
  .cell.off {
    display: none;
  }

  .stage {
    position: relative;
    flex: 1 1 auto;
    min-height: 0;
  }

  figcaption {
    padding: 5px 8px 6px;
    border-top: 1px solid var(--line-soft);
    background: var(--panel);
  }

  .pick {
    display: flex;
    align-items: center;
    min-width: 0;
    padding: 0;
    background: none;
    border: 0;
    cursor: pointer;
    text-align: left;
  }

  .pick:hover .name {
    color: var(--halt);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
    color: var(--chalk);
  }

  .facts {
    font-size: 10px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .dim {
    color: var(--faint);
  }

  .bad {
    color: var(--halt);
  }

  /* Off, like any incompatible rig — but one rename from working, which is a
     different job from a re-rig and should not read the same. */
  .near {
    color: var(--signal);
  }



  .working {
    position: absolute;
    inset: auto 0 0 0;
    height: 1px;
    background: linear-gradient(90deg, transparent, var(--signal), transparent);
    animation: sweep 900ms linear infinite;
  }

  @keyframes sweep {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(100%);
    }
  }
</style>
