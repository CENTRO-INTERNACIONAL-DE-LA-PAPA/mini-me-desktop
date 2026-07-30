# Mini-Me Desktop — Phase 6 spike plan (P6.0)

A native **desktop research-acceleration workbench** for Mini-Me, built in Rust
on **GPUI** (the GPU UI framework extracted from [Zed](https://github.com/zed-industries/zed)).
This repo is the desktop **client**; the Mini-Me agent stack (the coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** that the client spawns and supervises.

> Status: **P6.0 — kickoff / scaffold.** This document is the risk-burndown and
> milestone plan; the code here is a skeleton to iterate on, not yet a working
> app. See "Honest risk register" before investing.

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
| R1 | **GPUI is not a stable published crate.** It ships inside the Zed monorepo; using it standalone means a `git` dependency on a large tree with an **unstable API**. | High | Pin a specific `rev`. Budget P6.1 purely to get a window on screen. **Kill-criterion:** if a "hello window" can't be built + run in ~2 days, fall back to Tauri. |
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

## 7. Build (once a Rust toolchain is present)

```bash
# On a machine with rustup + the GPUI system deps (Vulkan/Wayland/X11 on Linux):
cd mini-me-desktop
# 1) Pin the gpui rev in crates/app/Cargo.toml (see the TODO there).
# 2) Reconcile crates/app/src/main.rs against that rev's examples/ API.
cargo run -p mini-me-desktop-app
```

The authoring environment for this scaffold had **no `cargo`/`rustc`**, so the
skeleton is written but **not compile-verified**. P6.1's first task is to make it
build.
