"""An attachment a provider cannot take is replaced by a note, never sent.

deepagents' `read_file` hands images, audio, video and PDFs to the model as attachments, and
treats any profile field that is missing as supported. Most models here have no profile (every
OpenRouter id), so an unsupported file reached the provider, which refused the whole request with
a 400 — through OpenRouter, `400 Invalid value: 'file'` for a PDF. The profile has to say what the
provider cannot take, and the rest of the profile has to survive.
"""

from __future__ import annotations

import pytest

from backend.models import UNSUPPORTED_ATTACHMENTS, build_chat_model

_KEYS = {
    "openai": {"api_key": "sk-test"},
    "custom": {"api_key": "sk-test", "base_url": "https://openrouter.ai/api/v1"},
    "anthropic": {"api_key": "sk-test"},
    "mistral": {"api_key": "sk-test"},
}
_MODELS = {
    "openai": "gpt-4o",
    "custom": "openai/gpt-4o-mini",
    "anthropic": "claude-sonnet-4-5",
    "mistral": "mistral-large-latest",
}


@pytest.mark.parametrize("provider", sorted(UNSUPPORTED_ATTACHMENTS))
def test_each_provider_declares_what_it_cannot_take(provider):
    model = build_chat_model(f"{provider}::{_MODELS[provider]}", _KEYS[provider])
    for field, value in UNSUPPORTED_ATTACHMENTS[provider].items():
        assert model.profile[field] is value, f"{provider}: {field}"


def test_openrouter_models_never_get_a_pdf_attachment():
    model = build_chat_model("custom::openai/gpt-4o-mini", _KEYS["custom"])
    assert model.profile["pdf_tool_message"] is False


def test_images_stay_allowed_for_openai_models():
    # OpenAI's format accepts images in a tool result; ruling them out would cost the agent its
    # sight of the charts it draws.
    assert "image_tool_message" not in UNSUPPORTED_ATTACHMENTS["openai"]
    assert "image_inputs" not in UNSUPPORTED_ATTACHMENTS["custom"]
