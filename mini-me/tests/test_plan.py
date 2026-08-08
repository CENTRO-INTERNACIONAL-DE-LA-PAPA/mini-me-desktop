"""Tests for the P5 autonomous run-loop plan state machine (`backend.plan`).

These pin the human-gated contract: the planner's raw output is normalized into
a ``proposed`` plan with stable ids, the human drives every transition
(accept / complete / skip / activate / edit / add / remove / reorder / clear)
through pure edits, the lifecycle invariants always hold (exactly one active step
while active; all-terminal → done), and the plan folds into the project spine
only when the planner genuinely re-ran (distinguished by ``nonce``). Nothing here
executes a subagent — a step is only ever a prompt the user later sends.
"""

from __future__ import annotations

from types import SimpleNamespace
from typing import Any

from backend.middleware.artifacts import ArtifactCaptureMiddleware
from backend.plan import (
    PLAN_ACTIVE,
    PLAN_DONE,
    PLAN_PROPOSED,
    STEP_ACTIVE,
    STEP_DONE,
    STEP_PENDING,
    STEP_SKIPPED,
    apply_plan_edit,
    coerce_plan,
    has_plan_content,
    plan_from_output,
    sync_plan,
)
from backend.project import advance_project, empty_project


def _raw(*titles: str) -> dict:
    return {
        "goal": "understand drought stress",
        "steps": [
            {"title": t, "rationale": "because", "action": "Academic Research",
             "prompt": f"Use the academic_researcher subagent to {t}."}
            for t in titles
        ],
    }


def _statuses(plan: Any) -> list[str]:
    return [s["status"] for s in plan["steps"]]


def _active_ids(plan: Any) -> list[str]:
    return [s["id"] for s in plan["steps"] if s["status"] == STEP_ACTIVE]


# ---------------------------------------------------------------------------
# Normalization
# ---------------------------------------------------------------------------

def test_plan_from_output_assigns_ids_and_proposed_pending() -> None:
    plan = plan_from_output(_raw("a", "b", "c"), nonce="n1")
    assert plan is not None
    assert plan["status"] == PLAN_PROPOSED
    assert [s["id"] for s in plan["steps"]] == ["s1", "s2", "s3"]
    assert _statuses(plan) == [STEP_PENDING, STEP_PENDING, STEP_PENDING]
    assert plan["goal"] == "understand drought stress"
    assert plan["nonce"] == "n1"


def test_plan_from_output_drops_empty_and_returns_none_when_all_empty() -> None:
    raw = {"goal": "g", "steps": [{"title": "", "prompt": ""}, {"title": "keep", "prompt": "p"}]}
    plan = plan_from_output(raw)
    assert plan is not None and len(plan["steps"]) == 1 and plan["steps"][0]["title"] == "keep"
    assert plan_from_output({"goal": "g", "steps": []}) is None
    assert plan_from_output(None) is None


def test_plan_from_output_reads_pydantic_like_steps() -> None:
    class _Step:
        def __init__(self, title: str) -> None:
            self.title, self.rationale, self.action, self.prompt = title, "r", "DataVoyager", "p"

    class _Plan:
        goal = "g"
        steps = [_Step("x"), _Step("y")]

    plan = plan_from_output(_Plan())
    assert plan is not None and [s["title"] for s in plan["steps"]] == ["x", "y"]
    assert plan["steps"][0]["action"] == "DataVoyager"


# ---------------------------------------------------------------------------
# Accept / execute / advance
# ---------------------------------------------------------------------------

def test_accept_activates_first_step() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b")), {"op": "accept"})
    assert plan is not None
    assert plan["status"] == PLAN_ACTIVE
    assert _statuses(plan) == [STEP_ACTIVE, STEP_PENDING]


def test_complete_advances_to_next_then_finishes() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b")), {"op": "accept"})
    plan = apply_plan_edit(plan, {"op": "complete", "id": "s1"})
    assert plan is not None
    assert _statuses(plan) == [STEP_DONE, STEP_ACTIVE]
    assert plan["status"] == PLAN_ACTIVE
    plan = apply_plan_edit(plan, {"op": "complete", "id": "s2"})
    assert plan is not None
    assert _statuses(plan) == [STEP_DONE, STEP_DONE]
    assert plan["status"] == PLAN_DONE


