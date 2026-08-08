# CIP Vs Generic Dataverse

This skill is currently scoped to CIP Dataverse.

## CIP-specific behavior

The strongest current search flow is CIP-specific:
- `SearchCIPDataverse`
- `read_search_results`
- `list_dataset_files`

These are currently the best tools for:
- searching CIP datasets
- saving search results
- inspecting shortlisted dataset files without downloading them

Use them first when the user is clearly working in the CIP ecosystem.

## Generic Dataverse status

Several other MCP tools are generic because they accept `base_url` and optional
`api_token`, but this skill does not currently use them as part of its default
behavior.

If the MCP is extended later and this skill is revised, those generic tools may
support broader Dataverse discovery. For now, keep this skill focused on CIP.

## Important limitation

Do not pretend this skill supports generic multi-Dataverse discovery today.
That is future scope, not current behavior.

## Repository context

When returning dataset recommendations, include the repository or collection
context when possible so the user understands whether a dataset came from:
- CIP Dataverse
- a known collection within CIP
