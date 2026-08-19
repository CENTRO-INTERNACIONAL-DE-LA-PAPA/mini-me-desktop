"""You cannot report a run you did not start.

Driven against real LangChain objects for §222's reason: a double more permissive than production
certifies the bug it was meant to catch.
"""

from __future__ import annotations

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.submit_first import (
    ANALYZE_TOOL,
    THEORIZE_TOOL,
    SubmitBeforeReporting,
    TheorizeBeforeReporting,
)

CASES = [
    (SubmitBeforeReporting, ANALYZE_TOOL, "DataAnalysisResults"),
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
    """§230's failure, in the two subagents where it costs the most.

    A composed `DataAnalysisResults` carries findings, charts and hypotheses-tested, and looks
    exactly like a twenty-minute analysis. The researcher waits for a panel that will never fill,
    because no task was ever submitted.
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
        datavoyager_tools=[],
        file_sync=object(),
        sandbox_backend=Sandbox(),
        model_resolver=Resolver(),
        subagent_overrides={},
    )
    holding = lambda kind: {  # noqa: E731
        s["name"] for s in built if any(isinstance(m, kind) for m in s["middleware"])
    }
    assert holding(SubmitBeforeReporting) == {"data_voyager"}
    assert holding(TheorizeBeforeReporting) == {"hypothesis_generator"}