def test_skip_advances_like_complete() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b")), {"op": "accept"})
    plan = apply_plan_edit(plan, {"op": "skip", "id": "s1"})
    assert plan is not None
    assert plan["steps"][0]["status"] == STEP_SKIPPED
    assert plan["steps"][1]["status"] == STEP_ACTIVE


def test_exactly_one_active_step_while_active() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b", "c")), {"op": "accept"})
    assert plan is not None
    assert len(_active_ids(plan)) == 1


# ---------------------------------------------------------------------------
# Redirect: activate a specific step
# ---------------------------------------------------------------------------

def test_activate_redirects_and_demotes_previous_active() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b", "c")), {"op": "accept"})
    plan = apply_plan_edit(plan, {"op": "activate", "id": "s3"})
    assert plan is not None
    assert _active_ids(plan) == ["s3"]
    assert plan["steps"][0]["status"] == STEP_PENDING


def test_activate_reopens_a_completed_step() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b")), {"op": "accept"})
    plan = apply_plan_edit(plan, {"op": "complete", "id": "s1"})
    plan = apply_plan_edit(plan, {"op": "activate", "id": "s1"})
    assert plan is not None
    assert plan["steps"][0]["status"] == STEP_ACTIVE
    assert plan["status"] == PLAN_ACTIVE


# ---------------------------------------------------------------------------
# Structural edits
# ---------------------------------------------------------------------------

def test_edit_updates_step_fields() -> None:
    plan = plan_from_output(_raw("a", "b"))
    plan = apply_plan_edit(plan, {"op": "edit", "id": "s2", "title": "new title", "prompt": "new prompt"})
    assert plan is not None
    assert plan["steps"][1]["title"] == "new title"
    assert plan["steps"][1]["prompt"] == "new prompt"


def test_add_inserts_after_and_gets_fresh_id() -> None:
    plan = plan_from_output(_raw("a", "b"))
    plan = apply_plan_edit(
        plan,
        {"op": "add", "after_id": "s1", "title": "inserted", "action": "Report Writer", "prompt": "p"},
    )
    assert plan is not None
    assert [s["id"] for s in plan["steps"]] == ["s1", "s3", "s2"]
    assert plan["steps"][1]["title"] == "inserted"
    assert plan["steps"][1]["status"] == STEP_PENDING


def test_remove_active_step_advances() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b")), {"op": "accept"})
    plan = apply_plan_edit(plan, {"op": "remove", "id": "s1"})
    assert plan is not None
    assert [s["id"] for s in plan["steps"]] == ["s2"]
    assert plan["steps"][0]["status"] == STEP_ACTIVE  # promoted after removal


def test_reorder_reorders_and_keeps_omitted() -> None:
    plan = plan_from_output(_raw("a", "b", "c"))
    plan = apply_plan_edit(plan, {"op": "reorder", "order": ["s3", "s1"]})
    assert plan is not None
    # s2 was omitted from the order → appended after the explicit ones.
    assert [s["id"] for s in plan["steps"]] == ["s3", "s1", "s2"]


def test_clear_removes_the_plan() -> None:
    assert apply_plan_edit(plan_from_output(_raw("a")), {"op": "clear"}) is None


def test_unknown_op_and_bad_id_are_noops() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b")), {"op": "accept"})
    same = apply_plan_edit(plan, {"op": "complete", "id": "does-not-exist"})
    assert same == plan
    assert apply_plan_edit(plan, {"op": "bogus"}) == plan


# ---------------------------------------------------------------------------
# Invariants + coercion round-trip
# ---------------------------------------------------------------------------

def test_sync_demotes_extra_active_steps() -> None:
    plan = plan_from_output(_raw("a", "b"))
    assert plan is not None
    plan["status"] = PLAN_ACTIVE
    plan["steps"][0]["status"] = STEP_ACTIVE
    plan["steps"][1]["status"] = STEP_ACTIVE
    synced = sync_plan(plan)
    assert synced is not None
    assert len(_active_ids(synced)) == 1


