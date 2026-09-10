"""Hosted MCP servers: connection config, caching, resilience, and loaders.

Mini-Me reaches external knowledge through hosted MCP servers (AGROVOC, Crop
Ontology, Asta, the CIP Dataverse). This module owns their connection configs,
process-wide adapters whose FastMCP caches honor each server's discovery TTL,
the truncation + save-to-sandbox logic that keeps large MCP payloads from
poisoning agent state, and the ``get_*_mcp_tools`` loaders the subagents
consume. Failed discovery is memoized for this process so an unavailable
deployment remains non-fatal without delaying every graph factory. Large
results are written through whatever sandbox is active in the
``_active_sandbox`` ContextVar, so this module has no hard dependency on the
sandbox class.
"""

import asyncio
import contextvars
import json
import logging
import os
from datetime import datetime
from typing import Any, Sequence

from fastmcp import Client
from fastmcp.client.transports import StreamableHttpTransport
from langchain_core._api import suppress_langchain_beta_warning

with suppress_langchain_beta_warning():
    # First-party as of LangChain 1.4. Kept scoped because the namespace is explicitly beta and
    # otherwise emits a process warning on every backend boot even though this version is pinned.
    from langchain.mcp import MCPAdapter

from backend.runtime import (
    _active_sandbox,
    _mcp_clients,
    _mcp_statuses,
    _mcp_tools_cache,
    _mcp_tools_locks,
)


log = logging.getLogger(__name__)


MCP_SERVER_CONFIGS: dict[str, dict[str, Any]] = {
    "agrovoc": {
        "url": "https://agrovoc.fastmcp.app/mcp",
    },
    "crop_ontology": {
        "url": "https://CropOntology.fastmcp.app/mcp",
    },
    "asta": {
        "url": "https://asta-tools.allen.ai/mcp/v1",
        "headers_env": {
            "x-api-key": "ASTA_API_KEY",
        },
    },
    "dataverse": {
        "url": "https://dataverse-cip.fastmcp.app/mcp",
        # Horizon Authentication is on for this deployment, and it must stay on: the server
        # exposes six writing tools, and MCP calls do not pass through the `execute` approval
        # gate. `headers_env` cannot serve here — the deployment offers no `client_credentials`
        # grant, so no static key exists to put in one. See `backend/dataverse_auth.py`.
        "oauth": True,
    },
}

MCP_SERVER_LABELS = {
    "agrovoc": "AGROVOC",
    "crop_ontology": "Crop Ontology",
    "asta": "Asta",
    "dataverse": "CIP Dataverse",
}


def mcp_status_report() -> dict[str, list[dict[str, str]]]:
    """Return public availability state for the desktop client.

    Error details stay in the backend log: upstream exception strings can contain URLs or
    deployment details which do not belong in a user-facing response. ``checking`` is distinct
    from unavailable so asking this route before graph warm-up cannot invent an outage.
    """
    services = []
    for server_id in MCP_SERVER_CONFIGS:
        available = _mcp_statuses.get(server_id)
        status = (
            "checking"
            if available is None
            else "available" if available else "unavailable"
        )
        services.append(
            {
                "id": server_id,
                "name": MCP_SERVER_LABELS.get(server_id, server_id),
                "status": status,
            }
        )
    return {"services": services}


def _mark_mcp_unavailable(server_names: Sequence[str], error: BaseException) -> None:
    for server_name in server_names:
        _mcp_statuses[server_name] = False
    labels = ", ".join(MCP_SERVER_LABELS.get(name, name) for name in server_names)
    log.warning("MCP unavailable; continuing without %s: %s", labels, error)


def _normalize_mcp_server_names(server_names: Sequence[str]) -> tuple[str, ...]:
    names = tuple(sorted(set(server_names)))
    if not names:
        raise ValueError("At least one MCP server name is required")

    unknown = [name for name in names if name not in MCP_SERVER_CONFIGS]
    if unknown:
        raise ValueError(f"Unknown MCP server names: {', '.join(unknown)}")

    return names


