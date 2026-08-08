"""Research-project spine: persistent per-user mission + advisory next steps.

This is work item (C), phase 1 of the Asta integration plan — the first
coordinator upgrade from "reactive tool" toward "research workbench". It gives
the coordinator a durable *mission* with *Pending Work* / *Completed Work* that
survives across threads (persisted in the LangGraph store, per user), and, at
the end of a turn, surfaces 1–3 artifact-grounded "suggested next steps".

Design constraints (org policy: human-gated):
  * **Advisory only.** Nothing here executes a subagent. ``derive_suggestions``
    returns text the user reads and chooses to act on; promotion to execution
    is a later phase.
  * **Artifact-grounded.** Both the completed-work summary and the suggestions
    are derived deterministically from the structured artifacts a thread has
    produced (``ArtifactBundle``), never invented.

Everything in this module except :func:`load_project` / :func:`save_project` is
a pure function of its inputs, so the advisory logic is unit-testable without a
store, a sandbox, or a live model.
"""

from __future__ import annotations

import re
from typing import Any, NamedTuple

from typing_extensions import TypedDict

from langgraph.store.base import BaseStore

from backend.plan import coerce_plan, has_plan_content, plan_from_output
from backend.schemas import (
    ProjectArtifactPayload,
    ProjectSuggestionPayload,
    ResearchPlanPayload,
)

# Store key under the per-user project namespace (see
# ``backend.runtime._project_namespace``). A single record holds the whole
# project state.
PROJECT_STORE_KEY = "state"

# Cap on how much of the first user message we keep as the mission line.
_MISSION_MAX_CHARS = 280


class ProjectState(TypedDict):
    """The persisted project spine (one record per Project in the store).

    ``completed`` is keyed by *category* (e.g. ``"sources"``, ``"theories"``)
    so a later turn refreshes a category's count in place ("Gathered 3
    sources" → "Gathered 5 sources") instead of accumulating stale lines. It is
    rendered to a flat ``list[str]`` in the frontend payload. ``plan`` is the
    opt-in autonomous run-loop plan (P5): ``None`` until the ``research_planner``
    authors one, then the human-accepted/edited plan persists here.
    """

    mission: str
    completed: dict[str, str]
    pending: list[str]
    plan: ResearchPlanPayload | None


class ProjectSuggestion(NamedTuple):
    """A single advisory next step.

    ``action`` names the subagent that would run it; ``prompt`` is the ready-to-
    send message that *promotes* the suggestion — the frontend drops it into the
    composer for the user to review and send (P3.2). It is phrased "Use the
    <subagent> subagent to …" to match the composer's slash-command routing.
    Advisory only: filling the composer never sends the message.
    """

    title: str
    rationale: str
    action: str
    prompt: str


def empty_project() -> ProjectState:
    return {"mission": "", "completed": {}, "pending": [], "plan": None}


# ---------------------------------------------------------------------------
# Mission seeding
# ---------------------------------------------------------------------------

def _message_text(message: Any) -> str:
    """Best-effort plain text of a LangChain/​dict message content field."""
    content = getattr(message, "content", None)
    if content is None and isinstance(message, dict):
        content = message.get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for block in content:
            if isinstance(block, str):
                parts.append(block)
            elif isinstance(block, dict) and isinstance(block.get("text"), str):
                parts.append(block["text"])
        return "\n".join(parts)
    return ""


def _message_role(message: Any) -> str:
    role = getattr(message, "type", None)
    if role is None and isinstance(message, dict):
        role = message.get("type") or message.get("role")
    return str(role or "")


def _strip_attached_files_blockquote(text: str) -> str:
    """Drop the leading "> Attached files …" blockquote the frontend prepends."""
    lines = [line for line in text.splitlines() if not line.lstrip().startswith(">")]
    return "\n".join(lines)


