"""The path from a drafted run to the thing the approval modal reads.

§254: every piece of this existed and the modal still never opened. The schema, the artifact
payload, the bundle slice, the reducer key, the Rust decoder and the panel bucket were all built
and wired — and `ArtifactCaptureMiddleware` has an explicit `if self.source == …` branch per
subagent, with no branch for `autodiscovery`. So a run drafted successfully, reported "Status:
awaiting approval", and put nothing anywhere the app looks.

These tests walk the whole path in one go, which is the only kind that could have caught it: a
structured response in, and the exact key the frontend filters on out.
"""

from __future__ import annotations

import re

from backend.middleware.artifacts import ArtifactCaptureMiddleware
from backend.schemas import (
    DiscoveryRunResults,
    _merge_artifacts,
    artifact_node_id,
)


def _captured(result: DiscoveryRunResults):
    update = ArtifactCaptureMiddleware("autodiscovery").after_agent(
        {"structured_response": result, "messages": []}, None
    )
    return (update or {}).get("artifacts") or {}


def test_a_drafted_run_reaches_the_bucket_the_modal_filters_on():
    """`decode_drafts` reads `artifacts.discoveries` and keeps `status == "awaiting_approval"`.
    If either the key or the status is wrong, nothing appears and nothing says why."""
    captured = _captured(
        DiscoveryRunResults(
            name="Testing functionalities - exploratory soil organic carbon covariates",
            run_id="f31c50d1-a358-49a0-8e79-62e891acd2db",
            domain="Soil science",
            intent="open-ended exploration of structure and signal",
            dataset_paths=["./SOC_Covariables_TrainValV5.csv", "./SOC_Covariables_TESTV5.csv"],
            n_experiments=15,
        )
    )
    assert "discoveries" in captured, "the key the frontend reads"
    drafted = captured["discoveries"][0]
    assert drafted["status"] == "awaiting_approval", "the status the frontend filters on"
    assert drafted["run_id"] == "f31c50d1-a358-49a0-8e79-62e891acd2db"
    assert drafted["n_experiments"] == 15
    assert len(drafted["dataset_paths"]) == 2
    # The one field the modal makes editable, so it has to survive the trip.
    assert "open-ended exploration" in drafted["intent"]


def test_the_run_id_is_a_real_provenance_node():
    """`_NODE_ID_FIELDS` had no `discovery` entry, so every edge for a run was keyed
    `discovery:` — one phantom node that every run would have collapsed onto."""
    assert artifact_node_id("discovery", {"run_id": "abc"}) == "discovery:abc"
    captured = _captured(
        DiscoveryRunResults(
            name="run",
            run_id="abc",
            dataset_paths=["./a.csv"],
            derived_from=[{"kind": "dataset", "ref": "./a.csv"}],
        )
    )
    edges = captured["edges"]
    assert edges, "a produced-by edge and a declared one"
    assert all(e["source"] == "discovery:abc" or e["target"] == "discovery:abc" for e in edges)


def test_a_failed_draft_is_still_visible_somewhere():
    """It has no run id because it never got one — and §249 added this bucket precisely so the
    reason lands somewhere other than prose a model may compress."""
    captured = _captured(
        DiscoveryRunResults(
            name="attempted",
            status="failed",
            note="none of those files exist in the sandbox",
        )
    )
    failed = captured["discoveries"][0]
    assert failed["status"] == "failed"
    assert "none of those files exist" in failed["note"]
    # No id, so no edges: an edge from `discovery:` would collapse every failure onto one node.
    assert captured["edges"] == []


def test_a_run_with_neither_an_id_nor_a_failure_emits_nothing():
    """There is nothing to approve, nothing to poll and nothing to report."""
    assert _captured(DiscoveryRunResults(name="empty")) == {}


def test_the_lifecycle_updates_one_entry_rather_than_stacking_rows():
    """The reducer dedupes on `run_id`, so approval and completion refresh in place while a second
    run accumulates on its own."""
    drafted = _captured(
        DiscoveryRunResults(name="run", run_id="abc", dataset_paths=["./a.csv"])
    )
    running = _captured(
        DiscoveryRunResults(
            name="run", run_id="abc", dataset_paths=["./a.csv"], status="running"
        )
    )
    other = _captured(
        DiscoveryRunResults(name="second", run_id="def", dataset_paths=["./b.csv"])
    )
    merged = _merge_artifacts(_merge_artifacts(drafted, running), other)
    rows = {row["run_id"]: row["status"] for row in merged["discoveries"]}
    assert rows == {"abc": "running", "def": "awaiting_approval"}


def test_every_subagent_that_returns_a_schema_has_a_capture_branch():
    """The general version of §254's bug.

    `ArtifactCaptureMiddleware.after_agent` is a chain of `if self.source == …`. A subagent added
    with a `response_format` and no branch produces a structured result that goes nowhere,
    silently — which is exactly what happened, and no existing test could see it because each one
    only checked the subagent it was about.

    Read off the source rather than by constructing each schema and calling the method: a branch
    reads its fields with `getattr(structured, "x", default)`, so a half-built instance raises
    inside pydantic and the test would fail for a reason that has nothing to do with the property.
    What is being asserted is narrow and exact — a subagent that returns a schema is named in the
    chain that files it.
    """
    import inspect

    from backend import subagents as registry
    from backend.middleware import artifacts as capture

    chain = inspect.getsource(capture.ArtifactCaptureMiddleware.after_agent)
    missing = [
        subagent["name"]
        for subagent in registry.subagents
        if subagent.get("response_format")
        and f'self.source == "{subagent["name"]}"' not in chain
    ]
    assert not missing, (
        "these subagents return a structured result that `after_agent` files nowhere, so it "
        f"reaches no panel and no artifact: {missing}"
    )

    # And the converse, so a branch for a subagent that no longer exists is noticed rather than
    # quietly kept: every name the chain mentions is a real subagent.
    named = set(re.findall(r'self\.source == "([a-z_]+)"', chain))
    known = {subagent["name"] for subagent in registry.subagents}
    assert named <= known, f"branches for subagents that do not exist: {sorted(named - known)}"
