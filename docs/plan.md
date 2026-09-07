# Working plan — adopt Luciano's branches, recover stray files, quiet the UI

Step-by-step for the work Piero asked for on 2026-09-01, kept beside
`docs/desktop-app-plan.md` (the long-form record — §300 is its last entry) rather than
inside it, because this is a checklist that gets ticked and eventually deleted, not a
decision worth reading in two years.

**Read `docs/handover.md` first** if you are new to this repo: it carries the rules, the
failures worth not repeating, and the same open work with more context around each item.

Three jobs. Order set by Piero on 2026-09-01: **`feat/ui-update` first, and nothing
from `feat/wsl2less` until he has spoken to Luciano.**

1. **Adopt `feat/ui-update`** — the split of `main.rs`. ✅ **DONE** — `d51ac4c`.
2. **Quiet the two diagnostic panels** — WHAT RAN and WHAT WAS CLAIMED. ✅ **DONE** — `70544f4`.
3. **Fix the stray-file recovery** — ⚠️ **HALF DONE** (`70544f4`). The offer is reachable;
   *detection* still misses a script that writes beside itself. See below.
4. **`feat/wsl2less`** — *on hold at Piero's instruction.* Not to be touched until he asks.

**A consequence of the hold, recorded so it is not rediscovered:** the `cwd` field job 3
wants lives on `feat/wsl2less`. Job 3 can still move the *existing* recovery button
somewhere reachable — that works off `Command.wrote`, which exists today — but improving
*detection* of stray files waits for `cwd` or an independent derivation of it.

---

## What is on the two branches

Both are lajesfen-cip <luciano.aguirre@cgiar.org>.

### `feat/wsl2less` — tip `7e55274`, last pushed today

`origin/main` is an **ancestor** of it. Zero overlapping files, so it merges clean — but it
will not stay that way, and it is the branch still being pushed to. **On hold regardless.**

| what | where |
|---|---|
| Backend runs natively on Windows through Git Bash — no WSL2 | `crates/app/src/backend.rs` (−1464/+…), `preflight.rs` (−806/+…) |
| `setup-wsl.sh` (299 lines) replaced by `setup-backend.ps1` + `setup-backend.sh` | `scripts/` |
| `_execute_via_bash` — command written to a temp `.sh` and run as a file | `overlay/minime_local/workspace.py` |
| `run_asta_cli` — `sys.executable -m asta.cli`, no shell at all | `overlay/minime_local/workspace.py` |
| `asta-plugins` as a git submodule | `.gitmodules` |
| Real tool work + 4 new test files (~347 lines) | `mini-me/backend/{autodiscovery,datavoyager,paper,theory}_tools.py` |
| **`cwd` on the `Command` struct** | `crates/app/src/workspace.rs`, `overlay/minime_local/ledger.py` |

Two of these are better than anything in this repo today:

- **The temp-script trick is a measured finding, not a preference.** `bash -c` with a long
  heredoc-bearing string silently mis-parses the here-document past a few hundred lines —
  bash warns `here-document delimited by end-of-file` and the write never completes. The
  same content as a real `.sh` file works at any size tried. That is a limit in how Git
  Bash reads `-c`, not the cmd.exe 8191-character ceiling, and it would have been a very
  expensive thing to discover twice.
- **`cwd` on `Command` is the missing half of job 2.** See below.

### `feat/ui-update` — tip `c8a1b63`, 2026-08-30 — ✅ ADOPTED at `df7169a`

113 commits behind `main`. Splits `main.rs` from 19,999 lines to 9,959, extracting
~10k lines into `crates/app/src/components/`: `common`, `sidebar`, `chat`, `gallery_view`,
`provenance_view`, `settings_view`, `palette_view`, `modals`, `status_bar`. Plus 15 icon
SVGs, a sidebar toggle, a conversations/projects toggle, chat creation inside a project,
and a `ui-design` skill.

