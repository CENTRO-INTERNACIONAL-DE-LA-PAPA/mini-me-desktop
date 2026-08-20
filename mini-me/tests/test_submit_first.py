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


# --- the guidance the working run produced (§239) ------------------------------------------------

def test_the_prompt_asks_for_the_four_things_that_changed_the_outcome():
    """Measured, not asserted. A question meeting these four fitted models; one missing the last
    came back as a preprocessing plan with nothing fitted.

    Pinned as a test because prompt text is the easiest thing in this repository to lose in an
    edit, and the cost of losing it is a twenty-minute run that answers in prose.
    """
    from backend.subagents import DATA_VOYAGER_SYSTEM_PROMPT as prompt

    assert "NAME THE DATASETS" in prompt
    assert "NAME THE METHODS" in prompt
    assert "ASK FOR THE NUMBERS" in prompt
    assert "SAY TO RUN IT" in prompt
    # And the example, because a rule with no instance is a rule people interpret.
    assert "Actually run the code and report the numbers" in prompt


def test_the_prompt_defaults_to_a_fresh_session():
    """DataVoyager reasons over a context's whole history, so a question asked inside a session
    about something else is answered in the light of that other thing — which is how one run
    declined to fit anything at all."""
    from backend.subagents import DATA_VOYAGER_SYSTEM_PROMPT as prompt

    assert "pass NO `context_id`" in prompt
    assert "ONLY when the user is explicitly continuing" in prompt


def test_the_log_says_whether_the_session_was_reused():
    """Requested in the prompt, recorded in the log — so a run that ignored the request is visible
    rather than merely disappointing."""
    import asyncio
    import json as _json
    from types import SimpleNamespace

    from backend.datavoyager_tools import analyze_data

    record = {"id": "4ee871fd-64cc-48a7-947b-6baca0e95e4c", "contextId": "ctx-9"}
    seen: list[str] = []

    class Sandbox:
        async def aexecute(self, command, timeout=None):
            seen.append(command)
            return SimpleNamespace(exit_code=0, output=_json.dumps(record))

    question = "Fit and compare ridge and random forest predicting y from x.csv, report R2."
    from backend.runtime import _active_sandbox

    token = _active_sandbox.set(Sandbox())
    try:
        fresh = _json.loads(
            asyncio.run(analyze_data.ainvoke({"question": question, "dataset_paths": "x.csv"}))
        )
        assert fresh["status"] == "running"
        # A reused session still works; the point is that it is distinguishable.
        again = _json.loads(
            asyncio.run(
                analyze_data.ainvoke(
                    {"question": question, "dataset_paths": "x.csv", "context_id": "ctx-9"}
                )
            )
        )
        assert again["status"] == "running"
        assert "--context-id" in seen[-1] and "--context-id" not in seen[0]
    finally:
        _active_sandbox.reset(token)
