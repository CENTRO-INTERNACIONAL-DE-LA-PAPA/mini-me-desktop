"""Give an academic source the identifier Asta actually returned.

**The request, in the researcher's words:** *"I dont care to have a doi url in the front end. If
asta give a corpusId, we can put that ID into an url from semanthic scholar and we can be
redirected to semanthic scholar."*

That is the right shape, and this file is what makes it possible on our side of the line.

# The defect

`AcademicSourceFinding.citation` (`backend/schemas.py:31`) asks the model for an *"APA-style or
equivalent citation"* — which needs a year, a journal, a volume, pages and a DOI. The Asta paper
search returns none of them. Measured against the live tool:

    paper keys: ['authors', 'corpusId', 'openAccessInfo', 'title']

A title, an author list, and a numeric ``corpusId``. So the model is handed a paper it cannot cite
and asked to cite it, and it supplies the missing fields from memory — which is the only move left
to it. Six references from one run were checked against Crossref: three DOIs resolved to *different
real papers* (one to a study of lichen symbioses), three matched nothing (docs §119, §120).

Meanwhile ``corpusId`` — the one identifier that genuinely arrived — was dropped before anything
downstream could see it.

# Why an overlay and not a fix in the checkout

Same reason as every other file here (§18): the Mini-Me checkout is reference material, bundled
unmodified, and a `git pull` there must never conflict with us. The proper fix belongs upstream and
is written up in `docs/upstream/mini-me/academic-sources-drop-the-corpus-id.md`. This is the bridge
until it lands.

# Why the identifier is captured at the tool and not reconstructed later

Because at the tool it is *known*, and everywhere after it is a guess. The alternative — take the
title out of the citation and search for it — is what the desktop client does as a repair, and it
carries all the uncertainty of a search: near-matches, ambiguity, an index that does not cover
books. Here the corpus id is sitting in the response the model is reading. Nothing needs inferring.

`api.semanticscholar.org/CorpusID:<n>` rather than the website path, because
`backend/theory_tools.py:_paper_ref` already established which of those resolves — and its comment
records that the other one *"sent users to the wrong paper"*. Somebody has paid for that once.
"""

from __future__ import annotations

import asyncio
import functools
import json
import logging
import re
from typing import Any

logger = logging.getLogger(__name__)

#: The link form that redirects to the canonical paper page. See `_paper_ref`.
CORPUS_URL = "https://api.semanticscholar.org/CorpusID:{}"

#: Papers seen in a search result, as ``{normalised title: link}``.
#:
#: **The value is a finished URL, not a corpus id, because there are now two ways a paper
#: arrives.** The MCP snippet search carries a ``corpusId`` and nothing else, so its link has to
#: be built here. `find_papers` carries a link already built from the publisher's record by
#: `backend/citations.py`, which prefers the DOI when the record has one — better than anything
#: this file could reconstruct. Storing the id would mean throwing that away and rebuilding a
#: worse link from a field the CLI path does not even return.
#:
#: **Process-global, and it started as a `ContextVar`.** The reasoning for a ContextVar was that
#: two concurrent turns must not read each other's papers. That reasoning was wrong twice over.
#:
#: It does not work. A `ContextVar` set inside a child task is invisible to the parent — copy on
#: write, one direction only — and LangGraph runs a tool call in a task while the middleware that
#: reads this runs outside it. Measured, not assumed:
#:
#:     await asyncio.create_task(tool_call())   # records one paper
#:     len(_papers())                           # 0
#:
#: So every source would have kept the model's invented link, and nothing would have said why —
#: the §114 failure exactly: an isolation mechanism that silently isolated the wrong thing.
#:
#: And it was not needed. Sharing is safe *because* the match is on the title: a citation only
#: takes a corpus id when it names that paper, and a paper named in one conversation is the same
#: paper when it is named in another. Cross-talk here can only produce the right answer sooner.
_seen: dict[str, str] = {}

#: Bounded, because this outlives a turn now. Old entries cost only memory, but a researcher who
#: leaves the app open for a week should not accumulate one without limit.
_MAX_PAPERS = 4_000

#: Words too common to help decide that two titles are the same work.
_NOISE = {
    "a", "an", "and", "as", "at", "by", "for", "from", "in", "is", "of", "on", "or", "the",
    "to", "with", "into", "its", "their", "this", "that", "using", "via", "between",
}


