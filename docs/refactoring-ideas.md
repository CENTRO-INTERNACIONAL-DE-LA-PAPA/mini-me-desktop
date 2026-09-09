# Refactoring & debloat ideas

A running list of concrete opportunities to reduce duplication, remove leftover scope from
earlier design directions, and simplify systems that have outgrown their original shape. Items
land here only after review — this is not a backlog of every idea raised, just the ones judged
worth doing.

Status values: `open` (not started), `done` (landed, with commit/date).

---

## 0. One shared answer contract: reply now, or defer with a watchable handle

**Status:** open — anchor goal, not a single scoped task

**The complaint this answers:** Sending a message to the backend
today can resolve in at least four structurally different ways depending on what it touches — a
plain streamed turn, an Asta CLI job (3 separate implementations: theorizer/DataVoyager/
AutoDiscovery), a background subagent (its own new LangGraph thread, its own poll loop), or a
Draft awaiting approval (its own bespoke status string). Adding a fifth kind of "thing that might
take a while" today means inventing a new poll route, a new status vocabulary, and new Rust
polling code from scratch — which is exactly the pattern that already produced three near-copies.

**The target shape, from the caller's side, regardless of what's underneath:**

```
Envelope = Answered(result)
         | Deferred(watch_handle)   # "check back on this" — however that source needs to be checked
```

**What this does NOT mean** — deliberately, so this doesn't turn into forcing incompatible things
into one box (already checked this session: Asta's job vocabulary and LangGraph's native run
vocabulary say opposite words for the same two states — `completed`/`failed` vs `success`/`error`
— because this codebase doesn't own either vocabulary and can't rename them at the source):
- **Not** one internal type unifying Asta jobs, LangGraph background runs, and Drafts. Each keeps
  its own internal representation and its own way of actually doing the work.
- **Not** folding regular chat turns into the same watch mechanism as background jobs. Turns are
  interactive/token-level and only exist while something is actively watching; jobs are
  fire-and-forget and have to survive the app restarting. That difference is load-bearing, not an
  oversight, and forcing it away would be wrong.

**What it DOES mean, concretely:**
- **Backend:** one shared entry helper any tool/route can call — run the work; if it resolves
  quickly (or is inherently synchronous), hand back the result; if it's going to be a job, hand
  back a `watch_handle` instead. Built on small per-source adapters that translate each source's
  native state vocabulary into one shared `JobState` enum used everywhere *above* the adapter —
  the vocabulary differences get absorbed once, at the boundary, instead of leaking into every
  caller (today: `"failed"` vs `"error"`, `"canceled"` vs `"cancelled"`, checked inconsistently in
  at least 5 places).
