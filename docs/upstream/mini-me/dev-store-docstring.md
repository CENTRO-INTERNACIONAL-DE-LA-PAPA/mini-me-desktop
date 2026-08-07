# `make_backend`'s docstring says the dev store loses content on restart. It does not.

**Repo:** Mini-Me (`backend/agent.py`)
**Severity:** low as code, higher as guidance
**Found:** 2026-08-06

## Summary

`make_backend`'s docstring states:

> In `langgraph dev` the store is in-memory and loses content on process restart; production
> deployments should configure a durable LangGraph store (Postgres / Redis) so memories survive.

The store `langgraph dev` installs is `DiskBackedInMemStore`
(`langgraph_runtime_inmem/store.py:18`), which subclasses `InMemoryStore` and replaces `_data` and
`_vectors` with `PersistentDict`s backed by `.langgraph_api/store.pckl` and
`store.vectors.pckl` (`store.py:22-24, 83-84`). They are registered with a flush thread that syncs
every ten seconds (`_persistence.py:17`), and persistence is on unless
`LANGGRAPH_DISABLE_FILE_PERSISTENCE=true` is set.

**Memories and skills survive a restart.**

## Why it is worth fixing despite being a comment

The desktop client has a **Restart backend** button and tells researchers to press it after an
update. If the docstring were right, that button would be discarding their memories every time —
so anyone reading it has to choose between trusting the documentation and using the product.

A stale comment that contradicts observable behaviour also costs the next reader the time it takes
to disprove it, which in this case meant reading three files in the runtime package.

## Suggested fix

Replace the claim with what the runtime actually does, and keep the production advice, which is
still sound for a different reason (concurrency and durability guarantees, not restart survival).
