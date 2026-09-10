"""Signing in to an MCP deployment that sits behind Horizon Authentication.

CIP's Dataverse deployment has Horizon Authentication switched on, so the endpoint answers
every unauthenticated call with::

    HTTP/1.1 401 Unauthorized
    WWW-Authenticate: Bearer realm="FastMCP",
      resource_metadata="https://dataverse-cip.fastmcp.app/.well-known/oauth-protected-resource"

AGROVOC and Crop Ontology are public and need none of this. Dataverse cannot simply be made
public alongside them: it exposes six writing tools — ``publish_dataset``,
``replace_file_in_dataset``, ``set_embargo_on_dataset_files``, ``unset_embargo_on_dataset_files``,
``update_dataset_metadata`` and ``update_file_categories`` — and MCP calls do not pass through the
``execute`` approval gate, so an open deployment would let anyone holding the URL publish and
modify CIP datasets.

# Why a browser sign-in rather than an API key

Measured from the deployment's own metadata rather than assumed::

    grant_types_supported: ["authorization_code", "refresh_token"]

There is no ``client_credentials`` grant, so **no machine-to-machine token exists**. An
``ASTA_API_KEY``-style entry in ``MCP_SERVER_CONFIGS["dataverse"]["headers_env"]`` is not merely
inconvenient here, it is impossible: nothing can issue the key it would carry. The only way in is
an interactive authorization-code login, once, whose refresh token then keeps working.

``fastmcp.client.auth.OAuth`` already implements that flow — dynamic client registration, PKCE,
the callback listener, and refresh — so this module supplies only the two things it leaves open:
somewhere to keep the tokens, and a rule about who may open a browser.

# The two providers, and why they are not the same object

`for_runtime()` never opens a browser. A turn that discovers it is signed out must fail in
milliseconds and be reported as an unavailable service (which since the MCP resilience work is
survivable), not block a researcher's question behind a login prompt they cannot see. Only
`for_login()` — reached from Setup, by someone who just pressed a button — is allowed to start an
interactive flow.
"""

from __future__ import annotations

import json
import os
import time
from pathlib import Path
from typing import Any

# The deployment this module exists for. Kept here rather than imported from `mcp_tools` so the
# login entry point can run without importing the agent graph.
DATAVERSE_MCP_URL = "https://dataverse-cip.fastmcp.app/mcp"

# Fixed, so the redirect URI registered at login is the one the callback listens on. A random
# port would work for a single sitting and then be wrong for the cached client registration.
CALLBACK_PORT = 41999


def state_dir() -> Path:
    """Where the tokens live: beside the conversation database, not in the checkout.

    ``MINIME_STATE_DIR`` lets the desktop app say; otherwise this follows the same
    ``.langgraph_api`` directory the checkpointer writes to, so a researcher who deletes their
    backend state deletes their sign-in with it rather than leaving a token behind.
    """
    override = os.getenv("MINIME_STATE_DIR")
    base = Path(override) if override else Path.cwd() / ".langgraph_api"
    return base / "auth"