def _resolve_mcp_server_config(server_name: str) -> dict[str, Any]:
    config = dict(MCP_SERVER_CONFIGS[server_name])
    header_envs = config.pop("headers_env", None)
    if not header_envs:
        return config

    headers = dict(config.get("headers") or {})
    missing_env_vars: list[str] = []
    for header_name, env_var in header_envs.items():
        value = os.getenv(env_var)
        if not value:
            missing_env_vars.append(env_var)
            continue
        headers[header_name] = value

    if missing_env_vars:
        raise ValueError(
            f"Missing required environment variables for MCP server '{server_name}': "
            f"{', '.join(sorted(missing_env_vars))}"
        )

    config["headers"] = headers
    return config


def _get_or_create_mcp_client(
    server_names: Sequence[str], auth: Any | None = None
) -> MCPAdapter:
    """Build (or reuse) the one adapter for this deployment.

    `auth` is **passed in rather than decided here**. Choosing it means reading the token store,
    and this runs on the event loop during a graph build, where langgraph's `blockbuster` guard
    turns filesystem access into a raised `BlockingError`. Deciding it here is what reported
    Dataverse as unavailable on every launch whether or not the researcher had signed in — the
    reasoning is in `dataverse_auth.auth_for_runtime`.
    """
    bundle = _normalize_mcp_server_names(server_names)
    if len(bundle) != 1:
        raise ValueError(
            "Mini-Me opens one MCP deployment per adapter so one outage cannot affect another"
        )
    if bundle not in _mcp_clients:
        server_name = bundle[0]
        config = _resolve_mcp_server_config(server_name)
        config.pop("oauth", None)
        transport = StreamableHttpTransport(
            config["url"],
            headers=config.get("headers"),
            auth=auth,
        )
        # `mode="auto"` negotiates the modern stateless protocol and falls back for a legacy
        # deployment. The response cache honors the server's discovery TTL; keeping the adapter
        # process-wide means repeated graph factories share that cache without crossing users in
        # today's one-user desktop process. Key this cache by account when WorkOS lands.
        client = Client(
            transport,
            name=MCP_SERVER_LABELS.get(server_name, server_name),
            mode="auto",
            cache=True,
        )
        _mcp_clients[bundle] = MCPAdapter(client)
    return _mcp_clients[bundle]


def _make_mcp_error_handler(tool_name: str):
    """Translate cryptic upstream MCP errors into actionable observations.

    Some hosted MCP servers (e.g. Asta's `search_paper_by_title`) raise
    Python errors like `'NoneType' object is not iterable` when the upstream
    API returns an empty or rate-limited response. Without a custom handler
    those exceptions crash the run; with one, the agent receives a normal
    tool message it can act on (retry with a different query, switch tools).
    """

    def handler(error: Exception) -> str:
        message = str(error) or repr(error)
        lower = message.lower()
        if "nonetype" in lower and "not iterable" in lower:
            return (
                f"Tool '{tool_name}' returned no parseable result from the upstream "
                f"service. This usually means an empty response, an invalid query, "
                f"or a transient rate limit. Try a different query (more specific "
                f"or less specific terms), a different tool from the same server, "
                f"or proceed without this lookup if it is not essential."
            )
        return (
            f"Tool '{tool_name}' failed: {message}. Consider retrying with "
            f"different arguments or proceeding with another approach."
        )

    return handler


MCP_TOOL_OUTPUT_MAX_BYTES = 128_000  # fallback for unknown MCPs

# Per-MCP byte threshold above which results are saved to the sandbox instead
# of passed directly to the model. Asta papers can be hundreds of KB; the
# other MCPs return bounded structured data that rarely approaches these limits.
MCP_SAVE_THRESHOLDS: dict[str, int] = {
    "asta": 32_000,
    "dataverse": 512_000,
    "agrovoc": 512_000,
    "crop_ontology": 512_000,
}


