# tweak-agent-client custom features

`tweak-agent-client` is a personal customization branch on top of `master`.
`master` tracks the upstream/official repo and is rebased onto periodically —
each rebase can conflict with any of the features below. This file is the
checklist used by the `/rebase-tweak-agent-client` command (see
`.claude/commands/rebase-tweak-agent-client.md`) to confirm every
customization survived conflict resolution. Keep it in sync: when a
customization commit is added, split, renamed, or dropped (because master
grew an equivalent/better version), update its entry here in the same PR/session.

Each entry: what it does, where it lives, how a rebase conflict on it should
usually be resolved, and how to verify it survived (unit test and/or a live
signal to grep for).

---

## 1. Monster targeting by level tier, not just distance

**What**: world state lists monsters strongest-level-first (ties broken by
distance), capped at 10, instead of pure nearest-distance capped at 5.

**Lives in**: `format_world_state` in `src/state.rs` (`MAX_MONSTERS_IN_STATE`,
the `monster_level` closure, the sort). Base monster levels come from
`MonsterAiManager::level_for` (`src/monster_ai.rs`), fed by `monsters.json`
via `main.rs`/`orchestrator.rs`.

**Conflict resolution note**: master may change the monster listing/cap
independently (e.g. add a different filter). Keep the level-descending sort
as the primary key and re-apply whatever distance/filter logic master added
as the secondary key.

**Verify**: unit tests `world_state_lists_monsters_strongest_tier_first`,
`world_state_monster_sort_uses_level_override`,
`world_state_caps_the_monster_list` in `src/state.rs`. Live signal: pull the
watch panel's `llm-prompt` feed entries and confirm `Monster:` lines are
level-descending (cross-check against `data/monsters.json` levels) and
capped at 10.

---

## 2. Cross-floor / stairs movement fix

**What**: a coordinate move with an explicit `y` that differs from the NPC's
current height (e.g. "go upstairs to (x, 4, z)") fetches that area's house
data and resolves the correct floor via `get_floor_at_position`, instead of
silently staying on the current floor.

**Lives in**: the `Move` handler in `src/driver/execute.rs` (the `floor`
resolution block using `y`/`current_y`/`ensure_house_data_near`).

**Conflict resolution note**: master has separately grown cross-floor
dungeon-depth handling (`move_to_dungeon_floor`, height-based floor
detection) — these are complementary, not competing; both need to survive
together (see the `execute.rs` merge history from the master rebase).

**Verify**: no dedicated unit test currently. Live signal: send an NPC
toward a housing coordinate with a `y` that differs >1.5 from its current
height and confirm it changes `floor_level` instead of pathing through
walls; or trace `ensure_house_data_near` being called with a nonzero-height
target.

---

## 3. Walking stall-timeout

**What**: `walk_waypoints` gives up (`MoveResult::Blocked`) if a move makes
no distance progress for 6 seconds (`STALL_TIMEOUT`), instead of spinning the
driver loop forever on an unreachable waypoint.

**Lives in**: `src/driver/movement.rs` (`STALL_TIMEOUT`, `PROGRESS_EPSILON`,
the `last_progress_at`/`best_dist` tracking in `walk_waypoints`).

**Conflict resolution note**: master has grown its own stall-detection
variables in a differently-structured `walk_path`/`walk_waypoints` split —
when this conflicts, keep the timeout behavior, adapt it to whichever
function structure master settled on.

**Verify**: no dedicated unit test currently. Live signal: hard to trigger
naturally; note as "not live-verified this rebase" unless a genuinely stuck
waypoint is observed, and rely on the code being present (grep for
`STALL_TIMEOUT` in `movement.rs`).

---

## 4. Trade windows no longer hard-block walking away

**What**: an NPC mid-trade can `move` away to voluntarily end an unwanted
trade; only `Attack`/`Pickup`/`OpenChest`/`Sell`/`Buy`/`Buyback`/`BreakProp`
stay blocked during a trade. Fishing (`self_fishing`) is folded into
`holding_position` instead — like a scheduled rest, it's a real physical
constraint that blocks `Move` too, not a courtesy hold like `trade_busy`
(master's own `skip_movement` treats fishing/trade/scheduled-action as one
undifferentiated block-everything flag; this branch keeps splitting it).