**Only two files overlap with `main`: `main.rs` and `update.rs`.** `main` added 814 lines
to `main.rs` and 71 to `update.rs` since the split base. So this is not a textual merge —
`main.rs` was gutted on one side and grown on the other — but the work is bounded and
mechanical: re-home 814 known lines into the new module layout. That is a good trade for
deleting a 20,763-line file.

---

## Why this order

Doing the split first was right for a reason that only became visible afterwards: both
remaining jobs edit code that has now moved into `components/`. Had they gone first, every
edit would have been re-homed by hand a second time.

**One hard ordering constraint, still live:** the only existing path to recovering stray
files is the button at the bottom of the WHAT RAN modal. Hiding that panel before the offer
lives somewhere reachable would remove the sole route to files a researcher cannot
otherwise get at. Move the button out **first**, hide the panel **second**.

---

## Job 4 (ON HOLD) — adopt `feat/wsl2less`

> Piero, 2026-09-01: *"Dont adopt any wsl less yet until I ask him"*. Nothing below has
> been started. Kept because the branch moves and this is what to check when it resumes.


The prize is large and so is the blast radius: this changes how the backend is provisioned
on every existing install. The failure mode to fear is §283's, which cost a fortnight —
a release whose bundle and whose app disagreed about a filename, both sides internally
consistent, every test green.

- [ ] **1.1** Branch off `origin/main`, merge `origin/feat/wsl2less`, confirm the merge is
      the fast-forward the ancestry check promises.
- [ ] **1.2** `cargo test --workspace` and the Python suite (541 tests as of v0.3.29) on the
      merge, before reading anything.
- [ ] **1.3** **Check the artifact, not the source tree.** `scripts/package.sh` and
      `crates/app/src/update.rs` name provisioning scripts by filename, and this branch
      deletes `setup-wsl.sh`. Build the bundle, `unzip -l` it, and confirm every script the
      app looks for by name is in it. `BUNDLE_MARKERS` / `BUNDLE_BACKENDS` are the two
      constants that decide whether an install accepts a download at all.
- [ ] **1.4** **The submodule.** `asta-plugins` is a submodule; CI and `package.sh` must
      init it or the bundle ships an empty directory that fails at runtime, not at build.
      Verify by listing the bundle, not by reading the workflow.
- [ ] **1.5** **The upgrade path for an install that already has a WSL backend.** A
      researcher on v0.3.29 has a provisioned backend at
      `~/.local/share/mini-me-desktop/backend` inside WSL. After this change the app looks
      for one on Windows. Decide and write down what happens to the old one — adopted,
      re-provisioned, or ignored — because "it works on a fresh install" is how §283
      happened.
- [ ] **1.6** Run `scripts/backend-refresh-rehearsal.sh` against the new scripts.
- [ ] **1.7** Hand Piero the exact command to try, with **literal paths only** — no `~`,
      no `$(...)`, no variables. They do not survive PowerShell → wsl → bash.

**Not adopted blind.** Four backend tool files change substantially and bring their own
tests; those get read, not merged on faith. `verify=False` in the MCP is still open and
unrelated.

---

## Job 3 — the plots nobody could bring in

### What actually happened

> *"the agent generated files in another folder in wsl and I couldnt bring the plots
> because any button appeared!!"*

The screenshot shows the answer naming eight files —
`native_potato_biodiversity_cleaned_v1.csv`, `missingness.png`,
`correlation_heatmap.png`, and five more — under
`— named above but not in this conversation's folder`. And WHAT RAN saying
`6 commands · 3 failed`.

**A button to copy those files in has existed since §279.** `main.rs:16955` renders
`Copy N files into this conversation`; `collect_outside` (`main.rs:16765`) performs it;
`workspace::adopt` (`workspace.rs:608`) does the copy and never overwrites. Two things
kept it away from this researcher:

1. **It is two clicks deep inside a diagnostic modal.** Outputs panel → click the WHAT RAN
   card → scroll the modal → button. That is the panel Piero wants *hidden*.