def _trim_json_array_text(text: str, budget: int, tool_name: str) -> str:
    """Keep as many complete items from a JSON array as fit within `budget` bytes.

    Asta's snippet_search returns a single content block whose ``text`` field
    is a JSON object like ``{"data": [...N items...]}`` — one block, many items.
    Dropping the whole block when it exceeds the byte budget gives the agent
    nothing. Instead we parse the array, keep items greedily, and rebuild.
    """
    try:
        obj = json.loads(text)
    except (json.JSONDecodeError, ValueError):
        return text  # not parseable — fall back to caller

    # Find the first top-level list value (handles {"data": [...]} or bare [...])
    if isinstance(obj, list):
        items = obj
        wrap: Any = None
        wrap_key: str | None = None
    elif isinstance(obj, dict):
        items = []
        wrap_key = None
        for k, v in obj.items():
            if isinstance(v, list):
                items = v
                wrap = obj
                wrap_key = k
                break
        else:
            return text  # no list found
    else:
        return text

    kept: list[Any] = []
    for item in items:
        candidate = kept + [item]
        rebuilt = json.dumps(
            {wrap_key: candidate} if wrap_key else candidate,
            indent=2,
        )
        if len(rebuilt.encode("utf-8")) > budget:
            break
        kept = candidate

    dropped = len(items) - len(kept)
    if dropped == 0:
        return text  # nothing was trimmed

    result_obj = {wrap_key: kept} if wrap_key else kept
    suffix = (
        f"\n\n[{dropped} item(s) omitted — output exceeded {budget // 1024} KB. "
        f"Use a lower limit or a more specific query for '{tool_name}'.]"
    )
    return json.dumps(result_obj, indent=2) + suffix


def _truncate_mcp_content_blocks(blocks: list[Any], tool_name: str) -> list[Any]:
    """Cap a list of MCP content blocks, preserving complete JSON items where possible."""
    budget = MCP_TOOL_OUTPUT_MAX_BYTES
    result: list[Any] = []
    total = 0
    for block in blocks:
        if not isinstance(block, dict):
            result.append(block)
            continue
        text = block.get("text", "")
        block_size = len(json.dumps(block).encode("utf-8"))
        if total + block_size <= budget:
            result.append(block)
            total += block_size
        else:
            # Block is too big — try to trim its inner JSON array
            remaining = budget - total
            trimmed_text = _trim_json_array_text(text, max(remaining - 200, 4000), tool_name)
            result.append({**block, "text": trimmed_text})
            break
    return result


def _truncate_str_result(raw: str, tool_name: str) -> str:
    """Byte-cap a plain-string tool result with head+tail elision."""
    encoded = raw.encode("utf-8")
    if len(encoded) <= MCP_TOOL_OUTPUT_MAX_BYTES:
        return raw
    head_bytes = MCP_TOOL_OUTPUT_MAX_BYTES // 2
    tail_bytes = MCP_TOOL_OUTPUT_MAX_BYTES // 4
    head = encoded[:head_bytes].decode("utf-8", errors="ignore")
    tail = encoded[-tail_bytes:].decode("utf-8", errors="ignore")
    dropped_kb = (len(encoded) - head_bytes - tail_bytes) // 1024
    return (
        f"{head}\n\n"
        f"...[output truncated — {dropped_kb} KB elided; "
        f"use a more specific query for '{tool_name}']...\n\n"
        f"{tail}"
    )


def _truncate_tool_result_any(result: Any, tool_name: str) -> Any:
    """Truncate MCP tool output regardless of return type.

    Asta tools return ``(list_of_content_blocks, None)`` — a tuple, not a list.
    We handle both list and tuple, and only cap the blocks portion.
    """
    if isinstance(result, str):
        return _truncate_str_result(result, tool_name)
    if isinstance(result, (list, tuple)):
        # Unwrap tuple: (blocks, metadata) → work on blocks
        if isinstance(result, tuple):
            blocks = result[0] if result else []
            rest = result[1:]
        else:
            blocks = result
            rest = None
        if not isinstance(blocks, list):
            return result
        total = sum(len(json.dumps(item).encode("utf-8")) for item in blocks)
        if total > MCP_TOOL_OUTPUT_MAX_BYTES:
            capped_blocks = _truncate_mcp_content_blocks(blocks, tool_name)
            return (capped_blocks, *rest) if rest is not None else capped_blocks
    return result