def first_user_goal(messages: list[Any]) -> str | None:
    """The first human message's text, cleaned up, as the mission seed."""
    for message in messages:
        if _message_role(message) not in ("human", "user"):
            continue
        text = _strip_attached_files_blockquote(_message_text(message)).strip()
        text = re.sub(r"\s+", " ", text)
        if not text:
            continue
        if len(text) > _MISSION_MAX_CHARS:
            text = text[: _MISSION_MAX_CHARS - 1].rstrip() + "…"
        return text
    return None


# ---------------------------------------------------------------------------
# Artifact inspection
# ---------------------------------------------------------------------------

def _has_visualizations(files: list[dict[str, Any]]) -> bool:
    """True if any produced file is an image (a plot from EDA/diagnostic/etc.).

    Images are the cleanest signal that analysis has actually run against data:
    the analysis subagents emit plots, whereas user *uploads* are almost always
    data files (CSV/XLSX/PDF), so this avoids treating an uploaded dataset as
    "analysis already done".
    """
    return any(
        str((f or {}).get("media_type") or "").startswith("image/") for f in files
    )


def _theory_count(hypotheses: list[dict[str, Any]]) -> int:
    return sum(len((h or {}).get("theories") or []) for h in hypotheses)


def _library_indexed(libraries: list[dict[str, Any]]) -> int:
    return max((int((lib or {}).get("paper_count") or 0) for lib in libraries), default=0)


def summarize_completed(artifacts: dict[str, Any]) -> dict[str, str]:
    """Category-keyed, artifact-grounded summary of what the project has done."""
    datasets = artifacts.get("datasets") or []
    sources = artifacts.get("sources") or []
    reports = artifacts.get("reports") or []
    files = artifacts.get("files") or []
    hypotheses = artifacts.get("hypotheses") or []
    libraries = artifacts.get("libraries") or []
    analyses = artifacts.get("analyses") or []

    completed: dict[str, str] = {}

    if sources:
        completed["sources"] = (
            f"Gathered {len(sources)} academic source"
            f"{'' if len(sources) == 1 else 's'} from the literature."
        )
    if datasets:
        completed["datasets"] = (
            f"Identified {len(datasets)} candidate dataset"
            f"{'' if len(datasets) == 1 else 's'} in CIP Dataverse."
        )
    theories = _theory_count(hypotheses)
    if theories:
        completed["theories"] = (
            f"Generated {theories} literature-grounded "
            f"theor{'y' if theories == 1 else 'ies'}."
        )
    indexed = _library_indexed(libraries)
    if indexed:
        completed["library"] = (
            f"Indexed {indexed} document{'' if indexed == 1 else 's'} "
            "into the local library."
        )
    image_files = [f for f in files if str((f or {}).get("media_type") or "").startswith("image/")]
    if image_files:
        completed["visualizations"] = (
            f"Produced {len(image_files)} visualization"
            f"{'' if len(image_files) == 1 else 's'} from the data."
        )
    completed_analyses = [a for a in analyses if (a or {}).get("status") == "completed"]
    if completed_analyses:
        completed["analysis"] = (
            f"Tested hypotheses against the data in {len(completed_analyses)} "
            f"DataVoyager run{'' if len(completed_analyses) == 1 else 's'}."
        )
    if reports:
        title = (reports[-1] or {}).get("title") or "a report"
        completed["report"] = f"Wrote report: “{title}”."

    return completed


