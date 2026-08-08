"""The autonomous run-loop plan: a pure, human-gated state machine (P5).

The plan is what turns the coordinator from "answer this" into "advance the
investigation": an AI-authored, ordered sequence of subagent steps that the user
**accepts or edits**, then executes **one confirmed step at a time**. Nothing
here runs a subagent — org policy is human-gated, so a step only ever *drops a
prompt into the composer* (via the P3.2 prefill) for the user to send.

Two levels of lifecycle:

  * the **plan** is ``proposed`` (freshly authored, awaiting accept/edit),
    ``active`` (accepted; the loop is stepping through it), or ``done`` (every
    step done or skipped); and
  * each **step** is ``pending``, ``active`` (the current one — exactly one while
    the plan is active), ``done``, or ``skipped``.

Invariant (``sync_plan``): while a plan is ``active`` there is exactly one
``active`` step, and once no step is left to run the plan flips to ``done``.

Everything is a pure function of ``(plan, edit)`` — no store, no model, no
side-effects — so the whole loop is unit-testable offline. Stable step ids and
statuses are assigned here (never by the model): :func:`plan_from_output`
normalizes the planner's raw output, and :func:`apply_plan_edit` applies one
human edit (accept / complete / skip / edit / add / remove / reorder /
set-active / clear).
"""

from __future__ import annotations

import re
from typing import Any

from backend.schemas import PlanStepPayload, ResearchPlanPayload

PLAN_PROPOSED = "proposed"
PLAN_ACTIVE = "active"
PLAN_DONE = "done"

STEP_PENDING = "pending"
STEP_ACTIVE = "active"
STEP_DONE = "done"
STEP_SKIPPED = "skipped"

_TERMINAL_STEP = {STEP_DONE, STEP_SKIPPED}

_MAX_STEPS = 20
_MAX_TEXT = 600


def _clip(text: Any, limit: int = _MAX_TEXT) -> str:
    return str(text or "").strip()[:limit]


def _next_step_id(steps: list[PlanStepPayload]) -> str:
    """A fresh ``s<N>`` id, one past the highest numeric suffix in use.

    Robust to reorder/removal: ids never collide because N only grows.
    """
    highest = 0
    for step in steps:
        match = re.fullmatch(r"s(\d+)", str(step.get("id") or ""))
        if match:
            highest = max(highest, int(match.group(1)))
    return f"s{highest + 1}"


def _coerce_step(raw: Any, *, step_id: str) -> PlanStepPayload:
    raw = raw if isinstance(raw, dict) else {}
    status = str(raw.get("status") or STEP_PENDING)
    if status not in (STEP_PENDING, STEP_ACTIVE, STEP_DONE, STEP_SKIPPED):
        status = STEP_PENDING
    return {
        "id": step_id,
        "title": _clip(raw.get("title"), 200),
        "rationale": _clip(raw.get("rationale"), 400),
        "action": _clip(raw.get("action"), 80),
        "prompt": _clip(raw.get("prompt")),
        "status": status,
    }


def _raw_steps(source: Any) -> list[Any]:
    """Pull the ``steps`` list off a Pydantic ``ResearchPlan`` or a dict."""
    if source is None:
        return []
    steps = source.get("steps") if isinstance(source, dict) else getattr(source, "steps", None)
    if not isinstance(steps, list):
        return []
    out: list[Any] = []
    for step in steps:
        if isinstance(step, dict):
            out.append(step)
        else:  # Pydantic PlanStep
            out.append(
                {
                    "title": getattr(step, "title", ""),
                    "rationale": getattr(step, "rationale", ""),
                    "action": getattr(step, "action", ""),
                    "prompt": getattr(step, "prompt", ""),
                }
            )
    return out


def _get(source: Any, key: str) -> Any:
    return source.get(key) if isinstance(source, dict) else getattr(source, key, None)


def plan_from_output(source: Any, *, nonce: str | None = None) -> ResearchPlanPayload | None:
    """Normalize a planner's raw output into a fresh, ``proposed`` plan.

    Assigns stable ``s1..sN`` ids, drops empty steps, and caps the count. Returns
    ``None`` when there is no usable step (so an empty plan never renders).
    """
    raw_steps = [s for s in _raw_steps(source) if _clip(_get(s, "title") or _get(s, "prompt"))]
    if not raw_steps:
        return None
    raw_steps = raw_steps[:_MAX_STEPS]
    steps: list[PlanStepPayload] = [
        _coerce_step({**s, "status": STEP_PENDING}, step_id=f"s{i + 1}")
        for i, s in enumerate(raw_steps)
    ]
    plan: ResearchPlanPayload = {
        "goal": _clip(_get(source, "goal"), 400),
        "status": PLAN_PROPOSED,
        "steps": steps,
    }
    if nonce:
        plan["nonce"] = str(nonce)
    return plan


def has_plan_content(plan: ResearchPlanPayload | None) -> bool:
    return bool(plan and plan.get("steps"))


def coerce_plan(value: Any) -> ResearchPlanPayload | None:
    """Normalize a stored/legacy plan record; ``None`` when there is nothing usable."""
    if not isinstance(value, dict):
        return None
    raw_steps = value.get("steps")
    if not isinstance(raw_steps, list) or not raw_steps:
        return None
    steps = [_coerce_step(s, step_id=str((s or {}).get("id") or f"s{i + 1}"))
             for i, s in enumerate(raw_steps)]
    status = str(value.get("status") or PLAN_PROPOSED)
    if status not in (PLAN_PROPOSED, PLAN_ACTIVE, PLAN_DONE):
        status = PLAN_PROPOSED
    plan: ResearchPlanPayload = {
        "goal": _clip(value.get("goal"), 400),
        "status": status,
        "steps": steps,
    }
    if value.get("nonce"):
        plan["nonce"] = str(value["nonce"])
    return sync_plan(plan)


