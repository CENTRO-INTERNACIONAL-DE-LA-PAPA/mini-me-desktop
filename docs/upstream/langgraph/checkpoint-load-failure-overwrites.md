# A failed checkpoint load silently overwrites the checkpoint file

**Package:** `langgraph-runtime-inmem` (used by `langgraph dev`)
**Severity:** critical — silent, unrecoverable loss of all local conversation history
**Found:** 2026-08-06, building a desktop client against `langgraph dev`

## Summary

When `PersistentDict.load()` raises, the exception is caught and an **empty** dict is left in
place. That dict has already been registered with the background flush loop, which calls `sync()`
every ten seconds and atomically replaces the on-disk pickle with the empty contents.

Ten seconds after a failed load, every checkpoint in the installation is gone. The only trace is
one `logger.error` line.

## The chain

**1. The dict is registered before it is loaded** — `langgraph_runtime_inmem/checkpoint.py:70-75`:

```python
d = PersistentDict(*args, filename=thisfname)
if __persistence_hook__:
    __persistence_hook__(d)          # ← registers with the flush loop

try:
    d.load()                         # ← may raise
```

`checkpoint.py:239` and `:246` pass `__persistence_hook__=register_persistent_dict`, so this is
the normal path, not an edge case.

**2. The failure is swallowed and the empty dict is returned** — `checkpoint.py:76-95`:

```python
except ModuleNotFoundError:
    logger.error(
        "Unable to load cached data - your code has changed in a way that's incompatible…"
        "\n  - Pulled updates that modified class definitions in a way that's incompatible…"
        "\n\nRemoving invalid cache data stored at path: .langgraph_api"
    )
    try:
        os.remove(self.filename)
    except Exception:
        pass
except Exception as e:
    logger.error("Failed to load cached data: %s", str(e))
    try:
        os.remove(self.filename)
    except Exception:
        pass
return d                             # ← empty, and already registered
```

Note the recovery is itself dead code: `self.filename` is the **prefix**
`.langgraph_api/.langgraph_checkpoint.` with no `N.pckl` suffix (set at `checkpoint.py:59`, used
with the suffix at `:69`). `os.remove` therefore always raises `FileNotFoundError` and is always
swallowed. The file survives — which is worse, because the next step overwrites it with valid,
empty data instead of leaving a corrupt file a human could notice.

**3. The flush loop writes it back** — `_persistence.py:17,51-57`:

```python
_flush_interval: int = 10
...
while not stop_event.wait(timeout=_flush_interval):
    for store_key in list(_stores.keys()):
        if store := _stores[store_key]():
            store.sync()
```

**4. `sync` is an atomic replace** — `langgraph/checkpoint/memory/__init__.py:657-670`:

```python
def sync(self) -> None:
    tempname = self.filename + ".tmp"
    fileobj = open(tempname, "wb" ...)
    try:
        self.dump(fileobj)          # pickle.dump(dict(self), …) — the empty dict
    ...
    shutil.move(tempname, self.filename)  # atomic commit
```

## Impact

The `ModuleNotFoundError` branch names the trigger in its own message: *"Pulled updates that
modified class definitions in a way that's incompatible with the cache."* So the ordinary act of
updating a dependency can destroy every local conversation, with no confirmation and no backup.

The generic `except Exception` widens this considerably. Unpickling imports the modules the pickle
references, and `langgraph_api.config` reads configuration at import time — so an unset environment
variable is enough to raise inside `pickle.load` and reach the same path.

## Suggested fix

**A persistence layer that cannot read its file must refuse to write it.** Any of:

1. Register with the flush loop only *after* a successful load, so a failed one cannot be flushed.
2. Set a `read_only` flag on the dict when the load fails, and have `sync()` return early.
3. Move the unreadable file aside (`.pckl.corrupt-<timestamp>`) before continuing, so recovery
   is possible.

(1) is the smallest and closes the whole path. (3) is worth having regardless, since a user whose
pickle is unreadable currently has no copy of it once the ten seconds elapse.

Separately, `os.remove(self.filename)` at `:88` and `:94` should target `thisfname`, or be removed
— as written it can never succeed, which hides how much the failure costs.
