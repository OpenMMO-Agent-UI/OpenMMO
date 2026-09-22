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
picks one (`fighter`, `fisher`, `dungeoneer`, or `none` for the LLM agent)
and carries its knobs (level margin, low-health threshold, food, potion and
return-scroll stock, bag-full threshold).
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

Workers do not ride. The reins are still `is_keeper` kit — the dearest thing in
the bag is not sold off a stray Sellable mark, and the player may ride by hand —
but no worker climbs on. A rider is steered by the server on an arc from its
current facing, with no wall slide (`tick_player_movement`), and a leg the
worker decides is walked to its end by `execute_move` without coming back to
ask. Every rule written to reconcile those two — when the server would refuse a
mount, what to do when it snapped the horse back, how to face the route first,
how to cut a leg so the decision could be revisited — added a state the
spectator could see the character struggling in, and the sum never read as a
horse setting off. Upstream's `recover_mount` in `walk.rs` still answers a
snapped-back mounted walk, so a hand-mounted character an LLM drives is unaffected.

Every worker but the dungeoneer climbs back to ground level before anything
else it would do (`back_to_the_surface`). Off the surface, `resolve_goal_floor`
refuses a goal outside the dungeon we stand in — and one off the storey we
stand on — before a route is even looked for, so the anchor, the town and the
water are all equally unreachable and the worker fights whatever wanders past
while never moving toward what it is for. Two ways in: switching the worker
kind while the dungeoneer was three floors down (the switch restarts the
process; it does not walk the character out), and dying, which wakes it in a
bed upstairs. `Step::Surface` is a bare `{"type": "move", "depth": 0}` — a
depth with no dungeon named is the executor's "leave where you are", which is
the one leg a worker that never learned the dungeon's name can walk.

The fisher walks to its spot (`fishing_x`/`fishing_z`; unset fishes from
wherever it stands), then scans the terrain grid around the spot for water —
sea from the heightmap, rivers from the splat's river-bed entry — and remembers
the cast it planned: the water cell and the shore to stand on. Every later tick
casts from memory rather than re-sampling tiles, and a worker sent away (a
town trip, a fight it was dragged into) walks back to that shore first. The
memory is dropped only when the shore has been reached and the water still
reads out of cast range, which is what a stale plan looks like. A spot with no
water within `EVENT_DELIVERY_RADIUS` is reported once and the fisher waits
there: the setting is the fix, and wandering off in search of water would hide
that it was set wrong. Without a rod in hand or in the bag the fisher's town
trip fires and buys one from the merchant in front of it (Rica stocks it),
under the same purse bound as any restock; what it catches is sold under the
same Sellable marks as any other loot, a `coin_catch` (the sunken purse) is
opened the tick it is noticed, and junk marked Dropable is dropped where it
stands rather than carried to town (so the fisher re-reads the labels every
tick, not only on a town errand). Being hit is answered with the rod, the way
every worker answers it — swapping to a weapon and back is not attempted.
Meals go cheapest first: every fish feeds the same, and a trophy sturgeon is
a purse, not a lunch.

Being hit is answered whatever the rules say, and a pack is answered one at
a time: every monster that has hit us and still stands is remembered
(`Pack`), the current fight is seen through — switching mid-fight leaves the
whole pack alive — and the moment a kill lands the nearest remaining attacker
is the target, before the loot sweep and without a fresh decision. An escape
(the return scroll) forgets the pack, or the next tick would turn round.

A meal runs to `satiation_target` (default 700, the starting satiation): it
starts when the sprint goes, and once started keeps eating — one confirmed
bite per `HungerUpdate`, cheapest food first — until satiation reaches the
target. Stopping at the first bite past hungry meant a raw fish (40 of 1000)
bought about a minute of sprinting before the next one.