def derive_suggestions(artifacts: dict[str, Any]) -> list[ProjectSuggestion]:
    """Up to 3 advisory next steps, grounded in gaps between the artifacts.

    Ordered by priority; only the first three are returned. Every suggestion is
    text the user reads — none of them run anything.
    """
    sources = artifacts.get("sources") or []
    datasets = artifacts.get("datasets") or []
    reports = artifacts.get("reports") or []
    files = artifacts.get("files") or []
    hypotheses = artifacts.get("hypotheses") or []
    libraries = artifacts.get("libraries") or []
    analyses = artifacts.get("analyses") or []

    has_sources = bool(sources)
    has_datasets = bool(datasets)
    has_report = bool(reports)
    has_theories = _theory_count(hypotheses) > 0
    has_library = _library_indexed(libraries) > 0
    # A completed DataVoyager run is the strongest "analysis ran" signal; its
    # charts also land as image files, but the artifact itself is authoritative.
    has_analysis = _has_visualizations(files) or any(
        (a or {}).get("status") == "completed" for a in analyses
    )

    suggestions: list[ProjectSuggestion] = []

    if has_theories and not has_analysis:
        suggestions.append(
            ProjectSuggestion(
                title="Test your theories against data",
                rationale=(
                    "You have literature-grounded theories but no analysis has "
                    "been run against a dataset yet. DataVoyager can generate and "
                    "test them against your data, closing the loop from theory to "
                    "evidence."
                ),
                action="DataVoyager",
                prompt=(
                    "Use the data_voyager subagent to test the theories generated "
                    "so far against my dataset and report which ones the evidence "
                    "supports."
                ),
            )
        )
    if has_report and not has_sources:
        suggestions.append(
            ProjectSuggestion(
                title="Ground the report in sources",
                rationale=(
                    "A report exists but no academic sources back it. A "
                    "literature search would let you cite its claims."
                ),
                action="Academic Research",
                prompt=(
                    "Use the academic_researcher subagent to find academic "
                    "sources that support the claims in the report, then list "
                    "them with citations."
                ),
            )
        )
    if has_datasets and not has_analysis:
        suggestions.append(
            ProjectSuggestion(
                title="Explore the recommended dataset",
                rationale=(
                    "Candidate datasets were found but not yet analyzed. "
                    "Profiling one shows what is actually in it."
                ),
                action="Exploratory Data Analysis",
                prompt=(
                    "Use the exploratory_data_analysis subagent to profile the "
                    "recommended dataset and summarize what is in it."
                ),
            )
        )
    if has_sources and not has_theories:
        suggestions.append(
            ProjectSuggestion(
                title="Synthesize theories from the literature",
                rationale=(
                    "You have gathered sources but not yet synthesized "
                    "mechanistic or causal theories from them."
                ),
                action="Hypothesis Generator",
                prompt=(
                    "Use the hypothesis_generator subagent to synthesize "
                    "literature-grounded theories from the sources gathered so "
                    "far."
                ),
            )
        )
    if has_theories and not has_library:
        suggestions.append(
            ProjectSuggestion(
                title="Pull the full text behind your theories",
                rationale=(
                    "Your theories cite supporting papers, but their full text "
                    "is not in your library yet — indexing them lets you read "
                    "and quote the actual evidence."
                ),
                action="PDF Librarian",
                prompt=(
                    "Use the pdf_librarian subagent to pull and index the full "
                    "text of the papers supporting the generated theories."
                ),
            )
        )
    if has_analysis and not has_report:
        suggestions.append(
            ProjectSuggestion(
                title="Write up the analysis",
                rationale=(
                    "You have produced analysis outputs but not yet compiled "
                    "them into a shareable report."
                ),
                action="Report Writer",
                prompt=(
                    "Use the report_writer subagent to write a markdown report "
                    "of the analysis and findings so far."
                ),
            )
        )

    return suggestions[:3]


# ---------------------------------------------------------------------------
# State transition + payload
# ---------------------------------------------------------------------------

def _fold_plan(
    prev_plan: ResearchPlanPayload | None,
    carrier: Any,
) -> ResearchPlanPayload | None:
    """Decide the plan to persist this turn.

    The ``research_planner`` emits its output into the ``plan`` artifact carrier,
    stamped with a fresh ``nonce`` per generation. That carrier then lingers in
    checkpoint state on every later turn, so folding it blindly would keep
    resetting the user's accepted/edited plan to ``proposed``. We therefore only
    adopt the carrier when its ``nonce`` differs from the plan we already hold —
    i.e. the planner really ran again — and otherwise preserve the stored plan.
    """
    prev_nonce = (prev_plan or {}).get("nonce")
    carrier_nonce = (carrier or {}).get("nonce") if isinstance(carrier, dict) else None
    if carrier_nonce and carrier_nonce != prev_nonce:
        return plan_from_output(carrier, nonce=str(carrier_nonce))
    return coerce_plan(prev_plan) if prev_plan else None


