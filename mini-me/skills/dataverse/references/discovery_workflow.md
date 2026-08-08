# Discovery Workflow

Use this workflow when the task is to find and recommend datasets.

## CIP-first workflow

For CIP Dataverse, use this sequence:

1. `SearchCIPDataverse`
   - Use targeted search queries.
   - **Always set `output_filename="dataverse_search.json"`.** Never use any
     other name. Successive searches in the same conversation simply
     overwrite this file.
   - Prefer dataset-level search unless the user specifically wants files.

2. `read_search_results`
   - **Always call this with `filename="dataverse_search.json"`** (the same
     fixed name used in step 1). Do not invent, shorten, or vary the
     filename — any mismatch causes `read_search_results` to fail with
     "File ... not found".
   - Identify the top candidate datasets.
   - Extract dataset title, DOI/persistent ID, description snippets, authors,
     and other visible metadata when present.
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
