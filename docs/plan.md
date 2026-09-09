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
3. **Fix the stray-file recovery** — ✅ **DONE** (`70544f4` plus §301). The offer is reachable,
   and bounded cwd observation now detects a script that writes beside itself.
4. **`feat/wsl2less`** — *on hold at Piero's instruction.* Not to be touched until he asks.

**A consequence of the hold, recorded so it is not rediscovered:** the `cwd` field first appeared
on `feat/wsl2less`, but §301 derived it independently without adopting that branch. The WSL-less
work remains untouched and on hold.

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

### Detection — done in §301

The offer can only fetch what `Command.wrote` knows about, and that is still decided by
`ledger.outside`, which reads **absolute paths out of the command text**. A script that
writes `missingness.png` next to itself names nothing, so the file is invisible to the
recovery path however good the button is. **This is the remaining half of the reported
defect** and it needs the filesystem, not the string.

**Stop parsing the command string; look at the filesystem.** Luciano's `cwd` field makes
this cheap — but it is on `feat/wsl2less`, which is on hold. Either wait, or derive the
working directory independently here.

- [x] **2.1** A test first, red before anything else: a command whose text names no path,
      run in a folder outside the conversation, writing one file — assert it is offered for
      recovery. This is the exact case that produced eight orphaned plots and today it
      passes silently.
- [x] **2.2** In the overlay, record files that **appeared in the command's own `cwd`
      during its window**, by mtime, alongside the existing named-path list. `written_during`
      already does the mtime work and `CLOCK_SLACK` already handles the coarseness. Keep
      the two lists distinct: a named path may be the researcher's own input, and a
      discovered one is a file we watched appear. Only the second may be acted on
      automatically — the existing comment on `Command.wrote` is right and stays right.
- [x] **2.3** Bound the scan. An agent can create a virtualenv or unpack a dataset; walking
      an unbounded tree per command is not acceptable in `execute`'s hot path. Cap depth
      and count, and say when the cap bites — a silent cap turns this defect into a
      missing-513th-file defect.
- [x] **2.4** Handle the worker-folder case: a sibling thread's folder is not "inside" for
      the researcher looking at *this* conversation, whatever it is for the worker.
- [x] **2.5** **Move the offer to where the researcher already is.** The message that named
      the files is the place — that is where they looked, and the note is already there.
      The button belongs beside it, not inside a modal.
- [x] **2.6** Two-sided contract test: the Python producer writes the fixture, the Rust
      decoder asserts every key is read or declared-unread with a reason.
- [x] **2.7** Mutation-check every fix: revert it, confirm a **named** test fails. Three
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

### B. Conversation storage is wired to the background-work switch — ✅ FIXED

**Seen:** VHUALLA laptop, backend at `/root/.local/share/mini-me-desktop/backend`.
*"we could not reopen conversations. It seems these were deleted or never saved."*

**Two wrong readings were published before this one. Both are recorded because the way they failed
is the lesson.** First: *"the package is not installed"* — disproved by
`ls …/site-packages/langgraph/checkpoint/` returning `base memory serde sqlite`. Second:
*"`aiosqlite` is missing so the `.aio` import fails"* — disproved by running the exact import
`make_config.py` runs, which **succeeded**, on a machine where `aiosqlite` was already present.
Two plausible mechanisms, each fitting the symptom, each wrong. What settled it was reading the
launch path instead of theorising about the environment.

**The cause.** `backend.rs`:

```rust
let config_flag = if async_subagents {
    prepare.push_str(&generate_config_command(...));   // runs make_config.py
    prepare.push_str(" && ");
    format!(" --config {GENERATED_CONFIG}")
} else {
    String::new()                                       // ← upstream langgraph.json, as-is
};
```

- `make_config.py:88` is the **only** place `config["checkpointer"]` is ever set.
- `mini-me/langgraph.json` — what a default install actually runs — has **no `checkpointer` key**:
  `graphs.agent`, `http`, `auth`, `env`, and nothing else.
- `settings.rs:221`: `async_subagents: false`.

