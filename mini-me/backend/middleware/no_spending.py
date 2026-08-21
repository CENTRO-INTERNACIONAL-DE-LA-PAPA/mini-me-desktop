"""Refuse, in middleware, the shell commands that would spend a researcher's credits.

# Why this file exists

`autodiscovery_tools.py` claimed there was "no code path from a model decision to a spent credit".
**That claim was false**, and a review found it: the tool surface has no submit, but `execute` is a
general shell that every agent and subagent keeps on purpose (`sandbox.py`), and `ASTA_TOKEN` is
injected into every command it runs. So

    asta autodiscovery submit <run-id> -y

is a shell command like any other. With `approve_execute = false` — a supported automation setting —
it runs with no human press at all, and 500 credits is one typo away. The same applies to reaching
the app's own submit route with `curl`, and to `asta autodiscovery fork`, which copies a run and can
then be submitted.

A prompt saying "do not do this" is not a guard. This is.

# What it does not claim

**This closes the accidental path, not an adversarial one.** A model determined to get around a
substring check has options — a shell variable, a different quoting, a Python one-liner building the
argv. The threat being defended against is a confused or over-helpful model taking an instruction
literally, which is what actually happens; and the residual is stated in `docs/desktop-app-plan.md`
§252 rather than papered over. The route-side nonce is the other half: even a `curl` that gets past
here has no approval token to present.
"""

from __future__ import annotations

import re
from typing import Any, Awaitable, Callable

from langchain_core.messages import ToolMessage

from backend import diagnostics

logger = diagnostics.arriving(__name__)

#: The shell tool every agent keeps, and the one this middleware watches.
EXECUTE_TOOL = "execute"

#: Command shapes that spend credits or start a run, matched on the *normalised* command.
#:
#: Whitespace is collapsed first, so `submit   -y` and a line break in the middle both match.
#: `fork` is here because a forked run is submittable and copies the parent's budget, so it is the
#: same decision one step removed.
_SPENDS = (
    re.compile(r"\basta\b[^|;&]*\bautodiscovery\b[^|;&]*\b(submit|fork)\b"),
    # The app's own gate, reached over HTTP instead of through the CLI.
    re.compile(r"/discovery/[^\s\"']+/submit"),
)

#: What the model is told instead. Names the reason and the way it *is* allowed to happen, because a
#: refusal that does not say what to do next gets retried.
REFUSAL = (
    "Refused: that command would spend the researcher's AutoDiscovery credits, and only they can "
    "authorise that. Each experiment costs one credit from a fixed grant. Draft the run with "
    "`draft_discovery_run` and report the run id — the researcher approves the budget in the app, "
    "which is the only thing that may start it. Do not try another way to run this."
)


def _normalise(command: str) -> str:
    """Collapse whitespace and lower-case, so a match is not defeated by formatting alone."""
    return re.sub(r"\s+", " ", (command or "")).strip().lower()


def spends_credits(command: str) -> bool:
    """Whether this shell command would start or pay for an AutoDiscovery run."""
    normalised = _normalise(command)
    return any(pattern.search(normalised) for pattern in _SPENDS)


def _command_of(request: Any) -> str:
    """The command string out of a tool call, whatever the tool named its argument.

    `execute` takes `command`; sibling shells have used `cmd` and `script`. Reading every plausible
    key and joining them means a rename upstream degrades to *still checking*, rather than to
    silently checking nothing — which is the failure mode that matters here.
    """
    args = (getattr(request, "tool_call", None) or {}).get("args") or {}
    if not isinstance(args, dict):
        return str(args)
    parts = [
        str(value)
        for key, value in args.items()
        if key in ("command", "cmd", "script", "code", "input") and value
    ]
    return " ; ".join(parts) if parts else ""


def _refuse(request: Any) -> ToolMessage:
    call = getattr(request, "tool_call", None) or {}
    logger.warning(
        "refused a shell command that would spend discovery credits: %.200s",
        _command_of(request),
    )
    return ToolMessage(
        content=REFUSAL,
        tool_call_id=call.get("id", ""),
        name=call.get("name", EXECUTE_TOOL),
        status="error",
    )


class NoSpendingWithoutApproval:
    """Block `execute` commands that would spend credits, before they reach the shell.

    Attached to the coordinator *and* every subagent, because either can call `execute` and the
    credits are the same credits. Middleware rather than a prompt, because a prompt is advice.
    """

    # Duck-typed to match this project's other middleware; `AgentMiddleware` is not subclassed so
    # the class stays importable and testable without a graph.
    name = "NoSpendingWithoutApproval"

    def wrap_tool_call(self, request: Any, handler: Callable[[Any], Any]) -> Any:
        if self._blocked(request):
            return _refuse(request)
        return handler(request)

    async def awrap_tool_call(
        self, request: Any, handler: Callable[[Any], Awaitable[Any]]
    ) -> Any:
        if self._blocked(request):
            return _refuse(request)
        return await handler(request)

    @staticmethod
    def _blocked(request: Any) -> bool:
        name = (getattr(request, "tool_call", None) or {}).get("name")
        # Checked for every tool, not only `execute`. A second shell arriving under another name is
        # exactly the kind of change that would quietly reopen this.
        return spends_credits(_command_of(request))