#: The last answer that was too big to hand the model whole, before it was cut down.
#:
#: **The model's budget is not the researcher's.** Everything below this line — the 128 KB cap,
#: the trimmed array, the pointer — exists so an answer fits in a context window, and all of it is
#: right for that purpose. None of it is right for the copy kept in the conversation folder, which
#: a person opens and which the datasets panel renders: a search returning a hundred datasets
#: should file a hundred, whatever the model was shown (§294).
#:
#: A `ContextVar` rather than a return value because the capping happens inside the tool's own
#: coroutine, several frames below the middleware that wants it, through code this file does not
#: own. Per-context, so two concurrent tool calls cannot read each other's — the same argument
#: `minime_local.spine` makes for the same reason.
#:
#: Set **only** when the answer was actually too big. Under the cap the model gets the whole thing
#: and the consumer can read it from the result, so serialising a copy of every small answer would
#: be pure cost.
_full_answer: contextvars.ContextVar[tuple[str, str] | None] = contextvars.ContextVar(
    "minime_full_mcp_answer", default=None
)


def last_full_answer(tool_name: str) -> str | None:
    """The untruncated text of the most recent oversized call to `tool_name`, if there was one.

    `None` when the last big answer came from a different tool, or when nothing was capped — both
    of which mean *read the result you were given*, which is already whole.
    """
    held = _full_answer.get()
    if held and held[0] == tool_name:
        return held[1]
    return None


def _mcp_save_threshold(tool_name: str) -> int:
    """Return the byte threshold for `tool_name` above which we save to disk."""
    for prefix, limit in MCP_SAVE_THRESHOLDS.items():
        if tool_name.lower().startswith(prefix):
            return limit
    return MCP_TOOL_OUTPUT_MAX_BYTES


def _mcp_result_bytes(result: Any) -> int:
    """Estimate byte size of an MCP tool result without full serialisation."""
    if isinstance(result, str):
        return len(result.encode("utf-8"))
    if isinstance(result, (list, tuple)):
        blocks = result[0] if isinstance(result, tuple) else result
        if isinstance(blocks, list):
            return sum(len(json.dumps(b).encode("utf-8")) for b in blocks)
    return len(json.dumps(result).encode("utf-8"))


def _mcp_result_to_text(result: Any) -> str:
    """Serialise an MCP result to a human-readable UTF-8 string for disk storage."""
    if isinstance(result, str):
        return result
    if isinstance(result, tuple):
        blocks = result[0] if result else []
    elif isinstance(result, list):
        blocks = result
    else:
        return json.dumps(result, indent=2)
    if not isinstance(blocks, list):
        return json.dumps(result, indent=2)
    parts: list[str] = []
    for block in blocks:
        if isinstance(block, dict):
            text = block.get("text", "")
            if text:
                parts.append(text)
            else:
                parts.append(json.dumps(block, indent=2))
        else:
            parts.append(str(block))
    return "\n---\n".join(parts)


async def _save_mcp_to_sandbox(
    sandbox: Any, result: Any, tool_name: str
) -> tuple[str, dict[str, Any] | None]:
    """Write a large MCP result to the sandbox and return a pointer message.

    Returns a ``(content, artifact)`` 2-tuple to satisfy the MCP tool wrapper's
    ``response_format="content_and_artifact"`` contract. The artifact carries
    the saved path + size so callers (and LangSmith traces) can find the file.
    """
    text = _mcp_result_to_text(result)
    ts = datetime.now().strftime("%Y%m%d_%H%M%S")
    path = f"/workspace/mcp_results/{tool_name}_{ts}.txt"
    try:
        await sandbox.awrite(path, text)
    except Exception:  # noqa: BLE001
        # Write failed — fall back to inline truncation, preserving shape.
        return _ensure_tuple(_truncate_tool_result_any(result, tool_name))
    size_bytes = len(text.encode("utf-8"))
    size_kb = size_bytes // 1024
    preview = text[:2048]
    pointer = (
        f"Full result ({size_kb} KB) saved to `{path}`.\n"
        f"Use code execution to read specific sections, e.g.:\n"
        f"  with open('{path}') as f: print(f.read())\n\n"
        f"Preview (first 2 KB):\n---\n{preview}"
    )
    return pointer, {"saved_path": path, "size_bytes": size_bytes}


