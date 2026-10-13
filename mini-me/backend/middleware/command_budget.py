"""Cap how many shell commands the data-cleaning agent runs in one task.

# Why

A test run issued about 56 one-line pandas commands in eight minutes, and every `execute` can need
the researcher's approval. The prompt now asks for whole scripts and about five commands, but a
prompt is advice; this is the backstop.

# What it does

- From command `SOFT_CAP` on, each `execute` result ends with a note saying how many commands have
  run and asking the agent to validate the cleaned file and answer.
- Once `HARD_CAP` commands have run, further `execute` calls are refused with an error result
  instead of reaching the shell. The run is not stopped: `profile_dataset`, `validate_dataset` and
  the answer stay available, so the agent can still finish with what it has.

Counted from the agent's own message list, so nothing is stored between runs and a new task starts
from zero.
"""

from __future__ import annotations

import logging
from typing import Any, Awaitable, Callable

from langchain.agents.middleware import AgentMiddleware
from langchain_core.messages import ToolMessage

logger = logging.getLogger(__name__)

#: The shell tool whose calls are counted.
EXECUTE_TOOL = "execute"

#: From this command on, results carry a note asking the agent to wrap up.
SOFT_CAP = 5

#: Commands allowed in one task; later `execute` calls are refused.
HARD_CAP = 10


def commands_run(messages: list[Any]) -> int:
    """How many `execute` calls have returned a result in this run."""
    return sum(
        1
        for message in messages or []
        if getattr(message, "type", None) == "tool" and getattr(message, "name", None) == EXECUTE_TOOL
    )


def _wrap_up_note(number: int) -> str:
    return (
        f"\n\n[command budget] That was command {number} of at most {HARD_CAP}. Stop investigating: "
        "run validate_dataset on the cleaned file and answer with what you have."
    )


def _refusal(call: dict[str, Any]) -> ToolMessage:
    return ToolMessage(
        content=(
            f"Refused: this task has already run {HARD_CAP} commands, the most allowed. Do not run "
            "more commands. Validate the cleaned file with validate_dataset if you have not, then "
            "answer with what you have and list what is left to do."
        ),
        tool_call_id=call.get("id", ""),
        name=call.get("name", EXECUTE_TOOL),
        status="error",
    )


def _with_note(result: Any, number: int) -> Any:
    """Append the wrap-up note to a plain-text tool result; anything else is returned as is."""
    if isinstance(result, ToolMessage) and isinstance(result.content, str):
        return result.model_copy(update={"content": result.content + _wrap_up_note(number)})
    return result


class CommandBudget(AgentMiddleware):
    """Nudge after `SOFT_CAP` shell commands and refuse after `HARD_CAP`."""

    name = "CommandBudget"

    @staticmethod
    def _number(request: Any) -> int | None:
        """This call's position among the run's commands, or None if it is not a command."""
        call = getattr(request, "tool_call", None) or {}
        if call.get("name") != EXECUTE_TOOL:
            return None
        state = getattr(request, "state", None) or {}
        return commands_run(state.get("messages", [])) + 1

    def wrap_tool_call(self, request: Any, handler: Callable[[Any], Any]) -> Any:
        number = self._number(request)
        if number is None:
            return handler(request)
        if number > HARD_CAP:
            logger.warning("data_cleaning hit the command budget (%d)", HARD_CAP)
            return _refusal(request.tool_call)
        result = handler(request)
        return _with_note(result, number) if number >= SOFT_CAP else result

    async def awrap_tool_call(
        self, request: Any, handler: Callable[[Any], Awaitable[Any]]
    ) -> Any:
        number = self._number(request)
        if number is None:
            return await handler(request)
        if number > HARD_CAP:
            logger.warning("data_cleaning hit the command budget (%d)", HARD_CAP)
            return _refusal(request.tool_call)
        result = await handler(request)
        return _with_note(result, number) if number >= SOFT_CAP else result
