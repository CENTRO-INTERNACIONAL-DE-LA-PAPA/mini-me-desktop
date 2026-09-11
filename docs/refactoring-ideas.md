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

**Status:** done (2026-09-09)

`theory_tools.py`, `datavoyager_tools.py`, and `autodiscovery_tools.py` each independently
reimplemented the same shape. Created `mini-me/backend/asta_jobs.py` holding what turned out to
be genuinely shared: `_run` (the §224 dict-response fix, identical in all three), `_UUID_RE` +
`is_valid_task_id` (identical in theorizer/DataVoyager), `_state_of` (identical in
theorizer/DataVoyager), and `_extract_json`. Each of the three files now imports these instead of
keeping its own copy.

**Deliberately left separate** (forcing these together would have been dishonest to what each
actually does): each file's own CLI command-building, markdown rendering, and
`persist_*_outputs`; `autodiscovery_tools.py`'s `is_valid_run_id` (different validation strictness
than `is_valid_task_id`, not the same check); `autodiscovery_tools.py`'s `normalise_status` (maps
a completely different, uppercase REST-job vocabulary, not an A2A task's `status.state`);
`theory_tools.py`'s own `_extract_json` (a genuinely simpler, different algorithm — a theorizer
record is reduced in-sandbox before it reaches this parser).

**A real regression was caught and fixed during review, not by the test suite.** The first pass
took `autodiscovery_tools.py`'s `_extract_json` verbatim as "the shared one" on the assumption
(from earlier research this session) that it was byte-identical to `datavoyager_tools.py`'s. It
wasn't: `datavoyager_tools.py`'s version additionally split off a `[stderr]` suffix before
parsing, and losing that meant a record could fail to parse whenever the command also wrote to
stderr. No existing test exercised that combination, so `572 passed` looked clean while shipping a
real behavior loss — caught by manually diffing old-vs-new output on a constructed stderr-suffixed
input, not by CI. Fixed by writing `_extract_json` as the union of what both originals actually
tried (tries a `[stderr]`-split head before the full text, each as whole-string JSON before
falling back to `{...}`/`[...]` bracket-matching), so it cannot parse less than either original
did. Added a regression test for this exact case, plus a new `mini-me/tests/test_asta_jobs.py`
giving the shared module its own direct test coverage instead of relying only on indirect exercise
through whichever of the three callers happens to hit a given path.

- `mini-me/backend/asta_jobs.py` (new), `mini-me/backend/theory_tools.py`,
  `mini-me/backend/datavoyager_tools.py`, `mini-me/backend/autodiscovery_tools.py`
- `mini-me/tests/test_asta_jobs.py` (new), `mini-me/tests/test_datavoyager_tools.py` (added the
  stderr-suffix regression test)
- Public/imported names preserved exactly via re-export (e.g. `theory_tools.py` does
  `from backend.asta_jobs import _UUID_RE, _run, _state_of, is_valid_task_id`), so no other
  module's imports or existing tests needed to change.
- Verified: 572 passed, 1 skipped, 6 failed — the exact same pre-existing failure set as items 1
  and 4 (confirmed identical, not just similar-looking). Checked the Rust side for any source-
  slicing tests reading these three files' exact structure — found none, only doc-comment
  mentions, unaffected.
- This is also the natural place to eventually introduce the Asta-side adapter into `JobState`
  (item 0) — `_state_of`/each file's own `_progress_of` are exactly the code that would translate
  Asta's own vocabulary (`"completed"/"failed"/"canceled"/"running"`, inconsistently also
  `"error"`) into the shared enum, when that work happens.

## 3. Consolidate the three Asta poll-status HTTP routes

**Status:** done (2026-09-11)

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

