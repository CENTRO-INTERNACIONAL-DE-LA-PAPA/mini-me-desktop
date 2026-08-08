"""Tests for shared Asta-token resolution (`backend.runtime`).

Regression guard for the field bug where the theorizer/DataVoyager status polls
ran outside ``agent()`` and therefore authenticated the sandbox `asta` CLI with
the stale process-wide ``ASTA_TOKEN`` env var — so a completed run polled as
"running" forever even after the user refreshed their token. ``resolve_asta_token``
+ ``asta_token_scope`` are what the poll routes now use to bind the user's Vault
token into the ContextVar the sandbox reads.
"""

from __future__ import annotations

import asyncio

import backend.vault as vault_store
from backend.runtime import _active_asta_token, asta_token_scope, resolve_asta_token


def _run(coro):
    return asyncio.run(coro)


# ---------------------------------------------------------------------------
# resolve_asta_token
# ---------------------------------------------------------------------------

def test_resolve_prefers_client_token(monkeypatch) -> None:
    """A client-supplied token wins and Vault is never consulted."""
    async def _t() -> None:
        called = {"vault": False}

        async def fake_get(_uid):
            called["vault"] = True
            return "vault-tok"

        monkeypatch.setattr(vault_store, "get_asta_token", fake_get)
        assert await resolve_asta_token("u1", "client-tok") == "client-tok"
        assert called["vault"] is False

    _run(_t())


def test_resolve_falls_back_to_vault(monkeypatch) -> None:
    async def _t() -> None:
        async def fake_get(uid):
            assert uid == "u1"
            return "vault-tok"

        monkeypatch.setattr(vault_store, "get_asta_token", fake_get)
        assert await resolve_asta_token("u1", None) == "vault-tok"

    _run(_t())


def test_resolve_none_without_user() -> None:
    assert _run(resolve_asta_token(None, None)) is None


def test_resolve_swallows_vault_error(monkeypatch) -> None:
    """Vault failure degrades to None so the env fallback still applies."""
    async def _t() -> None:
        async def boom(_uid):
            raise RuntimeError("vault down")

        monkeypatch.setattr(vault_store, "get_asta_token", boom)
        assert await resolve_asta_token("u1", None) is None

    _run(_t())


# ---------------------------------------------------------------------------
# asta_token_scope
# ---------------------------------------------------------------------------

def test_scope_sets_then_resets(monkeypatch) -> None:
    async def _t() -> None:
        async def fake_get(_uid):
            return "vault-tok"

        monkeypatch.setattr(vault_store, "get_asta_token", fake_get)
        assert _active_asta_token.get() is None
        async with asta_token_scope("u1") as tok:
            assert tok == "vault-tok"
            assert _active_asta_token.get() == "vault-tok"
        # Reset on exit — never leaks across requests.
        assert _active_asta_token.get() is None

    _run(_t())


def test_scope_resets_even_on_exception(monkeypatch) -> None:
    async def _t() -> None:
        async def fake_get(_uid):
            return "vault-tok"

        monkeypatch.setattr(vault_store, "get_asta_token", fake_get)
        try:
            async with asta_token_scope("u1"):
                assert _active_asta_token.get() == "vault-tok"
                raise ValueError("boom")
        except ValueError:
            pass
        assert _active_asta_token.get() is None

    _run(_t())


def test_scope_with_no_token_binds_none(monkeypatch) -> None:
    async def _t() -> None:
        async def none_get(_uid):
            return None

        monkeypatch.setattr(vault_store, "get_asta_token", none_get)
        async with asta_token_scope("u1") as tok:
            assert tok is None
            assert _active_asta_token.get() is None

    _run(_t())
