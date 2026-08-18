"""Make the academic researcher search before it is allowed to answer.

# The defect

The subagent produced eight references without calling a single search tool. Not once — every
run, for days, with the citations composed from the model's own memory. The titles were plausible
and the identifiers pointed at other papers, which is exactly what memory produces.

The prompt was not the problem. It says *"Use available tools to find and synthesize relevant
scientific evidence"*, and a further block of identifier rules was appended on top of it. Neither
moved the behaviour, because the behaviour is structural: `academic_researcher` carries
``response_format=AcademicResearchResults``, which LangChain binds as a tool and then forces
``tool_choice="any"`` around, so a one-step answer from memory is the cheapest legal move the
model has. The mechanism, and the fix, are set out once in `middleware/tool_gate.py`.

# What is specific to this one

The tool the first call is forced into is `find_papers` and not one of the Asta MCP searches,
because it is the only one that returns a reference already built from the publisher's record
(`backend/paper_tools.py`). The MCP snippet search returns a title and a corpus id, which is what
left the model composing the other five fields from memory in the first place.

But *any* search satisfies the gate. A model that reached for Asta's own tools has engaged with
the literature, and forcing it back through ours would be overriding a reasonable choice rather
than preventing an unreasonable one.

# Verified

    academic_researcher has not searched yet — forcing find_papers
    find_papers('late blight resistance…') -> 20 paper(s)
    7 of 7 sources relinked to a paper the search returned (43 recorded)
"""

from __future__ import annotations

import json
import logging
from typing import Any

from langchain.agents.middleware import AgentMiddleware

from backend import paper_tools
from backend.middleware.tool_gate import Step, ToolsBeforeAnswering
from backend.schemas import ArtifactState

logger = logging.getLogger(__name__)

#: The tool the first call is forced into.
SEARCH_TOOL = "find_papers"

#: Any of these having run counts as "the agent has searched".
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


class SearchBeforeCiting(ToolsBeforeAnswering):
    """Force a literature search before the structured answer becomes reachable."""

    steps = (
        Step(
            force=SEARCH_TOOL,
            because="academic_researcher has not searched yet",
            satisfied_by=SEARCH_TOOLS,
        ),
    )


def _has_searched(messages: list) -> bool:
    """Whether any search tool has already returned in this conversation.

    Kept as a module function because it is the thing worth asserting on directly, and because
    `paper_tools` and the tests both read it as the definition of "this run searched".
    """
    from backend.middleware.tool_gate import _returned

    return _returned(messages, SEARCH_TOOLS)


class KeepSources(AgentMiddleware):
    """Write the papers a search returned into the workspace, as a file the researcher owns.

    *"I noticed that we are not saving the json file with the papers inside the thread folder. I
    want the user to have it."* They were right: `find_papers` results lived in the Sources panel
    and in `minime_local.sources._seen`, an in-process dict — nothing on disk, nothing that
    survives the app closing, nothing to hand to a colleague.

    `papers.json` carries `paper_tools.complete_sources`, so it holds **everything the searches
    returned** and not just the shortlist the model discussed. `FileSyncMiddleware` surfaces it in
    Outputs like any other produced file.

    Records only. A file that could not be written is logged and costs nothing else — the answer
    the subagent produced is not worth losing over a copy of it.
    """

    state_schema = ArtifactState

    #: Beside `dataverse_search.json`, which `middleware/dataverse_first.py` keeps the same way.
    FILENAME = "papers.json"

    def __init__(self, sandbox_backend: Any):
        super().__init__()
        self.sandbox_backend = sandbox_backend

    async def aafter_agent(self, state: Any, runtime: Any) -> dict[str, Any] | None:
        structured = state.get("structured_response")
        if structured is None:
            return None
        try:
            sources = paper_tools.complete_sources(structured, state.get("messages", []))
            if not sources:
                return None
            work_dir = str(await self.sandbox_backend.aget_work_dir()).rstrip("/")
            written = await self.sandbox_backend.awrite(
                f"{work_dir}/{self.FILENAME}",
                json.dumps(sources, indent=2, ensure_ascii=False),
            )
            if getattr(written, "error", None):
                logger.warning("could not keep %s: %s", self.FILENAME, written.error)
            else:
                logger.info("kept %s in the workspace (%d paper(s))", self.FILENAME, len(sources))
        except Exception:
            logger.exception("could not keep %s", self.FILENAME)
        return None
