# Mini-Me Desktop — handover

You are taking over development of **Mini-Me Desktop**, a native Rust/GPUI research
workbench used by researchers at CIP (International Potato Center). It is the desktop
client for the Mini-Me Python/LangGraph agent stack, which runs as a sidecar in WSL.

You can do something the previous developer could not: **run the app on Windows and
watch it behave.** Almost every expensive mistake in this project's history came from
reasoning about behaviour instead of observing it. Use that.

---

## Where things stand

- **Released:** `v0.3.29`, published and latest.
- **Open and unmerged:** PR #218 on `CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop`,
  branch `claude/adopt-ui-modularization`. Mergeable, clean, 491 Rust tests green.
  It adopts a colleague's UI split (`main.rs` 20,763 → 10,468 lines) and moves the
  stray-file recovery button somewhere findable. **Read its description before merging** —
  it explains why it merges a mid-branch commit rather than a branch tip.
- **Two colleague branches** by `lajesfen-cip` (Luciano, `luciano.aguirre@cgiar.org`):
  - `feat/ui-update` — adopted through `df7169a` by PR #218. Its tip commit `c8a1b63`
    is deliberately **not** adopted: it deletes 1,591 comment lines.
  - `feat/wsl2less` — removes the WSL2 dependency, runs the backend natively on Windows
    through Git Bash. **On hold at Piero's instruction** until he speaks to Luciano.
    `origin/main` is an ancestor of it, so it still merges clean. Do not adopt it without
    Piero saying so.

## Layout

| what | where |
|---|---|
| Rust app | `crates/app/src/` — `main.rs`, `components/*.rs`, `workspace.rs`, `backend.rs`, `protocol.rs` |
| UI component library | `crates/app/src/ui.rs`, palette in `theme.rs`, skill at `.claude/skills/ui-design/` |
| Backend (vendored) | `mini-me/backend/` — Python, LangGraph, deepagents |
| Overlay copied into WSL | `overlay/minime_local/` — `workspace.py`, `ledger.py`, `spine.py` |
| The design record | `docs/desktop-app-plan.md` — 17k lines, §300 is the last entry |
| Step-by-step for the open work | `docs/plan.md` |
| MCP server (separate repo) | `/home/ppalacios/Documents/AskPapa/mcp_server_stdio` |

**`docs/desktop-app-plan.md` is the source of truth.** Every numbered section (§N) records
one problem and what was done about it. Code comments cite them. When you fix something
non-obvious, add a section — the `§` references in comments are how anyone reconstructs a
decision two months later, and they are load-bearing.

---

## Rules that are not negotiable

1. **No credit may be spent without a human press.** This is the hardest-stated rule in the
   project. Any path that can start a paid model run must go through an explicit approval
   the researcher clicks. Never weaken this to make a flow smoother.
2. **The model API key is request-only.** It travels from the OS keychain into the run
   request (`configurable.__llm_keys`). It must never be written to the backend's
   environment, a `.env`, or a log. **Never log an API key.**
3. **Windows is the target.** ~98% of users are on Windows. A POSIX assumption is a defect,
   not a style choice.
4. **Never paste the GitHub PAT into chat or any file in the repo.**
5. **CIP policy applies to everything produced here:** validate AI-generated content with
   subject-matter experts; never enter confidential or personal data; never enter
   unpublished research or third-party IP; be mindful of biases; disclose when generative
   AI has been used.

---

## What has actually cost weeks here

These are not general advice. Each one is a specific failure this project paid for.

**Check the artifact, not the source tree.** A release shipped **7 of 12** middleware
modules for a fortnight — including the credit gate — while every test passed. `package.sh`
copied `vendor/Mini-Me`; the app looked for `mini-me/`. Both files were correct about their
own job and nothing compared them. When reported behaviour contradicts the code, the first
move is `gh release download <tag>` and `unzip -l`, before reading any code.

**Two things do not follow an app update on their own:** the backend provisioned at
`~/.local/share/mini-me-desktop/backend` inside WSL, and the overlay copied to
`.desktop-overlay`. "The fix is on main" says nothing about what is running on a
researcher's machine. Setup → *Update the backend* is a button somebody has to press, and
since §283 a content stamp makes the mismatch visible in the Setup pane.

**Test the join, not the part.** Six consecutive bugs were correct components with no
caller, or two correct components that disagreed about a filename. Prefer a test that
crosses the seam.

**A test that asserts nothing is worse than no test.** Three shipped this year: one
inspected module source for a guard near a call site; one re-implemented the filter it was
testing; one handed a mock prepared pages while ignoring the `start` argument whose
handling was the entire fix — it survived reverting that fix. **Mutation-check every fix:
revert it, confirm a *named* test fails, restore it.** If nothing fails, the test is
decoration.

**Do not `git checkout --` during a mutation check.** It has already discarded uncommitted
work in this repo. Copy the file aside and copy it back.

**Build the instrument, not the next guess.** Three silent failures in a row means the
system cannot be observed. Stop fixing and make it say what it is doing.

**A truncated search is a false negative.** Never conclude "it doesn't exist" from a
`head`-cut grep.

**Literal paths only in commands handed to the user.** `~`, `$(...)` and shell variables do
not survive PowerShell → `wsl bash -lc` → bash. Write the whole path out.

