# Mini-Me Desktop — Phase 6 plan & status

A native **desktop research-acceleration workbench** for Mini-Me, built in Rust
on **GPUI** (the GPU UI framework extracted from [Zed](https://github.com/zed-industries/zed)).
This repo is the desktop **client**; the Mini-Me agent stack (the coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** that the client spawns and supervises.

## Where we are now (updated 2026-07-30)

| Milestone | Status |
|---|---|
| **P6.0** — spike doc + scaffold | ✅ done |
| **P6.1** — buildable window *(go/no-go gate)* | ✅ **PASS** — builds green; window renders natively (verified on Windows/DirectX). §8 |
| **P6.2** — talk to the real backend | ✅ **done** — a real coordinator turn spawned, streamed and rendered **on Windows** (2026-07-30). §9 |
| **P6.2.5** — local-first backend (drop LangSmith/WorkOS) | 🔴 **queued** — the change that makes an installable app possible. §10/§11 |
| **P6.3** — port the core panels | 🟡 **in progress** — composer + scrolling transcript done (§12); spine/artifacts/palette next |
| **P6.4** — native affordances + shipping | ⬜ not started |

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
  2. ⬜ **Project spine** — `GET /project` → `{mission, completed, pending,
     suggestions}` instead of the hardcoded mission.
  3. ⬜ **Artifacts/Outputs** — the `values` stream event (`artifacts`, `todos`)
     plus `GET /files/{thread_id}`.
  4. ⬜ **`sandbox_status`** from `custom` stream events in the status line.
  5. ⬜ **Command palette** — Zed-style `Ctrl-P`: run turn, new thread, switch
     project.
- ⬜ **P6.4 — Native affordances + shipping.** Local file → analysis,
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

**Open:**

- **Where the §10 change lands:** a branch/PR in Mini-Me, or a thin overlay in
  this repo? *Needs sign-off — nothing in Mini-Me has been touched.*
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
- **`--check-backend [--stream]`** — a headless self-check that exercises
  spawn → health → thread → stream with **no window**, so the contract is
  testable on a headless machine (and doubles as a debug tool).

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

> **Scope gate.** The change itself lands in the **Mini-Me repo**, which this
> project treats as read-only reference (it has open PRs). Still **awaiting
> sign-off on where the code lands** — a branch/PR there, or a thin overlay here.

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

**Decision needed before writing P6.2.5.** Not resolved yet.
