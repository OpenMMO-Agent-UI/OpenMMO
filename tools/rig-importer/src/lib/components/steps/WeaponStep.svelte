<script lang="ts">
  import { untrack } from 'svelte'
  import { session } from '../../session.svelte'
  import { stage } from '../../viewer/current.svelte'
  import { weaponOffsetFor, WEAPON_OFFSET_RATIO } from '../../game/rig'
  import { gripFromCsv, gripRadians, gripToCsv, isRotated, type Grip } from '../../game/grip'
  import { splitClipList } from '../../game/clips'

  let attached = $state(false)
  let note = $state('')

  let grip = $derived(gripFromCsv(session.csvValues))
  let suggestion = $derived(weaponOffsetFor(session.handReach))
  let weapon = $derived(session.repo?.weapons.find((entry) => entry.id === session.weaponId) ?? null)
  let boneScale = $derived(session.result ? (stage.viewport?.boneScale('RightHand') ?? 1) : 1)
  let span = $derived(Math.max(0.4, suggestion * 2.5))

  /**
   * Every rebuild of the model replaces the scene the weapon was parented to,
   * so it has to be hung again — and a monster loaded with a weapon already in
   * its CSV row never went through the picker at all.
   */
  $effect(() => {
    const id = session.weaponId
    void session.result
    void session.subjectGeneration
    if (!stage.viewport) return
    untrack(() => void hang(id))
  })

  async function hang(id: string) {
    const model = session.repo?.weapons.find((entry) => entry.id === id)
    const current = gripFromCsv(session.csvValues)
    attached =
      (await stage.viewport?.setWeapon(model ? `/models/${model.model}` : null, 'RightHand', {
        position: [current.x, current.y, current.z],
        rotation: gripRadians(current),
      })) ?? false
    note = id && !attached ? 'No RightHand bone on this rig — the weapon has nowhere to hang.' : ''
  }

  function equip(id: string) {
    session.weaponId = id
    session.csvValues = {
      ...session.csvValues,
      weapon: id,
      weaponBone: id ? 'RightHand' : '',
      ...gripToCsv(id ? { x: 0, y: suggestion, z: 0, rx: 0, ry: 0, rz: 0 } : { x: 0, y: 0, z: 0, rx: 0, ry: 0, rz: 0 }),
    }
  }

  function set(axis: keyof Grip, value: number) {
    const next = { ...grip, [axis]: value }
    session.csvValues = { ...session.csvValues, ...gripToCsv(next) }
    stage.viewport?.moveWeapon({ position: [next.x, next.y, next.z], rotation: gripRadians(next) })
  }

  function reset() {
    const next: Grip = { x: 0, y: suggestion, z: 0, rx: 0, ry: 0, rz: 0 }
    session.csvValues = { ...session.csvValues, ...gripToCsv(next) }
    stage.viewport?.moveWeapon({ position: [next.x, next.y, next.z], rotation: gripRadians(next) })
  }

  function playAttack() {
    const name = splitClipList(session.csvValues.animAttack ?? '')[0]
    const pool = session.sharedAnims ? stage.sharedClips : (stage.viewport?.subjectClips ?? [])
    stage.viewport?.playClip(pool.find((clip) => clip.name === name) ?? null)
  }

  const AXES = [
    { key: 'y' as const, label: 'Along bone', column: 'weaponOffset', hint: 'wrist → fingers' },
    { key: 'x' as const, label: 'Sideways', column: 'weaponOffsetX', hint: 'across the palm' },
    { key: 'z' as const, label: 'Forward', column: 'weaponOffsetZ', hint: 'front of the fist' },
  ]

  const TURNS = [
    { key: 'rx' as const, label: 'Pitch', column: 'weaponRotation' },
    { key: 'ry' as const, label: 'Yaw', column: '' },
    { key: 'rz' as const, label: 'Roll', column: '' },
  ]
</script>

