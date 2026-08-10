"""Tests for the research-project spine (`backend.project`), work item C phase 1.

These pin the *advisory* contract: the completed-work summary and the suggested
next steps are derived deterministically from the structured artifacts, the
mission is seeded from the first user message and is sticky across threads, and
the persisted state round-trips through a LangGraph store namespace. Nothing
here executes a subagent — suggestions are text only.
"""

from __future__ import annotations

import asyncio

from langgraph.store.memory import InMemoryStore

from backend.project import (
    PROJECT_STORE_KEY,
    advance_project,
    apply_project_edit,
    build_project_payload,
    derive_suggestions,
    empty_project,
    first_user_goal,
    has_content,
    load_project,
    render_mission_context,
    save_project,
    summarize_completed,
)
from backend.runtime import _project_namespace
from backend.schemas import _merge_artifacts


# ---------------------------------------------------------------------------
# Small fixtures
# ---------------------------------------------------------------------------

def _hyp(n_theories: int) -> dict:
    return {"question": "q", "theories": [{"laws": ["x→y"]} for _ in range(n_theories)]}


def _human(text: str) -> dict:
    return {"type": "human", "content": text}


def _ai(text: str) -> dict:
    return {"type": "ai", "content": text}


IMG = {"name": "eda.png", "path": "./eda.png", "media_type": "image/png"}
CSV = {"name": "data.csv", "path": "./data.csv", "media_type": "text/csv"}
DV_DONE = {"question": "What drives yield?", "status": "completed"}
DV_RUNNING = {"question": "What drives yield?", "status": "running"}


# ---------------------------------------------------------------------------
# Mission seeding
# ---------------------------------------------------------------------------

def test_first_user_goal_uses_first_human_message() -> None:
    messages = [_ai("hi"), _human("Why do potato yields drop in drought?"), _human("second")]
    assert first_user_goal(messages) == "Why do potato yields drop in drought?"


def test_first_user_goal_strips_attached_files_blockquote() -> None:
    text = "> Attached files (already saved): `./data.csv`\nAnalyze this dataset"
    assert first_user_goal([_human(text)]) == "Analyze this dataset"


def test_first_user_goal_none_when_no_human_message() -> None:
    assert first_user_goal([_ai("only assistant")]) is None
    assert first_user_goal([]) is None


def test_first_user_goal_truncates_long_text() -> None:
    goal = first_user_goal([_human("word " * 200)])
    assert goal is not None
    assert len(goal) <= 280
    assert goal.endswith("…")


# ---------------------------------------------------------------------------
# Completed-work summary (artifact-grounded, category-keyed)
# ---------------------------------------------------------------------------

def test_summarize_completed_counts_each_category() -> None:
    artifacts = {
        "sources": [{"citation": "a"}, {"citation": "b"}],
        "datasets": [{"title": "d"}],
        "hypotheses": [_hyp(3)],
        "libraries": [{"index_path": ".asta/documents", "paper_count": 4}],
        "files": [IMG, CSV],
        "reports": [{"title": "Findings", "markdown": "# x"}],
    }
    completed = summarize_completed(artifacts)
    assert "2 academic sources" in completed["sources"]
    assert "1 candidate dataset" in completed["datasets"]
    assert "3 literature-grounded theories" in completed["theories"]
    assert "4 documents" in completed["library"]
    assert "1 visualization" in completed["visualizations"]  # only the image
    assert "Findings" in completed["report"]


def test_summarize_completed_empty_for_no_artifacts() -> None:
    assert summarize_completed({}) == {}


def test_uploaded_csv_is_not_counted_as_analysis() -> None:
    # A user-uploaded CSV must not read as "analysis already done".
    assert "visualizations" not in summarize_completed({"files": [CSV]})


# ---------------------------------------------------------------------------
# Advisory suggestions
# ---------------------------------------------------------------------------

