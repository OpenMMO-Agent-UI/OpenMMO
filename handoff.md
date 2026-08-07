# Handoff — OpenMMO monster spawn starvation fix + 5,000-user cap raise

**Repo:** `/Users/tony.pai/OpenMMO`
**Branch:** `agent-target-addressing` (HEAD `af1e55b8`)
**Status:** work is complete and green, but **entirely uncommitted**. Nothing has been pushed or deployed.

---

## What happened in this session

Two connected pieces of work, in order:

1. **Diagnosed and fixed** the user's report: "monsters are no longer spawned after a couple hours when using agent-client." Ran the `mattpocock-skills:diagnosing-bugs` discipline end to end.
2. **Raised `maxMonstersTotal` from 1,000 → 135,000** to support 5,000 concurrent users, which required making the spawn cap checks O(1) first.

### Root cause (confirmed, not theorised)

Surface (ambient) monsters had **no distance-based despawn** — ground items have one (`inventory.rs:1454`), monsters did not. `max_per_player` counts every alive monster a player owns *regardless of distance*, so a roaming agent-client bot leaves monsters behind faster than it kills them. Once 27 accumulate (8+6+5+4+4 = sum of every `maxPerPlayer`), that bot never receives another `SpawnMonsterRequest`. At ~37 concurrent players the abandoned population also exhausted the old 1,000 global cap, which made `tick_monster_spawns` `break` and starved *every* player including humans.

The discriminating evidence: a **stationary** bot kept spawning forever; a **roaming** bot went to zero. Roaming was the single load-bearing element.

### What was built

- `tick_abandoned_monsters` in `server/src/game_state/monster.rs` — despawns surface monsters with no player inside their AOI for 60s. Dungeon monsters excluded (floor lifecycle owns them). Notifies only the owner, which also fixes the agent-client's `nearby_monsters` / `monster_ai` residue (it already handles `MonsterRemoved` at `agent-client/src/state.rs:1621` — **no agent-client change was needed or made**).
- `MonsterRegistry` in the same file — wraps the monster map and incrementally maintains alive counts globally and per `(owner, monster_type)`, so cap checks stop scanning the map.

Read the code and its doc comments for detail; not duplicated here.

---

## Working tree

```
 M data-src/world.json                       # maxMonstersTotal 1000 -> 135000
 M server/src/game_state/combat.rs           # death routed through mark_dead()
 M server/src/game_state/dungeon.rs          # owner reassignment -> reassign_owner()
 M server/src/game_state/mod.rs              # monsters field type; abandoned_monsters tracker
 M server/src/game_state/monster.rs          # MonsterRegistry + tick_abandoned_monsters + O(1) caps
 M server/src/game_state/tests/mod.rs        # registers the two new test modules
 M server/src/main.rs                        # registers the 10s despawn tick
 M server/src/world_config.rs                # default_max_monsters_total + derivation comment
?? server/src/game_state/tests/spawn_scale_tests.rs
?? server/src/game_state/tests/spawn_soak_tests.rs
```

`git diff` is the source of truth. The two untracked test files are new and intended to be kept.

---

## Verification state

All green as of end of session:

- `cargo test -p onlinerpg-server` → **382 passed, 1 ignored**
- `cargo clippy -p onlinerpg-server --all-targets` → **0 warnings**
- `cargo fmt` applied
- `cargo check -p agent-client` → clean (crate name is `agent-client`, not `onlinerpg-agent-client`)

### The two new test files

`spawn_soak_tests.rs` — compresses the 10s spawn tick into a loop to simulate 2 hours of bot behaviour. Four tests; the roaming/minimal ones were verified red before the fix and green after (verified by temporarily short-circuiting the despawn tick, then removing the hook). `stationary_bot_keeps_spawning` is the control and is green in both states — if it ever goes red, the harness is broken, not the server.

```
cargo test -p onlinerpg-server spawn_soak -- --nocapture --test-threads=1
```

`spawn_scale_tests.rs` — two tests:
- `registry_counts_track_the_map_through_every_mutation` — pins the registry counts against a full rescan after every mutation kind. Count drift is a *silent* failure mode (server quietly over/under-spawns), so this is the important guard. Verified red-capable by injecting a missing `debit`.
- `spawn_path_cost_at_scale` — `#[ignore]`d measurement, not an assertion:
  ```
  cargo test --release -p onlinerpg-server spawn_path_cost_at_scale -- --ignored --nocapture
  ```

