# Theorizer reliability + React update-loop — plan

Status: Problem 1 (theorizer) — poll-truncation bug FOUND + FIXED · Problem 2 (React loop) PENDING · updated 2026-07-14

Two problems to fix: (1) the theorizer works from the CLI but fails when the
subagent runs it; (2) a React "Maximum update depth exceeded" warning.

### Status log
- **#11 (merged)** — initial `generate_theories` tool + deterministic artifact
  parser + tests. (Also carried Phase 1 UI: clickable references, collapsible gaps.)
- **#12 (merged)** — novelty OFF by default (not auto-enabled from "novelty" in
  the question); replaced the resume-loop with a bounded wait + real elapsed.
- **#13 (open)** — self-updating Theories card: tool submits fast, artifact
  carries `status`/`task_id`, backend status route + frontend auto-poll. This is
  the final shape of Problem 1.
- **Poll truncation bug (this branch)** — the real reason a run left the card
  "generating" for 20 h. Reproduced live against the CLI: submit + autonomous
  server-side run both work, but **the status poll could never detect
  completion.** `poll_theory_status` ran `asta generate-theories task <id>`
  through `sandbox.aexecute`, whose output is capped at
  `EXECUTE_OUTPUT_MAX_BYTES` (32 KB). A completed task record is **~520 KB** (it
  embeds the full paper store + per-paper extraction markdown), so it was
  clipped to invalid JSON → `_extract_json` → `None` → `_state_of` → `None` →
  fall-through to `"running"` **forever**. The submit command, the flags, and
  the artifact parser were all correct — only the poll read was broken.
  *Fix:* reduce the task record **inside the sandbox** (drop `theory_store` /
  `extraction` / paperstore; keep `status` + `theory` artifacts, trimmed to the
  fields the parser reads) so ~19 KB crosses back, and read it via a new
  `aexecute_untruncated` (server-side callers parse the output; it never reaches
  the model). Validated end-to-end against a real completed task (8 theories,
  working S2 links); regression tests pin the 32 KB failure mode.
- **Agent-readable theories + real failure reasons (this branch)** — two
  follow-ups after the truncation fix:
  1. *Theories the agent can actually use.* Completed/failed runs are now
     persisted to the sandbox on the terminal poll: `theories/<task_id>.md`
     (readable) + `.json` (structured), or `theories/<task_id>.error.log`.
     Written under a **non-hidden** dir so `FileSyncMiddleware` surfaces them as
     file artifacts and the coordinator can `read_file` them on a later turn
     (prompt hint added) — instead of the theories living only in the frontend
     card. Durable + a first provenance edge (theory file → papers).
  2. *Real failure reasons.* A live re-run of the user's exact question actually
     **completed** (8 theories; full record ~1 MB → reduces to ~33 KB, i.e. just
     over the 32 KB cap, so the untruncated read is load-bearing), so the card's
     "Theorizer task failed" was a **transient asta-side failure** — but we were
     also swallowing the real reason: `poll_theory_status` only read
     `status.message.short_desc`. New `_failure_reason` probes text parts + data
     keys (error/message/detail/reason) and logs it; the error is surfaced in
     the card and written to the `.error.log`.
- **Problem 2 (React loop)** — still pending. Isolated harness repro of the
  self-updating card (steady `running` + streamed-tick churn) did **not** loop;
  needs the running→completed transition and/or a full-app component stack.

---

## Problem 1 — theorizer fails when the *agent* runs it

### Root cause (evidence-based)
The theorizer is a long async A2A task (5–15 min). Today the **subagent drives
the wait by hand**: submit → poll. Whether via many tiny `execute` calls (the
original skill) or one long blocking `execute` loop (my interim prompt patch),
it depends on the **LLM staying patient across many minutes and re-issuing**.
LLMs don't reliably loop for 15 minutes — they give up and return empty theories
while the task is still `working`. That is exactly what happened to task
`7438c565…`, which in fact **completed with 7 grounded theories** once waited on.

Conclusion: this cannot be fixed with prompt wording. The wait must be
**deterministic code**, not model discretion.