2. **In this case it could not have appeared at all.** The chain is
   `files_left_outside` → `Command.wrote` → `ledger.written_during(record["outside"], …)`
   → `ledger.outside(command, work_dir)` → `ledger.named_paths(command)`. The last link
   **only sees absolute paths literally typed in the command text.** A script that writes
   `missingness.png` relative to its own working directory names nothing, so `outside` is
   empty, so `wrote` is empty, so the button is structurally unreachable. The summary line
   confirms it: 6 commands, and not one word about files written outside.

There is a second, independent cause. `outside()` measures against **the command's own**
`work_dir`. A background worker runs on its own LangGraph thread and gets its own folder
(`spine.solo_scope`, `THREAD_PARAM`). Writing into that folder is *inside* from the
worker's point of view, while being a folder the researcher's conversation never lists.
Both defects produce the same silence.

And the note the researcher actually saw is a third code path: `message.unverified`
(`main.rs:11257`), built by `named_files(body)` from the answer's prose. It carries **bare
filenames with no directory**. There is nothing there for a button to copy even in
principle — which is why the fix cannot simply be "put a button on that note".

### Done in `70544f4` — the offer is reachable

- [x] The button moved out of the WHAT RAN modal onto the answer that named the files.
- [x] `place_recovery_offer` puts it on the newest flagged answer, and on the newest answer
      when nothing is flagged — the case a script that writes beside itself produces.
- [x] `recovery_offer` keeps the note's count and the button's count apart, and says so when
      they disagree.
- [x] Three tests, each mutation-checked.

### Still open — detection

The offer can only fetch what `Command.wrote` knows about, and that is still decided by
`ledger.outside`, which reads **absolute paths out of the command text**. A script that
writes `missingness.png` next to itself names nothing, so the file is invisible to the
recovery path however good the button is. **This is the remaining half of the reported
defect** and it needs the filesystem, not the string.

**Stop parsing the command string; look at the filesystem.** Luciano's `cwd` field makes
this cheap — but it is on `feat/wsl2less`, which is on hold. Either wait, or derive the
working directory independently here.

- [ ] **2.1** A test first, red before anything else: a command whose text names no path,
      run in a folder outside the conversation, writing one file — assert it is offered for
      recovery. This is the exact case that produced eight orphaned plots and today it
      passes silently.
- [ ] **2.2** In the overlay, record files that **appeared in the command's own `cwd`
      during its window**, by mtime, alongside the existing named-path list. `written_during`
      already does the mtime work and `CLOCK_SLACK` already handles the coarseness. Keep
      the two lists distinct: a named path may be the researcher's own input, and a
      discovered one is a file we watched appear. Only the second may be acted on
      automatically — the existing comment on `Command.wrote` is right and stays right.
- [ ] **2.3** Bound the scan. An agent can create a virtualenv or unpack a dataset; walking
      an unbounded tree per command is not acceptable in `execute`'s hot path. Cap depth
      and count, and say when the cap bites — a silent cap turns this defect into a
      missing-513th-file defect.
- [ ] **2.4** Handle the worker-folder case: a sibling thread's folder is not "inside" for
      the researcher looking at *this* conversation, whatever it is for the worker.
- [ ] **2.5** **Move the offer to where the researcher already is.** The message that named
      the files is the place — that is where they looked, and the note is already there.
      The button belongs beside it, not inside a modal.
- [ ] **2.6** Two-sided contract test: the Python producer writes the fixture, the Rust
      decoder asserts every key is read or declared-unread with a reason.
- [ ] **2.7** Mutation-check every fix: revert it, confirm a **named** test fails. Three
      tests this month asserted nothing — one inspected module source for a guard, one
      re-implemented the filter it was testing, one handed out prepared pages ignoring the
      argument whose handling was the fix. Assume the same of these until proven.

