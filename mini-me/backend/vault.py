"""Per-user secret + preference storage backed by WorkOS Vault.

Holds each researcher's bring-your-own LLM provider API keys (and a mirror of
their non-secret model-routing preferences) encrypted at rest in WorkOS Vault,
scoped by their WorkOS user id (the JWT ``sub``). This is the storage layer for
the "Vault ON" mode of the model-configuration feature; "Vault OFF" keeps keys
in the browser and never calls this module.

Objects are named ``minime/{user_id}/llm/{provider}`` for keys and
``minime/{user_id}/config`` for the preference mirror. Names are namespaced by
user id so one tenant can never read another's secrets, and each object also
carries a ``key_context`` binding the ciphertext to that user.

Requires ``WORKOS_API_KEY`` and ``WORKOS_CLIENT_ID``; without them every call
raises ``VaultUnavailable`` so callers can surface a clear error instead of a
500. The same env vars already power ``auth.py``.
"""

from __future__ import annotations

import json
import logging
import os
from typing import Any, Iterable

logger = logging.getLogger(__name__)

# Providers a stored key may belong to. Mirrors the frontend provider list.
KNOWN_PROVIDERS = ("openai", "anthropic", "google", "mistral", "custom")

_NAME_PREFIX = "minime"

_async_client: Any | None = None
_not_found_cls: type[BaseException] | None = None


class VaultUnavailable(RuntimeError):
    """Raised when WorkOS Vault is not configured on the server."""


def _not_found_error() -> type[BaseException]:
    """The WorkOS 'object does not exist' exception (cached)."""
    global _not_found_cls
    if _not_found_cls is None:
        from workos import NotFoundError

        _not_found_cls = NotFoundError
    return _not_found_cls


def _get_async_client() -> Any:
    global _async_client
    if _async_client is not None:
        return _async_client
    api_key = os.getenv("WORKOS_API_KEY")
    client_id = os.getenv("WORKOS_CLIENT_ID")
    if not api_key or not client_id:
        raise VaultUnavailable(
            "WORKOS_API_KEY / WORKOS_CLIENT_ID must be set to use WorkOS Vault "
            "key storage. Switch the config panel to client-only mode, or set "
            "these env vars."
        )
    import workos  # local import keeps startup cheap when Vault is unused

    _async_client = workos.AsyncWorkOSClient(api_key=api_key, client_id=client_id)
    return _async_client


def _safe_segment(value: str) -> str:
    """Sanitize a path segment so it can never break out of its namespace."""
    cleaned = "".join(ch for ch in str(value) if ch.isalnum() or ch in ("-", "_"))
    if not cleaned:
        raise ValueError(f"Invalid name segment: {value!r}")
    return cleaned


def _key_object_name(user_id: str, provider: str) -> str:
    return f"{_NAME_PREFIX}/{_safe_segment(user_id)}/llm/{_safe_segment(provider)}"


def _config_object_name(user_id: str) -> str:
    return f"{_NAME_PREFIX}/{_safe_segment(user_id)}/config"


def _asta_object_name(user_id: str) -> str:
    return f"{_NAME_PREFIX}/{_safe_segment(user_id)}/asta/token"


def _key_prefix(user_id: str) -> str:
    return f"{_NAME_PREFIX}/{_safe_segment(user_id)}/llm/"


def _key_context(user_id: str, purpose: str) -> dict[str, str]:
    return {"app": "minime", "user_id": _safe_segment(user_id), "purpose": purpose}


async def _read_value(name: str) -> str | None:
    """Return the decrypted value for ``name``, or None if it does not exist."""
    client = _get_async_client()
    try:
        obj = await client.vault.get_name(name=name)
    except _not_found_error():
        return None
    return getattr(obj, "value", None)


async def _upsert(name: str, value: str, key_context: dict[str, str]) -> None:
    """Create the object, or update it in place if it already exists."""
    client = _get_async_client()
    try:
        existing = await client.vault.get_name(name=name)
    except _not_found_error():
        existing = None
    if existing is not None:
        await client.vault.update_kv(id=existing.id, value=value)
        return
    await client.vault.create_kv(key_context=key_context, name=name, value=value)


