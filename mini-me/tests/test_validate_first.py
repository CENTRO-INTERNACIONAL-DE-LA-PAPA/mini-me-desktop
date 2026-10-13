"""The data-cleaning agent must validate with Pointblank before it can answer.

The gate works after the model call and never through `tool_choice`, because Claude 5 models
behind OpenRouter reject a forced tool choice with HTTP 400. These pin both halves: what it does
to a reply that would end the run, and that it has no way of touching the request.
"""

from __future__ import annotations

import asyncio

from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.validate_first import (
    MAX_REMINDERS,
    REMINDER_MARKER,
    VALIDATE_TOOL,
    ValidateBeforeAnswering,
)


def _state(*messages):
    return {"messages": [HumanMessage(content="Clean data.csv"), *messages]}


def _reminder():
    return HumanMessage(content=f"{REMINDER_MARKER} run it")


def _validated(content='{"checks": []}'):
    return ToolMessage(content=content, name=VALIDATE_TOOL, tool_call_id="call-1")


def test_an_answer_without_validating_is_sent_back():
    result = ValidateBeforeAnswering().after_model(_state(AIMessage(content="All clean.")), None)

    assert result is not None
    assert result["jump_to"] == "model"
    (reminder,) = result["messages"]
    assert reminder.content.startswith(REMINDER_MARKER)
    assert VALIDATE_TOOL in reminder.content


def test_an_answer_after_validating_is_let_through():
    state = _state(_validated(), AIMessage(content="Two checks failed."))
    assert ValidateBeforeAnswering().after_model(state, None) is None


def test_a_validation_that_failed_still_opens_the_gate():
    """A file that cannot be read must cost a finding, never the turn."""
    state = _state(_validated('{"error": "unreadable", "checks": []}'), AIMessage(content="Could not run."))
    assert ValidateBeforeAnswering().after_model(state, None) is None


def test_a_reply_that_calls_a_tool_is_not_an_answer():
    reply = AIMessage(content="", tool_calls=[{"name": "profile_dataset", "args": {"path": "d.csv"}, "id": "c"}])
    assert ValidateBeforeAnswering().after_model(_state(reply), None) is None


def test_reminders_stop_at_the_cap():
    reminders = [message for _ in range(MAX_REMINDERS) for message in (_reminder(), AIMessage(content="no"))]
    state = _state(AIMessage(content="no"), *reminders)
    assert ValidateBeforeAnswering().after_model(state, None) is None


def test_the_async_hook_behaves_the_same():
    result = asyncio.run(ValidateBeforeAnswering().aafter_model(_state(AIMessage(content="Done.")), None))
    assert result is not None and result["jump_to"] == "model"


def test_the_gate_never_touches_the_model_request():
    """No `wrap_model_call` means no way to set `tool_choice` or `response_format`."""
    assert "wrap_model_call" not in vars(ValidateBeforeAnswering)
    assert "awrap_model_call" not in vars(ValidateBeforeAnswering)
