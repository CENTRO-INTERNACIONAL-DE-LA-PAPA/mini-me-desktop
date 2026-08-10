"""Tests for the pasted-Asta-token introspection helpers (`backend.asta_auth`).

These pin the pure contract used by the self-service token-refresh feature: a
JWT's ``exp`` is decoded (unverified) to compute expiry, an already-expired paste
is detectable, opaque (non-JWT) tokens report unknown expiry, and malformed input
never raises. No network, no clock — ``now`` is supplied.
"""

from __future__ import annotations

import base64
import json

from backend.asta_auth import (
    decode_jwt_claims,
    looks_like_token,
    token_expiry,
    token_status,
)


def _jwt(claims: dict) -> str:
    """Build an unsigned JWT-shaped string with the given claims."""
    def seg(obj: dict) -> str:
        raw = json.dumps(obj).encode()
        return base64.urlsafe_b64encode(raw).rstrip(b"=").decode()

    return f"{seg({'alg': 'none'})}.{seg(claims)}.sig"


NOW = 1_700_000_000


def test_decode_jwt_claims_reads_payload() -> None:
    token = _jwt({"exp": NOW + 100, "sub": "user-1"})
    claims = decode_jwt_claims(token)
    assert claims is not None
    assert claims["sub"] == "user-1"
    assert claims["exp"] == NOW + 100


def test_decode_jwt_claims_rejects_non_jwt() -> None:
    assert decode_jwt_claims("not-a-jwt") is None
    assert decode_jwt_claims("a.b") is None  # only 2 segments
    assert decode_jwt_claims("a.!!!notbase64!!!.c") is None
    assert decode_jwt_claims("") is None


def test_token_expiry_variants() -> None:
    assert token_expiry(_jwt({"exp": NOW + 500})) == NOW + 500
    assert token_expiry(_jwt({"sub": "x"})) is None  # no exp
    assert token_expiry(_jwt({"exp": True})) is None  # bool is not a valid exp
    assert token_expiry("opaque-token-value") is None


def test_looks_like_token() -> None:
    assert looks_like_token("a" * 20)
    assert looks_like_token(_jwt({"exp": NOW}))
    assert not looks_like_token("")
    assert not looks_like_token("   ")
    assert not looks_like_token("short")  # < 16 chars
    assert not looks_like_token("has spaces in it and is long enough")


def test_token_status_valid() -> None:
    status = token_status(_jwt({"exp": NOW + 3600}), NOW)
    assert status["connected"] is True
    assert status["expired"] is False
    assert status["expires_at"] == NOW + 3600
    assert status["seconds_left"] == 3600


def test_token_status_expired() -> None:
    status = token_status(_jwt({"exp": NOW - 10}), NOW)
    assert status["connected"] is True
    assert status["expired"] is True
    assert status["seconds_left"] == -10


def test_token_status_opaque_token_is_connected_unknown_expiry() -> None:
    status = token_status("opaque-token-abcdefgh", NOW)
    assert status["connected"] is True
    assert status["expired"] is False
    assert status["expires_at"] is None
    assert status["seconds_left"] is None


def test_token_status_empty_is_disconnected() -> None:
    status = token_status("", NOW)
    assert status == {
        "connected": False,
        "expires_at": None,
        "expired": False,
        "seconds_left": None,
    }
