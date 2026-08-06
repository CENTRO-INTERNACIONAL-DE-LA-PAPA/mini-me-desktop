"""Record which subagents the coordinator was actually built with.

**Why the desktop app needs this.** `/subagent` commands (docs §55) let a researcher name the
specialist they want instead of hoping the coordinator delegates to it. Naming one requires a
list of what can be named, and §55 was explicit that the list must come from the backend rather
than be hardcoded in the client — a copy would drift the first time upstream renames a subagent,
and the failure would be a command that silently does nothing.

**Why it is captured here rather than served over HTTP.** The obvious move is a `GET /subagents`
route. It does not work: `langgraph.json` mounts `http.app` from a *file path*
(`./backend/routes/__init__.py:app`), and file-path loading bypasses `sys.meta_path` entirely —
the same trap documented in `minime_local/__init__.py`, which is why the approval patch had to
move to the `deepagents` package. A route added by an import hook would never be mounted.

**Why capturing the call is better than reading the file.** `backend/subagents.py` has a module
level list, and parsing it would be one more thing to keep in sync. But the coordinator is
assembled *per request* (`_build_runtime_subagents`), and what it ends up with is what can
actually be delegated to. Reading the kwarg the factory was called with reports the truth,
including anything upstream adds or assembles conditionally.

The file lands beside the researcher's own work, in the workspace root the desktop app already
shares with this process (`MINIME_LOCAL_WORKSPACE`) — the same directory figures appear in
(docs §42), so no new path has to be agreed between the two sides.
"""

from __future__ import annotations

import json
import logging
import os
from typing import Any

logger = logging.getLogger(__name__)

#: Written into the workspace root. Read by the desktop app; never read back here.
FILENAME = "subagents.json"

#: Version the file, so a client from a different release can tell rather than guess. A
#: `/subagent` picker that silently mis-reads its registry would offer commands that do nothing.
FORMAT = 1


def describe(subagents: Any) -> list[dict[str, str]]:
    """Reduce whatever the factory was handed to `{name, description}` pairs.

    Deliberately tolerant. This runs inside the call that builds the coordinator, on a value
    upstream owns and may change the shape of — a `TypeError` here would take down every turn to
    populate a picker. Anything unrecognised is skipped, and a subagent with no name is not
    nameable anyway.
    """
    described: list[dict[str, str]] = []
    if not isinstance(subagents, (list, tuple)):
        return described
    for entry in subagents:
        # A dict today. Guarded because an object with attributes is the obvious next shape.
        if isinstance(entry, dict):
            name = entry.get("name")
            description = entry.get("description")
        else:
            name = getattr(entry, "name", None)
            description = getattr(entry, "description", None)
        if not isinstance(name, str) or not name.strip():
            continue
        described.append(
            {
                "name": name.strip(),
                "description": (description or "").strip()
                if isinstance(description, str)
                else "",
            }
        )
    return described


def record(subagents: Any) -> None:
    """Write the registry, or say why not and carry on.

    Never raises. This is called on the path that answers a researcher's question, and a picker
    that cannot be populated is worth strictly less than the turn it would have broken.
    """
    try:
        described = describe(subagents)
        if not described:
            logger.warning("minime_local: no nameable subagents to record")
            return
        root = os.getenv("MINIME_LOCAL_WORKSPACE", "").strip()
        if not root:
            # Only set when the desktop app launched this server. A plain `langgraph dev` in
            # the checkout should write nothing at all.
            return
        os.makedirs(root, exist_ok=True)
        path = os.path.join(root, FILENAME)
        payload = {"format": FORMAT, "subagents": described}
        # Written whole and replaced, so a client never reads a half-written list.
        temporary = f"{path}.tmp"
        with open(temporary, "w", encoding="utf-8") as handle:
            json.dump(payload, handle, indent=2)
        os.replace(temporary, path)
        logger.warning(
            "minime_local: recorded %d nameable subagents in %s", len(described), path
        )
    except Exception as error:  # noqa: BLE001 — see the docstring
        logger.warning("minime_local: could not record the subagent registry: %s", error)
