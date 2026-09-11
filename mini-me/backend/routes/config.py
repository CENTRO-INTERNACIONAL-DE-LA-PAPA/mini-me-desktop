"""Model & API-key configuration endpoints (WorkOS Vault, "Vault ON" mode)."""

from __future__ import annotations

import asyncio
import time
from typing import Awaitable, TypeVar

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

import backend.vault as vault_store
from backend.asta_auth import looks_like_token, token_status
from backend.models import PROVIDER_SPECS, build_chat_model
from backend.routes.common import _parse_json_body, _require_auth, _require_user

_T = TypeVar("_T")


async def _vault_call(action: str, awaitable: Awaitable[_T]) -> _T | Response:
    """Run one vault call, turning its two known failure modes into a Response.

    Every handler below repeated this same `try/except` around whatever it
    actually wanted from the vault, with only the ``action`` word in the
    message changing (`"vault read failed"` / `"vault write failed"` / ...).
    """
    try:
        return await awaitable
    except vault_store.VaultUnavailable as exc:
        return JSONResponse({"error": str(exc)}, status_code=503)
    except Exception as exc:  # noqa: BLE001
        return JSONResponse({"error": f"vault {action} failed: {exc}"}, status_code=502)


# ---------------------------------------------------------------------------
# Model & API-key configuration (WorkOS Vault storage for "Vault ON" mode).
#
# These endpoints never return key material — only connected-provider booleans
# and the non-secret routing config. Client-only mode never calls them (it
# keeps keys in the browser and passes them per run).
# ---------------------------------------------------------------------------


async def get_config(request: Request) -> Response:
    user_id, err = _require_user(request)
    if err is not None:
        return err

    async def _both() -> tuple[dict, list]:
        return await vault_store.get_config(user_id), await vault_store.list_connected(
            user_id
        )

    result = await _vault_call("read", _both())
    if isinstance(result, Response):
        return result
    model_config, connected = result
    return JSONResponse(
        {"model_config": model_config, "providers_connected": connected}
    )


async def save_config(request: Request) -> Response:
    user_id, err = _require_user(request)
    if err is not None:
        return err
    payload, err = await _parse_json_body(request)
    if err is not None:
        return err
    model_config = payload.get("model_config")
    if not isinstance(model_config, dict):
        return JSONResponse(
            {"error": "'model_config' object is required"}, status_code=400
        )
    result = await _vault_call("write", vault_store.save_config(user_id, model_config))
    if isinstance(result, Response):
        return result
    return JSONResponse({"saved": True})


async def save_key(request: Request) -> Response:
    user_id, err = _require_user(request)
    if err is not None:
        return err
    payload, err = await _parse_json_body(request)
    if err is not None:
        return err
    provider = payload.get("provider")
    api_key = payload.get("api_key")
    base_url = payload.get("base_url") or None
    if provider not in PROVIDER_SPECS:
        return JSONResponse({"error": "unknown provider"}, status_code=400)
    if not isinstance(api_key, str) or not api_key.strip():
        return JSONResponse({"error": "'api_key' is required"}, status_code=400)
    result = await _vault_call(
        "write", vault_store.save_key(user_id, provider, api_key.strip(), base_url)
    )
    if isinstance(result, Response):
        return result
    return JSONResponse({"saved": True, "provider": provider})


async def delete_key(request: Request) -> Response:
    user_id, err = _require_user(request)
    if err is not None:
        return err
    provider = request.path_params["provider"]
    if provider not in PROVIDER_SPECS:
        return JSONResponse({"error": "unknown provider"}, status_code=400)
    result = await _vault_call("delete", vault_store.delete_key(user_id, provider))
    if isinstance(result, Response):
        return result
    existed = result
    return JSONResponse({"deleted": existed}, status_code=200 if existed else 404)


async def test_key(request: Request) -> Response:
    """Validate a key by issuing one minimal model call. Works in both modes."""
    if (unauth := _require_auth(request)) is not None:
        return unauth
    payload, err = await _parse_json_body(request)
    if err is not None:
        return err
    provider = payload.get("provider")
    api_key = payload.get("api_key")
    model_id = payload.get("model_id")
    base_url = payload.get("base_url") or None
    if provider not in PROVIDER_SPECS:
        return JSONResponse({"ok": False, "error": "unknown provider"}, status_code=400)
    if not model_id:
        return JSONResponse({"ok": False, "error": "'model_id' is required"}, status_code=400)
    if PROVIDER_SPECS[provider]["needs_base_url"] and not base_url:
        return JSONResponse({"ok": False, "error": "base_url is required"}, status_code=400)

    try:
        model = build_chat_model(
            f"{provider}::{model_id}", {"api_key": api_key, "base_url": base_url}
        )
        await asyncio.wait_for(model.ainvoke("ping"), timeout=25)
    except asyncio.TimeoutError:
        return JSONResponse({"ok": False, "error": "provider timed out"}, status_code=200)
    except Exception as exc:  # noqa: BLE001 — surface auth/quota errors to the UI
        return JSONResponse({"ok": False, "error": str(exc)[:300]}, status_code=200)
    return JSONResponse({"ok": True})


# ---------------------------------------------------------------------------
# Asta access token (per-user, paste-and-store — self-service token refresh).
#
# The token authenticates the `asta` CLI in the sandbox (theorizer, DataVoyager,
# PDF extraction). It expires ~weekly; storing it per-user in Vault and editing
# it here means an expired token is refreshed in seconds, without a redeploy.
# These endpoints never return the token itself — only its connection + expiry.
# ---------------------------------------------------------------------------


async def get_asta_status(request: Request) -> Response:
    """Report whether the user has an Asta token stored, and when it expires."""
    user_id, err = _require_user(request)
    if err is not None:
        return err
    result = await _vault_call("read", vault_store.get_asta_token(user_id))
    if isinstance(result, Response):
        return result
    token = result
    return JSONResponse(token_status(token or "", int(time.time())))


async def save_asta_token(request: Request) -> Response:
    """Validate and store a pasted Asta access token; reject empty/expired pastes."""
    user_id, err = _require_user(request)
    if err is not None:
        return err
    payload, err = await _parse_json_body(request)
    if err is not None:
        return err
    token = payload.get("token")
    if not isinstance(token, str) or not looks_like_token(token):
        return JSONResponse(
            {"error": "a valid Asta token is required (run `asta auth print-token`)"},
            status_code=400,
        )
    token = token.strip()
    status = token_status(token, int(time.time()))
    if status["expired"]:
        return JSONResponse(
            {"error": "that token is already expired — run `asta auth login` again, then paste a fresh one"},
            status_code=400,
        )
    result = await _vault_call("write", vault_store.save_asta_token(user_id, token))
    if isinstance(result, Response):
        return result
    return JSONResponse({"saved": True, **status})


async def delete_asta_token(request: Request) -> Response:
    """Remove the user's stored Asta token."""
    user_id, err = _require_user(request)
    if err is not None:
        return err
    result = await _vault_call("delete", vault_store.delete_asta_token(user_id))
    if isinstance(result, Response):
        return result
    existed = result
    return JSONResponse({"deleted": existed}, status_code=200 if existed else 404)
