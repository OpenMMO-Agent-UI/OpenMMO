<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { session, STEPS, type StepId } from '$lib/session.svelte'
  import { stage } from '$lib/viewer/current.svelte'
  import StepRail from '$lib/components/StepRail.svelte'
  import ViewportPane from '$lib/components/ViewportPane.svelte'
  import StartStep from '$lib/components/steps/StartStep.svelte'
  import SourceStep from '$lib/components/steps/SourceStep.svelte'
  import SkeletonStep from '$lib/components/steps/SkeletonStep.svelte'
  import SizeStep from '$lib/components/steps/SizeStep.svelte'
  import MaterialStep from '$lib/components/steps/MaterialStep.svelte'
  import AnimationStep from '$lib/components/steps/AnimationStep.svelte'
  import WeaponStep from '$lib/components/steps/WeaponStep.svelte'
  import DataStep from '$lib/components/steps/DataStep.svelte'
  import ValidateStep from '$lib/components/steps/ValidateStep.svelte'
  import ApplyStep from '$lib/components/steps/ApplyStep.svelte'
  import { isValidId } from '$lib/game/defaults'
  import type { Viewport } from '$lib/viewer/viewport'

  const PANELS = {
    start: StartStep,
    source: SourceStep,
    skeleton: SkeletonStep,
    size: SizeStep,
    material: MaterialStep,
    animation: AnimationStep,
    weapon: WeaponStep,
    data: DataStep,
    validate: ValidateStep,
    apply: ApplyStep,
  } as const

  let Panel = $derived(PANELS[session.step])
  let stepIndex = $derived(STEPS.findIndex((step) => step.id === session.step))

  onMount(() => {
    session.loadRepo().catch((error) => (session.error = String(error)))
  })

  let framedGeneration = -1

  /** The preview always loads the bytes the tool would write. */
  $effect(() => {
    const bytes = session.result?.bytes
    const generation = session.subjectGeneration
    const viewport = stage.viewport
    if (!bytes || !viewport) return

    untrack(() => {
      // Reframe for a new model, but not for a rebuild of the same one — that
      // would fight the camera on every slider tick.
      const reframe = generation !== framedGeneration
      framedGeneration = generation
      viewport.loadSubject(bytes, reframe).catch((error) => (session.error = String(error)))
      stage.sharedClips = []
      stage.playing = ''
    })
  })

  $effect(() => {
    stage.viewport?.setReferencesVisible(session.showReferences && session.step === 'size')
  })

  function ready(viewport: Viewport) {
    stage.viewport = viewport
    viewport.setReferences([
      { url: '/models/characters/knight.glb', x: -1.05 },
      { url: '/models/monsters/ogre.glb', x: 1.05 },
    ])
    viewport.setReferencesVisible(false)
  }

  type Status = 'todo' | 'done' | 'warn' | 'blocked'

  let status = $derived.by((): Record<string, Status> => {
    const loaded = session.loaded
    const reds = session.findings.filter((finding) => finding.severity === 'red').length
    const open = session.unresolved.length
    const mapped = new Set(session.mappedBones)

    return {
      start: loaded ? 'done' : 'todo',
      source: !loaded ? 'todo' : isValidId(session.id) && session.source.generator ? 'done' : 'warn',
      skeleton: !loaded
        ? 'todo'
        : !mapped.has('Hips')
          ? 'blocked'
          : session.missingCore.length > 0
            ? 'warn'
            : 'done',
      size: loaded && session.settings.targetHeight ? 'done' : 'todo',
      material: !loaded ? 'todo' : session.materialsNeedRepair && session.settings.material.metallicFactor > 0.5 ? 'warn' : 'done',
      animation: !loaded ? 'todo' : session.csvValues.animIdle && session.csvValues.animDie ? 'done' : 'warn',
      weapon: !loaded ? 'todo' : 'done',
      data: !loaded ? 'todo' : session.kind === 'character' || isValidId(session.id) ? 'done' : 'warn',
      validate: !loaded ? 'todo' : reds > 0 ? 'blocked' : open > 0 ? 'warn' : 'done',
      apply: 'todo',
    }
  })

  function enabled(id: StepId): boolean {
    return id === 'start' || session.loaded
  }

  function step(delta: number) {
    const next = STEPS[stepIndex + delta]
    if (next && enabled(next.id)) session.step = next.id
  }

  let overlay = $derived.by(() => {
    if (!session.result) return 'Drop a rigged .glb or .fbx on the Source step.'
    const stats = session.result.stats
    return [
      `${stats.height.toFixed(2)} m`,
      `${stats.triangles.toLocaleString()} tri`,
      `${stats.joints} joints`,
      `${stats.images.length} tex`,
      `${(stats.byteLength / 1_000_000).toFixed(2)} MB`,
      stage.playing && `▶ ${stage.playing}`,
    ]
      .filter(Boolean)
      .join('   ·   ')
  })