A worker decides every 200 ms when nothing is being fought (`DECISION_TICK`,
the walk's own idle tick); in a fight the tick is whatever is left of the
attack cooldown, so a swing goes out the moment the last one has run.

Underground, a door leaving the interest set (`DungeonDoorState` with
`is_open: None`) keeps the state it was last seen in rather than reading as
shut. Nothing but a locked door closes on its own, and the climb back up a
section targets the floor above's arrival landing — behind that floor's door,
which the mover cannot open from the floor below. Read as shut, every door
opened on the way down sealed the way back up, and the dungeoneer swept the
section's bottom floor forever ("the way is sealed"). The server restates
the real state the moment the door is back in range.

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
its cap, the scroll ride home, the reins never sold,
a leg that is getting somewhere not counting as a stall, the sweep resuming
where a fight left the character, a surface worker left underground or upstairs
climbing out first, surplus supply, and that every emitted step parses as an
action). App side:
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

## 5. Walk recovery on server-approved movement

**What**: `walk` hands the server one goal at a time and follows its
`MoveStatus`; these are the ways it used to get stuck, and what it does
instead.

- A goal beyond `MAX_LEG` (48 m) is a leg cut `MAX_LEG` along our own route
  (`leg_along`), not a straight-line point that can sit inside a fenced yard.
  On the surface the route is `find_long_path_to` (60k-node budget; the
  server's 2000 cannot route round a walled-in block), and a leg the server
  ends `NodeLimit`/`Partial` short of is marked a dead end
  (`mark_unreachable`, lapses after `DEAD_END_FOR`) and re-planned round, up to
  `MAX_REPLANS` times, before doors are tried.
- Only the aim moving re-plans (`last_aim`). The leg derived from where we
  stand drifts with every step; re-sending on that drift flipped between
  near-equal routes and walked the body back and forth.
- `Blocked`, or no metre of progress in `STALL_AFTER`, first steps to the
  centre of the current cell (`recentre`): the server plans from wherever we
  stand but refuses a first leg whose body sweep grazes an obstacle beside a
  one-cell gap. Still stuck ends the walk as `LostReason::Stalled`.
- A cross-floor aim comes only from a route that reached the other floor
  (`stair_aim`); otherwise a door is opened first. A failed route's first
  stair led up and out of the dungeon when the way down was behind a door.
- A door is walked to on the side our route reaches, in legs
  (`walk_to_door_side`) — the server refuses any goal past
  `MAX_MOVE_TARGET_DISTANCE` (60 m). `Dungeon::door_sides` puts each side on a
  cell centre; a two-cell door's midpoint sat on a cell edge the server read
  as wall.
- A chase that ends `NoPath`/`LockedDoor`/`Timeout` gives the monster up for
  `MONSTER_GIVE_UP` (`give_up_on_monster`), and `fighter::is_eligible` skips
  it, so a monster behind a fence is not picked, chased and lost every tick.
- Failures are reported: a lost walk leaves `walk_diagnostic` (our A*
  verdict, what closes our cell, a 9×9 map and the last `walk_trace` steps,
  including the server's own `PlayerMovePath`), and a worker mirrors it and
  `[MoveFailed]`/`[Unreachable]` notes to the watch feed. `AGENT_LOG_FILE`
  tees the log to a file.

Every set this adds is bounded: `monsters_given_up` and `unreachable_cells`
expire, `walk_trace` keeps 24 lines, `wet_cells` starts over at
`WET_CELLS_CAP`, and `loot_given_up` forgets an item once it leaves the
ground.

**Lives in**: `driver/walk.rs` (`walk`, `walk_inner`, `stair_aim`,
`leg_along`, `mark_dead_end`, `recentre`, `open_blocking_door`,
`walk_to_door_side`, `describe_blockage`, `trace`), `state/movement.rs`
(`find_long_path_to`, `mark_unreachable`, `learn_wet_cells`,
`give_up_on_monster`, `push_walk_trace`), `state/events.rs` (the
`PlayerMovePath` trace line, `loot_given_up` on `GroundItemRemoved`),
`dungeon.rs` (`door_sides`), `driver/combat.rs` (`chase_monster`),
`driver/worker/fighter.rs` (`is_eligible`), `driver/worker/mod.rs`
(`report_failures`), `main.rs` (`AGENT_LOG_FILE`).

**Conflict resolution**: most of this works round server behaviour — the
body-sweep check `advance_plan` applies to a first leg its smoothing never
checked, the 2000-node surface budget, the 60 m goal cap. When master changes
any of those, re-check the matching workaround before keeping it; if master
fixes the cause, let master win and drop ours. `door_sides` is upstream code
with our fix on top.

**Verify**: `cargo test -p agent-client walk::` —
`a_descent_behind_a_closed_door_opens_it_instead_of_climbing_out`,
`dungeon_door_sides_are_standable_cell_centres`,
`a_long_leg_follows_the_route_not_the_straight_line`,
`a_dead_end_leg_is_routed_round_next_time`,
`a_dead_end_is_tried_again_once_it_lapses`,
`known_water_starts_over_rather_than_growing_without_bound`,
`a_failed_walk_describes_what_closes_our_cell`; and
`a_monster_no_chase_reached_is_passed_over_until_the_give_up_lapses`. Live: a
dungeoneer started in town walks to Skeleton Crypt and descends floor by
floor, opening doors, with no `[MoveFailed]` in the watch feed.

---

## 6. Camera rotation (web client and spectator)

**What**: the camera turns about the player at its isometric pitch, in the
web client and the spectator alike. Middle-button drag turns it (a click
without a drag resets); a trackpad's sideways two-finger swipe, or Shift+wheel
on a mouse, turns it while vertical scrolls and pinches still zoom; holding
`,` / `.` turns it and `/` resets. The debug panel's CAM ROT free orbit is
unchanged.

Upstream drew everything for a fixed south-west camera, so every
"what hides the player" decision is now read off one place,
`utils/view-direction.ts` (`viewRay`, `facesCamera`, `viewQuadrant`), which
`GameScene` updates from the camera each frame:

- `iso-occlusion.ts` is a general ray–box test (`viewRayRun`), used by house,
  tree and dungeon occlusion; the tree layer re-tests when `viewRevision`
  moves, not only when the player does.
- The bridge fade turns the view ray into deck-local space.
- A house's interior walls ghost along the current ray
  (`interiorWallOccludes`).
- House geometry is merged per wall side (`walls.north|south|east|west`) plus
  a `roof` group, instead of `front` (south+west+roof) and `back`; inside, the
  roof and whichever sides `facesCamera` hide, and the layer re-applies when
  that set changes (`facingKey`). Gable ends go with their side.
- Dungeon wall runs carry `fadeGroups` per view quadrant. Corridor corners
  are joined per quadrant: joined across all four sides, a corridor's walls
  chain round into one group.

**Lives in**: `client/src/lib/utils/view-direction.ts`,
`utils/camera-yaw-input.ts` (the input, `swipeYaw`, `rotateOffset`),
`utils/iso-occlusion.ts`, `utils/house-geometry.ts`, `utils/house-geo-utils.ts`
(`FloorEntries.walls/roof`, `interiorWallOccludes`), `utils/house-geo-walls.ts`,
`utils/house-geo-roof.ts` (`roofTarget` / `gableTarget`),
`utils/dungeon-geo-floor.ts` (`fadeGroups`), `managers/bridgeManager.ts`,
`components/GameScene.svelte` (`turnCamera`, the `OrbitControls` middle button
handed to the yaw input), `game-scene/GameScene{Housing,Dungeon,Tree}Layer.svelte`.

**Conflict resolution**: upstream adds occluders against the old fixed ray
(`px − s, py + s, pz + s`) — port any new one onto `viewRayRun` /
`viewRay`, or it will hide the wrong things once the camera turns. A change to
house geometry's grouping has to keep sides separate; merging back into
front/back breaks the interior cutaway for every view but the default.

**Verify**: `npx vitest run src/lib/utils/view-direction.test.ts
src/lib/utils/camera-yaw-input.test.ts src/lib/utils/dungeon-geo-floor.test.ts`
— the default yaw reproduces the old south-west test exactly, the input maps
drag, swipe and keys, and dungeon fade groups follow the quadrant. Live: turn
the camera beside a house, walk in, and check the walls toward the camera are
the ones hidden; do the same in a dungeon corridor.

---

## Superseded / intentionally dropped (do not re-add without checking master first)

- **Continuous walking (leg handover)** — superseded by master's
  server-approved goal movement: the client no longer sends `PlayerMove`
  steps, so there is no queue to chain onto.

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
