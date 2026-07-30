# Mini-Me Desktop

A native desktop **research-acceleration workbench** for
[Mini-Me](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me), built in
Rust on **GPUI** (the GPU UI framework from [Zed](https://github.com/zed-industries/zed)).

This repo is the desktop **client**. The Mini-Me agent stack (coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** the client spawns and supervises — which also means the app inherits
the local `asta` CLI's auto-refreshing auth, so the web app's token-expiry pain
goes away.

> **Status: P6.0 — kickoff scaffold.** Skeleton + plan only; not yet a working
> app. This was authored in an environment without a Rust toolchain, so the code
> is **not compile-verified** — P6.1's first job is to make it build. Read
> [`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) first, including the
> honest risk register (GPUI is a `git`-only, API-unstable dependency).

## Layout

```
crates/app        the desktop binary (GPUI app + backend supervisor)
docs/             the Phase 6 spike plan
```

## Build (needs a Rust machine)

```bash
# Prereqs: rustup, and on Linux the GPUI system deps (Vulkan/Wayland/X11).
# 1) Pin the gpui `rev` in crates/app/Cargo.toml (see TODO).
# 2) Reconcile crates/app/src/main.rs against that rev's examples/ API.
cargo run -p mini-me-desktop-app
```

## Direction

Chosen over Tauri (the lower-risk fallback) to get a native, GPU-rendered,
keyboard-first workbench — "the best of Zed" for scientific discovery. See the
spike plan for the milestone ladder (P6.1 hello-window → P6.2 real backend →
P6.3 panels → P6.4 native affordances) and the go/no-go kill-criteria.

Org policy: human-gated (nothing auto-runs). AI-assisted (Claude Code) per CIP
Acceptable Use policy.
