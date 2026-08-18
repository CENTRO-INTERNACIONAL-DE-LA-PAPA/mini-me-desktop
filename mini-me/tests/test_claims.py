"""The record of what a subagent claimed, against what the workspace holds.

Driven through the real middleware rather than its helpers, because the property that matters is
not "the comparison is right" but "this cannot cost a researcher a turn": `aafter_agent` has to
return `None` and swallow its own failures, and a test of `missing_from` alone would pass against
a version that raised.
"""

from __future__ import annotations

import asyncio
import json
import logging
from types import SimpleNamespace

import pytest

from backend.middleware.claims import (
    CLAIMED_PATHS,
    DATAVERSE_SEARCH,
    NO_PATHS,
    ClaimsRecorder,
    claimed_paths,
    missing_from,
    unsearched,
)
from backend.middleware.dataverse_first import FIXED_FILENAME
from backend.schemas import (
    DataAnalysisResults,
    DataFinding,
    DataVerseFindings,
    DataVerseSearchResults,
    IndexedPaper,
    LibraryArtifact,
    ReportWriterOutput,
)

WORK = "/home/user/workspace"


class FakeSandbox:
    """The three calls the recorder makes, and a switch for each one failing."""

    def __init__(self, entries=(), search=None, explode=False):
        self.entries = list(entries)
        self.search = search
        self.explode = explode
        self.globs = 0
        self.reads = 0

    async def aget_work_dir(self):
        if self.explode:
            raise RuntimeError("sandbox is gone")
        return WORK

    async def aglob(self, pattern, path):
        self.globs += 1
        return SimpleNamespace(
            error=None,
            matches=[{"path": f"{WORK}/{e}", "is_dir": False} for e in self.entries],
        )

    async def aread(self, file_path, limit=2000):
        self.reads += 1
        if self.search is None:
            return SimpleNamespace(error="not found", file_data=None)
        return SimpleNamespace(
            error=None, file_data=SimpleNamespace(content=self.search)
        )


def record(source, structured, sandbox):
    """Run the middleware the way the graph does, and hand back what it returned."""
    return asyncio.run(
        ClaimsRecorder(source, sandbox).aafter_agent(
            {"structured_response": structured}, None
        )
    )


@pytest.fixture
def recorded():
    """The lines the module emits, read from its own logger rather than through `caplog`.

    `claims.py` turns `propagate` off deliberately — the record must not depend on a root
    configuration nobody sets (docs §132, where an INFO line that never reached the backend log
    cost a diagnosis). `caplog` captures at the root, so reading it there would test the
    opposite of the guarantee this module makes.
    """
    logger = logging.getLogger("backend.middleware.claims")
    lines: list[str] = []

    class Capture(logging.Handler):
        def emit(self, record):
            lines.append(record.getMessage())

    handler = Capture()
    logger.addHandler(handler)
    try:
        yield lines
    finally:
        logger.removeHandler(handler)


# ---------------------------------------------------------------------------
# Paths a subagent typed
# ---------------------------------------------------------------------------

def test_a_chart_that_was_never_written_is_named(recorded):
    """§207a in miniature: the response names four paths and two of them are not there."""
    analysis = DataAnalysisResults(
        question="does yield track rainfall",
        dataset_paths=["data/trials.csv"],
        charts=["analysis/corr.png", "analysis/resid.png"],
        findings=[DataFinding(title="Correlated", chart_path="./analysis/scatter.png")],
    )
    sandbox = FakeSandbox(entries=["data/trials.csv", "analysis/corr.png"])
    assert record("data_voyager", analysis, sandbox) is None
    line = "\n".join(recorded)
    assert "2 missing" in line
    assert "analysis/resid.png" in line
    assert "analysis/scatter.png" in line
    # The two that are there must not be named, or the record stops being readable.
    assert "corr.png," not in line and "trials.csv" not in line


