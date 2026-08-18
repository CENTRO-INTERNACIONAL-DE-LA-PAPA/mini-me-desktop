"""Keep a copy of the conversation index before the server is allowed to delete it.

**The remedy upstream chose for an unreadable file is `os.remove`.**
`langgraph_runtime_inmem/database.py:start_pool` loads `.langgraph_api/.langgraph_ops.pckl` — the
index of every thread, run and assistant this installation has — and on *any* exception deletes it,
with no existence check and no backup:

```python
except Exception as e:
    logger.error("Failed to load cached data: %s", str(e))
    await asyncio.to_thread(os.remove, OPS_FILENAME)
```

The `ModuleNotFoundError` branch above it names the trigger in its own message: *"Pulled updates
that modified class definitions in a way that's incompatible with the cache."* On this product that
is not an edge case — it is the **update path**. The desktop app mirrors the backend source into the
checkout on every launch, so `git pull` on the app *is* the backend update (docs §135/§139), and a
pickle written by last week's classes is exactly what the next launch reads.

§95 already removed the worse twin of this: conversations themselves live in SQLite now, so a failed
load cannot take thirty threads with it. That fix replaced the *checkpointer*. This is the **ops
index**, a different store, still `PersistentDict`, and still flushed over its own file every ten
seconds by `_persistence.py:57` — so even preventing the delete would not be enough on its own.

**What this does, and what it deliberately does not.** It copies the file aside before `start_pool`
runs and removes the copy when the load succeeded. It does *not* stop the server deleting or
rewriting anything: refusing would leave a server that cannot start, and a researcher with an
unreadable index needs a working app more than they need that file in place. What they must not have
is the file silently gone — so the copy survives, and the log says where.

The plan's own statement of the principle is `a persistence layer that cannot read its file must
refuse to write it`. Upstream is not ours to change; keeping the evidence is the part that is.
"""

from __future__ import annotations

import functools
import logging
import os
import shutil
import time

logger = logging.getLogger(__name__)

#: How many rescued copies to keep before the oldest is dropped.
#:
#: More than one because the second failure is the informative one — it says the first rescue did not
#: help — and few enough that a broken install cannot fill a disk with pickles.
KEEP = 5

#: Suffix that marks our copies, so the sweep below can never match anything else.
MARK = ".minime-rescued-"


def install(module) -> None:
    """Wrap `start_pool` so the index is copied aside before it can be removed."""
    original = getattr(module, "start_pool", None)
    filename = getattr(module, "OPS_FILENAME", None)
    if original is None or not filename:
        logger.warning(
            "minime_local: no start_pool/OPS_FILENAME to guard — an unreadable conversation "
            "index would be deleted with no copy kept (docs §218)"
        )
        return

    @functools.wraps(original)
    async def guarded(*args, **kwargs):
        rescued = _copy_aside(filename)
        try:
            return await original(*args, **kwargs)
        finally:
            # **The file's absence is the signal.** A load that worked leaves it alone — the flush
            # loop rewrites it later, but not before this returns — and the only thing that removes
            # it here is the recovery path. So there is no need to inspect the exception, which
            # `start_pool` has already swallowed by this point anyway.
            if rescued:
                if os.path.exists(filename):
                    _forget(rescued)
                else:
                    logger.warning(
                        "minime_local: the conversation index could not be read and the server "
                        "deleted it. A copy is at %s — the app's conversation list will look "
                        "empty until it is restored (docs §218)",
                        os.path.abspath(rescued),
                    )

    module.start_pool = guarded
    logger.warning("minime_local: the conversation index is copied aside before every load")


def _copy_aside(filename: str) -> str | None:
    """Copy the index next to itself, stamped, or `None` if there was nothing to copy."""
    try:
        if not os.path.exists(filename) or os.path.getsize(filename) == 0:
            return None
        # Stamped rather than one fixed name: the second failure must not overwrite the copy taken
        # before the first, which is the one holding the last good index.
        #
        # And made unique, because the stamp alone does not do it. A test that failed twice inside
        # one second produced one file, silently — the claim above was false for exactly the case it
        # was written for. Launches are minutes apart in practice, which is why it would never have
        # been noticed and why it is worth closing anyway (§218).
        stamp = f"{filename}{MARK}{time.strftime('%Y%m%d-%H%M%S')}"
        rescued, attempt = stamp, 1
        while os.path.exists(rescued):
            rescued = f"{stamp}-{attempt}"
            attempt += 1
        shutil.copy2(filename, rescued)
        _sweep(filename)
        return rescued
    except OSError as error:
        # Never the reason a backend fails to start. A missing copy is a worse day later; a crash
        # here is no app at all (§18).
        logger.warning("minime_local: could not copy the conversation index aside (%s)", error)
        return None


def _forget(rescued: str) -> None:
    try:
        os.remove(rescued)
    except OSError:
        pass


def _sweep(filename: str) -> None:
    """Keep the [`KEEP`] most recent copies and drop the rest.

    **By modification time, not by name.** Sorting the stamped names looked equivalent and is not:
    the collision suffix makes `…-101533` sort before `…-101533-1`, and once the numbers reach two
    digits it stops matching age at all. Measured while checking this file: the cap held at five, and
    they were the five *oldest*. mtime is the thing actually meant.
    """
    directory = os.path.dirname(filename) or "."
    prefix = os.path.basename(filename) + MARK
    try:
        ours = [
            os.path.join(directory, name)
            for name in os.listdir(directory)
            if name.startswith(prefix)
        ]
        ours.sort(key=os.path.getmtime)
    except OSError:
        return
    for path in ours[: max(0, len(ours) - KEEP)]:
        _forget(path)
