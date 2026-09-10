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


def test_the_callback_is_reachable_from_the_windows_browser(state):
    """Bound on every interface, advertised on a fixed port.

    The listener runs in WSL and the browser is on Windows. This repository already learned that
    WSL2's loopback forwarding "is not always visible from Windows" for a 127.0.0.1-only bind —
    it is why the backend binds 0.0.0.0 — and a callback the browser cannot reach fails after a
    five-minute timeout with nothing to read.

    The port is fixed because the redirect URI registered at sign-in has to be the one the
    listener later answers on.
    """
    provider = dataverse_auth.for_login()
    assert provider._callback_host == "0.0.0.0"
    assert provider._callback_port == dataverse_auth.CALLBACK_PORT
