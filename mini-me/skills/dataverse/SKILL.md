---
name: dataverse
description: >-
  Discover, inspect, and recommend datasets from CIP Dataverse using the
  Dataverse MCP. Use when the task is to search CIP datasets, summarize their
  metadata, and compare candidates. Do not use this skill for generic
  multi-Dataverse discovery, file download, data cleaning, analysis, or
  Dataverse curation workflows.
---

# Dataverse Explorer

Use this skill to find and evaluate datasets from CIP Dataverse before analysis.

This skill is discovery-first:
- find candidate datasets
- inspect their metadata
- inspect file availability for shortlisted datasets
- recommend the best options

Do not dump raw Dataverse results back to the user. Return concise, useful
dataset recommendations.
Do not use this skill as the default path for non-CIP Dataverse repositories.

## Scope

This subagent should primarily do:
- CIP dataset discovery
- metadata inspection
- file inspection for shortlisted datasets
- dataset selection and recommendation

This subagent should not default to:
- data cleaning
- exploratory analysis
- predictive or diagnostic modeling
- file download
- creating, publishing, deleting, or editing Dataverse records

## Tooling

Use the Dataverse MCP as the primary execution path.

Allowed MCP functions for this subagent:
- `SearchCIPDataverse`
- `read_search_results`
- `list_dataset_files`

Do not use other Dataverse MCP functions in this subagent unless this skill is
explicitly revised later.

Read these references when needed:
- For the default CIP-first search flow: `references/discovery_workflow.md`
- For what metadata to extract and how to summarize it:
  `references/metadata_extraction_rules.md`
- For CIP-specific versus generic Dataverse behavior:
  `references/cip_vs_generic_dataverse.md`

## Default workflow

1. Search for candidate datasets.
2. Read and summarize the search results.
3. Inspect the strongest candidates for metadata depth and file availability.
4. Recommend a short list of the most relevant datasets.

## Tool-use rules

Use only these MCP functions:
- `SearchCIPDataverse`
  - for dataset search in CIP Dataverse
- `read_search_results`
  - for reading and summarizing the saved search output
- `list_dataset_files`
  - for checking filenames, content types, and restricted/public status for
    shortlisted datasets when search-level metadata is not enough

Do not use:
- `download_dataset_files_by_doi`
- `list_dataverse_collections`
- `create_dataverse_collection`
- `view_dataverse_collection`
- `list_dataset_metadata_templates`
- `get_dataset_schema_for_collection`
- `create_dataset_in_collection`
- `update_dataset_metadata`
- `edit_dataset_metadata`
- `add_file_to_dataset`
- `update_file_categories`
- `set_embargo_on_dataset_files`
- `unset_embargo_on_dataset_files`
- `publish_dataset`
- `delete_dataset_draft`
- `replace_file_in_dataset`
- `delete_file_from_dataset`

This subagent is intentionally limited to search and recommendation.

## Output expectations

For each recommended dataset, extract and return the following when available:
- title
- short description
- authors
- DOI or persistent ID
- repository or collection context
- subjects or keywords
- file availability or file count
- restricted/public status
- related publications when available

Prefer search-result metadata first. Use `list_dataset_files` only when you
need finer file-level inspection beyond the dataset-level `fileCount` or when
restricted/public status is important for selection.

Useful secondary fields to include when they materially affect reuse:
- author affiliations or identifiers
- related publication URLs or IDs
- producer or distributor
- time period covered
- geospatial coverage
- related datasets or related material
- license

If a field is not available from the MCP results you inspected, say it is not
available. Do not infer or invent missing metadata.

## Selection rules

- Prefer a small number of strong candidates over long unranked lists.
- Explain why each recommended dataset is relevant to the user's goal.
- Note obvious limitations such as missing description, restricted files, or
  weak metadata.
- If multiple datasets are similar, distinguish them by scope, metadata
  completeness, file accessibility, and likely analytical usefulness.

## Out-of-scope Dataverse operations

This skill is search-only. Do not download files, and do not create, edit,
publish, replace, or delete Dataverse objects.

Without an API key, repository curation operations are not available. Keep this
subagent focused on discovery and selection.
