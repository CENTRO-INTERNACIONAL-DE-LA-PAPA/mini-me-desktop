# Mini-Me Desktop

A native desktop **research-acceleration workbench** for
[Mini-Me](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me), built in
Rust on **GPUI** (the GPU UI framework from [Zed](https://github.com/zed-industries/zed)).

This repo is the desktop **client**. The Mini-Me agent stack (coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** the client spawns and supervises — which also means the app inherits
the local `asta` CLI's auto-refreshing auth, so the web app's token-expiry pain
goes away.

> **Status: P6.3 done — the core panels work against the real agent stack.** The
> window renders natively (verified on Windows/DirectX), the app **spawns the local
> Python sidecar and streams real coordinator turns** over LangGraph SSE, and it now
> shows the **project spine**, live **outputs**, sandbox provisioning, an **agent
> activity trace** (what each subagent is doing, while it does it) and a **`ctrl-p`
> command palette**. One thread spans the conversation, so follow-up questions work.
> GPUI is pinned at published **`gpui 0.2.2`**.
>
> Next: **markdown rendering** — answers currently show their `**asterisks**`, and
> reports and citations are the deliverable (§16). Read
> [`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) — the risk register plus the
> execution logs (§8–§17).

## Layout

```
crates/app        the desktop binary (GPUI app + backend supervisor)
docs/             the Phase 6 spike plan
```

## Windows (the primary platform)

~98% of our users are on Windows. The app runs **natively** on Windows (GPUI uses
DirectX), while the Python backend runs **inside WSL2** — the agent stack shells out
with POSIX commands and needs `bash`/`python3`/`asta`, which don't behave under
`cmd.exe`. Inside WSL it's just Linux, and the app reaches it over localhost.

In WSL (Ubuntu):

```bash
bash scripts/setup-wsl.sh
```

That installs `uv`, clones the backend, runs `uv sync --extra dev`, and writes a
`.env` template for your keys. Then, from Windows:

```powershell
$env:MINIME_BACKEND_WSL=1; cargo run -p mini-me-desktop-app
```

The app launches the backend inside WSL itself. Override the checkout path with
`MINIME_BACKEND_WSL_DIR` (default `~/Mini-Me`).

## Backend prerequisite

The app spawns the Mini-Me Python backend as a sidecar. In that checkout run:

```bash
uv sync --extra dev
```

**`--extra dev` matters** — the LangGraph CLI is an optional extra, so plain
`uv sync` leaves you with no `langgraph` entry point. Populate `.env` too
(`OPENAI_API_KEY`, `ASTA_API_KEY`, `ASTA_TOKEN`, and for now `LANGSMITH_API_KEY`).
The app looks for the checkout via `MINIME_BACKEND_DIR`, else
`~/Documents/Mini-Me` or `~/Documents/GitHub/Mini-Me`.

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

Drop `--stream` to stop before the model call, or swap it for
`--prompt "find the deseq2 paper"` to run your own — which is how a *delegating*
turn gets checked: the output then lists each step and a per-subagent tally.
Repeat `--prompt` to run several turns **on one thread**, which is how conversation
continuity gets verified without a window.

To decode a **saved** SSE capture instead, with no backend and no tokens spent:

```bash
cargo run -p mini-me-desktop-app -- --replay crates/app/tests/fixtures/delegated-turn.sse
```

Env overrides: `MINIME_BACKEND_DIR`, `MINIME_BACKEND_PORT`, `MINIME_BACKEND_URL`,
`MINIME_BACKEND_ATTACH_ONLY`.

### Settings

`ctrl-,` opens Settings: provider (Anthropic / OpenAI / Google / Mistral / any
OpenAI-compatible endpoint such as OpenRouter), model, API keys, and whether code runs on
this machine. **Keys go into your OS keychain, never into a file** — so you do not need to
edit the backend's `.env` at all. On a fresh install the pane opens by itself.

Headless equivalent, which never echoes the value:

```bash
cargo run -p mini-me-desktop-app -- --set-secret llm:anthropic "sk-…"
```

The model and key apply to the next turn. The port and execution locality are baked into
the sidecar's launch command, so those need a restart.

### Host execution (the default)

The agent's code runs **on this machine** — no LangSmith key, no cold start, no upload
dance. Files land in `~/.mini-me/workspaces/<thread>/`, where you can open them yourself.

**Every `execute` call stops and asks first.** The run pauses, the app shows you the
command verbatim, and nothing runs until you approve it. That is what makes running on
your own machine reasonable rather than reckless.

The remote LangSmith sandbox is still there if you want it:

```bash
cargo run -p mini-me-desktop-app -- --sandbox
```

`--local` / `--sandbox` override `MINIME_EXECUTION_BACKEND`, and are the better habit on
Windows: PowerShell has no `VAR=value cmd` prefix form, and a `$env:` assignment
outlives the command that needed it.

This works by putting [`overlay/`](overlay/) on the backend's `PYTHONPATH`; **the
Mini-Me checkout is not modified**. See [`overlay/README.md`](overlay/README.md) for the
mechanism and the plan's §18/§19 for the trade-offs. `MINIME_APPROVE_EXECUTE=0` disables
the gate — it exists for automation, and is not a recommendation.

## Direction

Chosen over Tauri (the lower-risk fallback) to get a native, GPU-rendered,
keyboard-first workbench — "the best of Zed" for scientific discovery. See the
spike plan for the milestone ladder (P6.1 hello-window → P6.2 real backend →
P6.3 panels → P6.4 native affordances) and the go/no-go kill-criteria.

Org policy: human-gated (nothing auto-runs). AI-assisted (Claude Code) per CIP
Acceptable Use policy.
