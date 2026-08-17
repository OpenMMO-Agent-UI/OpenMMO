# tweak-agent-client custom features

Personal customization branch on top of `master`, which tracks upstream and is
rebased onto periodically. This is the checklist
`.claude/commands/rebase-tweak-agent-client.md` uses to confirm each
customization survived a rebase — keep it in sync when one is split, renamed
or dropped. Each entry: what, where, how to resolve a conflict on it, how to
verify it survived.

---

## 1. Web-client spectator mode

**What**: the client can open read-only against an agent's mirror socket and
watch its world live. `isObserver` skips login/character-select straight to
`game` and calls `networkManager.observe()`; every send path returns early,
so the mirror's relayed `JoinSuccess` is the only handshake. The agent arrives
as a *remote* player, so `GameScene` copies its position/animation onto
`currentPlayer` each frame (camera, terrain streaming and HUD all read it).
`PlayerControl`, the action cluster and the chat input are not rendered, and
`monsterManager` gates ownership through `ownedByMe()` so a spectator never
runs WASM brains competing with the agent's own.

**Lives in**: `client/src/App.svelte` (`screen` init, `observe()` on mount,
`sceneCanMount`, banners), `client/src/lib/network/socket.ts` (`observe()`,
`isObserver` early returns), `client/src/lib/components/GameScene.svelte`
(per-frame copy + `nextLeg` handoff), `.../game-scene/
GameScenePlayersLayer.svelte` (`PlayerControl` gate), `.../GameHud.svelte`
(`.action-cluster` gate), `.../ChatPanel.svelte` (`.chat-input` gate +
`handleGlobalKeydown` return), `client/src/lib/managers/monsterManager.ts`
(`ownedByMe`), `client/vite.config.ts` (`resolve.preserveSymlinks`).

**Conflict resolution**: `observerStore.ts` and `observedPath.ts` are **not in
this repo** — they are gitignored symlinks into `~/openmmo-client/overlay/`
(`link.sh`), so a rebase can never touch them. Only the call sites above live
here, so a conflict is always a gate (`{#if !isObserver}`, an early return)
inside code master restructured: keep the gate, adopt master's structure.
`preserveSymlinks` must stay or rollup resolves the symlinks outside the
project and their relative imports break. If `observerStore` fails to resolve,
run `link.sh` — do not vendor it in.

**Verify**: no unit tests (all Svelte/UI). `npm run check` in `client/` must
pass; it resolves the symlinks, so a broken overlay link fails there. A
missing-wasm-export error is stale generated output — `client/src/lib/wasm/`
is gitignored, so run `npm run build:wasm` after master adds an export. Live:
open the client against a mirror URL and confirm login is skipped, no
quickslot bar / corner buttons / chat input, and the camera follows the agent.

---

## 2. Configurable OpenAI-compatible history cap

**What**: `openai.max_messages` sets how many messages of history the
OpenAI-compatible backend carries, system prompt included. Replaces a
hardcoded `MAX_MESSAGES = 41` and defaults to it, so an unset config is
unchanged. The desktop app writes the key as "Messages kept".

**Lives in**: `agent-client/src/openai.rs` (`OpenAiConfig::max_messages`,
`DEFAULT_MAX_MESSAGES`, `MIN_MAX_MESSAGES`, the clamp in `endpoint()`, the
trim in the invoker), `agent-client/src/openrouter.rs` (passes the default).

**Conflict resolution**: `Endpoint` and the invoker are shared with OpenRouter,
so both constructors must keep setting `max_messages` — OpenRouter passes the
default rather than growing a key of its own. If master grows its own cap,
prefer master's and drop this entry. The floor is not optional: the trim
computes `turn.len() - (max_messages - 1)` on a `usize`, so anything below 3
underflows and panics mid-turn.

**Verify**: `cargo test -p agent-client openai` —
`max_messages_never_resolves_below_the_trim_floor`. Live: set it low and grep
the log for `trimmed conversation history to`.

---

## 3. Rule-based workers

**What**: deterministic, LLM-free engines for Automatic play. `[npcs.worker]`
picks one (`fighter`, `fisher`, or `none` for the LLM agent) and carries its
knobs (level margin, low-health threshold, potion stock, bag-full threshold).
A worker ticks a small state machine over `SharedState` and runs its
decisions through the LLM driver's own action executor, so combat,
pathfinding, looting, trading and the spectator mirror are all reused as-is.
Turns are mirrored to the watch feed under the kind `worker`, which is what
keeps the desktop app's action captions working with no model in the loop.

Workers respect the desktop app's bag labels: the sell/drop marks written
into the character's `instance.txt` under the `<!-- BAG LABELS -->` block are
re-read on every town errand, and only marked loot is sold / marked junk is
dropped. Unmarked items stay in the bag, so a worker never dumps a full bag
the player did not get the ok to sell (`labels.rs` parses the block).