def advance_project(
    prev: ProjectState,
    artifacts: dict[str, Any],
    messages: list[Any],
) -> tuple[ProjectState, list[ProjectSuggestion]]:
    """Fold this turn's artifacts + messages into the persistent project state.

    Returns the updated state (to persist) and the advisory suggestions (to
    surface this turn). Pure: no store, no side effects.
    """
    mission = (prev.get("mission") or "").strip()
    if not mission:
        mission = first_user_goal(messages) or ""

    # Preserve completed items from prior threads (they live in the store);
    # only refresh the categories this thread's artifacts speak to.
    completed: dict[str, str] = dict(prev.get("completed") or {})
    completed.update(summarize_completed(artifacts))

    suggestions = derive_suggestions(artifacts)

    new_state: ProjectState = {
        "mission": mission,
        "completed": completed,
        # ``pending`` is the user-curated backlog (P3.3) — it is owned by the
        # user, so a run never overwrites it. The per-turn advisory
        # ``suggestions`` are returned separately and surfaced live; they are
        # NOT persisted into ``pending`` (that was the phase-1 behaviour, which
        # would have clobbered hand-added items every run).
        "pending": list(prev.get("pending") or []),
        # The run-loop plan (P5) is likewise user-owned: preserve the accepted /
        # edited plan across turns, and only replace it when the planner just
        # authored a genuinely new one (distinguished by ``nonce``).
        "plan": _fold_plan(prev.get("plan"), artifacts.get("plan")),
    }
    return new_state, suggestions


# ---------------------------------------------------------------------------
# Hand-edits (P3.3): apply a single edit op to the persisted project state
# ---------------------------------------------------------------------------

class ProjectEdit(TypedDict, total=False):
    """One hand-edit from the user (via the ``/project`` route).

    All fields optional; a request may carry more than one. ``mission`` replaces
    the mission (empty string clears it). ``pending_add`` appends a backlog
    item; ``pending_remove`` drops one by exact text; ``complete`` moves a
    pending item into Completed Work.
    """

    mission: str
    pending_add: str
    pending_remove: str
    complete: str


_MANUAL_COMPLETED_PREFIX = "manual:"


def apply_project_edit(state: ProjectState, edit: ProjectEdit) -> ProjectState:
    """Return a new ProjectState with the edit applied. Pure; store-agnostic.

    Unknown keys are ignored. Duplicate/empty additions are no-ops so repeated
    clicks are idempotent.
    """
    mission = state.get("mission") or ""
    completed: dict[str, str] = dict(state.get("completed") or {})
    pending: list[str] = list(state.get("pending") or [])

    if "mission" in edit:
        mission = re.sub(r"\s+", " ", str(edit["mission"]).strip())

    add = (edit.get("pending_add") or "").strip()
    if add and add not in pending:
        pending.append(add)

    remove = edit.get("pending_remove")
    if remove is not None:
        pending = [item for item in pending if item != remove]

    done = (edit.get("complete") or "").strip()
    if done:
        pending = [item for item in pending if item != done]
        # Keyed by text so completing the same item twice is idempotent and it
        # never collides with the auto category keys (sources/theories/…).
        completed[f"{_MANUAL_COMPLETED_PREFIX}{done}"] = done

    # The plan is edited through its own op set (``apply_plan_edit``); a spine
    # edit leaves it untouched.
    return {
        "mission": mission,
        "completed": completed,
        "pending": pending,
        "plan": state.get("plan"),
    }