</script>

<div class="shell">
  <header class="topbar">
    <strong>rig-importer</strong>
    <span class="sep">/</span>
    <span class="subject">
      {#if session.loaded}
        {session.displayName || session.id || 'untitled'}
        <span class="badge badge-dim">{session.kind}</span>
      {:else}
        no model loaded
      {/if}
    </span>

    <span class="spacer"></span>

    {#if session.busy}
      <span class="muted">{session.busy}…</span>
    {:else if session.savedAt}
      <span class="muted">saved {session.savedAt}</span>
    {/if}
    <button class="btn" onclick={() => session.save()} disabled={!session.result}>Save draft</button>
    <div class="btn-row">
      <button class="btn" onclick={() => step(-1)} disabled={stepIndex <= 0}>←</button>
      <button class="btn" onclick={() => step(1)} disabled={stepIndex >= STEPS.length - 1}>→</button>
    </div>
  </header>

  <StepRail current={session.step} {status} {enabled} onSelect={(id) => (session.step = id)} />

  <div class="panel-column">
    <div class="panel-head">
      <h2>{STEPS[stepIndex].label}</h2>
      <p>{STEPS[stepIndex].hint}</p>
    </div>
    {#if session.error}
      <div class="panel">
        <p class="error">{session.error}</p>
      </div>
    {/if}

    {#each session.result?.warnings ?? [] as warning, i (i)}
      <div class="panel">
        <p class="hint"><span class="badge badge-yellow">note</span> {warning}</p>
      </div>
    {/each}

    {#if session.step !== 'start' && !session.loaded}
      <div class="panel">
        <p class="hint">
          No model loaded yet. Go back to <button class="link" onclick={() => (session.step = 'start')}>Start</button>
          and pick one — every step past it needs something to work on.
        </p>
      </div>
    {:else}
      <svelte:boundary>
        <Panel />
        {#snippet failed(error, reset)}
          <div class="panel">
            <!-- Not named: by the time this renders the active step may already
                 have changed, and the stack says which component it was. -->
            <p class="error">A step failed to render.</p>
            <pre class="mono-block">{error instanceof Error
                ? `${error.message}\n\n${error.stack ?? ''}`
                : String(error)}</pre>
            <div class="btn-row" style="margin-top: 10px">
              <button class="btn" onclick={reset}>Try again</button>
            </div>
          </div>
        {/snippet}
      </svelte:boundary>
    {/if}
  </div>

  <ViewportPane
    onReady={ready}
    lighting={session.lighting}
    onLighting={(id) => (session.lighting = id)}
    {overlay}
  />
</div>

<style>
  .sep {
    color: var(--dim);
  }

  .subject {
    display: flex;
    align-items: center;
    gap: 7px;
    color: var(--muted);
  }

  .spacer {
    flex: 1;
  }

  .muted {
    color: var(--dim);
    font-size: 12px;
  }

  .panel-head {
    padding: 14px 16px 12px;
    border-bottom: 1px solid var(--line);
    position: sticky;
    top: 0;
    background: var(--panel);
    z-index: 1;
  }

  .panel-head h2 {
    font-size: 15px;
  }

  .panel-head p {
    margin: 3px 0 0;
    color: var(--muted);
    font-size: 12px;
  }

  .link {
    color: var(--accent);
    text-decoration: underline;
    padding: 0;
  }
</style>