**So with "Let work run in the background" off — the default — no checkpointer is ever configured,
however completely the package is installed.** Conversations live in `langgraph dev`'s in-memory
store for the life of the process. Threads are listed from `.langgraph_ops.pckl`; the messages are
never written at all.

**Confirmed in both logs, on the one line that distinguishes them.** `make_config.py` also adds a
`background` graph, unconditionally, in the same pass. The working laptop imports two graphs:

```
Importing graph profiling … graph_id=agent      … path=./backend/agent.py
Importing graph profiling … graph_id=background … path=…/minime_local/async_agents.py
```

The broken one imports `agent` only. Two capabilities behind one flag, and the absence of the
second is the proof that the first never ran.

Disk state agrees: `.langgraph_api/` holds `.langgraph_ops.pckl` (17 KB, growing) and
`.langgraph_retry_counter.pckl`, and **no `checkpoints.sqlite`**. No `.minime-rescued-*` copies, so
`index_guard` never fired and nothing was deleted. Of the researcher's two guesses — *"deleted or
never saved"* — **never saved** is the true one.

**Every safety net measures the package; none measures the wiring.**

| where | what it checks | verdict on the broken laptop |
|---|---|---|
| `setup-wsl.sh:266` | installs `langgraph-checkpoint-sqlite` | done, correctly |
| `ensure_checkpointer_command()` | installs it again at launch | already there, skipped |
| `preflight.rs:636` | the package **directory** exists | **green** — "SQLite — conversations load without unpickling" |
| `backend.rs:1968` | `make_config.py`'s **output** carries the key | passes — the output is correct |

Four correct mechanisms about the package. **Zero about whether it is connected.** The one test in
the area asserts the generated config is right and never asks whether the launch uses it — the
join, untested, one more time.

**Immediate relief, with its cost stated.** Turning **on** *Settings → "Let work run in the
background"* restores persistence today, because it is what causes `--config` to be passed. It
also enables the preview background-subagent feature, which is off by default for its own reasons
(`settings.rs`: a preview deepagents API whose docs say "APIs may change"). Coupling those two is
the bug; a researcher should not have to accept a preview feature to keep their history.

- [x] **B.1** **Unbind the two.** Generate the config and pass `--config` on **every** launch. The
      `background` graph can stay declared without being used; the checkpointer cannot be
      configured without being passed. Nothing about durable storage belongs behind a preview flag.
- [x] **B.2** A test on the **join**: build the launch argv with `async_subagents = false` and
      assert `--config` is still there. Today that assertion fails, which is the point.
- [x] **B.3** Make Setup check the wiring, not the directory. "Is the package present" and "are
      conversations being saved" turned out to be different questions, and only the first is asked.
- [x] **B.4** Fix the Setup wording. It reads *"the pickle store — boot slows as history grows,
      and a failed load can overwrite it"*. On this laptop the truth was **nothing was saved at
      all** — a different sentence, and a worse one.
- [ ] **B.5** Decide what to tell someone whose history was never written. It cannot be recovered;
      it was never on disk. Silence is the wrong answer.


**What landed (§303).**

- `backend.rs` generates and passes `--config` whenever there **is an overlay**, instead of
  whenever background work is on. The condition was never about the feature; the generator lives
  in the overlay, and a sandbox run has none. The first attempt generated unconditionally and
  produced `"/minime_local/make_config.py"` on the sandbox path — caught immediately by
  `the_sandbox_path_is_left_exactly_as_it_was`, which is what that test is for.
- `conversations_are_saved_with_background_work_off` asserts the launch argv carries `--config`
  with `async_subagents = false`, and that `MINIME_ASYNC_SUBAGENTS` is still absent — persistence
  must not switch a preview feature on. Mutation-checked: restoring the flag fails it by name.
- **A test that asserted the defect** had to be inverted. `background_work_registers_its_graph…`
  ended with *"with the feature off, the launch is exactly what it always was"* and asserted no
  `make_config` and no `--config`. Faithful to its intent, and the intent was the bug. The two
  assertions are inverted rather than deleted so the file records that this was once believed
  correct; the third — that the feature stays off — is untouched.
