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
`currentPlayer` each frame (camera, terrain streaming and HUD all read it) and
publishes that pose through `gameStore` on the minimap's own quantization —
mutating the Vector3 in place notifies no subscriber, and the local player's
publisher (`PlayerControl`) is not mounted here. `PlayerControl`, the action
cluster and the chat input are not rendered, and `monsterManager` gates
ownership through `ownedByMe()` so a spectator never runs WASM brains
competing with the agent's own.

A mirrored body is drawn at the speed its frames claim, so the mirror only
looks right when the frames carry the whole of it. The watched character is
`currentPlayer`, never in `otherPlayers`, so `remotePlayerManager` reads its
mount from there (the same singleton trap the floorLevel lookup documents),
and a leg handed over mid-route (`nextLeg`) inherits the sprint it was already
travelling at rather than defaulting to a walk. The relay half is the app
repo's (`src/proxy.js` synthesizes the agent's own `PlayerMoved` and must
carry `sprinting`, and replays `PlayerMountChanged` to late joiners): drop
either half and the drawn body falls behind its own frames until the 8 m
desync guard snaps it forward — teleporting every few seconds.

**Lives in**: `client/src/App.svelte` (`screen` init, `observe()` on mount,
`sceneCanMount`, banners), `client/src/lib/network/socket.ts` (`observe()`,
`isObserver` early returns), `client/src/lib/components/GameScene.svelte`
(per-frame copy, `publishObservedPose`, `nextLeg` handoff), `.../game-scene/
GameScenePlayersLayer.svelte` (`PlayerControl` gate), `.../GameHud.svelte`
(`.action-cluster` gate), `.../ChatPanel.svelte` (`.chat-input` gate +
`handleGlobalKeydown` return), `client/src/lib/managers/monsterManager.ts`
(`ownedByMe`), `client/src/lib/managers/remotePlayerManager.ts`
(`drawnState`, the `setTargetPosition` sprint default),
`client/vite.config.ts` (`resolve.preserveSymlinks`).

**Conflict resolution**: `observerStore.ts` and `observedPath.ts` are **not in
this repo** — they are gitignored symlinks into `~/openmmo-client/overlay/`
(`link.sh`), so a rebase can never touch them. Only the call sites above live
here, so a conflict is always a gate (`{#if !isObserver}`, an early return)
inside code master restructured: keep the gate, adopt master's structure.
`preserveSymlinks` must stay or rollup resolves the symlinks outside the
project and their relative imports break. If `observerStore` fails to resolve,
run `link.sh` — do not vendor it in.

**Verify**: `remotePlayerMirror.test.ts` covers the mirror's speed; the rest
is Svelte/UI with no unit tests. `npm run check` in `client/` must
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
knobs (level margin, low-health threshold, food, potion and return-scroll
stock, bag-full threshold).
The trip home for food fires when the sprint goes (satiation at or under
`NORMAL_MIN`, the threshold `should_eat` uses), not at `Weak` two thirds
further down: waiting meant walking home from as far out as the ring goes at
`WEAK_MOVE_MULT`, with `WEAK_CARRY_MULT` shrinking the bag on the way so the
trip that finally fired read as a full-bag one instead.

A worker ticks a small state machine over `SharedState` and runs its
decisions through the LLM driver's own action executor, so combat,
pathfinding, looting, trading and the spectator mirror are all reused as-is.
Turns are mirrored to the watch feed under the kind `worker`, which is what
keeps the desktop app's action captions working with no model in the loop.

A town trip searches the town rather than glancing at it: the worker walks the
zone's centre and its four quarters until a merchant is in sight, because
NPC_SIGHT_RADIUS is smaller than a town and one look from the middle wrote
every trip off. Zones under 20m a side are map-editor slivers, not towns, and
are skipped. Restocking buys food from the merchant's own catalog (Wick opens
with bread, Rica with apples) so an order is never for something unstocked.

On the way out it takes what is already in reach: the ring outranks
*chasing* the fodder, because stopping to run at every kobold pins the worker
to the weakest ring, but something inside `STRIKE_RANGE` costs no walking at
all, so passing it up buys nothing. `free_kill` is the same predicate the
walk interrupt reads, and that is deliberate — a leg that stopped for prey
the fighter then declined to swing at would stutter in place beside it.