Added `_poll_asta_job(request, *, id_param, validate_id, poll, persist)` to
`routes/artifacts.py`, holding the one shape all three actually shared (validate id → resolve
sandbox → bind Asta token → poll → best-effort persist on a terminal state → respond).
`theorizer_status`/`analyze_data_status`/`discovery_status` are now thin closures over it —
each still owns its own id validator (`is_valid_task_id` vs `is_valid_run_id`, still genuinely
different per item 2), its own poll/persist function signatures (analyze-data's extra
`context_id`, discovery's extra metadata read before persisting), and its own docstring/route
registration. Nothing about *why* each is a job was unified — only the orchestration around
"poll it, then maybe persist it" that was byte-for-byte identical three times.

- `mini-me/backend/routes/artifacts.py` (`_poll_asta_job`, `theorizer_status`,
  `analyze_data_status`, `discovery_status`)
- Verified: `uv run pytest tests/` — 546 passed, 1 skipped, same 2 pre-existing failures as
  items 1/2/7 (deselected, not silently ignored).
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

**Status:** done for `watch_job`/`watch_task` (2026-09-11); Draft/DraftCost deliberately left as-is
(see below) — third concrete step toward item 0 (was "merge SSE + job-poll"; narrowed after
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

**Landed:** added a `Watched` trait (`poll` + `is_finished`, an associated `Context` for whatever
extra a source needs beyond its own fields) and one generic `watch<T: Watched>(runtime, base_url,
interval, initial, context, on_change)` function to `sidecar.rs`. `Job`'s `Context` is the shared
`ThreadId` mutex it already needed to re-read each tick ("New thread" can move it mid-watch);
`AsyncTask`'s `Context` is a small `TaskWatchLog` holding the two one-shot logging flags the
original loop had (`reported`/`complained` — both tied to a documented support issue, §207) so that
diagnostic behavior wasn't lost in the unification, just relocated onto the type that actually
needs it. `watch_job`/`watch_task` are now each a single call into `watch()`.

**Small, disclosed trim, not hidden:** the original `watch_task` loop also logged a
per-state-change `tracing::info!` including the raw `ThreadState.next` field. `on_change` only
sees the resulting `AsyncTask` (which has no `next` field), so that one field is gone from the
"state changed" log line; the `reported`/`complained` diagnostics (the ones actually tied to a
past bug) are unchanged.

- `crates/app/src/sidecar.rs` (`Watched`, `watch`, `TaskWatchLog`, `impl Watched for Job`,
  `impl Watched for AsyncTask`, `watch_job`, `watch_task`)
- Verified: `cargo check -p mini-me-desktop-app` clean; `cargo test -p mini-me-desktop-app` — 493
  passed, 5 failed, and the same 5 fail identically with this change stashed out (confirmed by
  re-running them against unmodified code) — all pre-existing and unrelated: 4 are
  display/contrast-calibration assertions in `theme.rs` (environment-dependent, nothing to do with
  polling), 1 (`backend::tests::a_failed_preparation_never_starts_a_server`) requires a working WSL
  `uv sync` in this environment.

## 7. Custom-route boilerplate: auth+identity guard, vault-error shaping, JSON-body parsing

**Status:** done (2026-09-11)

A fresh audit (outside the job/turn-unification track above) found three small, genuinely
identical patterns repeated across `mini-me/backend/routes/*.py`, none overlapping items 0-6:

- **Auth + identity guard** — `if (unauth := _require_auth(request)) is not None: return unauth`
  then `user_id = _request_user_id(request); if not user_id: return 401` was inlined 7 times in
  `config.py`, 2 times in `project.py`, and once more as a locally-defined `_auth_user` helper in
  `projects.py` (so even the "already extracted" version was its own private copy, not shared).
- **Vault-call error shaping** — every one of `config.py`'s 7 vault-touching handlers repeated the
  identical `except vault_store.VaultUnavailable → 503` / `except Exception → 502` pair, differing
  only in the one word naming the action (`"read"`/`"write"`/`"delete"`).
- **JSON-body parsing** — `try: body = await request.json() / except: 400 invalid JSON body`
  appeared 8 times across `artifacts.py`, `config.py`, `project.py`, `projects.py`, `rendering.py`.

Added `_require_user`, `_parse_json_body`, and (moved up from `projects.py`/`project.py`, which had
each grown their own copy) `_get_store_or_error` to `routes/common.py`; added a small
`_vault_call(action, awaitable)` wrapper local to `config.py` (vault is only ever touched there).
All handlers in `config.py`, `project.py`, `projects.py`, `rendering.py` now use these instead of
repeating the pattern inline.

**Deliberately left alone**: `artifacts.py::discovery_submit`'s JSON-parse — it folds a parse
failure and a non-dict body into the *same* refusal with its own message (documented at §252: "an
unreadable body is a refusal, not an approval"), which is a real, deliberate behavioral difference
from the other 7 sites, not the same thing wearing different words.

- `mini-me/backend/routes/common.py` (new `_require_user`, `_parse_json_body`,
  `_get_store_or_error`), `routes/config.py`, `routes/project.py`, `routes/projects.py`,
  `routes/rendering.py`
- Verified: `uv run pytest tests/` — 546 passed, 1 skipped, 2 failed, both pre-existing and
  unrelated (confirmed against items 1/2's already-documented failure set: the WSL-vs-native-
  Windows real-subprocess-shell mismatch in `test_autodiscovery_tools.py`, and the Windows-path-
  separator bug in `test_pdf_fetch.py::test_dest_path_uses_title_then_ref`).

---

## Flagged, not actioned: `road_strip` in the sidebar

Not added as a numbered item — this is a live in-progress feature marker, not settled debloat, and
needs a call from whoever owns that plan rather than being removed on a debloat pass:

`crates/app/src/ui/sidebar.rs:843-1074` (~230 lines, `road_strip` and helpers) has been commented
out wholesale behind `// ToDo: [!!] ROAD COMMENTED FOR NOW UNTIL IMPLEMENTED IN CHAT [!!]` since
`f064ef2` (2026-09-02). Its only caller in `main.rs` is also commented out. But the supporting state
is still fully wired and doing nothing: `road_open: bool` is a live persisted `Settings` field
(`settings.rs:176,217,791,816`), and `toggle_road()`/`remember_panels()` still read/write it
(`main.rs:6105-6128`) despite `toggle_road`'s only caller also being commented out. Real dead-ish
scope, but "paused, marked for later" rather than "abandoned" — confirm before treating it as debloat.

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
