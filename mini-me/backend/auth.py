"""WorkOS AuthKit auth layer for the LangGraph runtime.

Verifies WorkOS access tokens (JWTs) against the WorkOS JWKS, enforces an
email-domain allowlist (default ``cgiar.org``), and stamps thread/store/
assistant metadata with the caller's identity so users only see their own
resources.

Local development: when ``DEEP_ATD_RUNTIME_MODE`` is not ``production`` and
the request has no ``Authorization`` header, the handler returns a stub
identity (``local-user``) so ``langgraph dev`` keeps working without any
WorkOS configuration.
"""

from __future__ import annotations

import asyncio
import logging
import os
from typing import Any

import jwt
from jwt import PyJWKClient
from langgraph_sdk.auth import is_studio_user, Auth

logger = logging.getLogger(__name__)

auth = Auth()


WORKOS_ISSUER = "https://api.workos.com"

# In-process cache mapping ``sub`` (WorkOS user id) → email. WorkOS access
# tokens don't include an ``email`` claim, so we look it up via the
# ``user_management.get_user`` API once per user per process.
_email_cache: dict[str, str] = {}
_workos_client: Any | None = None
_workos_warned_missing_key = False


def _is_production() -> bool:
    return os.getenv("DEEP_ATD_RUNTIME_MODE", "").lower() in {"production", "prod"}


def _allowed_domains() -> tuple[str, ...]:
    raw = os.getenv("AUTH_ALLOWED_EMAIL_DOMAINS", "cgiar.org")
    return tuple(d.strip().lower() for d in raw.split(",") if d.strip())


def _local_identity() -> Auth.types.MinimalUserDict:
    return {
        "identity": os.getenv("DEEP_ATD_LOCAL_USER_ID", "local-user"),
        "email": "local@cgiar.org",
        "is_authenticated": True,
    }


_jwks_client: PyJWKClient | None = None


def _get_workos_client() -> Any | None:
    """Return a cached AsyncWorkOSClient, or None if ``WORKOS_API_KEY`` is unset.

    The API key is required to call ``user_management.get_user`` so we can
    look up the email for a given access-token ``sub``. If it's missing, we
    skip the email-domain check and rely on WorkOS's own organization
    domain policy as the only barrier — logged once as a warning.
    """
    global _workos_client, _workos_warned_missing_key
    if _workos_client is not None:
        return _workos_client
    api_key = os.getenv("WORKOS_API_KEY")
    client_id = os.getenv("WORKOS_CLIENT_ID")
    if not api_key or not client_id:
        if not _workos_warned_missing_key:
            logger.warning(
                "WORKOS_API_KEY not set — skipping backend email-domain check. "
                "Make sure the WorkOS organization domain policy is restricted "
                "to your allowed domain(s) (e.g. cgiar.org)."
            )
            _workos_warned_missing_key = True
        return None
    import workos  # local import keeps startup cheap when auth is disabled
    _workos_client = workos.AsyncWorkOSClient(api_key=api_key, client_id=client_id)
    return _workos_client


async def _resolve_email(sub: str) -> str | None:
    """Return the cached email for ``sub``, fetching from WorkOS on miss."""
    cached = _email_cache.get(sub)
    if cached is not None:
        return cached
    client = _get_workos_client()
    if client is None:
        return None
    try:
        user = await client.user_management.get_user(sub)
    except Exception as exc:  # noqa: BLE001 — surface as 401 below
        raise Auth.exceptions.HTTPException(
            status_code=401, detail=f"Failed to resolve user email: {exc}"
        ) from exc
    email = getattr(user, "email", None)
    if email:
        _email_cache[sub] = email.lower()
    return _email_cache.get(sub)


def _get_jwks_client() -> PyJWKClient:
    """Lazy JWKS client. WorkOS publishes per-client JWKS at
    ``https://api.workos.com/sso/jwks/<CLIENT_ID>``.
    """
    global _jwks_client
    if _jwks_client is None:
        client_id = os.getenv("WORKOS_CLIENT_ID")
        if not client_id:
            raise Auth.exceptions.HTTPException(
                status_code=500,
                detail="WORKOS_CLIENT_ID is not configured on the server",
            )
        _jwks_client = PyJWKClient(
            f"{WORKOS_ISSUER}/sso/jwks/{client_id}",
            cache_keys=True,
            lifespan=24 * 3600,
        )
    return _jwks_client


