"""Find papers and hand back finished references.

# Why this exists

`academic_researcher` holds every Asta tool, and the one it reaches for is `snippet_search` —
reasonably, since that is the tool whose purpose is guessable from its name. It returns passages of
text with just enough paper attached to identify one:

    paper keys: ['authors', 'corpusId', 'openAccessInfo', 'title']

No year, no venue, no volume, no pages, no DOI. And `AcademicSourceFinding.citation` then asks the
model for an APA reference, which needs all five. So it supplies them from memory, and produces
citations whose journal and year are right and whose identifier points at a different paper — see
`backend/citations.py` for the six that were checked against Crossref.

This tool closes that gap by never opening it. It searches, and returns each paper with its
reference **already written from the record**. The model receives finished citations rather than
composing them, so there is no field left for it to invent.

# Why the CLI and not the MCP

Both reach the same Semantic Scholar data, and the MCP does expose metadata tools. The difference
is who chooses. Through the MCP the model picks a tool and we take whatever comes back; here the
call is ours, so the fields are guaranteed to be present and the reference can be built before the
model ever sees the paper.

It is also the pattern this repository already uses for Asta: `generate_theories` and
`analyze_data` both drive the CLI through the sandbox, with `ASTA_TOKEN` surfaced into the command
environment (`backend/sandbox.py`). This is the third, not the first.

# Shape

`_build_search_command` is **pure**, for the reason `theory_tools._build_submit_command` is: the
deployed bug that motivated that one was an agent hand-building a CLI invocation with a flag that
did not exist. A command builder with no I/O can be pinned by a unit test, and flag drift is then
caught in CI rather than in a researcher's run.
"""

from __future__ import annotations

import json
import logging
import shlex
from typing import Any

from langchain_core.tools import tool

from backend import citations
from backend.runtime import _active_sandbox

logger = logging.getLogger(__name__)

#: A search is a single API call behind the CLI; this is generous for a cold start.
SEARCH_TIMEOUT_S = 90

#: What a reference needs, plus the two fields worth reading before opening a paper.
#:
#: `tldr` is a one-sentence statement of what the paper found, and for a literature question it is
#: often better evidence than a 500-word passage cut out of a results section. `abstract` is the
#: fallback where there is no tldr.
SEARCH_FIELDS = "title,authors,year,venue,journal,externalIds,abstract,tldr"

#: Above this the result stops being a shortlist and starts being a database dump.
MAX_LIMIT = 25


def _build_search_command(query: str, limit: int) -> list[str]:
    """The argv for a paper search (asta CLI v0.101).

    Pure, so the CLI contract is unit-testable. `--fields` is not optional: without it the CLI
    returns titles and ids and none of the bibliographic fields this tool exists to supply.
    """
    # `None` means "not given" and takes the default; every other value is clamped. Written this
    # way because `limit or 10` also swallows `0` — a model passing zero would silently get ten
    # papers, which is neither what it asked for nor what the docstring promises.
    wanted = 10 if limit is None else int(limit)
    bounded = max(1, min(wanted, MAX_LIMIT))
    return [
        "asta",
        "papers",
        "search",
        query,
        "--limit",
        str(bounded),
        "--fields",
        SEARCH_FIELDS,
    ]


def _parse_search(output: str) -> list[dict[str, Any]]:
    """The paper records out of merged stdout/stderr execute output.

    Tolerant in the same way `theory_tools._extract_json` is, and for the same reason: `aexecute`
    appends stderr after a `[stderr]` marker, and a warning on stderr must not cost the whole
    result.
    """
    if not output:
        return []
    head = output.split("[stderr]", 1)[0].strip()
    for candidate in (head, output):
        start, end = candidate.find("{"), candidate.rfind("}")
        if start == -1 or end <= start:
            continue
        try:
            payload = json.loads(candidate[start : end + 1])
        except Exception:  # noqa: BLE001
            continue
        data = payload.get("data")
        if isinstance(data, list):
            return [item for item in data if isinstance(item, dict)]
    return []


def summarise(papers: list[dict[str, Any]]) -> list[dict[str, str]]:
    """Every paper found, with its reference and link built from the record.

    **Every one, not a filtered subset.** Deciding which results are relevant is the researcher's
    judgement; a tool that quietly dropped some would be substituting a machine's opinion of
    relevance for theirs, which is the same mistake this module exists to remove wearing a
    different coat.
    """
    return [citations.describe(paper) for paper in papers]


