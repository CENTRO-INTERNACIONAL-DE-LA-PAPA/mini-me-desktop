"""The data-cleaning agent gets a nudge after SOFT_CAP commands and a refusal after HARD_CAP.

The refusal is a tool result, not a stopped run: the agent must still be able to validate and
answer, which is why other tools pass straight through at any count.
"""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

from langchain_core.messages import HumanMessage, ToolMessage

from backend.middleware.command_budget import (
    EXECUTE_TOOL,
    HARD_CAP,
    SOFT_CAP,
    CommandBudget,
    commands_run,
)


def _ran(count: int) -> list:
    return [
        ToolMessage(content="ok", name=EXECUTE_TOOL, tool_call_id=f"done-{i}") for i in range(count)
    ]


def _request(already_ran: int, tool: str = EXECUTE_TOOL):
    return SimpleNamespace(
        tool_call={"name": tool, "args": {"command": "python cleaning/02_clean.py"}, "id": "call-x"},
        state={"messages": [HumanMessage(content="Clean data.csv"), *_ran(already_ran)]},
    )


def _shell(request):
    call = request.tool_call
    return ToolMessage(content="exit 0", name=call["name"], tool_call_id=call["id"])


def _calls_shell():
    calls = []

    def handler(request):
        calls.append(request)
        return _shell(request)

    return calls, handler


def test_commands_are_counted_from_execute_results_only():
    messages = [*_ran(3), ToolMessage(content="{}", name="validate_dataset", tool_call_id="v")]
    assert commands_run(messages) == 3


def test_early_commands_pass_through_untouched():
    calls, handler = _calls_shell()
    result = CommandBudget().wrap_tool_call(_request(SOFT_CAP - 2), handler)
    assert len(calls) == 1
    assert result.content == "exit 0"


def test_from_the_soft_cap_results_ask_the_agent_to_wrap_up():
    calls, handler = _calls_shell()
    result = CommandBudget().wrap_tool_call(_request(SOFT_CAP - 1), handler)
    assert len(calls) == 1
    assert result.content.startswith("exit 0")
    assert "[command budget]" in result.content
    assert f"command {SOFT_CAP} of at most {HARD_CAP}" in result.content


def test_the_last_allowed_command_still_runs():
    calls, handler = _calls_shell()
    CommandBudget().wrap_tool_call(_request(HARD_CAP - 1), handler)
    assert len(calls) == 1


def test_past_the_hard_cap_the_shell_is_never_reached():
    calls, handler = _calls_shell()
    result = CommandBudget().wrap_tool_call(_request(HARD_CAP), handler)
    assert calls == []
    assert result.status == "error"
    assert result.tool_call_id == "call-x"
    assert "validate_dataset" in result.content


def test_other_tools_pass_through_at_any_count():
    calls, handler = _calls_shell()
    result = CommandBudget().wrap_tool_call(_request(HARD_CAP + 5, tool="validate_dataset"), handler)
    assert len(calls) == 1
    assert result.content == "exit 0"


def test_the_async_hook_behaves_the_same():
    async def handler(request):
        return _shell(request)

    budget = CommandBudget()
    refused = asyncio.run(budget.awrap_tool_call(_request(HARD_CAP), handler))
    nudged = asyncio.run(budget.awrap_tool_call(_request(SOFT_CAP - 1), handler))
    assert refused.status == "error"
    assert "[command budget]" in nudged.content
