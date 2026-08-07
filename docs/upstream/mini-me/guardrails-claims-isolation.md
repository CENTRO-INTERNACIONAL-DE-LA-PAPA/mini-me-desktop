# The approval prompt claims sandbox isolation that host execution does not provide

**Repo:** Mini-Me (`backend/guardrails.py`)
**Severity:** high — it is a safety claim, shown to the person granting permission
**Found:** 2026-07-31

## Summary

The human-approval guardrail describes commands as running in an isolated sandbox. That is true
when the agent executes through the LangSmith sandbox. It is **not** true when execution is local,
where `execute` runs against the researcher's own filesystem with their own permissions.

The wording is the one thing standing between a researcher and a command they are about to
approve, and in local mode it tells them the blast radius is smaller than it is.

## Why it matters here

The desktop client runs local execution as its default: it is a single-user research workbench,
the outputs are meant to be on the researcher's own disk, and requiring a cloud sandbox for a
local tool is a dependency without a purpose.

That is a legitimate mode, and the guardrail text was written before it existed. The problem is
not the mode; it is that the prompt does not distinguish them.

## Suggested fix

Make the claim conditional on the backend actually in use, and say plainly what the researcher is
agreeing to:

- **sandbox:** "runs in an isolated sandbox — it cannot reach your files"
- **local:** "runs on this machine, with your permissions, in `<workspace path>`"

The second sentence is not a warning to be softened. Someone approving a command needs to know
which of the two they are in, and it is the only place they can learn it.
