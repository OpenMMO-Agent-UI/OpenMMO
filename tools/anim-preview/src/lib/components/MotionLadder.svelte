<script lang="ts">
  /**
   * Every pack on disk, each a group of rungs, as a standing list rather than
   * a dropdown.
   *
   * A motion name is not unique across groups — `walk` exists in every pack
   * that has one — and `locomotion` and `locomotion2` are meant to be judged
   * independently, so both "current" and "overridden" are keyed on the
   * (pack, motion) pair, never the name alone. Deciding `locomotion`'s `walk`
   * must not light up — or replace — `locomotion2`'s `walk`.
   *
   * Every motion always plays something — its own pack's clip, until a take
   * overrides it — so there is no "undecided" state to track here, only
   * "replaced or not". The amber marker fills in as you replace them. There
   * is no numbering, because the order you go through them in carries
   * nothing.
   */
  import { overrideKey, session, takesFor } from '../session.svelte'
  import type { Motion } from '../types'

  /**
   * Group headers read as prose, so `combat_melee` becomes "combat melee" —
   * but that rule has to name the one shipped pack it was written for, not
   * blanket-replace every underscore. Applied generally, it silently eats
   * the underscore out of a candidate pack you named `locomotion_new`, and
   * "locomotion new" is a different, wrong file name.
   */
  function displayName(pack: string): string {
    return pack === 'combat_melee' ? 'combat melee' : pack
  }

  let {
    groups,
    onpick,
  }: { groups: { pack: string; motions: Motion[] }[]; onpick: (name: string, pack: string) => void } = $props()

  function takeName(pack: string, motion: string): string {
    const path = session.overrides[overrideKey(pack, motion)]
    if (!path) return ''
    return session.takes.find((take) => take.path === path)?.name ?? path
  }
</script>

<nav class="ladder" aria-label="Motions">
  {#each groups as group (group.pack)}
    <div class="group">
      <div class="group-head">
        <span class="eyebrow">{displayName(group.pack)}</span>
        <span class="eyebrow num">
          {group.motions.filter((m) => session.overrides[overrideKey(group.pack, m.name)]).length}/{group.motions
            .length}
        </span>
      </div>
      {#each group.motions as motion (motion.name)}
        {@const overridden = !!session.overrides[overrideKey(group.pack, motion.name)]}
        {@const count = takesFor(motion.name).length}
        {@const current = session.motion === motion.name && session.pack === group.pack}
        <button
          class="rung"
          class:current
          class:overridden
          aria-current={current ? 'true' : undefined}
          onclick={() => onpick(motion.name, group.pack)}
        >
          <span class="mark" aria-hidden="true"></span>
          <span class="name">{motion.name}</span>
          {#if overridden}
            <span class="take">{takeName(group.pack, motion.name)}</span>
          {:else}
            <span class="count num">{count || '—'}</span>
          {/if}
        </button>
      {/each}
    </div>
  {/each}
</nav>

<style>
  .ladder {
    overflow-y: auto;
    padding-bottom: 24px;
  }

  .group + .group {
    margin-top: 18px;
  }

  .group-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    padding: 0 12px 5px;
    margin-bottom: 4px;
    border-bottom: 1px solid var(--line-soft);
  }

  .rung {
    display: grid;
    grid-template-columns: 10px 1fr auto;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: var(--row);
    padding: 0 12px;
    background: none;
    border: 0;
    text-align: left;
    cursor: pointer;
    color: var(--dim);
    transition: color 120ms, background 120ms;
  }

  .rung:hover {
    background: var(--panel);
    color: var(--chalk);
  }

  /* The amber marker is the only thing that moves down the rail, so it reads as
     position rather than as decoration. */
  .mark {
    width: 6px;
    height: 6px;
    border: 1px solid var(--faint);
  }

  .rung.overridden .mark {
    background: var(--signal);
    border-color: var(--signal);
  }

  .rung.current {
    background: var(--raised);
    color: var(--chalk);
  }

  .rung.current .mark {
    border-color: var(--signal);
  }

  .name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .rung.current .name {
    color: var(--chalk);
  }

  .take {
    max-width: 96px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 10.5px;
    color: var(--signal);
    direction: rtl;
    text-align: right;
  }

  .count {
    font-size: 10.5px;
    color: var(--faint);
  }
</style>