def _ensure_tuple(result: Any) -> Any:
    """Coerce a value into ``(content, artifact)`` shape if it isn't already.

    MCP tools wrapped by ``langchain.mcp`` declare
    ``response_format="content_and_artifact"``; their coroutines must return a
    2-tuple. When our truncation helpers return a bare string, wrap it so the
    contract is preserved (warning suppressed in LangChain's tool layer).
    """
    if isinstance(result, tuple) and len(result) == 2:
        return result
    if isinstance(result, str):
        return result, None
    if isinstance(result, list):
        return result, None
    return result


def _make_mcp_tools_resilient(tools: list[Any]) -> list[Any]:
    """Attach error handler and output-size cap to each MCP tool.

    MCP tools are Pydantic models — arbitrary instance attributes (like
    `ainvoke`) cannot be set on them; the assignment silently fails via
    Pydantic's __setattr__. The correct injection point is `tool.coroutine`,
    which IS a declared Pydantic field on StructuredTool and can be
    reassigned. `coroutine` is what `arun` / `_arun` calls directly.
    """
    for tool in tools:
        name = getattr(tool, "name", "<unknown>")
        # `langchain.mcp` already preserves an MCP `isError` result as a failed ToolMessage with
        # the server's content blocks. Keep that richer handler; the fallback only applies to a
        # non-standard tool that arrived without one.
        if not getattr(tool, "handle_tool_error", None):
            try:
                tool.handle_tool_error = _make_mcp_error_handler(name)
            except Exception:  # noqa: BLE001
                pass
        original_coro = getattr(tool, "coroutine", None)
        if asyncio.iscoroutinefunction(original_coro) and not getattr(
            original_coro, "_minime_mcp_capped", False
        ):
            async def _capped(
                *args: Any,
                _orig: Any = original_coro,
                _name: str = name,
                **kwargs: Any,
            ) -> Any:
                result = await _orig(*args, **kwargs)
                threshold = _mcp_save_threshold(_name)
                size = _mcp_result_bytes(result)
                # Kept whole for whoever files it, before anything below cuts it down for the
                # model. Bookkeeping must never cost a tool call, so a failure here is silent by
                # design — the consumer falls back to the capped result, which is today's
                # behaviour (§294).
                if size > MCP_TOOL_OUTPUT_MAX_BYTES:
                    try:
                        _full_answer.set((_name, _mcp_result_to_text(result)))
                    except Exception:  # noqa: BLE001
                        pass
                if size <= threshold:
                    return result
                sandbox = _active_sandbox.get()
                if sandbox is not None:
                    return await _save_mcp_to_sandbox(sandbox, result, _name)
                # No sandbox — fall back to inline truncation, preserving the
                # (content, artifact) 2-tuple shape expected by the MCP wrapper.
                return _ensure_tuple(_truncate_tool_result_any(result, _name))
            try:
                # FastMCP may return the same LangChain tool object while its discovery response is
                # cached. Mark the wrapper itself so graph reconstruction cannot stack another copy.
                setattr(_capped, "_minime_mcp_capped", True)
                tool.coroutine = _capped
            except Exception:  # noqa: BLE001
                pass
    return tools


_sign_in_marks: dict[tuple[str, ...], str] = {}


async def _sign_in_moved(bundle: tuple[str, ...]) -> bool:
    """Whether the stored sign-in changed since this bundle's failure was cached.

    Only asked of deployments that have a sign-in at all: for a public server the answer is
    always no, and asking would put a `stat` on every discovery for nothing.

    **Compares only.** The mark is taken where the failure is recorded, not here — taking it on
    the read path meant the first comparison had nothing to compare against, read as a change,
    and retried an unreachable deployment on every single request. That is the behaviour the
    cache exists to prevent, and it passed the "signing in works" assertion while doing it.
    """
    if not MCP_SERVER_CONFIGS[bundle[0]].get("oauth"):
        return False
    from backend.dataverse_auth import sign_in_changed

    changed, _current = await sign_in_changed(_sign_in_marks.get(bundle))
    return changed


async def _remember_sign_in(bundle: tuple[str, ...]) -> None:
    """Record which sign-in was in force when a failure was cached."""
    if not MCP_SERVER_CONFIGS[bundle[0]].get("oauth"):
        return
    from backend.dataverse_auth import sign_in_changed

    _, current = await sign_in_changed(None)
    _sign_in_marks[bundle] = current


