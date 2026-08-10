"""Pure helpers for introspecting a pasted Asta access token.

The Asta CLI authenticates with an OAuth access token (`asta auth print-token`).
Access tokens expire (~weekly), and the deployed backend historically pinned a
single static ``ASTA_TOKEN`` env var with no refresh — so once it lapsed, every
`asta` call in the sandbox silently returned nothing ("Could not obtain a task
id …"). To make refresh self-service, users paste a fresh token into Settings;
these helpers let us **reject an already-expired paste** and **show when a stored
token expires**, without ever verifying the signature ourselves — real
verification happens at Asta when the CLI uses the token.

Everything here is a pure function of its inputs (the token string + a supplied
``now`` timestamp), so it is fully unit-testable with no network or clock.
Access tokens are JWTs; we base64url-decode the claims segment to read ``exp``.
Opaque (non-JWT) tokens are accepted but report an unknown expiry.
"""

from __future__ import annotations

import base64
import binascii
import json
from typing import Any


def _b64url_decode(segment: str) -> bytes:
    padding = "=" * (-len(segment) % 4)
    return base64.urlsafe_b64decode(segment + padding)


def decode_jwt_claims(token: str) -> dict[str, Any] | None:
    """Return a JWT's claims (unverified), or ``None`` if it is not a JWT."""
    parts = (token or "").split(".")
    if len(parts) != 3:
        return None
    try:
        claims = json.loads(_b64url_decode(parts[1]))
    except (ValueError, binascii.Error, json.JSONDecodeError, UnicodeDecodeError):
        return None
    return claims if isinstance(claims, dict) else None


def token_expiry(token: str) -> int | None:
    """The token's ``exp`` (unix seconds), or ``None`` for opaque/exp-less tokens."""
    claims = decode_jwt_claims(token)
    if not claims:
        return None
    exp = claims.get("exp")
    if isinstance(exp, bool):  # bool is an int subclass — reject explicitly
        return None
    return int(exp) if isinstance(exp, (int, float)) else None


def looks_like_token(token: str) -> bool:
    """Cheap sanity check that a paste is plausibly a token (non-empty, no spaces)."""
    token = (token or "").strip()
    return bool(token) and " " not in token and len(token) >= 16


def token_status(token: str, now_ts: int) -> dict[str, Any]:
    """Describe a token for the UI: connected / expiry / expired / seconds left.

    Never returns the token itself. ``expires_at`` is unix seconds (the frontend
    formats it); ``None`` means the expiry could not be determined (opaque token).
    """
    token = (token or "").strip()
    if not token:
        return {
            "connected": False,
            "expires_at": None,
            "expired": False,
            "seconds_left": None,
        }
    exp = token_expiry(token)
    if exp is None:
        # Opaque token / no exp claim — can't introspect; assume usable.
        return {
            "connected": True,
            "expires_at": None,
            "expired": False,
            "seconds_left": None,
        }
    seconds_left = exp - int(now_ts)
    return {
        "connected": True,
        "expires_at": exp,
        "expired": seconds_left <= 0,
        "seconds_left": seconds_left,
    }