- Setup now runs the **import the backend performs** (`langgraph.checkpoint.sqlite.aio`) through
  the backend's own interpreter, rather than checking that a directory exists. The row is a
  `Fail`, not a `Warn`, and says *"conversations are not being saved"* instead of describing a
  slower store. Its fix installs `aiosqlite` alongside the checkpointer, and its note says
  plainly that anything from before was never written.
- The four docstrings end with starlette's documented `---` separator
  (`schemas.py:parse_docstring` takes `split("---")[-1]`), so prose is never fed to
  `yaml.safe_load`. `mini-me/tests/test_route_docstrings_parse.py` applies that exact rule to
  every **registered** endpoint — read from the `Route(..., endpoint=…)` table, after a first
  version flagged three private helpers starlette never sees. Mutation-checked by removing one
  separator.

495 Rust tests, 568 Python.

**Left open deliberately:** B.5 — what to tell a researcher whose history was never written. It
cannot be recovered; it was never on disk. Nothing in the app says so today, and inventing that
sentence without knowing how many installs are affected would be guessing.

**Noticed in passing, not touched:** `ui/settings_view.rs:528` computes `current =
self.sidecar.project()` and never uses it. It arrived with the merged UI work. Prefixing it with
an underscore would silence a warning that may be a missing feature — the picker marking which
project you are already in — so it is reported rather than quieted.

**Settling the state of any install** — the generated config is only meaningful if the launch
passes it, so read the log rather than the filesystem:

```
Get-Content "$env:TEMP\mini-me-desktop-backend.log" | Select-String "custom checkpointer"
```

`Using custom checkpointer: AsyncSqliteSaver` means conversations are being written. **No match
means they are not.**

### C. Four fake tracebacks at every startup — ✅ FIXED

Starlette parses route docstrings as OpenAPI YAML. `collect_outside_files`, `start_sandbox`,
`theorizer_status` and `get_project` all contain `: ` sequences YAML reads as mappings, so every
boot logs four full `ScannerError` tracebacks. Nothing is broken. But the backend log is the
diagnostic path for A and B above, and it opens with four stack traces that mean nothing.

- [x] **C.1** Reword the four docstrings so they parse — or stop feeding them to the schema
      generator. Cheap, and it makes every future diagnosis easier.

---

## D. `backend exited during startup with exit code: 15` — OPEN, and the logs could not say why

**Seen:** VHUALLA laptop on v0.3.32, immediately after §303 made `make_config.py` run on machines
where it never had. Setup all green, including *"SQLite — conversations are written to disk as they
happen"*. Status bar: `backend exited during startup with exit code: 15`.

**What the evidence rules out.** Running the app's own config by hand on that machine starts
cleanly in 4.6 seconds — checkpointer loaded, `AsyncSqliteSaver` in use, both graphs imported,
`Application started up in 4.636s`:

```
wsl bash -lc "cd /root/.local/share/mini-me-desktop/backend && .venv/bin/langgraph dev \
  --host 127.0.0.1 --port 2025 --config .mini-me-desktop.langgraph.json --no-reload --no-browser"
```

So **the generated config is not the cause**, and neither is the checkpointer or the `background`
graph — both were tested by importing them directly and both are fine. Three hypotheses about that
machine were published before this and all three were wrong: the package was installed, `aiosqlite`
was installed, the `.aio` import succeeds.

**What is known.** Exit 15 is SIGTERM, and nothing in `langgraph dev` exits 15 on its own — the app
terminated it. `stop()` is the only path that sends SIGTERM, and it is called from `Drop` and from
restart. The app's health budget is 120 attempts at 500 ms, so a 60-second startup is not it.

**Why it could not be diagnosed — fixed in §305, still the reason D is open.** Both logs were
opened with `File::create`. Every spawn truncated the sidecar log, so the failing run's output was
erased by the next attempt and the researcher's log held a single line. And two app instances each
truncating the app log overwrote the other's regions, producing timestamps out of order — 16:22:20
printed above 16:22:09 — which reads as impossible if taken for one process, and was.

