# tweak-agent-client custom features

`tweak-agent-client` is a personal customization branch on top of `master`.
`master` tracks the upstream/official repo and is rebased onto periodically —
each rebase can conflict with the feature below. This file is the checklist
used by the `/rebase-tweak-agent-client` command (see
`.claude/commands/rebase-tweak-agent-client.md`) to confirm the customization
survived conflict resolution. Keep it in sync: when the customization commit
is split, renamed, or dropped (because master grew an equivalent/better
version), update this entry in the same PR/session.

Each entry: what it does, where it lives, how a rebase conflict on it should
usually be resolved, and how to verify it survived (unit test and/or a live
signal to grep for).

---

## 1. Web-client spectator mode

**What**: the web client can open read-only against an agent's mirror socket
and watch the agent's world live. `isObserver` (from `observerStore`, read off
the URL) makes `App.svelte` skip login/character-select straight to `game` and
call `networkManager.observe()`; every send path in `NetworkManager` returns
early, including `ensureHandshake`, so the mirror's relayed `JoinSuccess` is
the only handshake. The watched agent arrives as a *remote* player, so
`GameScene` copies its interpolated position/animation onto `currentPlayer`
each frame (the camera, terrain streaming and HUD all read `currentPlayer`)
and hands over the next `observedPath` leg on arrival. `PlayerControl` is not
mounted, the action cluster and chat input are not rendered, and
`monsterManager` routes its ownership checks through `ownedByMe()` so a
spectator never spins up WASM brains competing with the agent's own
simulation.

**Lives in**: `client/src/App.svelte` (`screen` init, `observe()` on mount,
`sceneCanMount`, the waiting/error banners), `client/src/lib/network/socket.ts`
(`observe()`, the `isObserver` early returns), `client/src/lib/components/
GameScene.svelte` (the per-frame copy + `nextLeg` handoff),
`client/src/lib/components/game-scene/GameScenePlayersLayer.svelte`
(`PlayerControl` gate), `GameHud.svelte` (`.action-cluster` gate),
`ChatPanel.svelte` (`.chat-input` gate + `handleGlobalKeydown` early return),
`client/src/lib/managers/monsterManager.ts` (`ownedByMe`),
`client/vite.config.ts` (`resolve.preserveSymlinks`).

**Conflict resolution note**: the two modules this depends on —
`client/src/lib/stores/observerStore.ts` and
`client/src/lib/managers/observedPath.ts` — are **not in this repo**. They are
symlinks into `~/openmmo-client/overlay/`, created by
`openmmo-client/scripts/link.sh` and gitignored, precisely so a master rebase
can never touch them. Only the *call sites* listed above live here, so a
conflict will always be about a gate (`{#if !isObserver}`, an early return)
sitting inside code master restructured — keep the gate, adopt master's
structure around it. `preserveSymlinks` must stay or rollup resolves the
symlinked modules to their real paths outside the project and their relative
imports break. If `observerStore` ever fails to resolve, the fix is to run
`link.sh`, not to vendor the file into this repo.

**Verify**: no unit tests (all Svelte/UI). Static: `npm run check` in
`client/` must pass — it resolves the symlinked modules, so a broken overlay
link fails loudly there. A missing-wasm-export error there is stale generated
output, not a regression: `client/src/lib/wasm/` is gitignored, so run
`npm run build:wasm` after master adds a `wasm_api.rs` export. Live signal:
not part of the agent-client run; open the client against a mirror URL and
confirm the login screen is skipped, no quickslot bar / corner buttons / chat
input are drawn, and the camera follows the agent.

---

## 2. Configurable OpenAI-compatible history cap

**What**: `openai.max_messages` in `config.toml` sets how many messages of
conversation history the OpenAI-compatible backend carries, system prompt
included. It replaces a hardcoded `const MAX_MESSAGES: usize = 41`, and
defaults to that same 41, so an unset config behaves exactly as before. The
desktop app writes the key and exposes it as "Messages kept". Note this is the
first Rust customization back on this branch after the 2026-07-30 drop below.

**Lives in**: `agent-client/src/openai.rs` (`OpenAiConfig::max_messages`,
`DEFAULT_MAX_MESSAGES`, `MIN_MAX_MESSAGES`, the clamp in `endpoint()`, and the
trim in the invoker), `agent-client/src/openrouter.rs` (passes
`DEFAULT_MAX_MESSAGES` when building its `Endpoint`).

**Conflict resolution note**: `Endpoint` and the invoker are shared with
OpenRouter, so if master restructures either, both constructors must keep
setting `max_messages` — OpenRouter deliberately passes the default rather
than growing a config key of its own, since only the OpenAI-compatible
backend's context window is user-tunable. If master grows its own history cap,
prefer master's and drop this entry. The floor is not optional: the trim
computes `turn.len() - (max_messages - 1)` on a `usize`, so any path that lets
a value below 3 reach it underflows and panics mid-turn.

**Verify**: `cargo test -p agent-client openai` —
`max_messages_never_resolves_below_the_trim_floor` covers the default, the
clamp, and a passthrough value. Live signal: set `max_messages` low and grep
the agent log for `trimmed conversation history to N messages`.

---

## Superseded / intentionally dropped (do not re-add without checking master first)

- **All agent-client (Rust) customizations** (2026-07-30) — dropped by
  request, not master-superseded. This branch used to also carry: monster
  targeting by level tier, a cross-floor/stairs movement fix, a walking
  stall-timeout, trade windows not hard-blocking walking away, blocked/failed
  actions reporting back to the LLM, NPCs thinking without a nearby human
  audience, combat counted as prompt-pacing activity, a visible monster
  counting as "active", ground-loot pickup prioritized as Urgent, an anti-loop
  safeguard for dropped items, and Negan's instance-prompt landmark/vendor
  knowledge. None of that remains on this branch; only the web-client
  spectator mode above is still customized. Recoverable from tag
  `backup/tweak-agent-client-pre-feature-drop-20260730-1920` if any of it is
  wanted back — check whether master has grown an equivalent feature first.
