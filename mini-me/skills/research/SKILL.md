---
name: research
description: >-
  Use Asta to discover, inspect, and synthesize scientific literature with
  grounded evidence and proper attribution. Use when the task is to find
  relevant papers, verify a known paper, trace citations, explore an author's
  work, or ground claims in actual paper text snippets.
---

# Academic Research Guidelines

Use this skill when the goal is to answer a research question with scientific
literature.

This skill is **tool-first**. Use Asta tools to discover, inspect, expand, and
ground evidence. Do not rely on vague memory when the literature can be checked
directly.

## Goals

- Find the most relevant scientific papers for the question.
- Inspect paper metadata and content efficiently.
- Ground claims in actual paper text when possible.
- Preserve citation discipline and uncertainty.
- Stay within Asta usage constraints.

## Boundaries

This skill does **not** own:

- data cleaning
- statistical modeling
- predictive modeling
- final report writing

If the task becomes about analyzing a local dataset rather than reviewing
literature, hand off to the appropriate analysis subagent.

## Compliance and usage constraints

Read [references/compliance_and_attribution.md](references/compliance_and_attribution.md)
before using the tools extensively.

Important rules:

- Use Asta only for non-commercial research use within the granted terms.
- Respect the stated rate limit. Keep queries targeted and avoid brute-force
  retrieval loops.
- Preserve Asta attribution requirements and the AstaBench citation requirement
  for published materials that build on this work.

## Preferred workflow

1. Clarify the research question.
2. Choose the right Asta tool path.
3. Retrieve a small set of high-value papers first.
4. Inspect the strongest candidates.
5. Use snippet-grounded evidence for important claims.
6. Expand through citations or authors only when needed.
7. Return concise findings with citations and caveats.

## Choose the right tool

Read [references/asta_query_strategy.md](references/asta_query_strategy.md)
before searching.

**Start with `find_papers`.** It searches the same corpus and returns each paper with its
citation *already written from the publisher's record*, plus a one-sentence summary of what the
paper found and its abstract.

- `find_papers`
  - for any search where you intend to cite what you find — which is most of them

Use the `citation` field exactly as it is given to you. Do not rewrite it, and never write a DOI,
a year, a volume or a page number yourself: those come from the record, and one you compose will
look correct and point at a different paper. If a citation is missing a field, that field is
missing from the record — an incomplete reference is better than an invented one. Pass `link`
through unchanged.

Report every paper the search returned and say which ones bear on the question. Deciding what is
relevant is the reader's job; do not silently drop results.

The remaining Asta tools stay available for the jobs `find_papers` does not do:

- `search_papers_by_relevance`
  - for topic discovery when you have a research question or keyword query
- `search_paper_by_title`
  - for verifying or locating a known paper title
- `get_paper`
  - for retrieving the detailed record once you have a paper ID
- `get_citations`
  - for forward citation chasing and follow-up work
- `search_authors_by_name`
  - for finding the correct researcher profile
- `get_author_papers`
  - for author-centric literature reviews once you have an author ID
- `snippet_search`
  - for grounding specific claims, methods, or findings in paper text

## Research patterns

### Topic review

Use when the user asks a broad question such as:

- what methods exist for X?
- what does the literature say about Y?

Default flow:

1. `search_papers_by_relevance`
2. `get_paper` on the strongest hits
3. `snippet_search` for exact methodological or result claims
4. `get_citations` on the most important paper if recent follow-up work matters

### Known-paper verification

Use when the user provides or implies a specific paper.

Default flow:

1. `search_paper_by_title`
2. `get_paper`
3. `get_citations` if follow-up studies are needed

### Author-centric review

Use when the user asks about a specific scientist or lab.

Default flow:

1. `search_authors_by_name`
2. `get_author_papers`
3. `get_paper` on the most relevant results

### Claim grounding

Use when the user asks for evidence supporting a specific claim, phrase, method,
or result.

Default flow:

1. `snippet_search`
2. `get_paper` for the source paper metadata
3. optionally `get_citations` if the claim needs more recent support

## Query discipline

- Start with 1-3 precise queries, not many weak ones.
- Use publication date restrictions when recency matters.
- Use venue restrictions when the field has clear flagship journals or venues.
- Request only the fields you need when inspecting papers.
- Expand the search breadth only if the first pass is inadequate.

## Evidence synthesis

Read [references/evidence_synthesis_rules.md](references/evidence_synthesis_rules.md)
before writing the final answer.

Rules:

- Distinguish between direct evidence and inference.
- Prefer grounded evidence from `snippet_search` for exact claims.
- Do not overstate one paper as field consensus.
- If the literature is mixed, say it is mixed.
- If recency matters, prioritize recent evidence but note seminal older work
  when relevant.

## Expected output

Return:

- the research question addressed
- the main papers or lines of evidence used
- concise findings
- explicit caveats or disagreements in the literature
- properly formatted citations or citation-ready metadata
