"""Write down what a subagent said it produced, beside what is actually there.

# Why this exists

A worker reported `success` while the four folders it named were empty (docs §207a). Three rounds
of diagnosis went into finding that, and it was found by luck: the coordinator happened to look.
Nothing in the system compares a subagent's account of its work against the workspace, so a
subagent that writes nothing and says otherwise is indistinguishable from one that worked.

The two failures this catches are different from each other and both are silent:

* **A file that was never written.** `DataAnalysisResults.charts`, `LibraryArtifact.papers[].path`
  and the image refs inside a report are all *paths the model typed*. Nothing has ever checked
  that a file is there. A report that embeds `![Distribution](./eda_dist.png)` renders as a broken
  image in the PDF, and the researcher finds out at the end.
* **A dataset that was never in the search.** Every `DataVerseFindings` requires a
  `persistent_id`, and one composed from memory is a citation a researcher will paste into a paper
  (`middleware/dataverse_first.py`). `SearchBeforeRecommending` already forces the search and the
  read; what it cannot do is check that the ids coming out are the ids that went in.

# It records, and does not block

Nothing here refuses a turn, edits a response, or changes state — `aafter_agent` returns `None` on
every path, and the whole body is wrapped so a bug in this file cannot cost a researcher their
subagent's work. That is deliberate rather than timid: the enforcement rules worth writing are the
ones that come from failures actually observed, and three of the eleven subagents have never been
run end to end. Measure first.

Read the result in the backend log the app already writes — `%TEMP%\\mini-me-desktop-backend.log`
on Windows, `$TMPDIR/mini-me-desktop-backend.log` elsewhere — by searching for `claims:`.
"""

from __future__ import annotations

import json
import logging
import re
import sys
from pathlib import PurePosixPath
from typing import TYPE_CHECKING, Any

from langgraph.runtime import Runtime
from langchain.agents.middleware import AgentMiddleware

from backend.schemas import ArtifactState

if TYPE_CHECKING:
    from backend.sandbox import LazyLangsmithSandbox

logger = logging.getLogger(__name__)


def _make_the_record_arrive() -> None:
    """Give this module's log a channel that is known to reach the file.

    Nothing in the backend configures logging, so the level is whatever the LangGraph server
    happens to set, and docs §132 records what that turned out to be: *every line this overlay has
    ever been seen to produce in the backend log arrived at WARNING*. An `INFO` that never lands
    cost a diagnosis once already, because the absence of a line was read as "the tool did not
    run" when it may only have meant "INFO does not reach this file".

    That distinction is this whole module. A record that shows the failures and swallows the
    clean runs cannot tell "checked, nothing wrong" apart from "never ran" — which is precisely
    the question being asked of three subagents nobody has watched end to end.

    So the level is not left to chance: one handler on stderr, which `crates/app/src/backend.rs`
    hands straight to the log file. `propagate` is off so a server that *does* configure INFO
    prints each line once rather than twice.
    """
    if getattr(logger, "_minime_claims_handler", False):
        return
    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(logging.Formatter("%(levelname)s %(message)s"))
    logger.addHandler(handler)
    logger.setLevel(logging.INFO)
    logger.propagate = False
    logger._minime_claims_handler = True  # type: ignore[attr-defined]


_make_the_record_arrive()


#: Which fields of each `response_format` carry a path the model typed, by schema class name.
#:
#: Declared rather than discovered. A walk that collected every string "looking like a path" would
#: report citations and free prose — and the cost of a false "missing file" is that the next
#: reader stops believing the record. A schema that gains a path field is a line here; that it is
#: not automatic is the point, since `_UNMAPPED` makes the omission visible in the log.
#:
#: `[]` means "each element of this list". Resolution is `_resolve`.
CLAIMED_PATHS: dict[str, tuple[str, ...]] = {
    "DataAnalysisResults": (
        "charts[]",
        "findings[].chart_path",
        # The inputs, checked as well as the outputs: an analysis run against a dataset path that
        # does not exist is precisely the silent failure this file is for, and DataVoyager reports
        # the paths it was *given*, not the ones it opened.
        "dataset_paths[]",
    ),
    "LibraryArtifact": (
        "index_path",
        "papers[].path",
    ),
}

