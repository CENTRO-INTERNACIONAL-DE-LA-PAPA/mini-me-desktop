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

# The filename is set, not requested

The prompt carried this:

    Mandatory fixed filename rule: ALWAYS call `SearchCIPDataverse` with
    `output_filename="dataverse_search.json"` and ALWAYS call `read_search_results` with
    `filename="dataverse_search.json"`. Do not invent or vary this name.

That is a mechanical fact about two tools that have to agree on one string, written in capital
letters and handed to a model to remember across a multi-step episode. Nothing about it needs
judgement, and the failure mode when it is forgotten is `read_search_results` returning
"File ... not found" — a dead end the model then narrates its way around.

`FixedSearchFilename` sets the argument. The paragraph comes out of the prompt, because a rule
that is enforced and *also* asked for teaches the next reader that the prompt is where such things
live.
"""

from __future__ import annotations

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

#: Which argument carries the filename, per tool. They are spelled differently — `output_filename`
#: on the way out, `filename` on the way back — which is precisely the kind of detail a model
#: recalls wrong three steps into an episode.
FILENAME_ARG = {
    SEARCH_TOOL: "output_filename",
    READ_TOOL: "filename",
}


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


class FixedSearchFilename(AgentMiddleware):
    """Put `dataverse_search.json` in the call, rather than asking the model to remember it."""

    def _fix(self, request: Any) -> Any:
        """The call to actually run, with the filename argument set to the fixed name."""
        call = getattr(request, "tool_call", None) or {}
        argument = FILENAME_ARG.get(call.get("name") or "")
        if argument is None:
            return request
        args = call.get("args") or {}
        if args.get(argument) == FIXED_FILENAME:
            return request
        # Logged when it actually corrects something, so the line means "the model got this wrong
        # again" rather than "the middleware is installed". A diagnostic that prints on every call
        # tells the reader nothing on the call that mattered.
        logger.info(
            "%s(%s=%r) -> %r",
            call.get("name"),
            argument,
            args.get(argument),
            FIXED_FILENAME,
        )
        return request.override(
            tool_call={**call, "args": {**args, argument: FIXED_FILENAME}}
        )

    def wrap_tool_call(
        self,
        request: Any,
        handler: Callable[[Any], Any],
    ) -> Any:
        return handler(self._fix(request))

    async def awrap_tool_call(
        self,
        request: Any,
        handler: Callable[[Any], Awaitable[Any]],
    ) -> Any:
        # The server runs the graph on the async path. `AgentMiddleware.wrap_tool_call` raises
        # `NotImplementedError` with a message about this exact omission, which is a good sign it
        # is a common one.
        return await handler(self._fix(request))
