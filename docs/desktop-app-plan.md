# Mini-Me Desktop — Phase 6 plan & status

A native **desktop research-acceleration workbench** for Mini-Me, built in Rust
on **GPUI** (the GPU UI framework extracted from [Zed](https://github.com/zed-industries/zed)).
This repo is the desktop **client**; the Mini-Me agent stack (the coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** that the client spawns and supervises.

## Where we are now (updated 2026-08-02)

| Milestone | Status |
|---|---|
| **P6.0** — spike doc + scaffold | ✅ done |
| **P6.1** — buildable window *(go/no-go gate)* | ✅ **PASS** — builds green; window renders natively (verified on Windows/DirectX). §8 |
| **P6.2** — talk to the real backend | ✅ **done** — a real coordinator turn spawned, streamed and rendered **on Windows** (2026-07-30). §9 |
| **P6.2.5** — local-first backend (drop LangSmith/WorkOS) | ✅ **done, and now the default** — turns run on the host with no `LANGSMITH_API_KEY`/`WORKOS_*`, via a `PYTHONPATH` overlay that leaves the Mini-Me checkout untouched, and **every `execute` call waits for approval**. `--sandbox` still available. §18/§19 |
| **P6.3** — port the core panels | ✅ **done** — composer, spine, outputs, sandbox status, agent activity trace, **command palette**; plus conversation continuity (turns used to each start a new thread) |
| **P6.3.5** — visuals pass, starting with **markdown rendering** | ✅ **verified on Windows** — emphasis, inline code, links, headings, lists and fenced code render; accented Spanish came through intact. Tables deferred by agreement. §16/§23 |
| **Native-Windows probe** | ✅ **answered** — `cmd.exe` is ruled out by upstream's *own* tool code (POSIX pipes, `mkdir -p`, `shlex.quote`), so WSL2 stays the v1 runtime and the installer's job is guided provisioning. Native-plus-Git-Bash is a documented half-day experiment. §21 |
| **P6.4a** — settings panel + keychain secrets | ✅ **built** — a turn runs with no provider key in the backend's `.env`; keys come from the OS keychain and ride in the run request, and `ctrl-,` opens a Settings pane (provider, model, keys, execution). **Never rendered — needs a look on Windows.** §20/§22/§22b |
| **P6.4b** — native affordances + shipping | ✅ **ships** — `bundle-backend.sh` → `--release` → `package.sh` gives a 21 MB folder; **the packaged build ran a real turn on Windows**. **Job Object verified 2026-08-01** — after closing the app, `wsl -- pgrep -af "langgraph dev"` prints nothing. Resources resolve beside the executable, and **drop a file on the window** turns it into a question. Remaining: click-to-update, cancel a running fix. §24/§25/§26/§28 |
| **P6.5** — background work + Jobs panel | ✅ **done, end to end (2026-08-01)** — background work had in fact never run until §39: our graph factory took no `config` and raised `TypeError` at construction. Now a worker generates data, **stops at the approval gate on its own thread**, and the answer reaches it. Failures report the real exception, and the panel shows which subagent is running. §29–§31/§36–§42 |
| **P6.6** — outputs the researcher can see | ✅ **done** — files land in `Documents\Mini-Me\<thread>`, figures render in the chat, and OUTPUTS opens the folder. §42 |
| **P6.7** — the UI itself | ✅ **done, verified on Windows** — a role-based palette with four built-ins **and Zed's whole theme gallery** installable in-app; conversation sidebar with fuzzy search and rename; collapsible panels; a file preview modal; visible scrollbars; rainbow CSV; a three-state send button; rounded panels and a window-wide status bar. §43/§47–§53 |

### What is left (updated 2026-08-03)

Every milestone P6.0–P6.7 is closed and the app is in daily use by its first researcher.
What remains splits three ways: **shipping it to anyone else**, **paying down the UI debt
that keeps causing the same bug**, and **friction that is felt but not blocking**.

**Blocks a second person using it**
- ✅ **First run where WSL has never existed.** Proven on a third laptop (§61): elevated
  install, restart, reopen — **4 ok**, with the runtime, checkout, `uv sync` and overlay all
  green without anyone typing a command. Took three rounds: §57 fixed elevation, §60 made the
  elevated output readable and stopped the app claiming "done" over a red row, §61 reverted a
  `--no-launch` flag that could have unregistered the distro.
- 🟡 **The download link.** `v0.1.0` is tagged, built green, and attached as
  `mini-me-desktop-v0.1.0-windows-x64.zip`. The release is a **draft** — deliberately, since
  a draft can be deleted where a published release cannot. **Remaining: someone decides to
  publish it**, which is a call about who is allowed to have this, not an engineering step.
- ⬜ **Code signing.** SmartScreen shows "Windows protected your PC" and most researchers
  stop there. An organizational decision on a certificate; the release notes and README
  say which two words to click in the meantime.
- ⬜ **Click-to-update.** The app detects a stale checkout but cannot update itself (§27).

**UI debt — the same call-site mistake, six times now**
- ✅ **`Button`, `Label`, `Modal`, `Toggle`** (§67, §68) — confirmed on a real window across all
  five pages. 19 button sites, 6 label sites; **twelve square buttons rounded** on the way,
  including `Install Ubuntu` and `Copy ⧉`. `rounded_md` and `flex_none` are no longer reachable
  to omit, `disabled` is one flag rather than a colour beside a guard, `Label::ellipsis()`
  cannot produce §59's collapse, and `body`/`actions` being separate slots leaves "actions
  inside the scroll area" nowhere to happen. Setup is a page of the preferences window instead
  of a column that evicted the research panel. Two controls stay hand-written, each saying why.
- ✅ **The palette runs the row it highlights** (§69) — the selection was computed three ways and
  only the Enter path did not clamp, so past the end of a filtered list it silently did nothing.
  One function now, and the empty case says so out loud.
- 🟡 **UI polish** — all built (§70), **awaiting eyes**: theme and model as searchable dropdown
  popups; fenced code in a monospace stack (nothing bundled — `Consolas` ships with Windows);
  focus rings plus a tab order through each page's fields; toasts that stack and fade instead of
  one overwriting status line; panes resizable by dragging their edge.
- ✅ **Transcript re-parse** (§70) — markdown was being parsed *in `render`*, so every message
  was re-parsed sixty times a second. Cached beside the body now. The list's own entry said
  `uniform_list`, which was **wrong**: it lays every element out at the height of the first, and
  these are a one-line question next to a two-page report.
- ⬜ **Virtualize the transcript** with `list` + `ListState` — the honest version of the above,
  wanted only once a conversation is long enough to feel it.
- ⬜ **SVG icons** instead of `◎ ▤ ▥ ⏎` — deferred (§70): needs an `AssetSource` and
  hand-authored assets, to replace glyphs that render correctly, for no functional gain.

**Felt friction**
- ✅ **Multi-line composer** — Shift-Enter inserts a break, Enter still sends (§55).
- ✅ **Escape closes things**, inside-out; **conversations can be deleted** in two steps;
  **theme and model are filterable lists**; **corners are rounded** (§58/§59).
- ✅ **Text selection in the transcript** (§62) — drag across paragraphs, code blocks and table
  cells; `ctrl-c` copies, `ctrl-shift-a` takes everything, both also in the palette.
  Confirmed on a real window.
- ✅ **Cancel a running turn** (§63) — confirmed on a real turn stopped three steps in: the
  partial trace stayed, marked incomplete. Stop posts to LangGraph's cancel endpoint *and*
  aborts the stream, so the graph stops spending tokens rather than just losing its audience.
  The cancel now logs on success as well as failure, since whether the *backend* stopped is
  the one part nobody can see.
- ✅ **Right-click menu** (§64) — confirmed: copy/select-all in the transcript, cut/copy/paste
  in the composer, rows greyed when they would do nothing, each showing its own binding.
- ⬜ **Cancel a running setup fix** (§28).
- ✅ **Markdown gaps** (§65, §66) — verified on a real answer: blockquotes, nested lists whose
  depth comes from the indents actually seen (two- and four-space sources rendered identically,
  which was the point), images no longer showing their own punctuation. The one defect the
  eleven unit tests missed — a `>>` after a `>` folding into the outer quote — was found by
  that answer in seconds, because every test covered a construct alone and the bug lived in the
  transition between two.

**Proposed — P7**
- ⬜ **The provenance graph** (§73) — a modal showing which subagents a conversation used and
  where it doubled back: *paper search → theories → get data → clean data → analyze → theories*.
  Cycles are the point, not a defect. The live data exists; **what does not is any of it after a
  reload**, so this is a persistence problem first — the record has to be written to the thread's
  directory as it streams. And adjacency is arrival order, not causation: ask the backend for the
  real parent edge before inferring one. Sequenced after `/subagent`.
- ⬜ **`/subagent` slash commands** (`/eda-subagent`, `/research-paper`, `/report-write`),
  designed in §55: a registry read from the backend, the existing fuzzy picker as the
  trigger, foreground *or* background dispatch, and the approval gate still applying.
  Sequenced after the release and the component set.

**Deliberate deferrals**
- ⬜ **Old workspaces are not migrated** (§42), and **threads from before §51's tag do not
  appear** in the sidebar.
- ⬜ **Async subagents stay opt-in**, on a preview deepagents API.

**A correction, since it changes what is possible.** This document said text selection was
"the one thing the framework genuinely makes hard — GPUI 0.2.2 cannot." That is **wrong**:
`TextLayout::index_for_position` exists (`gpui-0.2.2/src/elements/text.rs:483`, and
`line_layout.rs:283`), and §55's multi-line composer already built per-line hit-testing and
per-line selection quads against it. Checked, not assumed — which is the fourth time in this
project a confident claim about the artefact dissolved on one `grep` (§52 has the other
three).

**Owed upstream** (found here, belongs in Mini-Me)
- ⬜ `guardrails.py` claims sandbox isolation host execution does not provide (§18).
- ⬜ The theorizer reports a *guess* instead of the command's real output (§35) — seven
  rounds, the most expensive defect of this project.
- ⬜ `deepagents`' `start_async_task` passes no config, so no self-hosted deployment can
  give a background run its model, key or recursion limit (§38/§39).

**Health of the bet.** The two risks that could have killed this are both down:
**R1** (GPUI as an unstable `git` dep) — GPUI is a *published* crate, pinned at
`gpui 0.2.2`. **R2** (API churn) — the P6.0 sketch compiled against it unchanged.
What remains is scope risk (**R3**: rebuilding rich UI) and packaging (**R4**) —
work, not uncertainty. **R4 shrank** once the target became local-first for
colleagues rather than a notarized public installer.

## What this product is (clarified 2026-07-30)

A **local-first, single-user research workbench** — deliberately *not* a hosted
service. The web app is the thing we are leaving behind, so the desktop app should
shed its infrastructure rather than reproduce it:

> **Windows is the primary platform: ~98% of our users are on Windows**
> (stated 2026-07-30). Linux is the *development* platform, not the target. Every
> feature is only "done" once it works on Windows, and anything that assumes a
> POSIX shell is a defect for almost the whole user base — see §13.

- **Drop the hosted services.** No **WorkOS** (auth is meaningless for a local
  single user) and no **LangSmith** (sandbox *and* tracing). §11 proves both are
  droppable — WorkOS for free today, LangSmith once execution is local.
- **Execution runs on the user's machine** (§10). That is also what makes an
  installable app possible: you cannot ask every scientist to provision their own
  remote sandbox.
- **The user's own API keys, on their own computer** — OS keychain, plus a setup
  tutorial. Two externals remain by nature: **Asta** and the **model API**.
- **"Click to update"**, Zed-style. The backend is Python, so an update is a fetch
  + dependency sync of a pinned checkout — no compile step. (Self-updating the
  Rust binary is a separate, later problem.)
- **Mini-Me stays upstream, unmodified and pinned** — bundled, never forked. The
  agent stack *is* the product and is actively developed; a modified copy would
  either accrue permanent merge debt or freeze and drift from the web app. Desktop
  needs are met by one opt-in seam (§10), not a fork.

---

## 1. Why desktop, why GPUI

**Why desktop.** The web app's ceiling is the browser sandbox. A desktop client
unlocks: local filesystem + native file dialogs (drop a CSV, no upload dance),
long-running/background agent jobs as first-class OS processes, offline, OS
keychain for secrets, multi-window, and a fast keyboard-driven multi-pane UX.

**The token win.** Mini-Me's deployed backend can't auto-refresh the Asta token
(it expires ~weekly; PR #33 added a manual paste-and-store workaround). The
**local** `asta` CLI *does* auto-refresh. Running the backend as a local sidecar
means the desktop app inherits that — **the token-expiry pain disappears.**

**Why GPUI (the chosen direction).** The goal is to "copy the best from Zed": a
fast, native, GPU-rendered, keyboard-first workbench. GPUI is the framework that
makes Zed feel the way it does. This is the high-ceiling, high-effort path
(Tauri-wrapping the existing React app is the lower-risk fallback documented in
Mini-Me's `docs/asta-integration-plan.md`, Phase 6). We proceed on GPUI with eyes
open — see the risk register.

**What stays the same.** Agents are the product and do **not** get rewritten. The
desktop app speaks to them over the existing HTTP/stream protocol. Org policy
stays **human-gated**: nothing auto-runs.

---

## 2. Honest risk register (read before building)

| # | Risk | Severity | Mitigation / kill-criterion |
|---|------|----------|------------------------------|
| R1 | ~~**GPUI is not a stable published crate.**~~ **Resolved (P6.1):** GPUI *is* published to crates.io. `gpui 0.2.2` is self-contained (companions `gpui_macros`/`gpui_util` published too; no `git`/`path` deps), so no Zed-monorepo `git` dependency is needed. | ~~High~~ **Low** | Pin the published crate: `gpui = "=0.2.2"`. Bump deliberately. Zed `git` rev `00bd72e…` (v1.13.1) kept documented as a fallback if a newer API is ever required. |
| R2 | **GPUI API churn.** Examples online drift from the current API (`App`/`AppContext`/`Context`, `cx.new` vs `cx.new_view`, `Render` signature). | Med | Build against the `examples/` in the pinned Zed rev, not blog posts. The `crates/app/src/main.rs` here is a *starting sketch* to reconcile against that rev. |
| R3 | **Rewriting rich UI** (streaming markdown, artifacts panel, PDF/figure views, charts) in GPUI is a lot of surface the browser gave for free. | High | Port incrementally (P6.3). Start with plain text + a simple list; add markdown/artifacts later. Consider embedding a webview *per-panel* only if a surface proves impractical in GPUI. |
| R4 | **Sidecar packaging.** Bundling a Python backend (uv/venv + the `asta` CLI + system deps) into a shippable app is non-trivial per-OS. | Med | P6.2 spawns a *dev* sidecar (assume `uv`/venv on PATH). Packaging (PyInstaller / uv bundle / container) is a later milestone, not MVP. |
| R5 | **Linux GPU stack variance** (Vulkan/Wayland/X11). | Low-Med | GPUI supports Linux via `blade`. Confirmed the dev machine has `libvulkan/libwayland/libxkbcommon/libX11`. Test early on the target machine. |
| R6 | **Team Rust capacity.** The rewrite needs sustained Rust work. | Med | Confirm before P6.1. This is an organizational, not technical, gate. |

**Overall:** the direction is viable but front-loaded with framework risk. P6.1
(a buildable window) is the go/no-go gate before any real investment.

---

## 3. Architecture

```
┌─────────────────────────────────────────────────────────┐
│  mini-me-desktop  (Rust / GPUI)                          │
│                                                          │
│  ┌───────────────┐   ┌──────────────────────────────┐   │
│  │  UI (GPUI)    │   │  BackendSupervisor            │   │
│  │  - chat pane  │◄──┤  - spawns the Python sidecar  │   │
│  │  - artifacts  │   │  - health-check / restart     │   │
│  │  - spine/plan │   │  - streams turns over HTTP/SSE│   │
│  │  - cmd palette│   └───────────────┬──────────────┘   │
│  └───────────────┘                   │ localhost:PORT    │
└──────────────────────────────────────┼──────────────────┘
                                        │
                   ┌────────────────────▼────────────────────┐
                   │  Mini-Me backend (Python, unchanged)     │
                   │  coordinator + subagents + skills        │
                   │  local `asta` CLI (auto-refreshing auth) │
                   └──────────────────────────────────────────┘
```

- **Client ↔ backend boundary:** the existing HTTP + streaming protocol the web
  frontend already uses (LangGraph run/stream). The desktop app is *another
  client* of that protocol — no new agent code.
- **Local sidecar:** `BackendSupervisor` spawns the backend (e.g. `uv run …` or
  the LangGraph dev server) on a localhost port, waits for health, and tears it
  down on quit. Auth uses the local `asta` CLI's own refreshing token.
- **Secrets:** model/API keys and any Asta token go in the **OS keychain** (via
  the `keyring` crate), never a plaintext dotfile.

---

## 4. Crate / workspace layout

```
mini-me-desktop/
├── Cargo.toml               # workspace
├── rust-toolchain.toml      # pinned toolchain
├── .gitignore               # /target, etc.
├── README.md
├── docs/
│   └── desktop-app-plan.md  # this file
└── crates/
    └── app/                 # the desktop binary
        ├── Cargo.toml       # gpui (git, pinned), serde, tokio, keyring…
        └── src/
            ├── main.rs      # GPUI app entry + root workbench view (sketch)
            └── backend.rs    # BackendSupervisor: spawn/health/stream (stub)
```

Future crates as the app grows: `protocol` (typed request/response mirrored from
the backend), `ui` (reusable GPUI components), `sidecar` (packaging).

---

## 5. Milestones

- ✅ **P6.0 — Spike doc + skeleton.** Plan + Cargo workspace + a root view sketch
  + a sidecar-supervisor stub. Authored without a Rust toolchain, so unverified.
- ✅ **P6.1 — "Hello workbench" (go/no-go).** Pin `gpui`, get **one window** on
  screen, reconcile `main.rs` against the pinned API. *Kill-criterion R1 — passed;
  see §8.* (The command palette slipped to P6.3 — the gate was the window.)
- ✅ **P6.2 — Talk to the real backend.** `BackendSupervisor` spawns the Python
  sidecar, health-checks it, and streams **one real coordinator turn** end to end,
  rendering assistant text as it arrives. *Verified on Windows 2026-07-30 — the
  coordinator answered in the chat pane, status `done`.*
- 🔴 **P6.2.5 — Local-first backend** *(new; critical path — §10/§11).* Replace the
  remote LangSmith sandbox with host execution (`LocalShellBackend`) behind
  `MINIME_EXECUTION_BACKEND`, add the ~6 bespoke methods deepagents lacks
  (`aget_work_dir`, `aexecute_untruncated`, the lifecycle quartet,
  `_emit_sandbox_status`), and stop configuring WorkOS/LangSmith. Revisit
  `prompts.py` (path + `python3` rules) and `guardrails.py` (the isolation
  assumption), and gate `execute` with human approval.
  *Acceptance:* a real turn — including an `asta` subagent call — completes with
  **no `LANGSMITH_API_KEY` and no `WORKOS_*`**, executing on the host.
- 🟡 **P6.3 — Port the core panels.** In progress, in this order:
  1. ✅ **Composer + transcript scroll** — a real text field (type, Enter sends)
     and a scrollable transcript. §12.
  2. ✅ **Project spine** — the right panel now renders live `GET /project` data
     (mission, completed, pending, suggestions) instead of a hardcoded string. It
     refreshes on launch and after every turn, since the mission is derived from
     the first question. Clicking a suggestion **loads its prompt into the
     composer** — it never runs it, keeping the human gate. The headless
     `--check-backend` now covers this route too, so a decode regression shows up
     as a failed check rather than a silently empty panel.
  3. ✅ **Artifacts/Outputs** — an OUTPUTS section under the spine, fed by the
     `values` stream event so it fills in *during* a turn. Buckets come from the
     live payload: `datasets, sources, reports, files, hypotheses, libraries,
     analyses` (`edges` is graph wiring, not an output, so it's hidden). Each shows
     a count plus up to four titles, then "+N more" — a literature search can
     return dozens. Labels fall back through `title → name → filename → label →
     question → id`, and an unlabelled item is still *counted* rather than dropped.
     *Two corrections to what this plan previously assumed:* the state key is
     `files`, not `todos`; and **`GET /files/{thread_id}` is a download route**
     (it 400s with `missing 'path' query param`), not a listing — so artifacts come
     from the stream, not that route.
  4. ✅ **`sandbox_status`** from `custom` events now drives the status line
     (`Creating sandbox… → Sandbox ready`). This matters because the first turn on a
     cold thread blocks on that provisioning, and without it the UI looks stuck.
  5. ✅ **Command palette** — Zed-style `ctrl-p`/`cmd-p` (§17): a ranked, filterable
     list of seven commands over the workbench. Building it surfaced a real defect —
     "New thread" was meaningless because *every* turn created a new thread — so
     conversation continuity landed with it.
  6. ✅ **Agent activity trace** (§15/§15c) — a delegated turn is no longer silent.
     `stream_subgraphs: true` is now requested, subagent frames are attributed by
     namespace and named from `lc_agent_name`, and the transcript shows the
     coordinator's delegation plus a collapsible group per subagent with its tool
     calls and streamed text. *Verified live 2026-07-31.*
- ✅ **P6.4a — Settings panel + keychain secrets** (§20/§22/§22b). `ctrl-,`, two stores
  (`settings.toml` for settings, the OS keychain for keys), secrets delivered to the
  sidecar as environment variables so the checkout's `.env` becomes optional, and a
  first-run panel instead of a failed turn. *Gates the installable.*
- ⬜ **P6.4b — Native affordances + shipping.** Local file → analysis,
  background-run tray + notifications, **keychain-stored keys**, multi-window.
  Plus what "installable" now means: a **pinned Mini-Me checkout + venv the app
  provisions**, a **"click to update"** button, a **setup tutorial**, and Windows
  process-tree teardown via a Job Object (§9). Not a notarized public installer —
  a guided local install for colleagues.

**MVP acceptance:** a launchable app that opens a project, runs a real coordinator
turn against the local sidecar, streams the answer, renders the artifacts/spine
panels, and does **one** thing the web app can't (local file → analysis, or a
background-run notification).

---

## 6. Decisions

**Locked (2026-07-29):**

- **Repo shape:** ✅ **separate repo** — `mini-me-desktop`, this one. Published
  private at `CENTRO-INTERNACIONAL-DE-LA-PAPA/mini-me-desktop` (2026-07-29).
- **Backend locality:** ✅ **local sidecar** — the client spawns the Python backend
  on localhost. (Nuance found in P6.2: this removes the web app's paste-a-token
  dance, but the backend forwards a *pre-minted* `ASTA_TOKEN` rather than
  refreshing live — see §9.)
- **UI framework:** ✅ **Rust on GPUI**, pinned to published **`gpui = "=0.2.2"`**
  from crates.io — *not* a Zed monorepo `git` rev (§8). Tauri remains the
  documented fallback but was not needed.
- **Agents stay Python/TS:** ✅ no agent code is rewritten; the desktop app is
  another client of the existing HTTP/SSE protocol.
- **Where the app + sidecar run:** ✅ **co-located on Linux** for development
  (the checkout, `.env`, and `asta` CLI live there). The app itself also builds
  and runs on Windows.

**Locked (2026-07-30):**

- **Product shape:** ✅ **local-first, single-user**; not a hosted service, not
  production. This is the premise the rest follows from.
- **Execution locality:** ✅ **local host execution** (§10), proven to be the only
  blocker to dropping LangSmith (§11).
- **Hosted services:** ✅ **drop WorkOS and LangSmith** (sandbox + tracing).
- **Mini-Me:** ✅ **bundled upstream, pinned, unmodified — not forked.** Desktop
  needs are met through one opt-in seam.
- **Secrets:** ✅ the user's own keys, in the **OS keychain**, with a setup tutorial.

**Locked (2026-07-31):**

- **Execution default:** ✅ **host, with `execute` human-gated** (§19). The remote
  sandbox stays reachable via `--sandbox` but nothing uses it by default.
- **Where the §10 change lands:** ✅ **a `PYTHONPATH` overlay in this repo**
  (`overlay/`), not a PR or a fork — the checkout stays byte-for-byte upstream, which
  is what "bundled, pinned, unmodified" asks for. An upstream seam remains the nicer
  destination and the code would move across almost verbatim (§18).

**Open:**

- **Approval fatigue:** whether a long analysis holding a dozen commands is tolerable.
  If not, the answer is remembered decisions, not removing the gate (§19).
- **Human-gating `execute`:** approval UX for host commands (policy + design).
- **Rust capacity:** an organizational gate (R6) — sustained Rust availability.
- **`asta` version pinning on the host:** the sandbox pinned `v0.101.0`; the dev
  box has `0.101.1`. Needs a version check at startup.

---

## 7. Build & run

**Prereqs (Linux / Ubuntu 22.04):** rustup + the stable toolchain, plus the GPUI
system dev headers:

```bash
sudo apt-get install -y libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
                        libasound2-dev libvulkan-dev
```

(Already on the dev box: `libx11`/`libxcb`/`fontconfig`/`freetype`/`openssl`/`zlib`
plus a C toolchain + `cmake`. `protoc` is **not** required — we depend only on
`package = "gpui"`, not Zed's proto crates.)

```bash
cd mini-me-desktop
cargo build -p mini-me-desktop-app   # verified green: rustc 1.97.1, gpui 0.2.2
cargo run   -p mini-me-desktop-app   # opens the workbench window (needs a display)
```

`cargo build` is confirmed working (P6.1). `cargo run` must be launched from a
graphical session (Wayland/X11 + a Vulkan device) — it cannot open a window from a
headless TTY. On **Windows** GPUI renders via DirectX, so no Vulkan/Wayland is
needed and `cargo build && cargo run` works natively.

### Backend prerequisites (the sidecar)

The app spawns the Mini-Me Python backend, so that checkout must be able to serve:

```bash
git clone <Mini-Me>            # then, inside it:
uv sync --extra dev            # NOT plain `uv sync` — see below
```

**`--extra dev` is required.** The LangGraph *CLI* lives in an optional extra
(`langgraph-cli[inmem]` under `[project.optional-dependencies] dev`), which plain
`uv sync` skips. You then get the server libraries but **no `langgraph` entry
point**, and both `langgraph dev` and `uv run langgraph dev` fail with "program not
found" (hit on Windows 2026-07-30). The supervisor's spawn error now names this fix.

The checkout also needs a populated `.env` — at minimum `OPENAI_API_KEY`, plus
`ASTA_API_KEY` / `ASTA_TOKEN` for Asta features and, **until P6.2.5 lands**,
`LANGSMITH_API_KEY` (§11 explains why the run dies without it).

How the app finds the checkout: `MINIME_BACKEND_DIR` wins; otherwise it tries
`~/Documents/Mini-Me` and `~/Documents/GitHub/Mini-Me` (honouring `USERPROFILE` on
Windows) and then `../Mini-Me`. Related env vars: `MINIME_BACKEND_PORT`,
`MINIME_BACKEND_URL`, and `MINIME_BACKEND_ATTACH_ONLY` (never spawn — talk to a
backend you started yourself).

---

## 8. P6.1 execution log (2026-07-29)

The go/no-go gate. **Outcome: PASS on build.** `cargo build -p mini-me-desktop-app`
succeeds; the visual window-check is the user's remaining step (the build shell is
a headless TTY, so it can compile but not display).

**Key finding — GPUI is published.** The P6.0 assumption ("not on crates.io, must
be a Zed `git` dependency") was wrong. `gpui 0.2.2` is on crates.io (updated
2025-10-22), fully self-contained — no `git`/`path` deps, and its only companions,
`gpui_macros` and `gpui_util`, are published at the same version. We therefore pin
**`gpui = "=0.2.2"`**, which retires most of risk **R1** (no unstable monorepo
`git` dep). This Oct-2025 published snapshot still exposes the classic
`Application::new().run()` entry point, matching the scaffold. Newer Zed revs
(e.g. `v1.13.1` = `00bd72e7838f4b875a913cd112b47a0ebe1ca62b`) have since moved the
entry point into a separate `gpui_platform::application()` crate — kept documented
as the fallback if a newer API is ever needed.

**API reconciliation — zero code changes required.** The P6.0 `main.rs` sketch
compiled against `gpui 0.2.2` unmodified. Cross-checked against the crate's own
`examples/hello_world.rs`:
- `Application::new().run(|cx: &mut App| …)` ✓
- `cx.open_window(WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), .. }, |_, cx| cx.new(|_| …))` ✓
- `Render::render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement` ✓
- `Bounds::centered(None, size(px(w), px(h)), cx)`, `rgb(u32)`, and the
  macro-generated Tailwind-style helpers (`p_*`, `gap_*`, `w`, `h_full`,
  `size_full`, `border_r_1`, `flex_grow`, …) ✓

**Toolchain.** rustc/cargo **1.97.1** (stable), via rustup. Workspace
`rust-toolchain.toml` stays `channel = "stable"`. (gpui's own repo pins 1.95.0, but
building only the `gpui` crate downstream is fine on newer stable.)

**Linux system deps.** Missing on the dev box and installed for the build:
`libwayland-dev`, `libxkbcommon-dev`, `libxkbcommon-x11-dev`, `libasound2-dev`,
`libvulkan-dev`. Already present: X11/xcb/fontconfig/freetype/openssl/zlib + C
toolchain + cmake.

**Build result.** `cargo fetch` resolves the full graph with no conflicts;
`cargo build` finishes green (~1m35s cold on a 32-core box). One benign note: an
upstream future-incompat warning in `proc-macro-error2` (transitive; not our code).
The `BackendSupervisor` dead-code warnings are silenced with a documented
`#![allow(dead_code)]` — it's P6.2 scaffolding, constructed but not yet wired.

**P6.1 CLOSED (2026-07-30).** *(P6.2's log is §9.)* Visual confirmation done: `cargo run` on **Windows**
(GPUI's DirectX backend) opened the three-pane workbench window — orange-accented
rail, chat pane with the two placeholder turns, and the right panel with the
mission + P6.3 note — exactly as designed. Note the run environment: the app
**builds on Linux (headless)** and **runs/renders on a Windows dev machine**
(`C:\Users\LENOVO\…\mini-me-desktop`); Windows is a first-class GPUI target
(DirectX — no Vulkan/Wayland needed). **Go decision: proceed to P6.2.**

---

## 9. P6.2 — talk to the real backend (in progress, 2026-07-30)

**Decision: app + sidecar co-located on Linux**, where the Mini-Me checkout, the
`.env` keys, and the `asta` CLI already live. (The app also builds and runs on
Windows, but the backend/secrets are on the Linux box; keeping them together is
the true local-sidecar shape.) Verified present: `uv 0.9.28`, Python 3.12.2,
`asta 0.101.1`, and `.env` with `OPENAI_API_KEY`, `LANGSMITH_API_KEY`,
`ASTA_API_KEY`, `ASTA_TOKEN`, `DEEP_ATD_RUNTIME_MODE`.

### The protocol, as mapped from the Mini-Me repo

The backend is a **LangGraph server**; the desktop app is just another client of
the protocol the React frontend already speaks. No agent code is duplicated.

| Concern | Contract |
|---|---|
| Launch | `uv run langgraph dev --host 127.0.0.1 --port 2024 --no-reload` (cwd = Mini-Me repo; auto-loads `.env`; no browser) |
| Graph id | `assistant_id = "agent"` (`langgraph.json`) |
| Health | `GET /ok` → `200 {"ok":true}` — the P6.0 stub's guess was right |
| New thread | `POST /threads` `{}` → `{"thread_id": …}` |
| Run | `POST /threads/{id}/runs/stream`, `Accept: text/event-stream`, body `{assistant_id, input:{messages:[{type:"human",content}]}, stream_mode:["messages-tuple"]}` |
| Tokens | SSE `event: messages` → `data: [chunk, meta]`; append `chunk.content` where `chunk.type == "AIMessageChunk"` (content is a string *or* typed blocks) |
| Auth | **none needed in local dev** (`backend/auth.py` admits an unauthenticated `local-user`); the model falls back to `OPENAI_API_KEY` from `.env` |

Deliberate simplification: we leave `stream_subgraphs` off, so only *coordinator*
tokens arrive — no subagent namespaces to filter. Subagent streams, `values`
(state), and `custom` (`sandbox_status`) events are P6.3 material.

> ⚠️ **`messages-tuple`, not `messages`.** Asking for `stream_mode:["messages"]`
> looks right and fails silently with **zero tokens**: the server then takes its
> v1 path and emits `messages/partial` / `messages/complete` frames with a
> different payload shape. Only `messages-tuple` is rewritten into `event:
> messages` frames carrying `[chunk, metadata]` tuples
> (`langgraph_api/stream.py:231-233, 345-350`). Cost us one debugging cycle in
> P6.2; there is now a unit test pinning the request body.

**Correction to the north-star premise.** The "local `asta` auto-refresh" win is
real but indirect: the backend does **not** invoke `asta` per request. It reads a
pre-minted `ASTA_TOKEN` from `.env` (minted locally once via
`asta auth print-token --raw --refresh`) and forwards it to the remote LangSmith
sandbox where subagents actually execute. So locality removes the web app's
paste-a-token dance, but it is not a live token refresh. A plain coordinator turn
needs only `OPENAI_API_KEY`.

### What was built

- **`crates/app/src/protocol.rs`** — typed LangGraph client (`create_thread`,
  `stream_turn`, `is_healthy`) plus an **incremental SSE decoder**. Network chunks
  split anywhere, so bytes are buffered until a `\n\n` (or CRLF) terminator; a
  unit test feeds a stream **one byte at a time** to prove reassembly. 6 tests
  cover byte-split framing, string vs. block `content`, non-assistant chunks,
  subagent-namespaced events, and error events.
- **`crates/app/src/backend.rs`** — the stub became a real supervisor:
  attach-or-spawn (`ensure_running` attaches to an already-running backend
  instead of double-spawning), `/ok` polling that **fails fast if the child
  exits** rather than waiting out the budget, inherited stdio (no pipe to
  deadlock on), and repo-path resolution via `MINIME_BACKEND_DIR` → conventional
  locations. Kills the child on drop.
- **`crates/app/src/sidecar.rs`** — the async↔UI bridge. GPUI has its own
  executor and `reqwest` needs Tokio, so instead of mixing runtimes we keep a
  Tokio runtime here and hand events back over a `futures` channel (which is
  executor-agnostic, so GPUI awaits it directly). The runtime and child outlive
  individual turns, so ending a turn never kills the backend.
- **`crates/app/src/main.rs`** — streaming UI: tokens append live to the
  transcript via `cx.spawn` + `weak.update(…)` + `cx.notify()`, with a status bar
  (backend state / errors / base URL) and a Run button. A text composer is P6.3;
  P6.2 uses one seeded prompt.
- **`--check-backend [--stream | --prompt "…"]`** — a headless self-check that
  exercises spawn → health → thread → stream with **no window**, so the contract is
  testable on a headless machine (and doubles as a debug tool). `--prompt` runs an
  arbitrary turn and reports the activity trace (§15c).
- **`--replay <capture>`** — decodes a saved SSE capture into the transcript it would
  produce. No backend, no window, no tokens (§15c).

**Env overrides:** `MINIME_BACKEND_DIR`, `MINIME_BACKEND_PORT`,
`MINIME_BACKEND_URL`, `MINIME_BACKEND_ATTACH_ONLY`.

### Bugs the live run caught (all fixed)

Running against the real backend — not just compiling — found three defects the
type system could never have:

1. **Orphaned backend.** `uv run langgraph dev` *forks* the real server, so
   `Child::kill()` reaped the wrapper and left `langgraph dev` holding port 2024
   (reparented to init). Fixed two ways: prefer the checkout's
   `.venv/bin/langgraph` entry point (a single process we actually own), and put
   the child in **its own process group**, signalling the whole group (SIGTERM,
   then SIGKILL) on drop.
2. **`std::process::exit` skips destructors** — the `--check-backend` failure path
   leaked the backend for exactly that reason. The sidecar is now dropped
   *before* exiting.
3. **The browser hijack.** `langgraph dev` opens LangSmith Studio by default
   ("🎨 Opening Studio in your browser…"). A client shouldn't seize the user's
   browser: we pass `--no-browser`.

Also: piping the child's stdio to *us* meant the child held our stdout open (and
risked deadlocking on a full pipe buffer). Its logs now go to
`/tmp/mini-me-desktop-backend.log`, which the UI cites in error messages.

### Status: P6.2 backend path VERIFIED end to end (2026-07-30)

`cargo build` green · `cargo test` **7/7** · `cargo clippy` clean (only an
upstream `proc-macro-error2` note). Against the live sidecar:

```
health   : ok (sidecar started)     # spawned the venv binary; healthy in ~2s
thread   : 019fb3cb-4be8-…          # POST /threads
stream   : 75 chunk(s), 423 chars   # a real coordinator turn, streamed
backend check: PASS                 # and no orphaned process afterwards
```

**Remaining for P6.2:** the visual check — `cargo run` in a graphical session and
confirm tokens land in the chat pane live. (Unrelated observation, *not ours to
fix*: the backend prints a non-fatal `yaml.scanner.ScannerError` from a skill
docstring during startup; the server boots fine. Mini-Me is read-only here.)

---

## 10. Execution locality: remote sandbox → local host

**Decided (2026-07-30): go local.** The product is a **local-first, single-user
workbench** — not a hosted service. That makes the remote **LangSmith sandbox**
(and WorkOS auth) infrastructure we neither need nor want: it costs a per-user
API key, a cold start, a 10-minute idle TTL, a 1-concurrent-sandbox free tier, and
it ships the user's files to someone else's VM. Execution moves to the host via
deepagents' `LocalShellBackend`. **This is now on the critical path** — see §11 for
the experiment that proves it is the *only* thing standing in the way.

> **Resolved 2026-07-31 (§18):** it landed as a `PYTHONPATH` overlay in *this* repo.
> The Mini-Me checkout is not modified, so it remains read-only reference in fact and
> not just in intent.

### What the codebase says (read-only audit, 2026-07-30)

The good news: **the seam is narrow and the replacement already exists.**

- Mini-Me depends on `deepagents 0.6.1`, which **already ships**
  `LocalShellBackend(root_dir=…, virtual_mode=True)`. Critically it subclasses
  `FilesystemBackend` *and* `SandboxBackendProtocol`, so `supports_execution`
  stays true and the `execute` tool is **not** stripped from the agent/subagents.
  Its `virtual_mode` path-rooting is almost exactly the semantics
  `sandbox.py`'s `_resolve_for_read/_write` hand-rolls today.
- The LangSmith SDK is imported in **one** module (`backend/sandbox.py`), and the
  injection point is **~3 lines**: `agent.py:86` (construct), `routes/common.py:41`
  (HTTP routes), with `runtime.py:50`'s ContextVar deliberately typed `Any`.
- Every tool module is already **duck-typed** against the backend surface
  (`getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute`), and the
  test suite already substitutes fake sandboxes — so a swap is a proven pattern.
- `/skills/` and `/memories/` are already routed to `StoreBackend` via
  `CompositeBackend`; only the `default` route is the sandbox.
- Report rendering (`pypandoc` + `typst`) **already runs host-side**.

The tail that would have to be written — deepagents has no equivalent: a
`aget_work_dir()` (7 call sites, trivially `root_dir`), `aexecute_untruncated()`
(without it the ~500 KB theorizer record gets clipped to unparseable JSON), the
lifecycle quartet `aresolve`/`try_resolve`/`aresume`/`adelete` (locally mostly
no-ops or `mkdir`/`rmtree`), and `_emit_sandbox_status` (emit `ready` at once, or
the UI waits on a state that never comes). Keep the output-truncation cap — it
protects the UI from verbose PyMC/sklearn output, and is *not* sandbox-specific.

### The trade

**Wins (real, and aligned with why we went desktop):** no cold-start
provisioning; no LangSmith dependency for the filesystem (only for tracing); no
free-tier **1-concurrent-sandbox** limit; no 10-min idle TTL; and true local files
— the "no upload dance" promise.

**Costs:**

1. **Isolation disappears.** `guardrails.py` states the current design *relies on
   sandbox isolation* for the execution backend. `virtual_mode` constrains the
   filesystem *tools*; it does **not** constrain what a shell command the model
   wrote can reach. For a desktop app running the user's own code on the user's
   own machine that may be an acceptable trade — but it is a **product decision**,
   and `guardrails.py` plus CIP's human-gated policy must be revisited with it
   (deepagents explicitly recommends HITL for this backend).
2. **Host prerequisites.** `asta` must be on PATH at the pinned version (the
   snapshot pins `v0.101.0`; the dev box has `0.101.1`), plus a `python3` with the
   numerical stack — note that's a *different* interpreter from the backend venv
   unless we deliberately point `env`/PATH at one. Natural move: reuse the backend
   venv, which already carries most of those deps (and would retire
   `build_sandbox_snapshot.py`'s duplicate manifest).
3. **Platform assumptions.** Prompts instruct the model that `python3` exists and
   `python` doesn't — inverted on Windows. `als`/`aglob` shell out to GNU
   `find -printf`, which BSD/macOS `find` lacks. A local backend on Windows/macOS
   needs those revisited; the remote sandbox hid all of it.

### Recommended shape (if approved)

A **factory, not a replacement** — keep both paths behind
`MINIME_EXECUTION_BACKEND=local|langsmith`:

```
backend/execution.py  ->  LazyLangsmithSandbox(thread_id)                    # default, unchanged
                      ->  LocalWorkspaceBackend(LocalShellBackend)           # root_dir=<app_data>/threads/<id>,
                                                                             # virtual_mode=True, env={ASTA_TOKEN}
```

Wire it at `agent.py:86` + `routes/common.py:41` and **touch nothing else** —
`mcp_tools.py`, `theory_tools.py`, `datavoyager_tools.py`, `middleware/sync.py`,
`routes/rendering.py` are already duck-typed against the surface. Then revisit
`prompts.py` (path + `python3` rules) and `guardrails.py` (the isolation
assumption).

**Verdict: medium-low code risk, medium-high behavioural risk.** The plumbing is
a small bounded diff; the isolation question is the actual decision. Keeping the
remote sandbox behind a flag makes it reversible and lets the desktop app opt in.

**On isolation, decided:** for a local-first app the user runs on their own
machine against their own files, host execution is the *point*, not a regression —
the same trade Zed, Claude Code, and every local dev tool make. But
`guardrails.py` currently *states* it relies on sandbox isolation, so that
assumption must be rewritten rather than silently invalidated, and the
**human-gated** policy is honoured by putting approval on the `execute` tool
(deepagents recommends HITL here). Local ≠ ungoverned.

---

## 11. Experiment: what actually breaks without LangSmith / WorkOS (2026-07-30)

Rather than argue about the dependency surface, we measured it. A **stripped
overlay** of the backend was assembled in scratch space — every directory
symlinked to the real checkout (which stayed untouched, `git status` clean) plus a
hand-written `.env` containing **only** `OPENAI_API_KEY`, `ASTA_API_KEY`,
`ASTA_TOKEN`, and `LANGSMITH_TRACING=false`. No `LANGSMITH_API_KEY`, no
`WORKOS_*`, and those names were scrubbed from the launching environment too. Then
`--check-backend --stream` ran a real turn against it.

**Result:**

| Layer | Without LangSmith + WorkOS |
|---|---|
| Server boot / graph import | ✅ works |
| `GET /ok` health | ✅ works |
| Auth (`POST /threads`) | ✅ works — unauthenticated `local-user`, thread created |
| Tracing | ✅ fine, silently off |
| **Agent run** | ❌ **fails** |

The failure is precise and singular:

```
SandboxSyncMiddleware.before_agent
  -> sandbox.aget_work_dir()  (backend/sandbox.py:259)
  -> aresolve()               (backend/sandbox.py:161)
  -> langsmith client.get_sandbox(...)
  -> SandboxAuthenticationError: 401 Unauthorized
     https://api.smith.langchain.com/v2/sandboxes/boxes/minime-<thread-id>
```

**Conclusions.**

1. **WorkOS is already droppable — zero code change.** Local mode never
   authenticates; `auth.py` admits `local-user`. `vault.py` (WorkOS Vault for
   storing user keys) simply goes unused when keys come from the environment or
   the OS keychain.
2. **LangSmith *tracing* is droppable — one flag.** `LANGSMITH_API_KEY` appears
   **nowhere** in backend code; it is purely SDK-implicit.
3. **The LangSmith *sandbox* is the single hard blocker**, and it fails *before
   the agent even starts* (in `before_agent` middleware) — so nothing works
   partially. Replace the execution backend (§10) and LangSmith drops out
   entirely.
4. **Two externals remain by nature:** the **Asta** API/CLI and the **model API**.
   Those are the product, not infrastructure. The honest privacy claim is
   therefore *"no infrastructure services, and your files never leave your
   machine"* — not "no network".

This is the whole justification for §10, measured rather than assumed.

---

## 12. P6.3 step 1: the composer (2026-07-30)

Until now the app could only send one hardcoded prompt — the gap between a demo
and a tool. It now has a real text field: type, press **Enter**, the turn streams.

**Why this was the expensive step.** GPUI ships **no text-input widget** — only
primitives (focus, key actions, IME plumbing, `shape_line`). Its own
`examples/input.rs` is **746 lines**, because an input means cursor motion,
selection, clipboard, grapheme-aware boundaries, IME pre-edit, *and* a custom
`Element` that lays out the line and paints the caret. We adapted that example
into `crates/app/src/composer.rs` rather than hand-rolling a lesser one (decision
taken 2026-07-30). It is Apache-2.0, same as `gpui`; attribution is in `NOTICE`.

**Changes from upstream:**

- **Enter submits**, emitting `ComposerEvent::Submit(text)`; the parent view
  decides that means "run a coordinator turn". Empty/whitespace input is ignored.
- **Cross-platform bindings** — `ctrl-a/c/v/x` as well as `cmd-`; the example is
  mac-only and our primary dev machine is Windows. Bindings are scoped to a
  `Composer` key context so `enter` doesn't leak into other surfaces.
- **A disabled state** — the field is read-only while a turn is in flight.
- **No let-chains** — the example uses them; they need edition 2024, we're on 2021.
- Dark-theme placeholder, accent-coloured caret.

**Also in this step:**

- **The transcript scrolls** (`id` + `overflow_y_scroll`) — previously long
  conversations just ran off the bottom.
- **Empty assistant turns are dropped.** A failed run used to leave a blank
  `you`/`mini-me` pair in the transcript (visible in the P6.2 Windows screenshot).

**Known limitation:** single-line by design. `shape_line` lays out one line, so
soft wrap and `shift-enter` for a newline need a different layout path — deferred.

**Verified:** builds clean, clippy clean, and a real turn still streams
end to end headlessly (102 chunks). Typing itself needs a human at a window.

---

## 13. Windows is the target — what that costs P6.2.5 (2026-07-30)

~98% of our users run Windows. Linux is where we develop; Windows is where the
product lives. This reorders the local-execution work (§10) rather than the UI.

**The problem.** `LocalShellBackend.execute` runs `subprocess.run(..., shell=True)`,
which on Windows is **`cmd.exe`**. Mini-Me's tool layer builds **POSIX** command
strings — `cmd >/dev/null 2>&1; cat /tmp/…`, `… | python3 -c <reducer>` — and its
prompts instruct the model that `python3` exists and `python` does not. On Windows
all of that is wrong. The remote sandbox has been hiding it, because the sandbox is
Linux no matter what the client runs.

So "move execution to the host" is **not** platform-neutral: done naively it works
on our dev Linux box and fails for essentially every real user.

**The options, honestly:**

| Option | Cost | Consequence |
|---|---|---|
| **WSL2 runs the backend** (app stays native Windows, talks to `127.0.0.1:2024`) | An extra install step per user; WSL2 must be enabled | Real Linux userspace, so `bash`/`python3`/`asta` all behave; keeps the local-first story intact. Localhost forwarding makes the client unchanged. |
| **Keep the remote sandbox on Windows** | None | No install pain, but LangSmith stays, and the "no infrastructure, files never leave your machine" claim dies for 98% of users. |
| **Make the tool layer shell-agnostic** (upstream) | Largest change: rewrite the POSIX command construction in `theory_tools.py`, `datavoyager_tools.py`, prompts | The only option that makes native Windows a first-class execution host. Best long-term, most work, and it is upstream code. |

Note one thing that got *easier*: `LocalShellBackend`'s `ls`/`glob` are pure Python
(`rglob`) and `grep` falls back to Python when the binary is absent — so the GNU
`find -printf` shims in `sandbox.py` do **not** need porting. The shell is the
remaining problem, not file operations.

**Decided 2026-07-30: WSL2.** Confirmed available on the target machine
(`wsl --status` → default distro Ubuntu, version 2).

**Why WSL2 won.** The decisive argument isn't that it dodges `cmd.exe` — it's that
**inside WSL the backend simply *is* on Linux**, so the §10 local-execution design
works exactly as written, with **zero upstream changes to Mini-Me's tool layer**.
That matters because "bundle upstream, never fork" is a locked decision: WSL2
shrinks P6.2.5 from "rewrite the tool layer's shell handling" to "swap one backend
class". The client↔backend boundary is HTTP on localhost — which WSL2 forwards — so
the backend's OS is genuinely an implementation detail. Two bonuses: `uv sync` of
the PyMC/scikit-learn stack is far more reliable on Linux than through MSVC on
Windows (a support burden avoided), and it's the same environment we develop in.

**Why not the others.** *Remote sandbox* was ruled out as the primary path because
it needs a **LangSmith API key per user** — we cannot ship ours, so every scientist
would register an account on a free tier allowing one concurrent sandbox; worse
onboarding than WSL2 *and* it contradicts the privacy premise. It stays as a
documented fallback for machines where WSL2 is blocked by IT. *Shell-agnostic tool
layer* remains the best end state but is large upstream work that still can't fully
succeed, since the **model** writes shell commands at runtime — closing that would
mean constraining execution to Python-only. Worth revisiting later, incrementally.

**Implemented (client side).** `MINIME_BACKEND_WSL=1` (or a distro name) launches
the sidecar via `wsl.exe [-d <distro>] -- bash -lc "cd <dir> && exec
.venv/bin/langgraph dev --host 0.0.0.0 …"`, with `MINIME_BACKEND_WSL_DIR`
(default `~/Mini-Me`) giving the checkout path *inside* the distro. Details that
matter:

- **`--host 0.0.0.0`**, not loopback: WSL2's localhost forwarding reliably reaches
  services bound to all interfaces; loopback-only binds are not always visible.
- **`exec`** so the login shell is *replaced* by the server — otherwise killing our
  child leaves the real process running.
- **Teardown also runs `pkill -f "langgraph dev"` inside the distro**, because
  killing `wsl.exe` does not reliably reap the Linux process it fronted.
- The repo-layout check is skipped in WSL mode (we can't cheaply stat the distro's
  filesystem from Windows), and `current_dir` is *not* set — pointing `wsl.exe` at
  a host path is meaningless and would fail the spawn if it didn't exist.

`scripts/setup-wsl.sh` provisions the distro (uv, clone, `uv sync --extra dev`,
`.env` template); it is idempotent and never overwrites an existing checkout or
`.env`.

**Accepted wrinkle:** Windows files reach the backend as `/mnt/c/...`, so the
"drop a CSV, no upload dance" flow needs host→WSL path translation. That is the
P6.4 *local file → analysis* seam, ~10 lines, but it must be designed rather than
discovered.

**Still to verify on Windows (cannot be tested from the Linux dev box):** that
`wsl.exe` spawning works end to end, that localhost forwarding reaches the server,
and that teardown leaves no process behind.

---

## 14. Async subagents (P6.5) — and the sidecar-lifetime question they force

Evaluated 2026-07-30 against LangChain's
[async subagents](https://docs.langchain.com/oss/python/deepagents/async-subagents)
and [interpreters](https://docs.langchain.com/oss/python/deepagents/interpreters)
docs plus the live Mini-Me code. **Verdict: adopt async subagents as P6.5; skip
interpreters.**

### Where we actually are today (the premise, corrected)

The stack is *not* uniformly synchronous. The two genuinely long jobs already
don't block a chat turn — they submit and return, and the **client** polls:

- `hypothesis_generator` (theorizer) and `data_voyager` (DataVoyager) run
  `asta … --no-wait`, return `task_id` + `status="running"`, park the id in graph
  state, and the frontend polls `/theorizer/{thread}/{task}` and
  `/analyze-data/{thread}/{task}` until terminal. DataVoyager's own docstring:
  *"20–40 min for multi-step modelling, so — exactly like the theorizer — Mini-Me
  does NOT block a chat turn on it."*

So the worst case was hand-solved with bespoke plumbing. What **does** block is the
other eight subagents — `data_cleaning`, `exploratory_data_analysis`,
`diagnostic_analytics`, `predictive_analytics`, `report_writer`,
`academic_researcher`, `dataverse_explorer`, `pdf_librarian`. Seconds to minutes
each, with the conversation frozen throughout.

### Why async subagents fit *this* product

1. **The conversation stays live.** Launch an EDA and keep working — refine the
   mission, chase literature — while it runs. That is what "acceleration" means
   for a workbench.
2. **One mechanism instead of bespoke polling.** `start_async_task`,
   `check_async_task`, `update_async_task`, `cancel_async_task`,
   `list_async_tasks`, with task metadata in a dedicated `async_tasks` state
   channel that survives summarization. Long term this could retire the custom
   poll routes and per-tool poll code.
3. **`list_async_tasks` is the data model for a Jobs panel.** Background jobs +
   tray notifications + "close the window and come back" is precisely the native
   affordance we justified this app with (P6.4). This is the feature that makes
   desktop worth the rewrite.

### The four real costs

1. **Preview API.** `deepagents 0.6.1` does export `AsyncSubAgent` /
   `AsyncSubAgentMiddleware` (verified in the installed package), but the docs
   flag it preview: *"APIs may change."* Same class of churn risk we escaped with
   gpui — acceptable only with a pinned version.
2. **Each async subagent must be its own graph** (`graph_id` on an Agent Protocol
   server). Mini-Me declares **one** graph today (`agent` in `langgraph.json`), so
   this is a structural upstream change — in the repo we deliberately do not fork.
   Co-deployed ASGI mode (omit `url`) keeps it in-process with no network hop,
   which is the right starting point for a local sidecar.
3. **Worker starvation — measured, and it applied to us.** `langgraph dev`
   defaults to **one** concurrent job
   (`langgraph_api/cli.py`: `n_jobs_per_worker if … is not None else 1`). Async
   subagent runs are separate runs on separate threads, so with a single slot the
   supervisor's own run holds it and the child run queues — the feature would look
   broken. **Fixed now:** the sidecar launches with `--n-jobs-per-worker 10` on
   both the host and WSL paths. This already pays off for concurrent turns across
   threads/windows, independent of async subagents.
4. **It contradicts our sidecar lifetime.** ⚠️ **The open design question.** The
   supervisor kills the backend when the window closes, so background jobs would
   die with it. "Run in the background" and "the backend is a child of the window"
   are incompatible. Options: let the sidecar outlive the window (detached, with
   adoption on next launch — note the app already health-checks and attaches to a
   running backend, so the machinery half-exists); or keep the current lifetime and
   rely on jobs being resumable by `task_id` after a restart. **Decide before
   building P6.5.** Either way it needs a **Jobs panel** with visible state and
   cancel, so background work stays observably human-gated.

Documented model-discipline failure modes to expect, all mitigated only by prompt
engineering (upstream, fragile): supervisors polling immediately after launch
(turning async back into blocking), truncating `task_id`s, and reporting stale
status instead of re-checking.

### Why not interpreters

Not a rival to async subagents, and a poor fit for us on two counts:

1. **It bypasses the human gate.** *"PTC calls do not go through the normal tool
   calling path. As a result, `interrupt_on` approval workflows are not enforced
   per PTC-invoked tool call."* Our policy is human-gated; a mechanism that fans
   out tool calls around the approval path is a policy problem, not a feature gap.
2. **Wrong runtime for our workload.** QuickJS — JavaScript, 5s default timeout,
   64 MB heap, no filesystem/network. Our compute is pandas/PyMC/scikit-learn in a
   sandbox. The docs themselves scope interpreters to in-memory orchestration and
   point at sandboxes for real execution.

The legitimate use — collapsing multi-step *orchestration* (dedupe/merge/score
across many tool results) into one turn — is real but minor next to (1).

### Sequencing

**P6.5, after P6.2.5.** Building it before the panels and local execution would
stack two unsettled foundations. Prerequisites: pin the deepagents version, answer
the sidecar-lifetime question, and design the Jobs panel.

---

## 15. Agent activity: streaming subagent work and steps (2026-07-30)

**The gap.** The app renders only the coordinator's final text. Ask *"find the
deseq2 paper"* and you get a long silence, then an answer — while underneath, a
subagent ran a literature search. The web frontend surfaces this; we don't, and the
logic lives in TypeScript, so it has to be ported.

Measured on a real turn (`find the deseq2 paper`, `stream_subgraphs=true`,
718 KB captured), which is what makes this designable rather than guesswork:

| event | count | what it is |
|---|---|---|
| `messages\|tools:<uuid>` | **319** | the subagent's own token stream |
| `messages` | 176 | coordinator tokens + tool-call chunks |
| `updates` | 35 | node-level state changes |
| `updates\|tools:<uuid>` | 8 | subagent node changes |
| `values`, `values\|tools:…` | 6, 5 | state snapshots (already consumed) |
| `custom` | 2 | `sandbox_status` (already consumed) |

**Attribution is clean.** The `messages` tuple's metadata names the subagent
outright:

```json
{ "lc_agent_name": "academic_researcher",
  "checkpoint_ns": "tools:d6c187d3-…",
  "langgraph_node": "model",
  "ls_model_name": "gpt-5.4" }
```

So: namespace `tools:<uuid>` identifies *an invocation*, `lc_agent_name` gives the
*display name*, and `checkpoint_ns` groups a subagent's events together. The
LangChain docs describe the same thing as the `ns` tuple with
`any(s.startswith("tools:"))`.

**Steps are derivable from tool-call chunks.** Coordinator `messages` chunks carry
`tool_call_chunks`; on this turn: `task` (the deepagents delegation tool) and
`search_paper_by_title`. That yields real step labels — *"delegating to
academic_researcher"*, *"searching papers"* — without inventing anything.

**Two honest findings that shape the design:**

1. **There is no "thinking" channel today.** Every content block on that turn was a
   plain string — no `thinking`/`reasoning` block types. The event-streaming docs
   don't cover reasoning either. So what we can show is *work and steps*, not
   chain-of-thought. If a reasoning-exposing model is configured later, non-text
   content blocks would carry it and the same decoder path can surface it.
2. **Raw `updates` is unusable as UI.** Of the 35 events, almost all are middleware
   plumbing: `PIIMiddleware[email].before_model`, `ModelCallLimitMiddleware.*`,
   `TodoListMiddleware.after_model`, `SkillsMiddleware.before_agent`… Only `model`
   and `tools` are meaningful. The docs make the same point, recommending a filter
   to "interesting nodes". We should not render this stream directly.

### Design (P6.3 step 6)

1. Request `stream_subgraphs: true` — currently off, which is *why* none of the 319
   events reach us. Our SSE decoder already matches the `messages|<ns>` prefix, so
   the transport work is small.
2. Extend `TurnEvent` with `SubagentToken { agent, text }` and `Step { label }`,
   keyed off `lc_agent_name` / `checkpoint_ns` and `tool_call_chunks`.
3. Render an **activity trace** attached to the in-flight assistant turn:
   one collapsible group per subagent (`▸ academic_researcher`), streaming its text
   live, auto-collapsing when the turn completes so the transcript stays readable.
   Steps appear as one-line entries.
4. Keep the coordinator's answer visually primary — the trace is context, not the
   deliverable.
5. Do **not** render `updates`; if step granularity is later wanted beyond tool
   calls, filter to `model`/`tools` explicitly.

**Cost note:** subagent tokens outnumber coordinator tokens roughly 2:1 on a simple
literature lookup. The trace must be cheap to render and collapsible, or a long
research turn will bury the answer.

A full captured stream is kept in the session scratchpad as a decoder fixture, so
this can be built and unit-tested without burning tokens on live runs.

### 15b. How the web frontend does it (read-only audit, 2026-07-31)

Audited the React app to port rather than reinvent. **Four findings change §15's
design.**

**1. The logic is in the SDK, not the app.** `filterSubagentMessages` is not a local
helper — it is an option on `useStream` from `@langchain/react`, implemented inside
`@langchain/langgraph-sdk`. The app supplies ~40 lines of glue
(`ThreadStreamSession.tsx:54-74`); the SDK does namespace parsing, tool-call
correlation and chunk accumulation. **Porting means reimplementing SDK behaviour**,
which is a bigger job than §15 first implied.

**2. The web app *displays* subagent work — we are catching up, not leading.**
Subagent messages are stripped from the main transcript and re-routed into a
per-subagent side channel rendered as live collapsible cards:
`SubagentActivityPanel` (left sidebar) → `SubagentCard` (spinner, status pill, live
subtitle, tool list, markdown result). Chat also gets a one-line
`describeActivity` summary ("Academic Researcher · <task>", "Coordinating N
subagents…").

**3. Attribution: the SDK's path is fragile — ours can be simpler.**
- The SDK attributes `messages` events via `metadata.langgraph_checkpoint_ns`
  (fallback `checkpoint_ns`), splitting on `|` and taking the first `tools:` segment.
  Other modes (`updates`/`values`/`custom`) are attributed by the **event-name
  suffix** instead. Two different paths.
- **The namespace id is a pregel task UUID, *not* the `tool_call_id`.** The SDK
  reconciles them by matching the subgraph's first `HumanMessage` content against
  the `task` tool call's `description` argument — a three-pass heuristic (exact →
  substring → pending-retry) that can mis-attribute when two subagents receive
  identical descriptions in one turn.
- **Our measured shortcut:** the `messages` metadata already carries
  **`lc_agent_name: "academic_researcher"`** (§15). For *displaying* named,
  grouped subagent activity we can key off `lc_agent_name` + `checkpoint_ns` and
  **skip the description-matching heuristic entirely**. We only need the harder
  correlation if we want to tie a card to its originating `task` tool call (for the
  task description and the terminal `ToolMessage`). Prefer the simple path first.

**4. Reasoning is not rendered anywhere, and the extractor silently drops it.**
`messages.ts:37-58` duck-types on the presence of a `text` field with **no `type`
discrimination**, so an Anthropic-style `{type:"thinking", thinking:"…"}` block
yields `""` and disappears. "Thinking…" in the UI is a hardcoded placeholder. The
app never requests `events` mode either. Combined with §15's measurement (all
content blocks were plain strings under `gpt-5.4`), the honest position stands:
**no reasoning is available today**, and if a reasoning-exposing model is
configured, *not* dropping non-text blocks is a place we can exceed the web app.

**Other details worth copying:**

- Effective stream request: `stream_mode` = `messages-tuple`, `values`, `updates`,
  `custom`; `stream_subgraphs: true`; `config.recursionLimit: 10000`. (We were the
  only client running on LangGraph's default limit of 25 — **fixed 2026-07-31**.)
- Subagent registration accepts a tool call only when `name == "task"` **and**
  `args.subagent_type` matches `^[a-zA-Z][a-zA-Z0-9_-]{2,49}$` — a guard against
  half-streamed JSON args. Stored args are upgraded only when the new value is
  *longer*.
- Lifecycle: `pending` (registered from tool call) → `running` (first namespaced
  `updates`) → `complete`/`error` (main-namespace `ToolMessage` matched by
  `tool_call_id`).
- Tool calls pair with results by id; state is `error` if the result errored,
  `completed` if a result exists **or any later AI message exists** (an
  approximation worth deciding on deliberately rather than copying), else `pending`.
- Main transcript filter (`shouldRenderMainMessage`): user/assistant only, non-empty
  text, and excluding `message.name ∈ {academic_researcher, dataverse_explorer,
  data_cleaning, exploratory_data_analysis, diagnostic_analytics,
  predictive_analytics, report_writer}`. **Consequence:** a delegation turn that is
  *purely* tool calls renders as nothing in chat — its visibility comes entirely
  from the subagent panel. That is exactly the silent gap we see today.
- Truncation budgets: result preview 50 000 chars, tool result 480, tool args 200.
- **Theorizer and DataVoyager progress cards are HTTP-polled, not streamed** —
  `GET /theorizer/{thread}/{task}` and `GET /analyze-data/{thread}/{task}` every
  30 s while the artifact is `running`. A stream-only client will never show their
  progress; that needs a polling loop (own milestone, not part of §15).
- Also stream-fed: a todo/plan progress bar from `values.todos`, and the sandbox
  pill from `custom` (we already consume the latter).

*Caveat: this audit read the SDK's compiled `dist/` JavaScript, so names are
minifier-influenced though the logic is intact.*

### 15c. What was built (P6.3 step 6, shipped 2026-07-31)

**The gap is closed.** Ask *"find the deseq2 paper"* and the transcript now reads:

```
mini-me
· delegating to academic_researcher — Find the canonical DESeq2 paper. Return a concise citation…
▾ academic_researcher · 2 steps · 722 chars
    · search_papers_by_relevance
    · get_paper
    The canonical DESeq2 paper is the 2014 Genome Biology article… · 1 sources
Love MI, Huber W, Anders S. 2014. Moderated estimation of fold change …
```

Verified against a live delegating turn (`--check-backend --prompt "find the deseq2
paper"`, 2026-07-31): the delegation, both subagent tool calls and 722 characters of
subagent text all arrived. Note the tools differ from the §15 capture
(`search_papers_by_relevance` + `get_paper` vs `search_paper_by_title`) — the trace
reports what the run actually did, it does not replay a script.

**Where it lives.** `protocol::TurnDecoder` (the decoder), `TurnEvent::Step` /
`TurnEvent::SubagentToken` (the wire→UI contract), `AgentTrace` in `main.rs` (the
per-invocation group), `Workbench::activity_block` (the render).

**Six decisions worth keeping straight:**

1. **Attribution is by SSE event name, not metadata.** A subagent's frames arrive as
   `messages|tools:<uuid>`; the coordinator's arrive as plain `messages`. The
   metadata's own `langgraph_checkpoint_ns` is *not* usable as the discriminator —
   measured, top-level frames carry `model:<uuid>` there, which names a node, not a
   delegation. Display name comes from `lc_agent_name`, so §15b's
   description-matching heuristic was never needed.
2. **The grouping key is the whole namespace.** The JS SDK keys on the *first*
   `tools:` segment; we keep `tools:a|tools:b` intact, so a nested delegation gets
   its own group under its own name instead of being filed under its parent's while
   wearing the inner agent's label.
3. **The decoder had to become stateful.** Only the *first* `tool_call_chunk` of a
   call carries its name and id; later fragments are keyed by `index` alone. The
   `task` delegation's label lives in arguments that arrived across **60 fragments**,
   so `TurnDecoder` accumulates them and announces once — using "does the JSON parse
   yet" as the completeness signal, since the backend leaves `chunk_position` null.
   `subagent_type` is shape-checked (`^[a-zA-Z][a-zA-Z0-9_-]{2,49}$`, as the web
   client does) so a half-streamed value can't become a label.
4. **A subagent's "text" is often not prose — this was the surprise.** On the
   measured turn `academic_researcher` streamed its entire answer as *one JSON
   object* (its structured response, 678 chars over 173 frames). Dumping it would
   show the user a wall of braces, so `summarize_agent_result` lifts `summary` out
   and counts the array fields (`… · 1 sources`). A partial object still streaming,
   or genuine prose, passes through untouched — which incidentally makes the trace
   look alive and then resolve into a sentence.
5. **`values|tools:…` is deliberately ignored.** The subagent's own snapshot carries
   the same artifacts as the coordinator's, three events earlier; consuming both
   would render the outputs twice. `updates` is still not requested at all.
6. **Activity counts as content.** `finish_turn` used to drop an assistant message
   with an empty body. A purely delegated turn *has* an empty body (§15b), so that
   would have thrown away the only record of the work — the condition is now
   `is_silent()`: no text **and** no steps **and** no traces.

**Cost control.** Each group caps its stored text at 4 000 characters, dropping from
the *front* — a trace is a tail-followed log. New groups open expanded (they are what
is happening now) and all collapse when the turn ends, so the answer stays primary.

**Two new verification paths, both free:**

- `--replay <capture>` decodes a saved SSE capture and prints the transcript it would
  produce. No backend, no window, no tokens. The full 718 KB capture lives outside
  the repo at `~/Documents/mini-me-desktop-fixtures/subagent-stream-sample.txt`.
- `crates/app/tests/fixtures/delegated-turn.sse` (50 KB) is that capture reduced to
  fit the repo — middleware `updates` dropped, metadata narrowed to the fields a
  client reads, single-token text frames coalesced, tool results truncated, but
  **every `tool_call_chunks` fragment verbatim**, because that is where the only
  stateful logic lives. It replays to byte-identical output and is asserted by
  `a_real_delegated_turn_produces_one_named_trace_with_its_steps`.
- `--check-backend --prompt "…"` runs any prompt headlessly and prints steps and a
  per-subagent tally, so the trace can be checked on the Linux box where no window
  can open.

**Still not available, and not faked:** no reasoning/thinking channel (every content
block on the measured turns was a plain string), and **no per-subagent completion
signal** — the terminal `task` `ToolMessage` arrives in the *main* namespace and
can't be tied to a namespace without §15b's heuristic, so groups simply collapse when
the turn ends rather than showing a false "done" tick. Theorizer/DataVoyager progress
is still HTTP-polled upstream and remains unimplemented here.

## 16. Rendering markdown — and the visual-layer decision it forces (2026-07-31)

**The gap.** The coordinator writes markdown and we render the source. A real answer
currently reads `Love MI et al. 2014. **Moderated estimation…**` with the asterisks
showing. This is **not cosmetic**: reports, citations and tables *are* the
deliverable of this product, and a citation the user has to mentally de-escape is a
worse artifact than the web app's.

Measured on the DESeq2 turn, the coordinator emitted `**bold**`, `*italic*`, a bare
URL and a hard line break — so the minimum useful set is inline emphasis, inline
code, links, headings, lists, code blocks and tables (report subagents emit tables).

**What GPUI gives us.** `gpui 0.2.2` has the primitives but no markdown element:
`StyledText::with_highlights(Vec<(Range<usize>, HighlightStyle)>)` for inline runs,
and `InteractiveText` for clickable ranges (links). Zed's own markdown crate is not
something we can depend on — it is wired into Zed's internal `ui`/`theme`/`language`
crates. So the block layer (paragraphs, lists, tables, code fences) is ours to write
as GPUI elements either way.

**Two ways to close it, and a genuinely surprising option B:**

| | A — our own renderer | B — adopt `gpui-component` |
|---|---|---|
| Dependency | `markdown = "1.0"` (CommonMark → AST), 1 crate | `gpui-component 0.5.1`, **58k LOC**, 31 required deps (tree-sitter, html5ever, ropey, rust-i18n, lsp-types) |
| Effort | ~250 lines: walk the AST, emit divs + highlight runs | wire in its `TextView` |
| What we get | exactly the subset we need | markdown *plus* tables, theming, `dock` panel layout, notifications, virtual lists, spinners, a full text input |
| What we give up | tables and code highlighting cost extra work | our hand-rolled composer and palette become redundant; we inherit its theme system and its release cadence |

**The surprise:** `gpui-component 0.5.1` (Apache-2.0, Longbridge) depends on
**`gpui = "0.2.2"` — the exact version we pinned**, from crates.io, not a Zed git rev.
So there is no two-incompatible-gpui problem, which is normally what rules these
libraries out. It is a real option, not a fantasy.

**Recommendation: A now, B as a deliberate decision later.** A is proportionate to
the gap, keeps rebuild time low (which the "click to update" story in P6.4 depends on
— every update is a rebuild on the user's machine), and leaves B open. B is worth its
weight only if we decide to buy the *whole* visual layer at once, and that is a
locked-decision-level call for the visuals milestone, not something to slide in
under a markdown ticket.

**Sequencing.** Not part of P6.3. This is the first item of the visuals pass, before
the palette gets prettier and before any theming work — because every other visual
improvement is judged against text that is still showing its asterisks.

## 17. P6.3 step 5: the command palette — and the thread bug it exposed (2026-07-31)

`ctrl-p` / `cmd-p` opens a ranked, filterable command list; `↑↓` moves, `⏎` runs,
`esc` closes, and the status bar carries a `ctrl-p commands` hint because a palette
nobody knows the shortcut for is a palette nobody opens.

**Commands:** Run turn · New thread · Refresh project spine · Expand agent activity ·
Collapse agent activity · Copy last answer · Quit. A closed enum, not a registry of
closures: every command is reachable another way too, so there is nothing dynamic to
register.

**The bug it exposed.** Adding "New thread" made no sense until we looked — because
`run_turn` called `POST /threads` **on every turn**. Each question was its own
conversation, so a follow-up started from nothing. One `Arc<Mutex<Option<String>>>`
in `Sidecar` fixes it: create on first use, reuse after, and `reset_thread()` is what
"New thread" now means. Nothing is deleted server-side; we just stop adding to the
old thread, and the spine is thread-independent so the mission survives.

*Verified live:* `--check-backend --prompt "find the deseq2 paper" --prompt "who is
the first author of that paper?"` → turn 2 answered **"Michael I. Love."** in 5
chunks with no subagent and no re-search, on the same thread id. That answer was
impossible before the fix.

**Three implementation notes worth keeping:**

1. **Ranking, not filtering.** A plain subsequence test is too loose for a palette:
   `nt` also matches "ru**n** **t**urn" and "expa**n**d ac**t**ivity". So matches are
   *scored* — 8 for a word-initial hit, 1 mid-word, +4 for adjacency — and sorted, so
   `nt` puts "New thread" under the cursor without hiding the rest. Declaration order
   breaks ties, which keeps an empty query in a stable, authored order.
2. **The query field is a second `Composer`.** Reusing it gives the palette real text
   editing (selection, clipboard, IME) for nothing. It needed one new flag,
   `submits_empty`: in the chat composer an empty Enter is nothing to send, but in the
   palette Enter means "run the highlighted command" and must fire before anything is
   typed. It is created once and kept, so its subscriptions register once rather than
   per-open.
3. **Focus has to be handed back explicitly.** An entity subscription has no `Window`,
   so activating with Enter can't refocus the composer directly — a `restore_focus`
   flag is settled in `render`, which does have one. Without it, focus would sit on a
   field that is no longer rendered and typing would go nowhere.

**The headless check now runs the real path.** `check()` used to call `stream_turn`
directly with its own thread; it now goes through `run_turn` — the same function the
window uses — so it covers thread reuse rather than just the HTTP surface, and
repeating `--prompt` is how multi-turn continuity gets verified with no window.

## 18. P6.2.5: host execution, shipped as an overlay (2026-07-31)

**Acceptance met.** A real turn ran with **no `LANGSMITH_API_KEY` and no `WORKOS_*`**,
executing on this machine:

```
$ MINIME_BACKEND_DIR=<checkout with those keys stripped> MINIME_EXECUTION_BACKEND=local \
  mini-me-desktop-app --check-backend --prompt "compute the mean of [2,4,6,8] with pandas,
  write it to result.txt…"
status   : Local workspace: …/local-workspaces/019fb99b-…
step     : ls
step     : execute
--- assistant text ---
5.0
$ cat …/019fb99b-…/result.txt
5.0
```

`asta` resolves on `PATH` (0.101.1) with `ASTA_TOKEN` reaching executed commands, so
theory generation, DataVoyager and PDF extraction have what they need.

### The answer to "where does the change land"

**Neither a PR in Mini-Me nor a fork: a `PYTHONPATH` overlay in this repo.**
`overlay/` ships a Python package that the app injects; the checkout stays byte-for-byte
upstream. That is what the locked decision — *"bundled upstream, pinned, unmodified —
not forked"* — actually asks for, and it means a `git pull` in Mini-Me can never
conflict with us.

Mechanism: `PYTHONPATH=overlay/` makes Python auto-import `overlay/sitecustomize.py`,
which registers an import hook; when the backend later imports `backend.sandbox`, the
hook rebinds `LazyLangsmithSandbox`. Both construction sites (`backend/agent.py`,
`backend/routes/common.py`) import that name at *their* module load, so one rebinding
covers both — the "~3 lines" §10 identified, achieved with zero edits.

An import hook rather than a startup `import backend.sandbox`: for a console script
`sys.path[0]` is `.venv/bin`, and it is LangGraph that puts the checkout on the path
later while resolving `langgraph.json`. Hooking the import removes that ordering
guesswork. **The upstream seam is still the nicer destination** — if Mini-Me grows a
real `MINIME_EXECUTION_BACKEND` factory, `workspace.py` moves across almost verbatim
and the hook disappears. This is the bridge, not a rejection of the PR.

### Five things the audit and the live run corrected

1. **The replacement is thinner than §10 thought, for a different reason.** Every `a*`
   method in deepagents' `BackendProtocol` is a *concrete default* that offloads its
   sync twin with `asyncio.to_thread`. So subclassing `LocalShellBackend` inherits the
   whole async surface Mini-Me awaits — nothing to write. (`BaseSandbox`, which
   upstream's sandbox extends, needs only 4 abstract methods and implements file ops
   *in terms of* `execute`; interesting, but a dead end for us.)
2. **`virtual_mode=False`, not `True` as §10 recommended.** Upstream's tools build
   absolute paths from `aget_work_dir()` — `f"{work_dir}/theories/{task_id}"` — pass
   them to the file operations *and* print them in tool output the model then opens
   with executed Python. Both sides must agree on one namespace. Virtual mode re-roots
   only the file operations (deepagents is explicit that it never constrains
   `execute`), so `/workspace/x` would mean two different things. Verified: `awrite`
   then `cat` sees the same file.
3. **Absolute *writes* still get re-rooted.** With `virtual_mode=False` deepagents
   passes absolute paths through, but upstream's `_resolve_for_write` sent anything
   outside the work dir to `<work_dir>/<basename>`. We mirror it, and locally it does
   double duty as the only guardrail the file tools have: `write("/etc/hosts", …)`
   lands harmlessly in the workspace. Reads need nothing — `virtual_mode=False`
   already means "absolute as-is, relative under cwd", which is what upstream's
   `_resolve_for_read` arranged.
4. **`langgraph dev` runs a blocking-call detector, and it failed the first turn.** A
   bare `Path.mkdir` inside an `async def` raised `BlockingError: Blocking call to
   os.mkdir` and aborted the run in `SandboxSyncMiddleware.before_agent`. Every
   filesystem touch in the overlay now goes through `asyncio.to_thread`. Only a live
   run finds this.
5. **The overlay must not follow commands into their child processes.** `PYTHONPATH` is
   inherited, so every command the model ran re-imported `sitecustomize` and its
   startup line landed in the command's stderr — which `execute` merges into the
   output the *model* reads. The command environment now has the overlay stripped out.

Two smaller things worth keeping: `python3` is pointed at the venv interpreter via
`sys.executable`'s directory (the app launches `.venv/bin/langgraph` without
activating, so `python3` would otherwise be a bare system interpreter with no pandas —
this also retires `build_sandbox_snapshot.py`'s duplicate manifest), and truncation is
imported from upstream rather than reimplemented, so the cap behaves identically
(measured: 32 KB cap, 50 KB survives the untruncated path).

### Still open — and why it is not on by default

**Host execution is opt-in; the sandbox remains the default.** Turn it on with
`--local` (or `MINIME_EXECUTION_BACKEND=local`); `--sandbox` forces it back off. The
flags win over the variable on purpose: PowerShell has no `VAR=value cmd` prefix form,
and a `$env:` assignment persists for the whole session — which has already produced one
confusing debugging session on this project. The app logs a warning when host execution
is on, and the status bar says `host (local)` in the accent colour.

Flipping the default is gated on **human-approval for `execute`**. Org policy is
human-gated and deepagents explicitly recommends HITL for this backend; the file tools
have the re-rooting guardrail but `execute` has nothing, and locally that means the
user's own files. That needs `interrupt_on` in the agent plus an approve/reject path in
the Rust client — a new streaming concern (interrupt + resume), so it is its own step
rather than a rider on this one.

Also still true: **`guardrails.py` upstream still states the design relies on sandbox
isolation.** We cannot rewrite it from an overlay, and it should not be silently
invalidated — which is another reason the default stays where it is. And the sandbox
snapshot pins `asta v0.101.0` while this host has `0.101.1`; a startup version check
is still owed.

## 19. Host execution becomes the default, gated by approval (2026-07-31)

**Decided:** local is the default; nothing runs in the remote sandbox unless someone
asks for it with `--sandbox` (or `MINIME_EXECUTION_BACKEND=sandbox`).

What made that safe to do is the other half of this step: **every `execute` call now
stops and asks.** The run pauses, the desktop shows the command verbatim, and nothing
happens until the researcher approves or rejects.

```
step     : execute
approve  : execute — python3 - <<'PY' ⏎ value = 7 * 6 ⏎ … ⏎ PY
--- assistant text ---
42
$ cat …/answer.txt → 42
```

Verified with **no flags set** — default path, no `LANGSMITH_API_KEY`, command held,
approved, run resumed on the same thread.

### The gate

`create_deep_agent(interrupt_on={"execute": {"allowed_decisions": ["approve","reject"]}})`,
plus the same key on every subagent dict — most execution happens *inside* subagents
(data cleaning, EDA, predictive modelling), so gating only the coordinator would leave
the majority of commands unreviewed. Upstream already uses this mechanism
(`diagnostic_analytics` interrupts on `request_diagnostic_context`), so the shape is
Mini-Me's own, not an invention.

`edit` and `respond` are not offered: the client can approve or reject, and advertising a
decision the UI cannot produce would strand the run.

### Two things the measurements changed

1. **The import hook has to target `deepagents`, not `backend.agent`.** LangGraph loads
   the graph module by *file path* (`langgraph.json` → `./backend/agent.py:agent`) via
   `spec_from_file_location`, which bypasses `sys.meta_path` — so a hook on
   `backend.agent` never fires in the real server. This failed silently and looked like
   a working gate: the sandbox patch landed, the approval patch did not, and the command
   ran. `backend/agent.py` does `from deepagents import create_deep_agent`, and *that*
   goes through normal machinery, so patching the package attribute first is what takes
   effect. **A patch that can fail quietly is worse than one that cannot.**
2. **We were already broken on interrupts, before any of this.** Upstream's
   `diagnostic_analytics` gate means a paused run has always been possible — and to our
   client a pause is indistinguishable from a finished stream: the SSE connection simply
   ends. So that subagent's turns died silently with no answer. `TurnOutcome` now
   distinguishes them, and `Sidecar::resume` continues the turn on the same thread.

`__interrupt__` arrives inside the `values` frame we already request, so no new stream
mode was needed. Payload:
`[{"value":{"action_requests":[{"name","args","description"}],"review_configs":[{"action_name","allowed_decisions"}]},"id"}]`,
and the resume body is `{"command":{"resume":{"decisions":[{"type":"approve"}]}}}` — one
decision per held action, in order, which the middleware validates.

### Also added

`MINIME_CAPTURE_SSE=<path>` appends the raw stream to a file. Every wire shape in this
plan was measured that way; now it is a flag instead of a one-off probe, and what it
writes is exactly what `--replay` reads back.

### What this leaves open

- **`guardrails.py` upstream still says the design relies on sandbox isolation.** That
  sentence is now wrong for the default path. It cannot be fixed from an overlay, and it
  is the strongest remaining argument for eventually sending Mini-Me a PR.
- **Approval fatigue is untested.** A long analysis may hold a dozen commands. If it
  becomes tiring in practice the answer is remembered decisions (per-command-shape
  allowlists), not removing the gate — `MINIME_APPROVE_EXECUTE=0` exists but is not a
  recommendation.
- **The gate has never been driven by a human.** Approve/Reject buttons are unverified
  in a window; the headless check auto-approves because it cannot ask anyone.
- `asta` version pinning (sandbox pinned `v0.101.0`, host has `0.101.1`) is still owed.

## 20. Settings panel and secrets (added 2026-07-31, on request)

**This is a prerequisite for the installable, not a nicety.** The whole "click an icon"
goal dies if the first thing a researcher must do is hand-edit a `.env` file inside a WSL
distro. The settings panel is what replaces that.

### What actually has to be collected

Audited from `.env.example` and every `os.getenv` in the backend (read-only, 2026-07-31).
Dropping WorkOS and LangSmith removes most of it:

| | key | why |
|---|---|---|
| **required** | one model provider key — `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GOOGLE_API_KEY`, `MISTRAL_API_KEY`, or a custom OpenAI-compatible base URL | nothing runs without one (`backend/models.py`) |
| **required for research tools** | `ASTA_TOKEN`, `ASTA_API_KEY` | literature search, theorizer, DataVoyager, PDF extraction |
| **no longer needed** | `LANGSMITH_API_KEY`, `WORKOS_CLIENT_ID`, `WORKOS_API_KEY`, `AUTH_ALLOWED_EMAIL_DOMAINS`, `MINIME_SANDBOX_SNAPSHOT` | §11 and §19 removed the need |

Plus the desktop's own, which are *settings, not secrets*: model choice
(`MINIME_DEFAULT_MODEL`), backend port, checkout location, WSL on/off and distro,
execution locality, approval on/off, workspace root.

### The split, and why it matters

**Two stores, deliberately.** Settings go in a plain `settings.toml` under the platform
config dir — readable, diffable, safe to paste into a bug report. Secrets go in the **OS
keychain** (Windows Credential Manager / Secret Service / macOS Keychain) via the
`keyring` crate. A key must never land in a file the user might sync, zip, or attach; and
CIP policy is that credentials stay the user's own, on the user's own machine.

### Providers: upstream already built this for a panel

`backend/models.py`'s table is commented *"provider id (from the panel)"* — the web app
has a model-config panel and the backend takes its keys **per request**, so the desktop
should speak the same contract rather than invent one:

```json
"config": { "configurable": {
  "model_config": {
    "default": "anthropic::claude-sonnet-4-5",
    "subagents": { "data_cleaning": "openai::gpt-4o-mini" },
    "storage_mode": "client"
  },
  "__llm_keys": { "anthropic": { "api_key": "…", "base_url": null } },
  "__is_for_execution__": true
} }
```

Providers: `openai`, `anthropic`, `google`, `mistral`, and **`custom`** — an
OpenAI-compatible endpoint with a mandatory `base_url`, which is how OpenRouter, Groq,
Ollama, vLLM and friends are reached. Model specs are `"provider::model_id"`.

Three consequences that improve the design above:

1. **Model keys never have to become environment variables at all.** They go from the OS
   keychain into the request body, in memory — not into `.env`, not into the sidecar's
   environment, not onto a `wsl.exe` command line. That is a *better* security property
   than the env-var plumbing, and it removes the `WSLENV` concern for these keys entirely.
   `ASTA_TOKEN`/`ASTA_API_KEY` still need the environment, because the `asta` CLI reads
   them when `execute` runs a command — so `WSLENV` applies to those two only.
2. **`storage_mode: "client"` sidesteps the server-side Vault.** Left unset with no inline
   keys, the backend tries a Vault lookup that needs a user identity — i.e. the WorkOS
   world we dropped. Saying "client" explicitly keeps that path dormant.
3. **Per-subagent model overrides are free.** `model_config.subagents` already routes each
   subagent to its own model, so "cheap model for data cleaning, strong model for theory"
   is a panel row, not a feature to build. Worth exposing once the basics work.

Switching provider or model then needs **no sidecar restart** — it is just the next
request's config.

### Getting the Asta credentials into the sidecar without a `.env`

The launcher already injects environment variables (§18/§19), so the same seam carries
the two Asta credentials — which is what makes the checkout's `.env` *optional* and the
install clickable. One wrinkle worth getting right: **secrets must not go on the
`wsl.exe` command line**, where `ps` would show them. WSL's documented mechanism is
`WSLENV` — set the variables on the `wsl.exe` process and list their names in `WSLENV`,
and the distro inherits them. That is the plumbing to use, not the `VAR=… exec …` prefix
the execution flags use.

### Panel design (Zed-shaped)

- `ctrl-,` opens **Settings** as a pane, plus a palette entry. Not a modal dialog — Zed's
  lesson is that settings you can leave open while you work get fixed.
- Sections: **Model** (provider — including a custom OpenAI-compatible endpoint with its
  base URL — plus key and model id) · **Research tools** (Asta) ·
  **Execution** (host/sandbox, approval on/off, workspace root) · **Backend** (checkout,
  port, WSL distro).
- Secret fields are **masked**, show only "set / not set" once stored, and are never
  logged. This needs one new `Composer` capability — a mask mode — which is a small
  addition to what §12 already built.
- A **Test** button per section: for the model key, start the sidecar and run the trivial
  seed turn; for Asta, `asta --version` through the backend. Better a failure the user
  sees here, next to the field, than a cryptic error on their first real question.
- **First-run**: with no model key stored, the app opens Settings instead of letting a
  turn fail against a backend that cannot answer. That is the "setup tutorial" item made
  concrete — a filled-in panel beats a document.

### Interaction with the native-Windows question

Only the delivery detail depends on it: on WSL the keys travel via `WSLENV`, natively they
are plain child-process variables. The panel's UI and both stores are the same either way,
so this is *not* blocked on that probe.

## 21. Native-Windows probe: verdict (2026-07-31)

**Question:** now that host execution is the default, can we drop WSL2 and make the
installer "unzip and run"?

**Answer: not on `cmd.exe`, and not by re-prompting the model — because the POSIX
assumptions are in Mini-Me's *own tool code*, not in what the model writes.**

Evidence (read-only audit):

```python
# backend/theory_tools.py:246-247  — the theorizer
fetch = f"asta generate-theories task {shlex.quote(task_id)} 2>/dev/null"
return f"{fetch} | python3 -c {shlex.quote(_REDUCE_TASK_PY)}"

# backend/datavoyager_tools.py:_export_shell  — DataVoyager artifact export
f"mkdir -p {run_dir} && "
f"asta analyze-data task {tid} > {run_dir}/task.json 2>/dev/null && "
f"asta artifacts --input {run_dir} --output {export_dir} --format md 2>/dev/null"
```

`2>/dev/null`, `|`, `&&`, `mkdir -p`, and **`shlex.quote`** — which emits POSIX
single-quoting that `cmd.exe` passes through literally, quotes included. These are the
two headline research features, so "it mostly works" is not an option.

**What *did* clear up since §13:** the GNU `find -printf` dependency is gone (our
backend's file operations come from deepagents' pure-Python `FilesystemBackend`, not
upstream's shell-based sandbox), and `python3` resolves to the venv interpreter because
the overlay sets `PATH` (§18). So the remaining blocker is narrower than it was — it is
now *only* the shell dialect.

### Three options, and the recommendation

| | viability |
|---|---|
| **`cmd.exe` natively** | ❌ ruled out by the evidence above |
| **WSL2** | ✅ works today, verified end to end (§13, §18, §19) |
| **Native + a POSIX shell** (Git Bash / MSYS2) | 🟡 plausible, untested |

The third is genuinely interesting and cheap to try, because our overlay already owns
`aexecute`: route commands through `bash -lc` instead of letting `subprocess.run(shell=True)`
pick `COMSPEC`, add a `python3` shim next to the venv's `python.exe`, and Git for Windows
is a small silent-installable dependency compared with enabling a Windows feature and
provisioning a distro. The open risk is MSYS path translation — our `aget_work_dir()`
returns `C:\Users\…`, and drive-letter paths inside MSYS bash need testing on real
Windows, which cannot be done from the Linux dev box.

**Recommendation: keep WSL2 as the supported runtime for v1**, and put the effort into
making *provisioning* automatic (`wsl --install`, then `scripts/setup-wsl.sh`) rather than
betting the packaging design on an untested shell. Native-plus-bash stays a documented
follow-up that could simplify the installer later; it is a half-day experiment on a
Windows machine, not a prerequisite.

**Consequence for P6.4b:** the installer's job is a guided first run — detect WSL, offer
`wsl --install`, provision the distro, then hand off to the settings panel (§20). Nothing
about that is blocked by this verdict, which is why it was worth spending a day to get it
before designing the installer rather than after.

## 22. P6.4a part one: settings store, keychain, and the key path (2026-07-31)

**Proven:** a turn ran against a checkout whose `.env` contained **no provider key at
all**. The key came from the OS keychain, the model choice from `settings.toml`, and both
travelled in the run request. That is the mechanism a clickable install needs — nobody has
to edit a file inside a WSL distro to get started.

```
$ mini-me-desktop-app --set-secret llm:openai "sk-…"
llm:openai: stored in the OS keychain
$ mini-me-desktop-app --check-backend --prompt "…"      # .env has no OPENAI_API_KEY
--- assistant text ---
settings path works
```

### What exists now

- **`settings.rs`** — `Settings` (provider, model id, base URL, host execution, approval,
  port) in `settings.toml` under the platform config dir, plus keychain access. Every
  field defaults, so a file from an older build still loads.
- **Two stores, as designed (§20):** settings in plain TOML; keys in the OS keychain.
- **The request contract** — `model_config.default` = `"provider::model_id"`,
  `__llm_keys.<provider> = {api_key, base_url}`, `storage_mode: "client"`,
  `__is_for_execution__: true`. Also sent on **resume**, so a continuation cannot silently
  lose the key mid-turn.
- **`--set-secret NAME [VALUE]`** writes one credential and exits, never echoing the
  value. An empty value forgets it. The panel is the real interface; this is how a headless
  machine gets set up, and it is what made the test above possible.
- **Asta credentials** reach the backend as environment variables (they must — the `asta`
  CLI reads them when `execute` runs), via `WSLENV` in WSL mode rather than the command
  line.
- Settings now drive the port, execution locality and the approval gate, with environment
  variables still winning as the debugging escape hatch.

### Three things worth keeping

1. **`storage_mode` is omitted when there is no key.** Claiming client-only storage with
   nothing to supply would tell the backend to skip its own lookup and then find nothing —
   a confusing failure instead of a working fallback to its environment.
2. **Keychain reads must not happen on a Tokio thread.** The Linux client (zbus) runs its
   own `block_on`, so reading a secret from inside the runtime panics with *"Cannot start a
   runtime from within a runtime"* — which is exactly how the first live run died. Secrets
   are now read once on the main thread, before any runtime exists, and passed in.
3. **No `libdbus-1-dev`.** `keyring`'s default Linux backend needs that plus `pkg-config`;
   the zbus backend (`async-secret-service` + `crypto-rust`) is pure Rust. `cargo build` on
   a fresh machine has to just work.

### Not done yet — the panel itself

This is the plumbing, not the UI. Still to build: the `ctrl-,` Settings pane, masked secret
fields (needs a mask mode on `Composer`), the per-section **Test** button, and the
first-run behaviour of opening Settings instead of letting a turn fail. Until then the CLI
is the only way to store a key, which is fine for us and not fine for a researcher.

**Unverified:** keychain read/write has only been exercised on Linux/zbus. Windows
Credential Manager is the path that actually matters and needs a run on Windows.

### 22b. The Settings pane (2026-07-31)

`ctrl-,` (or the palette's **Settings**) opens Settings in place of the artifacts panel —
a pane, not a modal, so it can be left open while you work.

- **Provider** cycles on click through Anthropic / OpenAI / Google / Mistral / Custom.
  Five options do not need a dropdown, and a dropdown is a widget GPUI has none of.
  Switching also suggests a model that exists for the provider just chosen, rather than
  leaving one that does not.
- **Base URL** only appears for the custom provider, which is the only one that requires it.
- **Secret fields open empty and are masked.** What is in the keychain is never read back
  into the UI; the row says `· stored` or `· not set` instead. Leaving a field blank on
  save keeps what is already there — so changing your model does not mean re-pasting your
  key. Saving clears the field.
- **Toggles** for host execution and the approval gate.
- **Problems are listed before you hit them** — a custom provider with no base URL, a
  missing key — using the same `Settings::problems` the startup log uses.
- **First run opens the pane** with "Add a model key to get started", instead of letting a
  turn fail against a backend with no key.

**Masking is byte-for-byte.** The composer replaces each *byte* of the content with `*`, so
the mask is exactly as long as the text. Cursor and selection are byte offsets into the
string being shaped, and a mask of a different length would put the caret in the wrong
place or panic on a character boundary. Keys are ASCII in practice, so the count is exact.

**What applies when.** The model and key take effect on the **next turn** — the backend
resolves them per request, so `Sidecar::set_model` swaps them behind a lock with no
restart. The port and execution locality are baked into the sidecar's launch command, so
those need a restart, and the pane says so rather than leaving the user to wonder.

**Verified on Windows 2026-07-31:** the pane renders, keys store to **Windows Credential
Manager**, and a turn runs using a key read from there. So the whole point of §20 — a
researcher configuring the app without touching a `.env` inside WSL — works on the target
platform.

**Verified on Windows 2026-07-31 (second pass):** the approve/reject buttons on a held
command, the activity trace's delegation view, and the palette with arrow-key navigation
all work.

**Spanish keyboard verified 2026-07-31:** `¿qué papa es mas resisñente?` typed and
submitted intact — dead-key accents, `ñ` and inverted punctuation all survive the
composer's grapheme handling. That was the last open verification item for P6.3/P6.4a.

### The bug that pass found

**Suggestions vanished when the answer arrived**, so they could not be clicked. Cause: our
client treated every spine payload as authoritative, but upstream recomputes suggestions
opportunistically — `ProjectSpineMiddleware.abefore_agent` derives them from whatever
artifacts the thread has and emits a payload carrying mission and completed work even when
it produces none. Measured: every `values` snapshot in a turn had `suggestions: 0`.

Fixed by distinguishing advisory content from state: a payload without suggestions means
"no new advice", not "the advice is withdrawn", so suggestions survive while mission /
completed / pending still replace. Clicking one now also removes it from the list, since it
is in the composer at that point. **Only a human watching would have caught this** — every
headless check passed throughout, which is worth remembering the next time a panel looks
fine in a test.

*Also noted:* closing the window logs `window not found` and two invalid-window-handle
HRESULTs from GPUI's Windows text-input teardown, after the sidecar has already stopped.
Cosmetic shutdown noise, not a crash — a polish item.

## 23. Markdown rendering (2026-07-31)

The asterisks are gone. `**bold**`, `*italic*`, `` `code` ``, `[text](url)`, `#` headings,
`-`/`1.` lists, fenced code and `---` rules now render; anything else is shown as typed.

**Hand-written, not a parser crate** (option A of §16). GPUI has no Markdown element, so the
block layer had to be built regardless; the inline layer is then a few hundred lines against
a *measured* subset, with no dependency to track. Inline styling uses
`StyledText::with_highlights` — one shaped line per block with ranges carrying the
differences — which is how GPUI wants it, rather than a tree of nested elements.

Four decisions worth keeping:

1. **The user's own text is never reinterpreted.** Only assistant messages go through the
   parser: rewriting someone's asterisks in their own prompt would be presumptuous.
2. **A link keeps its URL beside the text.** Nothing is clickable yet, and dropping the URL
   would lose the DOI — the part of a citation a researcher actually needs.
3. **`snake_case` does not become italics.** `read_file` and `write_file` in one sentence
   would otherwise italicise everything between them, which is a real thing this
   coordinator writes about.
4. **Half-written markup renders as typed.** Text streams in token by token, so the
   transcript is *constantly* showing unclosed markers; an unterminated `**` must not
   swallow the rest of the line or make it disappear.

**The bug the tests caught:** stepping one *byte* past a marker lands inside `á`, and slicing
there panics. Every other branch is safe because it only steps past an ASCII marker, but the
plain-text branch had to advance by a whole character. Spanish text would have crashed the
renderer on the first accented word — worth remembering that ~98% of users type Spanish.

**Not covered:** blockquotes, nested lists, and images. (Tables were on this list and now
render — §27.) Code has no monospace face — no font is bundled — so
it is marked by colour instead, which is honest but not ideal.

**Unverified:** never rendered. Everything here rests on unit tests over measured output.

**Verified on Windows 2026-07-31** — bold, italics, inline code and links all render; tables
are still literal, and the user's own Spanish (`¿qué papa es mas resisñente?`) came through
intact, which is the accented-character path the boundary fix was for. Tables deferred by
agreement rather than by omission.

## 24. P6.4b part one: the Setup pane (2026-07-31)

**The problem.** A machine that was not already provisioned produced
`backend did not become healthy within 120 attempts` in the status bar. That is true, and
it is useless — the real answer is always one of a short list: WSL is not installed, the
checkout is not there, `uv sync --extra dev` was never run, the overlay is unreachable, or
no model key is stored. §21 settled P6.4b's shape as "a guided first run"; this is the part
that does the guiding.

`preflight.rs` asks those questions and returns each answer **with the command that fixes
it**. `ctrl-p → Setup & diagnostics` opens it, `--preflight` prints the same thing
headlessly and exits non-zero, and a turn that fails to *start* now opens the pane instead
of naming a log file (`looks_like_a_setup_failure`, whose marker strings are pinned by a
test because the routing reads them).

### Four things that make the checks trustworthy

1. **Every probe runs where the backend runs.** `BackendConfig::shell_argv` routes through
   `wsl.exe -- bash -lc` or plain `bash -lc`, the same hop as the launch command. Checking
   for `langgraph` on the Windows side would report green for a machine that cannot launch
   anything — a check on the wrong side of that boundary is worse than no check.
2. **WSL is probed by asking the distro to answer**, not by parsing `wsl -l`: that command
   prints **UTF-16LE**, which `from_utf8_lossy` turns into NUL-riddled nonsense. Round
   -tripping `echo ok` through bash also proves a distro is *usable* rather than merely
   registered. For the same reason `wsl.exe`'s own stderr is never displayed.
3. **Nothing can hang.** `Command::output()` has no timeout and a half-installed WSL can
   block rather than fail; probes poll `try_wait` and kill the child at 30s. A setup pane
   that spins forever is worse than the message it replaced.
4. **Failures never cascade.** No runtime means the checks that run *inside* it report
   `Skip`, with the reason naming what they actually wait on.

### The check that exists because the failure is silent

Host execution works by putting `overlay/` on the backend's `PYTHONPATH` so
`sitecustomize` swaps the sandbox class at interpreter startup (§18). If that path is not
reachable from the backend — the repo on a drive the distro has not mounted, a UNC path
`wsl_path` cannot translate — **Python imports nothing and raises nothing**, and the
backend quietly tries the *remote* sandbox instead. The user then sees an authentication
error about a service they thought they had stopped using. Nothing else in the app would
have caught that, so `overlay` is its own row.

### Two real defects found while building it

- **`cd '~/Mini-Me'` does not work.** Quoting suppresses tilde expansion, so bash looks for
  a directory literally named `~`. The launch command had been quoting nothing at all, so a
  configured `MINIME_BACKEND_WSL_DIR` containing a space would have split into a bogus
  command. `quote_path` now quotes only what follows the tilde: `~/'My Repos/Mini-Me'`
  expands *and* survives the space, which `Documents\My Repos\…` makes a real case.
- **A skip that named the wrong dependency.** On a machine with a working shell and no
  checkout, the dependencies row said "the runtime above has to work first" — sending the
  user to check WSL when the checkout was the problem. Caught by running the failure paths
  rather than by a test, then pinned with one.

### Deliberately not done yet

**No "Run" button on the fixes.** These commands clone repositories and ask for admin
rights; firing one from a click with no visible output would be the app's least accountable
moment. Each fix is shown as a copyable command instead. Streaming the runner — with live
output in the pane — is part two, and it is what turns "click to update" from a plan into a
button.

**Verified:** `--preflight` on the Linux dev box reports 5 ok / 1 to fix (no Anthropic key
stored here), and the three failure paths were run by hand — an empty checkout dir, WSL mode
where `wsl.exe` does not exist (which is exactly what Windows-without-WSL looks like), and
`--sandbox`. 59 tests pass. **The pane itself has never been rendered.**

## 25. P6.4b part two: install it for them (2026-08-01)

*"Do all the necessary so it works without complications. Remember that our users don't
know how to code anything."* — that instruction settled several questions that had been
open, and invalidated one thing the plan had assumed.

### The thing that was wrong: a private repo cannot be cloned by a scientist

Provisioning ran `git clone` against
`CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me`, which is **private**. GitHub stopped
accepting account passwords for git in 2021, so what that prompt actually wants is a
**personal access token**. No amount of UI polish makes "create a PAT" a reasonable first
step, and with `stdin` closed — which it must be, or the app hangs on an invisible prompt
— the clone simply fails.

So **the backend travels with the app.** `scripts/bundle-backend.sh` puts a pinned,
unmodified checkout in `vendor/` (gitignored — the locked decision is *bundled, never
forked*, and a vendored copy in git is a fork with extra steps), and
`BackendConfig::setup_script` passes it as `MINIME_BUNDLED_SOURCE`. Whoever prepares a
build needs GitHub access once; nobody else ever does. This also gives "click to update"
its real shape: the backend is version-matched to the app, so updating the app updates the
backend, and no user-side credentials are involved either way.

### Where the checkout lives, and who owns it

`~/.local/share/mini-me-desktop/backend`, **inside the distro** — not in the desktop
repo, and not on `/mnt/c`. WSL2 reaches Windows drives over a 9p mount whose per-file cost
is high, and a Python environment holding the scientific stack is thousands of small files
stat'd on every interpreter start. A venv there is the one placement guaranteed to feel
broken.

More important is **ownership**, now recorded in `settings.toml` and on `BackendConfig`:

| | the app may update it? |
|---|---|
| **Owned** — the app provisioned it | ✅ yes |
| **Adopted** — discovered, or set via `MINIME_BACKEND_*_DIR` | ❌ never |

This is not fussiness. Updating means `git checkout <pin>` + `uv sync`, and the reference
checkout on this developer's own machine has **ten local branches, several live in
worktrees**. Pointing an update button at a directory the app did not create is how you
destroy someone's work. The pane says which case applies, in words, because it changes
what the app is allowed to do to the user's files.

When a checkout *is* discovered, the pane offers **"Use the one I have"** before "Install
Mini-Me" — adopting takes a second and preserves their branches; installing a second copy
costs gigabytes.

### Fixes now run, with their output on screen

`preflight::run_streaming` + `Sidecar::run_fix` spawn the command and stream it line by
line into the pane. Three decisions:

- **Streamed, not buffered.** Provisioning takes minutes. A spinner with no detail is
  exactly the experience this pane exists to replace.
- **stdout and stderr on separate threads.** Reading them in sequence deadlocks the moment
  a chatty child fills the pipe nobody is draining — and `uv` writes its progress to
  stderr, which is most of what there is to watch.
- **`stdin` is null**, so nothing can wait on an invisible prompt, and ANSI codes are
  stripped because GPUI renders escape sequences as the mojibake they are.

A successful fix **re-checks by itself**, so the row the user just fixed turns green
without them having to work out that "Re-check" was the next step.

### The overlay stops depending on the Windows drive

Provisioning copies `overlay/` to `<checkout>/.desktop-overlay`, and the launch command
prefers that copy — decided by the distro's own shell at launch:

```
PYTHONPATH="$(if [ -f ~/'…/.desktop-overlay'/sitecustomize.py; then … else … fi)"
```

Not by probing from Windows: a `wsl.exe` round trip costs seconds on every start, and
there is nowhere to cache the answer that would not go stale the moment the user
re-provisioned. This retires the silent failure §24 built a check for.

### Also fixed

- **The `.env` template is gone.** It told users to paste keys into a file inside a Linux
  distro — which §22 made unnecessary and this instruction makes unacceptable. The script
  writes an intentionally empty `.env` (because `langgraph dev` auto-loads one, and its
  absence made people think they had missed a step) whose entire content explains that
  keys live in the app.
- **Setup is the front door.** Preflight runs on every launch, and the *first* report
  opens Setup when something blocks a turn — outranking the old "no key → Settings",
  because pasting a key into an app that cannot start its backend fixes nothing. Later
  re-checks never steal the pane; the user has seen the state of things by then.
- **A real bug in the script:** `${BASH_SOURCE[0]}` was resolved *after* `cd "$DIR"`, so a
  relative invocation looked for the overlay in the wrong place and silently skipped
  installing it. Found by running the script, not by reading it.

### Verified

The whole loop, on the Linux dev box, with `HOME` and `MINIME_DATA_DIR` redirected to
simulate a fresh machine:

1. `--preflight` → `3 ok · 2 to fix · 1 skipped`, "not installed", with the bundled source
   threaded into the install command.
2. That exact command run → copies from the bundle, discards the source machine's venv,
   installs the overlay, syncs `--extra dev`, confirms `langgraph` exists, exits 0.
3. `--preflight` again → **`5 ok · 1 to fix`**, the overlay now resolving to the
   provisioned copy. The only thing left is the model key, which is a Settings click.

Plus: the `PYTHONPATH` expression exercised in real bash, both branches, with a space in
the path (tilde expansion and quoting interact badly and it had to be checked, not
assumed); and the provisioned overlay confirmed importable. 61 tests pass.

**Not verified:** none of the pane has been rendered, and no Windows path has run — no
WSL on this box. The `wsl.exe --install` fix in particular is written from documentation,
not from a machine.

### Still open

- **Cancelling a running fix.** With `stdin` closed the realistic stalls are network ones,
  and the output makes a stall visible, but there is no button to stop it.
- **A prebuilt binary.** Users still `cargo build`, and `overlay_dir()`/`scripts_dir()` are
  compiled-in paths that assume a checkout. `MINIME_OVERLAY_DIR`/`MINIME_SCRIPTS_DIR`
  already exist for a packaged layout; nothing has been packaged.
- **Windows Job Object teardown** (§9) — still the last item on P6.4b.

### 25b. What rendering it on Windows found (2026-08-01)

The Setup pane ran on Windows and the guided install worked: **5 ok · 1 optional**, the
backend provisioned into `~/.local/share/mini-me-desktop/backend` inside the distro, with
the streamed output ending in "Done — Mini-Me is ready." Three things the screenshot
exposed, none of which a test would have.

**The pane named a different overlay than the launch would use.** It reported
`/mnt/c/Users/…/mini-me-desktop/overlay` while the launch command was already preferring
the copy provisioning had installed inside the distro. A check that reports a different
path from the one in use is worse than no check — it sends anyone debugging to the wrong
file. Both now come from `BackendConfig::overlay_candidates()`, one definition, so they
cannot drift again.

**The Asta CLI is a button now.** `allenai/asta-plugins` is **public** (Apache 2.0) —
unlike Mini-Me — so unlike the backend it needs no credentials and really can be installed
in one click: `uv tool install git+…@v0.101.1 && uv tool update-shell`. Pinned to the
version the Asta plugin itself pins (`skills/asta-cli/SKILL.md`), with the tag verified
against the remote and the install actually run end to end (it produced a working
`asta 0.101.1`). Bump both together — a CLI newer than the skills driving it is how a
subcommand goes missing.

**A PATH hazard that was working by luck.** The app launches the backend with `bash -lc`
— a login shell that is *not* interactive — which reads `~/.profile` and **never**
`~/.bashrc`, because Ubuntu's `.bashrc` returns in its first few lines when `$-` has no
`i`. The setup script had been writing its `~/.local/bin` PATH line only to `.bashrc`,
where the backend could never see it. It worked anyway because Ubuntu's default `.profile`
adds that directory itself. That is luck, and `asta` is precisely the tool that has to be
found on the backend's PATH at execute time, so the script now guarantees it (guarded, so
a distro that already handles it is left alone).

## 26. Shipping it: process teardown and a folder you can send (2026-08-01)

### The Job Object

Windows has no process group to signal, and killing a parent leaves its children running.
`uv run` forks the real server as a grandchild, and `wsl.exe` fronts a process living in
another kernel — both survived `Child::kill`, kept holding port 2024, and made the next
launch attach to a stale backend.

A **Job Object** with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` is the OS-level answer: every
process in the job dies when the last handle closes. Crucially that covers the app
**crashing** — the handle closes with the process, so the kernel cleans up even when no
destructor of ours runs. `taskkill /T` would only work during an orderly shutdown.

**Verified by cross-compiling**, which is worth recording as a technique: the whole crate
cannot be checked for `x86_64-pc-windows-msvc` from Linux (gpui pulls `stacker`, whose
build script needs `windows.h`), but the `job` module can be extracted into a throwaway
crate with only `windows-sys` and checked there. That found two missing feature gates —
`CreateJobObjectW` is behind `Win32_Security`, `JOBOBJECT_EXTENDED_LIMIT_INFORMATION`
behind `Win32_System_Threading` — which would otherwise have been a broken Windows build
discovered by the user. The extraction is scripted against the real file, so what was
checked is what ships.

Known gap: there is a window between spawn and `AssignProcessToJobObject` in which a
grandchild could escape. Closing it needs `CREATE_SUSPENDED`, which
`std::process::Command` does not expose. The child spends its first moments importing
Python, so the race is theoretical.

### A folder, not an installer

`scripts/package.sh` assembles `dist/mini-me-desktop/`: the executable beside `overlay/`,
`scripts/` and `vendor/Mini-Me`. **Deliberately not an MSI** — no code signing, no
notarization. The audience is a few dozen colleagues, and an unsigned installer is a
SmartScreen warning that teaches people to click through security dialogs.

What made this work is `resource()`: look at an env override, then **beside the
executable**, then the compiled-in repo path. Before that, `CARGO_MANIFEST_DIR` was baked
in at build time, so a shipped copy would have hunted for the overlay under a path that
existed only on the build machine — and, because a missing overlay fails *silently*
(§24), quietly fallen back to the remote sandbox.

The script refuses to be quiet about the one thing that would break a colleague's install:
if `vendor/Mini-Me` is absent it says so loudly, because without it the user is asked for
a GitHub token they do not have.

**Verified:** the bundle was copied to an unrelated directory with a fresh `HOME`, and
`--preflight` resolved the overlay, the setup script and the bundled backend **entirely
inside the bundle**, with no path pointing at the source tree. (First attempt reported the
source tree — a stale binary, because `cargo test` had run but `cargo build` had not. That
trap has now cost this project twice.)

## 27. Two debts that would have grown (2026-08-01)

### Tables

Report subagents emit them, and they were rendering as literal pipes — the one Markdown
gap that touched the actual deliverable.

**Recognised by the separator row (`|---|`), never by pipes alone.** This coordinator
writes about shell pipelines and alternatives constantly (`asta search | head -5`,
`main | develop`), and a parser that treated every pipe as a cell boundary would shred
ordinary sentences into columns. That means one line of lookahead, which is the only
structural change the parser needed.

Four decisions:

- **Ragged rows keep every cell they have.** Text streams token by token, so half-written
  tables are on screen constantly; a short row must not make the block vanish, and a long
  one must not be truncated. Column count comes from the widest row.
- **Escaped pipes stay inside their cell** — the agent writes regular expressions and
  shell commands into tables.
- **Cells are still Markdown**, because a bold verdict in a results table is the norm.
- **Equal-width columns.** GPUI has no table layout, and measuring text before shaping is
  not something this app can do honestly. Even columns are predictable; a naive
  proportional split collapses a column to nothing when one cell is long.

Outer pipes are optional, since models emit both GitHub styles.

### Approval fatigue

Every `execute` stopped and asked. In a real analysis that is ten identical dialogs, and
the tenth is not read — it is dismissed. Then neither is the eleventh, which is the one
that mattered. A gate that trains people to click through it is worse than no gate,
because it also carries the appearance of review.

The card now offers **"Approve the rest of this turn"**. Deliberately *not* a persistent
"always allow": that converts one bounded decision into a permanent one, and a stale
allowlist is invisible. One task's remaining commands is a decision someone can hold in
their head, and it expires by itself — `finish_turn` clears it, next to where the pending
request is cleared, so the two cannot drift.

Approved commands still appear in the activity trace. This removes the *interruption*, not
the record.

### A flaky suite, found by running it twice

The packaging test creates a directory beside the test binary and sets `MINIME_OVERLAY_DIR`;
the ownership test redirects `HOME`. `cargo test` runs tests as **threads in one process**,
so those writes changed what every concurrently running test saw. The suite passed with
`--test-threads=1` and failed at random otherwise — worse than a failing test, because it
teaches people to re-run until green.

Fixed with a shared `env_lock` that every environment-touching test takes first, rather
than pinning the whole suite to one thread. Poisoning is recovered from: the guarded data
is `()`, so one panicking test must not cascade into every other. Confirmed by running the
suite five times.

### Deliberately still open

**The multi-line composer.** The field is single-line at the layout level, so this is not a
key binding — it is soft wrap, cursor movement across visual lines, and a growing input
height. Half-implemented text editing is worse than none, and pasted newlines are still
flattened to spaces (`composer.rs`), which is a real if minor loss when someone pastes a
multi-paragraph question. Sized as its own piece of work rather than squeezed in here.

## 28. It ships, and it does the thing the web app can't (2026-08-01)

### Verified on Windows, end to end

`bundle-backend.sh` → `cargo build --release` → `package.sh` → a **21 MB** folder, and the
packaged binary ran a real coordinator turn with the spine populating beside it. That is
P6.4b's core proven on the target platform, not inferred from Linux.

Two corrections to what this plan assumed:

- **Size: 21 MB, not the 1–2 GB estimated.** That figure came from the *debug* binary
  (718 MB); release strips it to 18 MB, and the backend source is 3.5 MB. The bulk lands
  on the user's machine at install time, when `uv sync` builds the environment — which is
  the right place for it, since those wheels are machine-specific anyway.
- **Release builds need `fxc.exe`.** gpui pre-compiles its HLSL shaders only when
  `debug_assertions` is off (`build.rs:259`), so `cargo build` works for months and the
  build that actually ships fails. Its search is `GPUI_FXC_PATH`, then `PATH`, then **one
  hardcoded SDK version** — a different Windows SDK is enough to fail. Build-time only:
  the bytecode is `include!`d into the binary, so nobody receiving the zip needs an SDK.

Two papercuts fixed along the way, both the same shape — *a prompt that cannot be
satisfied*. `git clone --reference <local> <url>` still contacts the remote for refs, so
the bundle asked for a GitHub password despite existing to avoid one; and redirecting
stderr does **not** suppress git's credential prompt, which is written straight to the
terminal, so the update path asked again. `GIT_TERMINAL_PROMPT=0` is the fix for the
second. Git inside WSL has no credential helper at all, which is a third variant, now
documented.

### Local file → analysis

The MVP bar was "one thing the web app can't do" (§5). This is it: **drop a file on the
window** and the question is prepared for you. No upload, no bucket, no copy — the
researcher's data is already on this machine, and that is the entire advantage of being
native.

Three decisions:

- **The path is translated to the backend's view.** On Windows the agent lives inside WSL,
  where `C:\Users\…\yield.csv` is `/mnt/c/Users/…/yield.csv`. A prompt naming the Windows
  path would send it looking for a file that does not exist there, and the researcher
  would have no idea why. `path_for_backend` does this once, and a test asserts no
  backslash survives.
- **Referenced, never copied.** Keeping a scientist's data where they put it is most of
  the point; a copy in a working directory goes stale the moment they edit the original.
- **Loaded into the composer, not sent.** Dropping a file is a clumsy gesture that happens
  by accident, and this is the same rule the suggestion cards already follow — the app
  prepares the question, the person asks it.

Dropping is accepted anywhere on the window rather than on a designated strip: someone
dragging a file has their eyes on the file.

**Unverified:** never dropped anything. `on_drop` is wired to the root and the translation
is tested, but no file has been dragged onto a real window.

## 29. P6.5, redirected: collect the long jobs that were already running (2026-08-01)

§14 planned P6.5 as **deepagents async subagents**. Reading the code before building it
found a blocker and, more importantly, a live defect that mattered more.

### The blocker

Async subagents require **each async subagent to be its own graph** on the Agent Protocol
server. Mini-Me declares exactly one (`agent` in `langgraph.json`), so this is a structural
change to a repo we deliberately do not fork — on top of a **preview API** whose docs say
"APIs may change", and failure modes mitigated only by upstream prompt engineering. That is
three unsettled foundations for a feature whose user-visible payoff is "the conversation
stays live".

### The defect that was worth more than the feature

The two headline research features — the theorizer (5–15 min) and DataVoyager (20–40 min)
— **already** don't block a turn: they submit with `--no-wait`, return a `task_id`, and
leave the **client** to poll. This client never polled. That was not a missing panel:

> `persist_theory_outputs` and `persist_analysis_outputs` are called from the poll route
> and **nowhere else** (`backend/routes/artifacts.py:202,243`).

So a completed run wrote its results **nowhere**, while `prompts.py` instructs the
coordinator that "when a theorizer run completes, its theories are saved to the sandbox" —
and tells it to read them there on a later turn. Both headline features were quietly
losing their output in this client, and the agent was being told otherwise.

Polling therefore is not a display nicety. **It is the only thing that makes a finished run
durable.**

### What was built

The same user-visible payoff async subagents were meant to deliver — background work that
is observable and arrives on its own — using machinery that already exists upstream, with
no fork, no preview API and no new graphs.

- `Job` / `JobKind` decoded from the `values` snapshot, keyed on `task_id`. Fields taken
  from `HypothesisArtifactPayload` / `DataAnalysisArtifactPayload`
  (`backend/schemas.py:353,388`), not guessed.
- `Sidecar::watch_job` polls every **20s** on the Tokio runtime, which **outlives the
  turn**. Terminal states stop it — including `unavailable`, the subtle one: the thread's
  sandbox is gone, so no further poll can ever say anything and looping would burn requests
  forever.
- A **BACKGROUND JOBS** section above OUTPUTS, showing what is running, what it was asked,
  and *how long that kind of job usually takes* — a spinner with no expectation attached is
  indistinguishable from a hang.
- A finished job refreshes the spine, because the route has just written its results into
  the sandbox as it reported them.

Three details worth keeping:

- **Transport failures do not end a watch.** The sidecar may be restarting, or a turn may
  be saturating it; declaring a 40-minute job dead over one refused connection would be
  the worst possible failure.
- **The thread id is re-read on every poll**, not captured — "New thread" changes it, and
  polling the old one asks about a task that thread no longer knows.
- **A job with no `task_id` is never listed.** A completed artifact carries results but no
  id, and showing it as running would leave a spinner nobody could resolve.

### The lifetime question, answered narrowly

§14 flagged "the sidecar dies with the window" as blocking background work. This does not
need it solved: polling runs for as long as the window is open, and the job itself lives on
Asta's hosted service, recoverable by task id. Making the backend outlive the window is a
real design change with real costs (adoption, orphans, a second app instance) and it is not
required to collect a result the user is waiting for. **Deferred, not dodged** — closing the
window mid-job still means nobody persists that run.

**Unverified:** no long job has been run through this. The decode and route construction are
tested against the measured payload shapes, but the poll loop has never watched a real
theorizer run to completion.

## 30. Async subagents, without forking Mini-Me (2026-08-01)

Requested explicitly, with a fork authorised if needed. **It turned out not to be**, and
that is worth recording, because §14 had this filed as a structural upstream change.

### The three facts that made it an extension

1. **`AsyncSubAgent` is a reference, not an agent** — `{name, description, graph_id, url}`
   — and **`url=None` selects the in-process ASGI transport** (verified in
   `deepagents/middleware/async_subagents.py:_ClientCache.get_async`). No second server,
   no port, no credentials. The sync path *does* raise on `url=None`, so this only works
   because our stack is async throughout, which §18 already forced.
2. **`langgraph dev` accepts `--config PATH`**, and the desktop app builds the launch
   command. So extra graphs can be declared from the **client** side.
3. **`backend/agent.py:agent` is an async factory.** A second graph id can point at the
   same factory, so the background worker is a real Mini-Me with every tool and subagent
   it normally has.

### Why a background *coordinator*, not one graph per subagent

The obvious reading of the docs is "one graph per async subagent". Building those would
mean replicating `_build_runtime_subagents` — its MCP tool fetches, model resolution and
per-subagent middleware — inside our overlay. That is exactly the duplicated logic that
becomes merge debt the first time upstream touches it.

Delegating to a background **coordinator** reuses upstream's assembly verbatim, and is
strictly more capable: the worker can chain subagents, run its own analysis and write a
report. It works here specifically because execution is **local** (§19) — the background
worker shares the researcher's filesystem, so files it writes are simply *there*. Under
the remote sandbox each thread gets its own and the results would land somewhere the
user's thread cannot see.

### The pieces

- `overlay/minime_local/async_agents.py` — declares the async subagent (`url=None`),
  builds the background graph from upstream's own factory, and injects
  `AsyncSubAgentMiddleware` by wrapping `create_deep_agent`, the same patch point the
  approval gate uses (§18). Installed *after* approval, so the background worker inherits
  the same gate.
- `overlay/minime_local/make_config.py` — reads upstream's `langgraph.json` and writes
  `.mini-me-desktop.langgraph.json` beside it with the extra graph. **Beside it**, because
  every path in that file is relative to the file. **Extends rather than reconstructs**,
  because it carries `dependencies`, `env` and the `http` block that mounts the spine and
  job-poll routes. **Every launch**, because a copy generated once goes stale after a
  backend update.
- The launch joins them with `&&`, so a generator failure stops the launch instead of
  starting a coordinator whose tools point at a graph nobody serves.

### Two guards

**No recursion.** A background worker built by the same wrapped `create_deep_agent` would
be handed `start_async_task` too, and could spawn another, and so on — a runaway that
bills the user's model key. A `ContextVar` set while the background graph is built
suppresses the injection.

**Off by default.** It rests on a preview API whose docs say "APIs may change", and it
only functions when the generated config is in play. A coordinator holding tools for a
graph the server does not serve fails *mid-task, in front of the user* rather than at
startup, so `MINIME_ASYNC_SUBAGENTS` must be set and the Settings toggle
("Let work run in the background") is opt-in.

**Verified:** the config generator run against the real `langgraph.json` — `auth`,
`dependencies`, `env`, `http` and `python_version` all preserved, upstream's `agent` graph
untouched. The launch command is pinned by a test: generator before server, `--config`
present, and byte-identical to before when the toggle is off. 73 tests.

**Unverified — and this is the big one:** no background task has ever been started. The
wiring is measured but the round trip (`start_async_task` → the worker runs → results come
back) has not been exercised against a live backend.

## 31. Background work you can actually answer (2026-08-01)

Two gaps stood between §30's wiring and background work being usable. The first was not a
missing feature — it was a hang.

### The approval nobody could answer

The overlay wraps **one** `create_deep_agent`, so the background worker inherits the same
`execute` gate as the foreground agent. But the worker runs on its **own thread**, and the
client only ever resumed the conversation's (`sidecar.rs:173,246`).

So the first command a background task tried to run stopped it dead, waiting for a
decision nothing in the app could deliver. It would not have failed or errored — it would
have sat at "running" forever. Every data task (cleaning, EDA, analysis) hits `execute`;
only literature search and writing would have worked at all.

`GET /threads/{id}/state` answers everything needed in one call: its
`tasks[].interrupts[]` carry **exactly** the payload `decode_interrupt` already parses, so
a background approval and a foreground one are the same shape and render the same card.
Status is *derived* rather than reported — an interrupt means waiting, an empty `next`
with no interrupt means done, anything else is working.

Answering goes to `POST /threads/{that}/runs` with `resume_request_body` — the identical
body a foreground resume sends, so a change to the decision shape cannot fix one path and
leave the other broken.

**Not streamed into the transcript.** A background run's tokens are not the answer to
anything the researcher asked in the chat; mixing them in is how "what am I reading?"
happens. The Jobs panel reports it instead.

### Seeing the tasks

`async_tasks` is agent state, so it arrives in every `values` snapshot — no extra route.
Each entry gives `thread_id`, `agent_name`, `status` and the description. Three details:

- **`interrupted` is not terminal.** Treating it as finished would stop the watcher on the
  exact tick that needed a person. Terminal is `success`, `error`, `timeout`, `cancelled`.
- **Sorted by task id.** A map has no order, and the panel would otherwise reshuffle on
  every frame.
- **A stale snapshot never erases a pending approval.** The snapshot knows what the
  coordinator last recorded; the watcher knows what is true now. The card the user is
  looking at wins.

Watched every **4 seconds**, much faster than the 20s Asta job poll, because someone may
be sitting in front of the app waiting to say yes.

**Verified:** decode and terminal-state handling are tested against deepagents' own
`AsyncTask` field names. 75 tests, stable across three runs.

**Unverified:** no background task has been started, so no background approval has ever
been rendered or answered. The shape is measured; the round trip is not.

## 32. The Asta token expires every seven days — so the app mints it (2026-08-01)

**Reported symptom:** the theorizer failing repeatedly with *"The Asta theorizer returned
no task id — likely cause: missing or expired Asta access token"*, on a machine where
`asta auth print-token` worked perfectly.

Both were true. Decoding a real token: `exp - iat` = **604800 seconds — seven days**. So a
token pasted into Settings is a weekly chore, and when it lapses the failure names neither
the token nor the fix. Worse under WSL: being signed in on the *Windows* side proves
nothing, because the backend runs inside the distro.

`asta auth login` already leaves a **refresh** credential behind, and
`asta auth print-token --raw --refresh` turns it into a valid access token on demand. So
the app now mints one **per launch**, and the researcher signs in once.

Three details:

- **At spawn, not at window-open.** This can cost seconds on a cold WSL distro, and by
  then the user is already waiting on a backend start.
- **Shape-checked before use.** Without `--raw` the CLI pretty-prints a decoded header and
  payload; with nobody signed in it prints prose. Handing either to the backend as a
  credential produces an authentication failure that blames the wrong thing, so only a
  three-segment base64url JWT is accepted — asked *about* the value, never logging it.
- **Silent fallback.** No CLI, not signed in, a changed flag — the stored token still
  applies and the Setup pane reports the real problem separately.

The preflight check was upgraded to match: `command -v asta` said *installed*, which is not
*usable*. It now asks the CLI for a token, so an expired login is caught at the pane with a
**Sign in to Asta** button rather than in the middle of a research question.

**Verified:** the mint command run against a real CLI returns a 1015-character
three-segment JWT and nothing else; the non-raw form begins `JWT Header:` and is rejected
by the guard. The pane reports "installed and signed in". 76 tests.

### The gap signing in from the pane exposed

On Windows the **Sign in to Asta** button worked — browser, Auth0, "Authentication
successful", and the pane went to **6 ok**. The theorizer still failed.

Because the token is minted when the backend **starts**, and the backend had been running
since before the sign-in. Every check was green and the thing still did not work, which is
the worst state a diagnostic pane can be in: it was telling the truth about the machine and
the wrong thing about the session.

A successful sign-in now says so in the fix output — *"Close and reopen the app: the
backend reads your Asta sign-in when it starts."*

### The three holes, and why the backend mints its own token now

A restart did not fix it either. Passing the token in as an environment variable had
**three** separate holes, any one of which is enough:

1. **`_command_env()` is called once, in `__init__`.** The environment is a snapshot taken
   when a thread's workspace is built — so a token that arrives later never reaches a
   single command.
2. **`ensure_running` returns early when a backend is already healthy.** The app only
   minted while *spawning*, so attaching to a backend that was already up — including one
   orphaned by a previous session — skipped it entirely.
3. **On Windows it has to survive the crossing into WSL** via `WSLENV`.

`current_asta_token()` in the overlay removes all three: the backend asks the CLI itself,
in the same environment every other `asta` command runs in. If those can authenticate, so
can this. Cached for ten minutes so it is not a subprocess per command, and refreshed from
`_execute_with_token`, which is already inside `asyncio.to_thread` — `langgraph dev`'s
blocking-call guard rejects subprocesses on the event loop, and that guard has aborted a
run in this project before.

An explicitly set `ASTA_TOKEN` still wins, because someone who sets one means it. Anything
that is not JWT-shaped is ignored in both directions.

**Verified against the real CLI:** mints, is JWT-shaped, caches, prefers a supplied token,
and ignores junk in `ASTA_TOKEN` in favour of a minted one.

**Superseded:** have the overlay read the token from a small file at command
time rather than from the process environment, so the app can refresh it into a *running*
backend. `_command_env()` already reads `ASTA_TOKEN` at call time for exactly this kind of
reason — but it runs on the event loop, where `langgraph dev`'s blocking-call guard rejects
filesystem syscalls, so the read has to move somewhere off the loop first. That also fixes
the seven-day expiry landing mid-session rather than between launches.

### Showing who is signed in, and for how long

`asta auth status` reports everything worth surfacing, including — usefully —
**`Auto-Refresh: Enabled`**, which confirms the CLI refreshes its own access token and so
corroborates the design above. The Asta row now reads:

```
✓ Asta CLI    piero.palacios@cipotato.org · token 167h 55m left
```

Two reasons that is worth the parsing:

- **Which account.** On a shared machine, or after someone signs in with a personal
  address by mistake, "signed in" gives no way to work out why permissions look wrong.
- **How long.** Seven days is short enough to matter and long enough to forget.

The **Sign in again** button is offered even when the row is green: when the *refresh*
credential finally lapses — not the access token, which now renews itself — that is the
only cure, and a button that only appears once you are already broken is a button you
cannot find.

The parser splits the Rich table on `│` rather than matching prose, and is used **only to
enrich a row that already passed**, so a change to the CLI's formatting costs a label and
never a check. A test pins it against the real output verbatim, box-drawing and all.

### 32b. It was never the token — it was the account (2026-08-01)

After the minting fix, the theorizer still failed. Decoding the two access tokens the user
had produced, side by side, settled it:

| | `auth0\|69fe…` (cgiar.org) | `google-oauth2\|1142…` (cipotato.org) |
|---|---|---|
| permissions | `access:all_endpoints` | `access:all_endpoints`, `access:biopathways`, `enroll:asta_integration`, **`enroll:theory_generation`** |

The theorizer requires **`enroll:theory_generation`**. The account signed in inside WSL was
the first one. Its token was present, valid, server-verified and **not entitled** — and
upstream reports that as *"no Asta task ID was returned, which usually means the access
token is missing or expired"*.

That message is a guess, and being a *plausible* guess is what made it expensive: it sent
the user to re-authenticate, repeatedly, for something re-authenticating could never fix.
Two rounds of work here — minting the token, then reading it at command time — were both
real improvements aimed at the wrong target.

**The check now reads the claims.** `asta auth print-token` *without* `--raw` prints the
decoded payload, permissions and all, so no JWT decoding of our own is needed. An account
that lacks the permission gets a warning that says so in those words, plus the sign-in
button pointed at the account that has it.

The lesson worth keeping: *"signed in"* was never the question. **Entitled** was. A
diagnostic that reports authentication and calls it authorization will confidently send
someone the wrong way, which is worse than reporting nothing.

### 32c. Opening the sign-in page where the browser actually is

The **Sign in to Asta** button worked, but the real output showed why it was awkward:

```
gio: https://auth0.allenai.org/activate?user_code=DPMW-BJCG: Operation not supported
```

`asta auth login` prints its device-activation URL and then tries to open a browser — from
**inside the distro**, which has none. The sign-in only completed because the user opened
the link by hand.

The pane now catches the URL out of the streamed output and offers **Open the sign-in
page**, plus a copy. The opener deliberately does **not** go through `shell_argv`: routing
it into WSL is precisely what already fails. On Windows it is `cmd /C start "" <url>` —
with the empty title argument, without which `start` treats a quoted URL *as* the title and
opens nothing.

Prominent, and above the log, because while it is showing the command is **blocked**
waiting for someone to visit that page.

**What is deliberately not done: saving the token.** The obvious next step — "log in once,
store the token" — is the thing three rounds of debugging just removed. Access tokens last
seven days; a stored one is stale by definition, and `_command_env()` captured it once per
workspace anyway. The overlay asks the CLI for a fresh token every ten minutes instead
(§32), and the CLI's own `Auto-Refresh: Enabled` does the renewing. There is nothing to
save that would not immediately start rotting.

**The test earned its place immediately:** the first version left the trailing colon on
`gio: <url>:` — a character worth stripping only because the real line has it there.

**And the code gets its own line.** Seen on Windows: the log box showed
`| Visit: https://auth0.allenai.org/activate?user_code=KFDM-BQQG |` **clipped at the pane's
edge**, with the text unselectable. A URL is a single unbreakable word — it cannot wrap at
420px, and what falls off the end is the device code, the one part a person has to read and
type. It is now extracted and shown large on its own line, above the link buttons.

**And the actions moved out of the scroll area.** The buttons were children of the log box
— a flex child, therefore shrinkable — so it squeezed until "Open the sign-in page" was
sliced in half and unreadable. A button you cannot read is worse than no button: the user
can see something is there and cannot use it. The block now holds the header, the code and
the buttons at a fixed size, and only the output lines scroll beneath them.

Three rounds on one small panel, each found only by looking at a screenshot. Rendering is
not something this project can reason its way to from a Linux box.

## 33. The fix that never reached the machine (2026-08-01)

Two rounds of Asta token work — minting it, then reading it at command time — and the
theorizer still failed. Neither had ever run.

**The backend loads the overlay copy inside the distro.** §25 made the launch prefer
`<checkout>/.desktop-overlay`, and that was right: it removed host execution's dependence
on `/mnt/c` being reachable, which fails *silently*. What went unnoticed is that the copy
is made at **provisioning** time. So `git pull` + `cargo build --release` updated the
repo's `overlay/`, the app relaunched, and the backend went on importing a copy from days
earlier. Every overlay change since provisioning was invisible.

This is the worst shape a bug can take: the fix was correct, shipped, and verified on the
dev box, and the user watched the same failure three times. It also quietly invalidated the
verification — "verified against the real CLI" was true of code that was not running.

**Every launch now syncs it.** Three small files, so copying them unconditionally is
cheaper than working out when to. `|| true`, because a stale overlay still beats a backend
that will not start, and the repo's copy may be genuinely unreachable — the case the
in-distro copy exists for. Ordered before the server, and independent of the async-subagent
toggle, which had been gating the only other pre-launch step.

**Verified in real bash:** a stale provisioned copy is replaced with the current one, and
an unreachable source exits 0 so the launch continues. A test pins the ordering and that
the sandbox path stays untouched.

**The general lesson.** Anything the app *installs* onto the user's machine is a second
copy with its own version, and needs a story for how it gets updated. The overlay had none.
Worth checking the others: the generated config regenerates each launch (§30) and the
bundled backend is refreshed by re-provisioning — but `vendor/Mini-Me` inside a shipped
bundle has the same shape of problem, and click-to-update is still unbuilt.

### 33b. And then the fix itself was wrong

With the overlay finally syncing, the code ran — and failed immediately:

```
submit failed: 'LocalWorkspaceBackend' object has no attribute 'env'
```

`_execute_with_token` refreshed `self.env`. deepagents calls it **`self._env`**
(`LocalShellBackend.__init__` builds it; `execute` passes it to the subprocess). Guessed,
not checked — and because the refresh sits on the path *every* command takes, a wrong
attribute name turned "the theorizer has no token" into "nothing executes at all".

Now reached with `getattr(self, "_env", None)` and an `isinstance` check. If a later
deepagents renames it we lose the token refresh, which is a degradation; taking `execute`
down with it is not.

**Verified against the real class**, in the backend's own venv: `_env` exists, `env` does
not, and a token written into it is visible to the command that runs.

Two lessons, both cheap in hindsight. **Private attributes of a pinned dependency are
fair game, but only after looking** — this file already reads upstream internals
deliberately (`_truncate_execute_response`), and each one was checked except this. And
**anything on the universal path needs a failure mode that is a degradation**, because
its blast radius is everything.

## 34. A PATH problem wearing an authentication costume (2026-08-01)

With the overlay finally syncing and the attribute name fixed, the theorizer *still*
reported a missing or expired token — on an account whose token the Setup pane showed as
valid for 167 hours, with `enroll:theory_generation`, and whose exact submit command
returns a task id when run by hand.

**`execute` runs commands through `/bin/sh` with exactly the environment we hand it.** Not
a login shell — so `~/.profile` never runs, and `~/.local/bin` is not added. That is where
`uv tool install` puts the **asta CLI**. If the backend's own PATH happens to lack it,
every `asta` command exits **127, `sh: asta: not found`** — and upstream reports that as
*"no task id was returned, which usually means the access token is missing or expired."*

Proven directly against `LocalShellBackend`:

```
without ~/.local/bin : /bin/sh: 1: asta: not found   (exit 127)
with    ~/.local/bin : asta, version 0.101.1
```

`_command_env()` now puts it on PATH. And the Setup pane could not have caught this: its
probe runs `bash -lc`, a **login** shell, which reads `~/.profile` and finds `asta`
perfectly — a check that passes where the thing being checked would fail.

### The change that should have come first

`_log_failure` now writes any non-zero command, with its output, to the sidecar log.

Tools discard what a command actually printed and substitute their own summary. The
theorizer's is a *guess*, and a plausible one — which is precisely what made it expensive:
it named a cause, so nobody looked further. It sent this project through minting a token,
reading it at command time, syncing the overlay and chasing account entitlements, while
the real message — five words, `sh: asta: not found` — was being thrown away at every
step.

**The lesson worth keeping:** when a component reports a *cause* rather than an *error*,
the first move is to recover the real output, not to act on the guess. Four fixes here were
individually correct and aimed at a diagnosis nobody had verified.

## 35. The stale token that failed silently (2026-08-01)

The sidecar log finally settled it, and the culprit was our own precedence rule.

Reproduced directly against the CLI:

```
ASTA_TOKEN=<valid>   asta generate-theories … --no-wait  →  exit 0, a task id
ASTA_TOKEN=<stale>   asta generate-theories … --no-wait  →  exit 0, EMPTY OUTPUT
```

**The CLI prefers `ASTA_TOKEN` over its own stored credentials, and fails silently when
it is bad** — exit 0, nothing on stdout, nothing on stderr. Upstream then reports "no task
id was returned, which usually means the access token is missing or expired": correct
about the cause, useless about the source. And an exit-0 failure walks straight past the
failure logging added in §34, which is why that produced nothing.

`ASTA_TOKEN` reaches the backend from the **OS keychain**, where a token pasted days
earlier was still sitting. §32 had decided "an explicitly supplied token always wins —
someone who set it meant it". That reads well and is wrong: a value in a keychain from
last week is not an intention, and preferring it silently disabled every Asta tool.

**Inverted.** The CLI is the authority — `asta auth login` leaves a refresh credential and
the CLI renews itself, so a minted token is always at least as good as a stored one. A
supplied value is now tried *only* when nothing can be minted, and says so loudly when it
is used.

### Why this took so long

Six rounds, each a real defect, none of them this one:

| | what was wrong | why it looked right |
|---|---|---|
| §32 | token minted only at spawn | the error said "expired" |
| §32 | read once per workspace | ditto |
| §32b | account lacked `enroll:theory_generation` | two accounts genuinely differed |
| §33 | the overlay copy was months old | the fix was correct, just not running |
| §33b | `self.env` guessed, not checked | crashed loudly, so looked like *the* bug |
| §34 | `~/.local/bin` missing from PATH | reproduced exit 127 exactly |

Every one deserved fixing. But the diagnosis driving them came from a tool that reports a
**guess** as a cause, and a CLI that fails with **exit 0 and no output** — a combination
that defeats both "read the error" and "log the failures". The step that actually worked
was reproducing the command by hand with a deliberately bad token.

**The lesson:** when a component reports a cause rather than an error, do not act on it.
Reproduce the failing call directly, and vary one input at a time — including the ones the
app itself supplies.

### Verified on Windows, end to end (2026-08-01)

`genera una teoria de como se forman los rayos` → the theorizer **submitted**
(`845f8553-499c-4ea8-a3e4-6540101cb39d`), and the **BACKGROUND JOBS** panel showed it
running with its question and expected duration. Two features proved out at once: the
theorizer itself, and §29's job watching — which had never seen a real long job.

Still to observe: the completion. The poll route persists theories into the workspace on a
terminal state (§29), so the job should turn green and the spine refresh **without another
turn**. That last link is the one that was silently broken before any of this.

### 30b. Registered, but never handed over (2026-08-01)

First live test of background work: the coordinator answered *"lanza esto en segundo
plano"* by delegating to `academic_researcher` — the ordinary, **blocking** subagent — and
the chat froze for the whole literature search. Exactly what the feature exists to prevent.

`MINIME_ASYNC_SUBAGENTS` is what `async_agents.install()` checks before adding the
middleware, and **nothing set it**. The graph was registered (the log shows
`Importing graph profiling … graph_id=background`), the config generation worked, and the
coordinator was never given `start_async_task`. So it used the only delegation it had.

Two halves of one feature, and only one of them was wired: a Settings toggle that
generated the config but never enabled the tools. The toggle *looked* like it worked
because the visible half — the extra graph — did.

Set now via `feature_env`, deliberately **not** folded into `execution_env`: that returns
nothing at all for the remote sandbox, so combining them would silently disable background
work under `--sandbox`. The two settings are independent, and are kept that way.

A test asserts the variable is in the launch when the toggle is on and absent when it is
off — registering the graph is only half of it.

## 36. P6.5 verified on Windows (2026-08-01)

**Background work runs, and the conversation stays live.**

- `· start_async_task` fired, control came back immediately, and **two background workers
  ran concurrently** while the researcher kept typing. That is the payoff §14 justified
  going native for, delivered without forking Mini-Me (§30).
- The **Theorizer completed by itself** — `✓ Theorizer · completed` in BACKGROUND JOBS,
  with `sources · 11` and `hypotheses · 1` in OUTPUTS, no second turn asked for. That
  closes §29's loop end to end: poll → terminal state → **persist**, the step that was
  quietly doing nothing before any of this and losing every long run's results.
- A **SUGGESTED NEXT** card offered "Synthesize theories from the literature", derived from
  artifacts the run had just produced.

One correction to §30's design note, from watching it work: the background worker is a
*whole coordinator*, and the concern was that this made each task heavier than a
single-purpose subagent. Running two at once cost nothing visible, and the flexibility —
one worker doing search, another doing synthesis — is what made the test easy to write.

### Still unverified

**Background approvals** (§31). Both test tasks were literature work, which never touches
`execute`, so the gate was never reached. The next test has to run code — a task that
writes and analyses a dataset — because until that path is confirmed, any background task
touching data may simply hang.

**Completion of an async task.** The theorizer's completion is confirmed; a *background
worker* finishing and returning its result is not.

## 37. Background runs carry no key (2026-08-01)

Both background workers failed with *"The async subagent encountered an error"* — no
further detail. The Jobs panel caught it correctly (`✗ background worker · error`), which
is §31 working; the cause was structural and ours.

```python
run = client.runs.create(
    thread_id=thread["thread_id"],
    assistant_id=spec["graph_id"],
    input={"messages": [...]},        # ← no `config`
)
```

The middleware starts the run with **no config at all**. This app's whole key design is
that the model choice and API key travel *in the run request* (§20/§22) — which works for
turns we create, and cannot work for a run the backend starts by itself. So the background
run had neither `model_config` nor `__llm_keys`, upstream fell back to the WorkOS vault
(the `404` visible in the log all along), found nothing, and could not construct a model.

**Fixed by putting the key in the backend's environment**, which LangChain reads when no
key is passed — `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, and so on per provider.

**Only when background work is switched on.** With it off nothing needs this and the key
stays out of the environment entirely, which is the stronger posture and remains the
default. That is a real trade being made deliberately: enabling background work moves the
key from "request-only" to "also on the backend process", the same standing `ASTA_TOKEN`
has always had.

Worth noting what this says about §30's "no fork needed" claim, which still holds — but
the seam is thinner than it looked. Co-deploying the graph was free; the *run creation* is
upstream's, and anything the request has to carry is out of our reach. A wrapper around
`start_async_task` in the overlay could inject config properly, and is the better fix if a
second such gap appears.

## 38. The real reason background work failed — and it was never the key (2026-08-01)

§37 was right about the mechanism and wrong about the consequence. `client.runs.create()`
does pass no config, but the missing key was the *smaller* half of what that costs. The
larger half is on the very next line of our own code:

```rust
// protocol.rs — the config every foreground turn sends
"recursion_limit": 10_000,
```

with the comment we wrote ourselves months ago: *"LangGraph defaults to 25 supersteps, and
one turn already spends ~22 on middleware alone before any delegation."* A background
worker **is a whole Mini-Me coordinator** (§30). Started with no config it gets the default
25, spends ~22 on middleware, and dies before doing any work — on any provider, with or
without a key. That is why §37's fix changed nothing.

### Wrapping `start_async_task`, as §37 predicted

The overlay now replaces exactly one of the middleware's five tools. The other four
address a run by id and need nothing we have. The replacement reads the *live* run's config
via `langgraph.config.get_config()` and forwards it onto the background run:

```python
FORWARDED_CONFIG_KEYS = ("model_config", "__llm_keys", "__is_for_execution__")
```

An **allowlist, not a copy** — `configurable` also holds `thread_id`, `checkpoint_ns` and
`run_id`, and forwarding those would point the background run at the conversation's own
thread. Verified against a fake client: the run is created on the `background` graph with
the researcher's model, their key, `recursion_limit: 10000`, and no trace of the chat's
thread id.

This is strictly better than §37's environment variable, which is now **reverted**:

- it carries `base_url`, so a `custom` (OpenRouter/Groq/Ollama) endpoint works — no
  environment variable can express that;
- it uses the model the researcher actually picked, rather than upstream's
  `MINIME_DEFAULT_MODEL` fallback of `openai::gpt-5.4` (`backend/models.py:24`);
- it keeps the key **out of an environment the agent's own `execute` tool can read**.

So the key stays request-only whether background work is on or off, and §37's "deliberate
trade" is withdrawn — there was no need to make it.

### The placeholder that cost two rounds

*"The async subagent encountered an error"* is not upstream being unhelpful; it is upstream
having nothing to say:

```python
error_detail = run.get("error")
result["error"] = str(error_detail) if error_detail else "The async subagent encountered an error."
```

The dev server records no `error` on the run record, so that branch always fires. The real
text **is** available — on the thread's pending task, which `/threads/{id}/state` returns
and this app was already fetching for approvals. `thread_state` now reads it, and the Jobs
panel shows the exception line instead of the word "error".

That fixes a second defect found while looking: the watcher derived `success` from an empty
`next`, and a *failed* run leaves its task pending — so `next` is never empty and a dead
worker read as **running forever**. It only ever showed "error" because the researcher
happened to ask, which routed through `check_async_task`. Failure now beats every other
signal in that derivation.

This is the same lesson as §35, and it has now cost this project twice: **when a component
reports a cause rather than an error, go get the real output before fixing anything.**

## 39. Background work had never run once (2026-08-01)

The cause of every failed background task, from the first:

```python
async def background_graph():          # our factory — no parameter
    return await upstream_agent()      # TypeError: missing 1 required positional argument: 'config'
```

`backend/agent.py` declares `async def agent(config: RunnableConfig)`. Our factory took no
parameter, so it had none to pass on. Every background run raised `TypeError` while the
graph was being **constructed** — before a single node executed.

That also explains the thing §38 could not: why there was no error text to read anywhere.
A run that dies during construction writes no checkpoint, so `/threads/{id}/state` has no
task to hang an error on. The middleware's placeholder was genuinely all that existed.

**Fixed** — the factory takes `config` and passes it on. Verified three ways rather than by
inspection, since inspection is what missed it twice:

- against the dev server's own classifier, `_classify_factory(background_graph)` now
  resolves to `{"config": <RunnableConfig>}` (`langgraph_api/_factory_utils.py`);
- the graph builds — a real `CompiledStateGraph` with the full middleware stack, where
  before it raised;
- the built worker holds `execute` but **no** `start_async_task`, so `_BUILDING_BACKGROUND`
  still stops a worker spawning workers.

The call is adaptive (`inspect.signature`) rather than hardcoded: if upstream ever drops
the parameter this keeps working, and it warns instead, because the failure mode of getting
it wrong is invisible.

### On the two fixes that came before this one

Neither §37 nor §38 was the cause, and it is worth being exact about what they were:

- **§37 (key on the environment) was simply wrong** and is reverted. It addressed a real
  gap with the wrong mechanism.
- **§38 (forward the config) was a real bug and is still required** — it is what makes the
  `config` this factory now receives contain the researcher's model and key. It was also
  the *next* failure in line: the recursion limit would have killed the worker at superstep
  25 the moment the graph built.

The pattern in all three: a placeholder error was treated as evidence. §35 recorded the
lesson once — *recover the real output before fixing anything* — and it was not applied,
because the "real output" here was never going to appear in a log the run never reached. The
sharper rule: **when there is no error text anywhere, suspect the constructor, not the run.**

## 40. Background work verified end to end (2026-08-01)

`✓ background worker · success` on Windows, and the **approval gate fired** — the path §31
built and that nothing had ever exercised, because both earlier tests were literature work
that never touches `execute`. A background worker now generates data, asks permission on
its own thread, and the answer reaches it. P6.5 is done.

One defect surfaced the moment it worked: the approval card grew with the command. An
agent-written script is hundreds of lines, the card took all of it, and Approve/Reject —
along with the composer beneath them — were pushed off the bottom of the window. A gate
whose buttons cannot be reached is worse than no gate: it hangs the task and hides why.

The command now scrolls inside a capped region and the decision sits outside it, in **both**
cards — the foreground one and the Jobs-panel one, which has the same failure at a narrower
width. This is the third time the fix has been *"actions outside the scroll area,
`flex_none` on anything that must not be squeezed"*, and the third time it was found from a
screenshot rather than from the code.

## 41. Approval scope, widened on the researcher's evidence (2026-08-01)

*"I need to click too many times approve. maybe thats something scientist will dislike."*

Three separate gaps, only one of which was the missing button:

1. **"Approve the rest of this turn" already existed — and was unreachable.** It sat below
   the command, and §40's card overflow pushed it off the window. The feature had been
   there for weeks and had never been seen.
2. **Background tasks had no blanket option at all.** A worker asks once per command over
   several minutes, on its own thread, while the researcher has gone back to work. That is
   the worst place to require a click each time, and it is precisely where handing work to
   the background stops being useful.
3. **Turn scope was too small.** One analysis is a dozen commands across several turns.

Added: **"Approve everything in this conversation"** (covering background workers too) and
**"Approve the rest of this task"** on the Jobs-panel card.

### What keeps this from becoming a rubber stamp

§19's original argument still stands — *"the tenth identical dialog in one analysis is not
read, it is dismissed, and then neither is the eleventh — which is the one that mattered."*
The answer to that is not to make people click more; it is to make the grant **bounded,
visible and revocable**:

- **Never persisted.** Nothing is written to disk. Closing the app ends it.
- **Ends with the conversation.** "New thread" clears both the conversation grant and every
  per-task one.
- **Visible the whole time it holds.** The status bar shows *"approving everything — click
  to stop"* in accent colour whenever it is in force. A blanket grant that is invisible is
  the actual hazard.
- **Revocable in one click**, without starting a new conversation — otherwise "just this
  once" becomes permanent through inconvenience.
- **Still recorded.** Auto-answered commands still appear in the card and the trace. This
  removes the interruption, not the record.

The permanent, cross-session version remains what it was: a Settings toggle the user has to
go and find, worded *"Off is for automation, not a recommendation."*

## 42. Outputs a researcher can actually see (2026-08-01)

Three requests, one root cause: **the app did not know where the agent's files went.**

The backend writes each thread's files to `~/.mini-me/workspaces/<thread>` — inside the WSL
distro, which on Windows means `\\wsl.localhost\Ubuntu\home\<user>\…`. For a user base that
is ~98% Windows and none of whom are expected to code, files they cannot find are files
that do not exist.

So the app now **chooses** that directory instead of letting the backend default, and puts
it on the Windows side: `Documents\Mini-Me\<thread>`, passed in as `MINIME_LOCAL_WORKSPACE`.
All three requests fall out of that one decision:

1. **"A button to download all the documents."** There is nothing to package — the files
   are already in the researcher's own Documents. *Open this conversation's files* in the
   OUTPUTS panel opens the folder in Explorer.
2. **"Generated plots should show in the chat."** The app can now read them. Figures appear
   under the answer that produced them, capped at 420px, and clicking one opens it full
   size. They are found by **diffing the workspace across the turn**, not by being
   reported: a plot is written by a `matplotlib` script inside `execute`, which registers
   no artifact and tells the client nothing. The file appearing on disk is the only signal
   that exists.
3. **"I cannot see which subagent is doing the job."** Separate fix: `thread_state` now
   reads the last tool call off the worker's own thread, so the panel shows
   `running · academic researcher` rather than `running` for ten minutes. `task` calls
   report the subagent they delegated to; everything else reports the tool.

The cost of the move is that writes cross WSL's 9p mount, which is genuinely slow for
*many small* files — it is why the backend venv stays inside the distro (§25). A turn's
outputs are a handful of CSVs, figures and reports, and being able to find them is worth
more than the milliseconds.

**Migration:** files written before this change stay where they are, under
`~/.mini-me/workspaces` in the distro. Nothing is moved or deleted; new conversations use
the new location.

Also verified this round, and long outstanding: **the Windows Job Object works.** After
closing the app, `wsl -- pgrep -af "langgraph dev"` prints nothing — the backend dies with
its parent, so no orphaned server holds the port (§26).

## 43. Two bugs the plots exposed, and a UI debt worth naming (2026-08-01)

### The background worker was writing where nobody looked

A screenshot showed the coordinator running `ls`, `ls`, `ls`, `glob` ×8, `read_file` ×3 and
then admitting *"the files weren't at the root path I first tried"* — before printing three
absolute paths as text. No figure rendered.

The cause: **a background worker runs on its own LangGraph thread**, and the workspace is
one directory per thread (`workspace.py`). So the worker wrote to *its* directory, while
the app looked in the conversation's and the coordinator looked in its own. Three
components, three different folders, and the only one that could find anything was the
worker itself.

Fixed by pinning the worker to the conversation's workspace: `start_async_task` forwards
`__workspace_thread__`, and `LocalWorkspaceBackend` prefers it over the run's own thread
id. Note this is deliberately **not** forwarding `thread_id` — that would point the run at
the wrong thread and corrupt it; this is a separate key read only when choosing a
directory. An existing pin wins, so a worker started by a worker still writes to the
conversation's folder.

### Plots were diffed against the wrong moment

§42 snapshotted the figures at turn *start* and diffed at turn *end*. A background worker
finishes on its own schedule — usually between turns, sometimes minutes after the turn that
started it — so its figures fell outside every window and were never attached.

Now the diff is against **what the transcript already shows**, which makes `collect_plots`
safe to call from anywhere; it also runs when a background task completes.

### P6.7 — take the UI seriously

Stated plainly by the person using it: *"our current app is really awful hehe."* That is
fair, and it is not a mystery — every panel here was built to prove a mechanism worked, and
none was built to be looked at. Buttons are hand-rolled `div()`s with eight style calls
copy-pasted per site, which is exactly why they drift.

**What is actually borrowable.** GPUI *is* Zed's framework, but Zed's `ui` and `theme`
crates are monorepo-only — unlike `gpui` itself they are not published. So this is adopting
**patterns**, not adding dependencies. `gpui 0.2.2` already ships the primitives needed:
`svg`, `uniform_list`, `list`, `anchored`, `deferred`, `canvas`, `div().tooltip()`,
`ScrollHandle`, animation and an image cache — almost none of which this app uses.

In rough value order:

1. **Visible scrollbars.** `overflow_y_scroll` draws nothing, so content that scrolls looks
   like content that is cut off — the direct cause of "I cannot go to the bottom to approve
   or reject" (§40). Zed draws its own; so should we.
2. **A `Theme` struct with semantic roles** (`text`, `text_muted`, `border`,
   `element_hover`, `status_error`) replacing the scattered `const` hex in `main.rs`. One
   source of truth, and the precondition for a light theme.
3. **A component vocabulary** — `Button`, `IconButton`, `Label`, `Divider`, `Tooltip` — so a
   button is one call, not eight, and every card looks the same because it *is* the same.
4. **Bundle a font.** We ship none, so fenced code renders in Segoe UI. Register a mono at
   startup via the text system.
5. **Icons as `svg()`** tinted by the theme, replacing the text glyphs `◐ ✓ ✗ ◎`.
6. **Tooltips** — the framework has them; this app uses none.
7. **`uniform_list` for the transcript.** Every message is currently laid out every frame;
   a long session will crawl.
8. **Focus rings and a tab order.** One focusable field today, and no visible focus.
9. **Resizable/collapsible panels.** The right panel is a fixed width nobody chose.
10. **Toasts** instead of one status line that overwrites itself.

Deliberately *not* on this list: **text selection**, which needs a custom element and is the
one thing here GPUI genuinely makes hard (§16).

## 44. Why the approval button appeared to move (2026-08-01)

*"Sometimes the button approve for all the conversation appears bottom and sometimes in the
background panel."*

It was not moving. There are **two gates**, and which one you see depends on *who* needs
permission:

| Who asks | Where it appears |
|---|---|
| The coordinator, on the conversation's thread | The card above the composer |
| A background worker, on **its own** thread | The BACKGROUND JOBS panel |

That distinction is load-bearing — a background worker runs on a different thread and must
be answered there (§31) — but it was invisible, and the two cards offered **different
grants**: the chat card had *rest of this turn* and *everything in this conversation*, the
panel had only *rest of this task*. So the same intention had a different button, with
different wording, depending on which component happened to ask. That is indistinguishable
from a button that wanders.

Fixed by offering the conversation-wide grant in **both** places, worded identically. It
means the same thing in both — it is one flag — so wherever the researcher meets it, one
click ends the interruptions everywhere, foreground and background alike, until the
conversation ends or they click *stop* in the status bar (§41).

The narrower grant stays contextual, because its scope genuinely differs: *this turn* only
exists in the chat, *this task* only exists in the panel.

## 45. The download link (2026-08-01)

The largest remaining gap was not a feature. Installing meant `git clone` and
`cargo build` — which excludes, precisely and completely, every researcher this was built
for. `package.sh` already produced a folder; nobody could get at it.

`scripts/release.sh` closes that: it validates the bundle, zips it, checksums it, writes
release notes aimed at the person downloading, and creates the GitHub release.

**Version is now `0.1.0`**, not `0.0.0` and deliberately not `1.0`: WSL provisioning has
never run on a machine that never had WSL, and the executable is unsigned.

### It refuses to publish a bundle that would waste someone's afternoon

Each check is a way a colleague's first ten minutes get destroyed, and hearing about it
from them is the expensive way to find out:

- **a Linux binary in a Windows bundle** — easy to do from WSL, and useless to the entire
  audience;
- **`vendor/Mini-Me` missing** — the installer then asks for a GitHub token for a *private*
  repository, which is the exact wall the bundle exists to remove (§25);
- **`overlay/` missing** — host execution silently would not work;
- **a dirty working tree** — the tag would not describe what is in the zip.

`--dry-run` runs all of it, builds the zip, and prints the release note without touching
GitHub. Verified here end to end against a simulated Windows bundle: guards fire, the zip
builds, the checksum computes, the note reads correctly.

### Draft by default

Nobody has ever installed this app from a zip. A draft can be deleted; a published release
someone already downloaded cannot. Publishing is one printed command, to be run *after*
installing from the zip on a machine that is not the build machine.

### SmartScreen is addressed with words, since it cannot be addressed with a certificate

Unsigned, Windows shows *"Windows protected your PC"* with only a **Don't run** button
visible — and most researchers stop there, which would make every other fix in this
document irrelevant. Both the release notes and the bundled `README.txt` now say exactly
what will appear and which two words to click. Signing remains an organizational decision;
this is the honest interim.

### Not done: CI

A `windows-latest` workflow would remove the "build it on your own machine" step, but it
needs a token for the private backend repo and a Windows SDK with `fxc.exe` for the release
build (§26) — neither testable from here. A script that works today beats an untested
workflow that might.

## 46. Three shells, three spellings of a path (2026-08-01)

The first real release attempt died at the zip:

```
Compress-Archive : La ruta de acceso "\mnt\c\Users\...\dist" no existe
```

`release.sh` assumed **Git Bash** and reached for `cygpath`. It was run from **WSL**, where
`cygpath` does not exist, so the fallback handed PowerShell WSL's own spelling of the path —
which Windows cannot resolve. Neither shell ships `zip`, so the PowerShell branch is the one
that actually runs on the target machine, and it was the untested one.

Three shells, three spellings of the same directory:

| Shell | Path |
|---|---|
| WSL | `/mnt/c/Users/…/dist` |
| Git Bash | `/c/Users/…/dist` |
| PowerShell | `C:\Users\…\dist` |

`win_path()` now tries `wslpath -w`, then `cygpath -w`, then a transform that handles both
POSIX forms — verified against all three. It translates the **parent directory**, because
`wslpath` resolves paths that exist and the zip does not exist yet.

Worth naming the testing gap honestly: `--dry-run` passed here because Linux *has* `zip`, so
the branch that fails on Windows was never reached. A dry run that exercises a different code
path than the real one is not a dry run of the real thing — the guards it did verify were
real, but this one it structurally could not.

## 47. P6.7 begins: showing an agent's work without burying its answer (2026-08-01)

A screenshot of a real turn:

```
· read_file  · read_file  · read_file  · ls  · ls  · ls
· glob ×8 (as eight separate lines)  · ls  · ls  · read_file ×3
The files weren't at the root path I first tried...
```

Twenty-plus lines of process, then the answer — below the fold. The activity trace was
built in §15 to close a "silent gap" during long delegated turns, and it did. But it was
written for a turn that *works*; a turn that struggles emits the same call repeatedly, and
nothing folded it.

### What the field has already settled

Researched rather than guessed, and three sources converge:

- **Zed's own agent panel** proposes rendering each spawned subagent as a **collapsible
  sub-thread** — in the parent, one compact card (*"→ Subagent … (12 tool calls, 4.2s)"*)
  rather than the whole inlined timeline
  ([zed#57481](https://github.com/zed-industries/zed/discussions/57481)).
- A second Zed proposal names the temporal pattern exactly: **expand a turn live while the
  agent works, then collapse it on completion**
  ([zed#58314](https://github.com/zed-industries/zed/discussions/58314)).
- Luke Wroblewski's survey of shipped agent UIs reports the same lesson learned the
  expensive way: fully-disclosed tool calls proved *"too much information"*, and iterations
  *"focused on reducing the visual weight of tools and showing less process by default"* —
  the more tools an agent has, the more this matters
  ([lukew.com](https://lukew.com/ff/entry.asp?2142=)).

This app had already arrived at half of it: subagent groups have been collapsible with a
`▸ name · N steps · N chars` header since §15c. The **coordinator's own steps** were the
part still rendered flat and unbounded.

### What changed

- **Consecutive identical steps fold** — `glob ×8` on one line. Only *consecutive* runs:
  `read_file ×3, ls ×2, read_file ×3` is a different story from `read_file ×6`, and
  flattening the order would erase it. Applied to subagent steps too.
- **The coordinator's steps get the same disclosure the subagents had** — a
  `▾ 24 steps` header, open while the turn runs, **closed the moment it ends**. During a
  two-minute wait the steps are the only sign of progress; afterwards the answer is the
  point.
- *Expand / collapse agent activity* in the palette now reaches these too, rather than
  leaving half the activity shut.

Still ahead in P6.7, unchanged in priority: **visible scrollbars** (the highest-value single
fix), a theme struct, and a component vocabulary.

## 48. P6.7: a palette, a history, and files you can reach (2026-08-01)

*"In general looks awful."* Four things named: a colour palette, conversations in the left
panel with renaming, how to show a thread's files, and interactivity. Researched first —
the sources are cited where they changed a decision.

### The palette was one orange doing six jobs

`main.rs` held seven `const`s, and the brand orange was used for section headings, links,
buttons, the running mark, the host-execution warning and any border wanting attention.
When everything is emphasised nothing is. That, more than any individual panel, is why the
app read as amateur.

`theme.rs` replaces them with **roles**, on the model every dark-mode system converges on
([Muzli](https://muz.li/blog/dark-mode-design-systems-a-complete-guide-to-patterns-tokens-and-hierarchy/),
[Imperavi](https://imperavi.com/blog/designing-semantic-colors-for-your-system/)):

- **A surface ladder** — `BG` → `SURFACE` → `RAISED` → `OVERLAY`, so panels, rows and
  popovers separate by *elevation* instead of by drawing more borders.
- **Three text weights**, all AA: `TEXT`, `TEXT_MUTED`, `TEXT_FAINT`.
- **Status colours of their own** — `SUCCESS`, `WARNING`, `ERROR`, `RUNNING` — so "finished"
  and "clickable" stop looking identical.
- **One rule for the accent: orange means you can act on this, and nothing else.**

Two tests enforce it rather than trusting the eye: one computes WCAG contrast for every
ink/surface pair and fails below 4.5:1, the other asserts the ladder actually ascends and
that hover lifts. **The contrast test failed on first run** — `ERROR` on `RAISED` was
4.30:1, and `TEXT_FAINT` 4.17:1 — so both shipped values are ones the build chose, not ones
that looked fine.

### The conversations existed all along

The left rail was 64px holding one glyph. Meanwhile the backend had stored **every thread
since the first launch** — the app simply never asked, so each session looked like the
first and past work was unreachable, which for the researcher is the same as lost.

`POST /threads/search` lists them; `PATCH /threads/{id}` names them. The sidebar is now
220px with: newest-first list, click to reopen (transcript rebuilt from the thread's stored
messages, figures re-read off disk), a **New** button, and **rename in place** — the row
becomes a text field, the pattern chat apps use because it keeps the name next to the thing
being named.

**Auto-titling from the first prompt**, because a list of "New conversation" is a list of
nothing — the convention across ChatGPT, Claude and Codex, all of which also allow manual
rename, which is why both exist here
([thefrontkit](https://thefrontkit.com/blogs/ai-chat-ui-best-practices),
[codex#12564](https://github.com/openai/codex/issues/12564)).

Titles live in **thread metadata**, not a local file: the name belongs to the conversation,
survives a reinstall, and cannot drift out of sync with what it names.

### Files, grouped by what you would do with them

OUTPUTS now lists the thread's actual files above the agent-declared buckets, grouped
**Figures → Data → Documents → Other** — categories about use, not format. Read off disk
for the same reason plots are (§42): a file written by a script inside `execute` registers
no artifact, and those are most of them. Each row shows a size and opens in whatever the
researcher normally uses; figures first, because those are the ones you want to *look* at.

### Interactivity

Every row now responds to the pointer — `RAISED` on hover, `ACCENT_SOFT` for the selected
conversation, and the rename control appears only on the row under the cursor via
`group_hover`, so the list stays a list of names rather than a wall of controls.

### Still ahead

**Visible scrollbars** remain the highest-value single fix, and are still not done —
`overflow_y_scroll` draws nothing, so scrollable content reads as cut off (§40). Then a
`Button` component to end the copy-pasted eight-call button, bundled fonts, and SVG icons.

## 49. P6.7 continued: themes, panels, search, preview (2026-08-01)

Six requests. Researching Zed first collapsed three of them into one component.

### One pattern answers three requests

Zed has **over fifty picker modals**, all the same shape: a centred floating panel over a
dimmed workbench, list on one side, preview on the other, the editor still visible behind
([zed#59604](https://github.com/zed-industries/zed/pull/59604),
[Zed blog](https://zed.dev/blog/hidden-gems-part-1)). That shape is the answer to *view a
file in the middle*, *settings as a floating window*, and *fast search* — they are one
component with three contents, not three features.

The **file preview** is built on it: centred, dimmed backdrop, click-away to close,
figures rendered inline, Markdown through the existing renderer, everything else as text.
Reads a **bounded 400 lines** — the file most worth previewing is a big dataset, which is
exactly the one that would freeze the UI thread if read whole.

### Themes, adapted from Zed rather than copied

Zed ships theme *families* as JSON with semantic keys — `background`, `text`, `accent`,
`border`, `elevated_surface.background` — and loads more from extensions whose ids must end
in `-theme` ([Zed docs](https://zed.dev/docs/extensions/themes)). We took the shape and
dropped the registry: a researcher wants to pick a palette, not publish one.

- `theme::Theme` is that struct; colours are read through functions backed by **atomics**,
  so switching is a store the next frame sees — and the free rendering helpers, which have
  no `Context` to reach a GPUI global through, can still ask.
- **Four built-ins**: *Mini-Me Dark*, *Slate* (cool, blue accent, for people who do not
  want an orange app), *Paper* (light — the one case where a dark UI genuinely fails, on a
  projector), *High Contrast*.
- **Any JSON file in `themes/`** beside `settings.toml` is offered too, and a file named
  after a built-in *replaces* it — how someone tweaks the default instead of living with it.
  A malformed palette is logged and skipped, never fatal: a researcher locked out by their
  own theme file has no way back in to fix it.
- Clicking the picker **applies immediately**; cancelling Settings reverts. A palette is
  judged by looking at it, so a Save button between the click and the result is the wrong
  loop.

The contrast test now runs over **every** theme, so a palette added later cannot be added
unreadable. It failed twice while writing them — `Slate`'s error red at 4.35:1 and three of
`Paper`'s inks — and both shipped sets are values the build chose.

Writing `Paper` also exposed a rule worth stating: **elevation always raises luminance**.
The first attempt inverted it for light themes, which made "elevated" mean two different
things. A grey canvas with white cards gets the same rule everywhere.

### The rest

- **Conversation search** reuses the command palette's own fuzzy scorer, so `pap` finds
  *Rendimiento de papa*. Same behaviour as Zed's file finder, no new matching code.
- **Both panels toggle** from the status bar, and both toggles stay visible when their
  panel is closed — a collapsed panel with no way back is the commonest way this feature
  becomes a bug report.
- **A spinner while anything runs.** The first turn after launch spends 20–40 seconds
  building the agent, and a still window reads as a hang — the most common reason someone
  kills an app that was working. Four braille frames on GPUI's own animation: no font to
  ship, no SVG, reads as motion at any size.
- **The ◎ at top-left opens Settings**, as asked.

### Not done

Settings is still a right-hand pane rather than a modal — the picker component now exists,
so moving it is small, but it is a change to a pane that works and this batch was already
large. **Visible scrollbars** remain undone and remain the highest-value single fix.

## 50. P6.7 step two: three bugs, and Zed's gallery (2026-08-01)

Screenshots of Zed alongside the app, and six things wrong. Three were bugs.

### The bugs

**"Open outside" did nothing for any file.** `workspace::open` was written for folders and
began with `create_dir_all`, so on a *file* — which exists — it failed with `AlreadyExists`
and returned before Explorer was ever launched. It now only conjures a directory that is
missing. One line, and it had broken every file-open in the app.

**The conversation list was empty until the first question.** The backend spawned lazily on
the first turn, so at launch there was nothing to list and the app looked as though it had
never been used. It now warms up at startup, which pays twice: the sidebar has content
immediately, and the 20–40 second agent build happens while the researcher reads the window
instead of while they wait on an answer.

**Clicking the top-left mark reset the palette.** `open_settings` reloaded the whole draft
from disk *and* reapplied its theme, so opening the pane discarded the theme being looked
at. The live palette and the saved one are now separate: opening never touches the screen,
clicking a theme applies it, and **Close** — not open — puts the saved one back.

### Themes: a list, and Zed's whole gallery

The cycle button was wrong twice over — the only way to find a palette was to click through
every one, and there was no way to see what existed. Now every theme is listed with a
**five-dot swatch** of its background, panel, accent, text and error, so they can be
compared side by side. GPUI 0.2.2 has hover *styling* but no hover *event*, so a true live
preview needs a custom element; the swatch does the same job and shows all of them at once
rather than one at a time.

**Zed theme files load directly.** A Zed theme is JSON, so anything from
[zed.dev/extensions](https://zed.dev/extensions) can be dropped into `themes/` and used.
The importer maps the fifteen keys that mean something here out of the 142 in the published
schema (`zed.dev/schema/themes/v0.2.0.json`), falls back per field so a partial theme still
loads, and derives a hover shade Zed does not define.

What we **cannot** use is a Zed *extension*: those are WASM compiled against Zed's own
extension API, and running one would mean implementing Zed. The theme JSON inside them is
portable, and that is the part worth having — a distinction worth stating plainly rather
than promising "install Zed extensions" and shipping something that does not.

### Rainbow CSV, from the theme rather than a fixed rainbow

CSV cells are coloured by column index — the `rainbow-csv` trick — but cycling **the
palette's own roles** instead of a fixed spectrum. Those colours are already contrast-checked
against every surface, so a wide table stays readable in the light theme too, where a fixed
rainbow would wash out. Without column *layout*, which GPUI 0.2.2 lacks, colour is the only
thing that makes a wide CSV readable at all.

### Rounded panels

Panels are now rounded cards with a margin, sitting on the window background, rather than
full-bleed slabs meeting at a hairline — which is what the Zed screenshots show and most of
why they look finished.

### Still not done

Settings remains a right-hand pane rather than a centred modal, and **visible scrollbars**
remain undone and remain the highest-value single fix.

## 51. Thirty conversations that were not conversations (2026-08-01)

The sidebar worked, and immediately showed what was wrong with it: **thirty rows reading
"New conversation"**, above the two real ones.

They were not conversations. Every background worker creates a **thread of its own** (§43) —
that is what makes it a background worker — and `POST /threads/search` returns every thread
the backend has. The list was machinery, correctly listed.

Fixed by tagging what this app creates: `POST /threads` now writes
`metadata.minime_conversation`, and the search filters on it. The distinguishing fact is
**who created the thread**, and only this app tags what it creates — so the filter is exact,
where "has messages" or "has a title" would be guesses that keep being wrong. A background
worker's thread is real and deliberately is not one of these.

**Threads created before this change will not appear.** They are almost entirely the
"New conversation" rows; the two real ones are lost from the list, which is the cheaper
mistake than a list nobody trusts.

### Also from the same screenshot

- **The status bar was clipped off the bottom edge.** It is the last child of a column whose
  transcript grows, and a flex child shrinks by default — the third time `flex_none` has
  been the answer (§40, §48).
- **The empty state named a Run button** that has not existed since the composer landed
  (§12) — the first sentence a new user reads, describing a UI from weeks ago.
- **Settings became a centred modal.** As a column it took 420px off the chat for as long
  as it was open; it is somewhere you visit and leave, which is the same argument that
  makes Zed's fifty pickers modal rather than panels. This closes the last of the six
  requests from §49.
- The search field now looks like a field: rounded, its own background, a border.

## 52. Scrollbars, a real theme gallery, and a correction (2026-08-01)

### The correction first

I told the researcher that Zed extensions are WASM compiled against Zed's own API and so
could not be used here. That is true of **language** extensions and false of **theme**
extensions, and the registry says so plainly:

```json
{"id":"catppuccin","provides":["themes"],"wasm_api_version":null,"download_count":964535}
```

`wasm_api_version: null` — pure data, a `.tar.gz` of JSON. I generalised from the kind of
extension I had read about to all of them, and made a feasible feature sound impossible.
The lesson is the one from §35 and §39 in a new costume: **check the thing itself before
asserting what it is.** Two public endpoints and one `curl` would have settled it.

I also said the live theme preview needed a custom GPUI element. `InteractiveElement`
has **`on_hover`**. Same mistake, same size, found the same way — by looking.

### The gallery

`gallery.rs` searches `api.zed.dev/extensions`, keeps only entries whose `provides`
contains `themes`, sorts by installs, downloads the archive and writes `themes/*.json` into
the same directory a researcher can drop files into by hand. Author and install count are
shown because these are other people's work under their own licences.

It reads **only** `themes/*.json` and builds the write path from the *file name*, never the
archive's own path — the classic tar traversal, worth ruling out even from a registry we
trust. Everything else in the archive (licence, manifest, readme) is not ours to interpret.

It is a **theme** installer, not an extension installer, and the module says so: promising
the latter and shipping the former would be worse than naming which one this is.

### Scrollbars, at last

`overflow_y_scroll` draws nothing, which is why the approval card read as broken (§40) and
why nobody found Save below the fold in Settings. `ScrollHandle` exposes `offset()`,
`max_offset()` and `bounds()` — enough to size and place a thumb, with no custom element.
It sits *outside* the scrolling div, in a `relative()` wrapper; inside, it would scroll
along with the thing it measures.

### The send button

Researched rather than invented. Shipped composers converge on three states, and the
button now has all three: a filled circular control that **sends**, the same control greyed
when the field is empty — *empty means disabled* is near-universal — and a **stop** while a
turn streams. Clicking stop currently says cancelling is not built yet, which is at least
honest; the control is where it will live.

The composer and its button are now one bordered, rounded field rather than a text box next
to an unrelated button.

### Settings, again

The modal scrolled *as a whole*, so **Save and Close were below the fold** — the same
defect as the approval card in §40, in the component built to fix that class of problem.
Title and actions are now fixed and only the middle scrolls. Third occurrence; the lesson
is evidently not learned by writing it down, which is an argument for the `Button` and
`Modal` components P6.7 still owes.

## 53. The status bar belongs to the window, not the chat (2026-08-01)

Catppuccin installed from the gallery and applied — the §52 path works end to end. One
thing left over from the layout:

The status bar lived *inside* the chat pane, so it was only as wide as the chat, and its
controls slid left and right every time a panel was collapsed. **A status bar that moves is
one you have to look for**, which defeats the point of putting the panel toggles there.

Zed's runs the full width of the window for exactly this reason. The layout is now a column
— panels in a row on top, status bar spanning the window beneath — so the toggles, the
execution indicator and the URL stay where they are whatever is open.

`min_h_0` on the row, for the fourth time (§40, §48, §51): a flex child refuses to shrink
below its content, so without it the row pushed the status bar off the bottom edge. Four
bugs from one default is a strong argument that the layout primitives should be wrapped
once, correctly, rather than re-specified at every call site — which is the `Button` and
`Modal` work P6.7 still owes.

## 54. P6.7 closed (2026-08-02)

Verified on Windows: Catppuccin installed from Zed's gallery inside the app, panels
collapse without the status bar's controls moving, the composer reads as one field with a
live send control, and the sidebar remembers conversations across launches.

**Every milestone from P6.0 to P6.7 is now closed.** What the app does is done; what
remains is getting it to a second person, and paying down the debt underneath the UI.

### The debt, stated plainly, because it is now the top item

Two mistakes have each been made three or four times:

| Mistake | Where | Cost |
|---|---|---|
| A flex child shrinking or growing when it must not (`flex_none`, `min_h_0`) | §40, §48, §51, §53 | Four bugs |
| Actions placed *inside* a scrolling region | §40, §41, §52 | Three bugs |

The third and fourth occurrences happened **after** the lesson was written down, and one of
them was inside the component built to fix the first. Writing it down is evidently not the
mechanism. A wrapped `Button`, `Modal` and `Panel` — where the scroll region and the action
row cannot be mis-nested because the type does not allow it — is the mechanism, and it is
now worth more than any new feature.

### What was learned about researching before building

Three times this batch, checking the artefact beat reasoning about it:

- `on_hover` **exists** in GPUI 0.2.2 — I had said a live theme preview needed a custom
  element (§52).
- Zed **theme** extensions are pure data (`wasm_api_version: null`), not WASM — I had said
  the gallery was unusable (§52).
- `ScrollHandle` exposes `bounds()` and `max_offset()`, which is all a scrollbar needs — I
  had listed scrollbars as a large piece for weeks.

All three were one `grep` or one `curl` away, and all three had been asserted confidently
in the opposite direction. Same shape as §35's seven-round theorizer failure and §39's
`TypeError`: **the expensive errors in this project have all been things assumed rather
than checked.**

## 55. Multi-line prompts, and a design for naming a subagent (2026-08-02)

### The composer is a real field now

`Enter` still sends — rebinding that in a chat window would surprise everyone — and
**Shift-Enter** inserts a line break. That required making the element genuinely
multi-line, not just accepting a `\n` the renderer swallows:

- one `ShapedLine` per line, since GPUI's `shape_line` is exactly that, one line;
- height follows the content, capped at **8 lines**, so a pasted script enlarges the
  composer instead of eating the transcript;
- the caret finds its row, and a selection spanning lines paints **one rectangle per line**
  — a single box from start to end would paint over the text in between;
- hit-testing maps a click to a row and then along it;
- IME runs are **sliced per line**, or marked text would underline the wrong characters.

### P7 proposal — `/subagent` commands

The request: `/eda-subagent make an EDA of data.csv`, `/research-paper search this topic`,
`/report-write write a report from these papers`. Today the coordinator decides what to
delegate; this would let the researcher say it outright.

**What makes this cheap:** the machinery already exists. `start_async_task(description,
subagent_type)` takes the subagent by name, this app already wraps that tool (§38), and the
Jobs panel already tracks, approves and reports whatever it starts (§31, §42). A slash
command is a *front end* to a call we already make.

**Shape:**

1. **A registry of what can be named**, read from the backend rather than hardcoded — the
   coordinator's own subagent list is already in the graph, so a hardcoded list here would
   drift the first time upstream renames one.
2. **Completion in the composer.** Typing `/` opens the same fuzzy picker the command
   palette and conversation search use (`match_score`), showing each subagent's
   description. Nothing new to build but the trigger.
3. **Two dispatch modes**, and the difference matters:
   - **Foreground** — the turn runs as now, with the prompt prefixed to name the subagent.
     Right for a quick literature lookup.
   - **Background** — straight to `start_async_task` with `subagent_type` set, returning a
     task id immediately. Right for an EDA or a report, and the reason this is worth
     building: it is the only way to run three of them at once.
4. **The gate still applies.** A named subagent is still an agent running commands on the
   researcher's machine, and `/eda-subagent` must not become a way around the approval
   gate (§19, §41).

**The open question** is what happens when a named subagent does not exist — a typo, or an
upstream rename. Failing loudly at send is right; silently sending `/eda-subagent …` as
prose is how someone waits ten minutes for a turn that was never delegated.

Sequenced after the release link and the component work, because it is an accelerator for
people already using the app, and nobody outside this machine can install it yet.

## 56. The build no longer needs anyone's laptop (2026-08-02)

`release` #1, manually run, **green in 21m33s** on the first attempt — which is not what I
expected, having written the workflow without being able to run it.

The two steps I flagged as most likely to fail both passed:

- **`fxc.exe`** — found in the Windows SDK on the runner and exported as `GPUI_FXC_PATH`,
  so GPUI compiled its release shaders (§26).
- **The private backend** — cloned with the `MINIME_BACKEND_TOKEN` secret, so the bundle
  can install itself without the researcher having a GitHub account (§25).

`workflow_dispatch` was the right default: the run produced a downloadable artifact and
created no release, so the pipeline could be proved before anything public existed.

**What this changes.** A release stops depending on one person's Windows machine being
free, and stops depending on the state of that machine — the packaged zip is now built from
a clean checkout every time, which is the only way "it works on mine" ever gets ruled out.
`scripts/release.sh` remains as the local path and as the thing that documented what the
pipeline had to do.

**Still not proven:** nobody has installed this zip on a machine that did not build it.
That is the next step and the one that matters — the first-run WSL path has never executed
anywhere.

## 57. The first clean machine, and the blindness it exposed (2026-08-02)

A colleague ran the CI build on a laptop that had never had this app. **Most of it worked
on the first try** — the window rendered, the Setup pane opened, the checks ran, and it
correctly diagnosed *"WSL is present but no distro answered"* and offered the fix.

Then the fix failed, and reported:

```
Install Ubuntu — failed
— the command reported a failure — the last lines say why
```

**There were no lines.** The box was empty.

### Two defects, and the second is worse than the first

**`wsl --install` needs administrator rights.** The app is not elevated and must not be, and
a process cannot elevate itself — so the command failed instantly. The check's own note even
said *"asks for admin rights"*, and nothing acted on it. Fixed by wrapping it in
`Start-Process -Verb RunAs`, which is Windows asking the researcher rather than us pretending
we can. `-Wait` so "finished" means finished, and `exit $p.ExitCode` so a **refused** UAC
prompt reports failure instead of success.

**The log was empty because we could not read it.** This line:

```rust
for line in BufReader::new(pipe).lines().map_while(Result::ok)
```

`lines()` yields an error on the first byte that is not UTF-8, and `map_while` **stops at the
first error**. `wsl.exe` writes **UTF-16LE** — a fact already recorded in this project's own
notes about `wsl -l` — so every line was an error and the iterator ended before yielding
anything. Every fix that failed with a UTF-16 message has been reporting *nothing* since
`run_streaming` was written.

Now the pipe is read as **bytes**, the encoding is detected once from the first chunk (BOM,
or ASCII padded with NULs), and lines are split on `0A 00` or `0A` accordingly — cutting a
UTF-16 stream on a bare `0A` would leave the stray `00` at the head of the next line and
shift every character after it. A trailing line with no newline is kept, because that is
often the only line a failing command produces.

### The pattern, for the fourth time

*"The last lines say why"* over an empty box is exactly §35's theorizer reporting a guess,
§38's *"The async subagent encountered an error"*, and §39's silent `TypeError`. **Four
times this project has told someone a reason exists while showing them nothing.** The note
now says only *"the command reported a failure"*, and the card says plainly *"The command
printed nothing"* when that is the truth.

The rule, stated as a rule this time: **never write a message that promises output unless
the code path that produces it has been seen to produce output.**

### What this run proved

The packaging, the CI build, the unsigned-binary path past SmartScreen, the window, the
preflight checks and the diagnosis all work on a machine that did not build them. The
remaining unknown is now narrow: whether the elevated install actually completes.

## 58. Six things that only appear once the app is used (2026-08-02)

Catppuccin installed and applied, and with eleven palettes in the list the design started
failing in ways four never could.

- **The theme list is scrollable and filterable.** Eleven entries already crowded the modal;
  a hundred would push Save out of it. Capped at 260px with its own scrollbar, and a filter
  using the same fuzzy scorer as everywhere else, so `mocha` finds *Catppuccin Mocha*. The
  cure for a long list is a filter, not a taller list.
- **The model is a scrollable list, not a remembered string.** Nobody should have to know
  whether it is `claude-sonnet-4-5` or `claude-4.5-sonnet`. A short curated set per
  provider, and **the field stays editable** — a list here can only ever be out of date, and
  a provider shipping a model the day after a release must not make the app unusable.
- **Providers are pills, not a cycle button.** Five fit on one row, and a cycle button hides
  four of the five — the same complaint the theme list had just answered.
- **Escape closes things.** It was bound *only* inside the palette's key context, and focus
  is almost always in the composer, so it never reached a modal. Now bound with **no**
  context, closing inside-out: preview, then a rename in progress, then Settings, then
  Setup. From a preview over Settings it returns to Settings, not to nothing.
- **Conversations can be deleted**, in two steps. There is no undo on the server and a
  conversation is somebody's work, so a stray click on a `✕` in a list must not destroy it.
  Files the turn wrote are deliberately **not** removed: those live in the researcher's own
  Documents and are not ours to delete.
- **Square corners are gone** — twelve buttons, the bordered inputs and the fix log now
  match the panels rounded in §50.

### The debt this keeps proving

Every one of these was a call-site mistake: a list without a cap, a control without
rounding, a binding with the wrong scope. None was a hard problem, and all of them were
invisible until someone used the app with real data in it. That is the fifth argument in this
document for the `Button` / `Modal` / `Panel` set — a `Button` cannot be square, and a
`Modal` body cannot grow past its frame, if the type is the only way to make one.

## 59. Every model rendered as "…" (2026-08-02)

The model list shipped in §58 showed five rows, each reading exactly `…`.

`.truncate()` was on the **row** — the flex item itself — together with `min_w_0`. That
combination gives the element zero intrinsic width, so there is nothing to truncate *to* and
the ellipsis is all that survives. Every other truncating label in this file happens to sit
in an inner div with `flex_grow().min_w_0()`, which is why none of them showed this and why I
did not notice writing it a sixth time.

Fixed by matching that pattern: the row lays out, the **label** truncates.

Two smaller things the same screenshot showed:

- The selected row had a border and its neighbours did not, so it was a pixel taller and the
  list **jumped** as the pointer moved down it. Selection is now background and colour only,
  with a `✓` — no geometry change.
- The free-text field beneath the list was labelled *"Model"*, which read as a second control
  contradicting the first. It now says **"Or type any model id"**, which is what it is for:
  the list is curated and will go stale, and typing has to keep working.

This is the sixth call-site mistake in two days, and the second where the correct pattern was
already in the same file. Not a knowledge problem — a repetition problem.

## 60. What a screenshot of a blank console proved (2026-08-04)

A machine that had WSL but no distro clicked **Install Ubuntu**. Two screenshots came back.

The first showed a real elevated `wsl.exe` console downloading *"Subsistema de Windows para
Linux 2.7.11"* — so §57's elevation fix works. The second, taken after it finished, showed the
Setup pane saying **"Install Ubuntu — done"** with a body containing exactly one line, `—
finished`, directly above the same red **✗ WSL2 runtime** row it had started from.

Three separate defects, and the first one is the interesting one.

**The output was never ours to read.** `-Verb RunAs` elevates through ShellExecute, which
cannot be handed the pipes we opened: the elevated child gets a console of its own. That is
the window in the second screenshot. We captured the PowerShell wrapper, which prints nothing.
§57 fixed the *encoding* of output we were never receiving — a correct fix to the wrong
layer, and it looked verified because the code path it repaired was real.

So the elevated command now runs through `cmd.exe` purely to get `> "<log>" 2>&1`, into a path
in the user's own temp directory, and `run_streaming` **follows that file while it grows**
rather than draining it at the end — `wsl --install` downloads for minutes, and a pane reading
"starting…" for four of them is a pane a researcher reads as stuck. A terminated line goes out
immediately; only an unterminated tail waits, because it may be half-written.

**"Done" over a red row.** The install exited 0 having replaced the WSL runtime, and Windows
will not register a distro until it restarts. The app had no way to notice, so it congratulated
itself and contradicted itself in the same card. Now a fix that succeeds is compared against
the row it was meant to fix, and when that row is still failing the card says so — for the
runtime row, that Windows has to restart first.

That verdict is drawn from the re-check, **never from the command's own words**. This machine
printed *Descargando*, not *Downloading*: `wsl.exe` speaks the system language, so matching
output for "restart" would have failed for precisely the user who needed it. Roughly 98% of
this app's users are on Windows and not all of them are on an English one.

**A dash where an admission should have been.** `— finished` was pushed into the same `Vec` as
the command's own output, so the "this command printed nothing" message could never fire — the
note itself made the list non-empty. Notes now live apart from output and render *outside* the
scrolling box, since the verdict and the next step are the two things a chatty command must not
be able to scroll out of sight.

**One hazard the fix introduced, caught before shipping.** Redirecting stdout would have made
`wsl --install -d Ubuntu`'s interactive prompt for a UNIX username *invisible* — a blank window
waiting forever on input. It now passes `--no-launch`. A researcher who cannot code should not
have to invent a Linux password to read a paper; unlaunched, the distro answers as root, which
is all the sidecar needs.

Two smaller notes:

- Writing the live-progress test is what found the hold-back bug: every complete line was
  waiting for the *next* one, which would have shown `wsl.exe`'s progress permanently one step
  behind. The test failed for the right reason before the feature was wrong in front of a user.
- The first two attempts at that test failed by racing each other over `elevated_log()`, a
  single fixed path. That path is correct — the app runs one fix at a time — so they are now
  one test, which is also the honest shape: three facts about one mechanism.

And once more for the record: I claimed `theme::danger()` existed on the strength of a grep
that was matching **my own uncommitted edit**. The compiler caught it in seconds. That is the
fifth confident claim about this artefact to dissolve on contact (§52 has three, the
text-selection claim has one) — and the first where the false evidence was something I had just
written myself.

## 61. A machine that had no WSL, working (2026-08-05)

A third laptop, and the answer the last three sections were waiting for. Elevated install,
restart, reopen — **4 ok · 1 to fix · 1 optional**:

- ✓ WSL2 runtime — *a distro started and answered*
- ✓ Mini-Me backend — *langgraph.json found in ~/.local/share/mini-me-desktop/backend*
- ✓ Python dependencies — *.venv/bin/langgraph is installed*
- ✓ Host execution overlay — *installed with the backend*
- ! Asta CLI — signed out, and mid-device-code as the screenshot was taken
- ✗ Model API key — the one thing left, and it is a paste

Nobody typed a command. The app installed WSL, provisioned the checkout, ran `uv sync`, put the
overlay in place, and then diagnosed the two things only a person can supply. **This closes the
item that has been top of "blocks a second person using it" since §45** — a first run on a
machine that had never had any of this now works, and the remaining two rows are a sign-in and
a key.

The restart is the part worth remembering: on both machines that lacked WSL, `wsl --install`
replaced the WSL runtime and Windows would not register a distro until it rebooted. §60 taught
the app to say so. This laptop confirms that once it does reboot, everything downstream lands.

### The correction: `--no-launch` was a mistake, and it was mine

§60 added `--no-launch` to the install, reasoning that the post-install launch asks for a UNIX
username and that with output redirected the question would be invisible. The reasoning about
the hazard was right. The flag was wrong.

`--no-launch` is documented, and it also has a documented failure: it can install the distro
**without registering it** under `HKCU\Software\Microsoft\Windows\CurrentVersion\Lxss`, so
`wsl -l -v` does not list it and the only cure is to run the install again *without* the flag
([microsoft/WSL#10646](https://github.com/microsoft/WSL/issues/10646)). Our runtime probe asks a
distro to answer. An unregistered distro does not answer. The flag could therefore have
manufactured the exact state the button exists to escape — on the very path this laptop had
just proven.

Reverted. The hazard is handled where it belongs, by denying stdin: `< NUL` on the elevated
command. At EOF the username prompt gives up instead of waiting forever, the install and
registration follow the ordinary proven path, and the distro answers as root — which is all the
sidecar needs. An elevated fix can never be interactive anyway; its console is not one we can
put a question in.

Two things about how this was caught. It was caught by **research, before shipping**, because
this project's standing rule is that research is mandatory and one `WebFetch` of the WSL docs
and issue tracker settled it. And it was a change made to a command that had *just been proven
to work* — speculative hardening on a proven path, which is a worse trade than the hazard it
was guarding against. §60 counted five confident claims that dissolved on contact; this is the
sixth, and the first one I found before a user did.

## 62. Text selection, the thing this document said was impossible (2026-08-05)

For two months the plan recorded that selecting text in the transcript "is the one thing here
the framework genuinely makes hard — GPUI 0.2.2 cannot." §59's follow-up already corrected the
claim; this is the feature.

`gpui::TextLayout` exposes `index_for_position` (`elements/text.rs:483`) and its inverse
`position_for_index` (`:517`) — a hit-test and a caret position, which between them are
everything selection needs. What GPUI genuinely does *not* provide is selection **state and
painting**: nothing under `gpui/src/elements/` mentions the word, and `InteractiveText` offers
click and hover indices only. So the missing half is `crates/app/src/selection.rs`, and the
original claim was two thirds wrong.

**Not one big element.** The transcript is a tree of divs — headings, paragraphs, list items,
table cells, code blocks — each holding a `StyledText`. Replacing that with a single custom
element would have meant re-implementing Markdown layout, so instead each run of text is
wrapped in a `Selectable` that delegates layout and paint to the `StyledText` it holds and adds
exactly two things: it registers its `TextLayout` in a shared registry under an index assigned
in document order, and it paints the part of the selection that falls inside it. Bold, links,
inline code and rainbow CSV columns all still work *because nothing about them changed*.

Selection quads are painted **before** the glyphs. Painted after, the highlight covers the words
it is meant to highlight.

The registry is rebuilt every frame. Layouts move when the window resizes, when the transcript
scrolls and on every streamed token, and a rectangle from the last frame is a highlight over the
wrong words. The *selection* survives, because it belongs to the user — except when the
transcript is emptied, where it is cleared at all three sites: span indices are positions in one
conversation, and keeping one across a thread switch would highlight whatever text happened to
land in the same place.

**Copy, without stealing a key.** `ctrl-c` is bound with no key context, the same reasoning as
Escape in §58: focus lives in the composer almost always, so a workbench-scoped binding would
never be reached. The composer's own `ctrl-c` is more specific and still wins — it just calls
`cx.propagate()` when it has nothing selected, handing the shortcut down. So copying out of the
transcript needs no click to move focus first, and copying out of the composer is unchanged.
"Select everything" is `ctrl-shift-a`, not `ctrl-a`: that one belongs to the composer, where it
selects the prompt being typed. Both are also in the command palette, because a reader who has
never met this app will not guess either.

### What is tested, and what a person still has to look at

Twelve tests. The selection algebra — a click with no drag selects nothing, dragging upwards
selects the same text as dragging down, a span passed straight through is covered end to end, an
offset past the end of a span is clamped rather than panicking, a new frame keeps the selection
and drops the rectangles.

And the geometry, which is the part that cannot be eyeballed on a headless machine: `rows_between`
was split out of the painting so it could be tested with plain numbers — one line is one
rectangle between the two points, a wrapped selection runs to the edge on every line but the
last, a selection ending exactly at a line start draws no sliver, and a zero line height does not
loop forever. Counted rows rather than `y += line_height` until it passes `to.y`: that compares
f32s against a bound it can overshoot, and a selection that drops or doubles its last line is a
bug that only appears on the one paragraph that wraps.

Three assumptions were checked against the pinned crate rather than assumed, since the whole
premise of this section is a claim that was not: `StyledText::prepaint` does set
`element_state.bounds` (so `TextLayout::bounds()`, which unwraps twice, cannot panic on a span
registered after its inner prepaint); `chat_pane` is called exactly once per render (so span
indices are handed out in document order and cannot collide); and `cx.propagate()` exists
(`app.rs:1720`), which is what the `ctrl-c` handoff rests on.

**Unverified: everything a person sees.** This machine has no display, so no one has yet dragged
across a paragraph and watched the highlight appear. The algebra and the geometry are tested, the
panics are reasoned out against the crate source, and the failure modes left are visual — a tint
too faint, a highlight a pixel off a descender, a drag that feels wrong at the edges of a table
cell. Those need the researcher's eyes, and saying so is more use than claiming they are done.

## 63. A stop button that stops something (2026-08-05)

Since §52 the stop button has been honest and useless: it said *"cancelling a running turn is
not built yet"* and meant it. What it needed was a name for the thing to stop.

**The run id was arriving all along and being thrown away.** The first frame of every stream is
`event: metadata`, and `tests/fixtures/delegated-turn.sse` — captured from a real turn — shows
its payload is `{"run_id":"019fb670-…","attempt":1}`. The decoder mapped that frame to
`Status("run started")` and dropped the rest. It now also emits `TurnEvent::Started { run_id }`,
which the sidecar notes on the way past.

**Aborting our own stream is not cancelling.** This is the part that would have made a
half-finished feature look finished. Dropping the SSE response closes the connection, and
LangGraph's `on_disconnect` defaults to `continue` — so the graph keeps running, and an agent
that spends tokens per step keeps spending them, with nobody reading the answer. Stop therefore
does both: `POST /threads/{id}/runs/{id}/cancel` and then aborts the local task.

Both facts came from the SDK vendored in the reference checkout rather than from memory:
`DisconnectMode = "cancel" | "continue"` and `cancel(threadId, runId, wait?, action?)`, whose
default action is `interrupt`. `interrupt` is the right one here — `rollback` would erase the
partial answer the reader is looking at, which is real work and quite possibly the reason they
pressed stop.

Three smaller decisions, each about not lying:

- **The partial answer stays**, and is *marked*: "you stopped this turn; the answer above is
  incomplete". A truncated answer and a complete one are otherwise identical on screen, and the
  difference decides whether the thing can be relied on. A stopped message with no body at all
  also stops counting as silent, so the pruner cannot delete the only record that it happened.
- **Resumed continuations are cancellable too.** An approved command is often the slowest part
  of a run, which is exactly when someone reaches for stop.
- **The window where there is no id** — the few milliseconds before the first frame — reports
  *"stopped watching — the run had not reported an id yet, so the backend may still be
  finishing it"*, rather than "turn stopped". Two different things happened; they get two
  different sentences.

## 64. Right-click, and a paste that had been broken since §55 (2026-08-05)

§62 gave the transcript something worth copying and then asked the reader to know that `ctrl-c`
is how. A right-click is what everyone tries first.

GPUI ships no menu widget, the same way it ships no text input. It does ship `anchored`, which
places a child at a point in the window and keeps it inside the frame, and `deferred`, which
paints it after everything else so it is not clipped by the pane it opened over. Those two are
the whole mechanism.

No menu item decides what it does — each names a method that already exists and is already on a
key, so the menu is a second door onto the same room rather than a second implementation. Cut
and Paste are **absent** from the transcript rather than greyed, because it is not editable and
never will be; inside the composer they are always present and greyed when unavailable, because
there they are only temporarily out of reach. Every row shows its own binding, and Select all
shows a *different* one per target, because it genuinely is a different key.

Escape closes the menu before anything else, innermost-first per §58. A right-click elsewhere is
handled only by the opener: closing on click-out as well would race it, and which won would
depend on paint order — sometimes leaving no menu at all.

**And the thing this turned up.** Wiring Paste meant reading the composer's paste handler, which
still said *"Newlines would break single-line layout; flatten them"* and replaced every `\n`
with a space. That comment was true when it was written and stopped being true in §55, which
made the composer genuinely multi-line — for the express purpose of accepting "a prompt carrying
a script, a table or a list". So for two months pasting exactly that silently ran it all
together, and the module header still described the field as "single-line by design".

Both fixed. `\r\n` is normalised on the way in, because a Windows clipboard is full of it and a
stray `\r` shapes as a box.

Worth naming as a category: this is the third defect found by *reading code adjacent to the one
being changed* rather than by testing the change — §60's empty-log message and §59's
already-correct-pattern-in-the-same-file being the others. A stale comment describing behaviour
that has since changed is not a documentation problem; it is a bug with a note attached.

## 65. Blockquotes, nested lists, and an image that was showing its punctuation (2026-08-05)

The last of the Markdown gaps. All three are what a coordinator writing a report actually emits,
and until now all three rendered as source.

**Nested lists are depth from a stack, not from arithmetic.** The obvious implementation is
`indent / 2`, and it is wrong: agents write two-space *and* four-space nesting, sometimes in the
same answer, so a fixed divisor renders one of them flat and the other twice as deep as written.
Instead the parser keeps the indents it has actually seen in the current list, shallowest first;
its length is the depth. Deeper than the top opens a level, shallower closes as many as it must.
Two-space and four-space sources therefore produce identical trees, and no level can be invented
that the source did not indent for. A tab counts as four columns, because counted as one it
would sort a tab-indented child *above* a two-space one.

A numbered item keeps the number its author wrote at any depth. Renumbering it — or swapping in
a bullet because it happens to be indented — would change what the answer says, and steps get
referred to by number.

**Blockquotes** fold consecutive quoted lines into one block and count their `>` markers, so a
quoted quote still reads as one. Emphasis inside is parsed normally. Rendered as a rule down the
left in muted text, because a quote is something the answer is *referring* to.

**The image was leaking its own syntax.** `![alt](url)` rendered as `!alt url`: the inline scan
stopped at `[`, so the bang was pushed as ordinary prose before the link branch ever saw it.
Adding `!` to the scan set is what fixes it — which then puts every exclamation mark in ordinary
writing through that branch, hence a test over `Done! Next?`, `Careful! [see here](x)`, `a ! b`
and a bare `!`.

An image alone on a line becomes a block; inside a sentence it stays inline, because promoting
it would tear the sentence in half.

**What the image block deliberately does not do is load the picture.** The path points into the
distro's filesystem, and translating it into something Windows can open does not exist in that
direction — §46 recorded the three spellings a path has here, and this is the missing fourth.
Meanwhile figures the agent really produced already appear beneath the answer, found by diffing
the thread's output directory *on the host* (§42). So the block names the file and stops. Showing
a broken image, or a path the reader cannot open, would be worse than a caption.

Eleven tests, which is the point of having done this one now: every claim above is checkable
without a window, unlike the UI debt sitting above it in the list.

### Why the component set is still not next

`Button`/`Label`/`Modal`/`Panel` has been top of the UI-debt list since §58, and it stays there
for now on a straight risk trade. It is a sweeping restyle of every control in a 6,000-line file,
its whole value is preventing a *visual* class of bug, and it cannot be verified from a machine
with no display — right after the app first worked end to end on someone else's laptop. The six
call-site mistakes it would prevent were each caught within a day. Blocking a release-adjacent
week on a blind refactor to prevent a bug class that is currently being caught cheaply is the
wrong order. It goes next to a window someone can look at.

## 66. The nested quote that had already vanished (2026-08-05)

§65 shipped blockquotes an hour before a real answer was asked to render one. The answer
contained `> First quoted line` followed by `>> Nested second-level quote`, and both lines came
out **run together on a single depth-1 rule**. The nesting was gone.

The fold collected every consecutive quoted line but kept only the *first* line's depth. So a
`>>` after a `>` was appended to the outer quote's text and its own depth thrown away — while
`Block::Quote` carried a `depth` field, and a test asserted `>>` alone produced depth 2. The
field worked; the fold never gave it the chance.

Now only lines at the *same* depth keep folding, with a test for both halves: a quote inside a
quote is its own block, and an ordinary two-line quote is still one block rather than two rules
stacked on each other.

Worth being precise about what the eleven tests in §65 did and did not buy. They caught nothing
here, because every one of them tested a single construct in isolation — `>>` on its own line
passed, `>` twice passed. The defect lived in the *transition* between two constructs, which is
exactly the shape unit tests miss and one real answer found immediately. Everything else in that
answer rendered correctly, including the thing the section was built for: two-space and
four-space nesting produced identical depth.

## 67. A button you cannot build wrong (2026-08-05)

The component set, finally, and starting with the half that could be verified from a headless
machine.

**What the survey said, rather than what I would have guessed.** Before writing anything, the
44 clickables in `main.rs` were counted by what they actually do: borders cluster into exactly
three — 36 `border()`, 12 `accent()`, 1 `error()`. Sizes into two — `px_3 py_1 text_sm` and a
compact `text_xs`. So `Tone` has two variants and `Size` has three, and every colour, padding
and radius in `ui.rs` was read out of the call sites it replaces. Migrating a button is meant to
change nothing on screen.

**Except that it changed twelve of them, which was the point.** §58 rounded the corners of the
app and missed **eleven bordered buttons**; a tighter scan while migrating found a twelfth,
`setup-to-settings` — the *Settings* button sitting immediately beside the already-rounded
*Re-check* in the pane every new user meets. `Install Ubuntu` and `Copy ⧉`, in every Setup
screenshot in this document, were square for two months. They are not any more, and not because
anyone remembered: `rounded_md` is not reachable to omit.

`Button` also makes `disabled` one flag instead of a colour plus a guard. Those were kept in
step by hand at each site, and the click listener is now attached *only* when the button is
live — so "disabled" cannot be true in the styling and false in the behaviour.

`Label::ellipsis()` is the §59 lesson as a type. `.truncate()` on a flex item with `min_w_0`
leaves zero intrinsic width and renders as nothing but an ellipsis; `ellipsis()` always pairs it
with `flex_grow`, and there is no way to ask for the broken combination. Six call sites, all of
which already had it right — the value is that the seventh cannot get it wrong.

### Three controls deliberately left hand-written

Each now carries a comment saying why, so the next person does not copy one as the template:

- **the provider pill** — has a *chosen* state with its own background; "which of these is
  picked" is a different control from "press this to do a thing";
- **the settings toggle** — a full-width checkbox row, where every `Button` is `flex_none`;
- **New** in the sidebar — brightens border *and* text on hover, which nothing else does. One
  call site is not worth a flag on a shared type.

### What is not here, and why

No `Modal`, no `Panel`. Both are about where actions sit relative to a scroll area — the other
repeated bug, three of them — and that means migrating thirteen scrolling panes whose only proof
is visual. `scrolling()` was written, found to have no honest caller in this pass, and deleted
rather than shipped unused. Same for a `Danger` tone: the one destructive control in the app is
a borderless chip inside an already-red confirmation row, so it is one of the twenty-three rows
this type deliberately does not cover, and a tone with no caller is a design vocabulary invented
ahead of the design.

That is the next increment, and it wants a window open.

## 68. Setup stops being a pane and becomes a page (2026-08-05)

Four screenshots of Zed's settings window settled a question this app had been answering badly:
**where does configuration live?** Zed's answer is one floating window with a nav rail down the
left, a page on the right, and every row shaped the same — title and description on one side,
the control that changes it on the other.

Ours had Settings as a centred modal and Setup as a 420px column in the right-hand slot. That
slot belongs to the research panel, so **opening Setup hid the outputs you were diagnosing it
about**, and Setup's own "Settings" button existed only to get from one to the other.

Both are now pages of one window: Appearance, Model, Research, Backend, Setup. Reaching Setup is
a click in the rail, and the button that used to shuttle between them is gone. The first blocked
report still opens the window by itself, on the page that says what is wrong — the guided first
run, unchanged in behaviour and no longer costing the chat 420px.

**`ui::Modal` is what made this cheap, and it is a shape rather than a style.** Actions inside
the scroll area is three separate bugs (§40, §41, §52) and the fix each time was moving one
`div` out of one container. Here `body` and `actions` are separate slots, so a Save button has
nowhere to scroll away to; `min_h_0` on the body is inside the component, so a long page cannot
push the buttons off the bottom (§40, §48, §51, §53). Both defects are now unreachable rather
than remembered.

Read out of `settings_ui.rs` rather than guessed from the pictures: Zed builds the rail from a
two-level `NavBarEntry` tree, and a row is `h_flex().justify_between()` with a title-and-
description stack against the control. The rail here is flat, because two levels is one more
than this app has sections for.

**The toggles gained a sentence each, and that is the real change.** They were `☑ Run code on
this machine` — one element that read the same way, which was the argument for them in the first
place and was true. But it made a setting's *name* and its *state* the same piece of text, so
nothing could say what the setting **did** without lengthening the line. Split into
`ui::setting_row` plus `ui::Toggle`, the description has somewhere to live: "Run code on this
machine" is a sentence about trust, not a preference, and the name alone never said so.

`ui::Toggle` is two flex boxes — `justify_end` moves the knob — rather than absolute positioning,
so there is no arithmetic to get wrong at a size nobody re-measures. It also retires one of the
three hand-written exceptions from §67: the checkbox row was only an exception because it was
full width, and a row that is a *row* plus a control that is `flex_none` is the shape that was
wanted all along.

**Still to do, and named so it does not quietly lapse:** Zed's theme picker is a dropdown
*button* that opens a searchable popup, and ours is still an always-open inline list. The
machinery for it exists — `anchored` + `deferred`, from the right-click menu in §64 — and it is
the next thing on this pane. The scrollbar, checked in the same pass, is fine.

## 69. Three ways to say which row is chosen (2026-08-05)

Reported: `ctrl-p`, Enter on **Settings**, nothing happens.

The palette computed its selection in three places and no two of them agreed.

```rust
// what got drawn as chosen
let selected = self.palette_selected.min(commands.len().saturating_sub(1));
// what the arrow keys moved from
let current = self.palette_selected.min(count - 1) as isize;
// what Enter ran
self.palette_commands(cx).get(self.palette_selected)   // <- no clamp at all
```

The render clamped, the navigation clamped, the activation did not. So the moment
`palette_selected` outran a filtered list, the palette drew the last row as chosen and Enter ran
either the *wrong* command or — past the end — nothing whatsoever. All three now go through one
`palette_choice`, so the row that is highlighted is by construction the row that runs.

**And the branch returned in silence**, which is why the report could only be "nothing happens":
a command that never ran and a command that ran and did nothing look identical from outside. It
now says so in the status bar.

Honest about the limit: typing notifies the query entity, whose observer resets the index to
zero, so a *freshly filtered* list self-corrects and I could not reproduce the exact sequence
from the description. What is certain is that one path on the reported route could silently do
nothing, and it cannot any more. If it recurs, the status line now distinguishes "no command
matched" from "the command ran".

Third time this shape has appeared: two call sites deriving the same fact separately and drifting
(§67's `disabled` colour beside its own guard, §60's fix verdict read from output instead of the
re-check). The fix is always the same — one function, both callers.

## 70. The UI polish list, and the item on it that was wrong (2026-08-05)

Six entries had been sitting under "UI debt" since §58. Five are done; one turned out to name
the wrong fix, and one is deferred with a reason rather than quietly dropped.

**The theme and model pickers are dropdowns.** A trigger showing what is chosen, opening a
searchable popup — Zed's shape, and the reason shows the moment there is more than a handful:
a hundred installed palettes is a hundred rows in a window that has four other settings in it.
The list itself did not change; it moved into a popup and gained a trigger, so hovering a row
still previews the palette (§50). Built from the same `anchored` + `deferred` pair as the
right-click menu (§64).

**Fenced code has a monospace face, and nothing was bundled.** A font is a multi-megabyte
binary, a licence to carry, and a rendering result nobody here can look at — and it buys nothing
on the platform that matters, because `Consolas` has shipped with every Windows since Vista.
`gpui::Font` takes `fallbacks`, so this is a stack: `Cascadia Mono`, then `Consolas`, then
`DejaVu Sans Mono` for the development machine. Zero bytes added. `HighlightStyle` carries no
font family, so this reaches fenced blocks only — *inline* code stays marked by colour, which is
what it already was.

**Focus rings.** Which field has the keyboard was invisible: there is a caret, and it is two
pixels wide. The composer and every settings field now light their border. `in_focus` rather
than `focus`, because the thing holding focus is a child entity and the border belongs to the
box around it — and `track_focus` is what ties the two together. That same handle is what makes
`tab_index` work: Tab walks the fields of a page in reading order and lands *in* the field
rather than on a div that swallows typing.

**Toasts.** The status bar holds one line, so "copied 12 lines" was routinely overwritten before
anyone looked at it. Outcomes now stack in the corner and fade on individual timers — a second
message must not cut the first one's time short — capped at three, and clickable to dismiss.
Deliberately only for things a *person did*: streaming progress still goes to the status bar,
because a toast per token is a wall of them.

**Resizable panels.** Both widths were fixed numbers. 240px of conversation list is generous on
a laptop and mean on a 4K monitor, and the person who knows which is the one looking at it. The
drag is tracked on the *root*, not on the four-pixel strip, because the pointer outruns four
pixels immediately. Clamped to 160–640: a pane dragged to nothing is one with no handle left to
drag back.

### The item that was wrong

The list said **"`uniform_list` for the transcript — every message is laid out every frame."**
The cost is real. The remedy was not: `uniform_list` "measures the first element and lays out
all remaining elements based on that measurement" and "only works for elements with uniform
height". Transcript messages are a one-line question and a two-page report. It would have
rendered every message at the height of the first.

The variable-height element is `list` + `ListState`, and it wants `splice` on every height
change — which for a message growing a token at a time is a splice per token, to virtualize a
list that is rarely more than a few dozen items.

So the actual cost got fixed instead: **the markdown was being parsed in `render`**, so every
message in the conversation was re-parsed sixty times a second for text that had not changed
since it arrived. Blocks are now cached beside the body and rebuilt only when it changes — once
for a finished message, once per token for the single one still streaming. Virtualization stays
on the list, honestly described, for when a conversation is long enough to need it.

### Deferred, and why

**SVG icons** instead of `◎ ▤ ▥ ⏎`. It needs an `AssetSource` registered on the app and a set of
hand-authored SVG files committed, to replace glyphs that render correctly today, for no
functional gain — and I cannot look at the result. It is the one item on this list where the
work is all risk and the payoff is all taste, so it waits for someone who can see it.

## 71. Four bugs from one pass of using it (2026-08-05)

§70 shipped five things at once. A single session with the app found four defects, and three of
them were invisible to every test in the suite because each lived in an *interaction* between
two pieces that are individually correct.

**Copy was greyed out over text that was visibly highlighted.** The span registry is rebuilt
each frame, and it was cleared at the top of `render`. But `render` is also where the
right-click menu decides whether to grey its Copy row — and it asks *before* prepaint has
registered anything. So the question "is there text to copy?" was answered against an empty map
every single time. `ctrl-c` kept working, because a key handler runs between frames when the map
is full, which is exactly the kind of half-working that hides a bug.

The registry is double-buffered now: prepaint fills the next frame's map while everything else
reads the last completed one. Nothing is ever read while half-populated.

**Escape stopped closing the preferences window.** Opening it focused `fields.first()` — the
Model page's first field. On Appearance or Setup that element is not rendered at all, and focus
on an unrendered element means key bindings stop arriving. Hence the shape of the report:
Escape did nothing *until you clicked Model*, which put that field back on screen. Focus now
goes to the first field of the page being shown, and pages that have no fields focus the window
itself — which is also why `Modal` gained a focus handle. Changing page moves focus for the same
reason. It is very likely the same fault behind "the focus ring doesn't work": the ring was
drawn correctly on an element nothing was focusing.

**A fenced code block rendered as an empty box, a stray paragraph and a second empty box.** All
three at once, which is the tell. An answer showing what a fenced block looks like wraps it in a
*longer* fence — four backticks around three — and the parser closed on any ` ``` `. So the outer
fence closed on the inner opener (empty block), the example became a paragraph, and the inner
closer opened a second fence. Fences are now measured, and close only on one at least as long,
which is what CommonMark says and what the failure was already telling me.

Worth noting how close I came to fixing the wrong thing: the only recent change to that code path
was the monospace font, and "empty box" reads exactly like a font that failed to resolve. Testing
`parse` before touching anything is what stopped a font revert from being shipped as a fix for a
parser bug. The font *did* get one change, but a defensive one: the primary family is now chosen
per platform from ones that are always installed, because `fallbacks` covers missing glyphs and
is not a promise about a missing family.

**Two identical filter boxes in the theme popup.** `theme_list` brings its own, and the popup
added another. Plainly visible in a screenshot, invisible to a test suite that never assembles
two components together.

The pattern across all four: every one is a *seam*. Render order against prepaint order, focus
against what is rendered, a parser against its own output, one component against another. The
unit tests were right about each piece and had nothing to say about any of the joins — which is
the second time in three days that a real screenshot found in seconds what the suite could not
reach (§66 was the first).

## 72. Two boxes that read identically and were not (2026-08-05)

The theme popup's gallery search rendered a quarter of the width of the filter box above it,
with its placeholder spilling out the right-hand side.

Both were `div().px_2().py_1().rounded_md().bg(…).border_1()` around a composer, and neither
said `w_full`. Both were relying on flex stretch to fill their column, and only one got it. I
did not chase down which nesting difference decided that, because the answer would have been a
fact about taffy rather than about this app: the fix is that a text field states its own width
instead of inheriting an argument.

So the gallery box is now the same `filter_field` as the theme box — one function, both callers
— and it gained the focus ring the hand-written copy never had. That is the fourth time this
week the fix has been "these two places compute the same thing separately" (§60, §67, §69), and
the third where making it one function removed a bug nobody had reported yet.

## 73. Proposed — the provenance graph (2026-08-05)

Requested: a modal showing which subagents a conversation used and how it moved between them.
The example given is the point of it:

> paper search → theories → get data → clean data → analyze data → theories → paper search

"This shows how in reality science works, so each scientist can track his work by conversation."

That sentence is the specification. Not a picture of the machinery — a **record of the enquiry**:
what was consulted, what it led to, and where the work doubled back. The doubling back is the
part worth building for. A run that goes out to the literature, forms a theory, gets data,
analyses it, and *returns to theories with what it found* is not a pipeline that malfunctioned;
it is the loop the whole method is made of, and it is invisible today.

### It is not a DAG, and the request already says so

"DAG-like (I think there can be cicles)" is exactly right, and it settles the data structure. The
acyclic assumption is what would have to be forced on this, and forcing it would delete the one
edge a scientist most wants to see — the return to a step already taken. So: a **directed
multigraph over subagent kinds**, cycles allowed and expected.

- **Node** = a kind of subagent (`paper search`, `theories`), *not* an invocation. `theories`
  appearing twice in the example is one node visited twice, which is what makes the loop visible
  at all. Visit count belongs on the node.
- **Edge** = an observed transition, with a count. `theories → paper search` traversed three
  times is one edge that says three, not three edges.
- **Order** = across the whole conversation, not one turn. The loop in the example spans turns.

### What exists, and the one thing that does not

The live data is already there. `Message.agents` is `Vec<AgentTrace>`, one entry per invocation,
carrying `ns` (unique per invocation, from the pregel checkpoint namespace) and `name` (the
display name the backend sends as `lc_agent_name`, §15b). Entries are appended in first-seen
order, so a turn's sequence is recoverable, and `Message` order gives the conversation's.

**What does not exist is any of it after a reload.** `conversation_messages` returns role and
text only, and says why: *"the activity trace is not replayable — it was assembled from a stream
that is over, and pretending otherwise would show an empty trace next to a real answer."* That
was the right call for the transcript. It is fatal here: a graph that empties when you reopen the
conversation is not a record of your work, and "track his work by conversation" is precisely the
thing it would fail at.

**So this feature is a persistence problem before it is a drawing problem**, and that ordering
should survive contact with the fun part. The graph is appended to as a turn streams and written
to the thread's own directory — `Documents\Mini-Me\<thread>\provenance.json`, beside the outputs
that turn produced (§42's directory, already ours, already the place a researcher is pointed at).
Written by the client because the client is the only thing that sees the stream.

### The honest limit, named before it is discovered

Adjacency in that list is **arrival order, not causation.** Subagents run concurrently — the `ns`
namespace exists precisely so two concurrent runs of the same kind stay in separate groups — and
nothing carries a timestamp or a parent. So `A` appearing before `B` may mean A led to B, or may
mean both were dispatched together and A's first token arrived first.

Three options, in order of preference:

1. **Ask the backend for the edge.** A delegation is a `task` tool call made *by* something; the
   coordinator knows which. If the stream can carry the parent, the graph stops being inferred.
   This is the only version that is true rather than plausible, and it is a small upstream
   question — worth asking before building the inference.
2. **Draw concurrency as concurrency.** Group invocations that overlap into a band rather than a
   chain, so the picture says "these two ran together" instead of inventing an order.
3. **Label the inference.** If the edges stay heuristic, the modal says so in one line. A
   provenance record that quietly guesses is worse than no provenance record, because it will be
   believed.

### Drawing it

GPUI has no graph layout and there is no crate to add. Both pieces needed do exist: `canvas()`
for arbitrary drawing (`elements/canvas.rs`) and `window.paint_path` for the edges. Two stages:

- **First, the chain.** Chips with arrows between them, wrapping — literally the notation the
  request was written in, which is a strong hint that it reads. Revisits repeat the chip; a
  repeated chip *is* the cycle, legible with no layout algorithm at all. This is a day's work on
  data that already exists and answers the question as asked.
- **Then the graph**, if the chain proves it wants one: nodes placed by first appearance, edges
  as curves, thickness by traversal count. Layered layout by hand — a real algorithm, and only
  worth it once the chain has shown which conversations are complex enough to need it.

Modal rather than a panel, and reached from the command palette and the conversation's row: it is
something you open, read and close, which is the same argument §68 used to move Setup. `ui::Modal`
already has the shape.

**Sequenced after `/subagent`**, deliberately. `/subagent` gives a researcher a way to *invoke*
work deliberately rather than hoping the coordinator delegates; this shows what was invoked.
Building the record first would mean building it twice — once for the delegations that happen
now, and again for the ones a researcher asks for by name.