#: Schemas with no path-bearing field, listed so that "not in `CLAIMED_PATHS`" can mean "nobody has
#: looked at this one" rather than "checked, nothing to check".
NO_PATHS = frozenset(
    {
        "AcademicResearchResults",
        "DataVerseSearchResults",
        "HypothesisOutput",
        "ResearchPlan",
        # ReportWriterOutput carries paths, but inside markdown rather than in a field — see
        # `_report_images`.
        "ReportWriterOutput",
    }
)

#: `![caption](./eda_distributions.png)`. The report writer is told to embed figures this way and
#: the PDF renderer resolves them against the sandbox at render time, so a path that is wrong here
#: reaches the researcher as a hole in their report.
MARKDOWN_IMAGE = re.compile(r"!\[[^\]]*\]\(\s*([^)\s]+)")

#: Claims that are not workspace paths at all. `IndexedPaper.path` is documented as "sandbox path
#: **or URL**", so a URL there is correct and must not be reported missing.
NOT_LOCAL = ("http://", "https://", "ftp://", "s3://", "asta://", "doi:", "data:")

#: The file `SearchCIPDataverse` writes and `read_search_results` reads back
#: (`middleware/dataverse_first.FIXED_FILENAME`). Imported rather than repeated would couple two
#: middlewares that are otherwise independent; it is one constant and it is asserted in the tests.
DATAVERSE_SEARCH = "dataverse_search.json"


def _resolve(obj: Any, spec: str) -> list[str]:
    """Every string a dotted spec names, with `[]` meaning 'each element of this list'."""
    head, _, rest = spec.partition(".")
    listed = head.endswith("[]")
    value = getattr(obj, head[:-2] if listed else head, None)
    if value is None:
        return []
    items = list(value) if listed else [value]
    if not rest:
        return [item for item in items if isinstance(item, str) and item.strip()]
    found: list[str] = []
    for item in items:
        found.extend(_resolve(item, rest))
    return found


def _report_images(structured: Any) -> list[str]:
    """The figures a report embeds, which live in its markdown rather than in a field."""
    markdown = getattr(structured, "markdown", None)
    if not isinstance(markdown, str):
        return []
    return MARKDOWN_IMAGE.findall(markdown)


def claimed_paths(structured: Any) -> tuple[list[str], bool]:
    """The workspace paths a structured response names, and whether its schema was recognised."""
    name = type(structured).__name__
    if name == "ReportWriterOutput":
        return _report_images(structured), True
    specs = CLAIMED_PATHS.get(name)
    if specs is None:
        return [], name in NO_PATHS
    found: list[str] = []
    for spec in specs:
        found.extend(_resolve(structured, spec))
    return found, True


def _normalise(claim: str) -> str:
    """A claim in the form the sandbox listing uses: no `./`, no trailing slash."""
    text = claim.strip().strip('"').strip("'")
    while text.startswith("./"):
        text = text[2:]
    return text.rstrip("/") or text


def missing_from(
    claims: list[str], present: set[str], work_dir: str = ""
) -> tuple[list[str], list[str]]:
    """Claims that name nothing in the workspace, and claims that point outside it.

    **Two answers, because they are two different facts and only one is an accusation.** The first
    real finding this recorder produced said:

        pdf_librarian named 2 paths, 2 missing from the workspace:
          .asta/documents, /mnt/c/Users/LENOVO/Downloads/Graph-neural-networks.pdf

    The second of those exists — it is the PDF the researcher attached, sitting in their Downloads
    folder. Calling it *missing* is true only in the sense that it is not in the thread's directory,
    and it reads as *this file does not exist*, which is false. A record that cries wolf once is a
    record nobody reads the second time, so the two cases now get their own words.

    Pointing outside is still worth saying: files there do not travel with the conversation and are
    invisible in Outputs, which is what the librarian's own prompt tells it to avoid.

    A claim matches if it names an entry outright or is a parent of one — the parent case is what
    makes `index_path=".asta/documents"` resolve when only its contents were listed.
    """
    base = work_dir.rstrip("/")
    missing: list[str] = []
    outside: list[str] = []
    seen: list[str] = []
    for claim in claims:
        if claim.lower().startswith(NOT_LOCAL):
            continue
        path = _normalise(claim)
        if not path or path in seen:
            continue
        seen.append(path)
        if path in present:
            continue
        if any(entry.startswith(path + "/") for entry in present):
            continue
        # Absolute, and not under the working directory: a real place we cannot vouch for.
        if path.startswith("/") and base and not path.startswith(base + "/"):
            outside.append(path)
        else:
            missing.append(path)
    return missing, outside


