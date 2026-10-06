# Bundled CIP Dataverse MCP

Copied from `AskPapa/mcp_server_stdio/dataverse_server.py` and its four JSON templates.
Source checkout revision: `06881390f7cf0634d2b3e38baa7318fbb0ec8149`.
The upstream files are licensed under Apache-2.0; the full license is included in `LICENSE`.
No AskPapa repository or virtual environment is required at runtime.

Desktop adaptations:

- Use `fastmcp.prompts.Message`, the public FastMCP 4 import.
- Use the OS temporary directory instead of hardcoded `/tmp` paths.
- Execute with the backend interpreter; dependencies and package data are declared in
  `mini-me/pyproject.toml` and dependencies are pinned in `mini-me/uv.lock`.

The full upstream tool catalogue is retained here, but Mini-Me only exposes
`SearchCIPDataverse`, `read_search_results`, and `list_dataset_files` to its specialist.
The stdio server does not listen on a network port. Public search and inspection require
no Dataverse API key; calls still need network access to CIP Dataverse.

Use the desktop loader rather than selecting the curation tools directly.