The fighter hunts on a ring, not wherever it happens to stand. Master gates
ambient spawns by distance from the **spawn point** — a level-N type is only
offered `(N - 1) x 70 m` out (`AMBIENT_SPAWN_METERS_PER_LEVEL`,
`min_ambient_town_distance`) — so a worker that only cleared the town margin
ground level-1 kobolds forever. `hunt_radius` mirrors that formula, capped by
the strongest type, and the walk out is checked *before* target selection:
the fodder underfoot is eligible at every level, and a level-up has to be able
to widen the ring even while the old one still has something standing in it.
The bearing is our own, out from the spawn point, turned until the spot is
standable — and carries the patrol's turn, so it is not one direction
forever. Standing exactly on the spawn point reads as bearing zero (due +x)
and a walk back down from a peak lands close enough to read the same way, so
a single bearing had the fighter march at the same mountain, get sent home by
`MAX_WALK_Y`, and set off at it again. Nothing in these decisions can see how
high a spot is — `is_standable` is sync and the height sampler is not — so
fanning the bearing out is what breaks that loop; a strand counts as a failed
attempt for exactly this. `level_margin` deliberately does not widen the ring —
`best_eligible` prefers our own level, so the extra walk unlocks what it then
declines to pick.

With nothing eligible and no town to leave, the fighter patrols the ring
rather than idling. Standing still is not patience since v37 — the server
rolls a spawn per metre walked (`SPAWN_CHANCE_PER_METER`, about one monster
per 12 m) and none at all for standing still, so idling is the one choice
guaranteed to produce nothing. `patrol_target` walks one `PATROL_LEG` around
the ring, holding the radius rather than picking a heading: the monster table
is gated on distance from the spawn point, spawns land in a ±30° cone off the
heading, and `is_standable` has no water or height test, so a random
direction would downgrade the table, scatter the spawns behind us, and walk
into the sea in turn. A blocked arc falls back to `hunt_target`'s own sweep in
eighths. `Patrol` remembers where the last leg was issued from: a leg that
moved us resets the arc offset, so every working leg is the same length,
while one that left us standing — standable target, unreachable ground —
reaches further round instead of being reissued unchanged.

A leg gives way to a fight. `execute_move` runs to its waypoint whatever
turns up — the only early exits are a server position correction and a send
error — and the server drops ambient spawns about 20 m ahead of a walker
inside a ±30° cone off the heading, so the monster worth fighting lands
squarely in the stretch the fighter is not looking at. `SharedState::
abandon_leg_for` carries the level margin while a hunting leg is walking;
`walk_waypoints` checks `prey_in_reach` between steps and returns
`MoveResult::Interrupted`, and the next tick attacks. It is armed for hunting
legs only — abandoning a town run every time something wanders past is how a
town trip never finishes.

A full bag reads a return scroll home rather than walking, keeping the last
one for the low-health escape — the scroll lands on the spawn point, which is
both where town is and where the ring is measured from. Supply carried past
its configured cap is sold on that trip without waiting for a Sellable mark:
writing `potion_stock = 10` is already saying ten is all we want.

Towns come from the terrain API, not the wire. Protocol v37 deleted
`ServerMessage::NoSpawnZones` along with the whole client-driven spawn system
(spawning is server-side and granted per metre walked now), but the server
still refuses to place an ambient monster inside a no-spawn zone, so a worker
that does not know where towns are stands in one waiting for monsters that
cannot come. `fetch_no_spawn_zones_around` reads
`/api/terrain/zones/{rx}/{rz}` — the same endpoint the browser client's map
editor uses — per region, alongside the houses/furniture prefetch that
already runs on startup and on every chunk crossing. Only a successful
response marks a region done: a region with no towns answers with an empty
list, so a miss is transient and has to stay retryable, and one dropped
request must not blind the worker to a town it is standing in.

