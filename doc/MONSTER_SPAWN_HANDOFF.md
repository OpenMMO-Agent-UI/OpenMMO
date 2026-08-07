# Monster spawn starvation fix — handoff

Context for anyone picking up the ambient monster spawn work.

**Branch:** `fix-ambient-monster-spawn-starvation`, branched from `master`
**Commit:** `d5657651` — everything below is committed; the tree is clean and not pushed.

Branched off `master` rather than `agent-target-addressing` on purpose: that
branch only touches `agent-client/`, this one only `server/` and `data-src/`,
so the two stay independently mergeable.

---

## The bug

Reported as: *"monsters are no longer spawned after a couple hours when using
agent-client."*

Ambient surface monsters had **no distance-based despawn** — ground items have
one (`server/src/game_state/inventory.rs`, `tick_ground_item_despawn`),
monsters did not. `max_per_player` counts every alive monster a player owns
*regardless of distance*, so a roaming client abandons monsters faster than it
kills them. Once 27 accumulate (8+6+5+4+4, the sum of every `maxPerPlayer` in
`data-src/world.json`), that client never receives another
`SpawnMonsterRequest`.

Past ~37 concurrent players the abandoned population also exhausted the old
1,000 global cap, at which point `tick_monster_spawns` breaks out of its loop
and **every** player starves, humans included.

The discriminating evidence: a **stationary** bot kept spawning indefinitely, a
**roaming** bot went to zero. Roaming was the single load-bearing element.

## The fix

See the commit for detail; in short:

- `tick_abandoned_monsters` (`server/src/game_state/monster.rs`, registered in
  `main.rs` on a 10s tick) despawns surface monsters with no player inside
  their AOI for 60s. Only the owner is notified — nobody else can see them —
  which also clears that client's monster AI entry. Dungeon monsters are left
  to their floor lifecycle.
- `MonsterRegistry` (same file) wraps the monster map and maintains alive
  counts globally and per `(owner, monster_type)` incrementally, so the spawn
  caps stop scanning the map.
- `maxMonstersTotal` raised 1,000 → 135,000 (5,000 users × 27).

No `agent-client` change was needed: it already handles `MonsterRemoved`
(`agent-client/src/state.rs`, the `ServerMessage::MonsterRemoved` arm), so the
server-side despawn also clears its `nearby_monsters` / `monster_ai` residue.

## Measurements

Release build, 5,000 players, 134k monsters:

| | before | after |
|---|---|---|
| `spawn_monster` | 422µs | **1.66µs** (flat, O(1)) |
| `tick_monster_spawns` | 9.12ms | 1.05ms |
| `tick_abandoned_monsters` | n/a | 22.9ms per 10s tick |
| single monster move | — | 2.65µs (flat) |

Reproduce with the ignored benchmark:

```
cargo test --release -p onlinerpg-server spawn_path_cost_at_scale -- --ignored --nocapture
```

## Tests

`server/src/game_state/tests/spawn_soak_tests.rs` compresses the 10s spawn tick
into a loop covering two simulated hours.

```
cargo test -p onlinerpg-server spawn_soak -- --nocapture --test-threads=1
```

The roaming and minimal cases were verified red before the fix and green after.
`stationary_bot_keeps_spawning` is the **control** and is green in both states —
if it ever goes red, the harness is broken, not the server.

`server/src/game_state/tests/spawn_scale_tests.rs` holds the benchmark plus
`registry_counts_track_the_map_through_every_mutation`, which pins the registry
counts against a full rescan after every mutation kind. Count drift is a
*silent* failure mode (the server quietly over- or under-spawns), so that test
matters more than its size suggests; it was verified red-capable by injecting a
missing `debit`.

## Harness gotchas

Two of these cost real debugging time and produced convincing but wrong results:

1. **`player_spatial_cells` is not maintained if you write the `players` map
   directly.** Move test players with `teleport_player`, which is public and
   keeps the index in sync. Writing the roster directly leaves every proximity
   query silently stale.
2. **`tick_abandoned_monsters_at(now)` takes an injected clock** (the public
   `tick_abandoned_monsters()` passes `now_ms()`). A compressed-time test must
   use **one continuous simulated clock** — restarting it mid-test sends `now`
   backwards relative to tracker entries, `saturating_sub` clamps to zero, and
   nothing ever expires.
3. `spawn_monster` returns early on the cap, so a benchmark that preloads to the
   cap measures the reject path rather than real work. Use `floor_level = -1`
   (dungeon) to skip the per-player cap when timing successful spawns.

## Open items

Nothing is blocked. In rough priority:

1. **Move packets are the next ceiling, not spawn.** 135k monsters each
   reporting at 1Hz is ~358ms CPU/sec (~0.36 core), fully serialised on the
   single `monsters` write lock. Most ambient monsters are Idle and don't
   report, so this will not bind immediately. The direction is **sharding the
   monster lock**, not lowering the cap again.
2. **Corpse cleanup is an unbounded detached Tokio task per kill**
   (`server/src/game_state/combat.rs`, the 30s `tokio::spawn` sleep) holding a
   `GameState` clone, instead of the `run_ticks` batching pattern every other
   periodic job uses. It could fold into an existing tick.
3. **`maxMonstersTotal` is now exactly the theoretical maximum** (5,000 × 27),
   so it will effectively never bind — it no longer functions as a runaway
   safety valve. That tradeoff was accepted deliberately to meet the
   5,000-user target, but is worth revisiting if the per-player caps change.
4. The 40-bot soak test is ~13s of the ~10s server suite runtime. Fine for now;
   shrink the bot or tick count if suite time becomes a problem, at the cost of
   scale coverage.
5. The benchmark numbers above may belong in `doc/RUNTIME_PERFORMANCE.md` once
   this branch merges.