# ---------------------------------------------------------------------------
# API keys
# ---------------------------------------------------------------------------


async def save_key(
    user_id: str, provider: str, api_key: str, base_url: str | None = None
) -> None:
    """Encrypt and store one provider key for a user (upsert)."""
    if provider not in KNOWN_PROVIDERS:
        raise ValueError(f"Unknown provider: {provider}")
    payload = json.dumps({"api_key": api_key, "base_url": base_url or None})
    await _upsert(
        _key_object_name(user_id, provider),
        payload,
        _key_context(user_id, "llm-key"),
    )


async def get_keys(
    user_id: str, providers: Iterable[str]
) -> dict[str, dict[str, Any]]:
    """Return ``{provider: {api_key, base_url}}`` for the providers that exist."""
    out: dict[str, dict[str, Any]] = {}
    for provider in {p for p in providers if p in KNOWN_PROVIDERS}:
        raw = await _read_value(_key_object_name(user_id, provider))
        if not raw:
            continue
        try:
            record = json.loads(raw)
        except (json.JSONDecodeError, ValueError):
            continue
        if record.get("api_key"):
            out[provider] = {
                "api_key": record["api_key"],
                "base_url": record.get("base_url"),
            }
    return out


async def delete_key(user_id: str, provider: str) -> bool:
    """Delete a provider key. Returns True if one existed."""
    if provider not in KNOWN_PROVIDERS:
        raise ValueError(f"Unknown provider: {provider}")
    client = _get_async_client()
    name = _key_object_name(user_id, provider)
    try:
        obj = await client.vault.get_name(name=name)
    except _not_found_error():
        return False
    await client.vault.delete_kv(id=obj.id)
    return True


async def list_connected(user_id: str) -> list[str]:
    """Return the providers that currently have a stored key for this user."""
    client = _get_async_client()
    prefix = _key_prefix(user_id)
    connected: list[str] = []
    # ``list_kv`` returns an AsyncPage that auto-paginates when iterated.
    page = await client.vault.list_kv(limit=100)
    async for summary in page:
        name = getattr(summary, "name", "")
        if name.startswith(prefix):
            provider = name[len(prefix):]
            if provider in KNOWN_PROVIDERS:
                connected.append(provider)
    return connected


# ---------------------------------------------------------------------------
# Asta access token (per-user, paste-and-store; see backend.asta_auth)
# ---------------------------------------------------------------------------


async def save_asta_token(user_id: str, token: str) -> None:
    """Encrypt and store this user's Asta access token (upsert)."""
    await _upsert(
        _asta_object_name(user_id),
        token,
        _key_context(user_id, "asta-token"),
    )


async def get_asta_token(user_id: str) -> str | None:
    """Return this user's stored Asta access token, or None if unset."""
    raw = await _read_value(_asta_object_name(user_id))
    return raw or None


async def delete_asta_token(user_id: str) -> bool:
    """Delete this user's Asta token. Returns True if one existed."""
    client = _get_async_client()
    name = _asta_object_name(user_id)
    try:
        obj = await client.vault.get_name(name=name)
    except _not_found_error():
        return False
    await client.vault.delete_kv(id=obj.id)
    return True


# ---------------------------------------------------------------------------
# Non-secret preference mirror (default model + per-subagent overrides)
# ---------------------------------------------------------------------------


async def save_config(user_id: str, model_config: dict[str, Any]) -> None:
    await _upsert(
        _config_object_name(user_id),
        json.dumps(model_config),
        _key_context(user_id, "llm-config"),
    )


async def get_config(user_id: str) -> dict[str, Any] | None:
    raw = await _read_value(_config_object_name(user_id))
    if not raw:
        return None
    try:
        return json.loads(raw)
    except (json.JSONDecodeError, ValueError):
        return None