**Lives in**: `agent-client/src/driver/worker/` (`mod.rs` the loop and the
shared survival/town decisions, plus `fighter.rs`, `fisher.rs`, `labels.rs`,
`tests.rs`) — self-contained. Five touch points outside it:
`driver/mod.rs` (`mod worker;` + the `pub use`), `orchestrator.rs`
(`NpcConfig::worker`, entering the game when a worker is configured, spawning
`worker_driver` in place of the LLM task, the mode log line), `state/mod.rs`
(`no_spawn_zones` made `pub` — towns are how a worker finds a merchant), and
`item_defs.rs` (`ItemDef::weight`, for the bag-full check against the
server's STR×15 carry cap).

**Conflict resolution**: everything under `driver/worker/` is ours; take it
whole. The touch points are additive one-liners — re-apply them onto master's
structure rather than keeping our version of the surrounding code. If master
grows its own non-LLM driver, prefer master's and port the fighter/fisher
rules onto it. `handle_response`, `tick_combat`, `respawn_due`,
`request_respawn` and `decline_lapsed_trade` are reached through `super::`,
so a rename upstream is a compile error here, never silent drift.

**Verify**: `cargo test -p agent-client worker` (25 tests: eligibility,
approach, potion/scroll/eat/town-trip decisions, loot radius, water
selection, label parsing and that every emitted step parses as an action).
App side: `npm test` covers the `[npcs.worker]` config generation and the
LLM-validation skip. Live: pick a worker under Settings → Behaviour →
Automatic play, hit **Apply & restart**, and watch it grind in the spectator
view — the Log drawer carries its decisions, the Thoughts drawer stays
empty. Mark items in the Bag drawer, Apply labels, and the next town trip
sells only those.

---

## 4. Agent sprinting

**What**: agents sprint (1.5×) on every walk while well fed, instead of always
sending `sprinting: false`. Per-agent `always_sprint` (`[[npcs]]`, defaults
true) sets the default; the LLM opts out of one walk with `"sprint": false` on
`move`, `attack`, `pickup` or `follow`; workers hardcode `"sprint": true` on
their `Walk` step. The decision is `override.unwrap_or(always_sprint)` and is
resolved at step-send time against the server's own hunger gate
(`satiation > 300`), so a journey that drains across the boundary downgrades
mid-walk instead of rubber-banding. Pacing divides by the sprint multiplier so
the local prediction matches the server.

**Lives in**: `state/movement.rs` (`sprint_allowed`, `send_step` takes
`Option<bool>` and returns the resolved flag), `state/mod.rs`
(`always_sprint` field), `driver/movement.rs` (`travel_ms`, the `sprint`
parameter down `execute_move`/`walk_path`/`walk_waypoints`/
`open_blocking_door`, and the schedule force-move legs), `driver/combat.rs`
(the parameter down `chase_target` and its five wrappers,
`compute_step_toward`), `driver/action.rs` (the `sprint` field on the four
actions + their doc blocks and the movement-speed paragraph),
`driver/execute.rs` (call sites), `driver/worker/mod.rs` (`Step::Walk`),
`orchestrator.rs` (`NpcConfig::always_sprint`, copied onto `SharedState`).

**Conflict resolution**: the threading is mechanical — re-apply the extra
`Option<bool>` parameter onto master's mover signatures rather than keeping
our version of the movers. The two rules that matter: the gate is strictly
`satiation > NORMAL_MIN` (matching the server, not the client's Normal band),
and `sprint_allowed` stays the only place that gate is evaluated — `send_step`
and the no-path fallback in `compute_step_toward` both stamp the flag, but
neither re-derives it.

**Verify**: `cargo test -p agent-client sprint` plus
`a_sprinting_step_is_paced_by_the_sprint_speed` and
`agents_sprint_unless_the_config_says_otherwise`. Live: watch an agent in the
spectator view — a fed agent runs, and it drops to a walk once `[Hunger]`
reports sprinting unavailable.

---

## Superseded / intentionally dropped (do not re-add without checking master first)

- **All pre-2026-07-30 agent-client (Rust) customizations** — dropped by
  request, not master-superseded: monster targeting by level tier, cross-floor
  stairs movement fix, walking stall-timeout, trade windows not blocking
  walking away, blocked/failed actions reported to the LLM, NPCs thinking with
  no human nearby, combat and visible monsters counting as "active",
  ground-loot pickup as Urgent, dropped-item anti-loop, Negan's instance-prompt
  landmarks. Not on this branch and **not recoverable** — the
  `backup/tweak-agent-client-pre-feature-drop-20260730-1920` tag they were
  parked on no longer exists. If one surfaces in a conflict it is master's
  code, not ours.
