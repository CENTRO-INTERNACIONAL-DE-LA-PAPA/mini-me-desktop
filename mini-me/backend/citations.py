"""Build a reference from a paper record, so no model has to remember one.

# The defect this removes

`AcademicSourceFinding.citation` (`backend/schemas.py`) asks the model for an *"APA-style or
equivalent citation for the source"*. An APA reference needs the year, the journal, the volume, the
pages and a DOI.

`snippet_search` — the Asta tool the academic researcher reaches for — returns none of them:

    paper keys: ['authors', 'corpusId', 'openAccessInfo', 'title']

So the model is handed a paper it cannot cite and asked to cite it. It supplies the missing fields
from memory, which is the only move available to it, and it is good at that: the journal, the year
and the volume usually come out right. The DOI does not, because a DOI suffix is a high-entropy
string carrying no meaning — the first thing a language model loses and the last thing a reader can
check by eye.

Six references from one run, resolved against Crossref:

| the citation claimed | the DOI is registered to |
|---|---|
| Hijmans & Spooner 2001, Am. J. Bot. 88(11) | *Algal switching among lichen symbioses* |
| Plaisted & Hoopes 1989, Am. Potato J. 66 | a different paper; the right DOI is `BF02853982` |
| Vargas et al. 2012, AJPR 89(6) | a different article in that same issue |
| three others | not registered at all |

Every field a reader would sanity-check was right. Only the identifier was wrong, and in half the
cases it resolved — to a real paper, on a plausible subject.

# Why this is a formatting module and not a validator

Semantic Scholar already returns everything a reference needs:

    journal  {"name": "Theoretical and Applied Genetics", "volume": "110", "pages": "252-258"}
    year     2004
    authors  ['W. Smilde', 'G. Brigneti', 'L. Jagger', 'Sara Perkins', 'Jonathan D. G. Jones']
    DOI      10.1007/s00122-004-1820-8

So a citation is a formatting job. Building it here means the model *receives* finished references
instead of composing them, which removes the failure rather than detecting it afterwards — there is
no field left for it to invent.

Measured across seventeen papers in six unrelated fields, every built DOI resolved at Crossref and
every title and year matched the registry. Where Semantic Scholar carries a volume or a page range
it agreed with Crossref; where it does not, the reference renders without one. **Nothing is
guessed.** An incomplete reference is correctable by a person; a complete wrong one is not.
"""

from __future__ import annotations

from typing import Any

#: Surname particles that belong with the family name rather than the given names.
#:
#: Without these, "M. del R. Herrera" and "R. de Paz" — both CIP authors — come out as
#: "R. Herrera, M. del" and "Paz, R. de", which is a misattribution rather than a formatting slip.
#: Lowercase only: an uppercase "De" is usually part of the surname proper.
PARTICLES = {
    "de", "del", "della", "der", "di", "da", "das", "dos", "du", "la", "le", "van",
    "von", "vander", "ter", "ten", "bin", "ibn", "al", "el", "y", "e",
}


def _split_name(name: str) -> tuple[str, list[str]]:
    """``"Jonathan D. G. Jones"`` -> ``("Jones", ["Jonathan", "D.", "G."])``.

    Semantic Scholar returns names in natural order and in two styles — ``"W. Smilde"`` with the
    given names already initialised, and ``"Jonathan D. G. Jones"`` spelled out — so this handles
    both without turning either into the other's mistake.
    """
    parts = (name or "").split()
    if not parts:
        return "", []
    cut = len(parts) - 1
    while cut > 0 and parts[cut - 1].lower().strip(".") in PARTICLES:
        cut -= 1
    return " ".join(parts[cut:]), parts[:cut]


def _initials(given: list[str]) -> str:
    """``["Jonathan", "D.", "G."]`` -> ``"J. D. G."``.

    An initial is left alone; a spelled-out name is reduced to one. A particle keeps its spelling —
    initialising "de" to "d." would be inventing part of a name.
    """
    out: list[str] = []
    for part in given:
        bare = part.strip(".")
        if not bare:
            continue
        if bare.lower() in PARTICLES:
            out.append(bare)
        elif len(bare) == 1:
            out.append(f"{bare}.")
        else:
            out.append(f"{bare[0]}.")
    return " ".join(out)


def authors(names: list[str]) -> str:
    """An APA author list.

    APA 7: surname first, an ampersand before the last, and for twenty-one or more authors the
    first nineteen, an ellipsis, then the final one. Large collaborations are common in genomics
    and that rule exists so a reference does not run to a paragraph.
    """
    formatted: list[str] = []
    for name in names:
        surname, given = _split_name(name)
        if not surname:
            continue
        initials = _initials(given)
        formatted.append(f"{surname}, {initials}" if initials else surname)

    if not formatted:
        return ""
    if len(formatted) == 1:
        return formatted[0]
    if len(formatted) > 20:
        return ", ".join(formatted[:19]) + ", … " + formatted[-1]
    return ", ".join(formatted[:-1]) + f", & {formatted[-1]}"


