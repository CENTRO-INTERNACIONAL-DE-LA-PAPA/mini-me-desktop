"""Build an APA citation from fields, so nothing has to remember one.

**The request, in the researcher's words:** *"We should have a code to make an apa citation with
code not let it AI build it from memory. Also, the ai should recieve the apa citations and link
asta gave us so I just see a list of papers."* And, on the idea of dropping unverifiable
references: *"It doesnt make sense to have fewer sources. We should get all the papers that asta
finds and is up to the scientist to select and drop the ones they want."*

Both corrections land, and together they invert the design.

# What was wrong with the previous shape

Every fix before this one accepted that the **model** produces the citation and tried to check it
afterwards: read the structured link instead of the prose (§119), carry the corpus id through
(§121), verify the DOI against Crossref and repair it (§122), tell the model which tools return
identifiers (§124). Each was an improvement and none addressed the premise — that a language model
was being asked to emit bibliographic data it had not been given.

It is not a hard problem once it is turned around. Semantic Scholar returns every field an APA
reference needs:

    journal  {"name": "Theoretical and Applied Genetics", "volume": "110", "pages": "252-258"}
    year     2004
    authors  ['W. Smilde', 'G. Brigneti', 'L. Jagger', 'Sara Perkins', 'Jonathan D. G. Jones']
    DOI      10.1007/s00122-004-1820-8

So the citation is a formatting job, and formatting is what code is for. The model then *receives*
finished references rather than composing them, which removes the failure rather than detecting
it: there is no field left for it to invent.

# And every paper, not a filtered subset

The earlier plan was to drop citations that matched no retrieved paper. That is the wrong end.
Deciding which of twelve retrieved papers are relevant is the researcher's judgement, and hiding
some of them behind a model's opinion of relevance is the same mistake in a different coat.
Everything the search returned is listed; the scientist keeps what they want.

# What this deliberately does not do

No field is invented and none is guessed. A record with no volume renders without a volume rather
than with a plausible one — an incomplete reference is correctable by a person, and a complete
wrong one is not (docs §126).
"""

from __future__ import annotations

from typing import Any

#: Surname particles that belong with the family name rather than the given names.
#:
#: Without these, "M. del R. Herrera" and "R. de Paz" — both real Mini-Me authors from CIP — come
#: out as "R. Herrera, M. del" and "Paz, R. de", which is a misattribution rather than a formatting
#: slip. Lowercase-only on purpose: an uppercase "De" is usually part of the surname proper.
PARTICLES = {
    "de", "del", "della", "der", "di", "da", "das", "dos", "du", "la", "le", "van",
    "von", "vander", "ter", "ten", "bin", "ibn", "al", "el", "y", "e",
}


def _split_name(name: str) -> tuple[str, list[str]]:
    """`"Jonathan D. G. Jones"` → `("Jones", ["Jonathan", "D.", "G."])`.

    Semantic Scholar returns names in natural order and in two styles — `"W. Smilde"` with the
    given names already initialised, and `"Jonathan D. G. Jones"` spelled out — so this has to
    handle both without turning either into the other's mistake.
    """
    parts = (name or "").split()
    if not parts:
        return "", []
    # Walk back over any particles so they travel with the surname.
    cut = len(parts) - 1
    while cut > 0 and parts[cut - 1].lower().strip(".") in PARTICLES:
        cut -= 1
    return " ".join(parts[cut:]), parts[:cut]


