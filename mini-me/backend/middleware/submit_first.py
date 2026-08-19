"""Make the long-running specialists start their run before they can report one.

# Why these two, and why now

`pdf_librarian` returned a complete `LibraryArtifact` — index path, document title, page count,
summary — having executed nothing (§230). Not a subtle failure and not an unlucky one: it is the exit
`middleware/tool_gate.py` describes, which every subagent carrying a `response_format` has unless
something closes it.

`data_voyager` and `hypothesis_generator` have the same shape and one difference that matters: **both
submit a job and then report on it.** Their artifacts carry a `task_id`, a `status` and — once
complete — findings, charts, theories and papers. A model that composes one has produced something
that looks exactly like a twenty-minute analysis and cost nothing, and the researcher waiting for the
panel to fill will wait for a task that was never submitted.

So the claim enforced is narrow and mechanical: **you cannot report a run you did not start.**

# Why not the other two

`report_writer` and `research_planner` also carry a `response_format` and are deliberately left
alone. The planner runs nothing by design — *"producing a plan runs no subagent and writes no
files"* — and the report writer synthesises from the conversation it was given. Neither has a tool a
gate could force without inventing a requirement, and a gate that forced an unnecessary call would be
teaching the next reader that this file is decoration.

That leaves §140's "seven subagents" at five that admit a gate, three of them closed before this and
these two after.

# Checking is starting

Both tools double as their own status check — `analyze_data(resume_task_id=…)` and
`generate_theories(resume_task_id=…)`. Satisfying the gate that way is correct: a subagent asked to
check a specific run has engaged with a real task id, which is the thing being insisted on.
"""

from __future__ import annotations

from backend.middleware.tool_gate import Step, ToolsBeforeAnswering

#: The tool that submits a DataVoyager run (`backend/datavoyager_tools.py`).
ANALYZE_TOOL = "analyze_data"

#: The tool that submits an Asta Theorizer run (`backend/theory_tools.py`).
THEORIZE_TOOL = "generate_theories"


class SubmitBeforeReporting(ToolsBeforeAnswering):
    """`data_voyager` must submit an analysis before `DataAnalysisResults` is reachable."""

    steps = (
        Step(
            force=ANALYZE_TOOL,
            because="data_voyager has not submitted an analysis, so it has no run to report",
        ),
    )


class TheorizeBeforeReporting(ToolsBeforeAnswering):
    """`hypothesis_generator` must submit a theorizer run before `HypothesisOutput` is reachable."""

    steps = (
        Step(
            force=THEORIZE_TOOL,
            because="hypothesis_generator has not submitted a run, so it has no theories to report",
        ),
    )
