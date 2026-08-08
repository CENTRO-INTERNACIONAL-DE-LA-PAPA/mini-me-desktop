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
import contextvars
import functools
import json
import logging
import re
from typing import Any

logger = logging.getLogger(__name__)

#: The link form that redirects to the canonical paper page. See `_paper_ref`.
CORPUS_URL = "https://api.semanticscholar.org/CorpusID:{}"

#: Papers seen in Asta results during the current run, as ``{normalised title: corpus id}``.
#:
#: A `ContextVar` so two concurrent turns cannot read each other's papers — a citation matched
#: against another conversation's search results would be exactly the wrong-paper failure this
#: file exists to remove.
_seen: contextvars.ContextVar[dict[str, str]] = contextvars.ContextVar("minime_local_papers")

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
    """This run's recorded papers, creating the store on first use."""
    try:
        return _seen.get()
    except LookupError:
        store: dict[str, str] = {}
        _seen.set(store)
        return store


def remember(payload: Any) -> int:
    """Record every ``{corpusId, title}`` pair anywhere in an Asta result.

    Walks the whole structure rather than reaching for a known path. The snippet search returns
    ``{"data": [{"paper": {...}}]}`` today, and other Asta tools nest differently; a recursive walk
    keeps working when one of them changes shape, and costs nothing on a payload that has no
    corpus ids in it at all.
    """
    store = _papers()
    before = len(store)

    def walk(node: Any) -> None:
        if isinstance(node, dict):
            corpus = node.get("corpusId")
            title = node.get("title")
            if corpus is not None and isinstance(title, str) and title.strip():
                key = " ".join(_significant(title))
                if key:
                    store.setdefault(key, str(corpus))
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
    for key, corpus in _papers().items():
        want = key.split()
        if not want:
            continue
        ranked.append((sum(word in have for word in want) / len(want), corpus))
    if not ranked:
        return None
    ranked.sort(reverse=True)
    best, corpus = ranked[0]
    if best < 0.6:
        return None
    # Two plausible papers is not an answer.
    if len(ranked) > 1 and best - ranked[1][0] < 0.15:
        return None
    return CORPUS_URL.format(corpus)


def install_mcp(module) -> None:
    """Record the papers every Asta tool call returns.

    Patches `_wrap_mcp_tools` **and lets the original run afterwards**, so our recorder ends up
    *inside* upstream's capping wrapper. That ordering is the whole point: above it we would see
    the truncated result, or the 2 KB preview left when a large result is written to the sandbox
    (`mcp_tools.py:132` puts the `asta` threshold at 32 KB, and paper searches are hundreds of KB
    by that file's own comment). The corpus ids we need are in the part that gets cut.
    """
    original = getattr(module, "_wrap_mcp_tools", None)
    if original is None:
        logger.warning("minime_local: no _wrap_mcp_tools — sources keep the model's own links")
        return

    @functools.wraps(original)
    def _recording(tools):
        for tool in tools:
            coroutine = getattr(tool, "coroutine", None)
            if not asyncio.iscoroutinefunction(coroutine):
                continue

            async def _watched(*args, _inner=coroutine, **kwargs):
                result = await _inner(*args, **kwargs)
                # Never let bookkeeping break a tool call: a failure here costs a link, and
                # raising would cost the search.
                try:
                    observe(result)
                except Exception:  # noqa: BLE001
                    logger.debug("minime_local: could not read papers from a tool result")
                return result

            try:
                tool.coroutine = _watched
            except Exception:  # noqa: BLE001
                pass
        return original(tools)

    module._wrap_mcp_tools = _recording
    logger.warning("minime_local: recording the corpus id of every paper Asta returns")


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
            logger.warning(
                "minime_local: %d of %d sources carry the corpus id Asta returned",
                replaced,
                len(sources),
            )
        return produced

    middleware.after_agent = _linked
    logger.warning("minime_local: academic sources link through Semantic Scholar")