**A promise this cannot make.** Nothing here can find a file written to a path nobody
recorded, by a process whose cwd we never saw. The offer covers what we watched happen.
Where it cannot see, it must stay quiet rather than imply the folder was checked.

---

## Job 1 — adopt the split — ✅ DONE (`d51ac4c`)

Merged at **`df7169a`**, not at the tip. The last commit, `c8a1b63` "refactor: make ui
function names more descriptive", deletes **1,591 comment lines** — a third of everything
recording why this app is shaped as it is. Every other commit on the branch is
comment-neutral or better; that one accounts for the whole loss. It is also not the rename
commit its subject claims: `road_strip` → `sidebar_panel` replaces a 181-line function with
a different 279-line one. Skipped whole.

Verified rather than assumed:

| check | before | after |
|---|---|---|
| `main.rs` lines | 20,763 | **10,468** |
| functions (all of `crates/app/src`) | 1,331 | 1,352 — **none of main's missing** |
| comment lines | 11,879 | **11,994** |
| Rust tests | 488 | **488 pass** |

Three defects the merge surfaced, each caught by a test already in this repo:

- Four new icons drawn on a 20×20 canvas against the app's 24×24 convention. Rescaled 1.2×.
  Their hardcoded hex fills are left alone on purpose — gpui's `svg_renderer` keeps only
  `p.alpha()` and paints a `MonochromeSprite` in the element's own colour, so nothing in
  those bytes can affect the tint. Checked in gpui's source rather than assumed.
- `New project…` had lost its ellipsis while the comment above it still explained what the
  ellipsis promises, and the handler still asks for a name. Restored.
- An `Open folder` row the branch adds was real and wired, and was hidden behind the
  ellipsis assertion failing first. Now asserted.

Also narrowed `#![allow(dead_code, unused_imports)]` to `unused_imports` on all nine
modules — `dead_code` in a file that is nothing but render methods would hide a feature
that stopped being drawn. Removing it produced no new warnings, which is the evidence that
nothing was orphaned. And kept `.claude/` ignored: the branch un-ignored it wholesale to
ship the skill, but `.claude/worktrees/` holds entire linked checkouts in the primary clone.

**Left for Luciano, not done here:** the two genuine renames in `c8a1b63`
(`divider` → `pane_divider`, `rail` → `provenance_rail`) are worth having, and are better
redone on top of this commit where they will not take the comments with them.

---

## Job 2 — quiet the two panels

> *"I dont like to see in the ui the What was claimed and the what ran because that noise
> to users."*

Agreed for a researcher. But these are not decoration: the claims recorder produced two
true findings about fabricated DOIs, and I called the first a false positive and shipped
on that reading. So the plan is **demote, not delete** — default off, behind a Settings
toggle, reachable when a run goes wrong.

- [ ] **3.1** Rebase `feat/ui-update` onto `main` (after job 1 and job 2 have landed).
      Re-home the 814 lines `main.rs` gained and the 71 `update.rs` gained into the new
      `components/` layout. Mechanical, but it is where a line gets silently dropped —
      diff the module set against the pre-rebase file, do not eyeball it.
- [ ] **3.2** `cargo test --workspace` green at the same count as before the rebase. A test
      that vanished in a file move is indistinguishable from one that never existed.
- [ ] **3.3** Read what the branch changed in `update.rs` — the updater is the one component
      whose bugs cannot be fixed by an update.
- [ ] **3.4** Add the setting. Default off. Name it for what it shows, not for the audience
      it is aimed at.
- [ ] **3.5** Gate the two summary cards on it — `commands_line` (WHAT RAN) and
      `claims_line` (WHAT WAS CLAIMED). Note that `outputs_are_empty` counts both; hiding
      them must not resurrect the §277 bug where a turn that wrote everything to `/tmp`
      left the panel silent.
- [ ] **3.6** Confirm **job 2's offer is reachable with the panels off.** This is the whole
      reason for the ordering. If turning the toggle off hides the only route to a stray
      file, the two changes have cancelled out and the researcher is worse off than today.
