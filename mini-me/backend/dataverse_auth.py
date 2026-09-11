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

import asyncio
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


_STATE_DIR: Path | None = None


def state_dir() -> Path:
    """Where the tokens live: beside the conversation database, not in the checkout.

    ``MINIME_STATE_DIR`` lets the desktop app say; otherwise this follows the same
    ``.langgraph_api`` directory the checkpointer writes to, so a researcher who deletes their
    backend state deletes their sign-in with it rather than leaving a token behind.

    **Blocking, and memoised because of it.** ``Path.cwd()`` is ``os.getcwd()``, which langgraph
    runs the server under `blockbuster` to forbid on the event loop — and forbidding it means
    raising. Every call on the async path must therefore reach this through
    [`auth_for_runtime`], which moves it to a thread. Memoising is not an optimisation: it means
    the *first* resolution is the only one that can be caught in the wrong place.
    """
    global _STATE_DIR
    if _STATE_DIR is None:
        override = os.getenv("MINIME_STATE_DIR")
        base = Path(override) if override else Path.cwd() / ".langgraph_api"
        _STATE_DIR = base / "auth"
    return _STATE_DIR


class FileTokenStore:
    """The smallest thing that satisfies what ``OAuth`` asks of a token store.

    ``key-value``'s own ``DiskStore`` would do, but it needs ``diskcache``, and adding a
    dependency moves ``uv.lock`` — which costs every researcher a full re-sync on their next
    launch. That is a poor trade for `get`/`put`/`delete` over a JSON file, which is all
    ``PydanticAdapter`` ever calls.
    """

    def __init__(self, directory: Path | None = None) -> None:
        # Resolved on use, not here: `state_dir()` blocks, and a store constructed on the event
        # loop would raise before it had read anything.
        self._directory = directory

    @property
    def _path(self) -> Path:
        return (self._directory or state_dir()) / "dataverse-tokens.json"

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
        entry = (await asyncio.to_thread(self._read)).get(self._slot(key, collection))
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
        data = await asyncio.to_thread(self._read)
        entry: dict[str, Any] = {"value": dict(value)}
        if ttl is not None:
            entry["expires_at"] = time.time() + float(ttl)
        data[self._slot(key, collection)] = entry
        await asyncio.to_thread(self._write, data)

    async def delete(self, key: str, *, collection: str | None = None) -> bool:
        data = await asyncio.to_thread(self._read)
        if data.pop(self._slot(key, collection), None) is None:
            return False
        await asyncio.to_thread(self._write, data)
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
        def _bind(self, mcp_url: str) -> None:
            """Advertise a loopback address, then listen on every interface.

            ``OAuth`` uses one ``callback_host`` for two different jobs: the address written into
            ``redirect_uri`` — which a **browser** has to navigate to — and the address the
            callback server binds. Those are not the same requirement here, and setting the field
            to ``0.0.0.0`` to satisfy the second broke the first:

                http://0.0.0.0:41999/callback  ->  ERR_ADDRESS_INVALID

            ``0.0.0.0`` means "every interface" to a listener and nothing at all to Chrome. So
            bind is chosen after the URI is built: the browser is sent to ``127.0.0.1`` on
            Windows, and WSL2 forwards that to a listener bound on every interface inside the
            distro — which is the same reason the backend itself binds ``0.0.0.0`` rather than
            loopback (a loopback-only bind "is not always visible from Windows").
            """
            super()._bind(mcp_url)
            self._callback_host = "0.0.0.0"

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
        # What the **browser** is told to come back to. `_Provider._bind` widens the listener
        # afterwards; the two are separate requirements and one field.
        callback_host="127.0.0.1",
    )


def for_runtime():
    """The provider a graph build uses. Never interactive."""
    return _oauth(interactive=False)


def for_login():
    """The provider the Sign in button uses."""
    return _oauth(interactive=True)


def signed_in() -> bool:
    """Whether a token is on disk. Says nothing about whether it still works.

    **Blocking.** Never call this from the event loop — use [`auth_for_runtime`].
    """
    return bool(FileTokenStore()._read())


def sign_in_fingerprint() -> str:
    """Cheap evidence of *which* sign-in is stored, for invalidating a cached failure.

    Blocking, like everything that touches the store — reached from the async path through
    [`sign_in_changed`].
    """
    try:
        stat = FileTokenStore()._path.stat()
    except OSError:
        return "none"
    return f"{stat.st_mtime_ns}:{stat.st_size}"