def test_coerce_plan_round_trips_and_rejects_junk() -> None:
    plan = apply_plan_edit(plan_from_output(_raw("a", "b"), nonce="n1"), {"op": "accept"})
    reloaded = coerce_plan(dict(plan))  # simulate a store round-trip
    assert reloaded is not None
    assert reloaded["status"] == PLAN_ACTIVE
    assert reloaded["nonce"] == "n1"
    assert coerce_plan({"steps": []}) is None
    assert coerce_plan("nope") is None
    assert not has_plan_content(None)


# ---------------------------------------------------------------------------
# Fold into the project spine (nonce guards a lingering carrier)
# ---------------------------------------------------------------------------

def test_advance_project_folds_new_plan_carrier() -> None:
    carrier = {"goal": "g", "steps": _raw("a", "b")["steps"], "nonce": "n1"}
    state, _ = advance_project(empty_project(), {"plan": carrier}, [])
    assert state["plan"] is not None
    assert state["plan"]["status"] == PLAN_PROPOSED
    assert len(state["plan"]["steps"]) == 2


def test_lingering_carrier_does_not_reset_user_accepted_plan() -> None:
    carrier = {"goal": "g", "steps": _raw("a", "b")["steps"], "nonce": "n1"}
    # Turn 1: planner runs → plan folded (proposed).
    state, _ = advance_project(empty_project(), {"plan": carrier}, [])
    # User accepts + completes the first step.
    accepted = apply_plan_edit(state["plan"], {"op": "accept"})
    accepted = apply_plan_edit(accepted, {"op": "complete", "id": "s1"})
    prev = {**empty_project(), "plan": accepted}
    # Turn 2: SAME carrier still lingers in checkpoint state (same nonce), no new
    # planner run → the user's progress must be preserved, not reset to proposed.
    state2, _ = advance_project(prev, {"plan": carrier}, [])
    assert state2["plan"] is not None
    assert state2["plan"]["status"] == PLAN_ACTIVE
    assert state2["plan"]["steps"][0]["status"] == STEP_DONE


def test_new_generation_replaces_prior_plan() -> None:
    carrier1 = {"goal": "g", "steps": _raw("a")["steps"], "nonce": "n1"}
    state, _ = advance_project(empty_project(), {"plan": carrier1}, [])
    prev = {**empty_project(), "plan": apply_plan_edit(state["plan"], {"op": "accept"})}
    carrier2 = {"goal": "g2", "steps": _raw("x", "y", "z")["steps"], "nonce": "n2"}
    state2, _ = advance_project(prev, {"plan": carrier2}, [])
    assert state2["plan"] is not None
    assert state2["plan"]["status"] == PLAN_PROPOSED  # fresh plan awaits accept
    assert len(state2["plan"]["steps"]) == 3
    assert state2["plan"]["nonce"] == "n2"


# ---------------------------------------------------------------------------
# Capture: the research_planner subagent emits a nonce-stamped plan carrier
# ---------------------------------------------------------------------------

def test_planner_capture_emits_plan_carrier_with_nonce() -> None:
    structured = SimpleNamespace(
        goal="understand X",
        steps=[
            SimpleNamespace(title="a", rationale="r", action="Academic Research", prompt="p1"),
            SimpleNamespace(title="b", rationale="r", action="DataVoyager", prompt="p2"),
        ],
    )
    mw = ArtifactCaptureMiddleware(source="research_planner")
    result = mw.after_agent({"structured_response": structured}, None)
    assert result is not None
    carrier = result["artifacts"]["plan"]
    assert carrier["goal"] == "understand X"
    assert [s["title"] for s in carrier["steps"]] == ["a", "b"]
    assert carrier["nonce"]  # stamped, so the spine folds it exactly once
    # A fresh nonce per generation so a re-run is distinguishable.
    result2 = mw.after_agent({"structured_response": structured}, None)
    assert result2 is not None
    assert result2["artifacts"]["plan"]["nonce"] != carrier["nonce"]


def test_planner_capture_with_no_steps_is_ignored() -> None:
    mw = ArtifactCaptureMiddleware(source="research_planner")
    assert mw.after_agent({"structured_response": SimpleNamespace(goal="g", steps=[])}, None) is None
