# File Access And Transfer

Use this reference only if the Dataverse skill is explicitly revised later to
support download or acquisition workflows.

The current Dataverse skill is search-only and should not call download tools.

## Default rule

Do not download dataset files by default.

Dataset download should be opt-in:
- the user explicitly asks for the files
- or the workflow clearly requires the selected dataset to move into later
  analysis

## Inspect before download

Before downloading, inspect file availability when possible with:
- `list_dataset_files`

Check:
- filenames
- content types
- restricted/public status
- likely tabular usability

## Important runtime boundary

The Dataverse MCP download tool writes files to MCP host-managed directories.
Those files do not automatically appear inside the sandbox.

That means:
- discovery can happen through the MCP
- but later sandbox analysis needs an additional handoff step

## Handoff implications

If a selected dataset must be analyzed in the sandbox, a later application step
is needed to move files across the boundary, for example:
- host download through the MCP
- then explicit upload into the sandbox

Do not assume that calling the MCP download tool alone makes files available to
data-cleaning or EDA scripts.

## User communication

When download is requested, state clearly:
- whether the dataset has accessible files
- whether files are restricted
- that downloaded files land on the host side first
- whether an additional upload step will be needed for sandbox analysis