def test_theories_without_data_test_suggests_datavoyager() -> None:
    suggestions = derive_suggestions({"hypotheses": [_hyp(2)]})
    titles = [s.title for s in suggestions]
    assert "Test your theories against data" in titles
    # P2: the "test against data" nudge now routes to DataVoyager (the real
    # theory→data loop), not Diagnostic Analytics as a proxy.
    dv = next(s for s in suggestions if s.title == "Test your theories against data")
    assert dv.action == "DataVoyager"
    assert "data_voyager" in dv.prompt


def test_every_suggestion_carries_a_routable_prompt() -> None:
    # P3.2: each suggestion ships a ready-to-send prompt that names the
    # subagent (matches the composer's "Use the <name> subagent to ..." routing)
    # so promoting it drops a routable message into the composer.
    artifacts = {
        "hypotheses": [_hyp(1)],
        "reports": [{"title": "r", "markdown": "x"}],
        "datasets": [{"title": "d"}],
        "sources": [{"citation": "a"}],
        "files": [CSV],
    }
    for suggestion in derive_suggestions(artifacts):
        assert suggestion.prompt.strip(), f"{suggestion.title} has no prompt"
        assert "subagent" in suggestion.prompt.lower()


def test_report_without_sources_suggests_academic_research() -> None:
    suggestions = derive_suggestions({"reports": [{"title": "r", "markdown": "x"}]})
    assert any(s.action == "Academic Research" for s in suggestions)


def test_sources_without_theories_suggests_hypothesis_generator() -> None:
    suggestions = derive_suggestions({"sources": [{"citation": "a"}]})
    assert any(s.action == "Hypothesis Generator" for s in suggestions)


def test_datasets_without_analysis_suggests_eda() -> None:
    suggestions = derive_suggestions({"datasets": [{"title": "d"}]})
    assert any(s.action == "Exploratory Data Analysis" for s in suggestions)


def test_theories_with_visualizations_does_not_suggest_data_test() -> None:
    # If analysis (an image) already ran, drop the "test against data" nudge.
    suggestions = derive_suggestions({"hypotheses": [_hyp(1)], "files": [IMG]})
    assert all(s.title != "Test your theories against data" for s in suggestions)


def test_completed_analysis_clears_test_against_data_nudge() -> None:
    # A completed DataVoyager run is a first-class "analysis ran" signal even
    # without an image file, so it clears the "test theories against data" nudge.
    suggestions = derive_suggestions({"hypotheses": [_hyp(1)], "analyses": [DV_DONE]})
    assert all(s.title != "Test your theories against data" for s in suggestions)


def test_running_analysis_does_not_clear_the_nudge() -> None:
    # A still-running run has produced no evidence yet, so the nudge stays.
    suggestions = derive_suggestions({"hypotheses": [_hyp(1)], "analyses": [DV_RUNNING]})
    assert any(s.title == "Test your theories against data" for s in suggestions)


def test_summarize_completed_counts_completed_analyses() -> None:
    completed = summarize_completed({"analyses": [DV_DONE]})
    assert "1 DataVoyager run" in completed["analysis"]
    # A still-running run is not counted as completed work.
    assert "analysis" not in summarize_completed({"analyses": [DV_RUNNING]})


def test_suggestions_capped_at_three() -> None:
    # An artifact set that trips many rules still yields at most 3.
    artifacts = {
        "hypotheses": [_hyp(1)],  # theories → data test + pull full text
        "reports": [{"title": "r", "markdown": "x"}],  # report + no sources
        "datasets": [{"title": "d"}],  # dataset + no analysis
    }
    assert len(derive_suggestions(artifacts)) == 3


def test_no_artifacts_no_suggestions() -> None:
    assert derive_suggestions({}) == []


# ---------------------------------------------------------------------------
# advance_project: mission stickiness + completed merge across threads
# ---------------------------------------------------------------------------

def test_advance_seeds_mission_once_and_keeps_it_sticky() -> None:
    state1, _ = advance_project(empty_project(), {}, [_human("Study drought stress")])
    assert state1["mission"] == "Study drought stress"
    # A later turn (even in another thread with a different first message) does
    # not overwrite the established mission.
    state2, _ = advance_project(state1, {"sources": [{"citation": "a"}]}, [_human("unrelated")])
    assert state2["mission"] == "Study drought stress"


