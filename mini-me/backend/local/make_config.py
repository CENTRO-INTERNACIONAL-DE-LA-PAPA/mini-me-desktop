"""Write a LangGraph config that serves the coordinator graph *and* a background one.

Run just before `langgraph dev` starts, by the desktop app's launch command.

**Why this exists.** `deepagents.AsyncSubAgentMiddleware` requires each async subagent to
be a graph on the Agent Protocol server, and `langgraph.json` declares exactly one. `langgraph
dev` accepts `--config`, and the desktop app builds the launch command — so the extra graph is
declared from a generated config rather than by hand-editing `langgraph.json`.

**Why it extends rather than reconstructs.** The base config carries `dependencies`, `env` and
`http` — the last of which mounts the custom routes the project spine and the background-job
polling depend on. Rebuilding the file by hand would drop whichever of those changes next, and
the failure would look unrelated to this.

**Why it runs every launch.** It is derived from `langgraph.json`. Generated once at
provisioning, it would keep serving yesterday's dependencies after a backend update.

    Usage:  python make_config.py <checkout-dir>
"""

from __future__ import annotations

import json
import os
import sys

#: Must match `BACKGROUND_GRAPH_ID` in async_agents.py and `BACKGROUND_GRAPH_ID` in
#: crates/app/src/backend.rs. A mismatch fails when the coordinator first delegates —
#: mid-task, in front of the user — rather than at startup.
BACKGROUND_GRAPH_ID = "background"

OUTPUT_NAME = ".mini-me-desktop.langgraph.json"


def sqlite_available() -> bool:
    """Whether the backend can load the SQLite checkpointer.

    **Inlined rather than imported from `backend.local.checkpointer`, which is the sibling that
    owns this question.** The launch command runs this file *as a script*
    (`.venv/bin/python <checkout>/backend/local/make_config.py <checkout>`), and Python then puts
    the script's own directory on `sys.path` — `backend/local/`, not the checkout root above it.
    So `from backend.local import ...` raises `ModuleNotFoundError` unless the checkout root is
    also on `sys.path`, and a generator that exits non-zero stops the backend from starting at
    all (the launch command chains this with `&&`).

    Three lines of duplication against a launch that cannot start. The sibling keeps its own copy
    for the server's benefit; this one exists because a script is not a package.
    """
    try:
        import langgraph.checkpoint.sqlite.aio  # noqa: F401
    except Exception:  # noqa: BLE001 — any import failure means "not available"
        return False
    return True


def build(checkout: str, local_dir: str) -> str:
    """Write the extended config beside the base one and return its path."""
    source = os.path.join(checkout, "langgraph.json")
    with open(source, encoding="utf-8") as handle:
        config = json.load(handle)

    graphs = config.get("graphs")
    if not isinstance(graphs, dict):
        raise SystemExit(f"{source}: no 'graphs' object to extend")

    if BACKGROUND_GRAPH_ID in graphs:
        # A graph of this name already exists. Better to say so than to overwrite it.
        raise SystemExit(
            f"{source} already declares a '{BACKGROUND_GRAPH_ID}' graph; "
            "backend/local/make_config.py needs revisiting"
        )

    graphs[BACKGROUND_GRAPH_ID] = (
        os.path.join(local_dir, "async_agents.py") + ":background_graph"
    )

    # Conversations in SQLite rather than one pickle of everything: constant boot instead of a
    # boot that grows with history, and per-row writes instead of a format where one unreadable
    # byte takes every conversation with it.
    #
    # **Only when the package is importable.** Naming a checkpointer the backend cannot load
    # would turn a missing optional dependency into a server that does not start; leaving the
    # key out gives exactly today's behaviour. The Setup pane checks for it and offers to
    # install it, so this is a choice a researcher can see and make, not a silent downgrade.
    if sqlite_available():
        config["checkpointer"] = {
            "path": os.path.join(local_dir, "checkpointer.py") + ":checkpointer"
        }
    elif "checkpointer" in config:
        # A checkpointer is already declared and this one cannot be honoured: leave it alone.
        pass

    destination = os.path.join(checkout, OUTPUT_NAME)
    with open(destination, "w", encoding="utf-8") as handle:
        json.dump(config, handle, indent=2)
        handle.write("\n")
    return destination


def main(argv: list[str]) -> int:
    checkout = argv[1] if len(argv) > 1 else "."
    # `backend/local`, wherever this file lives — no second path to keep in sync.
    local_dir = os.path.dirname(os.path.abspath(__file__))
    written = build(checkout, local_dir)
    storage = (
        "sqlite" if sqlite_available() else "the built-in pickle (see Setup)"
    )
    print(f"backend.local: wrote {written}; conversations in {storage}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
