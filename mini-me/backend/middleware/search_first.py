"""Make the academic researcher search before it is allowed to answer.

# The defect

The subagent produced eight references without calling a single search tool. Not once — every
run, for days, with the citations composed from the model's own memory. The titles were plausible
and the identifiers pointed at other papers, which is exactly what memory produces.

The prompt was not the problem. It says *"Use available tools to find and synthesize relevant
scientific evidence"*, and a further block of identifier rules was appended on top of it. Neither
moved the behaviour, because the behaviour is structural.

`academic_researcher` carries ``response_format=AcademicResearchResults``. Anthropic models report
``structured_output: False`` in their profile, so LangChain resolves that to a `ToolStrategy` —
the schema is bound as *a tool*. And in `langchain/agents/factory.py`:

    # Force tool use if we have structured output tools
    tool_choice = "any" if structured_output_tools else request.tool_choice

So on the **first** model call the agent is compelled to call some tool, and among its choices sits
one that answers the entire question in a single step, from memory, and ends the episode. It is the
cheapest legal move available and the model takes it. A prompt asking for diligence is competing
against the grain of the loop.

Note the second consequence, because it decides the shape of the fix: while a structured output
tool is bound, ``request.tool_choice`` is **discarded**. Middleware that merely sets it is
middleware that changes nothing — the failure this repository keeps rediscovering, where a value is
written somewhere nothing reads.

# The fix

Withhold the exit until the work is done. On any model call where no search has happened yet, drop
the response format — which un-binds the structured output tool and hands ``tool_choice`` back to
us — and point it at the search:

    request.override(response_format=None, tool_choice=SEARCH_TOOL)

The first move is then a literature search, and it cannot be anything else. Once a result exists
the request passes through untouched, the schema is bound again, and the model finishes as before —
now writing its references from records that are in front of it rather than from recollection.

# Why this cannot spin

The gate opens on the *presence* of a search result, not on its success. A search that returns
nothing, times out, or reports a missing sandbox still leaves a `ToolMessage` behind, so the next
call is unforced and the agent proceeds to answer. A failed search must cost a citation, never a
turn.
"""

from __future__ import annotations

import logging
from collections.abc import Awaitable, Callable
from typing import Any

from langchain.agents.middleware import AgentMiddleware

logger = logging.getLogger(__name__)

#: The tool the first call is forced into. `find_papers` and not one of the Asta MCP tools,
#: because it is the only one that returns a reference already built from the publisher's record
#: (`backend/paper_tools.py`); the MCP snippet search returns a title and a corpus id, which is
#: what left the model composing the other five fields from memory in the first place.
SEARCH_TOOL = "find_papers"

#: Any of these having run counts as "the agent has searched". Broader than `SEARCH_TOOL` on
#: purpose: a model that reached for the MCP search has *engaged with the literature*, and forcing
#: it back through a second tool would be overriding a reasonable choice rather than preventing an
#: unreasonable one.
SEARCH_TOOLS = frozenset(
    {
        SEARCH_TOOL,
        "snippet_search",
        "search_papers_by_relevance",
        "search_paper_by_title",
        "get_papers",
        "get_paper_batch",
    }
)


def _has_searched(messages: list[Any]) -> bool:
    """Whether any search tool has already returned in this conversation.

    Reads tool *results* rather than the model's requests: a call that was made and never came
    back has not put a single record in front of the model, and treating it as a search would
    reopen the exit at the moment the evidence is thinnest.
    """
    for message in messages or []:
        if getattr(message, "type", None) != "tool":
            continue
        if getattr(message, "name", None) in SEARCH_TOOLS:
            return True
    return False


class SearchBeforeCiting(AgentMiddleware):
    """Force a literature search before the structured answer becomes reachable."""

    def _gate(self, request: Any) -> Any:
        """The request to actually run: forced into a search, or passed through untouched."""
        messages = (request.state or {}).get("messages", [])
        if _has_searched(messages):
            return request
        # Both, and neither alone is enough. Dropping the response format un-binds the structured
        # output tool — which is what makes `tool_choice` reach the model at all — and naming the
        # tool is what makes the forced call a search rather than whichever of `ls`, `execute` or
        # `write_todos` the model happens to pick when told only that it must call *something*.
        logger.info("academic_researcher has not searched yet — forcing %s", SEARCH_TOOL)
        return request.override(response_format=None, tool_choice=SEARCH_TOOL)

    def wrap_model_call(
        self,
        request: Any,
        handler: Callable[[Any], Any],
    ) -> Any:
        return handler(self._gate(request))

    async def awrap_model_call(
        self,
        request: Any,
        handler: Callable[[Any], Awaitable[Any]],
    ) -> Any:
        # Defined explicitly. The server runs the graph on the async path, and a middleware with
        # only a sync hook is one that does nothing where it matters.
        return await handler(self._gate(request))
