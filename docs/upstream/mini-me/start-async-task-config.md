# `start_async_task` launches background runs with no config

**Repo:** `deepagents` (`middleware/async_subagents.py`), surfaced through Mini-Me
**Severity:** high — background work is unusable on any self-hosted deployment
**Found:** 2026-08-01

## Summary

Both `start_async_task` and `astart_async_task` create the background run without passing
`config` (`async_subagents.py:291-296` and `:331-336`):

```python
run = client.runs.create(
    thread_id=thread["thread_id"],
    assistant_id=spec["graph_id"],
    input={"messages": [{"role": "user", "content": description}]},
)
```

`RunsClient.create` accepts a `config` argument. Without it, the background run starts with an
empty `RunnableConfig`.

## Impact

Everything a deployment carries per-request is dropped at the boundary:

- **the model**, where it is resolved from the request rather than from process environment;
- **the API key**, for any deployment that passes keys per-request instead of setting them on the
  server — which is the correct arrangement when the agent has a shell tool, since anything on the
  server's environment is readable by the code the agent writes;
- **the recursion limit**, so a background worker gets the default and stops mid-task on work the
  foreground agent would have completed.

The result is that background subagents work under LangGraph Platform, where the platform supplies
these, and fail on a self-hosted or local deployment — with no error at the launch site, because
the run is created successfully and only fails later, inside a graph nobody is watching.

## Suggested fix

Forward the caller's config, minus the keys that must not be inherited:

```python
parent = runtime.config or {}
run = client.runs.create(
    thread_id=thread["thread_id"],
    assistant_id=spec["graph_id"],
    input={"messages": [{"role": "user", "content": description}]},
    config={
        "configurable": {
            k: v for k, v in parent.get("configurable", {}).items()
            # not thread_id / checkpoint_ns: the background run is its own thread
            if k not in ("thread_id", "checkpoint_id", "checkpoint_ns")
        },
        "recursion_limit": parent.get("recursion_limit", 25),
    },
)
```

A related note for whoever picks this up: a graph factory declared as `async def agent(config)`
must be called with that argument. A wrapper that takes no parameter raises `TypeError` during
graph *construction*, before any node runs — which writes no checkpoint, so
`GET /threads/{id}/state` has no task to hang the error on and the failure is invisible from the
API. That cost two misdiagnoses here before the traceback was found in the server log.