**Three filesystems, three log paths.** Windows `%TEMP%` is not WSL `/tmp`. The logs are
`%TEMP%\mini-me-desktop-app.log`, `%TEMP%\mini-me-desktop-backend.log`,
`%TEMP%\mini-me-desktop-update.log`.

**`langgraph dev` forbids blocking calls in `async def`.** `blockbuster` raises
`BlockingError` on synchronous filesystem calls on the event loop. Offload with
`asyncio.to_thread`. This silently killed a diagnostic for a week.

**A merged PR branch is dead.** Squash merge kills it — branch fresh off the base.

---

## Build, test, release

```bash
cargo test --workspace          # 491 tests
cargo check --workspace         # 5 warnings expected, all in ui.rs
```

Python tests live in `mini-me/tests/` (29 files, ~541 tests); the venv is in the sibling
checkout, not the vendored copy.

Release, **from Windows in Git Bash**:

```bash
cargo build --release -p mini-me-desktop-app
bash scripts/bundle-backend.sh
bash scripts/package.sh
bash scripts/release.sh
```

`release.sh` produces a **draft**. A draft has no resolvable tag, so `gh release edit <tag>`
fails — publish by numeric id:
`gh api -X PATCH repos/.../releases/{id} -F draft=false -f make_latest=true`.

Before publishing, unzip the bundle and confirm `overlay/`, `scripts/`, `vendor/` and
`mini-me/` are all present. `vendor/` is an empty compatibility directory for installs
older than §283 — dropping it makes every existing install reject the download, which
already happened once (v0.3.14).

---

## The open work, in the order I would take it

### 1. Stray-file detection — the half that is still broken

**This is the live user complaint.** An agent wrote eight plots into a folder outside the
conversation and the researcher could not retrieve them. PR #218 fixed *reachability* —
the fetch button now sits on the answer that named the files instead of two clicks inside
a diagnostic modal. It did **not** fix detection.

`ledger.outside(command, work_dir)` decides what was written outside by reading **absolute
paths out of the command text**. A script doing `plt.savefig("missingness.png")` names
nothing, so `outside` is empty, so `Command.wrote` is empty, so there is nothing to offer.
There is a second cause: `outside()` measures against *the command's own* `work_dir`, and a
background worker runs on its own LangGraph thread with its own folder — writing there is
"inside" for the worker while being invisible to the researcher's conversation.

The fix is to stop parsing strings and look at the filesystem: scan the command's real
working directory for files whose mtime falls inside the command's window. `written_during`
already does the mtime work and `CLOCK_SLACK` already handles clock coarseness. Keep the
named-path list and the observed-write list **distinct** — a named path may be the
researcher's own input, and only an observed write may be acted on automatically. Bound the
scan; an agent can unpack a dataset or build a virtualenv under its workspace.

Luciano's `feat/wsl2less` already adds a `cwd` field to the command record, which makes
this much cheaper — but that branch is on hold. Either wait for Piero, or derive the
working directory independently.

### 2. `GET /discovery/{thread_id}/drafts`

A draft created inside a background worker cannot be approved, because the app can only
learn about drafts through the conversation snapshot. This does not weaken the spending
contract — press plus nonce still gate it — but the work is unreachable.

### 3. The tool gate accepts a failed tool

`tool_gate._returned()` checks `type` and `name` but never `status`, so a **failed** tool
satisfies its gate and a "completed" artifact can follow a failed analysis. Deliberately
unpatched so far: `_gate` forces the tool on every model call with no escape, so simply
refusing failures would trap the run in a loop. It needs a retry budget plus a sentence
saying the gate gave up.

### 4. `mcp_tools.py` has no tests of its own

Known defects, none acted on: `_truncate_mcp_content_blocks` has an unconditional `break`
that drops later blocks; `_trim_json_array_text` discards sibling fields (`total`,
`next_cursor`, `facets`) and takes the first top-level list, which may be `facets`;
`_truncate_str_result` cuts inside tokens; `_save_mcp_to_sandbox` ignores `awrite`'s error
return; filenames collide at one-second resolution; `_make_mcp_error_handler` turns errors
into prose. **Step one there is a test file, not a patch.**

### 5. `verify=False` in the MCP disables TLS verification

Flagged and deliberately not changed — it may be load-bearing if CIP's certificate is
unusual. Piero's call.

### 6. Unconfirmed

Conversations after a self-relaunch. The blocking-call fix shipped in v0.3.18 and was never
confirmed working. If the sidebar comes up blank, grab `%TEMP%\mini-me-desktop-app.log`
**before** reopening the app.

---

## Two things nothing here can fix

**Nothing prevents a model writing a DOI in prose.** What exists is a recorder that says
when it happened — `WHAT WAS CLAIMED`, now off by default behind the `run_record` setting.
It has been right twice about fabricated identifiers, and the first time the developer
called it a false positive and shipped on that reading. If it disagrees with you, it has
the better track record.

**The app cannot tell a missing file from a file written elsewhere from a name the model
invented.** The UI deliberately reports the check and not a verdict. Keep it that way;
turning that note into an accusation was tried and cost the record its credibility.
