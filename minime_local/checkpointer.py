"""Keep conversations in SQLite instead of a pickle of the whole world.

Requested directly: *"Maybe its worth the effort to construct our custom store and custom
checkpointer … so we can accelerate the conversation loading and also avoid conversations
lost."* Both halves are right, and both are properties of the same file format.

**What it replaces.** `langgraph dev` runs `langgraph_runtime_inmem`, whose checkpointer is a
`PersistentDict` — every conversation in the installation, pickled into
`.langgraph_api/.langgraph_checkpoint.N.pckl`, loaded whole at boot and rewritten whole every ten
seconds. Two consequences the researcher met in the same week:

* **Boot cost grows with history** (docs §80). Nothing the client does can help; the server is
  unpickling megabytes before it answers anything.
* **A failed load destroys everything** (docs §90/§94). `checkpoint.py:71-75` registers the dict
  with the flush loop *before* calling `load()`. When the load raises — the
  `ModuleNotFoundError` branch names the trigger itself, *"Pulled updates that modified class
  definitions in a way that's incompatible with the cache"* — the exception is swallowed and an
  **empty** dict is left registered. Ten seconds later `PersistentDict.sync()` pickles that empty
  dict over the real file, under a comment reading ``# atomic commit``.

SQLite has nothing to offer either failure. Rows are read when asked for, so boot is constant;
writes are transactional and per-checkpoint, so a version change that breaks one row cannot take
the other thirty conversations with it.

**Why not Rust**, since the request framed it that way: the cost here is unpickling megabytes and
writing them back, which is serialisation and I/O rather than computation — there is no work for a
faster language to do. Reaching Rust from Python would mean PyO3 and a compiled wheel per
platform, which is a new way for the install to fail on machines that spent docs §57–§60 fighting
WSL2 alone. Rust earns its keep in this project where it already is: the client.

**Why this is a file the server loads rather than a patch.** Unlike everything else in this
overlay, no import hook is involved: `langgraph.json` takes a ``checkpointer`` key naming an async
context manager, and the desktop app already generates its own copy of that config
(`make_config.py`, docs §30) rather than editing a checkout it does not own. So this is
configuration, and the checkout stays untouched.
"""

from __future__ import annotations

import contextlib
import logging
import os
from typing import AsyncIterator

logger = logging.getLogger(__name__)

#: Beside the pickles it supersedes, inside the distro. **Never on a Windows-visible path**:
#: SQLite's file locking over WSL's 9p mount is not reliable, and a corrupted database would be
#: a worse outcome than the slow boot this replaces.
DIRECTORY = ".langgraph_api"
FILENAME = "checkpoints.sqlite"


def path(root: str | None = None) -> str:
    """Where the database lives, relative to the server's working directory."""
    return os.path.join(root or os.getcwd(), DIRECTORY, FILENAME)


def available() -> bool:
    """Whether the SQLite saver can be imported at all.

    Checked by `make_config.py` **before** the config names this module, so a backend without
    the package keeps the built-in checkpointer and behaves exactly as it did. The alternative —
    naming it unconditionally and failing at startup — would turn a missing optional dependency
    into a backend that does not boot.
    """
    try:
        import langgraph.checkpoint.sqlite.aio  # noqa: F401
    except Exception:  # noqa: BLE001 — any import failure means "not available"
        return False
    return True


@contextlib.asynccontextmanager
async def checkpointer() -> AsyncIterator[object]:
    """Yield the saver the server should use for the life of the process.

    The shape `langgraph.json`'s ``checkpointer`` key expects: an async context manager, so the
    server can open the connection at startup and close it on shutdown.
    """
    from langgraph.checkpoint.sqlite.aio import AsyncSqliteSaver

    database = path()
    os.makedirs(os.path.dirname(database), exist_ok=True)
    async with AsyncSqliteSaver.from_conn_string(database) as saver:
        # `setup` creates the tables and applies migrations. Idempotent, and calling it here
        # rather than lazily means a schema problem surfaces at startup — where the log is being
        # read — instead of in the middle of someone's first question.
        setup = getattr(saver, "setup", None)
        if setup is not None:
            await setup()
        # Logged on **success**, and this is not decoration. Three attempts at the subagent
        # registry were misdiagnosed because its installer spoke only on failure, so "absent",
        # "never reached" and "ran and failed" produced identical evidence — nothing (docs §81).
        # A checkpointer that silently did not take effect would look exactly like one that did,
        # right up until someone noticed their conversations were still slow to load.
        logger.warning("minime_local: conversations are stored in %s", database)
        yield saver
