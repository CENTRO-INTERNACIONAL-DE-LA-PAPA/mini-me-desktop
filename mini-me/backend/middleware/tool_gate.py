"""Withhold a subagent's structured answer until the work behind it has been done.

# The mechanism, once, for all of them

A subagent that carries ``response_format`` has its schema bound *as a tool*, because Anthropic
models report ``structured_output: False`` and LangChain resolves that to a `ToolStrategy`. Then
in `langchain/agents/factory.py`:

    # Force tool use if we have structured output tools
    tool_choice = "any" if structured_output_tools else request.tool_choice

Two consequences, and the fix turns on both. The first model call is **compelled** to call
something — and among the options sits one that answers the whole question in a single step, from
memory, and ends the episode. It is the cheapest legal move available, so the model takes it. And
while that structured tool is bound, ``request.tool_choice`` is **discarded**, so middleware that
merely sets it changes nothing.

The fix is to withhold the exit until the required tools have returned:

    request.override(response_format=None, tool_choice=step.force)

Dropping the response format un-binds the structured tool — which is what lets ``tool_choice``
reach the model at all — and naming the tool is what makes the forced call the *right* one rather
than whichever of `ls`, `execute` or `write_todos` the model picks when told only that it must
call something.

# Why this is a base class

`academic_researcher` was the first (`middleware/search_first.py`), and it produced eight
citations from memory every run for four days while three rounds of prompt edits argued with it.
Nine other subagents carry a `response_format` and therefore carry the same exit; their prompts
say `NEVER` and `ALWAYS` in capital letters, which is what somebody writes when they already
suspect the model will decline. This is the second of them. Writing the mechanism a third time by
hand is how the ninth ends up subtly different from the first.

**The rule this repository keeps arriving at: if it must be true, it cannot be asked for.**

# Why this cannot spin

Each step opens on the *presence* of a tool result, never on its content. A call that returned
nothing, timed out, or reported a missing sandbox still leaves a `ToolMessage` behind, so the next
model call is unforced and the agent proceeds to answer with whatever it has. **A failed tool must
cost a finding, never a turn.** A gate that waited for success would loop until the recursion
limit on exactly the runs where the tools are broken.
"""

from __future__ import annotations

import logging
from collections.abc import Awaitable, Callable
from dataclasses import dataclass, field
from typing import Any

from langchain.agents.middleware import AgentMiddleware

logger = logging.getLogger(__name__)


@dataclass(frozen=True)
class Step:
    """One tool that must have returned before the structured answer is reachable."""

    #: The tool the model is pointed at while this step is outstanding.
    force: str
    #: Why, in the log. Read as ``f"{because} — forcing {force}"``, so it is a clause and not a
    #: sentence. This is the line somebody greps for at 1am on a Windows laptop, so it names the
    #: subagent rather than assuming the reader knows which one is speaking.
    because: str
    #: Any of these having returned satisfies the step. Defaults to `force` alone. Wider than one
    #: name where a model reaching for a *different* tool has genuinely done the work — forcing it
    #: back through our preferred one would be overriding a reasonable choice rather than
    #: preventing an unreasonable one.
    satisfied_by: frozenset[str] = field(default=frozenset())

    def __post_init__(self) -> None:
        if not self.satisfied_by:
            object.__setattr__(self, "satisfied_by", frozenset({self.force}))


def _returned(messages: list[Any], names: frozenset[str]) -> bool:
    """Whether any of `names` has already come back in this conversation.

    Reads tool **results**, not the model's requests. A call that was made and never returned has
    put nothing in front of the model, and counting it would reopen the exit at the moment the
    evidence is thinnest.
    """
    for message in messages or []:
        if getattr(message, "type", None) != "tool":
            continue
        if getattr(message, "name", None) in names:
            return True
    return False


class ToolsBeforeAnswering(AgentMiddleware):
    """Force each of `steps`, in order, before the structured response is offered again.

    Subclasses set `steps`. Everything else is here.
    """

    #: In order. The first one not yet satisfied is the one the next call is forced into.
    steps: tuple[Step, ...] = ()

    def _pending(self, messages: list[Any]) -> Step | None:
        """The first step still outstanding, or `None` when the agent may answer."""
        for step in self.steps:
            if not _returned(messages, step.satisfied_by):
                return step
        return None

    def _gate(self, request: Any) -> Any:
        """The request to actually run: forced into the next step, or passed through untouched."""
        step = self._pending((request.state or {}).get("messages", []))
        if step is None:
            return request
        # INFO, and it arrives — a working run shows these lines in the deployment log. They were
        # briefly moved to WARNING on the theory that INFO never reached that file, a conclusion
        # drawn from the line being absent when the reason it was absent is that the gate had never
        # run. The absence of a diagnostic is evidence about the code that emits it, not about the
        # channel, unless the channel has been shown to carry something else.
        logger.info("%s — forcing %s", step.because, step.force)
        return request.override(response_format=None, tool_choice=step.force)

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
