"""A logger whose lines actually reach the backend log.

# The channel problem

Nothing in this backend configures logging, so the level is whatever the LangGraph server happens to
set — and docs §132 established what that turned out to be: **every line ever seen to reach the
backend log arrived at WARNING.** An `INFO` that never lands cost a diagnosis once, because the
absence of a line was read as *"the tool did not run"* when it may only have meant *"INFO does not
reach this file"*.

`middleware/claims.py` solved that for itself by attaching one stderr handler, which
`crates/app/src/backend.rs` hands straight to the log file. This is that solution, extracted, because
a second module needed it the moment somebody asked a reasonable question of the log and the log had
nothing to say.

# Where it came from

*"I launch it now so I must assume I should wait"* — followed by a grep that found one line. A
DataVoyager submission logged nothing on success: `datavoyager_tools` had four `logger.warning` calls
and every one of them was a failure path. So a run that submitted correctly and a run that never
submitted produced identical output, and the only way to tell them apart was to watch the panel
(docs §235).

That is the same defect §219 was written about, in a different module: a record that shows failures
and swallows successes cannot tell *"working"* from *"never ran"*.
"""

from __future__ import annotations

import logging
import sys

#: Marks a logger this module has already fitted out, so repeated calls are free and do not stack
#: handlers — which would print every line as many times as the module was imported.
_FITTED = "_minime_arriving_handler"


def arriving(name: str) -> logging.Logger:
    """A logger for `name` whose INFO lines reach the backend log.

    One handler on stderr, which the app tees to the file. `propagate` is off so a server that
    *does* configure INFO prints each line once rather than twice.
    """
    logger = logging.getLogger(name)
    if getattr(logger, _FITTED, False):
        return logger
    handler = logging.StreamHandler(sys.stderr)
    handler.setFormatter(logging.Formatter("%(levelname)s %(message)s"))
    logger.addHandler(handler)
    logger.setLevel(logging.INFO)
    logger.propagate = False
    setattr(logger, _FITTED, True)
    return logger
