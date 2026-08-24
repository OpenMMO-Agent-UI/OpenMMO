<script lang="ts">
  import { onMount } from 'svelte'
  import { Viewport } from '../viewer/viewport'
  import { LIGHTING_PRESETS } from '../viewer/lighting'

  interface Props {
    onReady: (viewport: Viewport) => void
    lighting: string
    onLighting: (id: string) => void
    overlay?: string
  }

  let { onReady, lighting, onLighting, overlay }: Props = $props()
  let host: HTMLDivElement
  let viewport: Viewport | null = null

  onMount(() => {
    viewport = new Viewport(host)
    onReady(viewport)
    return () => viewport?.dispose()
  })

  $effect(() => {
    viewport?.setLighting(lighting)
  })
</script>

<div class="view">
  <div class="canvas-host" bind:this={host}></div>

  <div class="lighting">
    <button class="btn frame" onclick={() => viewport?.frameCamera()} title="Fit the model on screen">
      Frame
    </button>
    <div class="segmented">
      {#each LIGHTING_PRESETS as preset (preset.id)}
        <button
          aria-pressed={lighting === preset.id}
          onclick={() => onLighting(preset.id)}
          title={preset.hint}
        >
          {preset.label}
        </button>
      {/each}
    </div>
  </div>

  {#if overlay}
    <p class="overlay">{overlay}</p>
  {/if}
</div>

<style>
  .canvas-host {
    position: absolute;
    inset: 0;
  }

  .canvas-host :global(canvas) {
    display: block;
    width: 100%;
    height: 100%;
  }

  .lighting {
    position: absolute;
    top: 12px;
    left: 12px;
    display: flex;
    gap: 8px;
    background: rgba(10, 12, 17, 0.82);
    border-radius: var(--radius);
    backdrop-filter: blur(6px);
  }

  .frame {
    padding: 5px 10px;
    font-size: 12px;
  }

  .overlay {
    position: absolute;
    inset: auto 12px 12px 12px;
    margin: 0;
    padding: 8px 11px;
    border-radius: var(--radius);
    background: rgba(10, 12, 17, 0.86);
    color: var(--muted);
    font-size: 12px;
    backdrop-filter: blur(6px);
  }
</style>