async def sign_in_changed(previous: str | None) -> tuple[bool, str]:
    """Whether the stored sign-in differs from when a caller last looked.

    # Why a cached failure has to expire on this and not on time

    Discovery caches an empty tool list when a deployment cannot be reached, so a read-only
    request cannot hammer it. That cache is process-wide and has no notion of a researcher
    signing in halfway through its life — and the sign-in happens in a *different process*, so
    nothing can reach in and clear it. The result was the state Codex measured: a correct
    sign-in, a healthy deployment, and an app that kept reporting the service as unavailable
    until the whole backend was restarted.

    A stat is enough to tell the two apart, and it costs nothing on the path where the answer is
    "no change" — which is every path except the one immediately after a sign-in.
    """
    current = await asyncio.to_thread(sign_in_fingerprint)
    return current != previous, current


async def auth_for_runtime():
    """The provider a graph build should use, or `None` when signed out.

    # Why this exists rather than callers deciding for themselves

    The first version had `mcp_tools` call `signed_in()` directly while constructing the MCP
    client. That construction happens on the event loop during a graph build, and langgraph runs
    the server under `blockbuster`, which does not merely warn about blocking I/O — it raises.
    So every launch produced:

        MCP unavailable; continuing without CIP Dataverse: Blocking call to os.getcwd

    from `signed_in()` → `state_dir()` → `Path.cwd()`, **before authentication was ever
    attempted**. Discovery caught it, marked the service unavailable and cached the empty
    result, so a correctly signed-in researcher saw exactly the same modal as a signed-out one,
    and `agent.py` then dropped the `dataverse_explorer` specialist. The sign-in worked
    throughout; nothing could use it.

    Fixing `Path.cwd()` alone would only have moved the error — `_read` opens and reads a file,
    which is blocked as well. So the whole decision moves to a thread, once, here, and the async
    callers have nothing left to get wrong.
    """
    if not await asyncio.to_thread(signed_in):
        return None
    return await asyncio.to_thread(for_runtime)


def _drop_stale_registration(provider) -> None:
    """Forget a client registration that names a redirect URI we no longer use.

    A registration is cached so repeated sign-ins do not create a new OAuth client every time.
    That cache outlives a change to the callback address, and a stale one is not merely useless —
    it is invisible: the sign-in reuses it, the browser is sent to the old address, and the only
    symptom is the callback never arriving. The first build of this shipped
    ``http://0.0.0.0:41999/callback``, so the first machines to try it have exactly that cached.

    Matching on the URI rather than clearing unconditionally, because re-registering on every
    sign-in would leave a trail of client records on the researcher's Horizon account.
    """
    try:
        wanted = {str(uri) for uri in provider.context.client_metadata.redirect_uris}
    except AttributeError:
        return

    store = FileTokenStore(state_dir())
    data = store._read()
    kept = {
        slot: entry
        for slot, entry in data.items()
        if not _names_another_callback(entry, wanted)
    }
    if len(kept) != len(data):
        store._write(kept)


def _names_another_callback(entry: Any, wanted: set[str]) -> bool:
    value = entry.get("value") if isinstance(entry, dict) else None
    if not isinstance(value, dict):
        return False
    uris = value.get("redirect_uris")
    if not isinstance(uris, list) or not uris:
        # Not a client registration — a token, which no callback address invalidates.
        return False
    return not any(str(uri) in wanted for uri in uris)


async def _login() -> int:
    """Perform the interactive flow, then prove it worked.

    Driven through a real call rather than by poking the provider: the OAuth machinery is an
    httpx auth flow, so it runs when a request needs it and not before, and a login that
    "succeeded" without one would be a token nobody has spent.

    **`list_tools`, not `ping`.** `ping` was the cheaper request and this deployment answers it
    with *"Method not found"* — MCP servers are not obliged to implement it. That turned a
    successful sign-in into a reported failure while the tokens sat correctly on disk. Listing
    tools is the thing the backend will actually do with this credential, so it verifies what
    matters rather than what was convenient.
    """
    from fastmcp import Client
    from fastmcp.client.transports import StreamableHttpTransport

    provider = for_login()
    _drop_stale_registration(provider)
    transport = StreamableHttpTransport(DATAVERSE_MCP_URL, auth=provider)
    async with Client(transport) as client:
        await client.list_tools()
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
        transport = StreamableHttpTransport(DATAVERSE_MCP_URL, auth=await auth_for_runtime())
        async with Client(transport) as client:
            await client.list_tools()
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