def render_mission_context(state: ProjectState) -> str | None:
    """Render the active project's mission (+ progress) as a system-prompt block.

    The spine payload built by :func:`build_project_payload` only ever reaches the
    *frontend*; nothing there feeds the coordinator's model. So the mission the
    user sets is displayed but never *read* by the agent — editing it changes what
    is shown, not how the agent behaves. This function produces a short block that
    ``ProjectSpineMiddleware`` appends to the coordinator's system prompt each turn
    so the mission actually grounds the model's answers, delegations, and the plans
    the ``research_planner`` authors.

    Returns ``None`` when there is no mission, so an empty spine injects nothing
    (and the large static coordinator prompt stays a stable, cacheable prefix).
    """
    mission = (state.get("mission") or "").strip()
    if not mission:
        return None

    lines = ["## Active research project", "", f"Mission: {mission}"]

    completed = [c for c in (state.get("completed") or {}).values() if c]
    if completed:
        lines.append("")
        lines.append("Completed so far:")
        lines.extend(f"- {c}" for c in completed)

    pending = [p for p in (state.get("pending") or []) if p]
    if pending:
        lines.append("")
        lines.append("Pending work (user-curated backlog):")
        lines.extend(f"- {p}" for p in pending)

    lines.append("")
    lines.append(
        "Ground every answer, plan, and delegation in this mission. When you "
        "delegate to a subagent, write a concrete task derived from this mission "
        "— state the actual research subject, never refer to “the project "
        "mission” in the abstract. When you invoke the research_planner, pass "
        "it this mission verbatim as the goal so the plan and each step prompt name "
        "the real subject."
    )
    return "\n".join(lines)


def build_project_payload(
    state: ProjectState,
    suggestions: list[ProjectSuggestion],
) -> ProjectArtifactPayload:
    """Shape the state + suggestions into the frontend artifact payload."""
    suggestion_payloads: list[ProjectSuggestionPayload] = [
        {"title": s.title, "rationale": s.rationale, "action": s.action, "prompt": s.prompt}
        for s in suggestions
    ]
    return {
        "mission": state.get("mission") or "",
        "completed": list((state.get("completed") or {}).values()),
        "pending": list(state.get("pending") or []),
        "suggestions": suggestion_payloads,
        "plan": state.get("plan"),
    }


def has_content(payload: ProjectArtifactPayload) -> bool:
    """True if there is anything worth rendering (avoids an empty panel)."""
    return bool(
        payload.get("mission")
        or payload.get("completed")
        or payload.get("pending")
        or payload.get("suggestions")
        or has_plan_content(payload.get("plan"))
    )


# ---------------------------------------------------------------------------
# Store IO
# ---------------------------------------------------------------------------

def _coerce_state(value: Any) -> ProjectState:
    """Normalize a stored record (tolerating older/partial shapes)."""
    if not isinstance(value, dict):
        return empty_project()
    completed = value.get("completed")
    if isinstance(completed, list):
        # Legacy/defensive: a flat list becomes an index-keyed map.
        completed = {str(i): item for i, item in enumerate(completed) if isinstance(item, str)}
    elif not isinstance(completed, dict):
        completed = {}
    pending = value.get("pending")
    if not isinstance(pending, list):
        pending = []
    return {
        "mission": str(value.get("mission") or ""),
        "completed": completed,
        "pending": [str(p) for p in pending],
        "plan": coerce_plan(value.get("plan")),
    }


async def load_project(store: BaseStore, namespace: tuple[str, ...]) -> ProjectState:
    item = await store.aget(namespace, PROJECT_STORE_KEY)
    if item is None:
        return empty_project()
    return _coerce_state(item.value)


async def save_project(
    store: BaseStore,
    namespace: tuple[str, ...],
    state: ProjectState,
) -> None:
    await store.aput(namespace, PROJECT_STORE_KEY, dict(state))