class FileTokenStore:
    """The smallest thing that satisfies what ``OAuth`` asks of a token store.

    ``key-value``'s own ``DiskStore`` would do, but it needs ``diskcache``, and adding a
    dependency moves ``uv.lock`` — which costs every researcher a full re-sync on their next
    launch. That is a poor trade for `get`/`put`/`delete` over a JSON file, which is all
    ``PydanticAdapter`` ever calls.
    """

    def __init__(self, directory: Path) -> None:
        self._path = directory / "dataverse-tokens.json"

    def _read(self) -> dict[str, Any]:
        try:
            return json.loads(self._path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            # A truncated or absent file is "signed out", never an error: the caller's next step
            # is to offer a sign-in either way.
            return {}

    def _write(self, data: dict[str, Any]) -> None:
        self._path.parent.mkdir(parents=True, exist_ok=True)
        # Written beside and moved into place, so an interrupted write cannot leave a half token
        # that reads as a valid one.
        scratch = self._path.with_suffix(".json.new")
        scratch.write_text(json.dumps(data, indent=2), encoding="utf-8")
        try:
            os.chmod(scratch, 0o600)
        except OSError:
            pass
        scratch.replace(self._path)

    @staticmethod
    def _slot(key: str, collection: str | None) -> str:
        return f"{collection or 'default'}::{key}"

    async def get(
        self, key: str, *, collection: str | None = None
    ) -> dict[str, Any] | None:
        entry = self._read().get(self._slot(key, collection))
        if not isinstance(entry, dict):
            return None
        expires = entry.get("expires_at")
        if isinstance(expires, (int, float)) and expires <= time.time():
            return None
        value = entry.get("value")
        return value if isinstance(value, dict) else None

    async def put(
        self,
        key: str,
        value: Any,
        *,
        collection: str | None = None,
        ttl: Any | None = None,
    ) -> None:
        data = self._read()
        entry: dict[str, Any] = {"value": dict(value)}
        if ttl is not None:
            entry["expires_at"] = time.time() + float(ttl)
        data[self._slot(key, collection)] = entry
        self._write(data)

    async def delete(self, key: str, *, collection: str | None = None) -> bool:
        data = self._read()
        if data.pop(self._slot(key, collection), None) is None:
            return False
        self._write(data)
        return True


def _open_in_browser(url: str) -> None:
    """Best effort, in the order most likely to work where this actually runs.

    The backend runs inside WSL and the researcher's browser is on Windows, so the useful
    openers are the interop ones. Every failure is swallowed: the URL has already been printed,
    and a login that cannot open a window is inconvenient, not broken.
    """
    import subprocess

    attempts = [
        # wslu, when the distro has it.
        ["wslview", url],
        # Windows interop. `-NoProfile` so a slow or broken user profile cannot hang the login,
        # and the URL is a single argument so `&` between query parameters is never a separator.
        ["powershell.exe", "-NoProfile", "-Command", "Start-Process", "-FilePath", url],
        ["cmd.exe", "/c", "start", "", url],
    ]
    for argv in attempts:
        try:
            subprocess.Popen(
                argv,
                # From a Linux path, Windows binaries warn about the working directory and fall
                # back to a Windows one. Starting there keeps the noise out of the login output.
                cwd="/mnt/c" if argv[0].endswith(".exe") and os.path.isdir("/mnt/c") else None,
                stdout=subprocess.DEVNULL,
                stderr=subprocess.DEVNULL,
            )
            return
        except OSError:
            continue

    # Not WSL at all — a developer running the backend on Linux directly.
    try:
        import webbrowser

        webbrowser.open(url)
    except Exception:  # noqa: BLE001
        pass


class NotSignedIn(RuntimeError):
    """Raised instead of opening a browser during a turn."""


def _oauth(interactive: bool):
    """Build the provider. Imported lazily so `--status` runs without the MCP stack."""
    from fastmcp.client.auth import OAuth

    class _Provider(OAuth):  # type: ignore[misc]
        async def redirect_handler(self, authorization_url: str) -> None:
            if not interactive:
                # The whole point of the split: a turn discovers it is signed out here, and this
                # exception becomes "CIP Dataverse unavailable" rather than a hung question.
                raise NotSignedIn(
                    "not signed in to CIP Dataverse — open Settings › Setup and press Sign in"
                )
            # **Printed first, opened second.** `webbrowser.open` finds nothing inside WSL and
            # says nothing about it, so a login would appear to hang on a page that never
            # appeared. The URL goes to stdout before any attempt, so the researcher can always
            # paste it themselves — that line is the contract, and opening a browser is a
            # convenience layered on top of it.
            print(f"MINIME_AUTHORIZE_URL {authorization_url}", flush=True)
            _open_in_browser(authorization_url)

    return _Provider(
        mcp_url=DATAVERSE_MCP_URL,
        scopes=["openid", "profile", "email", "offline_access"],
        client_name="Mini-Me Desktop",
        token_storage=FileTokenStore(state_dir()),
        callback_port=CALLBACK_PORT,
        # The listener runs in WSL and the browser is on Windows. WSL2 forwards Windows loopback
        # to a listener bound on all interfaces; a 127.0.0.1-only bind is "not always visible
        # from Windows", which is why the backend itself binds 0.0.0.0 (docs §the launch).
        callback_host="0.0.0.0",
    )


def for_runtime():
    """The provider a graph build uses. Never interactive."""
    return _oauth(interactive=False)


def for_login():
    """The provider the Sign in button uses."""
    return _oauth(interactive=True)


def signed_in() -> bool:
    """Whether a token is on disk. Says nothing about whether it still works."""
    store = FileTokenStore(state_dir())
    return bool(store._read())


async def _login() -> int:
    """Perform the interactive flow, then prove it worked.

    Driven through a real call rather than by poking the provider: the OAuth machinery is an
    httpx auth flow, so it runs when a request needs it and not before. `ping` is the cheapest
    request that exercises the whole path — registration, browser, callback, token exchange —
    and a login that "succeeded" without one would be a token nobody has spent.
    """
    from fastmcp import Client
    from fastmcp.client.transports import StreamableHttpTransport

    transport = StreamableHttpTransport(DATAVERSE_MCP_URL, auth=for_login())
    async with Client(transport) as client:
        await client.ping()
    print("MINIME_SIGNED_IN")
    return 0


async def _status() -> int:
    """Say whether the stored sign-in still opens the door.

    Three answers, not two: a token that is present but rejected is a different problem from no
    token at all, and telling a researcher to sign in when they already have is how a support
    conversation goes in a circle.
    """
    if not signed_in():
        print("MINIME_SIGNED_OUT")
        return 1
    from fastmcp import Client
    from fastmcp.client.transports import StreamableHttpTransport

    try:
        transport = StreamableHttpTransport(DATAVERSE_MCP_URL, auth=for_runtime())
        async with Client(transport) as client:
            await client.ping()
    except Exception as error:  # noqa: BLE001 — every failure means the same thing to the caller
        print(f"MINIME_SIGN_IN_EXPIRED {type(error).__name__}")
        return 1
    print("MINIME_SIGNED_IN")
    return 0


def _logout() -> int:
    path = FileTokenStore(state_dir())._path
    try:
        path.unlink()
    except FileNotFoundError:
        pass
    print("MINIME_SIGNED_OUT")
    return 0


def main(argv: list[str] | None = None) -> int:
    import asyncio
    import sys

    args = list(sys.argv[1:] if argv is None else argv)
    command = args[0] if args else "status"
    if command == "login":
        return asyncio.run(_login())
    if command == "status":
        return asyncio.run(_status())
    if command == "logout":
        return _logout()
    print(f"unknown command: {command}", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