**Lives in**: the `holding_position`/`trade_busy` split in
`src/driver/execute.rs::handle_response` (the `is_move`/`blocked` logic,
`handle_response`'s parameter comments) and the two call sites in
`src/driver/mod.rs::llm_driver` that compute
`(has_action || s.self_fishing, s.trade_busy)`; matching system-prompt
wording in `src/driver/prompt.rs`.

**Conflict resolution note**: don't let a rebase silently revert this back
to "trade_busy (or self_fishing) blocks everything including Move" — that's
master's simpler `skip_movement: bool` design, which this feature
intentionally splits. When master adds a new merchant/prop-interaction
action (as it did with `Buy`/`Buyback`/`BreakProp`) or a new hold-in-place
state (as it did with fishing on 2026-07-28), decide which bucket it
belongs in: a real physical constraint goes into `holding_position` (blocks
Move), a courtesy/social hold goes into `trade_busy` (Move exempted) — then
add it to the right side of both the `blocked` match arm and the call-site
tuple in `mod.rs`.

**Verify**: no dedicated unit test currently. Live signal: needs a live
trade partner; check for the `[ActionFailed]` wording mentioning "you can
still walk away" if triggered, otherwise verify statically that `is_move`
only checks `holding_position`, not `trade_busy`.

---

## 5. Blocked/failed actions report back to the LLM

**What**: previously-silent failures now push agent events so the LLM can
react: `[ActionFailed]` when an action is blocked by holding-position/trade;
`[CombatEnded]` when a *mid-combat* attack target becomes unreachable or the
face/attack `send_command` call itself errors (2026-07-26: found live —
Negan retried the same dead `send_command` 53 times in a row with zero
feedback, since that path only did `error!()` and returned `None` silently).

**2026-07-28 update**: the *initial-attack* half of this feature (the one
that used to push a generic `[CombatEnded] Could not reach monster ...`) is
gone — not regressed, absorbed. Master independently built the same idea
better: `ChaseResult::Lost(LostReason)` now tags *why* a chase gave up
(`TargetGone`/`PlayerDied`/`TooFar`/`Timeout`/`NoPath`), and
`execute.rs::unreachable_note` turns that into a specific `[Unreachable]`
message. This branch's rebase conflict resolution took master's version
wholesale rather than keep the old generic wording — the commit that used
to carry this (`Fix silent stall when an initial attack target is
unreachable`) now rebases to an empty diff and `git rebase` drops it
automatically. Don't try to re-add a separate initial-attack `[CombatEnded]`
push; `unreachable_note` already covers it, better.

**Lives in**: `src/driver/execute.rs` (`[ActionFailed]`; `unreachable_note`
and its call site are master's, not this branch's, but this branch's
`blocked` match arm sits right above them) and
`src/driver/combat.rs::tick_combat`'s three `[CombatEnded]` pushes:
lost/errored chase (still the generic `ChaseResult::Lost(_) | Error` form,
not destructured into `LostReason` — master's richer reasons never made it
into `tick_combat`, only the initial-attack path in `execute.rs`), and the
face/attack `send_command` failure branches.

**Conflict resolution note**: if a future rebase brings `LostReason` into
`tick_combat` too (master extending its own design further), that's a
genuine improvement — let it, and update this entry to match. Don't
resurrect the old generic wording as a "revert."

**Verify**: no dedicated unit test currently (behavioral, not pure-function).
Live signal: grep the watch panel's `llm-prompt` feed for `[CombatEnded]`,
`[ActionFailed]`, or `[Unreachable]` — reliably triggers within a couple
minutes of normal grinding (monsters routinely wander out of chase range).

---

## 6. NPCs keep thinking without a nearby human audience

**What**: removed the branch that skipped the initial prompt and drained
events when no human players were nearby; `always_active` NPCs act
independently of an audience.

**Lives in**: `src/driver/mod.rs::llm_driver` (the removed `else` branch
after the `always_active || has_nearby_human_players()` check).

**Conflict resolution note**: `always_active` config semantics (distinguishing
registry NPCs from player-run agents) have previously been a source of
rebase confusion — re-read `NpcConfig::always_active()`'s doc comment before
resolving any conflict here.

**Verify**: unit tests `only_registry_npcs_wait_for_an_audience` and related
in `src/main.rs`/`src/orchestrator.rs`. Live signal: hard to isolate (a human
happening to be nearby doesn't disprove it) — trust the unit test over a
live run for this one.

---

## 7. Combat counted as "activity" for prompt pacing

**What**: a combat tick (even a lost/errored chase) resets
`last_activity_at`, so the next LLM decision uses the short `min_interval`
instead of falling back to `idle_interval`.

**Lives in**: `src/driver/mod.rs::llm_driver`'s combat-tick block.

**Verify**: no dedicated unit test currently. Live signal: watch the log
during combat — `submitting Urgent prompt`/routine prompt cadence should
stay in the few-seconds-to-tens-of-seconds range, not jump to `idle_interval`
(default 3600s) right after a lost chase.

---

## 8. Ground-loot pickup prioritized as Urgent

**What**: `GroundItemSpawned`/`GroundItemAppeared` classified `Urgent`
(was `Noise`), waking the driver immediately when loot drops; system prompt
instructs looting before continuing to fight. The junk-sell trigger is the
real carry-weight limit, not a fixed bag-count rule — sell junk once weight
gets tight (a pickup failing with "Too heavy to carry" is the signal), not
at some arbitrary item count.

**Lives in**: `classify_event` in `src/state.rs` (must fall through to the
normal `push_event` path below the dedup match arm, not the early-return
one — see the comment there about why `GroundItemRemoved` still
short-circuits but `Spawned`/`Appeared` must not); `data/system_prompt.txt`.

**Conflict resolution note**: this is the one place a "correct-looking"
merge silently broke behavior before — reclassifying to Urgent is a no-op if
the early-return in `push_event` still intercepts the message before the
`urgent_notify.notify_one()` call. Always check that path together with the
classification change.

**Verify**: unit test coverage is indirect (existing ground-item tests don't
assert urgency/notify). Live signal: watch feed should show `[ItemDropped]`
followed by the LLM's own reasoning choosing to pick it up before continuing
combat, and `Agent picked up ... [id ...]` in the log within one prompt turn.

---

## 9. Landmark/vendor/junk-item knowledge lives in Negan's instance prompt

**What**: landmark coordinates (Aldermark Village, monster cluster),
Rica's morning-only trading hours, and the known junk-item list are NOT in
`data/system_prompt.txt` (that file is generic action-schema mechanics
shared by every NPC). They live in `data/npcs/negan/instance.txt` under
"Negan's preference", layered on last via `instance_prompt` in
`config.toml` — this is one character's personal world knowledge, not
something every agent needs.

**Conflict resolution note**: `data/system_prompt.txt` should stay free of
named NPCs/coordinates — if a rebase reintroduces world-specific facts
there (e.g. master adding its own landmark section), move them out to the
relevant NPC's `instance.txt` rather than merging them in place. Master's
own examples in the sell/buy/buyback docs use "Rica" as a placeholder name
the same way other actions use "PlayerName" — that's not a real fact and
doesn't need scrubbing.

**Verify**: no live signal (system prompt isn't sent to the watch panel, and
`instance.txt` files aren't tracked by the checklist's usual live-run).
Read `data/system_prompt.txt` directly after the rebase and confirm it has
no named NPCs or coordinates, and read `data/npcs/negan/instance.txt` to
confirm the landmark/junk-item/target-tier content is still there.

---

## Superseded / intentionally dropped (do not re-add without checking master first)

- **`fn api_base_url(server_url: &str)` in `orchestrator.rs`** (2026-07-28) —
  this branch's original WS→REST URL deriver (bump the port by one), used to
  compute `spawn_llm_task`'s `api_base_url` locally from `server_url`.
  Superseded by master's `derive_api_base_url` in `main.rs`, which also
  accounts for the terrain URL/reverse-proxy case and is computed once in
  `main` then threaded down as a parameter — `spawn_llm_task` already took
  `api_base_url: &str` as an argument by the time this conflict landed, so
  the local recomputation was leftover dead code. Removed the function and
  its three unit tests; if a future rebase conflict reintroduces it, check
  whether `derive_api_base_url` (main.rs) already covers the need before
  bringing it back.
- **`data/user_prompt.txt` as an implicit fallback role file** (2026-07-26) —
  intentionally dropped, not master-superseded. It was a stale duplicate of
  `data/user_prompts/veteran.txt` (missing that file's Dungeon Chests
  section) once the target-tier-by-level paragraph moved out to
  `data/npcs/negan/instance.txt`. Negan's `config.toml` now sets
  `template_prompt = "data/user_prompts/veteran.txt"` explicitly instead of
  relying on the implicit `data/user_prompt.txt` convention documented in
  `config.toml.example`. Don't recreate the file; if a future ad-hoc NPC
  needs the veteran role, point its `template_prompt` at `veteran.txt`
  (or a new named file) directly.
- **OpenAI-compatible LLM backend draft** (old `openai.rs`, `OPENAI_COMPAT_API_KEY`
  env var naming) — superseded by master's merged version (env var renamed to
  `OPENAI_API_KEY`, refined serde defaults). If a future rebase conflicts here
  again, master's version wins unless it's regressed since.
- **`Pickup{instance_id: u64}` action design** — superseded by master's
  `Pickup{item: PickupRef}` (name-or-id resolution via `resolve_ground_item`).
  Keep master's design.
- **`Sell{player, item}` action design** (2026-07-26) — superseded by
  master's full economy system: `Sell{item, merchant}`, plus `Buy`,
  `Buyback`, and `BreakProp`, none of which this branch had. Master's
  version is a strict superset (proper merchant resolution, wallet/catalog
  checks, buyback ledger) — keep master's design entirely, including its
  `AgentAction::Sell` field names (`item`/`merchant`, not `player`/`item`).
  If a future rebase conflict looks like our old `player`/`item` Sell
  fighting master's `item`/`merchant` one, master wins; delete ours.
- **`Drop{item}` action design allowing worn gear to be dropped directly**
  (2026-07-26) — superseded by master's own `Drop` action, which is
  stricter (worn gear must `use`-unequip first) but otherwise the same
  shape (`item: String`, same aliases). Master's design wins on conflict.
  **Important**: master's `Drop` handler in `execute.rs` does NOT include
  our anti-loop safeguard (`SharedState::mark_self_dropped` /
  `self_dropped_items`, see `src/state.rs`) — without it, a drop echoes back
  as `GroundItemSpawned` and immediately re-surfaces as loot next prompt,
  looping drop/pickup forever. This call was manually re-added to master's
  `Drop` handler once already (2026-07-26 rebase) — if a future rebase
  conflict drops it again, re-add `s.mark_self_dropped(instance_id);` right
  after master's `send_command` succeeds. Proven by unit test
  `self_dropped_items_never_resurface_as_loot` in `src/state.rs`, which must
  keep passing regardless of which side's `Drop` handler wins a merge.
