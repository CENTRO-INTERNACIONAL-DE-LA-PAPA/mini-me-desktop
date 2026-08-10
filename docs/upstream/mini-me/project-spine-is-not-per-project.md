# The research spine is one per user, and cannot be scoped to a project

**Repo:** Mini-Me (`backend/runtime.py`, `backend/routes/project.py`)
**Severity:** medium — correct for one line of work, wrong as soon as there are two
**Found:** 2026-08-07

## Summary

`_project_namespace` keys the research spine as `(user_id, "project")`
(`backend/runtime.py:141-154`), and says so deliberately:

> User-scoped as `(user_id, "project")` — deliberately **not** keyed by assistant_id (unlike
> memories). Two reasons: The research project is the user's, spanning every assistant/thread.

That is a sound decision for a researcher with one line of work. With more than one it means a
single spine that mixes every study a person has ever run and never forgets a deleted
conversation — a mission from one project above completed items from another.

## Why a client cannot fix it alone

The namespace is computed in two places, and they must agree:

- `_project_namespace_for_runtime` (`runtime.py:157`) runs inside a turn and can see whatever the
  request's `configurable` carries;
- `get_project` and `patch_project` (`routes/project.py:76,100`) run in HTTP handlers with no run
  config — and `runtime.py`'s docstring names that symmetry as the reason for the current key:

  > It lets the `/project` HTTP route reproduce the exact same namespace from
  > `request.user.identity` without having to resolve the platform's assistant_id, which a custom
  > route does not see.

Scope only the runtime side and `GET /project` reads a namespace turns no longer write to: the
panel goes blank instead of becoming correct.

## Suggested change

Take an optional project on both sides, defaulting to today's behaviour.

```python
def _project_namespace(user_id: str, project: str = "") -> tuple[str, ...]:
    base = (user_id, "project")
    return (*base, project) if project else base
```

and in the route:

```python
project = request.query_params.get("project", "").strip()
state = await load_project(store, _project_namespace(user_id, project))
```

with `_project_namespace_for_runtime` reading the same name from `configurable`.

**Backwards compatible by construction:** with no project the namespace is unchanged, so every
spine that exists today is what an unscoped caller reads.

## What the desktop client does meanwhile

`overlay/minime_local/spine.py` patches both — the namespace function and the two route handlers —
through the import hook, so the checkout stays byte-for-byte upstream. It is a bridge and not a
preference: the seam belongs here, where the route can simply take a parameter, rather than in an
overlay that has to smuggle one through a `ContextVar`.
