# Discovery Workflow

Use this workflow when the task is to find and recommend datasets.

## CIP-first workflow

For CIP Dataverse, use this sequence:

1. `SearchCIPDataverse`
   - Use targeted search queries.
   - **Do not pass `output_filename`.** The middleware sets it, and it sets the
     one name the reader looks for. This document used to tell you to set it
     yourself; that instruction moved into code because a rule two tools must
     agree on is not something to remember across a long episode.
   - Prefer dataset-level search unless the user specifically wants files.
   - Several narrow searches are better than one broad one, and every result
     from every search this turn is kept — a dataset found on your first query
     is still available to recommend after your fifth.

2. `read_search_results`
   - **Call it with no arguments.** The middleware supplies the path.
   - This document used to say *"always call this with
     `filename="dataverse_search.json"`"*. That argument **does not exist** —
     the tool takes `file_path`, and passing `filename` is a hard error that
     made every read fail for nine days (docs §220/§221). The middleware now
     strips it, so following the old instruction cost nothing but your
     attention. It is corrected here so it costs neither.
   - Identify the top candidate datasets.
   - **Copy** each dataset's `global_id` verbatim as its DOI/persistent ID, and
     extract title, description snippets, authors and other visible metadata
     when present. A dataset whose record carries no identifier field is
     omitted, not guessed at — see `metadata_extraction_rules.md`.
   - Use dataset-level fields already present in search results such as
     `fileCount`, keywords, subjects, publications, repository context, and
     authors whenever available.

3. `list_dataset_files`
   - Use for shortlisted datasets only.
   - Use when you need file-level detail beyond the search results.
   - Check filenames, content types, and restricted/public status.
   - Use this to strengthen dataset recommendations, not to trigger download.

4. Return a short ranked recommendation set.

## Current scope

This skill is currently scoped to CIP Dataverse only.

Do not use it as the default workflow for other Dataverse instances unless the
skill is explicitly revised later.

## Recommendation format

Prefer a short list such as:
- best match
- strong alternative
- fallback option

For each candidate, explain:
- why it matches the user goal
- whether the metadata is strong enough for confident reuse
- whether the files appear accessible and relevant for analysis

## When to stop

Stop once you have enough evidence to recommend a small set of good datasets.
Do not keep searching just to increase the number of results.