- [ ] **D.1** Re-collect both logs on that laptop now that they append and carry a spawn banner.
      The question is what happens between `spawning backend sidecar` and the SIGTERM.
- [ ] **D.2** Establish whether two app instances were running. The duplicate
      `reusing the valid Asta token` lines and the impossible ordering both point that way, and a
      second instance stopping the first one's backend would produce exactly this.
- [ ] **D.3** If two instances is the cause, decide what the second one should do. Attaching to a
      healthy backend is already the behaviour (`ensure_running`); terminating one that another
      app is waiting on is not.

**Not to be done before D.1:** reverting §303. The config is proven good on the affected machine,
so a revert would remove the persistence fix without addressing this, and would put that laptop
back to losing conversations silently. Turning **on** *"Let work run in the background"* is the
workaround while this is open — it is what the working laptop does.

---

## E. One MCP server returning 401 takes the whole agent down — NEW, and two separate problems

**Seen:** VHUALLA laptop, 2026-09-08, on v0.3.32 with background work switched on. Saying "hi"
failed. **Not seen before:** on 2026-09-07 the same server answered `200 OK` and the backend
logged `wrapped 23 of 23 tool(s)`. It now answers:

```
HTTP Request: POST https://dataverse-cip.fastmcp.app/mcp "HTTP/1.1 401 Unauthorized"
```

**The good news in the same log, worth recording because three readings of this laptop were
wrong:** the backend started cleanly — `Application started up in 9.952s`,
`Using custom checkpointer: AsyncSqliteSaver`, `conversations are stored in …checkpoints.sqlite`.
So §303's persistence fix works there, and **D did not recur** with background work on.

### E1 — the deployment stopped being public (theirs, not ours)

`MCP_SERVER_CONFIGS["dataverse"]` carries **no `headers_env`**, unlike `asta`, which sends
`x-api-key` from `ASTA_API_KEY`. The backend has never sent credentials to that server; it worked
because the FastMCP Cloud deployment was open. A 401 means it is not any more.

Piero's to settle, since it is his deployment: either make it public again, or give it a token and
add a `headers_env` entry beside Asta's.

### E2 — and one server failing should not cost the agent (ours)

This is the part worth fixing regardless of E1. In `get_mcp_tools`:

```python
loaded = await client.get_tools()
_mcp_tools_cache[bundle] = _make_mcp_tools_resilient(loaded)
```

`_make_mcp_tools_resilient` wraps tools **after** they load — it guards tool *calls*. Tool
*loading* is unguarded, so a 401 propagates out through `agent.py:133` while the **graph is being
constructed**, which means:

- `GET /assistants/{id}/schemas` → **500**
- the run → `Background run failed`
- and every conversation dies, not only the ones that would have touched Dataverse

Asta answered 200 and wrapped 8 tools in that same log. Agrovoc and CropOntology were never
reached. One unauthorised server out of four and the researcher gets nothing — for a question that
needed none of them.

- [ ] **E.1** `get_mcp_tools` survives a server it cannot load: keep the tools it got, log which
      server failed and why, and let the agent build. A specialist whose tools are missing already
      has a story for that (`get_dataverse_search_mcp_tools` computes `missing` and reports it);
      an agent that will not construct has none.
- [ ] **E.2** Say it where the researcher is. "An internal error occurred" for *"hi"* is the §303
      complaint again — *"the Dataverse catalogue is not reachable (401); everything else works"*
      is actionable and true.
- [ ] **E.3** A test on the join: one server in the bundle raising during `get_tools` must still
      yield a usable agent. Nothing covers this today, which is why one 401 was enough.

**Careful about caching.** `_mcp_tools_cache` is keyed by bundle and populated only on success.
Degrading must not cache the degraded set for the process's life — a server that recovers should
be picked up, and a fix that pins "dataverse is down" until the next restart trades one bad day
for a longer one.

---

## E — ✅ FIXED by Codex, adopted with three conflicts resolved by hand

