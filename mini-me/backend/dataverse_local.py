"""Launch configuration for the Dataverse stdio server bundled with Mini-Me."""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path
from typing import Any


def server_config() -> dict[str, Any]:
    """Resolve and validate paths off the event loop; never import the server."""
    script = Path(
        os.environ.get(
            "MINIME_DATAVERSE_SERVER",
            str(Path(__file__).parent / "dataverse_mcp" / "server.py"),
        )
    ).expanduser().resolve()
    default_python = sys.executable
    # Keep the virtualenv symlink: resolving it would run the base interpreter instead.
    python = (
        Path(os.environ.get("MINIME_DATAVERSE_PYTHON", str(default_python)))
        .expanduser()
        .absolute()
    )
    if not script.is_file():
        raise FileNotFoundError(
            f"Dataverse server not found: {script}. Set MINIME_DATAVERSE_SERVER."
        )
    if not python.is_file() or not os.access(python, os.X_OK):
        raise FileNotFoundError(
            f"Dataverse Python not executable: {python}. Install backend dependencies "
            "with uv sync or set MINIME_DATAVERSE_PYTHON."
        )
    # Forward server settings and system proxies, not model-provider credentials.
    # FastMCP adds the standard subprocess environment. Defaults require no external repo.
    env = {
        key: os.environ[key]
        for key in (
            "DATAVERSE_API_KEY",
            "DATAVERSE_BASE_URL",
            "ENV_FILE",
            "MCP_JSON_DIR",
            "MCP_DOWNLOAD_DIR",
            "DATAVERSE_VERIFY_TLS",
            "SSL_CERT_FILE",
            "SSL_CERT_DIR",
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "NO_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
            "no_proxy",
        )
        if key in os.environ
    }
    return {
        "command": str(python),
        "args": [str(script)],
        "cwd": str(script.parent),
        "env": env,
    }


def setup_status() -> dict[str, Any]:
    """Offline path check for Desktop Setup; discovery verifies runtime health."""
    try:
        config = server_config()
    except (OSError, ValueError) as error:
        return {"ready": False, "detail": str(error)}
    return {
        "ready": True,
        "detail": (
            f"local stdio server configured: {config['args'][0]} "
            "(runtime health checked at startup)"
        ),
    }


if __name__ == "__main__":
    print(json.dumps(setup_status()))
