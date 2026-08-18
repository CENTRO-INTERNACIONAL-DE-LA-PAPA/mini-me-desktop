"""Make the dataverse explorer search, and read what it found, before it recommends anything.

# Why this one is next

`dataverse_explorer` carries ``response_format=DataVerseSearchResults``, so it has the exit set
out in `middleware/tool_gate.py`: its first model call is forced, and one of the things it may
call is the schema itself, answering the whole question from memory in a single step.

What comes back when it does is a list of `DataVerseFindings`, and the required field on each is
``persistent_id`` — *"Dataset DOI or persistent identifier"*. **A persistent id composed from
memory is a citation a researcher will paste into a paper without checking**, exactly as they
clicked the DOIs that started this. It is the same failure as `academic_researcher`'s with a
shorter fuse, and unlike a plausible-looking reference, a wrong `persistent_id` is not something a
reader can catch by recognising the title.

# Two steps, because one would not be enough

`SearchCIPDataverse` writes its results to a **file**; `read_search_results` is what puts the
metadata in front of the model. So a gate that opened as soon as a search returned would let the
subagent search, satisfy the gate, and then still compose every field from memory — having proven
only that it can call a tool.

The workflow the skill documents is search → read → recommend
(`skills/dataverse/references/discovery_workflow.md`), and both of the first two are forced here.
`list_dataset_files` is step three in that document and is *not* forced: it is for shortlisted
datasets only, it is a judgement about how much detail a recommendation needs, and nothing in the
schema depends on it.

# The handoff is made, not requested

The prompt carried this:

    Mandatory fixed filename rule: ALWAYS call `SearchCIPDataverse` with
    `output_filename="dataverse_search.json"` and ALWAYS call `read_search_results` with
    `filename="dataverse_search.json"`. Do not invent or vary this name.

That is a mechanical fact about two tools that have to agree, written in capital letters and handed
to a model to remember across a multi-step episode.

**Both the prompt and the middleware that replaced it named an argument that does not exist.**
`read_search_results` takes `file_path`, and it wants the *server-side absolute path* — not a bare
name. Probed against the live MCP (docs §220):

    read_search_results(filename=...)                    -> 'file_path' is a required property
    read_search_results(file_path=..., filename=...)     -> Unexpected keyword argument
    read_search_results(file_path="/tmp/mcp/json_files/dataverse_search.json")  -> the metadata

So injecting `filename` did not merely fail to help: it made every read fail, including the reads
the gate above forces. `dataverse_explorer` could search and could never read, which is what the
researcher saw — nine steps, ninety seconds, and *"couldn't extract parseable metadata."*

The path is taken from the search's own answer (`{"output_file": "/tmp/mcp/json_files/..."}`)
rather than assumed, so a server that moves its directory is followed rather than guessed at.

# The results are kept where the researcher can open them

That file lives on the MCP host — `/tmp/mcp/json_files/` on a machine at
`dataverse-cip.fastmcp.app`, which is nobody's workspace. *"I want the user to have it."* So what
comes back from the read is written into the sandbox as `dataverse_search.json`, where
`FileSyncMiddleware` surfaces it in Outputs and `middleware/claims.py` can check the recommended
`persistent_id`s against it. Until this, that check was reading a path that never existed.
"""

from __future__ import annotations

import json
import logging
from collections.abc import Awaitable, Callable
from typing import Any

from langchain.agents.middleware import AgentMiddleware

from backend.middleware.tool_gate import Step, ToolsBeforeAnswering

logger = logging.getLogger(__name__)

#: The tool that queries CIP Dataverse and writes its results to disk.
SEARCH_TOOL = "SearchCIPDataverse"

#: The tool that reads those results back. Until this has returned, the model has a file it has
#: never seen the contents of.
READ_TOOL = "read_search_results"

#: The one name both tools must agree on. Successive searches overwrite it, which is intended:
#: the file is a hand-off between two calls, not an archive.
FIXED_FILENAME = "dataverse_search.json"

#: Where the MCP writes when it is not told otherwise, used only if a read is somehow reached
#: without a search having answered first. The gate above makes that ordering hard to produce, and
#: a stale constant is still better than no argument at all.
DEFAULT_SERVER_DIR = "/tmp/mcp/json_files"


class SearchBeforeRecommending(ToolsBeforeAnswering):
    """Force a Dataverse search, then a read of it, before recommendations become reachable."""

    steps = (
        Step(
            force=SEARCH_TOOL,
            because="dataverse_explorer has not searched CIP Dataverse yet",
        ),
        Step(
            force=READ_TOOL,
            because="dataverse_explorer has not read what its search returned",
        ),
    )


