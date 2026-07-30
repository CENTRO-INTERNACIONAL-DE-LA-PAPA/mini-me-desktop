# Mini-Me Desktop

A native desktop **research-acceleration workbench** for
[Mini-Me](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me), built in
Rust on **GPUI** (the GPU UI framework from [Zed](https://github.com/zed-industries/zed)).

This repo is the desktop **client**. The Mini-Me agent stack (coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** the client spawns and supervises — which also means the app inherits
the local `asta` CLI's auto-refreshing auth, so the web app's token-expiry pain
goes away.

> **Status: P6.1 — it builds.** `cargo build -p mini-me-desktop-app` is green on
> Linux against **`gpui 0.2.2`** (crates.io — GPUI turned out to be a *published*
> crate, not a `git`-only dep, which retires the biggest risk in the register).
> Visual window-check (`cargo run` in a graphical session) is the one remaining
> step. Read [`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) — the risk
> register and the P6.1 execution log (§8).

## Layout

```
crates/app        the desktop binary (GPUI app + backend supervisor)
docs/             the Phase 6 spike plan
```

## Build

On Linux (Ubuntu 22.04), install the GPUI system dev headers once:

```bash
sudo apt-get install -y libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev \
                        libasound2-dev libvulkan-dev
```

Then:

```bash
cargo build -p mini-me-desktop-app   # verified green (rustc 1.97.1, gpui 0.2.2)
cargo run   -p mini-me-desktop-app   # opens the workbench window (needs a display)
```

`cargo run` must be launched from a graphical session (Wayland/X11 + Vulkan) — it
can't open a window from a headless TTY.

## Direction

Chosen over Tauri (the lower-risk fallback) to get a native, GPU-rendered,
keyboard-first workbench — "the best of Zed" for scientific discovery. See the
spike plan for the milestone ladder (P6.1 hello-window → P6.2 real backend →
P6.3 panels → P6.4 native affordances) and the go/no-go kill-criteria.

Org policy: human-gated (nothing auto-runs). AI-assisted (Claude Code) per CIP
Acceptable Use policy.
