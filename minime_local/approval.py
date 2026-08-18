"""Human approval for the `execute` tool.

Host execution means model-written shell commands run on the researcher's own
machine, against their own files. The file tools have a guardrail — writes outside the
workspace are re-rooted (`workspace.py`) — but `execute` has none, and deepagents is
explicit that nothing constrains it. CIP policy is human-gated, and deepagents
recommends HITL for exactly this backend.

So every `execute` call stops and waits for the person. This is what makes host
execution safe enough to be the default (desktop plan §19).

Mini-Me already uses this mechanism — `diagnostic_analytics` interrupts on
`request_diagnostic_context` — so the shape is upstream's, not something invented here:
``create_deep_agent(interrupt_on={tool: {"allowed_decisions": [...]}})`` and, for
subagents, the same key on each subagent dict.
"""

from __future__ import annotations

import logging
import os

#: The tool that runs shell commands on this machine.
EXECUTE_TOOL = "execute"

#: Set to `0` to run host execution unattended. Off-label: the whole reason host
#: execution is allowed to be the default is that this is on.
APPROVAL_ENV = "MINIME_APPROVE_EXECUTE"

log = logging.getLogger("minime_local")


def approval_requested() -> bool:
    return os.getenv(APPROVAL_ENV, "1").strip().lower() not in {"0", "false", "no"}


def _execute_gate() -> dict:
    return {
        EXECUTE_TOOL: {
            # `edit` and `respond` are deliberately not offered yet: the desktop client
            # can approve or reject, and offering a decision the UI cannot produce
            # would just strand the run.
            "allowed_decisions": ["approve", "reject"],
            "description": (
                "This command will run on your computer, with your permissions, in the "
                "thread's workspace. Review it before approving."
            ),
        }
    }


def _merge_gate(existing) -> dict:
    """Add the `execute` gate without disturbing gates upstream already set."""
    merged = dict(existing or {})
    # Never override an upstream entry: if Mini-Me ever gates `execute` itself, its
    # configuration is the more specific one and should win.
    for tool, config in _execute_gate().items():
        merged.setdefault(tool, config)
    return merged


def _gate_subagents(subagents):
    """Return `subagents` with every dict-shaped entry gated.

    Most execution happens *inside* subagents (data cleaning, EDA, predictive
    modelling), so gating only the coordinator would leave the majority of commands
    unreviewed. Non-dict entries — `CompiledSubAgent`, `AsyncSubAgent` — are passed
    through untouched: they carry their own compiled graph and there is no dict key to
    add here.
    """
    if not subagents:
        return subagents
    gated = []
    for subagent in subagents:
        if isinstance(subagent, dict):
            subagent = {**subagent, "interrupt_on": _merge_gate(subagent.get("interrupt_on"))}
        gated.append(subagent)
    return gated


def install(deepagents_module) -> None:
    """Wrap ``deepagents.create_deep_agent`` so every agent gates `execute`.

    One patch point covers both levels, because the coordinator's own gate and the
    subagents' gates are both arguments to that single call.

    Wrapped on the ``deepagents`` package rather than on ``backend.agent``: LangGraph
    loads the graph module from a file path, so that module never passes through the
    import hook. ``backend/agent.py`` does ``from deepagents import create_deep_agent``,
    so patching the package attribute before that import is what takes effect.
    """
    if not approval_requested():
        log.warning(
            "minime_local: execute approval is OFF (%s) — commands run unreviewed",
            APPROVAL_ENV,
        )
        return

    original = getattr(deepagents_module, "create_deep_agent", None)
    if original is None:
        raise RuntimeError(
            "minime_local: deepagents has no create_deep_agent to wrap — the pinned "
            "deepagents version has moved and overlay/minime_local needs updating "
            "(desktop plan §19)."
        )

    def create_deep_agent_with_approval(*args, **kwargs):
        kwargs["interrupt_on"] = _merge_gate(kwargs.get("interrupt_on"))
        if "subagents" in kwargs:
            kwargs["subagents"] = _gate_subagents(kwargs["subagents"])
        return original(*args, **kwargs)

    deepagents_module.create_deep_agent = create_deep_agent_with_approval
    log.warning("minime_local: every `execute` call will wait for your approval")
