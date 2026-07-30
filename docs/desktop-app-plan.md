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
| **P6.2** — talk to the real backend | 🟡 **backend path verified end to end** (spawn → health → thread → 75-chunk streamed turn). Remaining: confirm the live token stream *visually*. §9 |
| **P6.3** — port the core panels | ⬜ next |
| **P6.4** — native affordances | ⬜ not started |

**Health of the bet.** The two risks that could have killed this are both down:
**R1** (GPUI as an unstable `git` dep) — GPUI is a *published* crate, pinned at
`gpui 0.2.2`. **R2** (API churn) — the P6.0 sketch compiled against it unchanged.
What remains is scope risk (**R3**: rebuilding rich UI) and packaging (**R4**),
both of which are work, not uncertainty.

**Open decision on the table:** whether the local backend should keep using the
**remote LangSmith sandbox** for execution or switch to a **local execution
backend** now that everything runs on the user's machine — see §10.

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
- 🟡 **P6.2 — Talk to the real backend.** `BackendSupervisor` spawns the Python
  sidecar, health-checks it, and streams **one real coordinator turn** end to end;
  render the assistant text as it arrives. *Backend path verified headlessly
  (§9); the on-screen stream still needs a human look.*
- ⬜ **P6.3 — Port the core panels.** Artifacts/Outputs, the project spine
  (mission + completed/pending), the plan/Autopilot panel — the workbench
  identity. Plus the two P6.2 deferrals: a **text composer** (P6.2 uses one
  seeded prompt) and the **command palette**; and `values`/`custom` stream events
  for state + `sandbox_status`.
- ⬜ **P6.4 — Native affordances.** Local file → analysis, background-run tray +
  notifications, keychain-stored keys, multi-window. Also: Windows process-tree
  teardown via a Job Object (§9), and sidecar packaging (R4).

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

**Open:**

- **Execution locality (§10, new):** keep the remote **LangSmith sandbox** for
  code/`asta` execution, or move to a **local execution backend** now that the
  whole stack is local? Affects the Mini-Me repo, not this one.
- **Rust capacity:** an organizational gate (R6) — sustained Rust availability.
- **Packaging (R4):** bundling Python + `asta` per OS is deferred past MVP.

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
headless TTY.

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

## 10. Open decision: execution locality (remote sandbox → local?)

**The question.** Today the agent executes code and the `asta` CLI in a **remote
per-thread LangSmith sandbox**, even when the backend itself runs on the user's
machine. Now that the whole stack is local, should execution move to the host —
via deepagents' `LocalShellBackend`? Raised 2026-07-30; **not yet decided.**

> **Scope warning.** This is a change to the **Mini-Me repo**, which this project
> treats as **read-only reference** (it has open PRs). Nothing here is actionable
> without explicit sign-off, and it should land as its own branch/PR *there*, not
> as edits entangled with in-flight work.

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
remote sandbox as the default makes it reversible and lets the desktop app opt in.