async def get_mcp_tools(server_names: Sequence[str]) -> list[Any]:
    bundle = _normalize_mcp_server_names(server_names)
    # Only failures are held here. Successful discovery goes through FastMCP's TTL-aware response
    # cache so a server can change its catalog without requiring an app restart.
    if _mcp_tools_cache.get(bundle) == []:
        # **Unless the researcher has signed in since.** The sign-in happens in another process,
        # so it cannot clear this cache; without the check, a correct sign-in kept reporting the
        # service as unavailable until the whole backend was restarted.
        if not await _sign_in_moved(bundle):
            return _mcp_tools_cache[bundle]
        _mcp_tools_cache.pop(bundle, None)

    if bundle not in _mcp_tools_locks:
        _mcp_tools_locks[bundle] = asyncio.Lock()

    async with _mcp_tools_locks[bundle]:
        if _mcp_tools_cache.get(bundle) == []:
            return _mcp_tools_cache[bundle]

        try:
            # **Never interactive.** Resolved before the client is built, on a thread, so a
            # researcher who has not signed in fails in milliseconds and is reported as an
            # unavailable service rather than blocked behind a browser prompt nobody can see.
            # Setup's Sign in button is the only caller allowed to start a login.
            auth = None
            if MCP_SERVER_CONFIGS[bundle[0]].get("oauth"):
                from backend.dataverse_auth import auth_for_runtime

                auth = await auth_for_runtime()
            adapter = _get_or_create_mcp_client(bundle, auth)
            loaded = await adapter.list_tools(cache_mode="use")
            if not loaded:
                raise ValueError("MCP server returned no tools")
            resilient = _make_mcp_tools_resilient(loaded)
        except Exception as error:  # noqa: BLE001 — a hosted service is optional
            # Cache the empty result for this process. Without it every read-only thread-state
            # request would immediately hammer the same unavailable deployment again. A restart
            # performs a fresh handshake; the future account flow can invalidate this cache after
            # a successful sign-in.
            _mark_mcp_unavailable(bundle, error)
            _mcp_tools_cache[bundle] = []
            await _remember_sign_in(bundle)
            # The adapter goes with it. One built while signed out holds `auth=None` for the
            # life of the process, so keeping it would make the retry above discover nothing
            # even once the sign-in is valid.
            _mcp_clients.pop(bundle, None)
            return _mcp_tools_cache[bundle]
        _mcp_tools_cache.pop(bundle, None)
        for server_name in bundle:
            _mcp_statuses[server_name] = True
        return resilient


async def get_data_cleaning_mcp_tools() -> list[Any]:
    # One deployment must not take its healthy neighbour down with it. Separate adapters also let
    # FastMCP negotiate the best protocol era and credentials independently for each service.
    catalogs = await asyncio.gather(
        *(get_mcp_tools((server_name,)) for server_name in ("agrovoc", "crop_ontology"))
    )
    return [tool for catalog in catalogs for tool in catalog]


async def get_academic_research_mcp_tools() -> list[Any]:
    return await get_mcp_tools(("asta",))


async def get_dataverse_search_mcp_tools() -> list[Any]:
    allowed_names = {
        "SearchCIPDataverse",
        "read_search_results",
        "list_dataset_files",
    }
    tools = await get_mcp_tools(("dataverse",))
    if not tools and _mcp_statuses.get("dataverse") is False:
        return []
    selected = [tool for tool in tools if getattr(tool, "name", None) in allowed_names]
    missing = sorted(allowed_names - {getattr(tool, "name", None) for tool in selected})
    if missing:
        error = ValueError(
            "Dataverse MCP is missing required tools: " + ", ".join(missing)
        )
        _mark_mcp_unavailable(("dataverse",), error)
        _mcp_tools_cache[("dataverse",)] = []
        return []
    return selected


def _tool_names(tools: Sequence[Any]) -> list[str]:
    names: list[str] = []
    for tool in tools:
        name = getattr(tool, "name", None)
        if isinstance(name, str) and name and name not in names:
            names.append(name)
    return names