<section class="panel">
  <h3>Weapon</h3>
  <div class="field">
    <label for="weapon-pick">From items.csv</label>
    <select id="weapon-pick" value={session.weaponId} onchange={(e) => equip(e.currentTarget.value)}>
      <option value="">— none, it fights bare-handed —</option>
      {#each session.repo?.weapons ?? [] as entry (entry.id)}
        <option value={entry.id}>{entry.name}</option>
      {/each}
    </select>
  </div>
  {#if weapon}
    <p class="hint">Loading <code>/models/{weapon.model}</code> onto <code>RightHand</code>.</p>
  {/if}
  {#if note}
    <p class="error">{note}</p>
  {/if}
  {#if session.weaponId && Math.abs(boneScale - 1) > 0.01}
    <p class="error">
      The hand bone has a world scale of {boneScale.toFixed(4)}, so the weapon inherits it and renders
      {boneScale < 1 ? 'far too small' : 'far too large'}. Flatten the rig's node scale before fitting the grip.
    </p>
  {/if}
</section>

{#if session.weaponId}
  <section class="panel">
    <h3>Position</h3>
    {#each AXES as axis (axis.key)}
      <div class="field">
        <label for={`grip-${axis.key}`}>
          {axis.label}
          <span class="dim">{axis.hint}</span>
          {#if axis.column}<code class="col">{axis.column}</code>{/if}
        </label>
        <div class="field-inline">
          <input
            id={`grip-${axis.key}`}
            type="range"
            min={axis.key === 'y' ? 0 : -span / 2}
            max={span}
            step="0.005"
            value={grip[axis.key]}
            oninput={(e) => set(axis.key, Number(e.currentTarget.value))}
          />
          <input
            class="num box"
            type="number"
            step="0.01"
            value={grip[axis.key]}
            oninput={(e) => set(axis.key, Number(e.currentTarget.value))}
          />
        </div>
      </div>
    {/each}
    <p class="hint">
      Metres in the bone's own space. A palm is not on the bone axis, so a grip that reads right from
      the front can still float beside the hand when you orbit round.
    </p>
  </section>

  <section class="panel">
    <h3>Rotation <code class="col">weaponRotation</code></h3>
    {#each TURNS as turn (turn.key)}
      <div class="field">
        <label for={`grip-${turn.key}`}>
          {turn.label} <span class="num dim">{grip[turn.key].toFixed(1)}°</span>
        </label>
        <div class="field-inline">
          <input
            id={`grip-${turn.key}`}
            type="range"
            min="-180"
            max="180"
            step="1"
            value={grip[turn.key]}
            oninput={(e) => set(turn.key, Number(e.currentTarget.value))}
          />
          <input
            class="num box"
            type="number"
            step="5"
            value={grip[turn.key]}
            oninput={(e) => set(turn.key, Number(e.currentTarget.value))}
          />
        </div>
      </div>
    {/each}
    <p class="hint">
      Degrees about the bone's local axes, applied in XYZ order — the same Euler the client builds.
      Written as <code>rx|ry|rz</code>, and left blank when the grip is unrotated.
    </p>
  </section>

  <section class="panel">
    <h3>Check it</h3>
    <div class="btn-row">
      <button class="btn" onclick={playAttack}>Play the attack</button>
      <button class="btn" onclick={reset}>Reset to derived</button>
    </div>
    <dl class="stats" style="margin-top: 10px">
      <dt>Hand reach along the bone</dt>
      <dd>{session.handReach.toFixed(3)} m</dd>
      <dt>Derived along-bone offset (×{WEAPON_OFFSET_RATIO})</dt>
      <dd>{suggestion}</dd>
      <dt>Written to CSV</dt>
      <dd>
        {session.csvValues.weaponOffset || '—'} /
        {session.csvValues.weaponOffsetX || '—'} /
        {session.csvValues.weaponOffsetZ || '—'}
        {#if isRotated(grip)}· {session.csvValues.weaponRotation}{/if}
      </dd>
    </dl>
    <p class="hint">
      The along-bone default is 80% of how far the vertices majority-weighted to <code>RightHand</code>
      reach — the rule the shipped monsters follow, and it reproduces them to the centimetre. The other
      five are by eye: play the swing, orbit round, and look for the weapon staying in the fist.
    </p>
  </section>
{/if}

<style>
  .box {
    width: 78px;
    text-align: right;
  }

  .dim {
    color: var(--dim);
    font-weight: 400;
  }

  .col {
    float: right;
    color: var(--accent);
    font-size: 10.5px;
  }
</style>