def _ids_in(text: str) -> set[str]:
    """Every string in the search results, so a claimed id can be looked for among them.

    Deliberately shape-blind. The MCP writes this file and its JSON layout is not ours; a reader
    that walked named keys would report every dataset as fabricated the day that layout changed,
    which is the one failure mode a record like this cannot afford. Collecting the leaf strings
    cannot produce a false alarm — an id the model composed is not in the file at all.
    """
    found: set[str] = set()

    def walk(node: Any) -> None:
        if isinstance(node, str):
            found.add(node.strip())
        elif isinstance(node, dict):
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(json.loads(text))
    return found


def content_of(result: Any) -> str | None:
    """The text a backend read returned, out of whichever shape carries it.

    **`ReadResult.file_data` is a `FileData`, and `FileData` is a `TypedDict`** — so it is a plain
    dict and `.content` on it raises `AttributeError`. The first version of this file did exactly
    that, and the test beside it passed because its fake used `SimpleNamespace(content=...)`: a
    double more permissive than the real type, which is the mistake §222 was written about. It cost
    the whole dataverse check, which failed on every turn (§224).

    Attribute access is kept as a fallback rather than removed, because two backends answer this
    call — `LazyLangsmithSandbox` and the overlay's `LocalWorkspaceBackend` under host execution —
    and a reader that only handled one shape is what got us here.
    """
    data = getattr(result, "file_data", None)
    if data is None:
        return None
    if isinstance(data, dict):
        text = data.get("content")
    else:
        text = getattr(data, "content", None)
    return text if isinstance(text, str) else None


def unsearched(ids: list[str], text: str) -> list[str]:
    """Recommended dataset ids that do not appear anywhere in what the search returned."""
    try:
        strings = _ids_in(text)
    except (ValueError, TypeError):
        # Not JSON — the read returned an error page, or the tool changed format. Say nothing
        # rather than accuse every dataset; `_record` logs that the check could not run.
        return []
    joined = "\n".join(strings)
    unmatched: list[str] = []
    for identifier in ids:
        needle = identifier.strip()
        if not needle or needle in unmatched:
            continue
        if needle in strings or needle in joined:
            continue
        unmatched.append(needle)
    return unmatched


