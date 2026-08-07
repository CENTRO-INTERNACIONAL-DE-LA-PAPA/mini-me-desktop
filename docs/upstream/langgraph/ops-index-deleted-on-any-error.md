# The thread index is deleted on any exception while loading it

**Package:** `langgraph-runtime-inmem` (used by `langgraph dev`)
**Severity:** critical — deletes the record of every thread in the installation
**Found:** 2026-08-06

## Summary

`langgraph_runtime_inmem/database.py:167-184` catches **every** exception raised while loading the
ops pickle and responds by deleting it, with no existence check, no backup, and no way to decline.

```python
except Exception:
    ...
    os.remove(OPS_FILENAME)
    os.remove(RETRY_COUNTER_FILENAME)
```

## Why this fires more often than it looks

Unpickling is not a pure read: it **imports** the modules the pickle references. The ops pickle
embeds references into `langgraph_api.auth.custom`, `langgraph_api.metadata` and
`langchain_core.messages`, and importing `langgraph_api.config` reads configuration at import
time.

Reproduced directly: with `REDIS_URI` unset, `pickle.load` raises

```
KeyError: "Config 'REDIS_URI' is missing, and has no default."
```

which lands in the bare `except Exception` above and takes the thread index with it. A missing
environment variable is not a corrupt file, and it should not be treated as one.

`langgraph_api/cli.py:188-190,246-254` patches defaults (`__redis_uri__ = "fake"`,
`__database_uri__ = ":memory:"`) into the environment before that import, so the plain
`langgraph dev` path is protected today. Anything that loads the store outside that CLI is not.

## Impact

Every thread disappears from the server's index. The checkpoints may still be on disk, but nothing
enumerates them, so from the user's side the history is gone.

## Suggested fix

Distinguish "this file is corrupt" from "this process could not import something", and in either
case **move the file aside rather than delete it**:

```python
except Exception:
    backup = f"{OPS_FILENAME}.unreadable-{int(time.time())}"
    os.replace(OPS_FILENAME, backup)
    logger.error("could not load %s; kept a copy at %s", OPS_FILENAME, backup)
```

A rename costs nothing, keeps recovery possible, and turns an unrecoverable event into an
inconvenient one.
