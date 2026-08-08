# Asta × Mini-Me Integration Plan

Status: Phase 1 SHIPPED (merged) · theorizer WORKING (PR #13, merging) · Research Project spine (C) COMPLETE + MERGED — advisory #19, promote-to-execute #20, mission/backlog edit #22 · P2 (DataVoyager) MERGED #25 + async/persist P2.1/P2.2 #26 · P4 (provenance) MERGED #27/#28/#29 · **P5 (explicit Projects + opt-in autonomous run loop) MERGED (#30/#32)** · Asta token self-service refresh MERGED (#33) · **Mission→model-context fix OPEN (#34)** · **PDF-librarian open-access download OPEN (#35)** · **P6 (Zed-like Rust desktop workbench) KICKOFF — see Phase 6 below** · updated 2026-07-29

### Status log
- **P6 — Zed-like Rust desktop workbench — KICKOFF (2026-07-29).** User feels
  constrained by the browser app and wants a native desktop research workbench,
  taking inspiration from Zed (`zed-industries/zed`, built on its own GPU UI
  framework GPUI). Evaluated the options: forking Zed (a GPL code editor — wrong
  shape) vs GPUI standalone (Apache-2.0, but a full Rust frontend rewrite) vs a
  Rust desktop shell (Tauri) wrapping the existing React frontend. My
  recommendation was **Tauri** (keep the React UX + Python backend, gain native
  files/processes/offline, and running the backend locally would end the Asta
  static-token pain since the local `asta` CLI auto-refreshes — see the #33
  entry). **User chose the GPUI-rewrite direction**; scope it honestly with its
  cost/risk and keep Tauri as the flagged lower-risk fallback. No app code yet —
  next deliverable is the Phase 6 spike doc + a first milestone. Agents stay in
  Python/TS either way (run as a sidecar/subprocess). Full plan: **Phase 6** below.
- **Mission → model context fix — OPEN (PR #34, branch `claude/mini-me-repo-session-633671`).**
  Field bug: setting/editing the Research Project *mission* did not change the
  agent's behaviour (a coffea-arabica mission produced a summary about LLMs in
  science). Root cause: the mission was persisted in the spine and rendered in
  the frontend by `ProjectSpineMiddleware`, but **never injected into the
  coordinator's LLM context** (`COORDINATOR_SYSTEM_PROMPT` is static, no mission
  slot). So editing it changed what was *shown*, not what the agent *read*, and
  Autopilot steps phrased "…the project mission" had no referent. Fix: pure
  `render_mission_context` (`backend/project.py`) + an `awrap_model_call` hook on
  `ProjectSpineMiddleware` that appends the mission (+ completed/pending + a
  grounding instruction) to the coordinator prompt each turn; static prompt stays
  the cacheable prefix; no mission ⇒ passthrough. Also fixes plan authoring (the
  coordinator can now hand the real mission to `research_planner`). 4 new tests;
  full suite green. **Needs SME review + redeploy.**
