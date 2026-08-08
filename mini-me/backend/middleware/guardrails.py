"""Filesystem-permission, PII-redaction, and call/retry guardrail builders."""

from typing import Any, Sequence

from deepagents.middleware.permissions import FilesystemPermission
from langchain.agents.middleware import (
    ModelCallLimitMiddleware,
    ModelRetryMiddleware,
    PIIMiddleware,
    ToolCallLimitMiddleware,
    ToolRetryMiddleware,
)


def _build_filesystem_permissions() -> list[FilesystemPermission]:
    """Return route-scoped filesystem permissions compatible with CompositeBackend.

    deepagents' built-in permission middleware does not support sandboxes with
    command execution unless every permission path is scoped to routed
    backends. We therefore enforce read-only skills and narrowly writable
    memory paths here, and rely on sandbox isolation plus subagent/tool
    scoping for the execution backend itself.
    """
    return [
        FilesystemPermission(
            operations=["read"],
            paths=["/skills/**"],
            mode="allow",
        ),
        FilesystemPermission(
            operations=["read"],
            paths=["/memories/**"],
            mode="allow",
        ),
        FilesystemPermission(
            operations=["write"],
            paths=["/memories/instructions.txt"],
            mode="allow",
        ),
        FilesystemPermission(
            operations=["write"],
            paths=["/skills/**"],
            mode="deny",
        ),
        FilesystemPermission(
            operations=["write"],
            paths=["/memories/**"],
            mode="deny",
        ),
    ]


def _build_pii_middleware() -> list[Any]:
    """Redact PII from user input, model output, and tool results.

    URLs are intentionally NOT redacted — research workflows depend on paper,
    dataset, and DOI links flowing freely through the conversation.
    """
    return [
        PIIMiddleware(
            "email",
            strategy="redact",
            apply_to_input=True,
            apply_to_output=True,
            apply_to_tool_results=True,
        ),
        PIIMiddleware(
            "credit_card",
            strategy="redact",
            apply_to_input=True,
            apply_to_output=True,
            apply_to_tool_results=True,
        ),
        PIIMiddleware(
            "ip",
            strategy="redact",
            apply_to_input=True,
            apply_to_output=True,
            apply_to_tool_results=True,
        ),
        PIIMiddleware(
            "mac_address",
            strategy="redact",
            apply_to_input=True,
            apply_to_output=True,
            apply_to_tool_results=True,
        ),
    ]


def _build_guardrail_middleware(external_tool_names: Sequence[str]) -> list[Any]:
    middleware: list[Any] = [
        *_build_pii_middleware(),
        ModelCallLimitMiddleware(run_limit=60, exit_behavior="end"),
        ToolCallLimitMiddleware(run_limit=150, exit_behavior="continue"),
        ModelRetryMiddleware(max_retries=3, backoff_factor=2.0, initial_delay=1.0),
    ]
    if external_tool_names:
        middleware.append(
            ToolRetryMiddleware(
                max_retries=2,
                tools=list(external_tool_names),
                retry_on=(Exception,),
                backoff_factor=2.0,
                initial_delay=1.0,
            )
        )
    return middleware