async def _verify_workos_jwt(token: str) -> dict[str, Any]:
    try:
        # PyJWKClient.get_signing_key_from_jwt does blocking HTTP to fetch the
        # JWKS. langgraph dev's blocking-call detector errors out if we run it
        # on the event loop, so push it to a worker thread.
        signing_key = (
            await asyncio.to_thread(
                _get_jwks_client().get_signing_key_from_jwt, token
            )
        ).key
    except Auth.exceptions.HTTPException:
        raise
    except Exception as exc:  # noqa: BLE001 — invalid JWT structure or JWKS lookup failure
        raise Auth.exceptions.HTTPException(
            status_code=401, detail=f"Invalid token: {exc}"
        ) from exc
    try:
        # We don't pin `issuer=` or `audience=` — WorkOS AuthKit access tokens
        # don't include an `aud` claim, and the `iss` value varies by product
        # variant. Authenticity comes from JWKS signature verification (the
        # JWKS is fetched per-client_id, so a valid signature already proves
        # the token was issued for this app); authorization comes from the
        # email-domain allowlist below.
        return jwt.decode(
            token,
            signing_key,
            algorithms=["RS256"],
            options={"require": ["sub", "exp"], "verify_aud": False},
        )
    except jwt.ExpiredSignatureError as exc:
        raise Auth.exceptions.HTTPException(status_code=401, detail="Token expired") from exc
    except jwt.InvalidTokenError as exc:
        raise Auth.exceptions.HTTPException(status_code=401, detail=f"Invalid token: {exc}") from exc
    except Exception as exc:  # noqa: BLE001 — surface any verification failure as 401
        raise Auth.exceptions.HTTPException(
            status_code=401, detail=f"Token verification failed: {exc}"
        ) from exc


@auth.authenticate
async def authenticate(authorization: str | None) -> Auth.types.MinimalUserDict:
    if not authorization:
        if _is_production():
            raise Auth.exceptions.HTTPException(
                status_code=401, detail="Missing Authorization header"
            )
        return _local_identity()

    scheme, _, token = authorization.partition(" ")
    if scheme.lower() != "bearer" or not token:
        raise Auth.exceptions.HTTPException(
            status_code=401, detail="Authorization header must be 'Bearer <token>'"
        )

    claims = await _verify_workos_jwt(token)
    sub = str(claims["sub"])

    # Access tokens don't carry email; look it up via WorkOS user_management.
    # When ``WORKOS_API_KEY`` is unset we skip the domain check and trust the
    # WorkOS organization domain policy.
    email = (claims.get("email") or "").lower() or (await _resolve_email(sub) or "")

    if email:
        allowed = _allowed_domains()
        if not any(email.endswith(f"@{domain}") for domain in allowed):
            raise Auth.exceptions.HTTPException(
                status_code=403,
                detail=f"Email domain not allowed: {email.split('@', 1)[-1]}",
            )

    return {
        "identity": sub,
        "email": email or "unknown",
        "is_authenticated": True,
    }


# ---------------------------------------------------------------------------
# Authorization: scope every LangGraph resource to its owner.
# ---------------------------------------------------------------------------


def _stamp_owner(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> None:
    """Stamp ``metadata["owner"] = ctx.user.identity`` on the value in place.

    Callers of the auth handler (e.g. ``langgraph_runtime_inmem.ops.Threads.put``)
    hold their own reference to the metadata dict before invoking us — so we
    must mutate that exact dict, not swap it out. ``{}`` is falsy in Python,
    so any ``or {}`` shortcut breaks this contract when metadata starts empty.
    """
    metadata = value.get("metadata")
    if metadata is None:
        metadata = {}
        value["metadata"] = metadata
    metadata["owner"] = ctx.user.identity


@auth.on.threads.create
async def threads_create(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> None:
    if is_studio_user(ctx.user):
        return
    _stamp_owner(ctx, value)


@auth.on.threads.read
async def threads_read(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.threads.update
async def threads_update(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.threads.delete
async def threads_delete(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.threads.search
async def threads_search(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.assistants.create
async def assistants_create(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> None:
    if is_studio_user(ctx.user):
        return
    _stamp_owner(ctx, value)


@auth.on.assistants.read
async def assistants_read(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.assistants.update
async def assistants_update(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.assistants.delete
async def assistants_delete(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.assistants.search
async def assistants_search(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> Auth.types.FilterType | None:
    if is_studio_user(ctx.user):
        return None
    return {"owner": ctx.user.identity}


@auth.on.store
async def scope_store(ctx: Auth.types.AuthContext, value: dict[str, Any]) -> None:
    """Force every store operation under ``(user_id, ...)``.

    The agent already namespaces memories as ``(assistant_id, user_id)``
    via ``_memory_namespace_for_runtime``; this guard is a defence in
    depth in case a future code path constructs a namespace from
    user-controlled input.
    """
    if is_studio_user(ctx.user):
        return
    namespace = tuple(value.get("namespace") or ())
    if not namespace or namespace[0] != ctx.user.identity:
        value["namespace"] = (ctx.user.identity, *namespace)