def _initials(given: list[str]) -> str:
    """`["Jonathan", "D.", "G."]` → `"J. D. G."`.

    A part that is already an initial is left alone; a spelled-out name is reduced to one. A
    particle keeps its own spelling — initialising "de" to "d." would be inventing a name.
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

    APA 7 rules that matter here: surname first, ampersand before the last, and for twenty-one or
    more authors the first nineteen, an ellipsis, then the final one. Large collaborations are
    common in genomics and the rule exists precisely so a reference does not run to a paragraph.
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
    """`"110"` → `"110"`, and `"88 11"` → `"88(11)"`.

    Semantic Scholar packs the issue into the volume string separated by a space, so a record for
    *American Journal of Botany* 88(11) arrives as `"88 11"` and renders as `", 88 11,"` — which
    is not a volume anyone recognises. Only the exact two-number shape is rewritten; anything else
    is passed through untouched, because a volume like `"12 Suppl 3"` is better left alone than
    reformatted on a guess.
    """
    parts = raw.split()
    if len(parts) == 2 and all(part.isdigit() for part in parts):
        return f"{parts[0]}({parts[1]})"
    return raw


def _clean(value: Any) -> str:
    """A field as a trimmed string, or empty. Never the word "None"."""
    if value is None:
        return ""
    return " ".join(str(value).split()).strip()


def apa(paper: dict[str, Any]) -> str:
    """One Semantic Scholar record as an APA reference.

    Every part is omitted when the record does not carry it. That is the whole discipline of this
    module: a citation missing its volume is obviously incomplete and a researcher can fill it in,
    while a citation carrying a *plausible* volume is indistinguishable from a correct one.
    """
    journal = paper.get("journal") or {}
    if not isinstance(journal, dict):
        journal = {}

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
        # A title already ending in punctuation does not take another full stop.
        out += f" {title}" + ("" if title[-1] in ".?!" else ".")
    if where:
        out += f" {where}"
        if volume:
            out += f", {volume}"
        if pages:
            # An en dash, as APA sets page ranges.
            out += f", {pages.replace('-', '–')}"
        out += "."
    if doi:
        out += f" https://doi.org/{doi}"
    return out.strip()


def link(paper: dict[str, Any]) -> str:
    """Where this paper can be read, always through Semantic Scholar.

    A DOI when the record has one, the corpus id otherwise. Both forms 301-redirect to the paper's
    own page — verified against the live service — and the corpus id is present on essentially
    everything the search returns, which is more than can be said for the DOI (docs §122).
    """
    ids = paper.get("externalIds") or {}
    doi = _clean(ids.get("DOI"))
    if doi:
        return f"https://api.semanticscholar.org/DOI:{doi}"
    corpus = _clean(ids.get("CorpusId")) or _clean(paper.get("corpusId"))
    return f"https://api.semanticscholar.org/CorpusID:{corpus}" if corpus else ""


def citable(node: Any) -> bool:
    """Whether this object is a paper record carrying enough to cite.

    A title plus at least one of the fields that make a citation more than a title. Deliberately
    not "has a corpusId": a snippet-search result has one and no year, venue or DOI, and writing
    `Smith. (n.d.). A title.` over it would replace a missing citation with a threadbare one while
    claiming the same authority.
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


def enrich(payload: Any) -> int:
    """Add `citation` and `link` to every paper record in a tool result, in place.

    **This is the wiring, and its shape is the point.** The model is not asked to call anything:
    a search result simply arrives with its reference already written. There is no path where it
    holds a paper and lacks the citation for it, so there is no path where it fills the gap from
    memory — which is what every previous fix was trying to detect after the fact.

    Walks the whole structure rather than a known path, because the seven Asta tools nest their
    papers differently and a walk keeps working when one of them changes shape.

    Returns how many records were enriched, so the caller can log a number rather than a claim
    (§81): "3 of 10" and "0 of 10" must not look alike.
    """
    count = 0

    def walk(node: Any) -> None:
        nonlocal count
        if isinstance(node, dict):
            if citable(node):
                built = apa(node)
                if built:
                    # `citation`, the field name the rest of the pipeline already uses, so the
                    # artifact layer and the desktop client need no new vocabulary.
                    node["citation"] = built
                    where = link(node)
                    if where:
                        node["link"] = where
                    count += 1
            for value in list(node.values()):
                walk(value)
        elif isinstance(node, list):
            for value in node:
                walk(value)

    walk(payload)
    return count


def describe(paper: dict[str, Any]) -> dict[str, str]:
    """One retrieved paper, as the model and the researcher should both see it.

    The same object serves both, deliberately: what reaches the transcript and what reaches the
    sources panel cannot then disagree about which paper is which.
    """
    tldr = paper.get("tldr") or {}
    return {
        "citation": apa(paper),
        "link": link(paper),
        "title": _clean(paper.get("title")),
        "summary": _clean(tldr.get("text") if isinstance(tldr, dict) else tldr),
        "abstract": _clean(paper.get("abstract")),
    }
