"""Signing in to the Horizon-protected Dataverse deployment, and what happens before you have.

The deployment answers unauthenticated calls with `401` and a `WWW-Authenticate: Bearer` header
naming an OAuth resource. Its metadata offers only `authorization_code` and `refresh_token` — no
`client_credentials` — so there is no key to put in `headers_env` and the sign-in has to be a
browser flow. `backend/dataverse_auth.py` carries the reasoning.

What is pinned here is the part that is easy to get wrong and invisible when wrong: a researcher
who has *not* signed in must not pay for it on every launch.
"""

from __future__ import annotations

import json

import pytest

from backend import dataverse_auth


@pytest.fixture()
def state(tmp_path, monkeypatch):
    monkeypatch.setenv("MINIME_STATE_DIR", str(tmp_path))
    return tmp_path


def test_tokens_survive_a_restart_and_a_truncated_file_reads_as_signed_out(state):
    """The store is the whole reason a sign-in is worth doing once."""
    store = dataverse_auth.FileTokenStore(dataverse_auth.state_dir())

    import asyncio

    assert asyncio.run(store.get("k")) is None
    asyncio.run(store.put("k", {"access_token": "t"}))
    assert asyncio.run(store.get("k")) == {"access_token": "t"}
    assert dataverse_auth.signed_in()

    # A different collection is a different slot: `OAuth` keeps the token and the client
    # registration side by side and must not read one as the other.
    assert asyncio.run(store.get("k", collection="client")) is None

    # An expired entry is gone, not stale — otherwise a dead token would be sent forever.
    asyncio.run(store.put("short", {"access_token": "t"}, ttl=-1))
    assert asyncio.run(store.get("short")) is None

    # Half a file is "signed out", never a crash: the next step is to offer a sign-in either way.
    store._path.write_text("{not json", encoding="utf-8")
    assert asyncio.run(store.get("k")) is None

    store._path.write_text(json.dumps({"default::k": {"value": {"a": 1}}}), encoding="utf-8")
    assert asyncio.run(store.delete("k")) is True
    assert asyncio.run(store.delete("k")) is False


def test_being_signed_out_does_not_register_an_oauth_client_on_every_launch(state, monkeypatch):
    """**The defect this test exists for.**

    `OAuth` performs dynamic client registration *before* it asks the redirect handler whether a
    browser may be opened — so refusing in the handler is too late. Attaching the provider while
    signed out would register a fresh OAuth client against the researcher's Horizon account every
    single launch, and nothing in the app would ever show it.

    Asserted on what the transport is *given*, because that is the only place the decision is
    visible: by the time anything is observable on the network the registration has happened.
    """
    from backend import mcp_tools

    seen: dict[str, object] = {}

    class _Transport:
        def __init__(self, url, headers=None, auth=None):
            seen["url"] = url
            seen["auth"] = auth

    monkeypatch.setattr(mcp_tools, "StreamableHttpTransport", _Transport)
    monkeypatch.setattr(mcp_tools, "Client", lambda *a, **k: object())
    monkeypatch.setattr(mcp_tools, "MCPAdapter", lambda client: client)
    monkeypatch.setattr(mcp_tools, "_mcp_clients", {})

    mcp_tools._get_or_create_mcp_client(("dataverse",))
    assert seen["auth"] is None, "a signed-out launch must send no OAuth provider at all"

    # And once there is a token, it is attached — or signing in would achieve nothing.
    import asyncio

    asyncio.run(
        dataverse_auth.FileTokenStore(dataverse_auth.state_dir()).put(
            "token", {"access_token": "t"}
        )
    )
    monkeypatch.setattr(mcp_tools, "_mcp_clients", {})
    mcp_tools._get_or_create_mcp_client(("dataverse",))
    assert seen["auth"] is not None, "a signed-in launch must carry the provider"


def test_a_turn_is_never_blocked_waiting_for_a_browser(state):
    """The runtime provider refuses; only the Sign in button may open one.

    A graph build that stopped to wait for a login would hang the researcher's question behind a
    prompt rendered nowhere — the failure mode `_mark_mcp_unavailable` was written to replace.
    """
    import asyncio

    with pytest.raises(dataverse_auth.NotSignedIn):
        asyncio.run(dataverse_auth.for_runtime().redirect_handler("https://example.invalid/auth"))