def test_advance_refreshes_category_count_in_place() -> None:
    state1, _ = advance_project(empty_project(), {"sources": [{"citation": "a"}]}, [])
    state2, _ = advance_project(
        state1, {"sources": [{"citation": "a"}, {"citation": "b"}]}, []
    )
    # The sources line is updated to 2, not duplicated as "1" + "2".
    source_lines = [c for c in state2["completed"].values() if "academic source" in c]
    assert len(source_lines) == 1
    assert "2 academic sources" in source_lines[0]


def test_advance_preserves_prior_thread_completed_when_artifacts_empty() -> None:
    # New thread starts with empty artifacts; prior completed work must persist.
    prior = empty_project()
    prior["completed"] = {"sources": "Gathered 5 academic sources from the literature."}
    state, _ = advance_project(prior, {}, [_human("new thread")])
    assert state["completed"]["sources"].startswith("Gathered 5")


def test_advance_does_not_clobber_user_pending() -> None:
    # P3.3: the user-curated backlog is owned by the user — a run must not
    # overwrite it with the auto-derived suggestion titles.
    prior = empty_project()
    prior["pending"] = ["My hand-added task"]
    state, suggestions = advance_project(prior, {"hypotheses": [_hyp(1)]}, [])
    assert state["pending"] == ["My hand-added task"]
    # Suggestions are still produced, just kept separate from pending.
    assert suggestions and all(s.title != "" for s in suggestions)


# ---------------------------------------------------------------------------
# Payload shape + has_content
# ---------------------------------------------------------------------------

def test_build_payload_flattens_completed_to_list() -> None:
    state, suggestions = advance_project(
        empty_project(), {"sources": [{"citation": "a"}], "hypotheses": [_hyp(1)]},
        [_human("goal")],
    )
    payload = build_project_payload(state, suggestions)
    assert payload["mission"] == "goal"
    assert isinstance(payload["completed"], list)
    assert all(isinstance(c, str) for c in payload["completed"])
    assert payload["suggestions"] and set(payload["suggestions"][0]) == {
        "title",
        "rationale",
        "action",
        "prompt",
    }


def test_has_content_false_for_empty_project() -> None:
    payload = build_project_payload(empty_project(), [])
    assert has_content(payload) is False


# ---------------------------------------------------------------------------
# Store round-trip (cross-thread persistence)
# ---------------------------------------------------------------------------

def test_store_round_trip() -> None:
    async def _run() -> None:
        store = InMemoryStore()
        namespace = _project_namespace("user-y", "proj-1")
        assert (await load_project(store, namespace)) == empty_project()

        state, _ = advance_project(empty_project(), {"sources": [{"citation": "a"}]}, [_human("m")])
        await save_project(store, namespace, state)

        reloaded = await load_project(store, namespace)
        assert reloaded["mission"] == "m"
        assert reloaded["completed"] == state["completed"]
        # Stored under the documented key.
        item = await store.aget(namespace, PROJECT_STORE_KEY)
        assert item is not None

    asyncio.run(_run())


# ---------------------------------------------------------------------------
# Reducer integration: the project slice is last-write-wins, not accumulating
# ---------------------------------------------------------------------------

def test_merge_keeps_newest_project_and_preserves_other_slices() -> None:
    left = {
        "datasets": [], "sources": [], "reports": [], "files": [],
        "hypotheses": [_hyp(1)],
        "project": {"mission": "old", "completed": [], "pending": [], "suggestions": []},
    }
    right = {
        "datasets": [], "sources": [], "reports": [], "files": [],
        "project": {"mission": "new", "completed": ["did x"], "pending": [], "suggestions": []},
    }
    merged = _merge_artifacts(left, right)
    # Newest project wins…
    assert merged["project"]["mission"] == "new"
    assert merged["project"]["completed"] == ["did x"]
    # …without clobbering the accumulated hypotheses from the left side.
    assert len(merged["hypotheses"]) == 1


