# Mini-Me Desktop — Phase 6 spike plan (P6.0)

A native **desktop research-acceleration workbench** for Mini-Me, built in Rust
on **GPUI** (the GPU UI framework extracted from [Zed](https://github.com/zed-industries/zed)).
This repo is the desktop **client**; the Mini-Me agent stack (the coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** that the client spawns and supervises.

> Status: **P6.1 — PASS (go/no-go cleared).** `cargo build` is green **and** the
> three-pane workbench window renders natively (verified on Windows / DirectX,
> 2026-07-30). GPUI pinned at published **`gpui 0.2.2`** (crates.io). Next:
> **P6.2** — spawn the local Python sidecar + stream one real coordinator turn.
> See §8 for the execution log; risk-register R1 is now **downgraded** — GPUI
> turned out to be a *published* crate, not a `git`-only dependency.

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

- **P6.0 — Spike doc + skeleton** *(this repo, now).* Plan + a Cargo workspace +
  a root view sketch + a sidecar-supervisor stub. Not yet buildable-verified
  here (no Rust toolchain in the authoring environment).
- **P6.1 — "Hello workbench" (go/no-go).** On a Rust machine: pin the `gpui` rev,
  get **one window** with a command palette and a chat pane that streams a
  hard-coded response. Reconcile `main.rs` against the pinned GPUI API. *Kill-
  criterion R1.*
- **P6.2 — Talk to the real backend.** `BackendSupervisor` spawns the Python
  sidecar, health-checks it, and streams **one real coordinator turn** end to
  end; render the assistant text as it arrives.
- **P6.3 — Port the core panels.** Artifacts/Outputs, the project spine (mission +
  completed/pending), and the plan/Autopilot panel — the workbench identity.
- **P6.4 — Native affordances.** Local file → sandbox path, background-run tray +
  notifications, keychain-stored keys, multi-window.

**MVP acceptance:** a launchable app that opens a project, runs a real coordinator
turn against the local sidecar, streams the answer, renders the artifacts/spine
panels, and does **one** thing the web app can't (local file → analysis, or a
background-run notification).

---

## 6. Open decisions (locked 2026-07-29)

- **Repo shape:** ✅ **separate repo** (`mini-me-desktop`) — this one.
- **Backend locality:** ✅ **local sidecar** (spawn the Python backend locally;
  inherits the auto-refreshing `asta` auth).
- **GPUI dependency:** pin a `rev` in `crates/app/Cargo.toml` (P6.1). Vendoring is
  a fallback if the git dep proves unstable.
- **Rust capacity:** confirm sustained Rust availability before P6.1.

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
