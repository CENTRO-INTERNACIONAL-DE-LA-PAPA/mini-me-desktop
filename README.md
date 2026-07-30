# Mini-Me Desktop

A native desktop **research-acceleration workbench** for
[Mini-Me](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me), built in
Rust on **GPUI** (the GPU UI framework from [Zed](https://github.com/zed-industries/zed)).

This repo is the desktop **client**. The Mini-Me agent stack (coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** the client spawns and supervises — which also means the app inherits
the local `asta` CLI's auto-refreshing auth, so the web app's token-expiry pain
goes away.

> **Status: P6.2 — the app talks to the real agent stack.** The workbench window
> renders natively (P6.1, verified on Windows/DirectX) and the app now **spawns
> the local Python sidecar, health-checks it, and streams a real coordinator
> turn** over LangGraph SSE (verified end to end on Linux: 75 chunks / 423 chars,
> `--check-backend --stream`). GPUI is pinned at published **`gpui 0.2.2`**.
> Remaining for P6.2: confirm the live token stream visually in a desktop
> session. Read [`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) — the risk
> register plus the P6.1 (§8) and P6.2 (§9) execution logs.

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

`cargo run` must be launched from a graphical session (Wayland/X11 + Vulkan, or
DirectX on Windows) — it can't open a window from a headless TTY.

### The backend sidecar

The app spawns and supervises the Mini-Me Python backend itself; it attaches to
one that's already running rather than double-spawning. It needs the Mini-Me
checkout (with its `.env`) — found via `MINIME_BACKEND_DIR`, else conventional
locations. Sidecar logs go to `/tmp/mini-me-desktop-backend.log`.

To exercise the whole backend path **without a display** (spawn → health →
thread → stream), which is also the fastest way to debug a bad turn:

```bash
cargo run -p mini-me-desktop-app -- --check-backend --stream
```

Drop `--stream` to stop before the model call. Env overrides:
`MINIME_BACKEND_DIR`, `MINIME_BACKEND_PORT`, `MINIME_BACKEND_URL`,
`MINIME_BACKEND_ATTACH_ONLY`.

## Direction

Chosen over Tauri (the lower-risk fallback) to get a native, GPU-rendered,
keyboard-first workbench — "the best of Zed" for scientific discovery. See the
spike plan for the milestone ladder (P6.1 hello-window → P6.2 real backend →
P6.3 panels → P6.4 native affordances) and the go/no-go kill-criteria.

Org policy: human-gated (nothing auto-runs). AI-assisted (Claude Code) per CIP
Acceptable Use policy.