### Fix — shipped design (self-updating card)
The design went through two intermediate steps (a blocking poll loop, then a
bounded wait + resume) before landing on the version that doesn't rely on the
LLM waiting at all. Both intermediates still failed the real UX: a long run left
the LLM to give up, and the card stayed empty because nothing watched the task
after the turn ended. Final shape (#13):

- **Tool submits and returns fast.** `generate_theories` runs `asta … --no-wait`
  in the active sandbox (via `_active_sandbox`) and returns immediately with
  `{status:"running", task_id}` — no multi-minute block. `poll_theory_status`
  is factored out as a one-shot poll+parse reused by the tool and the route.
- **Artifact carries lifecycle.** `HypothesisOutput`/payload/middleware gain
  `status` (running|completed|failed) + `task_id`. The subagent emits a
  `running` artifact on submit, so the Theories card shows a live progress state
  immediately instead of an empty box.
- **Backend status route.** `GET /theorizer/{thread_id}/{task_id}` polls the task
  in the thread's sandbox, parses artifacts into the `HypothesisOutput` shape
  (laws + `PaperRef`s with links), validates the task id, and returns
  `completed`/`running`/`failed`/`unavailable`.
- **Frontend auto-poll = the notification.** `useTheorizerStatus` polls that
  route every 30 s while a hypothesis artifact is `running` and swaps in the
  theories the moment it completes. Persistence is free: the `task_id` lives in
  graph state, so a refresh just re-polls; no state surgery, no localStorage.
- **Novelty OFF by default** and not auto-enabled from question wording — keeps
  normal runs ~10 min so most complete without any long wait.

Net: ask → live "generating… ~N min" card → theories + links appear on their own.
No button, no notification, no `task_id` juggling by the user.

### Deterministic parser (validated on real data first)
From the completed task `7438c565…`, a `theory` artifact has:
`data.name`/`data.description` (theory statement), `data.content` (laws),
`data.entities[paper-*]` = `{displayLabel (citation), s2Metadata (DOI/corpusId
→ PaperRef.url)}`, and `data.annotations` linking papers as support/contradict.
`novelty` artifacts carry the novelty signal. **Build and test the parser against
`7438c565…` (7 theories) before wiring anything** — no running app needed.

### Files (as shipped)
- `backend/theory_tools.py` — tool, `poll_theory_status`, artifact→HypothesisOutput parser
- `backend/agent.py` / `backend/subagents.py` — wire `theory_tools` into `hypothesis_generator`; submit-fast prompt
- `backend/schemas.py` / `backend/middleware/artifacts.py` — `status` + `task_id` on the hypothesis artifact
- `backend/routes/artifacts.py` + `backend/routes/__init__.py` — `GET /theorizer/{thread}/{task}` status route
- `backend/prompts.py` — coordinator: never fabricate; tell user the card fills in on its own
- `skills/hypothesis_generation/SKILL.md` — submit-fast, panel-auto-updates flow
- frontend: `types.ts`, `lib/artifacts.ts`, `lib/theorizerClient.ts` (poll hook),
  `lib/fileClient.ts` (shared token getter), `components/ArtifactPanel.tsx`
  (progress card), `styles.css`
- `tests/test_theory_tools.py` + root `conftest.py` — CLI-contract, parser, poll, task-id tests

### Risks / mitigations
- Long run + turn-driven app → frontend auto-polls a cheap status route; nothing blocks a turn.
- Sandbox idle TTL (600 s) → status polls keep it warm during a run; if it expires the route returns `unavailable`.
- Novelty eval (30–60 min) → off by default and not auto-enabled.

---

## Problem 2 — React "Maximum update depth exceeded"

### Status
The usual culprits are already guarded: `handleSnapshotChange` is
`useCallback(…, [])`, artifacts/subagents go through `useStableValue`, and panel
handlers use `useStableCallback`. So this is **not** the obvious fresh-reference
loop, and I won't blind-edit it.

### Plan
1. Reproduce in the running app and read `preview_console_logs` — the React
   warning includes the **component stack** that names the offending component.
2. Check the prime suspects the stack points to first: `ArtifactPanel`'s
   `activeTab` `useEffect` (a `setActiveTab` dependency cycle) and any effect
   keyed on `artifacts.*` that fires during streaming ticks.
3. Fix the specific cycle; verify the warning is gone across a full streamed run.

---

## Sequencing
1. [x] Build + unit-test the theory parser against task `7438c565…` (offline).
2. [x] Implement `generate_theories` + wiring + prompt/skill (#11), novelty-off +
   bounded wait (#12), submit-fast + status route + frontend auto-poll (#13).
3. [ ] Reproduce + fix the React update-depth loop from its console stack.
4. [~] End-to-end: validated on the user's real runs (task completed with theories;
   handoff/progress worked). Full browser confirmation of the self-updating card
   (#13) is best done on deploy — submit a question, watch the card populate itself.