# ---------------------------------------------------------------------------
# Invariants
# ---------------------------------------------------------------------------

def _first_pending_index(steps: list[Any]) -> int | None:
    for i, step in enumerate(steps):
        if step.get("status") == STEP_PENDING:
            return i
    return None


def sync_plan(plan: ResearchPlanPayload | None) -> ResearchPlanPayload | None:
    """Enforce the lifecycle invariants; returns a normalized copy (or ``None``)."""
    if not has_plan_content(plan):
        return None
    assert plan is not None
    steps = [dict(s) for s in plan["steps"]]  # type: ignore[assignment]
    status = plan.get("status") or PLAN_PROPOSED

    if status == PLAN_PROPOSED:
        # Not accepted yet: nothing is active.
        for step in steps:
            if step.get("status") == STEP_ACTIVE:
                step["status"] = STEP_PENDING
    else:
        # Accepted (active/done): at most one active step, and if every step is
        # terminal the plan is done; otherwise ensure exactly one active.
        active = [i for i, s in enumerate(steps) if s.get("status") == STEP_ACTIVE]
        for i in active[1:]:  # demote extras
            steps[i]["status"] = STEP_PENDING
        has_active = bool(active[:1])
        remaining = any(s.get("status") not in _TERMINAL_STEP for s in steps)
        if not remaining:
            status = PLAN_DONE
        else:
            status = PLAN_ACTIVE
            if not has_active:
                idx = _first_pending_index(steps)
                if idx is not None:
                    steps[idx]["status"] = STEP_ACTIVE

    out: ResearchPlanPayload = {
        "goal": plan.get("goal") or "",
        "status": status,
        "steps": steps,  # type: ignore[typeddict-item]
    }
    nonce = plan.get("nonce")
    if nonce:
        out["nonce"] = str(nonce)
    return out


# ---------------------------------------------------------------------------
# Edits (one human action at a time)
# ---------------------------------------------------------------------------

def _find(steps: list[PlanStepPayload], step_id: str) -> int | None:
    for i, step in enumerate(steps):
        if step.get("id") == step_id:
            return i
    return None


def apply_plan_edit(
    plan: ResearchPlanPayload | None, edit: dict[str, Any]
) -> ResearchPlanPayload | None:
    """Apply one human edit and return the new plan (or ``None`` to clear it).

    Recognized ``op`` values: ``accept``, ``complete``, ``skip``, ``activate``,
    ``edit``, ``add``, ``remove``, ``reorder``, ``clear``. Unknown ops and
    unresolvable ids are no-ops (idempotent — safe to click twice).
    """
    op = str((edit or {}).get("op") or "").strip()
    if op == "clear":
        return None
    if not has_plan_content(plan):
        return sync_plan(plan)
    assert plan is not None

    steps: list[PlanStepPayload] = [dict(s) for s in plan["steps"]]  # type: ignore[assignment]
    status = plan.get("status") or PLAN_PROPOSED
    step_id = str(edit.get("id") or "")

    if op == "accept":
        status = PLAN_ACTIVE

    elif op == "complete":
        idx = _find(steps, step_id)
        if idx is not None:
            steps[idx]["status"] = STEP_DONE
            status = PLAN_ACTIVE

    elif op == "skip":
        idx = _find(steps, step_id)
        if idx is not None:
            steps[idx]["status"] = STEP_SKIPPED
            status = PLAN_ACTIVE

    elif op == "activate":
        idx = _find(steps, step_id)
        if idx is not None:
            for step in steps:
                if step.get("status") == STEP_ACTIVE:
                    step["status"] = STEP_PENDING
            # Re-open a terminal step if the user redirects back to it.
            steps[idx]["status"] = STEP_ACTIVE
            status = PLAN_ACTIVE

    elif op == "edit":
        idx = _find(steps, step_id)
        if idx is not None:
            for field, limit in (("title", 200), ("rationale", 400), ("action", 80), ("prompt", _MAX_TEXT)):
                if field in edit:
                    steps[idx][field] = _clip(edit[field], limit)  # type: ignore[literal-required]

    elif op == "add":
        if len(steps) < _MAX_STEPS:
            new_step = _coerce_step(
                {
                    "title": edit.get("title"),
                    "rationale": edit.get("rationale"),
                    "action": edit.get("action"),
                    "prompt": edit.get("prompt"),
                    "status": STEP_PENDING,
                },
                step_id=_next_step_id(steps),
            )
            if new_step["title"] or new_step["prompt"]:
                after = _find(steps, str(edit.get("after_id") or ""))
                insert_at = (after + 1) if after is not None else len(steps)
                steps.insert(insert_at, new_step)

    elif op == "remove":
        idx = _find(steps, step_id)
        if idx is not None:
            steps.pop(idx)

    elif op == "reorder":
        order = edit.get("order")
        if isinstance(order, list):
            by_id = {s.get("id"): s for s in steps}
            reordered = [by_id[i] for i in order if i in by_id]
            # Keep any ids the client omitted, in their original order.
            reordered += [s for s in steps if s not in reordered]
            steps = reordered

    new_plan: ResearchPlanPayload = {
        "goal": plan.get("goal") or "",
        "status": status,
        "steps": steps,
    }
    nonce = plan.get("nonce")
    if nonce:
        new_plan["nonce"] = str(nonce)
    return sync_plan(new_plan)
