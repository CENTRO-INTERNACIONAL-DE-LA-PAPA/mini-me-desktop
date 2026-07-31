# Mini-Me Desktop — Phase 6 plan & status

A native **desktop research-acceleration workbench** for Mini-Me, built in Rust
on **GPUI** (the GPU UI framework extracted from [Zed](https://github.com/zed-industries/zed)).
This repo is the desktop **client**; the Mini-Me agent stack (the coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** that the client spawns and supervises.

## Where we are now (updated 2026-07-31)

| Milestone | Status |
|---|---|
| **P6.0** — spike doc + scaffold | ✅ done |
| **P6.1** — buildable window *(go/no-go gate)* | ✅ **PASS** — builds green; window renders natively (verified on Windows/DirectX). §8 |
| **P6.2** — talk to the real backend | ✅ **done** — a real coordinator turn spawned, streamed and rendered **on Windows** (2026-07-30). §9 |
| **P6.2.5** — local-first backend (drop LangSmith/WorkOS) | ✅ **done, and now the default** — turns run on the host with no `LANGSMITH_API_KEY`/`WORKOS_*`, via a `PYTHONPATH` overlay that leaves the Mini-Me checkout untouched, and **every `execute` call waits for approval**. `--sandbox` still available. §18/§19 |
| **P6.3** — port the core panels | ✅ **done** — composer, spine, outputs, sandbox status, agent activity trace, **command palette**; plus conversation continuity (turns used to each start a new thread) |
| **P6.3.5** — visuals pass, starting with **markdown rendering** | ⬜ queued — answers currently render with their asterisks showing, and reports/citations are the deliverable. §16 |
| **P6.4a** — settings panel + keychain secrets | ⬜ **next** — the prerequisite for a clickable install: nobody is hand-editing a `.env` inside WSL. §20 |
| **P6.4b** — native affordances + shipping | ⬜ not started — click-to-update, local file → analysis, tray notifications, Windows Job Object teardown |
| **P6.5** — async subagents + Jobs panel | ⬜ planned — the payoff that most justifies going native. §14 |

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
- ⬜ **P6.4a — Settings panel + keychain secrets** (§20). `ctrl-,`, two stores
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
