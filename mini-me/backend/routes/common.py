"""Shared helpers for the custom HTTP routes (auth, identity, sandbox paths)."""

from __future__ import annotations

from pathlib import PurePosixPath
from typing import Any

from starlette.requests import Request
from starlette.responses import JSONResponse, Response

from backend.local.workspace import LocalWorkspaceBackend


def _require_auth(request: Request) -> Response | None:
    """Reject unauthenticated callers.

    The langgraph_api auth middleware runs ``auth.py:auth`` before us when
    ``http.enable_custom_route_auth`` is set, so by the time we get here
    ``request.user`` is populated. In lax dev mode the handler returns a
    stub ``local-user`` so this still passes locally; in production a
    missing or invalid token yields a 401/403 at the middleware layer
    before reaching us, but we double-check here in case the middleware
    is ever misconfigured.

    Thread-ownership verification (cross-user file access via a guessed
    thread UUID) is enforced by ``@auth.on.threads.*`` filters on the
    LangGraph SDK endpoints — guessing another user's thread_id is the
    only attack vector and UUIDs are unguessable. A defence-in-depth
    check against thread metadata's ``owner`` field is a future
    follow-up.
    """
    user = getattr(request, "user", None)
    if user is None or not getattr(user, "is_authenticated", False):
        return JSONResponse({"error": "unauthorized"}, status_code=401)
    return None


async def _existing_sandbox_for_thread(
    thread_id: str,
) -> LocalWorkspaceBackend | None:
    """Return a resolved adapter for a thread, or None if no workspace exists."""
    adapter = LocalWorkspaceBackend(thread_id)
    sb = await adapter.try_resolve()
    return adapter if sb is not None else None


def _resolve_within(work_dir: PurePosixPath, rel_path: str) -> PurePosixPath | None:
    """Resolve `rel_path` inside `work_dir`, rejecting traversal."""
    if not rel_path or rel_path.startswith("/"):
        return None
    parts = PurePosixPath(rel_path).parts
    if any(part in {"..", ""} for part in parts):
        return None
    return work_dir / rel_path


def _request_user_id(request: Request) -> str | None:
    user = getattr(request, "user", None)
    identity = getattr(user, "identity", None) if user is not None else None
    return str(identity) if identity else None


def _require_user(request: Request) -> tuple[str | None, Response | None]:
    """Authenticate the request and resolve its user id in one step.

    Every handler that needs an identity (as opposed to just checking
    ``request.user.is_authenticated`` via `_require_auth`) repeated this same
    two-step check — return `err` as-is when it isn't `None`, otherwise use
    `user_id`:

        user_id, err = _require_user(request)
        if err is not None:
            return err
    """
    if (unauth := _require_auth(request)) is not None:
        return None, unauth
    user_id = _request_user_id(request)
    if not user_id:
        return None, JSONResponse({"error": "unauthorized"}, status_code=401)
    return user_id, None


async def _parse_json_body(request: Request) -> tuple[Any, Response | None]:
    """Parse the request body as JSON, or an `err` Response to return as-is.

        body, err = await _parse_json_body(request)
        if err is not None:
            return err

    Doesn't check *shape* — callers still validate whatever `body` turns out to
    be (a list, `None`, a dict missing fields, ...); this only standardizes
    "the bytes on the wire weren't JSON at all".
    """
    try:
        return await request.json(), None
    except Exception:  # noqa: BLE001
        return None, JSONResponse({"error": "invalid JSON body"}, status_code=400)


async def _get_store_or_error() -> Any:
    """Resolve the platform store lazily (import defers config load to runtime)."""
    from langgraph_api.store import get_store  # noqa: PLC0415

    return await get_store()