def test_the_url_is_printed_before_any_attempt_to_open_it(state, capsys, monkeypatch):
    """This process runs inside WSL, where `webbrowser.open` finds nothing and says nothing.

    So the printed line is the contract and the browser is a convenience: a researcher whose
    distro has no interop can always paste the URL themselves. Printing *after* a failed open
    would be the same bug wearing a different coat, which is why the order is what is asserted.
    """
    import asyncio

    order: list[str] = []
    monkeypatch.setattr(
        dataverse_auth, "_open_in_browser", lambda url: order.append(f"open {url}")
    )

    url = "https://example.invalid/authorize?a=1&b=2"
    asyncio.run(dataverse_auth.for_login().redirect_handler(url))

    printed = capsys.readouterr().out
    assert printed.startswith("MINIME_AUTHORIZE_URL ")
    # The whole URL, query string intact — a shell-mangled `&` would truncate it silently.
    assert url in printed
    assert order == [f"open {url}"], "the browser is opened, and only after the URL is printed"


def test_the_browser_is_sent_somewhere_a_browser_can_actually_go(state):
    """**The bug this test was written after failing to catch.**

    Its first version asserted `_callback_host == "0.0.0.0"` and passed while the sign-in was
    broken, because that field does two jobs: it is the bind address *and* the host written into
    `redirect_uri`. Chrome was handed `http://0.0.0.0:41999/callback` and answered
    `ERR_ADDRESS_INVALID` — a valid thing to listen on is not a valid thing to navigate to.

    So the assertion is now on the URI the browser is given, which was observable the whole time.
    Both halves matter and they differ:

    - the browser goes to loopback, on Windows;
    - the listener binds every interface, because WSL2's forwarding "is not always visible from
      Windows" for a loopback-only bind — the same finding that makes the backend bind 0.0.0.0.
    """
    provider = dataverse_auth.for_login()

    redirect = str(provider.context.client_metadata.redirect_uris[0])
    assert redirect == f"http://127.0.0.1:{dataverse_auth.CALLBACK_PORT}/callback", redirect
    # Said plainly, because this is the exact value that shipped broken.
    assert "0.0.0.0" not in redirect, "a browser cannot navigate to 0.0.0.0"

    # And the listener is still widened, or the browser reaches loopback and finds nothing.
    assert provider._callback_host == "0.0.0.0"
    # Fixed, because the redirect URI registered at sign-in must be the one it later answers on.
    assert provider._callback_port == dataverse_auth.CALLBACK_PORT


def test_a_registration_from_the_broken_build_is_not_reused(state):
    """The first build advertised `http://0.0.0.0:41999/callback` and cached that registration.

    A machine that tried the sign-in then has it on disk. Reusing it sends the browser back to
    the address that produced `ERR_ADDRESS_INVALID`, and the only symptom is a callback that
    never arrives — so the second attempt fails exactly like the first, for a reason nothing
    reports. The tokens beside it are untouched: a callback address says nothing about whether a
    sign-in is still good.
    """
    import asyncio

    store = dataverse_auth.FileTokenStore(dataverse_auth.state_dir())
    asyncio.run(
        store.put(
            "client",
            {"client_id": "old", "redirect_uris": ["http://0.0.0.0:41999/callback"]},
            collection="client",
        )
    )
    asyncio.run(store.put("token", {"access_token": "keep-me"}, collection="token"))

    # **Driven through `_login`, not by calling the helper.** A test that invokes the cleanup
    # itself proves the cleanup works and says nothing about whether the sign-in performs it —
    # and deleting the call is exactly the regression that would strand these machines.
    _run_login_without_a_network(state)

    assert asyncio.run(store.get("client", collection="client")) is None
    assert asyncio.run(store.get("token", collection="token")) == {"access_token": "keep-me"}

    # A registration that already names the current address is left alone, or every sign-in
    # would register a new client and litter the researcher's Horizon account.
    current = str(dataverse_auth.for_login().context.client_metadata.redirect_uris[0])
    asyncio.run(
        store.put("client", {"client_id": "new", "redirect_uris": [current]}, collection="client")
    )
    _run_login_without_a_network(state)
    assert asyncio.run(store.get("client", collection="client")) is not None


def _run_login_without_a_network(_state) -> None:
    """Run `_login` far enough to reach the cleanup, then stop before it talks to anything."""
    import asyncio
    import contextlib

    import backend.dataverse_auth as module

    class _Stop(Exception):
        pass

    @contextlib.asynccontextmanager
    async def _client(*_args, **_kwargs):
        raise _Stop
        yield  # pragma: no cover - unreachable, keeps this an async generator

    import fastmcp

    real = fastmcp.Client
    fastmcp.Client = _client
    try:
        with contextlib.suppress(_Stop):
            asyncio.run(module._login())
    finally:
        fastmcp.Client = real