def test_merge_carries_project_when_only_one_side_has_it() -> None:
    left = {"datasets": [], "sources": [], "reports": [], "files": []}
    right = {
        "datasets": [], "sources": [], "reports": [], "files": [],
        "project": {"mission": "m", "completed": [], "pending": [], "suggestions": []},
    }
    assert _merge_artifacts(left, right)["project"]["mission"] == "m"
    assert _merge_artifacts(right, left)["project"]["mission"] == "m"
    # No project on either side → key stays absent.
    assert "project" not in _merge_artifacts(left, left)


def test_load_coerces_legacy_list_completed() -> None:
    async def _run() -> None:
        store = InMemoryStore()
        namespace = _project_namespace("b", "proj-1")
        # Simulate an older/partial record where completed was a flat list.
        await store.aput(namespace, PROJECT_STORE_KEY, {"mission": "m", "completed": ["did x"]})
        state = await load_project(store, namespace)
        assert isinstance(state["completed"], dict)
        assert "did x" in state["completed"].values()
        assert state["pending"] == []

    asyncio.run(_run())


# ---------------------------------------------------------------------------
# Namespace is user- and project-scoped (route-reproducible, P3.3 + P5)
# ---------------------------------------------------------------------------

def test_project_namespace_is_user_and_project_scoped() -> None:
    # user_id first (matches @auth.on.store) and NOT keyed by assistant_id, so a
    # custom route can reproduce it from request.user.identity + project id.
    # The trailing project id (P5) scopes each named Project's spine separately.
    assert _project_namespace("user-1", "proj-9") == ("user-1", "project", "proj-9")
    # Different projects for the same user never collide.
    assert _project_namespace("user-1", "a") != _project_namespace("user-1", "b")


# ---------------------------------------------------------------------------
# Hand-edits (P3.3)
# ---------------------------------------------------------------------------

def test_edit_sets_and_normalizes_mission() -> None:
    state = apply_project_edit(empty_project(), {"mission": "  study   drought  "})
    assert state["mission"] == "study drought"


def test_edit_clears_mission_with_empty_string() -> None:
    prior = {"mission": "old", "completed": {}, "pending": []}
    assert apply_project_edit(prior, {"mission": ""})["mission"] == ""


def test_edit_adds_pending_and_is_idempotent() -> None:
    s1 = apply_project_edit(empty_project(), {"pending_add": "Do X"})
    assert s1["pending"] == ["Do X"]
    s2 = apply_project_edit(s1, {"pending_add": "Do X"})  # duplicate → no-op
    assert s2["pending"] == ["Do X"]


def test_edit_removes_pending_by_exact_text() -> None:
    prior = {"mission": "", "completed": {}, "pending": ["A", "B"]}
    assert apply_project_edit(prior, {"pending_remove": "A"})["pending"] == ["B"]


def test_edit_complete_moves_pending_into_completed() -> None:
    prior = {"mission": "", "completed": {}, "pending": ["Ship it"]}
    state = apply_project_edit(prior, {"complete": "Ship it"})
    assert "Ship it" not in state["pending"]
    assert "Ship it" in state["completed"].values()
    # Manual completions never collide with the auto category keys.
    assert all(k.startswith("manual:") for k in state["completed"])


def test_edit_does_not_mutate_input() -> None:
    prior = {"mission": "m", "completed": {}, "pending": ["keep"]}
    apply_project_edit(prior, {"pending_add": "new", "mission": "changed"})
    assert prior == {"mission": "m", "completed": {}, "pending": ["keep"]}


def test_edit_survives_a_run_then_refresh() -> None:
    # A hand-added backlog item + a run's artifacts coexist: the run refreshes
    # completed/suggestions but leaves the user's pending intact.
    edited = apply_project_edit(empty_project(), {"pending_add": "Read paper 3"})
    after_run, suggestions = advance_project(edited, {"sources": [{"citation": "a"}]}, [])
    assert "Read paper 3" in after_run["pending"]
    assert "sources" in after_run["completed"]
    assert isinstance(suggestions, list)


