"""Make the data-cleaning agent validate with Pointblank before it is allowed to answer.

# Why this is not a `ToolsBeforeAnswering`

`middleware/tool_gate.py` closes the same kind of exit by forcing `tool_choice` onto the next model
call. That cannot be used here:

- Some providers refuse a forced tool choice outright. Claude 5 models behind OpenRouter answer
  HTTP 400 *"tool_choice: type "tool" and "any" are not supported for this model"*, and the run
  fails. A gate that crashes the models people use is worse than no gate.
- `data_cleaning` has no `response_format`, so there is no structured-answer tool to withhold.
  Its exit is simply a reply with no tool calls.

So the check runs **after** the model: a reply that would end the run without a `validate_dataset`
result is sent back with a reminder, through `jump_to: "model"`. Nothing about the request changes,
which is why it works on every provider.

# Why this cannot spin

The gate opens on a `validate_dataset` result *existing*, not on it succeeding, so a dataset that
cannot be read costs a finding rather than a turn. And it reminds at most `MAX_REMINDERS` times;
after that the run ends with whatever the agent wrote, and the log says it never validated.
"""

from __future__ import annotations

import logging
from typing import Any

from langchain.agents.middleware import AgentMiddleware, hook_config
from langchain_core.messages import HumanMessage

logger = logging.getLogger(__name__)

#: The tool whose result must exist before the agent may answer.
VALIDATE_TOOL = "validate_dataset"

#: Reminders sent before the run is allowed to end without validating.
MAX_REMINDERS = 2

#: Starts every reminder, so they can be counted from the message list alone.
REMINDER_MARKER = "[validation required]"

REMINDER = (
    f"{REMINDER_MARKER} Before answering, run {VALIDATE_TOOL} on the dataset(s) and include every "
    "check it returns in a table, then name its html_report. If it returns an error, say that "
    "validation could not run and why."
)


def _has_validated(messages: list[Any]) -> bool:
    return any(
        getattr(message, "type", None) == "tool" and getattr(message, "name", None) == VALIDATE_TOOL
        for message in messages or []
    )


def _reminders_sent(messages: list[Any]) -> int:
    return sum(
        1
        for message in messages or []
        if getattr(message, "type", None) == "human"
        and isinstance(getattr(message, "content", None), str)
        and message.content.startswith(REMINDER_MARKER)
    )


def _is_final_reply(message: Any) -> bool:
    """Whether `message` would end the run: a model reply that calls no tool."""
    return getattr(message, "type", None) == "ai" and not getattr(message, "tool_calls", None)


class ValidateBeforeAnswering(AgentMiddleware):
    """Send a final reply back to the model until `validate_dataset` has returned.

    Never sets `tool_choice` or `response_format`; see the module docstring for why.
    """

    def _check(self, state: Any) -> dict[str, Any] | None:
        messages = (state or {}).get("messages", [])
        if not messages or not _is_final_reply(messages[-1]) or _has_validated(messages):
            return None
        sent = _reminders_sent(messages)
        if sent >= MAX_REMINDERS:
            logger.warning("data_cleaning finished without validating")
            return None
        logger.info("data_cleaning has not validated yet — reminding (%d/%d)", sent + 1, MAX_REMINDERS)
        return {"messages": [HumanMessage(content=REMINDER)], "jump_to": "model"}

    @hook_config(can_jump_to=["model"])
    def after_model(self, state: Any, runtime: Any) -> dict[str, Any] | None:
        return self._check(state)

    @hook_config(can_jump_to=["model"])
    async def aafter_model(self, state: Any, runtime: Any) -> dict[str, Any] | None:
        # Defined explicitly: the server runs the graph on the async path.
        return self._check(state)