class ClaimsRecorder(AgentMiddleware[ArtifactState, Any, Any]):
    """Log what a subagent claimed against what the workspace holds. Records only; blocks nothing."""

    state_schema = ArtifactState

    def __init__(self, source: str, sandbox_backend: "LazyLangsmithSandbox"):
        super().__init__()
        self.source = source
        self.sandbox_backend = sandbox_backend
        self._work_dir: PurePosixPath | None = None

    async def _ensure_work_dir(self) -> PurePosixPath:
        if self._work_dir is None:
            self._work_dir = PurePosixPath(await self.sandbox_backend.aget_work_dir())
        return self._work_dir

    async def _present(self, work_dir: PurePosixPath) -> set[str]:
        """Every entry in the workspace, in both the relative and absolute spelling.

        Its own listing rather than `sync._collect_sandbox_files`, which filters to the file types
        worth showing a researcher: it drops dotfiles and unknown extensions, so checking against
        it would report `.asta/documents` and a written `.pkl` as missing when both are there.
        """
        result = await self.sandbox_backend.aglob("**/*", str(work_dir))
        if getattr(result, "error", None) or not getattr(result, "matches", None):
            return set()
        present: set[str] = set()
        base = work_dir.as_posix().rstrip("/")
        for match in result.matches:
            absolute = str(match.get("path") or "")
            if not absolute:
                continue
            absolute = absolute.rstrip("/")
            present.add(absolute)
            if absolute.startswith(base + "/"):
                present.add(absolute[len(base) + 1 :])
        return present

    async def _dataverse(self, structured: Any, work_dir: PurePosixPath) -> None:
        """Check the recommended `persistent_id`s against the file the search wrote."""
        ids = _resolve(structured, "datasets[].persistent_id")
        if not ids:
            # Recorded rather than passed over. A `DataVerseSearchResults` with no datasets is what
            # the researcher saw twice (§220), and it reaches them as a polite paragraph: the MCP
            # error handler turns a failed tool call into an ordinary message
            # (`mcp_tools._make_mcp_error_handler`), so the turn completes and nothing is raised.
            # An empty search is a legitimate outcome; an empty search that nobody wrote down is
            # how a broken tool argument survived for weeks.
            logger.warning(
                "claims: dataverse_explorer recommended no datasets at all — "
                "check the log above for a failed SearchCIPDataverse or read_search_results"
            )
            return
        # An explicit line count rather than `limit=0`. The sandbox reads "everything" for a
        # falsy limit; deepagents' local backend slices `lines[offset:offset + limit]`, where zero
        # is an empty read — the same call meaning opposite things on the two backends that serve
        # it. `dataverse_search.json` is one JSON array on few lines; this is a ceiling, not a size.
        result = await self.sandbox_backend.aread(
            str(work_dir / DATAVERSE_SEARCH), limit=1_000_000
        )
        text = content_of(result)
        if getattr(result, "error", None) or text is None:
            logger.warning(
                "claims: dataverse_explorer recommended %d datasets and %s could not be read (%s)",
                len(ids),
                DATAVERSE_SEARCH,
                getattr(result, "error", "no content"),
            )
            return
        unmatched = unsearched(ids, text)
        if unmatched:
            logger.warning(
                "claims: dataverse_explorer recommended %d datasets, %d absent from %s: %s",
                len(ids),
                len(unmatched),
                DATAVERSE_SEARCH,
                ", ".join(unmatched),
            )
        else:
            logger.info(
                "claims: dataverse_explorer recommended %d datasets, all present in %s",
                len(ids),
                DATAVERSE_SEARCH,
            )

    async def _paths(self, structured: Any, work_dir: PurePosixPath) -> None:
        claims, recognised = claimed_paths(structured)
        if not recognised:
            logger.info(
                "claims: %s returned %s, which no path rule covers",
                self.source,
                type(structured).__name__,
            )
            return
        if not claims:
            return
        missing, outside = missing_from(
            claims, await self._present(work_dir), work_dir.as_posix()
        )
        if missing:
            logger.warning(
                "claims: %s named %d paths, %d not in the workspace: %s",
                self.source,
                len(claims),
                len(missing),
                ", ".join(missing),
            )
        if outside:
            # Not a warning about honesty — those files are real. A warning about durability: they
            # sit outside the conversation's folder, so they will not travel with it.
            logger.warning(
                "claims: %s used %d file(s) from outside this conversation's folder, which will "
                "not travel with it: %s",
                self.source,
                len(outside),
                ", ".join(outside),
            )
        if not missing and not outside:
            logger.info(
                "claims: %s named %d paths, all present", self.source, len(claims)
            )

    async def aafter_agent(
        self, state: ArtifactState, runtime: Runtime
    ) -> dict[str, Any] | None:
        structured = state.get("structured_response")
        if structured is None:
            return None
        try:
            work_dir = await self._ensure_work_dir()
            await self._paths(structured, work_dir)
            if self.source == "dataverse_explorer":
                await self._dataverse(structured, work_dir)
        except Exception:
            # A record that can end a researcher's turn is worse than no record. The traceback is
            # kept because a recorder that fails quietly is the thing it was written to prevent.
            logger.exception("claims: recording %s failed", self.source)
        return None
