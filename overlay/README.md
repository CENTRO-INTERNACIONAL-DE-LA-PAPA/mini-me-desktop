# `overlay/` — desktop-only Python, injected into the Mini-Me backend

This directory is **not** part of the desktop binary. It is a small Python package
that the app puts on the backend's `PYTHONPATH`, so the Mini-Me sidecar runs the
agent's code **on this machine** instead of in a remote LangSmith sandbox.

Nothing in the Mini-Me checkout is modified. That is the point — see
[`../docs/desktop-app-plan.md`](../docs/desktop-app-plan.md) §10, §11 and §18 for why,
and for the trade-offs.

## How it gets in

```
PYTHONPATH=overlay/                       →  Python auto-imports overlay/sitecustomize.py
MINIME_EXECUTION_BACKEND=local            →  the overlay arms itself (otherwise inert)
```

`sitecustomize.py` registers an import hook. When the backend later imports
`backend.sandbox`, the hook rebinds `LazyLangsmithSandbox` to
`minime_local.workspace.LocalWorkspaceBackend`. Both of upstream's construction sites
(`backend/agent.py` and `backend/routes/common.py`) do
`from backend.sandbox import LazyLangsmithSandbox` at *their* import time, so one
rebinding covers both.

The app sets both variables itself (`crates/app/src/backend.rs`, `Execution::Local`).
To drive it by hand:

```bash
cd /path/to/Mini-Me
PYTHONPATH=/path/to/mini-me-desktop/overlay \
MINIME_EXECUTION_BACKEND=local \
.venv/bin/langgraph dev --no-reload --no-browser
```

## What the replacement is

`LocalWorkspaceBackend` subclasses deepagents' `LocalShellBackend`, which already
implements the whole backend surface against the host. Every `a*` method in
deepagents' `BackendProtocol` is a concrete default that offloads its sync twin with
`asyncio.to_thread`, so the async API Mini-Me's tools await comes for free.

What this package adds is the handful of methods Mini-Me defined on top of the
protocol and deepagents has no equivalent for: `aget_work_dir`,
`aexecute_untruncated`, the lifecycle quartet (`aresolve` / `try_resolve` / `aresume` /
`adelete`), a `sandbox_status` emission so the UI does not wait forever, and an
`aexecute` override — the protocol's default silently drops the per-call `timeout`.

## Environment

| variable | effect |
|---|---|
| `MINIME_EXECUTION_BACKEND=local` | arms the overlay; anything else leaves the sandbox in place |
| `MINIME_LOCAL_WORKSPACE=<dir>` | where per-thread workspaces live (default `~/.mini-me/workspaces`) |

## Notes for whoever reads this next

- **Pyright will flag the imports** (`deepagents`, `backend.sandbox`) — they only
  resolve inside the Mini-Me venv, which this repo does not contain. Expected.
- **It fails loudly, on purpose.** If a future Mini-Me commit renames
  `LazyLangsmithSandbox`, `install()` raises and names this directory. The failure we
  refuse to allow is silently keeping the remote sandbox — and a LangSmith bill — while
  the app claims to be local.
- **Only the first `sitecustomize` on `sys.path` is imported.** Nothing in the Mini-Me
  venv ships one today (checked 2026-07-31).
- **This code is the upstream patch, minus the plumbing.** If Mini-Me ever grows a
  real `MINIME_EXECUTION_BACKEND` seam, `workspace.py` moves across almost verbatim and
  the import hook disappears.
