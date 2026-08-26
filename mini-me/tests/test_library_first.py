"""The PDF librarian must run something before it can report a library.

Driven through the real middleware against real LangChain objects, for §221's reason: a double more
permissive than production certifies the bug.
"""

from __future__ import annotations

import pytest
from langchain_core.messages import AIMessage, HumanMessage, ToolMessage

from backend.middleware.library_first import EXECUTE_TOOL, RunBeforeReporting


def _request(messages, response_format="LibraryArtifact"):
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


def _ran(name=EXECUTE_TOOL):
    return ToolMessage(content="ok", tool_call_id="call_1", name=name)


def _seen(messages):
    out = {}
    RunBeforeReporting().wrap_model_call(
        _request(messages),
        lambda request: out.update(
            format=request.response_format, choice=request.tool_choice
        ),
    )
    return out


def test_the_first_call_cannot_be_the_artifact():
    """§230: the first run composed an index path, a title, a page count and a summary, having run
    nothing at all. That is the cheapest legal move while the schema is reachable in one step."""
    out = _seen([HumanMessage(content="index the attached paper")])
    assert out["choice"] == EXECUTE_TOOL
    # The other half of the fix, and the half easy to leave out: while a structured output tool is
    # bound, `tool_choice` never reaches the model (§133).
    assert out["format"] is None


def test_once_a_command_has_run_the_schema_comes_back():
    """Or the subagent could never produce its artifact at all."""
    out = _seen(
        [
            HumanMessage(content="index it"),
            AIMessage(content="", tool_calls=[{"name": EXECUTE_TOOL, "args": {}, "id": "call_1"}]),
            _ran(),
        ]
    )
    assert out["format"] == "LibraryArtifact"
    assert out["choice"] is None


@pytest.mark.parametrize("looking", ["ls", "read_file", "glob", "grep"])
def test_looking_at_the_workspace_is_not_running_something(looking):
    """Deliberate. Checking whether a file arrived is useful and is not the work — and a gate a
    fabricating model could satisfy by looking is not a gate."""
    out = _seen(
        [
            HumanMessage(content="index it"),
            AIMessage(content="", tool_calls=[{"name": looking, "args": {}, "id": "call_1"}]),
            _ran(looking),
        ]
    )
    assert out["choice"] == EXECUTE_TOOL


def test_the_gate_is_attached_to_the_librarian_and_to_nothing_else():
    """§128: the wiring runs once, at graph assembly, on a path no other test reaches."""
    from backend.middleware.library_first import RunBeforeReporting as Gate
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
        discovery_tools=[],
        file_sync=object(),
        sandbox_backend=Sandbox(),
        model_resolver=Resolver(),
        subagent_overrides={},
    )
    gated = {
        s["name"] for s in built if any(isinstance(m, Gate) for m in s["middleware"])
    }
    assert gated == {"pdf_librarian"}
