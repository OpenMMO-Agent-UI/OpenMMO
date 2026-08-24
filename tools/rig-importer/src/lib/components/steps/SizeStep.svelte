<script lang="ts">
  import { session } from '../../session.svelte'
  import { walkSpeedFor, runSpeedFor } from '../../game/rig'

  // Heights the shipped monsters were authored to, from doc/assets/monsters.md.
  const REFERENCE_HEIGHTS = [
    { label: 'Kobold', height: 1.09 },
    { label: 'Human', height: 1.8 },
    { label: 'Hobgoblin', height: 1.9 },
    { label: 'Gnoll', height: 2.15 },
    { label: 'Bugbear', height: 2.2 },
    { label: 'Ogre', height: 2.4 },
    { label: 'Troll', height: 2.7 },
  ]

  function setHeight(value: number) {
    session.settings.targetHeight = value
    session.scheduleRecompute(() => session.applyDerivedValues())
  }
</script>

<section class="panel">
  <h3>Target height</h3>
  <div class="field-inline">
    <input
      type="range"
      min="0.3"
      max="4"
      step="0.01"
      value={session.settings.targetHeight ?? session.sourceHeight}
      oninput={(e) => setHeight(Number(e.currentTarget.value))}
    />
    <input
      class="num height"
      type="number"
      min="0.1"
      max="8"
      step="0.05"
      value={session.settings.targetHeight ?? session.sourceHeight}
      oninput={(e) => setHeight(Number(e.currentTarget.value))}
    />
    <span class="unit">m</span>
  </div>
  <div class="btn-row" style="margin-top: 8px">
    {#each REFERENCE_HEIGHTS as reference (reference.label)}
      <button class="btn tiny" onclick={() => setHeight(reference.height)}>
        {reference.label} <span class="num">{reference.height}</span>
      </button>
    {/each}
  </div>
  <p class="hint">
    Scale is baked into the vertex, joint and inverse-bind data — nothing is left on a node — so the
    file reads at this height without a runtime multiplier, the way the shipped monsters do.
    {#if session.result && Math.abs(session.result.scaleFactor - 1) > 0.001}
      Imported at {session.sourceHeight.toFixed(3)} m, scaled ×{session.result.scaleFactor.toFixed(4)}.
    {/if}
  </p>
</section>

<section class="panel">
  <h3>Origin</h3>
  <label class="field-inline">
    <input
      type="checkbox"
      bind:checked={session.settings.recentre}
      onchange={() => session.recompute()}
    />
    Put the origin at the floor centre
  </label>
  <p class="hint">
    Lowest vertex to y=0, bounding box centred on x/z. Measured, all five shipped Meshy monsters sit
    exactly there.
    {#if session.result}
      Moved by
      <span class="num"
        >{session.result.originShift.map((axis) => axis.toFixed(3)).join(', ')}</span
      >.
    {/if}
  </p>
</section>

<section class="panel">
  <h3>Comparison</h3>
  <label class="field-inline">
    <input type="checkbox" bind:checked={session.showReferences} />
    Show shipped models beside it
  </label>
  <p class="hint">A knight and an ogre, flattened to silhouettes. Numbers lie about scale; these do not.</p>
</section>

<section class="panel">
  <h3>What this decides</h3>
  <dl class="stats">
    <dt>Height</dt>
    <dd>{(session.result?.stats.height ?? 0).toFixed(3)} m</dd>
    <dt>Hips above ground</dt>
    <dd>{session.hipsHeight.toFixed(3)} m</dd>
    <dt>walkSpeed</dt>
    <dd>{walkSpeedFor(session.hipsHeight)}</dd>
    <dt>runSpeed</dt>
    <dd>{runSpeedFor(session.hipsHeight)}</dd>
  </dl>
  <p class="hint">
    Retargeting moves rotations only, so stride comes from how high the hips sit. The pack rig has
    them at 1.165 m walking 1.8 m/s; these are that, scaled. Getting it wrong makes the monster skate.
  </p>
</section>

<style>
  .height {
    width: 84px;
    text-align: right;
  }

  .unit {
    color: var(--muted);
    font-size: 11px;
  }

  .tiny {
    padding: 3px 8px;
    font-size: 11px;
  }
</style>
