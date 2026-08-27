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
import os
from pathlib import Path
from types import SimpleNamespace

import pytest

from deepagents.backends.protocol import FileData, GlobResult, ReadResult

from backend.middleware.claims import (
    CLAIMED_PATHS,
    content_of,
    DATAVERSE_SEARCH,
    NO_PATHS,
    ClaimsRecorder,
    _write,
    claimed_paths,
    entry,
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

#: A fixed stamp, so the shape can be asserted without a clock — the reason `entry` takes one.
AT = "2026-08-26T11:04:12Z"


class FakeSandbox:
    """The three calls the recorder makes, and a switch for each one failing."""

    def __init__(self, entries=(), search=None, explode=False, work_dir=WORK):
        self.entries = list(entries)
        self.search = search
        self.explode = explode
        # **One work_dir, used by both calls.** A fake that reported one directory and listed
        # another would let a test pass against a recorder comparing claims to the contents of
        # somewhere else, which is precisely the defect §278 was.
        self.work_dir = str(work_dir)
        self.globs = 0
        self.reads = 0

    async def aget_work_dir(self):
        if self.explode:
            raise RuntimeError("sandbox is gone")
        return self.work_dir

    async def aglob(self, pattern, path="/"):
        self.globs += 1
        # `GlobResult.matches` is a list of dicts, per the protocol — so a reader that used
        # attribute access on an entry would fail here as well.
        return GlobResult(
            error=None,
            matches=[{"path": f"{self.work_dir}/{e}", "is_dir": False} for e in self.entries],
        )

    async def aread(self, file_path, offset=0, limit=2000):
        """The real return types, not a friendlier stand-in for them.

        `ReadResult` is a dataclass, so attribute access is right for `error`; `FileData` is a
        **TypedDict**, so `file_data` is a plain dict and `.content` on it raises. The first version
        of this fake used `SimpleNamespace(content=...)` for both, which accepted the very call that
        failed on every turn in production (§224). `offset` is here because the protocol has it.
        """
        self.reads += 1
        self.read_limit = limit
        if self.search is None:
            return ReadResult(error="not found", file_data=None)
        return ReadResult(error=None, file_data=FileData(content=self.search, encoding="utf-8"))


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
    assert "2 not in the workspace" in line
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
    assert missing_from([".asta/documents"], {".asta/documents/index.json"}) == ([], [])
    assert missing_from([".asta/documents"], {".asta/other/index.json"}) == (
        [".asta/documents"],
        [],
    )


def test_a_file_outside_the_workspace_is_not_called_missing():
    """The first real finding this recorder produced, and half of it was wrong.

    `/mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf` is the PDF the researcher attached.
    It exists. Reporting it as *missing* reads as *this file does not exist*, and a record that
    cries wolf once is one nobody reads the second time. It is still worth saying — a file there
    does not travel with the conversation — but in its own words.
    """
    missing, outside = missing_from(
        [".asta/documents", "/mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf"],
        set(),
        work_dir="/home/user/workspace",
    )
    assert missing == [".asta/documents"]
    assert outside == ["/mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf"]


def test_an_absolute_path_inside_the_workspace_is_judged_on_whether_it_is_there():
    """Under the work dir, absolute or not, the question is simply existence."""
    missing, outside = missing_from(
        ["/home/user/workspace/papers/a.pdf"], set(), work_dir="/home/user/workspace"
    )
    assert missing == ["/home/user/workspace/papers/a.pdf"]
    assert outside == []


def test_the_outside_case_is_reported_in_its_own_words(recorded):
    library = LibraryArtifact(
        summary="indexed one",
        index_path=".asta/documents",
        papers=[
            IndexedPaper(title="Attached", path="/mnt/c/Users/LENOVO/Downloads/gnn.pdf")
        ],
    )
    record("pdf_librarian", library, FakeSandbox(entries=[".asta/documents/index.json"]))
    line = "\n".join(recorded)
    assert "will not travel with it" in line
    assert "/mnt/c/Users/LENOVO/Downloads/gnn.pdf" in line
    # And the index that *is* there is not named as a problem.
    assert "not in the workspace" not in line


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
    assert "1 not in the workspace" in line and "analysis/resid.png" in line


def test_the_same_missing_path_claimed_twice_is_named_once():
    assert missing_from(["a.png", "./a.png", "a.png"], set()) == (["a.png"], [])


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


def test_a_search_that_recommended_nothing_is_written_down(recorded):
    """What the researcher saw twice, and what nothing recorded.

    `mcp_tools._make_mcp_error_handler` turns a failed tool call into an ordinary message, so a
    Dataverse turn whose read never succeeded completes quietly with an empty shortlist. An empty
    search is a legitimate outcome; an unrecorded one is how a wrong argument name survived weeks.
    """
    record("dataverse_explorer", _recommendation(), FakeSandbox())
    assert "recommended no datasets at all" in "\n".join(recorded)


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


# ---------------------------------------------------------------------------
# It is actually attached
# ---------------------------------------------------------------------------

def test_the_recorder_is_attached_to_every_subagent_that_returns_a_schema():
    """§128's rule: a wiring that only executes in production is one nobody tests.

    `_build_runtime_subagents` is called once, at graph assembly, on a path no test reached — so a
    wrong keyword or a missing import here would surface as "An internal error occurred" on the
    researcher's first turn rather than in this file.
    """
    from backend.subagents import _build_runtime_subagents, subagents

    class Resolver:
        def for_subagent(self, name, overrides):
            return "openai::gpt-4o-mini"

    built = _build_runtime_subagents(
        academic_research_tools=[],
        dataverse_tools=[],
        data_cleaning_tools=[],
        diagnostic_tools=[],
        theory_tools=[],
        datavoyager_tools=[],
        discovery_tools=[],
        file_sync=object(),
        sandbox_backend=FakeSandbox(),
        model_resolver=Resolver(),
        subagent_overrides={},
    )

    expected = {s["name"] for s in subagents if s.get("response_format") is not None}
    recorded_by = {
        built_subagent["name"]
        for built_subagent in built
        if any(isinstance(m, ClaimsRecorder) for m in built_subagent["middleware"])
    }
    assert recorded_by == expected
    # The four that answer in prose are deliberately uncovered — claims.py says why.
    assert "data_cleaning" not in recorded_by
    assert "exploratory_data_analysis" not in recorded_by

    # Each recorder must know which subagent it speaks for, or every line in the log says the
    # wrong name and the record is worse than useless.
    for built_subagent in built:
        for middleware in built_subagent["middleware"]:
            if isinstance(middleware, ClaimsRecorder):
                assert middleware.source == built_subagent["name"]


# ---------------------------------------------------------------------------
# The wrapper the backends actually return
# ---------------------------------------------------------------------------

def test_the_content_is_read_out_of_a_real_FileData():
    """§224: `FileData` is a TypedDict, so `.content` on it raises `AttributeError`.

    This is the whole defect. The dataverse check failed on every single turn, and the suite stayed
    green because its fake handed back an object where production hands back a dict.
    """
    result = ReadResult(file_data=FileData(content='[{"global_id": "doi:1/x"}]', encoding="utf-8"))
    assert content_of(result) == '[{"global_id": "doi:1/x"}]'


def test_attribute_style_content_is_still_read():
    """Two backends answer this call; a reader that handled one shape is what got us here."""
    assert content_of(SimpleNamespace(file_data=SimpleNamespace(content="hi"))) == "hi"


def test_a_read_that_failed_yields_no_content_rather_than_raising():
    assert content_of(ReadResult(error="not found", file_data=None)) is None
    assert content_of(SimpleNamespace()) is None
    # A dict with no `content` key, which is what a partial payload looks like.
    assert content_of(SimpleNamespace(file_data={"encoding": "utf-8"})) is None


def test_the_read_asks_for_more_than_zero_lines():
    """`limit=0` means "everything" to the sandbox and "nothing" to deepagents' local backend.

    Same call, opposite meanings, and host execution is the one the researcher runs.
    """
    sandbox = FakeSandbox(entries=[DATAVERSE_SEARCH], search='[{"global_id": "doi:1/x"}]')
    record("dataverse_explorer", _recommendation("doi:1/x"), sandbox)
    assert sandbox.read_limit > 0


def test_the_channel_is_shared_rather_than_copied():
    """§235: a second module needed the same trick, so it lives in one place now.

    Attaching a handler per import would print each line as many times as the module was loaded,
    which is why `arriving` marks the logger it has already fitted.
    """
    from backend import diagnostics

    first = diagnostics.arriving("minime.test.channel")
    before = len(first.handlers)
    again = diagnostics.arriving("minime.test.channel")
    assert again is first
    assert len(again.handlers) == before, "fitting twice must not stack handlers"
    assert again.level == logging.INFO
    assert not again.propagate


# ---------------------------------------------------------------------------
# The record the app reads
# ---------------------------------------------------------------------------

def test_a_schema_nobody_checked_is_not_a_clean_bill_of_health():
    """`checked=False` and `missing=[]` are the same silence unless the record separates them.

    `HypothesisOutput` has no path rule, so it produces no missing files — and a reader that
    inferred "nothing missing" from that would report an unexamined answer as a verified one,
    which is the failure `NO_PATHS` exists to make visible rather than to hide.
    """
    unchecked = entry("hypothesis_generator", "HypothesisOutput", at=AT, checked=False)
    clean = entry("data_voyager", "DataAnalysisResults", at=AT, checked=True, claimed=3)
    assert unchecked["missing"] == clean["missing"] == []
    assert unchecked["checked"] is not clean["checked"]
    assert unchecked["claimed"] == 0 and clean["claimed"] == 3


def test_a_dataverse_run_that_recommended_nothing_is_not_a_run_that_was_never_asked():
    """`datasets: 0` against `datasets: null` — §220's morning, in one field."""
    asked = entry("dataverse_explorer", "DataVerseSearchResults", at=AT, checked=True, datasets=0)
    never = entry("data_voyager", "DataAnalysisResults", at=AT, checked=True)
    assert asked["datasets"] == 0
    assert never["datasets"] is None


def test_the_record_lands_beside_the_command_record(tmp_path):
    """One folder, two files, and the app finds both by the same address."""
    from minime_local import ledger

    _write(tmp_path, entry("pdf_librarian", "LibraryArtifact", at=AT, checked=True, claimed=2))
    written = ledger.read(tmp_path, name=ledger.CLAIMS_NAME)
    assert [line["source"] for line in written] == ["pdf_librarian"]
    assert (tmp_path / ledger.RECORD_DIR / ledger.CLAIMS_NAME).is_file()
    # And it did not disturb the other record in the same folder.
    assert ledger.read(tmp_path) == []


def test_a_work_dir_on_another_machine_is_not_built_here(tmp_path, recorded):
    """`work_dir` comes from the sandbox. Under a hosted one it names somebody else's filesystem.

    Creating it would leave a folder shaped like a conversation, holding a record no app reads,
    on a machine that never ran the turn.
    """
    elsewhere = tmp_path / "not-here" / "019ff651"
    _write(elsewhere, entry("data_voyager", "DataAnalysisResults", at=AT, checked=True))
    assert not elsewhere.exists(), "the folder must not be conjured"
    assert any("not a folder on this machine" in line for line in recorded), (
        "and skipping it silently is the thing this module was written against"
    )


def test_every_structured_answer_is_recorded_even_the_clean_ones(tmp_path):
    """A findings-only record cannot answer 'did the subagent answer at all'.

    That is the question the researcher actually arrived with: a coordinator reported a failed
    dataverse run after three seconds and one model call, having never called the subagent. The
    missing line is only visible if the present ones are there.
    """
    from minime_local import ledger

    clean = DataAnalysisResults(
        question="does yield track rainfall",
        dataset_paths=["data/trials.csv"],
        charts=["analysis/corr.png"],
        findings=[],
    )
    sandbox = FakeSandbox(entries=["data/trials.csv", "analysis/corr.png"], work_dir=tmp_path)
    assert record("data_voyager", clean, sandbox) is None

    written = ledger.read(tmp_path, name=ledger.CLAIMS_NAME)
    assert len(written) == 1, "the clean answer is a line, not a silence"
    assert written[0]["missing"] == [] and written[0]["checked"] is True
    assert written[0]["claimed"] == 2
    assert written[0]["source"] == "data_voyager"
    assert written[0]["schema"] == "DataAnalysisResults"
    assert written[0]["at"].endswith("Z"), "the same stamp the command record uses"


def test_a_fabricated_dataset_reaches_the_record_not_only_the_log(tmp_path):
    from minime_local import ledger

    sandbox = FakeSandbox(
        entries=[DATAVERSE_SEARCH], search='[{"global_id": "doi:1/real"}]', work_dir=tmp_path
    )
    record("dataverse_explorer", _recommendation("doi:1/real", "doi:1/invented"), sandbox)

    written = ledger.read(tmp_path, name=ledger.CLAIMS_NAME)
    assert written[0]["datasets"] == 2
    assert written[0]["unsearched"] == ["doi:1/invented"]
    assert written[0]["note"] is None, "the check ran; there is nothing to explain"


def test_a_check_that_could_not_run_says_so_in_the_record(tmp_path):
    """Distinct from finding nothing wrong, which is the distinction §224 cost two days."""
    from minime_local import ledger

    sandbox = FakeSandbox(entries=[], search=None, work_dir=tmp_path)  # the read fails
    record("dataverse_explorer", _recommendation("doi:1/x"), sandbox)

    written = ledger.read(tmp_path, name=ledger.CLAIMS_NAME)
    assert written[0]["unsearched"] == [], "no accusation from a check that never happened"
    assert written[0]["note"] == f"{DATAVERSE_SEARCH} could not be read"


def test_a_broken_record_never_costs_the_turn(tmp_path, recorded):
    """The folder is a file, so every write into it fails. The turn must not notice."""
    from minime_local import ledger

    (tmp_path / ledger.RECORD_DIR).write_text("not a folder", encoding="utf-8")
    sandbox = FakeSandbox(entries=["data/trials.csv"], work_dir=tmp_path)
    analysis = DataAnalysisResults(
        question="q", dataset_paths=["data/trials.csv"], charts=[], findings=[]
    )
    assert record("data_voyager", analysis, sandbox) is None


# ---------------------------------------------------------------------------
# The contract with the app that reads it
# ---------------------------------------------------------------------------

#: The record as the app must read it, written from this module's own code.
#:
#: Same discipline as `test_ledger.py` and `test_artifact_contract.py`: a field added here fails a
#: test until the client's authors have decided about it, rather than arriving unread for as long
#: as the feature exists — which is §223, and which is also what happened to this whole module for
#: seven months.
FIXTURE = (
    Path(__file__).resolve().parent.parent.parent
    / "crates" / "app" / "tests" / "fixtures" / "claim-record.jsonl"
)


def _sample() -> list[dict]:
    """Five answers, covering every branch the client can render wrong.

    Real-ish rather than minimal. Each of these is a shape that has actually occurred: the clean
    analysis, the librarian naming an index that is not there beside a PDF that is (§230), a
    schema no rule covers, a recommended dataset the search never returned, and a check that could
    not run at all (§224).
    """
    return [
        entry(
            "data_voyager",
            "DataAnalysisResults",
            at="2026-08-26T11:04:12Z",
            checked=True,
            claimed=4,
        ),
        entry(
            "pdf_librarian",
            "LibraryArtifact",
            at="2026-08-26T11:06:38Z",
            checked=True,
            claimed=2,
            missing=[".asta/documents"],
            outside=["/mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf"],
        ),
        entry(
            "hypothesis_generator",
            "HypothesisOutput",
            at="2026-08-26T11:09:01Z",
            checked=False,
        ),
        entry(
            "dataverse_explorer",
            "DataVerseSearchResults",
            at="2026-08-26T11:12:55Z",
            checked=True,
            datasets=3,
            unsearched=["doi:10.21223/INVENTED"],
        ),
        entry(
            "dataverse_explorer",
            "DataVerseSearchResults",
            at="2026-08-26T11:15:20Z",
            checked=True,
            datasets=2,
            note=f"{DATAVERSE_SEARCH} could not be read",
        ),
    ]


def test_the_committed_claim_fixture_matches_what_this_module_writes():
    """Regenerate with `MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_claims.py`."""
    generated = "\n".join(json.dumps(e, ensure_ascii=False, sort_keys=True) for e in _sample()) + "\n"
    if os.environ.get("MINIME_WRITE_CONTRACT"):
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(generated, encoding="utf-8")
        pytest.skip("fixture regenerated; read the diff")

    assert FIXTURE.exists(), f"{FIXTURE} is missing — regenerate with MINIME_WRITE_CONTRACT=1"
    assert FIXTURE.read_text(encoding="utf-8") == generated, (
        "the claim record changed shape. Regenerate with "
        "`MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_claims.py`, then decide whether the "
        "app should read the new field."
    )


def test_the_fixture_covers_both_branches_the_client_can_get_wrong():
    entries = _sample()
    assert any(e["missing"] for e in entries), "an answer naming a file that is not there"
    assert any(not e["missing"] and e["checked"] for e in entries), "and one that was clean"
    assert any(not e["checked"] for e in entries), "and one nothing looked at"
    assert any(e["outside"] for e in entries), "and one using a file from elsewhere"
    assert any(e["datasets"] is None for e in entries), "and one that was never a dataverse run"
    assert any(e["unsearched"] for e in entries), "and one recommending an unsearched dataset"
    assert any(e["note"] for e in entries), "and one whose check could not run at all"
    assert any(e["datasets"] and not e["unsearched"] and not e["note"] for e in entries) is False, (
        "no line here claims a clean dataverse check, so add one if the client needs to render it"
    )


def test_the_recorder_writes_the_shape_the_fixture_declares(tmp_path):
    """The fixture is hand-built from `entry`; this is the same shape arriving from a real run.

    Both halves matter. `test_ledger.py`'s first version asserted only the hand-built one, and a
    field could be added to the fixture, regenerated, and never written by the code that runs.
    """
    from minime_local import ledger

    sandbox = FakeSandbox(entries=["data/trials.csv"], work_dir=tmp_path)
    analysis = DataAnalysisResults(
        question="q", dataset_paths=["data/trials.csv"], charts=[], findings=[]
    )
    record("data_voyager", analysis, sandbox)
    written = ledger.read(tmp_path, name=ledger.CLAIMS_NAME)
    assert set(written[0]) == set(_sample()[0]), "the run and the fixture carry the same keys"


# ---------------------------------------------------------------------------
# Every path out of the write says which one it took
# ---------------------------------------------------------------------------

def test_a_written_record_says_so(tmp_path, recorded):
    """**Success is logged too**, or a working recorder reads like a missing one.

    That is `diagnostics.arriving`'s whole argument turned on this module: an absent line cannot
    distinguish "checked, nothing wrong" from "never ran", and the first time this record failed
    to appear there was nothing anywhere that said so.
    """
    _write(tmp_path, entry("pdf_librarian", "LibraryArtifact", at=AT, checked=True, claimed=2))
    line = "\n".join(recorded)
    assert "recorded pdf_librarian" in line
    assert "claims.jsonl" in line, "and it names the file, so somebody can go and look"


def test_a_record_that_could_not_be_written_is_a_warning(tmp_path, recorded):
    """The folder is a file, so every write into it fails — and it must not fail quietly."""
    from minime_local import ledger

    (tmp_path / ledger.RECORD_DIR).write_text("not a folder", encoding="utf-8")
    _write(tmp_path, entry("data_voyager", "DataAnalysisResults", at=AT, checked=True))
    line = "\n".join(recorded)
    assert "could not be written" in line
    assert "data_voyager" in line
    assert "Outputs panel" in line, "and says what the researcher will notice"


def test_a_missing_overlay_is_said_at_a_level_this_channel_prints(monkeypatch, tmp_path, recorded):
    """**INFO, not DEBUG.** The channel is set to INFO, so the debug line went nowhere.

    A sandboxed deployment legitimately has no `minime_local`, and "the overlay is missing" is the
    single most useful sentence for anyone looking for a panel row that never appeared.
    """
    import sys

    monkeypatch.setitem(sys.modules, "minime_local", None)
    _write(tmp_path, entry("academic_researcher", "AcademicResearchResults", at=AT, checked=True))
    line = "\n".join(recorded)
    assert "no minime_local on the path" in line
    assert "academic_researcher" in line
    assert not (tmp_path / ".mini-me").exists(), "and nothing was left behind"


def test_the_record_is_written_off_the_event_loop(tmp_path, monkeypatch):
    """**`langgraph dev` refuses a synchronous `os.mkdir` inside an async context.**

    `blockbuster` wraps the interpreter and raises `BlockingError`, so the first version of this
    failed on every real turn while every test here passed — the tests call `_write` directly,
    where there is no loop to block. The failure only exists on the path production takes.

    Asserted by *where the call ran*, not by reading the source: a thread that is not the one
    running the loop is the property that matters, and it stays true however the offload is
    spelled.
    """
    import threading

    import backend.middleware.claims as claims

    ran_on: list[str] = []

    def spy(work_dir, record):
        ran_on.append(threading.current_thread().name)

    monkeypatch.setattr(claims, "_write", spy)

    analysis = DataAnalysisResults(
        question="q", dataset_paths=["data/trials.csv"], charts=[], findings=[]
    )
    here = threading.current_thread().name
    record("data_voyager", analysis, FakeSandbox(entries=["data/trials.csv"], work_dir=tmp_path))

    assert ran_on, "the record was never written at all"
    assert ran_on[0] != here, (
        f"the write ran on {ran_on[0]}, the same thread as the loop — blockbuster raises there"
    )