- [ ] **3.7** Adopt the UI features on their own merits, named individually in the PR:
      sidebar toggle, conversations/projects toggle, chat creation inside a project, icons.

---

## Reported from real installs — diagnosed here, not yet fixed

Both found on 2026-09-07 from researcher logs. Evidence is written out so nobody re-derives it.

### A. A model call that dies mid-stream ends the turn, and no retry can reach it

**Seen:** paper finder, LENOVO laptop. `An internal error occurred — sidecar log: …`

```
16:34:36  tool_gate: academic_researcher has not searched yet — forcing find_papers
16:34:50  find_papers('diversidad de papas en Paucartambo, Cusco') -> 10 paper(s)   ← worked
16:34:53  POST openrouter.ai/api/v1/chat/completions "200 OK", stream opens
          ...132 seconds of silence...
16:37:05  openai.APIError: Upstream idle timeout exceeded          run_exec_ms=155228
```

The search succeeded. What died was the subagent's *next* model call — the one deciding what to
do with those 10 papers. "Upstream idle timeout exceeded" is OpenRouter's own message: it proxies
to an upstream model and cuts the stream when that upstream stops emitting. **Why the upstream
went quiet for 132s is not established** — possibly prompt size after 10 papers, possibly their
routing. Do not guess it in a fix.

**`MODEL_MAX_RETRIES` cannot fire here, and raising it does nothing.** Verified in the installed
SDK, not assumed:

- the error is raised at `openai/_streaming.py:91`, **inside SSE iteration**, from an error
  object the provider injected into an already-successful stream;
- `_should_retry(self, response: httpx.Response)` decides purely on **status code**.

The response was **200**. By the time the error arrives the request has long since returned. A
correct retry guarding a different failure than the one that happens — `test-the-join` again.

- [ ] **A.1** A retry at the **LangChain/agent** layer, not the SDK layer: catch a mid-stream
      `APIError` and re-issue the call. Bounded, and it must say in the transcript that it
      retried — a silent second call to a paid provider is the one thing this app does not do.
- [ ] **A.2** Recognise the shape and say it. `TurnEvent::Error` currently appends the sidecar
      path to langgraph's generic text, so a researcher reads 700 lines to learn their provider
      timed out. *"The model provider timed out — the search completed, try again"* is actionable;
      *"An internal error occurred"* is not.
- [ ] **A.3** The turn dies after the expensive part. 10 papers were found and recorded into
      `sources._seen`, which is an **in-memory dict** — no artifact survives for the researcher.
      Whether a failed turn should keep what its tools already returned is a real design question,
      not an obvious yes.

### B. A second laptop was silently running without the SQLite checkpointer

**Seen:** VHUALLA laptop, backend at `/root/.local/share/mini-me-desktop/backend` (root user in
WSL). *"we could not reopen conversations. It seems these were deleted or never saved."*

**Proven by comparing two logs.** The working laptop has four lines this one does not:

```
Configuring custom checkpointer at …/.desktop-overlay/minime_local/checkpointer.py:checkpointer
Loading checkpointer …
minime_local: conversations are stored in …/.langgraph_api/checkpoints.sqlite
Using custom checkpointer: AsyncSqliteSaver
```

On the second laptop the log goes straight from `Starting In-Memory runtime` to
`OTel metrics reporter initialized`. **No checkpointer was configured at all.**

**Why.** `make_config.py:sqlite_available()` omits the `checkpointer` key when
`import langgraph.checkpoint.sqlite.aio` fails — deliberately, so a missing optional dependency
cannot stop the server booting. So `langgraph-checkpoint-sqlite` is not in that venv.

**Why nobody was told.** `ensure_checkpointer_command()` runs at every launch and is:

```
{ .venv/bin/python -c 'import langgraph.checkpoint.sqlite' 2>/dev/null \
  || uv pip install langgraph-checkpoint-sqlite ; } >/dev/null 2>&1 || true
```

