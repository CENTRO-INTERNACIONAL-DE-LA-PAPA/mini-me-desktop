"""You cannot report a run you did not start.

Driven against real LangChain objects for §221's reason: a double more permissive than production
certifies the bug it was meant to catch.
"""

from __future__ import annotations

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.submit_first import (
    DraftBeforeReporting,
    THEORIZE_TOOL,
    TheorizeBeforeReporting,
)

CASES = [
    (TheorizeBeforeReporting, THEORIZE_TOOL, "HypothesisOutput"),
]


def _request(messages, response_format):
    class Request:
        def __init__(self, state, response_format, tool_choice=None):
            self.state = state
            self.response_format = response_format
            self.tool_choice = tool_choice

        def override(self, **changes):
            return Request(
                self.state,
                changes.get("response_format", self.response_format),
                changes.get("tool_choice", self.tool_choice),
            )

    return Request({"messages": messages}, response_format)


def _seen(gate, messages, response_format):
    out = {}
    gate().wrap_model_call(
        _request(messages, response_format),
        lambda request: out.update(
            format=request.response_format, choice=request.tool_choice
        ),
    )
    return out


@pytest.mark.parametrize(("gate", "tool", "schema"), CASES)
def test_the_first_call_cannot_be_the_artifact(gate, tool, schema):
    """§230's failure, where it costs the most.

    A composed `HypothesisOutput` carries theories and papers, and looks exactly like a
    twenty-minute theorizer run. The researcher waits for a panel that will never fill, because no
    task was ever submitted.
    """
    out = _seen(gate, [HumanMessage(content="analyse the attached data")], schema)
    assert out["choice"] == tool
    # While a structured output tool is bound, `tool_choice` never reaches the model (§133).
    assert out["format"] is None


@pytest.mark.parametrize(("gate", "tool", "schema"), CASES)
def test_once_the_run_is_submitted_the_schema_comes_back(gate, tool, schema):
    out = _seen(
        gate,
        [
            HumanMessage(content="analyse it"),
            AIMessage(content="", tool_calls=[{"name": tool, "args": {}, "id": "call_1"}]),
            ToolMessage(content='{"status":"running"}', tool_call_id="call_1", name=tool),
        ],
        schema,
    )
    assert out["format"] == schema
    assert out["choice"] is None


@pytest.mark.parametrize(("gate", "tool", "schema"), CASES)
def test_a_status_check_satisfies_it_too(gate, tool, schema):
    """Both tools double as their own status check via `resume_task_id`, and a subagent asked to
    check a specific run has engaged with a real task id — which is the thing being insisted on."""
    out = _seen(
        gate,
        [
            HumanMessage(content="check task 5f0c…"),
            AIMessage(
                content="",
                tool_calls=[{"name": tool, "args": {"resume_task_id": "x"}, "id": "call_1"}],
            ),
            ToolMessage(content='{"status":"completed"}', tool_call_id="call_1", name=tool),
        ],
        schema,
    )
    assert out["choice"] is None


@pytest.mark.parametrize(("gate", "tool", "schema"), CASES)
@pytest.mark.parametrize("other", ["execute", "ls", "read_file"])
def test_reading_the_workspace_is_not_starting_a_run(gate, tool, schema, other):
    """Reading a finished analysis off disk is documented and useful, and it is not a run."""
    out = _seen(
        gate,
        [
            HumanMessage(content="analyse it"),
            AIMessage(content="", tool_calls=[{"name": other, "args": {}, "id": "call_1"}]),
            ToolMessage(content="ok", tool_call_id="call_1", name=other),
        ],
        schema,
    )
    assert out["choice"] == tool


def test_the_gates_are_attached_to_exactly_the_two_subagents():
    """§128: the wiring runs once, at graph assembly, on a path no other test reaches.

    And the two deliberately left alone stay alone — `research_planner` runs nothing by design and
    `report_writer` synthesises from the conversation, so neither has a tool to force.
    """
    from backend.subagents import _build_runtime_subagents

    class Resolver:
        def for_subagent(self, name, overrides):
            return "openai::gpt-4o-mini"

    class Sandbox:
        async def aget_work_dir(self):
            return "/w"

    built = _build_runtime_subagents(
        academic_research_tools=[],
        dataverse_tools=[],
        data_cleaning_tools=[],
        diagnostic_tools=[],
        theory_tools=[],
        discovery_tools=[],
        validation_tools=[],
        file_sync=object(),
        sandbox_backend=Sandbox(),
        model_resolver=Resolver(),
        subagent_overrides={},
    )
    holding = lambda kind: {  # noqa: E731
        s["name"] for s in built if any(isinstance(m, kind) for m in s["middleware"])
    }
    assert holding(TheorizeBeforeReporting) == {"hypothesis_generator"}
    # And the third, added when AutoDiscovery arrived: the one whose fabricated artifact would
    # reach the credit gate rather than merely a waiting researcher.
    assert holding(DraftBeforeReporting) == {"autodiscovery"}


# --- the third gate, where the stakes are money (§248) --------------------------------------------

def test_the_draft_gate_forces_the_only_tool_autodiscovery_has():
    from backend.middleware.submit_first import DRAFT_TOOL

    assert DRAFT_TOOL == "draft_discovery_run"
    assert [step.force for step in DraftBeforeReporting.steps] == [DRAFT_TOOL]
    # The reason is stated, because a gate whose refusal says nothing teaches nothing.
    assert "no run id" in DraftBeforeReporting.steps[0].because


def test_the_autodiscovery_prompt_says_it_cannot_start_a_run():
    """The prompt and the tool surface have to agree. If a future edit softened this into "ask the
    user first", the model would go looking for a submit tool that deliberately does not exist."""
    from backend.subagents import AUTODISCOVERY_SYSTEM_PROMPT as prompt

    assert "CANNOT START A RUN" in prompt
    assert "There is no tool that submits" in prompt
    # And it must not invent results it has none of.
    assert "NEVER report experiments" in prompt
    # The one field that steers the search, and the trap of writing the hypothesis into it.
    assert "steering" in prompt


def test_autodiscovery_is_told_when_another_specialist_fits_better():
    """A question the user can already state does not need credits spent on it."""
    from backend.subagents import AUTODISCOVERY_SYSTEM_PROMPT as prompt

    assert "diagnostic_analytics" in prompt
    assert "decides its own questions" in prompt


def test_the_autodiscovery_prompt_says_the_run_cannot_see_the_researchers_machine():
    """§249: the first real run failed because the model was handed `/mnt/c/...` paths and the
    sandbox is a container. The prompt has to say so, and has to say what to do instead."""
    from backend.subagents import AUTODISCOVERY_SYSTEM_PROMPT as prompt

    assert "CANNOT SEE THE RESEARCHER" in prompt
    assert "/mnt/c/" in prompt
    assert "./name" in prompt
    # And it must not retry: a missing file does not become present on a second attempt.
    assert "Do NOT retry" in prompt


def test_the_autodiscovery_prompt_forbids_paraphrasing_a_failure():
    """The failure reached the researcher as "the subagent did not return the failure reason",
    which is what a summary of an actionable error looks like."""
    from backend.subagents import AUTODISCOVERY_SYSTEM_PROMPT as prompt

    assert "QUOTED VERBATIM" in prompt
    assert "Do not paraphrase" in prompt