# ---------------------------------------------------------------------------
# Mission → model context (the fix for "the agent ignores my mission")
# ---------------------------------------------------------------------------

def test_render_mission_context_empty_returns_none() -> None:
    assert render_mission_context(empty_project()) is None
    only_progress = {"mission": "  ", "completed": {"sources": "Gathered 1."}, "pending": [], "plan": None}
    # Whitespace-only mission is treated as absent — nothing to ground on.
    assert render_mission_context(only_progress) is None


def test_render_mission_context_includes_mission_and_grounding() -> None:
    state = empty_project()
    state["mission"] = "Whether coffea canephora gave heat-shock resistance to arabica."
    block = render_mission_context(state)
    assert block is not None
    assert "coffea canephora" in block
    assert block.strip().startswith("## Active research project")
    # The grounding instruction is what stops the model from inventing a topic
    # and from leaving "the project mission" as an unresolved placeholder.
    assert "research_planner" in block
    assert "in the abstract" in block


def test_render_mission_context_lists_completed_and_pending() -> None:
    state = empty_project()
    state["mission"] = "m"
    state["completed"] = {"sources": "Gathered 3 sources."}
    state["pending"] = ["Profile the recommended dataset"]
    block = render_mission_context(state) or ""
    assert "Gathered 3 sources." in block
    assert "Profile the recommended dataset" in block


def test_spine_middleware_injects_mission_into_system_prompt(monkeypatch) -> None:
    """End-to-end: the persisted mission is appended to the coordinator prompt.

    Regression guard for the reported bug — before this, the mission reached only
    the frontend, so the agent never read it. We stub namespace resolution (its
    own contract is covered in test_projects.py) and assert the mission text lands
    in the system prompt the model would actually receive.
    """
    from types import SimpleNamespace

    from langchain.agents.middleware import ModelRequest

    import backend.middleware.project as mw

    async def _t() -> None:
        store = InMemoryStore()
        namespace = ("u1", "project", "p1")
        state = empty_project()
        state["mission"] = "Test mission about coffea arabica heat shock."
        await save_project(store, namespace, state)

        async def _fixed_ns(_runtime):
            return namespace

        monkeypatch.setattr(mw, "_active_project_namespace", _fixed_ns)

        captured: dict[str, str | None] = {}

        async def _handler(req):
            captured["system_prompt"] = req.system_prompt
            return "ok"

        request = ModelRequest(
            model=object(),
            messages=[],
            system_prompt="STATIC COORDINATOR PROMPT",
            runtime=SimpleNamespace(store=store),
        )
        await mw.ProjectSpineMiddleware().awrap_model_call(request, _handler)

        sp = captured["system_prompt"] or ""
        assert "STATIC COORDINATOR PROMPT" in sp  # static prefix preserved
        assert "Test mission about coffea arabica heat shock." in sp

    asyncio.run(_t())


def test_spine_middleware_passthrough_when_no_mission(monkeypatch) -> None:
    from types import SimpleNamespace

    from langchain.agents.middleware import ModelRequest

    import backend.middleware.project as mw

    async def _t() -> None:
        store = InMemoryStore()
        namespace = ("u1", "project", "empty")
        await save_project(store, namespace, empty_project())

        async def _fixed_ns(_runtime):
            return namespace

        monkeypatch.setattr(mw, "_active_project_namespace", _fixed_ns)

        captured: dict[str, str | None] = {}

        async def _handler(req):
            captured["system_prompt"] = req.system_prompt
            return "ok"

        request = ModelRequest(
            model=object(),
            messages=[],
            system_prompt="STATIC ONLY",
            runtime=SimpleNamespace(store=store),
        )
        await mw.ProjectSpineMiddleware().awrap_model_call(request, _handler)
        # No mission ⇒ untouched.
        assert captured["system_prompt"] == "STATIC ONLY"

    asyncio.run(_t())
