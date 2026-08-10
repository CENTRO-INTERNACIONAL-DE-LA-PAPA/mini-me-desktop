# The theorizer reports an inferred cause instead of the command's real output

**Repo:** Mini-Me (`backend/`)
**Severity:** high — not a crash, but it cost seven rounds of debugging
**Found:** 2026-08-01

## Summary

When `asta generate-theories` produces no task id, the tool reports:

> no task id was returned, which usually means the access token is missing or expired

That sentence is a **hypothesis**, and it is presented where the command's actual result should be.
The real output — exit code, stdout, stderr — is discarded.

## Why it is expensive

The `asta` CLI fails **silently** in this case: exit 0, nothing on stdout, nothing on stderr, when
`ASTA_TOKEN` is set to a stale value. Reproduced by hand:

```
ASTA_TOKEN=<valid>  asta generate-theories … --no-wait  →  exit 0, a task id
ASTA_TOKEN=<stale>  asta generate-theories … --no-wait  →  exit 0, EMPTY OUTPUT
```

Combined, the two behaviours defeat both standard debugging moves:

- "read the error" returns a guess, which happened to be right about the *category* and useless
  about the *source*;
- "log the failures" catches nothing, because exit 0 is not a failure.

Six separate defects were found and fixed while chasing it, every one of them real and none of
them the cause:

| | what was wrong | why it looked right |
|---|---|---|
| 1 | token minted only at spawn | the message said "expired" |
| 2 | read once per workspace | ditto |
| 3 | account lacked `enroll:theory_generation` | two accounts genuinely differed |
| 4 | an overlay copy was months old | the fix was correct, just not running |
| 5 | an environment field was guessed, not checked | crashed loudly, so looked like *the* bug |
| 6 | `~/.local/bin` missing from `PATH` | reproduced exit 127 exactly |

The step that actually worked was running the command by hand with a deliberately bad token.

## Suggested fix

Report what happened, not what it might mean:

```python
if not task_id:
    raise RuntimeError(
        f"asta generate-theories returned no task id\n"
        f"  exit code : {result.returncode}\n"
        f"  stdout    : {result.stdout.strip() or '<empty>'}\n"
        f"  stderr    : {result.stderr.strip() or '<empty>'}\n"
        f"  token from: {token_source}"   # keychain / CLI / env — never the value
    )
```

A guess may be *added* after the facts, clearly marked as one. It must not replace them. `exit 0`
with empty output is itself the most diagnostic fact available here, and it is currently the one
thing thrown away.

Naming the **source** of the token (not its value) is what would have ended this in one round: the
token was valid-looking and came from a keychain entry set days earlier.