The fighter rides when it has the reins. `horse_reins` is a toggle the server
refuses in combat, indoors, in water and below ground, and it dismounts the
rider by itself the moment a fight starts (`server/src/game_state/mounts.rs`),
so the only rule worth keeping client-side is never to use them while already
up — that would climb straight back off. `ride_to` mounts before any leg of
`RIDE_WORTH_IT` (30 m) or more: the commute back to the anchor, the walk into
town and out of a useless one, the run at a distant target. A patrol leg
(`PATROL_LEG`, 28 m) sits just under that line on purpose — it is walked to
find a fight, and the fight would dismount. The reins are `is_keeper` kit:
the dearest thing in the bag is not sold off a stray Sellable mark.

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
(`no_spawn_zones` and `fetched_zone_regions` — wholly ours since v37, not a
`pub` on an upstream field any more), `driver/movement.rs`
(`RegionZones` + `fetch_no_spawn_zones_around`, modelled on
`fetch_furniture_around` right above it, plus the `abandon_leg_for` check in
`walk_waypoints` and the `MoveResult::Interrupted` arms its callers grew),
and `item_defs.rs` (`ItemDef::weight`, for the bag-full check against the
server's STR×15 carry cap). `fighter.rs` also reads `data-src/world.json`
directly for `spawnPosition` — the tracked source file, not the gitignored
`data/` output the monster levels come from.

**Conflict resolution**: everything under `driver/worker/` is ours; take it
whole. The touch points are additive one-liners — re-apply them onto master's
structure rather than keeping our version of the surrounding code. If master
grows its own non-LLM driver, prefer master's and port the fighter/fisher
rules onto it. `handle_response`, `tick_combat`, `respawn_due`,
`request_respawn` and `decline_lapsed_trade` are reached through `super::`,
so a rename upstream is a compile error here, never silent drift.

**Conflict watch**: `METERS_PER_LEVEL` here mirrors master's
`AMBIENT_SPAWN_METERS_PER_LEVEL`, the way `TOWN_MARGIN` mirrors
`NO_SPAWN_MARGIN`. Master retuning either constant is silent here — check
both on every sync. The same goes for the town data itself: every failure
mode of `no_spawn_zones` is a *stall*, never an error. An empty list reads as
"no towns anywhere", which makes `escape_target` return `None` and parks the
fighter exactly where it stands — and the unit tests set the field directly,
so they keep passing while nothing fills it. That is how v37 nearly shipped
with the feature silently inert. If master changes the shape of
`/api/terrain/zones/{rx}/{rz}` or the `noSpawnZones` key, `RegionZones`
deserialization fails into an empty list rather than complaining: check the
endpoint by hand on a sync that touches the zone or terrain code.

**Verify**: `cargo test -p agent-client worker` (tests: eligibility and
level-matched target choice, approach, potion/scroll/eat/town-trip decisions,
the town-exit rule and the in-town search, loot radius, water selection,
label parsing, restocking against a merchant's catalog, the hunting ring and
its cap, the scroll ride home, the horse on a long leg and not on a patrol
one, surplus supply, and that every emitted step parses as an action). App side:
`npm test` covers the `[npcs.worker]` config generation and the LLM-validation
skip. Live: pick a worker under Settings → Behaviour → Automatic play, hit
**Apply & restart**, and watch it grind in the spectator view — the Log drawer
carries its decisions, the Thoughts drawer stays empty. Mark items in the Bag
drawer, Apply labels, and the next town trip sells only those.

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

## 5. Continuous walking (leg handover)

**What**: a route is walked as one movement, not a step per network round
trip. A walk subdivides its path at `MAX_STEP_DIST` (4 m) and used to send
each leg with `append: false` once the previous one had run out — which
replaces the server's waypoint queue, so the queue was empty for a round trip
in every 4 m and the body stood still there. The legs after the first now
chain onto the queue (`append: true`, the same way the web client hands over a
path it has already validated), and the first handover is made
`HANDOVER_LEAD_MS` (250 ms) early so the queue never drains between two legs.

The lead is borrowed once and given back where the route runs out
(`settle_handover`): the pacing is what keeps our own position model honest,
and a walk that arrived 250 ms "early" would have the caller sell, pick up or
swing while the body is still a metre short — which is what the server
judges. A stop replaces rather than chains: the first leg of a walk, the leg
after a server position correction, and the no-path nudge. Anything that
gives a walk up (prey in reach, a target gone) is followed by a move that
replaces the queue anyway.

**Lives in**: `agent-client/src/state/movement.rs` (`send_step`'s `append`
parameter — the one place a `PlayerMove` is built), `driver/walk.rs`
(`HANDOVER_LEAD_MS`, `pace_after_step`, `settle_handover`, the `queued` /
`lead_owed` pair in `walk` and in `open_blocking_door`, `step_along`'s
`append`).

**Conflict resolution**: mechanical, like the sprint threading — re-apply the
extra parameter onto master's `send_step`/`step_along` rather than keeping our
movers. Two rules: the lead comes out of the *first* leg only (taking it from
every leg paces the route faster than the body can walk it, and the server's
queue grows for as long as the walk lasts), and every path that ends a route
settles the debt. Master's server side already supports this — `append`
chains off the queue tail for the distance and floor checks
(`server/src/game_state/player.rs`) and caps the queue at
`MAX_QUEUED_WAYPOINTS`; we never keep more than one leg in it.

**Verify**: `cargo test -p agent-client walk::` —
`a_route_hands_its_legs_over_without_stopping` asserts both halves (the append
flags on the wire, and that 20 m still takes 20 m of walking) and
`the_handover_lead_is_paid_once` pins the pacing. Live: watch an agent in the
spectator view — it walks a route without the twice-a-second hitch.

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
