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

## Picking this up

New to the repo, or taking over development? Read these two first, in order:

1. [`docs/handover.md`](docs/handover.md) — what the project is, the rules that are not
   negotiable, the failures that cost weeks and how to avoid repeating them, and how to
   build, test and release.
2. [`docs/plan.md`](docs/plan.md) — the open work as a checklist, newest state first.

[`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) is the long-form record behind
both: one numbered section (§N) per problem and what was done about it. The `§` markers
in code comments point here, and they are how a decision gets reconstructed later.

## Layout

```
crates/app        the desktop binary (GPUI app + backend supervisor)
docs/             the design record, the handover, and the open-work checklist
```

## Windows (the primary platform)

~98% of our users are on Windows. The app runs **natively** on Windows (GPUI uses
DirectX), while the Python backend runs **inside WSL2** — the agent stack shells out
with POSIX commands and needs `bash`/`python3`/`asta`, which don't behave under
`cmd.exe`. Inside WSL it's just Linux, and the app reaches it over localhost.

### For someone who is just using the app

Launch it. The **Setup** pane opens by itself and says what is missing, with a button
for each thing it can do for you — install WSL, install Mini-Me, install the Python
packages — showing the output as it runs. Then paste a model key in **Settings**.
Nothing needs to be typed in a terminal, and nothing needs to be edited in a file.

If a step can't be automated (installing WSL needs administrator rights and a
restart), the pane says so before you press it, and offers the command to copy.

### For whoever prepares the build

Run this **once**, on a machine that has GitHub access:

```bash
bash scripts/bundle-backend.sh
```

Mini-Me is a **private** repository, so `git clone` wants a personal access token —
a wall for the people this app is for. That script puts a pinned, unmodified copy in
`vendor/` (gitignored), and every install after that provisions from it without ever
contacting GitHub.

### For development

```powershell
$env:MINIME_BACKEND_WSL=1; cargo run -p mini-me-desktop-app
```

The app launches the backend inside WSL itself. It provisions into
`~/.local/share/mini-me-desktop/backend` — on the distro's **own** filesystem, because
a Python venv reached over `/mnt/c` is slow enough to feel broken. Point it at your
own checkout with `MINIME_BACKEND_WSL_DIR` (or `MINIME_BACKEND_DIR` outside WSL); the
app will run that one but **never modify it**, since it may hold your work.

To see what the pane would say, with no window:

```bash
cargo run -p mini-me-desktop-app -- --preflight
```

## Backend prerequisite

Provisioned for you by the Setup pane. By hand, it is:

```bash
bash scripts/setup-wsl.sh [target-dir]
```

**`--extra dev` matters** (the script passes it) — the LangGraph CLI is an optional
extra, so plain `uv sync` leaves you with no `langgraph` entry point. Keys do **not**
go in that checkout's `.env` any more: they live in your OS keychain and travel with
each request, so the app needs no secrets on disk.

## Git inside WSL asks for a password

Only on the **Windows** side does git have a credential helper; inside the distro it does
not, so `git pull` there prompts — and GitHub has not accepted account passwords since
2021, so the prompt cannot be satisfied. Reuse Windows' credential manager:

```bash
git config --global credential.helper "/mnt/c/Program Files/Git/mingw64/libexec/git-core/git-credential-manager.exe"
```

(If that path is wrong, `ls /mnt/c/Program\ Files/Git/mingw64/libexec/git-core/ | grep credential`.)

Worth knowing which repo the failure names: `bundle-backend.sh` itself needs **no**
network, since it clones from a checkout already on the machine. A prompt mentioning
`mini-me-desktop.git` is the `git pull` in front of it, not the bundle.

## Release builds need `fxc.exe` (Windows)

A **release** build of `gpui 0.2.2` pre-compiles its HLSL shaders; a debug build does not
(`build.rs:259` gates the step on `#[cfg(not(debug_assertions))]`). So `cargo build` can
work for months and `cargo build --release` still fail with:

```
Failed to find fxc.exe
```

`fxc.exe` is the DirectX shader compiler from the **Windows SDK**. gpui looks for it in
`GPUI_FXC_PATH`, then on `PATH`, then at one hardcoded SDK version
(`10.0.26100.0`) — so having a *different* SDK version installed is enough to fail.

Point it at yours (PowerShell):

```powershell
$env:GPUI_FXC_PATH = (Get-ChildItem "C:\Program Files (x86)\Windows Kits\10\bin" -Recurse -Filter fxc.exe -ErrorAction SilentlyContinue | Sort-Object { $_.FullName -notmatch '\\x64\\' }, FullName -Descending | Select-Object -First 1).FullName
```

Check it found something (`echo $env:GPUI_FXC_PATH`), then build. To avoid repeating it
every session:

```powershell
setx GPUI_FXC_PATH "$env:GPUI_FXC_PATH"
```

If the search comes back empty there is no SDK on the machine: install **Windows 11 SDK**
from the Visual Studio Installer (Individual components), then try again.

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
