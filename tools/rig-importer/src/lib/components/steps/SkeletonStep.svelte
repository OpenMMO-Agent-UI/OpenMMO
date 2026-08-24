<script lang="ts">
  import { session } from '../../session.svelte'
  import { CORE_BONES, CRITICAL_BONES, STANDARD_BONES } from '../../bones/skeleton'
  import { guessBoneMapping } from '../../bones/match'

  let showAll = $state(false)

  const IMPORTANT = new Set<string>([...CORE_BONES, ...CRITICAL_BONES])

  let rows = $derived(
    session.settings.boneMapping.filter(
      (guess) => showAll || IMPORTANT.has(guess.standard) || guess.node !== null
    )
  )

  let takenNodes = $derived(
    new Set(session.settings.boneMapping.map((guess) => guess.node).filter((node) => node !== null))
  )

  function assign(standard: string, value: string) {
    const node = value === '' ? null : Number(value)
    session.settings.boneMapping = session.settings.boneMapping.map((guess) => {
      if (guess.standard === standard) return { ...guess, node, how: 'similar' as const, confidence: node === null ? 0 : 1 }
      // One source joint cannot play two bones.
      if (node !== null && guess.node === node) return { ...guess, node: null, how: 'none' as const, confidence: 0 }
      return guess
    })
    session.recompute()
  }

  function reguess() {
    session.settings.boneMapping = guessBoneMapping(session.joints)
    session.recompute()
  }
</script>

<section class="panel">
  <h3>Mapping</h3>
  <dl class="stats">
    <dt>Joints in the rig</dt>
    <dd>{session.joints.length}</dd>
    <dt>Mapped to standard bones</dt>
    <dd>{session.mappedBones.length} / {STANDARD_BONES.length}</dd>
    <dt>Core bones missing</dt>
    <dd class:warn={session.missingCore.length > 0}>{session.missingCore.length}</dd>
  </dl>
  <p class="hint">
    The game speaks Mixamo bone names with the <code>mixamorig:</code> prefix stripped. Anything sharing
    animations or holding a weapon has to land on them. The guess uses names, side tokens and the rig's own
    shape — check the ones marked below.
  </p>
  <div class="btn-row" style="margin-top: 10px">
    <button class="btn" onclick={reguess}>Guess again</button>
    <label class="field-inline">
      <input type="checkbox" bind:checked={showAll} />
      Show all {STANDARD_BONES.length} bones
    </label>
  </div>
</section>

<section class="panel">
  <h3>Standard bone ← source joint</h3>
  <div class="rows">
    {#each rows as guess (guess.standard)}
      <div class="row" data-critical={CRITICAL_BONES.includes(guess.standard) || undefined}>
        <span class="bone" class:missing={guess.node === null}>{guess.standard}</span>
        <select value={guess.node === null ? '' : String(guess.node)} onchange={(e) => assign(guess.standard, e.currentTarget.value)}>
          <option value="">— none —</option>
          {#each session.joints as joint (joint.node)}
            <option value={String(joint.node)} disabled={takenNodes.has(joint.node) && joint.node !== guess.node}>
              {joint.name}
            </option>
          {/each}
        </select>
        <span class="how" data-how={guess.how}>
          {guess.how === 'exact' ? 'name' : guess.how === 'alias' ? 'alias' : guess.how === 'similar' ? '~' : '—'}
        </span>
      </div>
    {/each}
  </div>
</section>

<style>
  .rows {
    display: grid;
    gap: 3px;
  }

  .row {
    display: grid;
    grid-template-columns: 118px 1fr 34px;
    align-items: center;
    gap: 6px;
  }

  .row[data-critical] .bone {
    color: var(--text);
    font-weight: 600;
  }

  .bone {
    font-size: 11.5px;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .bone.missing {
    color: var(--dim);
  }

  .row select {
    padding: 3px 6px;
    font-size: 11.5px;
  }

  .how {
    font-size: 10px;
    text-align: center;
    color: var(--dim);
  }

  .how[data-how='exact'] {
    color: var(--green);
  }
  .how[data-how='alias'] {
    color: var(--blue);
  }
  .how[data-how='similar'] {
    color: var(--yellow);
  }

  .warn {
    color: var(--yellow);
  }
</style>
