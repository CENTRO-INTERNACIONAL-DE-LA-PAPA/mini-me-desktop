# Academic sources drop the only identifier Asta returns, so the model invents a DOI

**Repo:** Mini-Me (`backend/`)
**Severity:** high — every literature citation the app produces can carry a DOI pointing at a
different paper, and it looks correct
**Found:** 2026-08-07

## Summary

`AcademicSourceFinding.citation` (`backend/schemas.py:31`) asks the model for an *"APA-style or
equivalent citation for the source"*. An APA citation needs the year, the journal, the volume, the
pages and a DOI.

The Asta MCP paper search returns **none of those**. Measured against the live tool:

```
$ asta papers snippet-search "late blight resistance Andean potato landraces" --limit 2
items: 2
top-level keys per item: ['paper', 'score', 'snippet']
paper keys: ['authors', 'corpusId', 'openAccessInfo', 'title']
```

A title, an author list, and a numeric `corpusId`. So the model is handed a paper it cannot cite
and asked for a citation. It supplies the missing fields from memory, which is the only thing left
to do, and `corpusId` — the one identifier that *was* returned — is dropped before anything
downstream can use it.

## What it produces

Six references from one real run, checked against Crossref (the registrar, not an index):

| The citation claims | The DOI is registered to |
|---|---|
| Hijmans & Spooner 2001, AJB **88(11), 2101-2112** | *Algal switching among lichen symbioses*, AJB 88(8) |
| Plaisted & Hoopes 1989, Am. Potato J. **66, 603-627** | `BF02853934` — a different paper; the right one is `BF02853982` |
| Vargas et al. 2012, AJPR **89(6)** | *Resistance to Aphids, Late Blight and Viruses…*, Davis et al., AJPR 89(6) |
| Lindqvist-Kreuze & Forbes 2018, ch. 14 | *Gender Topics on Potato Research and Development* — right book, wrong chapter |
| Douches et al. 1997, Potato Research 40(4) | not registered |
| Ellis et al. 2018, Euphytica 214 | not registered |

The bold figures are **correct**. Hijmans & Spooner really is volume 88, issue 11, pages
2101-2112; Plaisted & Hoopes really is volume 66, pages 603-627. The model reconstructs the
bibliographic details accurately from the title and authors it was given, and then produces a DOI,
which is a high-entropy string with no meaning in it and therefore the one field it cannot
reconstruct.

That is what makes this expensive rather than merely wrong: **every field a reader can
sanity-check comes out right.** The journal is right, the year is right, the volume is right. Only
the identifier is wrong, and it is the one thing nobody verifies by eye — and in three of six
cases it resolves, so the link works and opens a real paper on the right subject.

## The fix already exists in this repository

`backend/theory_tools.py:56-84`, `_paper_ref`, handles exactly this case for the theorizer path:

```python
elif corpus is not None:
    # Theorizer papers usually carry ONLY a numeric corpusId (no DOI/url).
    # S2 paper *pages* are keyed by a 40-char hash, and the website's
    # /paper/CorpusID:<n> path resolves UNRELIABLY (it sent users to the
    # wrong paper). The API endpoint api.semanticscholar.org/CorpusID:<n>
    # 302-redirects to the correct canonical paper page — verified across
    # ids — so link through that instead.
    url = f"https://api.semanticscholar.org/CorpusID:{corpus}"
```

Someone met this problem, worked out that `corpusId` is all that arrives, established which URL
form resolves correctly, and wrote it down. The academic-research path has no equivalent — nothing
between the MCP tool and `SourceArtifactPayload` carries `corpusId` at all.

Verified that the link form works, on the ID from the failing run:

```
$ asta papers get CorpusId:45447591
The past record and future prospects for the use of exotic potato germplasm | 1989 | American Potato Journal
DOI: 10.1007/BF02853982
```

## Suggested change

1. **Carry `corpusId` through the academic-research path.** Add it to `AcademicSourceFinding` as a
   field the *tool layer* fills — not the model — and populate `SourceArtifactPayload.link` with
   `https://api.semanticscholar.org/CorpusID:<n>`, the same way `_paper_ref` does. A link that
   redirects to the right paper is worth more than a DOI that resolves to the wrong one.

2. **Stop asking the model for fields it was not given.** `citation` is described as APA-style,
   which requires a year, a journal, a volume and pages that the search does not return. Either
   fetch them (`corpusId` → `/graph/v1/paper/CorpusID:<n>?fields=externalIds,year,venue,journal`
   is one call and returns the real DOI), or describe the field as what it can honestly be — title
   and authors — and let the client render the rest from structured data.

3. **Reconsider the 32 KB save-to-sandbox threshold for `asta`** (`mcp_tools.py:132`). Above it the
   model receives a pointer plus a 2 KB preview and must use code execution to read the rest. Paper
   searches are "hundreds of KB" by that file's own comment, so this is the normal path, not the
   exceptional one — and a subagent that does not read the file back is composing its answer from
   two kilobytes and its own memory. Worth checking how often the read-back actually happens.

## A note on `used_asta`

`backend/routes/rendering.py:343` defaults `used_asta` to `len(sources) > 0`, which controls a
report footer reading *"Academic literature search performed using Asta tools (Allen Institute for
AI). Please cite the AstaBench paper."* `sources` counts citation objects the **model** emitted,
so on the run above that footer would have credited Asta for six references it never returned.

Whatever is done about the identifiers, that test should not be the count of a model-produced
list. The desktop client now decides it from its own provenance record — which specialists
actually ran — and passes it explicitly.

## How this was found

A researcher noticed the DOI links in the desktop app did not open the papers they named, and
observed that Semantic Scholar shows a Corpus ID where our citations show a DOI. Checking followed
in two stages: first against Semantic Scholar, then — after the correct objection that the API
might be being used wrongly — against Crossref, which is authoritative about whether a DOI was ever
registered. Crossref agreed with Semantic Scholar on all six. The index was never at fault.
