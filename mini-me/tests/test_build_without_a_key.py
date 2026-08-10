"""Building a model must not require a credential.

`build_chat_model`'s docstring asserted that `init_chat_model` constructs lazily and that passing
no key never raises. It is false for OpenAI and Google, which build their SDK client inside
`validate_environment` during `__init__`.

The cost was three symptoms on a real install that keeps provider keys off the backend's
environment on purpose: `GET /threads/{id}/state` returned 500 on every poll, so background runs
finished with unreadable results, conversations would not open, and the coordinator reported
*"completed, but it returned no result text"* — which reads as work that never happened.

Every route that builds the graph without a run config depends on this, and none of them say so.
"""

from __future__ import annotations

import pytest

from backend.models import (
    PROVIDER_SPECS,
    _PLACEHOLDER_KEY,
    _ModelResolver,
    _require_model_keys,
    build_chat_model,
)


def _record(provider: str) -> dict | None:
    """`custom` is an OpenAI-compatible endpoint and cannot be built without a base_url."""
    return {"base_url": "https://example.invalid/v1"} if provider == "custom" else None


@pytest.mark.parametrize("provider", sorted(PROVIDER_SPECS))
def test_every_provider_builds_with_no_key_at_all(provider, monkeypatch):
    """The environment is cleared, because a stray key would hide exactly this failure.

    It hid it for months: the developer machine has a populated `.env`, so the construction that
    raises on a researcher's install succeeds on the machine where the code is written.
    """
    for name in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GOOGLE_API_KEY", "MISTRAL_API_KEY"):
        monkeypatch.delenv(name, raising=False)
    assert build_chat_model(f"{provider}::some-model", _record(provider)) is not None


@pytest.mark.parametrize("provider", sorted(PROVIDER_SPECS))
def test_an_empty_key_is_treated_as_no_key(provider, monkeypatch):
    """A saved-but-blank field is as unusable as a missing one, and used to raise the same way."""
    for name in ("OPENAI_API_KEY", "ANTHROPIC_API_KEY", "GOOGLE_API_KEY", "MISTRAL_API_KEY"):
        monkeypatch.delenv(name, raising=False)
    record = {"api_key": "", **(_record(provider) or {})}
    assert build_chat_model(f"{provider}::some-model", record) is not None


def test_a_real_key_is_still_the_one_used():
    """The placeholder is a fallback, never a replacement."""
    model = build_chat_model("openai::gpt-4o", {"api_key": "sk-a-real-looking-key"})
    assert model.openai_api_key.get_secret_value() == "sk-a-real-looking-key"


def test_the_placeholder_could_never_be_mistaken_for_a_key():
    assert "no-api-key" in _PLACEHOLDER_KEY
    assert not _PLACEHOLDER_KEY.startswith(("sk-", "key-"))


def test_a_run_without_a_key_is_still_blocked():
    """The load-bearing assertion.

    A placeholder that let an unauthenticated *run* proceed would turn a clear "connect a
    provider" message into a provider authentication error the researcher cannot act on.
    `_require_model_keys` reads the key **record**, not the constructed client, so it is unaffected
    — and this test is what keeps it that way.
    """
    resolver = _ModelResolver("openai::gpt-4o", {})
    with pytest.raises(ValueError, match="No API key configured for: openai"):
        _require_model_keys(resolver, {})


def test_a_run_with_a_key_passes_the_gate():
    resolver = _ModelResolver("openai::gpt-4o", {"openai": {"api_key": "sk-real"}})
    _require_model_keys(resolver, {})  # does not raise


def test_a_subagent_override_needs_its_own_providers_key():
    """The gate covers overrides, not just the default — a mixed-provider setup is normal."""
    resolver = _ModelResolver("anthropic::claude-sonnet-4-5", {"anthropic": {"api_key": "sk-a"}})
    with pytest.raises(ValueError, match="openai"):
        _require_model_keys(resolver, {"academic_researcher": "openai::gpt-4.1"})
