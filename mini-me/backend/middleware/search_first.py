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

from backend.middleware.tool_gate import Step, ToolsBeforeAnswering

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