def test_every_path_present_is_recorded_as_such(recorded):
    """The clean case has to be visible too — silence would read as 'never ran'."""
    analysis = DataAnalysisResults(
        question="q", charts=["analysis/a.png"], dataset_paths=["d.csv"]
    )
    sandbox = FakeSandbox(entries=["analysis/a.png", "d.csv"])
    record("data_voyager", analysis, sandbox)
    assert "named 2 paths, all present" in "\n".join(recorded)


def test_a_url_in_the_librarians_path_field_is_not_a_missing_file(recorded):
    """`IndexedPaper.path` is documented as "sandbox path **or URL**"."""
    library = LibraryArtifact(
        summary="indexed one, linked one",
        index_path=".asta/documents",
        papers=[
            IndexedPaper(title="Downloaded", path="papers/blight.pdf"),
            IndexedPaper(title="Linked", path="https://example.org/paper.pdf"),
        ],
    )
    sandbox = FakeSandbox(entries=["papers/blight.pdf", ".asta/documents/index.json"])
    record("pdf_librarian", library, sandbox)
    assert "all present" in "\n".join(recorded)


def test_the_index_directory_resolves_through_what_is_inside_it():
    """`.asta/documents` is a directory; the listing may only carry its contents."""
    assert missing_from([".asta/documents"], {".asta/documents/index.json"}) == []
    assert missing_from([".asta/documents"], {".asta/other/index.json"}) == [
        ".asta/documents"
    ]


def test_a_report_embedding_a_figure_that_is_not_there_is_recorded(recorded):
    """The renderer resolves these against the sandbox, so a wrong one is a hole in the PDF."""
    report = ReportWriterOutput(
        title="Trial summary",
        markdown=(
            "# Findings\n\n"
            "![Distribution of yield](./eda_dist.png)\n\n"
            "![Residuals](analysis/resid.png)\n"
        ),
    )
    sandbox = FakeSandbox(entries=["eda_dist.png"])
    record("report_writer", report, sandbox)
    line = "\n".join(recorded)
    assert "1 missing" in line and "analysis/resid.png" in line


def test_the_same_missing_path_claimed_twice_is_named_once():
    assert missing_from(["a.png", "./a.png", "a.png"], set()) == ["a.png"]


# ---------------------------------------------------------------------------
# Dataset ids a subagent recommended
# ---------------------------------------------------------------------------

def _recommendation(*ids):
    return DataVerseSearchResults(
        summary="three candidates",
        datasets=[
            DataVerseFindings(
                title=f"Dataset {i}",
                persistent_id=identifier,
                recommendation_reason="relevant",
            )
            for i, identifier in enumerate(ids)
        ],
    )


def test_a_dataset_id_the_search_never_returned_is_named(recorded):
    """A composed `persistent_id` is a citation a researcher pastes into a paper."""
    search = json.dumps(
        {"data": [{"global_id": "doi:10.21223/REAL1"}, {"global_id": "doi:10.21223/REAL2"}]}
    )
    sandbox = FakeSandbox(entries=[DATAVERSE_SEARCH], search=search)
    record(
        "dataverse_explorer",
        _recommendation("doi:10.21223/REAL1", "doi:10.21223/INVENTED"),
        sandbox,
    )
    line = "\n".join(recorded)
    assert "1 absent" in line and "doi:10.21223/INVENTED" in line
    assert "REAL1," not in line


def test_ids_the_search_did_return_are_clean(recorded):
    search = json.dumps({"rows": [{"pid": "doi:10.21223/A"}, {"pid": "doi:10.21223/B"}]})
    sandbox = FakeSandbox(entries=[DATAVERSE_SEARCH], search=search)
    record("dataverse_explorer", _recommendation("doi:10.21223/A"), sandbox)
    assert "all present in" in "\n".join(recorded)


def test_an_id_nested_anywhere_in_the_search_shape_still_counts():
    """The MCP owns this file's layout; a reader that walked named keys would break with it."""
    search = json.dumps({"a": {"b": [{"c": {"d": "doi:10.21223/DEEP"}}]}})
    assert unsearched(["doi:10.21223/DEEP"], search) == []