- **Frontend:** one handling path for "I sent something" — got an answer, done; got a deferred
  handle, hand it to one generic Rust polling helper (`watch<T>(interval, poll_fn, is_finished,
  on_change)`) instead of the three independently hand-rolled poll loops that exist today
  (`sidecar.rs::watch_job` for Asta jobs, `sidecar.rs::watch_task` for background subagents, and
  Draft's own implicit one) — mechanically identical already, just copy-pasted three times with
  three different terminal-state spellings.

**Net effect:** adding a new "might take a while" capability later becomes "write the adapter,
call the shared entry point" instead of "invent a new route, a new vocabulary, and new polling
code." Items 2, 3, 5, and 6 below are concrete steps toward this; none of them require doing all
of this at once.

---

## 1. Fix `theory_tools.py`'s missing dict-response handling

**Status:** done (2026-09-09)

`theory_tools.py`'s `_run` was missing the fix that `datavoyager_tools.py` and
`autodiscovery_tools.py` already had (their comments cite "§224"): a dict-shaped sandbox response
read only for attributes yielded an empty string, indistinguishable from a command that printed
nothing. Ported the same `isinstance(resp, dict)` handling into `theory_tools.py::_run`, and added
a regression test (`test_run_reads_a_dict_shaped_response_too` in `tests/test_theory_tools.py`)
mirroring `test_datavoyager_tools.py::test_a_dict_shaped_response_is_read_too`.

- `mini-me/backend/theory_tools.py` (`_run`)
- `mini-me/tests/test_theory_tools.py`
- Full backend suite run after the change: 561 passed, 1 skipped, 6 failed — all 6 failures
  confirmed pre-existing and unrelated (verified by re-running the same failing tests with this
  change stashed out): 4 are the known WSL-vs-native-Windows real-subprocess-shell mismatches in
  `test_ledger.py`, 1 (`test_autodiscovery_tools.py::test_a_real_sized_response_survives_the_shell`)
  is the same category, and 1 (`test_pdf_fetch.py::test_dest_path_uses_title_then_ref`) is an
  unrelated Windows-path-separator test bug (`\` vs `/`).

## 2. Consolidate the three Asta job-lifecycle files

**Status:** open — first concrete step toward item 0

`theory_tools.py`, `datavoyager_tools.py`, and `autodiscovery_tools.py` each independently
reimplement the same shape: `_extract_json` (parse merged stdout/stderr into a JSON record),
`_run` (prefer `aexecute_untruncated`, handle both dict- and object-shaped sandbox responses),
`_state_of`/`_progress_of`/`_failure_reason`, id validation, a markdown renderer, and a
`persist_*_outputs` function. `_extract_json` is byte-for-byte identical between at least two of
them. Merge the shared shape into one base module; keep only what's genuinely
job-type-specific (the Asta CLI invocation itself, the markdown template, the persisted output
shape) in each file.

This is also the natural place to introduce the Asta-side adapter into `JobState` (item 0) — the
three files' `_state_of`/`_progress_of` functions are exactly the code that would translate Asta's
own vocabulary (`"completed"/"failed"/"canceled"/"running"`, inconsistently also `"error"`) into
the shared enum.

- `mini-me/backend/theory_tools.py`, `mini-me/backend/datavoyager_tools.py`,
  `mini-me/backend/autodiscovery_tools.py`
- Do item 1 first (or as part of this), so the merge doesn't propagate the bug into the shared code.

## 3. Consolidate the three Asta poll-status HTTP routes

**Status:** open — second concrete step toward item 0

`theorizer_status`, `analyze_data_status`, and `discovery_status` in
`mini-me/backend/routes/artifacts.py` are near-identical: validate id → look up existing sandbox →
bind the Asta token → call the matching `poll_*_status` → on terminal state, best-effort
`persist_*_outputs` → return JSON.

Important constraint, confirmed this session: these routes explicitly run **outside any LangGraph
turn** — a job can take 5-40+ minutes and is checked on later by an unrelated request, sometimes
after the app was closed and relaunched. That rules out moving this onto the SSE event stream (the
stream doesn't outlive the run, and the backend process doesn't outlive the app window). The
right shape is a shared `poll_asta_job(kind, sandbox, id, persist_fn, markdown_fn)` helper feeding
one generic route, not a stream-based collapse — this is the "how you check back" half of the
`Deferred(watch_handle)` shape in item 0.

- `mini-me/backend/routes/artifacts.py` (~lines 191-291, 477+)
- Considered and declined: routing job status through `get_stream_writer()`/SSE, the way
  `sandbox_status` works. Doesn't apply here — `sandbox_status` fires during an active, still-open
  run (seconds); these jobs are designed to let the run end immediately and get checked on much
  later, including after a full app restart. A WebSocket push was also considered for the same
  reason and declined: the backend process is killed when the app window closes
  (`crates/app/src/backend.rs:1315`, `Drop for BackendSupervisor`), so any open socket dies at the
  same moment polling would stop — it would only reduce latency/overhead while the app is
  continuously open, at the cost of a new Rust dependency (no WebSocket client exists today) and a
  second parallel connection type alongside the existing SSE stream and job-watcher. Not worth it
  for a same-machine, localhost-only connection where polling overhead is already negligible.

## 4. Remove the `vendor/Mini-Me` packaging fallback

**Status:** done (2026-09-09)

Same shape as the `overlay/` cleanup already done this session, but in the build/release pipeline
instead of runtime: this repo moved to a `mini-me/` monorepo layout, but packaging still accepted
the older `vendor/Mini-Me` layout (a clone of a separate private repo). Since `mini-me/` is now
always the layout in practice, this fallback was dead.

Deleted `scripts/bundle-backend.sh` entirely; removed the `vendor/Mini-Me` branches from
`package.sh` and `release.sh` (including the `vendor/BUNDLED.txt` pin-reading, which nothing
populates anymore, and the now-redundant "Backend pinned at" release-notes line — the backend is
the same commit as everything else in this monorepo); removed the fallback from
`bundled_backend_dir()` in `backend.rs`; updated the stale doc comments in `backend.rs` and
`setup-wsl.sh`; rewrote `README.md`'s "For whoever prepares the build" section, which was actively
instructing a new contributor to do the obsolete two-repo step.

**Explicitly NOT touched, a different thing entirely**: the empty `vendor/` marker folder
`package.sh` still creates in the packaged output, and `"vendor"` in `update::BUNDLE_BACKENDS`.
That's a compatibility shim so installs older than v0.3.15 (which require a folder literally named
`vendor` beside the executable) still accept new downloads — unrelated to the dead source-checkout
fallback, and still load-bearing for anyone on an old install. Left alone; a separate judgment call
about how many users might still be that old, not part of this item.

Verified: `cargo test -p mini-me-desktop-app` — 492 passed, 0 failed, including the release-check
self-tests that read `package.sh`/`release.sh` content directly. `bash -n` clean on all three
edited scripts.

## 5. AutoDiscovery's approval pipeline

**Status:** open — largest beneficiary of item 0's `JobState`, but not blocked on it

Approving a spend before an Asta run currently requires: a one-shot approval token (checked on
arrival, spent at point of use, separate from a `_merge_gate`-style shell-command blocklist),
per-run staging files written and deleted via raw shell `printf`+`rm` (deepagents' virtual `awrite`
is create-only and kept colliding with itself), and **three independently-updated sources of truth**
for one run's status — an artifact written at draft time, `snapshot.jobs`/`.tasks`/`.drafts`, and
the live Asta service.

This isn't a guess at overengineering — it's the project's own author diagnosing the same failure
mode four times in a row in `docs/desktop-app-plan.md` §254-§262: *"a correct component, complete
tests, and no writer for the thing downstream of it… four in a row."* A design with three caches
that each need updating in lockstep will keep producing this bug class regardless of how carefully
each instance is patched. Simpler shape: one owned state machine per discovery run, read everywhere,
instead of three independently-maintained snapshots of the same thing — i.e. this is where
`JobState` (item 0) would replace the three caches, once it exists.

- Read `docs/desktop-app-plan.md` §252-§264 in full before touching this — the false starts are
  already documented there and shouldn't be repeated.
- Relevant code: the AutoDiscovery approval/credit-gating path (`middleware/no_spending.py`,
  `autodiscovery_tools.py`, and whatever currently owns `snapshot.jobs`/`.tasks`/`.drafts`).
- This is the largest, riskiest item on this list — needs its own design pass, not a quick patch.

## 6. One generic Rust polling helper, replacing three hand-rolled loops

**Status:** open — third concrete step toward item 0 (was "merge SSE + job-poll"; narrowed after
confirming turns genuinely don't fit the same shape as jobs)

Three independently hand-rolled poll loops exist for the identical mechanical pattern (sleep →
poll → compare-to-last → send-on-change → return-on-terminal), each with its own terminal-state
spelling:
- `sidecar.rs::watch_job` — Asta CLI jobs — `"completed"/"failed"/"canceled"/"cancelled"/
  "unavailable"/"error"`
- `sidecar.rs::watch_task` — background subagents, polling a LangGraph thread's own native status
  — `"success"/"error"/"timeout"/"cancelled"/"canceled"`
- `Draft`/`DraftCost`'s own implicit status handling — a fourth bespoke vocabulary, deliberately
  *not* modeled as a job (good reasoning already in the code: "a job is something to wait on; this
  is something to answer") — this one should keep its own shape, but could still normalize into
  `JobState` for display purposes.

Replace the three loops' mechanics with one generic `watch<T>(interval, poll_fn, is_finished,
on_change)` helper. This is the client-side half of item 0's `Deferred(watch_handle)` — callers
watch a handle without needing to know whether it's an Asta job or a background subagent
underneath.

- `crates/app/src/sidecar.rs` (`watch_job`, `watch_task`), `crates/app/src/protocol.rs` (`poll_job`,
  `Job`, `AsyncTask`, `Draft`/`DraftCost`)
- Confirmed this session: regular chat turns (SSE) do NOT belong in this abstraction — they're
  interactive/token-level and only exist while something is actively watching, unlike jobs, which
  are fire-and-forget and must survive the app restarting. Don't fold turns in here.

---

## Declined (considered, not doing)

- **WebSocket push instead of polling for Asta jobs.** The backend process is killed when the app
  window closes (`crates/app/src/backend.rs:1315`), so an open socket dies at the same moment
  polling would stop — it only helps latency while the app is continuously open, at the cost of a
  new Rust dependency and a second parallel connection type. Not worth it for a same-machine,
  localhost-only connection.
- **Splitting `main.rs`/`protocol.rs` by line count.** Redirected toward feature-level
  simplification instead (items 0, 5, 6 above) — file size on its own isn't the problem worth
  solving right now.
- **One literal shared type for Asta jobs, LangGraph background runs, and Drafts.** Considered as
  part of item 0 and narrowed instead to "one shared enum via per-source adapters" — the underlying
  vocabularies (Asta's, LangGraph's) aren't ours to rename, and forcing them into one representation
  at the source would mean lying about what the backend actually said.
