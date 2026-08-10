"""Bring-your-own model configuration and per-request model routing.

Mini-Me ships no model of its own. The coordinator and every subagent get their
model from per-request configuration: the frontend sends a non-secret routing
config in ``configurable["model_config"]`` and either supplies keys inline
(client-only mode, in ``configurable["__llm_keys"]``) or has them read from
WorkOS Vault server-side (Vault mode). Model specs are encoded as
``"<provider>::<model_id>"`` to match the settings panel.
"""

import os
from typing import Any

from langchain.chat_models import init_chat_model
from langchain_core.runnables import RunnableConfig

import backend.vault as vault_store
from backend.runtime import _user_id_from_config

# Default model spec used for the coordinator (and any subagent left on
# "use default") when the request does not specify one — e.g. assistant schema
# inspection calls that never execute a node. Real runs without a usable key
# are blocked with a friendly error (see ``_require_model_keys``).
DEFAULT_MODEL_SPEC = os.getenv("MINIME_DEFAULT_MODEL", "openai::gpt-5.4")

# Auto-retry transient provider faults (5xx ``server_error``, 429, connection
# resets) so a one-off hiccup self-heals instead of failing the whole run in the
# user's face. ``init_chat_model`` forwards ``max_retries`` to the provider SDK
# client, which applies exponential backoff with jitter. Tune via env.
try:
    MODEL_MAX_RETRIES = int(os.getenv("MINIME_MODEL_MAX_RETRIES", "3"))
except ValueError:
    MODEL_MAX_RETRIES = 3

# provider id (from the panel) -> (langchain model_provider, api-key kwarg name,
# requires an explicit base_url).
PROVIDER_SPECS: dict[str, dict[str, Any]] = {
    "openai": {"lc_provider": "openai", "key_kwarg": "api_key", "needs_base_url": False},
    "anthropic": {"lc_provider": "anthropic", "key_kwarg": "api_key", "needs_base_url": False},
    "google": {"lc_provider": "google_genai", "key_kwarg": "google_api_key", "needs_base_url": False},
    "mistral": {"lc_provider": "mistralai", "key_kwarg": "api_key", "needs_base_url": False},
    # OpenAI-compatible custom endpoint; base_url is mandatory.
    "custom": {"lc_provider": "openai", "key_kwarg": "api_key", "needs_base_url": True},
}


def _split_spec(spec: str) -> tuple[str, str]:
    """Split ``"provider::model_id"`` into ``(provider, model_id)``."""
    provider, _, model_id = str(spec).partition("::")
    if not provider or not model_id or provider not in PROVIDER_SPECS:
        raise ValueError(f"Invalid model spec: {spec!r}")
    return provider, model_id


def build_chat_model(spec: str, key_record: dict[str, Any] | None):
    """Construct a chat model for ``spec`` using an optional key record.

    ``key_record`` is ``{"api_key": ..., "base_url": ...}`` (or None). Note
    ``init_chat_model`` constructs lazily — passing no key never raises here;
    an unauthenticated call only fails when the model is actually invoked.
    Execution-time requests are pre-checked by ``_require_model_keys``.
    """
    provider, model_id = _split_spec(spec)
    pspec = PROVIDER_SPECS[provider]
    record = key_record or {}
    api_key = record.get("api_key")
    base_url = record.get("base_url")

    kwargs: dict[str, Any] = {"max_retries": MODEL_MAX_RETRIES}
    if api_key:
        kwargs[pspec["key_kwarg"]] = api_key
    if base_url:
        kwargs["base_url"] = base_url

    return init_chat_model(model=model_id, model_provider=pspec["lc_provider"], **kwargs)


class _ModelResolver:
    """Builds and caches chat models for a single request's resolved keys."""

    def __init__(self, default_spec: str, keys: dict[str, dict[str, Any]]):
        self._default_spec = default_spec
        self._keys = keys
        self._cache: dict[str, Any] = {}

    def _model_for_spec(self, spec: str):
        if spec not in self._cache:
            provider, _ = _split_spec(spec)
            self._cache[spec] = build_chat_model(spec, self._keys.get(provider))
        return self._cache[spec]

    @property
    def default_spec(self) -> str:
        return self._default_spec

    def coordinator(self):
        return self._model_for_spec(self._default_spec)

    def for_subagent(self, name: str, overrides: dict[str, str]):
        spec = overrides.get(name) or self._default_spec
        return self._model_for_spec(spec)


async def _build_model_resolver(
    config: RunnableConfig,
) -> tuple[_ModelResolver, dict[str, str], bool]:
    """Resolve the per-request model routing + keys into a ``_ModelResolver``.

    Returns ``(resolver, subagent_overrides, is_execution)``.
    """
    configurable = config.get("configurable") or {}
    model_config = configurable.get("model_config") or {}
    default_spec = model_config.get("default") or DEFAULT_MODEL_SPEC
    overrides: dict[str, str] = dict(model_config.get("subagents") or {})
    storage_mode = model_config.get("storage_mode")

    # Providers referenced by the coordinator + any subagent override.
    needed_providers = {
        _split_spec(spec)[0]
        for spec in (default_spec, *overrides.values())
        if isinstance(spec, str) and "::" in spec
    }

    keys: dict[str, dict[str, Any]] = {}
    client_keys = configurable.get("__llm_keys") or {}
    # Vault mode (explicit) or unspecified-with-no-inline-keys: read server-side.
    # Read-only graph loads (history / state / schema inspection) build the factory
    # with no auth user in the config; they execute no nodes, so we skip Vault rather
    # than raise — otherwise the missing identity becomes a ValueError that
    # langgraph-api turns into a 400 on /threads/<id>/history (breaking refresh).
    if storage_mode == "vault" or (storage_mode is None and not client_keys):
        try:
            user_id = _user_id_from_config(config)
        except ValueError:
            user_id = None
        if user_id:
            try:
                keys.update(await vault_store.get_keys(user_id, needed_providers))
            except vault_store.VaultUnavailable:
                pass  # surfaced later by _require_model_keys if this is a real run
    # Inline client keys win / fill in (client-only mode).
    if isinstance(client_keys, dict):
        for provider, record in client_keys.items():
            if isinstance(record, dict) and record.get("api_key"):
                keys[provider] = {
                    "api_key": record["api_key"],
                    "base_url": record.get("base_url"),
                }

    is_execution = bool(configurable.get("__is_for_execution__"))
    return _ModelResolver(default_spec, keys), overrides, is_execution


def _require_model_keys(
    resolver: _ModelResolver, overrides: dict[str, str]
) -> None:
    """Block an execution run when any required provider key is missing."""
    specs = {resolver.default_spec, *overrides.values()}
    missing: set[str] = set()
    for spec in specs:
        if not isinstance(spec, str) or "::" not in spec:
            continue
        provider, _ = _split_spec(spec)
        if not resolver._keys.get(provider, {}).get("api_key"):
            missing.add(provider)
    if missing:
        raise ValueError(
            "No API key configured for: "
            + ", ".join(sorted(missing))
            + ". Open Settings → Model & API to connect a provider before running."
        )
