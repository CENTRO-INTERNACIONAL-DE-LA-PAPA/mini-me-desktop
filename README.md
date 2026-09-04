# Mini-Me Desktop

A native desktop **research-acceleration workbench** for
[Mini-Me](https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me), built on
**Tauri** (a Rust shell hosting the OS's native webview) with a **React + TypeScript**
frontend built by **Vite**.

This repo is the desktop **client**. The Mini-Me agent stack (coordinator +
Asta-backed subagents + skills) stays in Python/TypeScript and runs as a **local
sidecar** the client spawns and supervises — which also means the app inherits
the local `asta` CLI's auto-refreshing auth, so the web app's token-expiry pain
goes away.

> **Status: migrated from GPUI to Tauri.** The Rust side keeps everything that was
> never about rendering — the backend supervisor, the sidecar/LangGraph HTTP+SSE
> client, settings and OS-keychain secrets, the preflight checks, the self-updater —
> exposed to the frontend as Tauri commands and events instead of driven by a GPUI
> view. The UI itself (chat, sidebar, settings, the approval gate, the command
> palette, the research/outputs panel, a theme gallery) is a React app in
> [`crates/app/frontend/`](crates/app/frontend/), styled from the same ten palettes
> `theme.rs` always shipped.
>
> Read [`docs/desktop-app-plan.md`](docs/desktop-app-plan.md) for the history behind
> the app's design decisions — the `§N` markers in code comments point there.

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
crates/app            the desktop binary: Tauri shell + backend supervisor + commands
crates/app/frontend   the React/TypeScript UI, built by Vite
docs/                 the design record, the handover, and the open-work checklist
```

## Windows (the primary platform)

~98% of our users are on Windows. The app runs **natively** on Windows (Tauri wraps
WebView2, which ships with Windows 10/11), while the Python backend runs **inside
WSL2** — the agent stack shells out
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

## Build

On Linux, Tauri needs the system webview dev headers once:

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev
```

The frontend has to be built before Cargo, since `tauri-build` embeds
`crates/app/frontend/dist` into the binary at compile time:

```bash
npm --prefix crates/app/frontend ci
npm --prefix crates/app/frontend run build
cargo build -p mini-me-desktop-app
```

For day-to-day UI work, run the Tauri dev server instead — it gives the frontend real
hot reload and rebuilds the Rust side on change:

```bash
cd crates/app && npm install && npm run dev
```

`npm run dev` must be launched from a graphical session — it can't open a window from
a headless TTY.

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

GPUI was the original choice, for a native, GPU-rendered, keyboard-first workbench.
It was replaced with Tauri because the team could not maintain a GPUI UI without
heavy AI assistance for every change, it has no hot reload, and time spent on the
UI framework was time not spent on the backend, which is meant to be this project's
strongest part. React + TypeScript is a stack every contributor can read and extend
directly, and the Rust side keeps everything that was never about rendering —
process supervision, the sidecar client, settings, the keychain, the updater.

Org policy: human-gated (nothing auto-runs). AI-assisted (Claude Code) per CIP
Acceptable Use policy.