def _volume(raw: str) -> str:
    """``"110"`` -> ``"110"``, and ``"88 11"`` -> ``"88(11)"``.

    Semantic Scholar packs the issue into the volume separated by a space, so *American Journal of
    Botany* 88(11) arrives as ``"88 11"`` and would render as ``", 88 11,"``. Only that exact
    two-number shape is rewritten; a volume like ``"12 Suppl 3"`` is passed through, because
    reformatting it would be a guess.
    """
    parts = raw.split()
    if len(parts) == 2 and all(part.isdigit() for part in parts):
        return f"{parts[0]}({parts[1]})"
    return raw


def _clean(value: Any) -> str:
    """A field as a trimmed single-spaced string, or empty. Never the word "None".

    Collapsing whitespace is load-bearing, not tidiness: Semantic Scholar indents some fields
    across newlines, so ``pages`` arrives as ``"\\n          1-8\\n        "`` and would otherwise
    put a line break in the middle of a reference.
    """
    if value is None:
        return ""
    return " ".join(str(value).split()).strip()


def apa(paper: dict[str, Any]) -> str:
    """One Semantic Scholar record as an APA reference.

    Every part is omitted when the record does not carry it. That is the whole discipline of this
    module: a reference missing its volume is visibly incomplete and a researcher can fill it in,
    while one carrying a *plausible* volume cannot be told from a correct one.
    """
    journal = paper.get("journal") if isinstance(paper.get("journal"), dict) else {}
    journal = journal or {}

    names = [
        _clean(a.get("name") if isinstance(a, dict) else a)
        for a in (paper.get("authors") or [])
    ]
    who = authors([n for n in names if n])
    year = _clean(paper.get("year"))
    title = _clean(paper.get("title"))
    where = _clean(journal.get("name")) or _clean(paper.get("venue"))
    volume = _volume(_clean(journal.get("volume")))
    pages = _clean(journal.get("pages"))
    doi = _clean((paper.get("externalIds") or {}).get("DOI"))

    out = ""
    if who:
        out += who + " "
    out += f"({year})." if year else "(n.d.)."
    if title:
        out += f" {title}" + ("" if title[-1] in ".?!" else ".")
    if where:
        out += f" {where}"
        if volume:
            out += f", {volume}"
        if pages:
            out += f", {pages.replace('-', '–')}"
        out += "."
    if doi:
        out += f" https://doi.org/{doi}"
    return out.strip()


def link(paper: dict[str, Any]) -> str:
    """Where the paper can be read, through Semantic Scholar.

    A DOI when the record has one, the corpus id otherwise. Both forms 301-redirect to the paper's
    own page — verified against the live service — and a corpus id is present on essentially
    everything the search returns, which is more than can be said for the DOI.

    ``api.semanticscholar.org`` rather than the website path, for the reason already recorded in
    `theory_tools._paper_ref`: ``/paper/CorpusID:<n>`` resolves unreliably and *"sent users to the
    wrong paper"*.
    """
    ids = paper.get("externalIds") or {}
    doi = _clean(ids.get("DOI"))
    if doi:
        return f"https://api.semanticscholar.org/DOI:{doi}"
    corpus = _clean(ids.get("CorpusId")) or _clean(paper.get("corpusId"))
    return f"https://api.semanticscholar.org/CorpusID:{corpus}" if corpus else ""


def citable(node: Any) -> bool:
    """Whether an object is a paper record carrying enough to cite.

    A title plus at least one field that makes a reference more than a title. Deliberately not
    "has a corpusId": a snippet-search result has one and no year, venue or DOI, and writing
    ``(n.d.). A title.`` over it would replace a missing citation with a threadbare one carrying
    the same authority.
    """
    if not isinstance(node, dict) or not _clean(node.get("title")):
        return False
    journal = node.get("journal") if isinstance(node.get("journal"), dict) else {}
    return bool(
        node.get("year")
        or _clean(node.get("venue"))
        or _clean((journal or {}).get("name"))
        or _clean((node.get("externalIds") or {}).get("DOI"))
    )


def describe(paper: dict[str, Any]) -> dict[str, str]:
    """One retrieved paper, as the model and the researcher should both see it.

    The same object serves both on purpose: what reaches the answer and what reaches the sources
    panel cannot then disagree about which paper is which.
    """
    tldr = paper.get("tldr") or {}
    return {
        "citation": apa(paper),
        "link": link(paper),
        "title": _clean(paper.get("title")),
        "summary": _clean(tldr.get("text") if isinstance(tldr, dict) else tldr),
        "abstract": _clean(paper.get("abstract")),
    }
