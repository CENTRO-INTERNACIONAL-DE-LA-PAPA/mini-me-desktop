# Asta Query Strategy

Use the correct Asta tool for the question instead of defaulting to keyword
search every time.

## Tool selection

### `search_papers_by_relevance`

Use when:

- you have a topic, concept, or methodological question
- you need discovery rather than exact lookup

Best for:

- initial literature review
- finding candidate papers on a subject

### `search_paper_by_title`

Use when:

- you know the likely title
- you need to verify a citation

Best for:

- exact-paper lookup
- resolving title ambiguity

### `get_paper`

Use when:

- you already have a paper identifier
- you need the actual paper metadata and selected fields

Best for:

- retrieving abstract, authors, venue, year, TL;DR, references, or citations

### `get_citations`

Use when:

- you want to know who cited a paper later
- you need follow-up work or downstream influence

Best for:

- forward citation chasing
- tracking newer developments

### `search_authors_by_name`

Use when:

- the user asks about a researcher or lab
- author identity may be ambiguous

### `get_author_papers`

Use when:

- you already know the author ID
- you want their paper list within a date range

### `snippet_search`

Use when:

- you need evidence for a specific claim
- you need textual grounding for a method, result, or phrase

Best for:

- evidence-grounded synthesis
- checking whether papers explicitly discuss a specific concept

## Query shaping

- Keep keywords specific.
- Add method names, outcomes, or domain terms when useful.
- Use date filters when recency matters.
- Use venue filters when the field has trusted publication venues.
- Do not ask for unnecessary fields on every request.

## High-value sequence

For many questions, this is the best sequence:

1. `search_papers_by_relevance`
2. `get_paper`
3. `snippet_search`
4. `get_citations`

That pattern balances discovery, inspection, grounding, and expansion.