`>/dev/null 2>&1 || true`. If that install fails — no network, a root-owned WSL install, `uv` not
on PATH — **the failure is unobservable**. The backend starts, persistence is downgraded, and the
only trace is a `Warn` row in Setup that its own comment says a researcher should never need to
notice.

**What it costs.** §95 moved conversations to SQLite precisely so a failed index load could not
take thirty threads with it. Without it they are back in `langgraph_runtime_inmem`'s pickle
store — and upstream's `start_pool` **deletes the whole thread index on any load exception**, its
own message naming the trigger as *"pulled updates that modified class definitions"*, which on
this product **is the update path** (§218). "Deleted or never saved" is a literal description.

`index_guard` *is* installed on that machine (the log confirms it), so a deleted index left a
stamped copy beside it. That is recoverable evidence, not a fix.

- [ ] **B.1** Make the failure observable. The `|| true` is right — a backend that starts beats
      one that does not — but it must **record** what happened. A launch that could not install
      the checkpointer should say so where the app can read it.
- [ ] **B.2** Say it where a researcher is, not only in Setup. Running without SQLite means the
      next backend update can lose their history; that deserves better than an optional-looking
      amber row.
- [ ] **B.3** Fix the Setup wording. It reads *"the pickle store — boot slows as history grows,
      and a failed load can overwrite it"*. True but far too mild for **"an app update can delete
      your conversation index"**, which is what §218 documents.
- [ ] **B.4** Find out why the install failed on a root-owned WSL install specifically. Two
      laptops, same installer, different outcome — that difference is the bug, and it is
      reproducible on hardware Codex has.
- [ ] **B.5** Check the join, not the part. Provisioning installs it, the launch re-checks, Setup
      reports it — three correct mechanisms, and a machine still ran for weeks without
      persistence because none of them could report failing.

**One command settles the state of any install** (literal paths, no variables):

```
wsl ls /root/.local/share/mini-me-desktop/backend/.venv/lib/python3.12/site-packages/langgraph/checkpoint/
```

`sqlite` present means persistence is on. And to see whether an index was ever rescued:

```
wsl ls -la /root/.local/share/mini-me-desktop/backend/.langgraph_api/
```

Files ending `.minime-rescued-<stamp>` are copies taken before upstream deleted the index.

### C. Four fake tracebacks at every startup

Starlette parses route docstrings as OpenAPI YAML. `collect_outside_files`, `start_sandbox`,
`theorizer_status` and `get_project` all contain `: ` sequences YAML reads as mappings, so every
boot logs four full `ScannerError` tracebacks. Nothing is broken. But the backend log is the
diagnostic path for A and B above, and it opens with four stack traces that mean nothing.

- [ ] **C.1** Reword the four docstrings so they parse — or stop feeding them to the schema
      generator. Cheap, and it makes every future diagnosis easier.

---

## Risks I am flagging rather than deciding

- **Job 1 is the largest behavioural change in this app's history.** Removing WSL touches
  every path-translation assumption. It is also unambiguously right — Windows is ~98% of
  users and every WSL crossing has been a defect source. Worth doing, worth doing carefully.
- **The upgrade path for existing WSL installs** (1.5) is the part most likely to bite, and
  the part no test on either branch covers.
- **Deleting a diagnostic that has been right** is a real cost. Hidden-by-default keeps it.
- **`feat/ui-update` grows staler daily.** If job 1 and job 2 take long, the 814 lines to
  re-home become more. Cheaper to rebase it early onto a throwaway branch and keep it warm.

## For Piero

- Luciano is pushing to `feat/wsl2less` **today**. Before I merge, worth telling him — a
  branch adopted from under someone mid-work is how two people do the same job twice.
- The WSL removal changes what every existing install does at startup. I would rather ship
  it as its own release with nothing else in it, so that if it goes wrong on your machine
  there is exactly one suspect.