### Measured before/after (release, 5,000 players, 134k monsters)

| | before | after |
|---|---|---|
| `spawn_monster` | 422µs | **1.66µs** (flat, O(1)) |
| `tick_monster_spawns` | 9.12ms | 1.05ms |
| `tick_abandoned_monsters` | n/a | 22.9ms per 10s tick |
| single monster move | — | 2.65µs (flat) |

---

## Harness gotchas worth not rediscovering

Two harness bugs cost real time in this session — both produced convincing but wrong results:

1. **`player_spatial_cells` is not maintained if you write the `players` map directly.** Move test players via `teleport_player`, which is public and keeps the index in sync. Writing the roster directly makes every proximity query silently stale.
2. **`tick_abandoned_monsters_at(now)` takes an injected clock** (the public `tick_abandoned_monsters()` passes `now_ms()`). A compressed-time test must use **one continuous simulated clock**. Restarting the clock mid-test sends `now` *backwards* relative to tracker entries, `saturating_sub` clamps to 0, and nothing ever expires.

Also: `spawn_monster` returns early on the cap, so a benchmark that preloads to the cap measures the reject path, not the real work. Use `floor_level = -1` (dungeon) to skip the per-player cap when timing successful spawns.

---

## Open items / suggested next steps

Nothing is blocked. In rough priority:

1. **Commit the work.** It is a coherent single change but arguably two commits (despawn fix; cap raise + O(1) registry). The correct-hypothesis note for the commit message: *ambient monsters had no distance despawn, so roaming clients filled the per-player and global spawn caps with monsters nobody could see.*
2. **Move packets are the next ceiling, not spawn.** 135k monsters each reporting at 1Hz is ~358ms CPU/sec (~0.36 core) fully serialised on the single `monsters` write lock. Most ambient monsters are Idle and don't report, so this won't bind immediately. The direction is **sharding the monster lock**, not lowering the cap again. This was flagged to the user, not started.
3. **Corpse cleanup is an unbounded detached Tokio task per kill** (`server/src/game_state/combat.rs:~538`) — sleeps 30s holding a `GameState` clone, instead of using the `run_ticks` batching pattern every other periodic job uses. Noted twice to the user, never actioned. It could fold into an existing tick.
4. **Consider whether 135,000 should stay a real safety valve.** It is set to exactly the theoretical maximum (5,000 users × 27), so it will effectively never bind — which means it no longer protects against runaway. The user asked for 5,000-user capacity explicitly; this tradeoff was stated but not decided.
5. The 40-bot soak test takes ~13s of the ~10s suite runtime. Fine for now; shrink the bot/tick count if suite time becomes a problem, at the cost of scale coverage.

---

## Working preferences observed

- **Reply to the user in Traditional Chinese (zh-TW)** even though they write in English. This is in the user's persistent memory (`feedback_reply_language.md`).
- `CLAUDE.md` rules that mattered here: avoid comments unless necessary and keep them short; new assets must be recorded in `doc/assets/` (not relevant to this change); **the system must handle 5,000 simultaneous users without performance problems** — this is why the cap raise required the O(1) refactor rather than just editing JSON.
- The user does not want subagents or workflows spawned unless they ask. `/mattpocock-skills:diagnosing-bugs` was invoked by the user, and its discipline (feedback loop before hypotheses) was followed strictly and was what actually found the bug.

## Suggested skills

- **`commit-agent`** — for step 1. It runs format/lint/type checks before committing, which matches the current clean state.
- **`mattpocock-skills:diagnosing-bugs`** — if anything regresses in spawning. The loop already exists (`spawn_soak_tests.rs`); start from Phase 1 by pointing it at that command rather than rebuilding one.
- **`deploy`** — only if the user asks to ship. Note it restarts the game and disconnects every live player, and this change alters spawn behaviour for all players.
- **`simplify`** — optional pass over the diff. `MonsterRegistry` grew a fairly wide passthrough API (`get`/`get_mut`/`values`/`iter`/`Index`) to avoid touching 14 call sites; worth a look for altitude.
- Avoid `/mattpocock-skills:code-review` unless asked — the user has not requested review, and the work is already test-verified.