- **PDF-librarian open-access download — OPEN (PR #35, branch `claude/mini-me-pdf-download`).**
  Gap found while debugging an empty sandbox: the PDF librarian could OCR/index/
  search PDFs already on disk but could **not fetch** a paper. Confirmed the
  `asta` CLI has **no download command**, `asta documents add <url>` only records
  a URL (no bytes), and `academic_researcher` is told not to download — so "read
  these discovered papers" silently did nothing (`paper_count: 0`, sometimes a
  bogus `/data/asta_index` path). Fix: `skills/pdf_library/scripts/fetch_paper.py`
  resolves a ref (DOI/arXiv/CorpusId/PMID/**title**) → open-access URL via
  `asta papers` (prefer `openAccessPdf`, **fall back to arXiv** since that field
  is often empty even for arXiv) → fetch into `./papers/` → validate it is really
  a PDF (so a landing-page HTML is never saved). Open-access only; paywalled ⇒
  `no_oa` ("ask the user to upload"). Wired as **Step 0 — Download** in the skill
  + a fourth librarian move; prompt hardened (never claim extraction for a file
  not on disk; keep the index at `.asta/documents`). 10 new tests; verified live
  against an arXiv PDF. **Two review items: sandbox network egress must allow the
  fetch; landing-page OA URLs currently report `fetch_failed` (future follow-up).**
- **Asta token self-service refresh — MERGED (PR #33).**
  Root-caused a field failure: the theorizer/DataVoyager submit returned
  "Could not obtain a task id" because the deployed backend's single static
  `ASTA_TOKEN` env var had **expired** (Asta access tokens last ~1 week; the
  local CLI auto-refreshes, a static env var does not). Reproduced exactly by
  running the submit with a bad token → empty stdout, exit 0 → no task id.
  Fix: **per-user, paste-and-store** Asta tokens so refresh is self-service (no
  redeploy). New pure `backend/asta_auth.py` (unverified JWT `exp` decode → status
  + reject expired pastes; unit-tested), Vault storage
  (`minime/{user_id}/asta/token`), `/config/asta` GET/POST/DELETE routes, and
  per-request injection: `agent()` resolves the caller's token (client
  `__asta_token` → Vault) into an `_active_asta_token` ContextVar that
  `sandbox._aexecute_core` prefers over the env `ASTA_TOKEN` fallback. Frontend:
  an "Asta connection" card in the Model & API settings panel (status + expiry
  countdown, paste/update/remove), mirroring the BYO-key two-mode (vault/client)
  design. Theorizer/DataVoyager "no task id" errors now name the likely cause
  ("your Asta token may be expired — refresh it in Settings"). Still manual per
  expiry (~weekly, 20 s); server-side auto-refresh via the refresh token is a
  deliberately deferred follow-up (needs Asta OAuth internals). 8 new tests in
  `tests/test_asta_auth.py`; full suite 153 green; `tsc -b` + `vite build` green.
- **Phase 1 — DONE, merged (#11):** collapsible Knowledge gaps + clickable theory
  references (structured `PaperRef` end to end, with a Semantic Scholar search
  fallback). This turned out to be the first provenance edge (theory → paper).
- **Theorizer reliability — WORKING, PR #13 (merging):** grew into a
  `generate_theories` tool + self-updating Theories card (#11/#12/#13). #13's
  card was still broken in the field (stuck "generating" for 20 h) until this
  session found the cause: the status poll ran the CLI through the 32 KB sandbox
  execute cap, but a completed task record is 0.5–1 MB, so it clipped to invalid
  JSON and reported `running` forever. Fixed by reducing the record in-sandbox +
  reading it untruncated; also added agent-readable theory files
  (`theories/<id>.md`), real failure reasons, and a **paper-link fix** (Semantic
  Scholar corpus ids need the `CorpusID:` prefix — bare numeric ids 404'd, which
  is why theory links didn't work). **User-confirmed working locally.**
  Full detail in [theorizer-reliability-plan.md](theorizer-reliability-plan.md).
  **→ Deploy after merge** (the 20 h report was on the deployed app; the fix only
  reaches users on deploy).
- **Next (specced, ready to start cold):** see *Next work items* below —
  **(B)** theorizer papers → full text in the PDF Librarian, and **(C)** the
  Research Project spine. Either can go first; (C) is the differentiator.
- **Deploy surfaced a real Asta-side task failure (2026-07-14):** after deploying
  the poll fix, a theorizer run reached `failed` server-side but the card/error.log
  showed only the generic "Theorizer task failed." Root cause of the *useless
  reason* (not the run failing): the in-sandbox reducer (`_REDUCE_TASK_PY`)
  forwarded only `status.state` + `status.message`, so any failure detail Asta
  stores elsewhere (top-level `error`/`detail`, a plain-string `status.message`,
  or the message-history tail) was dropped before `_failure_reason` ran. Fixed:
  for non-completed tasks the reducer now also forwards the full status,
  top-level error fields, and the last few history-message texts; `_failure_reason`
  reads all of them. **Root cause CONFIRMED from the real record (authenticated
  poll in the thread's sandbox):** Asta failed the run server-side with a *bare*
  status — `{"state":"failed","timestamp":...}`, `status.message` **null**, no
  top-level `error`/`detail`, history just `"Task accepted"`. So **there is no
  machine-readable failure reason in the record at all** — the generic string was
  not a parsing miss, it was all Asta gave. The `artifacts`, though, showed the run
  got well past paper-finding: it produced an `extraction-schema` and a
  `theory_store`, then died before emitting any `theory` artifact. So this was NOT
  a bad query / "no papers" — it was a **transient Asta-side crash mid-extraction**.
  Final fix (revised after seeing the record): reducer forwards top-level error
  hints *and the artifact TYPES produced* for non-completed tasks (dropped the
  history-text passthrough — its tail `"Task accepted"` would masquerade as a
  reason); `_failure_reason` now returns an honest message — "Asta ended the run as
  failed without reporting a reason (it produced intermediate artifacts —
  extraction-schema, theory_store — but no theories). This is usually a transient
  Asta-side error; try running it again." **Not a Mini-Me bug; retry is the fix.**
  Also: the `asta` CLI prints errors to **stderr** and the auth check ("Not
  authenticated. Run 'asta auth login'.") only fails in a bare interactive shell —
  the app authenticates per-exec via `ASTA_TOKEN` (`backend/sandbox.py:538`), and
  submit succeeding (task id returned) proves auth held during the run. Console
  `400` on `/threads/<id>/history` is a LangGraph-platform call, separate from the
  theorizer; re-check after this deploy.
- **⚠️ INTERMITTENT — the Asta Theorizer *hosted service* is flaky server-side
  (NOT a Mini-Me bug, NOT a permanent block):** the theorizer integration itself
  works end-to-end; runs succeed when Asta's service is healthy and fail when it
  isn't. During a bad window (observed 2026-07-14) runs failed **identically
  across every axis** — sandbox AND local (same `asta` 0.101.0), default models
  (`gpt-5.2`/`gpt-5-mini`) AND explicit `gpt-4o`/`gpt-4o-mini` AND
  `claude-sonnet-4-6`/`claude-haiku-4-5`, trivial "why does caffeine increase
  alertness?" AND the coffee-subgenome query, 3→30 papers. Each built
  `extraction-schema` + `theory_store`, then died at the first extraction/synthesis
  model call with a bare `status:{state:failed}`, no reason even at
  `ASTA_LOG_LEVEL=DEBUG`. **Conclusion: transient Asta hosted-service failure for
  this account** — retry later, it recovers. Mini-Me side is done: honest failure
  surfacing (reducer forwards `artifact_types`; `_failure_reason` says it failed
  inside Asta, not Mini-Me) so users get "try again," not a mystery. **Impact on
  work item (B):** gated on the service being up, not a code blocker — build (B)
  whenever a trivial theorizer query `completes`. **Escalate to Asta** (asta-plugins
  repo / support) with task ids `0c50d194…`, `c28a1b7d…` only if a bad window
  persists for long.
- **(C) Research Project spine — phase 1 IMPLEMENTED (PR #19, advisory only):**
  a persistent per-user *mission* + *Completed / Pending Work* now lives in the
  LangGraph store under a new `("project", assistant_id, user_id)` namespace
  (`backend/runtime.py`), so it survives across every thread. A coordinator-level
  `ProjectSpineMiddleware` (`backend/middleware/project.py`) loads it at turn
  start (so reopening the app shows the mission immediately), folds the turn's
  merged artifacts into it at turn end, persists it, and emits it as a new
  `project` artifact slice. The derivation logic (`backend/project.py`) is pure
  and unit-tested: `summarize_completed` (artifact-grounded, category-keyed so
  counts refresh in place) and `derive_suggestions` (≤3 prioritized next steps,
  e.g. theories-but-no-data-test → Diagnostic Analytics; report-with-0-sources →
  Academic Research). Mission is seeded from the first human message and is
  sticky. Frontend renders a small **advisory** `ProjectPanel` above the Outputs
  tabs — mission, suggested next steps (title + rationale + which subagent could
  run), and collapsible Completed/Pending. **Nothing auto-runs**; suggestions are
  read-only text (org policy: human-gated). 24 new tests in `tests/test_project.py`;
  `tsc` + `vite build` green. **Next (phase 2 of C):** let the user *promote* a
  suggestion to execution (plan → execute → review), reusing the subagents.
- **Open:** React "Maximum update depth" warning (needs a live component stack;
  isolated harness did not reproduce it — needs a full-app stack).
- **Not started:** the rest of Phase 2+ (DataVoyager, provenance graph, autonomy).

## Goal

Turn Mini-Me from a set of transactional tools into a **persistent research
workbench** where a scientist feels durable value: continuity across sessions,
traceable evidence for every claim, and a system that proposes the next step —
all with the scientist as reviewer-in-the-loop.

## Architecture note (how Asta fits)

- **Asta *tools*** (theorizer, PDF librarian, DataVoyager) → become **subagents**,
  invoked via the `asta` CLI inside the sandbox. This is already how the
  `hypothesis_generator` and `pdf_librarian` subagents work.
- **Asta *flows* / asta-assistant** → an orchestration **pattern**, not a plugin
  to import. The coordinator (`backend/subagents.py` + `backend/prompts.py`)
  already *is* the orchestrator; we upgrade it to sequence steps and hold a
  persistent project, rather than nesting a foreign agent harness inside a
  subagent.

Net: the "flow" brain stays in the main coordinator; Asta tools are the
subagents it calls.

---

## Phase 1 — UI refinements — ✅ SHIPPED (merged in #11)

Self-contained, low risk, no coordinator changes. Both 1a and 1b are done; the
per-file checkboxes below are kept as the record of what was changed.

### 1a. Collapsible "Knowledge gaps" (frontend only)

- [ ] `frontend/src/components/ArtifactPanel.tsx` (~L181–190): extract the
      `knowledge-gaps` block into a small collapsible. Make the `<h4>` a button
      with `aria-expanded` + chevron; gate the `<ul>` on `useState`. Reuse the
      `.subagent-toggle` chevron pattern already used in `TheoryCard`/`SourceCard`.
- [ ] `frontend/src/styles.css` (`.knowledge-gaps h4`, ~L3630): make the header a
      clickable flex row (cursor, gap for chevron).
- Default state: **collapsed** (theories are the headline; gaps are secondary).

### 1b. Clickable reference links in theories (full stack)

**Why it's not just CSS:** `supporting_papers`/`conflicting_papers` are plain
`List[str]` today (`backend/schemas.py:59`, `frontend/src/types.ts:94`). The
paper's DOI / Semantic Scholar corpus ID is discarded in the theorizer skill
before it reaches the frontend — there is nothing to link to yet.

**Approach: hybrid** — emit a structured reference when an ID is available; fall
back to a search link when it isn't, so links never break and the feature works
from day one.

Backend:
- [ ] `backend/schemas.py`: add `PaperRef` model
      `{ citation: str, url: str | None, doi: str | None, corpus_id: str | None }`.
      Change `Theory.supporting_papers` / `conflicting_papers` from `List[str]` →
      `List[PaperRef]`. Add a `PaperRefPayload` TypedDict and update
      `TheoryPayload` to use it.
- [ ] `backend/middleware/artifacts.py`: confirm nested `PaperRef` models
      serialize (via `model_dump`) and survive the `_merge_artifacts` reducer.
- [ ] `skills/hypothesis_generation/SKILL.md` (Step 3 mapping): keep the
      readable citation string **and** pull each paper's identifier from the
      Asta task artifacts, building a canonical URL:
      DOI → `https://doi.org/{doi}`, else corpus ID →
      `https://www.semanticscholar.org/paper/{corpusId}`, else arXiv →
      `https://arxiv.org/abs/{id}`, else `url = null`.

Frontend:
- [ ] `frontend/src/types.ts`: add `PaperRef` + `PaperRefPayload`; retype the
      theory arrays.
- [ ] `frontend/src/lib/artifacts.ts`: normalize nested refs and **tolerate both
      shapes** — if an item is a string, wrap as `{ citation, url: null }` (keeps
      old persisted hypotheses working; safe migration).
- [ ] `frontend/src/components/TheoryCard.tsx` (~L56–71): render each paper as
      `<a href={url} target="_blank" rel="noreferrer">` with the `ExternalLink`
      icon when `url` exists; otherwise build a Semantic Scholar search link from
      the citation text. Reuse the `.source-card a` pattern from `SourceCard`.
- [ ] `frontend/src/styles.css` (`.theory-papers a`, ~L3602): link styling.

**Validation before finalizing the skill mapping:** run one real theorizer task
and inspect `asta generate-theories task <id>` output to confirm the exact ID
field names. The search-link fallback means we are not blocked on this.

---

## Phase 2+ — Roadmap (later, in order)

Additive until Phase 3; Phase 3 is where the coordinator gains a project spine.

- [x] **P2 — DataVoyager subagent** (`asta analyze-data`): auto-generates and
      tests hypotheses against a local dataset. Closes the loop from
      *theory* → *test it against my data*, which nothing in Mini-Me did before.
      **IMPLEMENTED + MERGED — PR #25.** Additive subagent mirroring
      hypothesis_generator/pdf_librarian: `backend/datavoyager_tools.py` (CLI
      contract unit-tested), `DataAnalysisResults` + `analyses` slice,
      `data_voyager` subagent + coordinator routing, capture middleware,
      `skills/data_voyager/`, and an **Analysis** tab on the frontend. Also
      repoints the spine's "Test your theories against data" suggestion at the new
      subagent. **Async + durability follow-ups (P2.1/P2.2) DONE — PR #26 (open).**
    - [x] **P2.1 — Make DataVoyager async (don't block the chat). DONE (PR #26).**
          `analyze_data` now just *submits* and returns a `task_id` + `context_id`
          immediately (like the theorizer); the subagent emits a `running`
          DataAnalysisResults and the new `/analyze-data/{thread}/{task}` route +
          `useDataVoyagerStatus` hook fill the Analysis card live. **The earlier
          "blocker" was wrong:** `asta analyze-data task <id>` IS a cheap,
          non-blocking status fetch (analogue of `generate-theories task <id>`), so
          the theorizer's status-route pattern dropped in directly — no background
          job needed. The live card shows the run's narrative summary (structured
          findings still need a follow-up ask, since only the subagent LLM can
          synthesize them from the record).
    - [x] **P2.2 — Persist + export DataVoyager outputs. DONE (PR #26).** On a
          terminal poll the route calls `persist_analysis_outputs`: writes
          `analysis/<task_id>.md` + `.json` and runs `asta artifacts --input …
          --output analysis/<task_id>/export --format md` entirely in-sandbox, so
          the charts/notebook/tables surface via FileSyncMiddleware and a later
          turn can `read_file` the results. Coordinator + subagent prompts now tell
          the model to read that file instead of re-running.
- [x] **P3 — Research Project spine**: give the coordinator a persistent mission
      doc with *Pending Work* / *Completed Work* that survives across threads.
      Advisory next-steps + promote-to-execute + hand-editable mission/backlog.
      **DONE + MERGED: P3.1 (advisory) #19, P3.2 (promote-to-execute) #20,
      P3.3 (mission edit + user-curated backlog) #22.** Full autonomous drive
      (plan→execute→review) is deferred to **P5**; the core differentiator ships.
- [x] **P4 — Provenance edges**: every artifact links to its inputs; the
      investigation renders as one linked graph. **IMPLEMENTED (PR open, branch
      `claude/p4-provenance`).** Representation: **stable content-derived node
      ids + a separate additive `edges` slice** (chosen over a per-node
      `derived_from` field — node payloads stay untouched, and the reducer's
      `{**old,**new}` merge would clobber a per-node list on running→completed
      updates; edges accumulate/dedup independently by `(source,target,relation)`).
      Node ids come from a pure `artifact_node_id(kind, payload)` in
      `backend/schemas.py`, mirrored by `nodeId()` in `frontend/src/lib/artifacts.ts`
      and keyed to the reducer's existing dedup fields, so an edge endpoint always
      names the node the reducer keeps; papers share the `source:` namespace so a
      theory's/library's paper coincides with a separately-searched Source node.
      Edges are derived **deterministically** in `ArtifactCaptureMiddleware`
      (`backend/middleware/artifacts.py`) from each subagent's own output — never
      guessed: `hypothesis --cites/contradicted_by--> paper` (Phase 1b's PaperRefs
      made first-class — the "first edge"), `library --indexes--> paper`,
      `analysis --analyzes--> dataset file` (path match). Frontend: a dependency-free
      SVG DAG in a new **Graph** tab (`frontend/src/components/ProvenanceGraph.tsx`)
      whose nodes are self-describing (edge `*_kind`/`*_label`) and clickable
      (jump to the artifact's tab). Tests in `tests/test_provenance.py`
      (`tsc -b` + `vite build` green).
    - **Follow-up SHIPPED — lineage edges (branch `claude/p4-lineage`, stacked on
      `claude/p4-provenance`):** adds the two axes the core deferred.
      **(a) producer identity** — every subagent stamps `artifact --produced_by-->
      subagent` from `ArtifactCaptureMiddleware(source=…)` (deterministic; the
      `subagent:` node is self-describing). **(b) declared data-dependency** — a
      typed `derived_from: list[DerivedRef]` on `DataAnalysisResults` /
      `ReportWriterOutput` lets the subagent name its inputs by natural key
      (theory's research question, dataset path, paper citation); the middleware
      resolves each via `declared_ref_node_id` (same convention) into
      `analysis --tests--> hypothesis` / `report --synthesizes--> …` edges. The
      LLM never sees node ids and never invents links: declared edges are kept
      **only if their target resolves to a real artifact node**, validated at
      render time in `ArtifactPanel` (deterministic edges stay self-describing).
      Prompts for `data_voyager` / `report_writer` instruct quoting the ref
      verbatim (omit if unsure). Still-deferred: per-*theory* nodes (edges remain
      at the hypothesis-run level; individual theories have no stable id).
- [x] **P5 — Explicit Projects + opt-in autonomous run loop**: the coordinator
      runs plan → execute → review, scoped to an explicit **Project**, gated by an
      opt-in **Autopilot** toggle and human validation at each decision point.
      **IMPLEMENTED (branch `claude/mini-me-p5-autonomous-loop`, PR open; decided
      with the user 2026-07-21). 29 new backend tests (`test_plan.py` +
      `test_projects.py`), `test_project.py` adjusted for the project-scoped
      namespace; full suite 145 green; `tsc -b` + `vite build` green.**

      **(1) Explicit Projects (scope expansion, replaces P3's single per-user
      spine).** The P3 spine lived at one implicit namespace `(user_id, "project")`
      — one mission for the whole user, which is why the mission never changed
      across conversations. We adopt the ChatGPT/Claude *Projects* model instead:
      a **named Project is a container that groups many conversations**, holding
      its own mission + Completed/Pending + P5 plan; each thread belongs to a
      project and inherits it. Hierarchy: `Project (named) → many threads`.
        - Store namespaces (`backend/runtime.py`): `(user_id, "projects")` — the
          project **registry** (one item per project: `{id, name, created_at,
          updated_at}`); `(user_id, "project", project_id)` — the **spine** for
          one project (mission/completed/pending/plan); `(user_id, "threads")` —
          the **thread→project** map (item key = thread_id → `{project_id}`).
        - `backend/projects.py` (new): registry + thread-index IO and
          `resolve_active_project_id(...)` (explicit → thread-map → a lazily-created
          `"default"` project). The active project id rides on every run via
          `configurable.project_id` (frontend `buildSubmitConfigurable`), captured
          into a `_active_project_id` ContextVar in `agent()` (same pattern as the
          sandbox); coordinator middleware reads it, else falls back to default.
        - `backend/routes/projects.py` (new): `GET/POST /projects` (list/create),
          `PATCH/DELETE /projects/{id}` (rename + mission/pending/plan edits;
          delete), and `PUT /threads/{thread_id}/project` (assignment). The
          existing `/project` hand-edit route becomes project-scoped (`?project_id=`).
        - **Deferred (its own future phase):** project-level *shared knowledge
          base* (uploaded files shared across a project's chats) + project-level
          memory synthesis + sharing/collaboration. Everything else (named
          projects, project-scoped mission/spine/plan, thread assignment, a project
          switcher) is in P5.

      **(2) Autonomous run loop (plan → execute → review).** Org policy is
      human-gated — *nothing auto-runs without confirmation* — so the loop **holds
      an AI-authored plan and walks the user through it one confirmed step at a
      time**, reusing the P3.2 prefill→send gate. Not a fire-and-forget agent.
        - **Plan** (AI-authored, human accept/edit): a new **`research_planner`**
          subagent (`response_format=ResearchPlan`, no tools/sandbox) produces an
          ordered `ResearchPlan` = 3–7 `PlanStep`s, each naming a subagent + a
          routable `prompt` ("Use the <subagent> subagent to …"). Captured in
          `middleware/artifacts.py` as a `plan` artifact slice (single object,
          last-write-wins like `project`), folded into the active project's spine
          by `ProjectSpineMiddleware` and persisted. The human **accepts or edits**
          the plan before any step runs (a real decision point).
        - **Execute** (per-step, human-gated): the one **active** step shows a
          *Run* button that drops its `prompt` into the composer (existing
          `onPromote`); the user reviews and **sends** it. Never auto-submitted.
        - **Review + redirect** (after each step): the user marks the step done to
          advance, or **changes direction** — edit/insert/reorder/skip a step, or
          **re-plan** the remainder. Pure, testable state machine in
          `backend/plan.py`: `plan_from_output`, `apply_plan_edit`
          (accept/complete/skip/edit/add/remove/reorder/set-active/clear),
          `sync_plan` (exactly-one-active invariant).
        - **Gate:** an opt-in **Autopilot** toggle (localStorage, off by default →
          today's advisory suggestions unchanged). On → the ordered run-loop panel
          (`RunLoopPanel.tsx`) renders with per-step Run + review controls.
          Executing steps needs the sandbox (the execution surface).
        - Representation: `PlanStep`/`ResearchPlan` Pydantic models + payloads in
          `backend/schemas.py`; `plan` added to `ProjectState` /
          `ProjectArtifactPayload` / `ArtifactBundle`. Steps get stable ids +
          lifecycle status (`pending`/`active`/`done`/`skipped`) assigned
          deterministically after generation, so the model never reasons about them.
        - Tests: `tests/test_projects.py` (registry/thread-index/default
          resolution/per-project isolation/round-trip) + `tests/test_plan.py`
          (normalization, every edit op, invariants, payload, fold-into-project,
          nothing auto-executes); `tests/test_project.py` adjusted for the
          project-scoped namespace. Frontend `tsc -b` + `vite build`.

---

## Next work items — ready to pick up cold

These two are specced enough to start in a fresh conversation. Both build on the
now-working theorizer (poll-truncation fix + `theories/<task_id>.md` persistence
+ `CorpusID:`-correct paper links, all in PR #13).

### (B) Theorizer papers → full text in the PDF Librarian  *(Phase 2, additive, lower risk)*

> ⚠️ **SERVICE-GATED (not a code blocker):** the theorizer works; this just needs a
> theorizer run that actually produces theories, which depends on Asta's hosted
> service being healthy (it's intermittent — see the ⚠️ note in the status log).
> Build (B) whenever a trivial theorizer query `completes`; no Mini-Me change
> unblocks it. (C) shipped and never depended on the theorizer.

**Goal.** Turn a theory's *supporting/conflicting papers* (currently citation-only,
`corpusId`) into full text the agent can read and ground on — not just links.

**Why.** Each theory ships evidence papers, but the agent only has titles/abstracts.
It can't read the actual paper behind a claim, quote it in a report, or let you
semantic-search the evidence base. This is the natural theory → evidence extension.

**What exists to build on.**
- Theorizer papers already carry `corpusId` (+ title/abstract/authors) and are
  parsed into `PaperRef`s in `backend/theory_tools.py` (`_paper_ref`); the completed
  set is persisted at `theories/<task_id>.md` / `.json` in the sandbox.
- PDF Librarian subagent (`backend/subagents.py`, `skills/pdf_library/`) already
  OCRs, indexes (`asta documents add`), and semantic-searches (`asta documents search`).
- Asta CLIs available in the sandbox: `pdf-extraction`, `literature`, `papers`
  (Semantic Scholar), `documents`. **Correction (2026-07-29):** there is **no
  `asta pdf-download` command** — this note was wrong and is why (B) looked
  turnkey. PR #35 fills that gap with `skills/pdf_library/scripts/fetch_paper.py`
  (resolve ref → open-access URL via `asta papers`, fetch into `./papers/`), so
  the download step (B) needs now exists.

**Approach (proposed).**
1. Add a coordinator/subagent flow: "index the papers behind theory N" → read
   `theories/<task_id>.json`, take the `supporting_papers` corpus ids/titles, hand
   them to **`fetch_paper.py`** (open-access first; PR #35), then PDF Librarian to
   extract + index. Paywalled ones degrade gracefully (`no_oa` → skip, note it).
2. Surface indexed papers in the Library artifact (already wired) so they show in
   the Library tab and other subagents can search them.
3. Prompt: teach the coordinator that after a theorizer run the user can ask to
   "pull/read the papers behind theory N", and route to this flow.

**Acceptance.** After a theorizer run, "pull the full text of the papers behind
theory 2 and summarize what each actually found" retrieves the OA PDFs, indexes
them, and answers from full text — no fabricated content; paywalled skips noted.

### (C) Research Project spine  *(Phase 3, first coordinator upgrade — the differentiator)*

**Goal.** A persistent per-user *mission* the coordinator holds across threads,
with *Pending Work* / *Completed Work*, so Mini-Me proposes the next step instead
of being purely reactive.

**Why.** Today each thread is siloed; continuity is only cross-session `memory`
facts. The value unlock is a system that remembers the investigation and advances it.

**What exists to build on.**
- Cross-session store/memory already used (`backend/runtime.py` memory namespace;
  see `MEMORY.md` pattern). Artifacts are structured and in graph state.
- The `asta-assistant` skills (`brainstorm`, `plan-work`, `do-work`, `review-*`,
  `run`) are the *pattern* to emulate (project.md + work/<slug>/), NOT to import —
  the coordinator in `backend/subagents.py`+`prompts.py` is already the orchestrator.

**Approach (staged — start advisory).**
1. ✅ **P3.1 — Persist structured project state** per user in the store: mission +
   Pending Work / Completed Work. Seed/update it from the artifacts a thread produces.
   **DONE (PR #19)** — store namespace `("project", assistant_id, user_id)` in
   `backend/runtime.py`; state shape + reducer slice in `backend/schemas.py`.
2. ✅ **P3.1 — Advisory next steps:** at the end of a turn, surface 1–3 "suggested
   next steps" derived from current artifacts. Render in a small panel; do NOT
   auto-execute. **DONE (PR #19)** — pure logic in `backend/project.py`
   (`derive_suggestions` / `summarize_completed`), coordinator middleware in
   `backend/middleware/project.py`, `ProjectPanel` on the frontend.
3. ✅ **P3.2 — Promote to execution:** each suggestion now ships a grounded,
   routable `prompt` ("Use the <subagent> subagent to …"); clicking its
   **Run <Subagent>** button drops that prompt into the composer, focuses it, and
   puts the cursor at the end for review. It **never auto-submits** — the user
   still hits send (human-gated, org policy). **DONE (PR #20)** — `prompt`
   field added end to end (`backend/project.py`, `schemas.py`, `types.ts`,
   `lib/artifacts.ts`); prefill signal threads AppShell → ChatPanel → Composer
   (nonce-keyed so a repeat click re-fills). A future step could add the full
   plan → execute → review loop, but prefill-and-confirm is the org-safe core.
4. ✅ **P3.3 — Mission editability + user-curated backlog:** the user can now edit
   the mission inline and add / complete / dismiss Pending Work by hand. **DONE
   (PR #22).** Enablers: the project store namespace is now user-scoped
   (`(user_id, "project")`) so a stateless route can address it; `pending` is a
   *persistent user backlog* that a run no longer overwrites (auto suggestions are
   kept separate); a new `GET/PATCH /project` route (`backend/routes/project.py`)
   reads/writes the store via `langgraph_api.store.get_store()` without a run;
   the frontend edits optimistically (`lib/projectClient.ts`) and reconciles with
   the server, layering `localProject` over the graph-state project between runs.

**Acceptance (phase-1 of C).** ✅ Reopening the app shows the persistent mission with
pending/completed items; after a run, concrete, artifact-grounded next steps appear;
nothing auto-runs without the user promoting it. Depends on nothing in (B); can go
first. Pairs naturally with **P4 provenance** once artifacts link to their inputs.

**Status of the phase-1 limitations (all now addressed by P3.2/P3.3).**
- ✅ (P3.3) The mission is still auto-seeded from the first human message, but is now
  editable/resettable inline; the seed only fills an empty mission.
- ✅ (P3.3) `pending` is now an independent, user-curated backlog (add/complete/
  dismiss); runs no longer overwrite it. Auto next-steps live in `suggestions`.
- ✅ (P3.2) The `Run <Subagent>` chip is a one-click *prefill-and-confirm* that
  drops a routable prompt into the composer; it never auto-runs.
- "Completed work" counts reflect the deduped artifact bundle (plus any items the
  user marks complete by hand); they do not yet attempt provenance (that is P4).

## Open questions to investigate (observability + session UX)

Raised from LangSmith tracing review. Answer/scope these in a fresh session.

### (D) Concurrency — does the app handle multiple/async conversations?
**Short answer: yes, already.** Each conversation is a separate LangGraph thread
with its own checkpoint; `App.tsx` mounts one `ThreadStreamSession` per thread and
deliberately keeps *background* runs alive (`mountedThreadIds`, `runningThreadIds`,
`hasBackgroundRun`, per-thread `threadCommands`). Runs in different threads are
independent. LangSmith's `RAA` project aggregates **all** threads' traces into one
list — that's why you see many conversations there; it is not cross-talk.
- **To verify:** start a run in thread A, switch to thread B and start another;
  both should progress (background-run banner shows A). Confirm no shared state
  leaks (artifacts/subagents are keyed by `threadId`).
- **Observed in `RAA`:** ~23% error rate / `CancelledError` traces on "resume/
  retrieve the theory…" runs — these are the *pre-fix* theorizer resume attempts
  (LLM-driven long polls that got cancelled). Expect them to disappear now that
  the poll is a cheap status route, not an agent loop. Re-check the error rate
  after the PR #16 deploy; if `CancelledError`s persist, investigate run timeouts.

### (E) Refresh drops the "thinking" UI / some card details
**Cause (hypothesis, code-grounded):** a reload discards the in-memory `useStream`
live state; only durable state rehydrates — LangGraph checkpoint (`messages`,
`values.artifacts`, `values.todos`) plus the localStorage subagent cache
(`saveSubagentCache`/`loadSubagentCache` in `App.tsx`/`lib/threads.ts`). Ephemeral
streaming detail — reasoning/"thinking" deltas, transient subagent progress not
yet cached, live token streams — is **not** checkpointed, so it vanishes until the
next stream event. The Theories card specifically re-derives on reload by
**re-polling** the task (`useTheorizerStatus`), so it can flash empty/again-"running"
briefly.
- **Where to look:** `ThreadStreamSession.tsx` (what `useStream` exposes vs. what
  the snapshot persists), `handleSnapshotChange` in `App.tsx` (what gets cached),
  `middleware/artifacts.py` (what lands in `values.artifacts`).
- **Fix directions (pick per value/effort):** (1) persist reasoning/thinking and
  in-progress subagent state into the snapshot cache so a mid-run reload restores
  them; (2) write completed theories into graph state (not just the sandbox file)
  so the card rehydrates without a re-poll; (3) show a clear "reconnecting…"
  placeholder instead of a blank flash while the stream re-attaches.
- **Note:** ties into work item (C) — a durable project/session state would make
  reloads lossless by design.

## Guardrails (org policy alignment)

- Reviewer-in-the-loop is a **feature**, not a limitation: every autonomous step
  is SME-reviewable.
- AI use disclosed on artifacts; data stays in the sandbox.

## Phase 6 — Zed-like Rust desktop workbench  *(next major initiative — KICKOFF 2026-07-29)*

**Goal.** A native **desktop** research-acceleration workbench, inspired by Zed
(`zed-industries/zed`), that lifts the flexibility ceiling of the browser app:
local filesystem + native file dialogs, long-running/background agent jobs as
first-class OS processes, offline, OS-keychain secrets, multi-window, and a
fast, keyboard-driven, multi-pane UX. UX bar = Claude/OpenAI desktop apps.

**Why now.** The user feels constrained by the web app. Several standing pains
are desktop-shaped: the Asta static-token expiry (the local `asta` CLI
auto-refreshes; a **local** backend would inherit that and end the pain — see the
#33 status entry), file uploads vs. direct local paths, and background run
management. None of these are UI-toolkit problems — they are "be a real desktop
app" problems.

**Direction chosen (with eyes open).** Three options were weighed:
- **Fork Zed** — it is a GPL code editor built around text buffers/LSP; wrong
  shape, viral license. Rejected.
- **Tauri shell + reuse the React frontend** (backend as a local sidecar) — my
  recommendation: keeps the existing UX and Python backend, lowest cost/risk, and
  is what Claude/OpenAI desktop apps effectively do (webview in a native shell).
  **Kept as the flagged fallback.**
- **GPUI rewrite** (Zed's Apache-2.0 GPU UI framework, used standalone) — a fresh
  Rust frontend. **This is the direction the user chose.** Highest cost/risk
  (rewrite the whole frontend in sparsely-documented Rust; agents still run as a
  Python/TS subprocess), but the "copy the best from Zed" goal and native feel are
  the payoff. Proceed honestly about the trade-off; do not silently drift to Tauri.

**Non-negotiables carried in.** Agents stay in **Python/TS** (the coordinator +
subagents + skills are the product; they run as a sidecar/subprocess, spoken to
over the existing HTTP/stream protocol — the desktop app is a *client*, not a
reimplementation of the agent stack). Org policy stays **human-gated**. Additive:
the web frontend keeps working; the desktop app is a new client, likely in a new
top-level `desktop/` crate (or a sibling repo — see open decisions).

**Staged approach (proposed — confirm before building each stage).**
1. **P6.0 — Spike doc + risk burndown (docs only).** A dedicated
   `docs/desktop-app-plan.md`: GPUI viability (rendering the core surfaces —
   streaming chat, the artifacts/Outputs panel, project spine, plan panel — in
   GPUI), crate/workspace layout, how the Rust client speaks to the Python
   backend (spawn + supervise a local sidecar vs. attach to the hosted backend;
   auth; streaming), secrets in the OS keychain, build/distribution
   (macOS/Linux/Windows), and a milestone plan with kill-criteria. **First
   deliverable of Phase 6.**
2. **P6.1 — GPUI "hello workbench" spike.** Minimal GPUI app: one window, a
   command palette, and a chat pane that streams a hard-coded response — proves
   the framework can carry the core interaction before committing to the rewrite.
3. **P6.2 — Talk to the real backend.** Spawn the Python backend as a local
   sidecar (or attach to hosted), stream a real coordinator turn end to end,
   render assistant text. Validates the client/agent boundary.
4. **P6.3 — Port the core panels.** Artifacts/Outputs, project spine (mission +
   completed/pending), and the plan/Autopilot panel — the workbench identity.
5. **P6.4 — Native affordances.** Local file open → sandbox path, background-run
   tray/notifications, keychain-stored Asta/model keys, multi-window.

**Open decisions (resolve in P6.0 before scaffolding).**
- **Repo shape:** new top-level `desktop/` in this monorepo, or a separate repo?
- **Backend locality:** bundle + run the Python backend locally (kills the token
  pain, enables offline) vs. attach to the deployed LangSmith backend (less to
  ship) vs. support both.
- **GPUI dependency:** pin `gpui` as a standalone crate (API is unstable) — decide
  the version/vendoring strategy and a fallback if it blocks.
- **Team Rust capacity:** the rewrite needs sustained Rust work; confirm before
  P6.1.

**Acceptance (Phase 6, MVP).** A downloadable desktop app that opens a project,
runs a real coordinator turn against the (local or hosted) backend, streams the
answer, and renders the artifacts/spine panels — with at least one native
affordance the web app cannot do (local file → analysis, or background-run
notification).

## Sequencing summary

1. ✅ Phase 1a (collapsible) — trivial, independent.
2. ✅ Phase 1b (clickable refs) — full stack, plus one theorizer-artifact inspection.
3. ✅ Phase 2 (DataVoyager) — additive subagent. **IMPLEMENTED (PR #25, open).**
   Also powers the spine's "test your theories against data" suggestion with a
   real loop (repointed from the Diagnostic Analytics proxy).
4. ✅ Phase 3 (project spine) — first coordinator upgrade; the value unlock.
   Shipped across #19 (advisory) / #20 (promote) / #22 (hand-edit).
5. ✅ Phase 4 (provenance graph) — IMPLEMENTED (PR open, `claude/p4-provenance`):
   stable node ids + an additive `edges` slice; deterministic theory→paper,
   library→paper, analysis→dataset edges; a clickable SVG DAG in a new Graph tab.
   Semantic analysis→theory / report→inputs edges deferred (LLM-declared) —
   makes "Completed work" traceable but not yet fully.
6. ✅ Phase 5 — explicit Projects + the opt-in autonomous run loop
   (plan → execute → review). **MERGED (#30/#32).** Two parts: (1) named
   **Projects** that group conversations (ChatGPT/Claude model), replacing P3's
   single per-user spine — each project scopes its own mission/spine/plan; (2) an
   AI-authored, human-accepted **run-loop plan** walked one confirmed step at a
   time behind an opt-in **Autopilot** toggle. Shared project knowledge-base/files
   deferred to a later phase. Full design in the P5 roadmap entry above.
7. 🩹 Correctness fixes on the current stack — **OPEN.** #34 injects the project
   mission into the coordinator's model context (it was display-only); #35 adds
   open-access PDF download to the PDF librarian (the `asta` CLI has no download
   command). Both additive, both awaiting SME review + redeploy.
8. 🔨 Phase 6 — **Zed-like Rust desktop workbench.** KICKOFF (2026-07-29); the
   next major initiative. Native desktop research workbench inspired by Zed/GPUI.
   User chose the GPUI-rewrite direction (Tauri is the flagged fallback). Agents
   stay in Python/TS as a sidecar. See the **Phase 6** section above for the
   staged plan and open decisions.

## Reference from Asta

https://github.com/allenai/asta-plugins/blob/main/README.md