def test_a_search_file_that_is_not_json_accuses_nobody():
    """Better to record nothing than to report every dataset as fabricated."""
    assert unsearched(["doi:10.21223/A"], "<html>gateway timeout</html>") == []


def test_an_unreadable_search_file_says_so_instead_of_passing(recorded):
    sandbox = FakeSandbox(entries=[], search=None)
    record("dataverse_explorer", _recommendation("doi:10.21223/A"), sandbox)
    assert "could not be read" in "\n".join(recorded)


def test_the_recorder_reads_the_file_both_dataverse_tools_agree_on():
    """One string, spelled in two middlewares. If they drift, the check reads nothing."""
    assert DATAVERSE_SEARCH == FIXED_FILENAME


# ---------------------------------------------------------------------------
# It records, and does not block
# ---------------------------------------------------------------------------

def test_a_broken_sandbox_costs_the_record_and_not_the_turn(recorded):
    """The whole point of `aafter_agent` returning None on every path."""
    analysis = DataAnalysisResults(question="q", charts=["a.png"])
    sandbox = FakeSandbox(explode=True)
    assert record("data_voyager", analysis, sandbox) is None
    assert "recording data_voyager failed" in "\n".join(recorded)


def test_the_record_arrives_whatever_the_root_log_level_is(recorded):
    """The defect this module was nearly shipped with.

    Nothing in the backend configures logging, and docs §132 found that INFO has never been seen to
    reach the backend log — so a recorder whose clean lines went out at INFO through the root
    would show its failures and swallow its successes. "Checked, nothing wrong" would be
    indistinguishable from "never ran", which is the one question being asked of the subagents
    nobody has watched end to end.
    """
    root = logging.getLogger()
    previous = root.level
    root.setLevel(logging.CRITICAL)
    try:
        record(
            "data_voyager",
            DataAnalysisResults(question="q", charts=["a.png"]),
            FakeSandbox(entries=["a.png"]),
        )
    finally:
        root.setLevel(previous)
    assert any("all present" in line for line in recorded)


def test_a_subagent_with_no_structured_response_is_not_looked_at():
    """Four subagents have none; the recorder must not reach for the sandbox for them."""
    sandbox = FakeSandbox()
    assert (
        asyncio.run(
            ClaimsRecorder("data_cleaning", sandbox).aafter_agent(
                {"structured_response": None}, None
            )
        )
        is None
    )
    assert sandbox.globs == 0 and sandbox.reads == 0


def test_a_schema_with_nothing_to_check_costs_no_sandbox_call():
    sandbox = FakeSandbox()
    record("research_planner", _recommendation(), sandbox)
    assert sandbox.globs == 0


def test_every_response_format_has_been_decided_about():
    """A new schema must be classified, not silently uncovered.

    This is the test that keeps the record honest as the subagents change: adding a
    `response_format` without saying whether it carries paths fails here rather than producing a
    quiet "no paths claimed" for the rest of the project's life.
    """
    from backend.subagents import subagents

    for subagent in subagents:
        schema = subagent.get("response_format")
        if schema is None:
            continue
        name = schema.__name__
        assert name in CLAIMED_PATHS or name in NO_PATHS, (
            f"{subagent['name']} returns {name}, which is in neither CLAIMED_PATHS nor "
            f"NO_PATHS — decide which and say so in backend/middleware/claims.py"
        )


def test_an_unrecognised_schema_is_recorded_as_uncovered(recorded):
    """If the guard above is ever bypassed, the log still says the check did not run."""

    class SomethingNew:
        pass

    sandbox = FakeSandbox()
    record("data_voyager", SomethingNew(), sandbox)
    assert "no path rule covers" in "\n".join(recorded)


def test_claimed_paths_reports_whether_it_recognised_the_schema():
    assert claimed_paths(ReportWriterOutput(title="t", markdown="![a](x.png)")) == (
        ["x.png"],
        True,
    )
    assert claimed_paths(object()) == ([], False)