@tool
async def find_papers(query: str, limit: int = 10) -> str:
    """Search the scientific literature and get back ready-to-use references.

    Use this for every paper you intend to cite. Each result comes with its citation already
    written from the publisher's record, so **use the `citation` field exactly as given** — do not
    rewrite it, and never write a DOI, year, volume or page number yourself. If a field is missing
    from a citation it is missing from the record, and an incomplete reference is better than an
    invented one.

    The `link` goes to the paper on Semantic Scholar. Pass it through unchanged.

    Results include a one-sentence `summary` of what each paper found and its `abstract`, which is
    usually enough to judge relevance. Use `snippet_search` when you need to quote a passage from
    the body of a paper.

    Every paper the search returns is listed. Choosing which are relevant is the reader's job, not
    yours — report what you found and say which ones bear on the question.

    Args:
        query: what to search for, in plain words.
        limit: how many papers to return (1–25, default 10).

    Returns:
        JSON: {"query": ..., "count": n, "papers": [{citation, link, title, summary, abstract}]}
    """
    sandbox = _active_sandbox.get()
    if sandbox is None:
        return json.dumps(
            {"error": "no sandbox available to run the search", "papers": []}
        )

    command = shlex.join(_build_search_command(query, limit))
    runner = getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute
    try:
        response = await runner(command, timeout=SEARCH_TIMEOUT_S)
    except Exception as exc:  # noqa: BLE001
        logger.warning("find_papers failed for %r: %s", query, exc)
        return json.dumps({"error": f"the search failed: {exc}", "papers": []})

    papers = _parse_search(getattr(response, "output", "") or "")
    found = summarise(papers)
    # Logged with a count rather than a claim: "0 papers" and "10 papers" must not look alike in
    # a log that is read when something has gone wrong.
    logger.info("find_papers(%r) -> %d paper(s)", query, len(found))
    return json.dumps({"query": query, "count": len(found), "papers": found}, indent=2)


def _key(text: str) -> str:
    """A title reduced to something two spellings of it can be compared by."""
    return " ".join("".join(c if c.isalnum() else " " for c in (text or "").lower()).split())


def papers_in(messages: list[Any]) -> list[dict[str, str]]:
    """Every paper this conversation's searches returned, in order, deduplicated.

    Reads the agent's own `find_papers` tool results, which makes it correctly scoped to one run
    with no bookkeeping: a subagent's message list *is* the record of what it retrieved. The
    desktop overlay reached the same information through a process-global dict, and had to,
    because it hooks the tool from outside where the conversation is not in reach.
    """
    found: list[dict[str, str]] = []
    seen: set[str] = set()
    for message in messages or []:
        if getattr(message, "type", None) != "tool":
            continue
        if getattr(message, "name", None) != "find_papers":
            continue
        content = getattr(message, "content", None)
        if not isinstance(content, str):
            continue
        try:
            payload = json.loads(content)
        except (json.JSONDecodeError, ValueError):
            continue
        if not isinstance(payload, dict):
            continue
        for paper in payload.get("papers") or []:
            if not isinstance(paper, dict):
                continue
            # The link identifies a paper; the title is the fallback for a record that had no
            # usable identifier, where two results with the same title really are one paper.
            key = (paper.get("link") or "").strip() or _key(paper.get("title", ""))
            if key and key not in seen:
                seen.add(key)
                found.append(paper)
    return found


def unreported(papers: list[dict[str, str]], sources: list[dict[str, Any]]) -> list[dict[str, str]]:
    """The retrieved papers that did not survive into the answer.

    **Why this exists.** A run that retrieved 24 papers reported 9. The subagent's prompt asks it
    to report everything and rank rather than filter, and it filtered anyway — the same way the
    prompt asked it to use its tools and it did not (`middleware/search_first.py`). A prompt is a
    request the model is free to decline, so what must not be dropped cannot be left to one.

    Matched by link first, then by the paper's title appearing in the citation, because a model
    that keeps a paper usually rewrites its reference a little and almost never its identifier.
    """
    shown_links = {
        (source.get("link") or "").strip()
        for source in sources
        if isinstance(source, dict) and (source.get("link") or "").strip()
    }
    shown_text = [_key(source.get("citation", "")) for source in sources if isinstance(source, dict)]
    missing = []
    for paper in papers:
        link = (paper.get("link") or "").strip()
        if link and link in shown_links:
            continue
        title = _key(paper.get("title", ""))
        if title and any(title in citation for citation in shown_text):
            continue
        missing.append(paper)
    return missing


async def get_paper_tools() -> list[Any]:
    """The paper tools the academic researcher should have."""
    return [find_papers]