class SearchResultsFile(AgentMiddleware):
    """Make the two Dataverse tools agree on one file, and keep a copy the researcher can open.

    Three things, all mechanical, none of them a judgement a model should be making mid-episode:

    * the search is told where to write (`output_filename`);
    * the read is told where to look (`file_path`), taken from what the search answered;
    * what the read returns is saved into the workspace, because the file itself is on the MCP
      host and the researcher has no way to reach it there.

    The copy is written on the async path only. The server runs the graph there, and the sandbox
    write is a coroutine; the sync path still fixes the arguments, so a synchronous run is
    correct, merely without the copy.
    """

    def __init__(self, sandbox_backend: Any | None = None):
        super().__init__()
        self.sandbox_backend = sandbox_backend
        #: Where the last search said it wrote. Instance state is per-request: the middleware is
        #: constructed in `_build_runtime_subagents`, which runs once per turn.
        self._server_path: str | None = None

    # -- reading what a tool answered ------------------------------------------------------

    @staticmethod
    def _texts(result: Any) -> list[str]:
        """Every string a tool answer carries, whatever wrapper it arrived in.

        **The handler does not return what the tool returned.** Its contract is
        ``ToolMessage | Command`` (`langchain/agents/middleware/types.py:652`), so calling the MCP
        tool directly — which is how the first version of this was checked — exercises a shape the
        middleware never sees in production. `ToolMessage.content` is then itself either a string
        or a list of content blocks, depending on the tool.
        """
        found: list[str] = []

        def collect(node: Any) -> None:
            if isinstance(node, str):
                found.append(node)
            elif isinstance(node, dict):
                text = node.get("text")
                if isinstance(text, str):
                    found.append(text)
            elif isinstance(node, list):
                for item in node:
                    collect(item)

        # A `Command` carries its messages in `update`; a `ToolMessage` carries `content`.
        update = getattr(result, "update", None)
        if isinstance(update, dict):
            for message in update.get("messages") or []:
                collect(getattr(message, "content", message))
        collect(getattr(result, "content", result))
        return found

    @classmethod
    def _payload(cls, result: Any) -> dict[str, Any] | None:
        """The JSON object an MCP tool answered with."""
        for text in cls._texts(result):
            try:
                parsed = json.loads(text)
            except (ValueError, TypeError):
                continue
            if isinstance(parsed, dict):
                return parsed
        return None

    # -- setting the arguments -------------------------------------------------------------

    def _fix(self, request: Any) -> Any:
        """The call to actually run, with its path argument set rather than remembered."""
        call = getattr(request, "tool_call", None) or {}
        name = call.get("name") or ""
        args = call.get("args") or {}

        if name == SEARCH_TOOL:
            argument, wanted = "output_filename", FIXED_FILENAME
        elif name == READ_TOOL:
            argument = "file_path"
            wanted = self._server_path or f"{DEFAULT_SERVER_DIR}/{FIXED_FILENAME}"
        else:
            return request

        # `filename` is not an argument of either tool, and passing it is a hard error rather than
        # a harmless extra — it is what the previous version of this file injected.
        cleaned = {key: value for key, value in args.items() if key != "filename"}
        if cleaned.get(argument) == wanted and cleaned == args:
            return request
        # Logged when it corrects something, so the line reads "the model got this wrong again"
        # rather than "the middleware is installed".
        logger.info("%s(%s=%r) -> %r", name, argument, args.get(argument), wanted)
        return request.override(tool_call={**call, "args": {**cleaned, argument: wanted}})

    # -- keeping what came back ------------------------------------------------------------

    def _remember_search(self, result: Any) -> None:
        payload = self._payload(result)
        path = (payload or {}).get("output_file")
        if isinstance(path, str) and path:
            self._server_path = path

    async def _keep(self, result: Any) -> None:
        """Write the metadata into the sandbox, where Outputs and the claims check can see it."""
        if self.sandbox_backend is None:
            return
        payload = self._payload(result)
        if not payload or "content" not in payload:
            return
        try:
            work_dir = await self.sandbox_backend.aget_work_dir()
            written = await self.sandbox_backend.awrite(
                f"{str(work_dir).rstrip('/')}/{FIXED_FILENAME}",
                json.dumps(payload["content"], indent=2, ensure_ascii=False),
            )
            if getattr(written, "error", None):
                logger.warning("could not keep %s: %s", FIXED_FILENAME, written.error)
            else:
                logger.info(
                    "kept %s in the workspace (%d item(s))",
                    FIXED_FILENAME,
                    len(payload["content"]) if isinstance(payload["content"], list) else 1,
                )
        except Exception:
            # A copy is a convenience. Losing it must not cost the search that produced it.
            logger.exception("could not keep %s", FIXED_FILENAME)

    # -- hooks -------------------------------------------------------------------------------

    def wrap_tool_call(
        self,
        request: Any,
        handler: Callable[[Any], Any],
    ) -> Any:
        result = handler(self._fix(request))
        if (getattr(request, "tool_call", None) or {}).get("name") == SEARCH_TOOL:
            self._remember_search(result)
        return result

    async def awrap_tool_call(
        self,
        request: Any,
        handler: Callable[[Any], Awaitable[Any]],
    ) -> Any:
        # `AgentMiddleware.wrap_tool_call` raises `NotImplementedError` with a message about this
        # exact omission, which is a good sign it is a common one.
        name = (getattr(request, "tool_call", None) or {}).get("name")
        result = await handler(self._fix(request))
        if name == SEARCH_TOOL:
            self._remember_search(result)
        elif name == READ_TOOL:
            await self._keep(result)
        return result