def _significant(text: str) -> list[str]:
    """A title reduced to the words worth comparing."""
    words = re.sub(r"[^a-z0-9]+", " ", (text or "").lower()).split()
    return [word for word in words if len(word) > 2 and word not in _NOISE]


def _papers() -> dict[str, str]:
    """Every paper recorded so far. See `_seen` for why this is not per-run."""
    if len(_seen) > _MAX_PAPERS:
        # Oldest first — dicts keep insertion order — so a long session drops what it has not
        # needed in a while rather than the search that just ran.
        for key in list(_seen)[: len(_seen) - _MAX_PAPERS // 2]:
            _seen.pop(key, None)
    return _seen


def _url_of(node: dict[str, Any]) -> str:
    """The link for one paper record, from whichever identifier its finder carries.

    Two shapes, because two tools now find papers. ``corpusId`` is what the MCP snippet search
    returns; ``link`` is what `find_papers` returns, already resolved against the record. Checked
    in that order so the MCP path behaves exactly as it did before this function existed.
    """
    corpus = node.get("corpusId")
    if corpus is not None and str(corpus).strip():
        return CORPUS_URL.format(str(corpus).strip())
    link = node.get("link")
    if isinstance(link, str) and link.strip().startswith("http"):
        return link.strip()
    return ""


def remember(payload: Any) -> int:
    """Record every identified paper anywhere in a search result.

    Walks the whole structure rather than reaching for a known path. The snippet search returns
    ``{"data": [{"paper": {...}}]}`` today, `find_papers` returns ``{"papers": [...]}``, and other
    Asta tools nest differently again; a recursive walk keeps working when one of them changes
    shape, and costs nothing on a payload that has no papers in it at all.
    """
    store = _papers()
    before = len(store)

    def walk(node: Any) -> None:
        if isinstance(node, dict):
            title = node.get("title")
            if isinstance(title, str) and title.strip():
                url = _url_of(node)
                key = " ".join(_significant(title))
                if url and key:
                    store.setdefault(key, url)
            for value in node.values():
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(payload)
    return len(store) - before


def _text_of(result: Any) -> list[str]:
    """Every text block in an MCP result, whatever shape it arrived in."""
    if isinstance(result, str):
        return [result]
    blocks = result[0] if isinstance(result, tuple) and result else result
    if not isinstance(blocks, list):
        return []
    found: list[str] = []
    for block in blocks:
        if isinstance(block, dict):
            text = block.get("text")
            if isinstance(text, str) and text:
                found.append(text)
        elif isinstance(block, str):
            found.append(block)
    return found


def observe(result: Any) -> int:
    """Pull papers out of one raw MCP tool result."""
    recorded = 0
    for text in _text_of(result):
        try:
            recorded += remember(json.loads(text))
        except (json.JSONDecodeError, ValueError):
            continue
    return recorded


def link_for(citation: str) -> str | None:
    """The Semantic Scholar link for a citation, if it names a paper this run actually found.

    Matched by asking *how much of a recorded title appears in this citation* — the citation is a
    whole reference, so a title it names is a subset of it. The threshold is deliberately high and
    a near-tie yields nothing: attaching the wrong link would reproduce the defect being fixed,
    with the backend's authority behind it rather than the model's.
    """
    have = set(_significant(citation))
    if not have:
        return None
    ranked: list[tuple[float, str]] = []
    for key, url in _papers().items():
        want = key.split()
        if not want:
            continue
        ranked.append((sum(word in have for word in want) / len(want), url))
    if not ranked:
        return None
    ranked.sort(reverse=True)
    best, url = ranked[0]
    if best < 0.6:
        return None
    # Two plausible papers is not an answer.
    if len(ranked) > 1 and best - ranked[1][0] < 0.15:
        return None
    return url


#: Appended to `academic_researcher`'s prompt.
#:
#: **Why a prompt and not more plumbing.** The subagent already holds every tool it needs. Its
#: MCP bundle arrives unfiltered (`backend/mcp_tools.py:413`), so `get_papers`,
#: `search_paper_by_title` and `search_papers_by_relevance` — all of which return a DOI, a year
#: and a venue as *fields* — are in its hands on every turn. It has simply never been told they
#: exist: its prompt says only *"use available tools"* and *"cite all claims with APA-format
#: references"* (`backend/subagents.py:36-47`).
#:
#: The document that does name them, `skills/research/SKILL.md:69-82`, is almost certainly never
#: delivered. Every subagent declares its skill one directory too deep —
#: `"skills": ["/skills/research/"]` — while the loader scans a path's *subdirectories* for a
#: `SKILL.md` (`deepagents/middleware/skills.py:749-762`). `research/` contains a file, not a
#: subdirectory, so nothing resolves. The coordinator's `skills=["/skills/"]` sits one level up
#: and loads all twelve.
#:
#: So the model reaches for the one tool whose purpose it can infer from the name — snippet
#: search — reads titles and authors, and supplies the year, journal, volume, pages and DOI from
#: memory, because it was asked for an APA citation and given no other way to produce one.
#:
#: This is the cheap experiment before the expensive one: name the tools, forbid the invention,
#: and see whether the identifiers come out right. If they do, the code-side search tool is
#: unnecessary. If they do not, that is evidence rather than a guess (docs §124).
IDENTIFIER_RULES = """

    ## Identifiers (mini-me local)

    Every paper you cite must be resolved before you cite it.

    You have tools that return bibliographic metadata as structured fields, not as prose:

      - `search_papers_by_relevance` - find papers by topic; returns metadata
      - `search_paper_by_title`      - find one paper by its title; returns metadata
      - `get_papers`                 - full metadata for a paper you already have an id for
      - `snippet_search`             - ~500-word passages of text; returns NO metadata

    `snippet_search` is for reading evidence. It does not return a DOI, a year, a venue or page
    numbers. If you cite a paper you found through it, look that paper up with
    `search_paper_by_title` or `get_papers` (its `corpusId` works as an id) and take the
    identifier from the result.

    Rules, in order of importance:

    1. **Never write a DOI from memory.** A DOI is an opaque string; one that looks right is not
       right. Use only a DOI a tool returned to you in this conversation. If no tool returned
       one, give no DOI at all - an incomplete citation is correctable, a confident wrong one is
       not.
    2. The same applies to the year, the journal, the volume and the page numbers. Report what
       the tools returned. Omit what they did not.
    3. **Cite only papers a tool returned in this conversation.** Do not add references from your
       own knowledge to round out the list, however relevant they seem. Fewer real sources is the
       correct answer; the source limit is a maximum, not a target.
    4. If you cannot verify a paper exists through these tools, leave it out and say in your
       summary that the evidence base was thin.

    Put the identifier in the `link` field of each source: a DOI as `https://doi.org/<doi>`, or,
    when you only have a corpus id, `https://api.semanticscholar.org/CorpusID:<id>`.
"""


def install_prompt(module) -> None:
    """Tell `academic_researcher` which tools return identifiers, and forbid inventing them.

    Appends rather than replaces: upstream's prompt sets the subagent's role and its source
    limit, and rewriting it here would silently drop whatever upstream adds next.

    `_build_runtime_subagents` spreads `**subagent` per request (`backend/subagents.py:492`), so
    editing the module-level dict at import time reaches every turn without touching the file.
    """
    entries = getattr(module, "subagents", None)
    if not isinstance(entries, (list, tuple)):
        logger.warning("minime_local: no subagents list to extend")
        return
    for entry in entries:
        if not isinstance(entry, dict) or entry.get("name") != "academic_researcher":
            continue
        prompt = entry.get("system_prompt")
        if not isinstance(prompt, str):
            logger.warning("minime_local: academic_researcher has no system_prompt to extend")
            return
        # Idempotent: `install()` patches an already-imported module as well as hooking future
        # imports, so this can run twice on one process.
        if "Identifiers (mini-me local)" in prompt:
            return
        entry["system_prompt"] = prompt + IDENTIFIER_RULES
        logger.warning(
            "minime_local: academic_researcher told which tools return identifiers"
        )
        return
    logger.warning("minime_local: no academic_researcher subagent found to extend")


def install_mcp(module) -> None:
    """Record the papers every Asta tool call returns.

    Patches `_wrap_mcp_tools` **and lets the original run afterwards**, so our recorder ends up
    *inside* upstream's capping wrapper. That ordering is the whole point: above it we would see
    the truncated result, or the 2 KB preview left when a large result is written to the sandbox
    (`mcp_tools.py:132` puts the `asta` threshold at 32 KB, and paper searches are hundreds of KB
    by that file's own comment). The corpus ids we need are in the part that gets cut.
    """
    # **Named from the file, not from memory.** The first version hooked `_wrap_mcp_tools`, which
    # does not exist and never did — it was a plausible name for a function whose *body* had been
    # read. It installed nothing, said so, and the corpus id was never captured. §113 exactly: a
    # wrapper that assumed something about code it does not own.
    #
    # A tuple because the real name is private and upstream may rename it. Ordered by what is
    # there today.
    candidates = ("_make_mcp_tools_resilient", "_wrap_mcp_tools")
    # **The name is kept, not just the function.** The first fix looked up the right name and
    # then assigned the wrapper back to the wrong one — so `_recording` was stored under an
    # attribute nothing calls, the installer logged success, and not one corpus id was recorded.
    # A rename fixed in one of its two places is a rename not fixed.
    found = next((name for name in candidates if getattr(module, name, None)), None)
    original = getattr(module, found) if found else None
    if found is None or original is None:
        # **Reports what *is* there.** The previous failure said only that the name it wanted was
        # absent, which named the guess and not the fact — so the log identified the symptom and
        # left the answer in the file it had just failed to read.
        present = [
            name
            for name in dir(module)
            if name.startswith("_") and callable(getattr(module, name, None))
        ]
        logger.warning(
            "minime_local: none of %s in %s — sources keep the model's own links; "
            "candidates present: %s",
            candidates,
            module.__name__,
            present,
        )
        return

    @functools.wraps(original)
    def _recording(tools):
        wrapped: list[str] = []
        for tool in tools:
            # **Read here, not borrowed from upstream's loop.** The previous version referenced a
            # bare `name` — which exists in `_make_mcp_tools_resilient`, whose body I had read,
            # and not in this one. Python evaluates a default argument when the `def` executes, so
            # it raised `NameError: name 'name' is not defined` the moment the tool list was
            # wrapped, and every turn failed with "An internal error occurred". Shipped after
            # checking only that the file parsed — `ast.parse` proves syntax, never that a line
            # runs (docs §128).
            name = getattr(tool, "name", "<unknown>")
            coroutine = getattr(tool, "coroutine", None)
            if not asyncio.iscoroutinefunction(coroutine):
                continue

            async def _watched(*args, _inner=coroutine, _tool=name, **kwargs):
                result = await _inner(*args, **kwargs)
                # Never let bookkeeping break a tool call: a failure here costs a link, and
                # raising would cost the search.
                try:
                    found = observe(result)
                except Exception:  # noqa: BLE001
                    logger.warning("minime_local: could not read papers from %s", _tool)
                    return result
                # **Logged per call, and this is the diagnostic that was missing.** The only
                # signal before this was "0 of 6 sources carry the corpus id", which is the
                # *outcome* and not the cause: it reads identically whether no search ran at
                # all, or ten papers were recorded and no citation matched one of them. Those
                # need completely different fixes, so they must not produce the same line
                # (docs §81).
                logger.warning(
                    "minime_local: %s returned %d paper(s) with a corpus id (%d known so far)",
                    _tool,
                    found,
                    len(_papers()),
                )
                return result

            try:
                tool.coroutine = _watched
                wrapped.append(name)
            except Exception:  # noqa: BLE001
                logger.warning("minime_local: could not wrap %s for recording", name)
        # **Named, not counted.** "0 of 7 sources carry the corpus id" told us the outcome and the
        # per-call line told us no wrapped tool ran — but that still had two causes: the tools were
        # never wrapped, or they were wrapped and the model never called one. This says which, and
        # names them, so "wrapped nothing" and "wrapped seven and none was used" cannot look alike
        # (docs §81, for the fourth time this week).
        logger.warning(
            "minime_local: wrapped %d of %d tool(s) for recording: %s",
            len(wrapped),
            len(list(tools)),
            ", ".join(wrapped) or "none",
        )
        return original(tools)

    setattr(module, found, _recording)
    logger.warning(
        "minime_local: recording the corpus id of every paper Asta returns (via %s)", found
    )


def install_papers(module) -> None:
    """Record what `find_papers` returns, and say in the log that it ran.

    **This is the tool the wrapper above cannot see.** `install_mcp` hooks the function that wraps
    the *MCP* bundle, and `find_papers` is ours — a plain `@tool` in `backend/paper_tools.py`. It
    never passed through that path, so a run that used it recorded nothing and logged nothing, and
    the only line left in the log was *"0 of 7 sources carry the corpus id"* — which is emitted
    just as readily when the search worked perfectly as when no search happened at all.

    Two tools that find papers and one that is watched is not a diagnostic; it is a coin toss
    dressed as evidence. Its own `logger.info` line does not close the gap either: everything this
    file has ever seen reach the backend log arrived at WARNING, so an INFO line is a message
    written to a channel nobody has confirmed is open.

    Mutates the tool object rather than rebinding the module attribute, because `backend/agent.py`
    does ``from backend.paper_tools import find_papers`` — by the time this runs, the agent already
    holds the object, and replacing the name in the module would patch a reference nothing reads
    (docs §125).
    """
    tool = getattr(module, "find_papers", None)
    coroutine = getattr(tool, "coroutine", None) if tool is not None else None
    if tool is None or not asyncio.iscoroutinefunction(coroutine):
        logger.warning(
            "minime_local: no async find_papers to record in %s — a run that uses it will look "
            "identical to a run that searched nothing",
            module.__name__,
        )
        return

    @functools.wraps(coroutine)
    async def _watched(*args, **kwargs):
        result = await coroutine(*args, **kwargs)
        try:
            payload = json.loads(result) if isinstance(result, str) else result
            papers = payload.get("papers") or [] if isinstance(payload, dict) else []
            recorded = remember(payload)
        except Exception:  # noqa: BLE001
            # Bookkeeping never costs a search — the same rule the MCP wrapper follows.
            logger.warning("minime_local: could not read papers from find_papers")
            return result
        logger.warning(
            "minime_local: find_papers returned %d paper(s), %d newly recorded (%d known so far)",
            len(papers),
            recorded,
            len(_papers()),
        )
        return result

    try:
        tool.coroutine = _watched
    except Exception:  # noqa: BLE001
        logger.warning("minime_local: could not wrap find_papers for recording")
        return
    logger.warning("minime_local: recording the link of every paper find_papers returns")


def install_artifacts(module) -> None:
    """Put the recorded link on each source artifact.

    Wraps `ArtifactCaptureMiddleware.after_agent`, which is where a subagent's structured output
    becomes the `sources` list the desktop app reads (`backend/middleware/artifacts.py:215`).
    Rewriting the artifact rather than the model's structured response, because the artifact is
    the thing that leaves the backend.
    """
    middleware = getattr(module, "ArtifactCaptureMiddleware", None)
    if middleware is None:
        logger.warning("minime_local: no ArtifactCaptureMiddleware to extend")
        return
    original = getattr(middleware, "after_agent", None)
    if original is None:
        logger.warning("minime_local: ArtifactCaptureMiddleware has no after_agent to extend")
        return

    @functools.wraps(original)
    def _linked(self, state, runtime):
        produced = original(self, state, runtime)
        try:
            sources = (produced or {}).get("artifacts", {}).get("sources") or []
        except AttributeError:
            return produced
        replaced = 0
        for source in sources:
            if not isinstance(source, dict):
                continue
            found = link_for(source.get("citation") or "")
            if found:
                # **Overwrites whatever the model put here.** That field is the one it is least
                # able to fill: it has the title and the authors, and it has never seen a DOI.
                source["link"] = found
                replaced += 1
        if sources:
            # Logged on success as well as failure. Three attempts at the subagent registry were
            # misdiagnosed because its installer spoke only when it broke, so "absent", "never
            # reached" and "ran and did nothing" produced identical evidence (docs §81).
            known = len(_papers())
            if known:
                logger.warning(
                    "minime_local: %d of %d sources relinked to a paper the search returned "
                    "(%d recorded)",
                    replaced,
                    len(sources),
                    known,
                )
            else:
                # **Not a count of failures — a statement that there is nothing to count.** The
                # old line said "0 of 7 sources carry the corpus id" here, which reads as seven
                # broken links. It is the same line the good case prints: `find_papers` builds
                # its link from the record, so a run where every citation is correct also records
                # no corpus id and also prints zero. A message that cannot distinguish the fix
                # from the defect sent four days into looking at the wrong half of the system.
                logger.warning(
                    "minime_local: no search recorded — the %d source(s) keep the links the "
                    "subagent supplied, which are its own unless find_papers logged above",
                    len(sources),
                )
        return produced

    middleware.after_agent = _linked
    logger.warning("minime_local: academic sources link through Semantic Scholar")
