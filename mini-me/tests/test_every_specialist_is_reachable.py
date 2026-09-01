"""Every registered subagent must be one the coordinator has been told about.

`autodiscovery_subagent` was in the registry, had its tools wired, had a credit gate written for
it and a panel in the app — and appeared in **none** of the three lists that tell a model it
exists. The coordinator's "Available sub-agents" prompt had eleven bullets for twelve subagents,
and the research planner's `action` field enumerated ten labels.

So the only way to reach it was to name it in a question. The researcher's own conversation list
carries the workaround as a title: *"usea autodiscovery to test this data"* (§295).

A registry is not a routing table. These tests hold the two together, because the failure is
silent in both directions: nothing errors, the specialist simply never runs.
"""

from __future__ import annotations

from pathlib import Path

import pytest

from backend.prompts import COORDINATOR_SYSTEM_PROMPT
from backend.schemas import PlanStep
from backend.subagents import subagents

#: The friendly label each registered subagent is named by, in the prompts a model reads.
#:
#: Written here rather than derived, deliberately: the labels are prose a model matches on, and a
#: test that generated them from the internal names would agree with any renaming — including one
#: that left the prompt behind, which is the defect this file exists for.
LABELS: dict[str, str] = {
    "academic_researcher": "Academic Research",
    "dataverse_explorer": "Dataverse Explorer",
    "data_cleaning": "Data Cleaning",
    "exploratory_data_analysis": "Exploratory Data Analysis",
    "diagnostic_analytics": "Diagnostic Analytics",
    "predictive_analytics": "Predictive Analytics",
    "report_writer": "Report Writer",
    "hypothesis_generator": "Hypothesis Generator",
    "pdf_librarian": "PDF Librarian",
    "data_voyager": "DataVoyager",
    "autodiscovery": "AutoDiscovery",
    "research_planner": "Research Planner",
}


#: Registered, and deliberately not something a plan step can name.
#:
#: Listed rather than inferred, so that "absent from the planner's labels" means *somebody decided*
#: rather than *somebody forgot* — which is the whole distinction this file exists to make, and the
#: same argument `claims.NO_PATHS` makes for schemas nothing checks.
NOT_PLANNABLE: dict[str, str] = {
    "research_planner": "it writes the plan; a step naming it would plan itself",
}


def _registered() -> list[str]:
    return [subagent["name"] for subagent in subagents]


def test_the_label_table_covers_every_registered_subagent():
    """The table is hand-written, so it has to be held to the registry rather than trusted."""
    missing = [name for name in _registered() if name not in LABELS]
    assert not missing, (
        f"{missing} are registered and this test does not know what to call them — add the label "
        "the prompts use, then check the prompts actually use it"
    )


@pytest.mark.parametrize("name", _registered())
def test_the_coordinator_has_been_told_this_specialist_exists(name):
    """**A subagent the coordinator's prompt never names cannot be routed to.**

    It is not an error, it is an absence: the model picks from what it was given, and this one was
    not on the list for as long as it existed.
    """
    assert f"- {LABELS[name]}:" in COORDINATOR_SYSTEM_PROMPT, (
        f"{name} is registered but is not a bullet in the coordinator's Available sub-agents list"
    )


@pytest.mark.parametrize("name", _registered())
def test_the_planner_can_name_this_specialist(name):
    """The planner writes `action` from a closed list; one missing means a step it cannot express.

    Its choices are: omit the step, invent a label nothing routes, or mislabel it as a neighbour.
    All three are worse than the step not being planned at all, because two of them look fine.
    """
    if name in NOT_PLANNABLE:
        pytest.skip(f"{name}: {NOT_PLANNABLE[name]}")
    described = PlanStep.model_fields["action"].description or ""
    assert f"'{LABELS[name]}'" in described, (
        f"{name} is registered but the planner's action field cannot name it"
    )


def test_nothing_is_excused_that_is_not_registered():
    """An exclusion for a subagent that no longer exists is a stale excuse, not a decision."""
    unknown = [name for name in NOT_PLANNABLE if name not in _registered()]
    assert not unknown, f"{unknown} are excused from planning and are not registered at all"


def test_the_two_lists_the_planner_reads_agree():
    """The label list is written twice — in the schema field and in the planner's prompt.

    Two copies of one list is two chances to update one of them, which is exactly what happened.
    """
    described = PlanStep.model_fields["action"].description or ""
    # **Whitespace-normalised**, because the prompt is a wrapped triple-quoted string: the label
    # `'Predictive Analytics'` is split across a line break and eight spaces of indentation, so a
    # raw substring check reports a label that is plainly there. A test that fails on formatting
    # gets deleted rather than believed.
    prompt = " ".join(
        (Path(__file__).resolve().parent.parent / "backend" / "subagents.py")
        .read_text(encoding="utf-8")
        .split()
    )
    for name in _registered():
        label = LABELS[name]
        if f"'{label}'" not in described:
            continue
        assert f"'{label}'" in prompt, (
            f"the schema offers '{label}' and the planner's own prompt does not list it"
        )


def test_a_credit_spending_specialist_says_so_where_the_model_reads_it():
    """AutoDiscovery and DataVoyager spend the researcher's Asta credits.

    The gate that stops an unapproved run is real (`NoSpendingWithoutApproval`), and it is not the
    coordinator's only defence: a coordinator that does not know a specialist costs money will
    route to it as freely as to a search. Naming the cost where the routing decision is made is
    the cheap half.
    """
    bullet = COORDINATOR_SYSTEM_PROMPT.split("- AutoDiscovery:")[1].split("\n  - ")[0]
    assert "credit" in bullet.lower(), "the bullet must say it spends credits"
    assert "human press" in bullet.lower() or "approval" in bullet.lower(), (
        "and that it cannot be started without one"
    )


def test_the_coordinator_is_told_where_a_draft_has_to_be_made():
    """**A draft in a background worker cannot be approved, so it cannot be run.**

    `decode_drafts` reads `artifacts.discoveries` from *this* conversation's snapshot, and the
    background worker is a separate graph with its own state. A run drafted there reaches the
    researcher as an id in prose and no approval modal ever opens — which is what happened on a
    real machine the day the coordinator was first told AutoDiscovery existed (§296).

    The deeper gap is that worker artifacts never fold into the snapshot at all. This is the half
    that costs nothing and unblocks the feature; the other half is in §296.
    """
    bullet = COORDINATOR_SYSTEM_PROMPT.split("- AutoDiscovery:")[1].split("\n  - ")[0]
    assert "start_async_task" in bullet, (
        "the bullet must name the tool whose drafts cannot be approved"
    )
    assert "approval modal" in bullet.lower()


def test_the_coordinator_is_told_to_look_before_asking_for_a_path():
    """**A file bought by a press arrives with no message announcing it.**

    A dataset downloaded from the Datasets panel lands in the working directory silently. The
    researcher then asked the coordinator to analyse "the files I just downloaded" and it asked
    for an exact path — to a 994 KB zip sitting beside it, listed in its own Outputs panel (§298).

    That is `_say_where_it_ran`'s lesson at a different moment: the model was not being lazy, it
    had never been told the file was there and did not think to look.
    """
    prompt = COORDINATOR_SYSTEM_PROMPT
    assert "List the working directory before asking the user for a path" in prompt
    assert "ls -la" in prompt, "the instruction has to name the thing to run"
    # And what to do when the listing really is empty — a question naming what was found is one a
    # researcher can answer; "give me the exact path" is not.
    assert "which did you mean" in prompt.lower()