`#230` (`codex/conversation-errors`) found the same defect independently and fixed it better than
E.1–E.3 proposed:

- **One adapter per server.** `_get_or_create_mcp_client` now *refuses* a multi-server bundle:
  *"Mini-Me opens one MCP deployment per adapter so one outage cannot affect another."*
- **Only failures are cached.** Successes ride FastMCP's TTL-aware response cache, so a server can
  change its catalogue without an app restart — which is the half of the trap E flagged.
- **`_minime_mcp_capped`** stops a second truncation wrapper being stacked when the graph is
  rebuilt and FastMCP returns the same tool object. Not something E anticipated.
- `get_dataverse_search_mcp_tools` marks the server unavailable and returns `[]` instead of raising.
- A `/mcp` status route with per-server labels, so the UI can name which service is down.
- Migrated `langchain-mcp-adapters` → first-party `langchain.mcp` (`MCPAdapter`) + `fastmcp.Client`.

**The trap E named is still there, deliberately.** Failure memoization is process-lifetime, so
**fixing the Dataverse deployment does not clear it until the backend restarts.** Codex documented
why — otherwise every read-only thread-state request hammers a dead deployment — and the `/mcp`
route at least makes the state visible. Worth knowing before someone reports "I fixed it and the
app still says unavailable".

### What the adoption cost, recorded because taking the branch wholesale would have broken things

`#230` was based on `#220` and **21 commits behind**. Three conflicts, and two of them would have
silently reverted shipped work:

| file | resolution |
|---|---|
| `main.rs` | Kept main's `sidebar_width: 300.` — the base had **320**, so Codex never changed it; **Luciano did**, in #225. Took his new `mcp_notice_focus`. |
| `ui/chat.rs` | Dropped his `composer_row`. Main **moved** it to `ui/chat_input.rs`, and his copy still carries the `.m_2()` that #225 removed on purpose ("fix composer margin"). Keeping it would have been a duplicate definition *and* a reverted fix. |
| `ui/gallery_view.rs` | Dropped his `output_card` (202 lines). Main removed it in #225 along with its only caller, replaced by `attachment_row`/`attachment_tile` — both called. His copy would have been dead code with no call site. |

Verified rather than assumed: **1,380 → 1,414 functions and 529 → 539 test fns with nothing from
main lost**, all twelve v0.3.31–v0.3.33 symbols present, and warnings identical to main's eleven
(an earlier reading of "six" predated #225). **512 Rust tests, 577 Python.**

### F. The dependency bump is the real risk in #230, and it is invisible in the title

`fastmcp>=3.2.4` → **`>=4.0.0`** and `langchain-mcp-adapters>=0.2.2` → **`langchain[mcp]>=1.4.0`**.
Resolved and verified in a clean venv: `fastmcp 4.0.3`, `langchain 1.4.0`, `mcp 2.2.0`, 577 tests
passing.

**But against the currently installed venv the suite does not even collect** — 9 collection errors,
`cannot import name 'InputRequiredResult' from 'mcp.types'`, because `fastmcp 4` needs `mcp >= 2`.
So an install that does not get the new packages has a backend that cannot import `mcp_tools`.

The upgrade path exists and is the right shape: the launch runs
`cmp -s uv.lock .mini-me-lock || { uv sync --extra dev && cp uv.lock .mini-me-lock; }`, and
`.mini-me-lock` is only stamped on success, so a failed sync retries next launch rather than
sticking. What changed is the **blast radius**: before #230 a failed sync meant slightly stale
libraries; after it, a backend that cannot start.

- [ ] **F.1** Make a failed `uv sync` observable. The whole prepare block is `>/dev/null || true`,
      so the one step that now decides whether the backend can import at all cannot say it failed.
      §305 means the resulting `ModuleNotFoundError` will at least survive in the sidecar log — the
      instrument works — but preventable is better than diagnosable.
- [ ] **F.2** Tell the researcher what a first launch after this update is doing. `uv sync` pulling
      a new `mcp` major over a slow connection looks exactly like a hang, and the health budget is
      60 seconds.

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
