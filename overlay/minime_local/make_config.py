"""Write a LangGraph config that serves upstream's graph *and* a background one.

Run just before `langgraph dev` starts, by the desktop app's launch command.

**Why this exists.** `deepagents.AsyncSubAgentMiddleware` requires each async subagent to
be a graph on the Agent Protocol server, and Mini-Me declares exactly one. That looked
like a structural change to a checkout we do not modify — until two facts lined up:
`langgraph dev` accepts `--config`, and the desktop app builds the launch command. So the
extra graph is declared from the *client* side, and the checkout is untouched (docs §30).

**Why it extends rather than reconstructs.** Upstream's config carries `dependencies`,
`env` and `http` — the last of which mounts the custom routes the project spine and the
background-job polling depend on. Rebuilding the file by hand would drop whichever of
those upstream adds next, and the failure would look unrelated to this.

**Why it runs every launch.** It is derived from `langgraph.json`. Generated once at
provisioning, it would keep serving yesterday's dependencies after a backend update.

    Usage:  python make_config.py <checkout-dir>
"""

from __future__ import annotations

import json
import os
import sys

from minime_local import checkpointer as sqlite_checkpointer

#: Must match `BACKGROUND_GRAPH_ID` in async_agents.py and `BACKGROUND_GRAPH_ID` in
#: crates/app/src/backend.rs. A mismatch fails when the coordinator first delegates —
#: mid-task, in front of the user — rather than at startup.
BACKGROUND_GRAPH_ID = "background"

OUTPUT_NAME = ".mini-me-desktop.langgraph.json"


def build(checkout: str, overlay: str) -> str:
    """Write the extended config beside upstream's and return its path."""
    source = os.path.join(checkout, "langgraph.json")
    with open(source, encoding="utf-8") as handle:
        config = json.load(handle)

    graphs = config.get("graphs")
    if not isinstance(graphs, dict):
        raise SystemExit(f"{source}: no 'graphs' object to extend")

    if BACKGROUND_GRAPH_ID in graphs:
        # Upstream has grown a graph of this name. Better to say so than to overwrite it.
        raise SystemExit(
            f"{source} already declares a '{BACKGROUND_GRAPH_ID}' graph; "
            "overlay/minime_local needs revisiting"
        )

    graphs[BACKGROUND_GRAPH_ID] = (
        os.path.join(overlay, "minime_local", "async_agents.py") + ":background_graph"
    )

    # Conversations in SQLite rather than one pickle of everything: constant boot instead of a
    # boot that grows with history, and per-row writes instead of a format where one unreadable
    # byte takes every conversation with it (docs §93, §95).
    #
    # **Only when the package is importable.** Naming a checkpointer the backend cannot load
    # would turn a missing optional dependency into a server that does not start; leaving the
    # key out gives exactly today's behaviour. The Setup pane checks for it and offers to
    # install it, so this is a choice a researcher can see and make, not a silent downgrade.
    if sqlite_checkpointer.available():
        config["checkpointer"] = {
            "path": os.path.join(overlay, "minime_local", "checkpointer.py")
            + ":checkpointer"
        }
    elif "checkpointer" in config:
        # Upstream declared one and we cannot honour ours: leave theirs alone.
        pass

    destination = os.path.join(checkout, OUTPUT_NAME)
    with open(destination, "w", encoding="utf-8") as handle:
        json.dump(config, handle, indent=2)
        handle.write("\n")
    return destination


def main(argv: list[str]) -> int:
    checkout = argv[1] if len(argv) > 1 else "."
    # The overlay is wherever this file lives — no second path to keep in sync, and it
    # stays correct when provisioning copies the overlay into the distro.
    overlay = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    written = build(checkout, overlay)
    storage = (
        "sqlite" if sqlite_checkpointer.available() else "the built-in pickle (see Setup)"
    )
    print(f"minime_local: wrote {written}; conversations in {storage}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
