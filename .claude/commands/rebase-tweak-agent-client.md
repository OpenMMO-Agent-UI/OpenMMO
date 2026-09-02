---
description: "Rebase the personal customization branch tweak-agent-client onto the latest master, resolve conflicts against the checklist in agent-client/CUSTOM_FEATURES.md, then verify every customized feature survived (tests + a bounded live run against the game server). Use when the user asks to rebase, sync, or update tweak-agent-client onto master, or to re-verify its customizations after such a rebase. Rewrites history — never push without asking first."
---

`tweak-agent-client` is a personal customization branch; `master` tracks the
official upstream repo and gets rebased onto repeatedly. Each rebase can
silently regress a customization during conflict resolution — that has
already happened once (an event-urgency reclassification was overwritten by
an early-return path that skipped the wakeup notification; a `Sell` action
was almost dropped along with a superseded `Pickup` design it shipped
alongside). This command exists to make that class of mistake structurally
harder to repeat.

The living checklist is `agent-client/CUSTOM_FEATURES.md` — read it in full
before resolving any conflict. It is not optional context; it is the
definition of "still valid" for this task. If it and this file ever disagree
on procedure, the checklist's per-feature "Conflict resolution note" wins.

## 1. Preflight

```bash
cd /Users/tony.pai/OpenMMO
git status --porcelain        # must be clean — stash or ask before proceeding otherwise
git branch --show-current     # must be tweak-agent-client; switch if not
git fetch origin master tweak-agent-client
git log --oneline master..tweak-agent-client   # the commits about to be replayed
```

Tag the current tip before touching anything, so a bad rebase is always one
`git reset --hard <tag>` away from undone (only do that reset if explicitly
asked — this is a safety net, not something to reach for unprompted):

```bash
git tag backup/tweak-agent-client-pre-rebase-$(date +%Y%m%d) tweak-agent-client
```

## 2. Rebase

```bash
git rebase master
```

Unlike the one-time history cleanup done when this branch was first set up
(which deliberately dropped superseded commits), a routine sync rebase
should replay every customization commit as-is — don't drop a commit here
just because part of it looks superseded. If a commit turns out to bundle a
genuinely-superseded piece alongside something still unique (like the `Sell`
action shipping inside the same commit as the old `Pickup{instance_id}`
design), read `CUSTOM_FEATURES.md` §"Superseded / intentionally dropped"
first — that section exists precisely to keep track of which half of a
mixed commit is safe to lose.

### Resolving conflicts

For every conflict, before picking a side:

1. Identify which numbered feature(s) in `CUSTOM_FEATURES.md` the conflicting
   hunk belongs to (the file list in each entry's "Lives in" narrows this
   down fast).
2. Read that feature's "Conflict resolution note" — several encode a
   specific past mistake (e.g. the ground-loot feature's note about the
   `push_event` early-return silently making an urgency reclassification
   inert). Feature numbers shift whenever one is added/retired, so match by
   name/content, not by number alone.
3. Prefer a structural merge over picking a side wholesale: adopt master's
   internal refactors/renames where they don't change behavior, but keep the
   customized user-facing behavior. When master's side is a strict
   superset/improvement of the customization (as happened with the OpenAI
   backend and the `Pickup` design — see the "Superseded" section), let
   master's version win and note it.
4. If a conflict doesn't map to anything in the checklist, it's either a new
   kind of overlap (add an entry to `CUSTOM_FEATURES.md` once resolved) or
   unrelated upstream work — resolve it on its own merits.

After the rebase completes, if any customization commit's diff changed
shape enough that `CUSTOM_FEATURES.md`'s file/function pointers are stale,
update them in the same pass — don't leave the checklist pointing at code
that moved.

## 3. Build/lint/test gate

```bash
cd agent-client
cargo fmt -- --check
cargo clippy --no-deps
cargo build
cargo test
```

All four must be clean. Fix and re-run rather than reporting partial
results — a fmt/clippy warning left in place tends to get silently
re-introduced at the next rebase too.

## 4. Feature-by-feature verification

Walk `CUSTOM_FEATURES.md` top to bottom. Each entry names its own "Verify"
method:

- **Unit-test-backed features**: already covered by step 3's `cargo test`
  passing — just note which test names correspond to which feature in the
  report, don't re-run them separately.
- **Live-signal features**: need a bounded run against the real game server.
  Batch all of them into one live session rather than one run per feature:

  ```bash
  cd agent-client
  cargo build   # if not already built this session
  nohup ../target/debug/agent-client > /tmp/tweak-verify.log 2>&1 &
  echo "PID=$!" | tee /tmp/tweak-verify.pid
  ```

  (`timeout`/`gtimeout` are not available on this machine — track the PID
  and kill it manually, don't rely on a wrapper.) Let it run a few minutes —
  ground-loot and `[CombatEnded]` events show up within the first 1-2
  minutes of normal grinding; movement/dungeon/trade-dependent features may
  not trigger naturally in any given run, and that's fine to report as
  "not triggered this run" rather than forcing a scenario.

  Pull the raw prompt/response feed to check for the markers each checklist
  entry lists (e.g. `[CombatEnded]`, `[ItemDropped]`, `Monster:` line
  ordering, the `Prompt layers: ... + data/user_prompt.txt` startup log
  line):

  ```bash
  curl -s "http://127.0.0.1:8808/api/state?since=0" | python3 -c "
  import json,sys
  d = json.load(sys.stdin)
  for e in d['feed']:
      print(e.get('k'), '|', e['m'][:200].replace(chr(10),' / '))
  "
  ```

  When done, stop the process cleanly and remove the temp files:

  ```bash
  PID=$(sed 's/PID=//' /tmp/tweak-verify.pid)
  kill "$PID"; sleep 1; kill -0 "$PID" 2>/dev/null && kill -9 "$PID"
  rm -f /tmp/tweak-verify.log /tmp/tweak-verify.pid
  ```

This connects to the live production server and controls a real character
on real API budget — it's the same tradeoff every prior live-verification
pass in this project has made, but don't run it if the user only asked for
the rebase itself, not verification.

## 5. Report

A table: feature # → name → status (✅ confirmed / ⚠️ not triggered this run,
relying on static check / ❌ regressed — fix before reporting done) → how it
was confirmed (test name or log/feed marker). Call out explicitly anything
that had to be re-fixed during conflict resolution, and anything added to or
removed from `CUSTOM_FEATURES.md` this pass.

Do **not** push. The rebase rewrites `tweak-agent-client`'s history, so
updating `origin/tweak-agent-client` needs a force-push — surface that it's
needed and wait for explicit confirmation before running it.

$ARGUMENTS
