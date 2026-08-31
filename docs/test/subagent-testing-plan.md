# Subagent Testing Plan

A semi-step-by-step guide for manually verifying that each of Mini-Me's 12
specialist subagents does what it claims, using one shared mock scenario so
results are comparable across agents. Written against
`mini-me/backend/subagents.py` (2026-08-31) — re-read that file if this drifts.

This is a manual QA checklist run through the desktop app's chat composer,
not an automated test suite. Each subagent's own automated tests live in
`mini-me/tests/test_*.py`; this plan is for exercising the whole coordinator
+ subagent path end to end, the way a researcher actually would.

## 1. Prerequisites

- [ ] Backend installed and passing (Setup pane fully green — see
      `scripts/setup-backend.ps1` / `.sh`).
- [ ] At least one model provider key set in **Settings**.
- [ ] `ASTA_TOKEN` valid (Setup pane → "Asta CLI" row shows a valid, entitled
      account) — every agent below except `data_cleaning`, `exploratory_data_analysis`,
      `diagnostic_analytics`, `predictive_analytics`, and `report_writer` calls
      out to Asta.
- [ ] "Run code on this machine" (host execution) turned on in Settings, so
      `execute` calls actually run instead of hitting the remote sandbox.
- [ ] A fresh conversation for each test unless a step says otherwise —
      DataVoyager in particular reasons over a session's whole history, so a
      stale conversation will contaminate results (see §5.10).

## 2. The shared test scenario

Using one topic across agents makes it obvious when an agent invents
something instead of grounding it in what earlier steps actually produced.

**Topic:** *Does soil organic carbon (SOC) affect potato tuber yield across
smallholder plots in the Peruvian highlands, and does irrigation change that
relationship?* — on-domain for CIP, small enough to reason about by eye,
open enough that literature, a dataset, cleaning issues, and modeling all
have something real to do.

### 2.1 Mock dataset

Save this as `potato_soc_yield.csv` and attach it to the conversation (drag
onto the window, or the composer's paperclip) before the data-oriented
tests. It is deliberately dirty — that dirtiness *is* the test fixture for
data cleaning and EDA.

```csv
plot_id,region,soc_pct,irrigation,rainfall_mm,temp_c,yield_t_ha,survey_date
P001,Huancavelica,2.1,yes,620,14.2,18.4,2024-11-03
P002,Huancavelica,1.8,Yes,590,14.5,16.9,2024-11-03
P003,Huancavelica,,no,610,14.1,15.2,2024-11-04
P004,Ayacucho,2.4,no,480,15.8,14.1,2024-11-05
P005,Ayacucho,2.6,Y,470,15.9,19.8,2024-11-05
P006,Ayacucho,2.5,no,490,16.0,13.7,2024-11-06
P007,Cusco,3.1,yes,710,13.4,22.6,2024-11-07
P008,Cusco,3.0,yes,700,13.2,999.0,2024-11-07
P009,Cusco,2.9,YES,690,13.6,21.9,2024-11-08
P010,Puno,1.5,no,350,16.9,9.8,2024-11-09
P011,Puno,1.4,no,340,17.1,10.1,2024-11-09
P011,Puno,1.4,no,340,17.1,10.1,2024-11-09
P012,Puno,-0.3,no,360,16.8,10.4,2024-11-10
P013,Apurimac,2.2,no,,15.1,15.5,2024-11-11
P014,Apurimac,2.3,yes,540,14.9,17.3,not_a_date
P015,Apurimac,2.2,yes,530,15.0,17.0,2024-11-12
```

Known defects to expect an agent to catch (used as the pass/fail rubric in
§4 and §5):

| # | Issue | Where |
|---|---|---|
| 1 | Missing value | `soc_pct` row P003, `rainfall_mm` row P013 |
| 2 | Inconsistent categorical encoding | `irrigation`: `yes`/`Yes`/`Y`/`YES`/`no` |
| 3 | Exact duplicate row | `P011` appears twice |
| 4 | Physically impossible value (outlier) | `yield_t_ha` = 999.0 for P008 |
| 5 | Physically impossible value | `soc_pct` = -0.3 for P012 (organic carbon can't be negative) |
| 6 | Malformed date | `survey_date` = `not_a_date` for P014 |

### 2.2 Test PDF (for the PDF librarian)

Any short agronomy-adjacent PDF works — e.g. export a 2-page PDF from a
public FAO or CIP factsheet on soil organic carbon or potato agronomy, or
use any PDF already on hand. Name it `soc_factsheet.pdf` and attach it
alongside the CSV for §5.9.

## 3. What "passing" means, in general

For every agent below, in addition to the agent-specific checks:

- **No fabrication.** Every number, citation, dataset ID, or file path in
  the reply must trace back to something a tool actually returned. If you
  can't find the source of a claim in the tool-call trace, that's a fail —
  file it as a `simplify`/correctness issue against the agent's prompt or
  its guard middleware (`SearchBeforeCiting`, `KeepSources`,
  `SearchBeforeRecommending`, `RunBeforeReporting`, `TheorizeBeforeReporting`,
  `SubmitBeforeReporting`, `DraftBeforeReporting` — see
  `mini-me/backend/middleware/`).
- **Files land where the app can see them.** Anything the agent claims to
  have written should show up in the **Outputs** panel for that
  conversation, not just in prose.
- **No silent spend.** Any subagent invoking `execute` should have stopped
  at an approval interrupt first (`NoSpendingWithoutApproval` — every
  subagent carries this, not just AutoDiscovery). If code ran without you
  clicking Approve, that's a hard fail.
- **Structured response matches the schema.** Open the raw event/JSON view
  (or the corresponding card in the UI) and check the fields named in each
  section against `mini-me/backend/schemas.py`. A subagent that "answers
  correctly" but returns the fields empty or as an unstructured chat message
  is still a fail — the frontend renders off the structured response, not
  the prose.

## 4. How to run one isolated test

To test a single subagent in isolation (rather than letting the coordinator
decide), address it directly — this is also the phrasing the
`research_planner` uses internally, so it's guaranteed to route:

> Use the `<subagent_name>` subagent to \<task>.

e.g. *"Use the `data_cleaning` subagent to clean `potato_soc_yield.csv` and
report every issue found."*

Then, for each test:
1. Send the prompt.
2. Watch the **agent activity trace** — confirm only the intended subagent
   ran (not the coordinator quietly also invoking another one).
3. Open **Outputs** and check any files the checklist below expects.
4. Check the structured response fields.
5. Log the result in the table in §7.

---

## 5. Per-subagent tests

### 5.1 `academic_researcher`

**Does:** Asta literature search and synthesis, cited with APA references.

**Prompt:**
> Use the `academic_researcher` subagent to find recent evidence on how soil
> organic carbon affects potato tuber yield in Andean/highland smallholder
> systems.

**Checks:**
- [ ] At least one Asta search tool call happened *before* any claim (this
  is enforced by `SearchBeforeCiting` — if the first assistant text with a
  citation appears with zero prior tool calls, that's a middleware bug, not
  a model quirk worth re-prompting around).
- [ ] `AcademicResearchResults.sources[]` is non-empty, each with `citation`
  and `relevance` filled in verbatim from what the search tool returned
  (spot-check one citation's DOI/year against the raw tool output).
- [ ] Every paper the search actually returned appears in `sources` — the
  prompt explicitly forbids dropping "marginal" ones silently.
- [ ] `KeepSources` wrote the raw search results to a file in the sandbox —
  confirm it's visible in Outputs.

### 5.2 `dataverse_explorer`

**Does:** Searches CIP Dataverse, recommends datasets — read-only, no
downloads or curation.

**Prompt:**
> Use the `dataverse_explorer` subagent to find datasets on potato yield or
> soil properties in Peru.

**Checks:**
- [ ] A dataverse search tool ran before any dataset was recommended
  (`SearchBeforeRecommending`).
- [ ] `DataVerseSearchResults.datasets[]` entries have a `persistent_id`
  that matches character-for-character something the search actually
  returned — this is the one field the prompt calls out as never to be
  reconstructed from memory.
- [ ] The search results file exists in Outputs (`SearchResultsFile`).
- [ ] The agent did **not** attempt to download a dataset file or perform
  any Dataverse curation action (out of scope per its own prompt).

### 5.3 `data_cleaning`

**Does:** Cleaning/validation plan and execution via pointblank / agrovoc /
crop-ontology MCP tools. No structured `response_format` — check the prose
and the files.

**Prompt:**
> Use the `data_cleaning` subagent to validate and clean
> `potato_soc_yield.csv`. Report every issue found and what you did about
> it.

**Checks (cross-reference §2.1's defect table):**
- [ ] Reports the missing values in `soc_pct` and `rainfall_mm`.
- [ ] Normalizes or at least flags the inconsistent `irrigation` encoding
  (`yes`/`Yes`/`Y`/`YES`/`no`).
- [ ] Catches the exact duplicate row (`P011`).
- [ ] Flags the impossible `yield_t_ha` = 999.0 outlier.
- [ ] Flags the impossible negative `soc_pct` (-0.3).
- [ ] Flags or repairs the malformed `survey_date` (`not_a_date`).
- [ ] The **original** `potato_soc_yield.csv` is untouched — cleaned output
  is a **new, versioned file** (e.g. `potato_soc_yield_clean.csv`), per the
  agent's own "never overwrite existing files" instruction. Verify this by
  re-opening the original attachment and diffing row count/content.
- [ ] The reply is a summary (cleaned-dataset shape, issues found, actions
  taken) — not a full dump of every row.

### 5.4 `exploratory_data_analysis` (EDA)

**Does:** Profiling, descriptive stats, missingness, outliers, correlations,
visualizations. No structured `response_format`.

**Prompt (run *after* 5.3, pointing at the cleaned file this time):**
> Use the `exploratory_data_analysis` subagent to explore
> `potato_soc_yield_clean.csv` and summarize what you find about the
> relationship between soil organic carbon and yield.

**Checks:**
- [ ] Produces at least one chart file (e.g. a scatter of `soc_pct` vs
  `yield_t_ha`, or a distribution plot), visible in Outputs.
- [ ] Reports descriptive stats (mean/median/range) for the key numeric
  columns.
- [ ] Calls out the SOC–yield relationship directionally (positive
  correlation is the honest read of the mock data) without overclaiming
  causality — EDA answers "what happened," not "why."
- [ ] Notes any remaining data-quality concern it finds (e.g. small `n`,
  regional imbalance) rather than treating the cleaned file as flawless.
- [ ] Reply is a short synthesis + key visuals, not a raw profiling dump.

### 5.5 `diagnostic_analytics`

**Does:** "Why did it happen" — inference techniques. Carries the only
`interrupt_on` tool in the file: `request_diagnostic_context`.

**Prompt (deliberately vague, to trigger the interrupt):**
> Use the `diagnostic_analytics` subagent to figure out why yield is lower
> in Puno than the other regions.

**Checks:**
- [ ] The run **stops at an interrupt** asking you to fill in
  `research_question_or_goal`, `primary_hypothesis`, `outcome_variable`,
  `candidate_drivers`, `unit_of_analysis`, `time_window`,
  `candidate_confounders`, `causal_expectation` — confirm the interrupt UI
  actually renders these fields and lets you `approve`/`edit`/`reject`.
- [ ] After you **edit and approve** (e.g. outcome = `yield_t_ha`, driver =
  `soc_pct`/`region`, confounder = `irrigation`, causal_expectation =
  "associative, not causal — observational data"), the run resumes using
  what you supplied rather than re-asking or ignoring it.
- [ ] After **reject**, confirm the subagent stops cleanly rather than
  running anyway or crashing the turn.
- [ ] The reply states its method (e.g. group comparison, regression
  controlling for irrigation) and is honest about what an n=15 mock dataset
  can and can't support causally.

### 5.6 `predictive_analytics`

**Does:** ML/statistical/Bayesian modeling — "what will happen."

**Prompt:**
> Use the `predictive_analytics` subagent to build a model predicting
> `yield_t_ha` from `soc_pct`, `irrigation`, `rainfall_mm`, and `temp_c` in
> `potato_soc_yield_clean.csv`, and report performance.

**Checks:**
- [ ] Reports at least one performance metric (R², RMSE, or equivalent) —
  not just "the model was trained."
- [ ] Reasoning for method choice is stated (dataset this small/noisy should
  produce a simple model and honest uncertainty, not a claimed
  state-of-the-art result).
- [ ] **Quiet-output rule:** the chat reply is a human-readable summary
  (a few thousand characters at most) — no raw PyMC progress bars,
  per-epoch training logs, or sklearn verbose spam leaking into the
  message. If you see that, it's a real regression against the prompt's
  explicit "Quiet-output rule" section.
- [ ] Any noisy log output, if produced, went to a file (e.g.
  `pymc_trace.log`) rather than the chat.

### 5.7 `hypothesis_generator`

**Does:** Runs the Asta Theorizer (`asta generate-theories`) — async,
background, **submits and returns immediately**.

**Prompt:**
> Use the `hypothesis_generator` subagent to generate theories for: does
> soil organic carbon affect potato tuber yield in Andean highland systems?

**Checks:**
- [ ] The tool call is `generate_theories(...)`, never a raw `asta` CLI
  invocation via `execute`, and never a polling loop (`TheorizeBeforeReporting`
  enforces "ran before reporting," not "waited for completion").
- [ ] First reply has `HypothesisOutput.status = "running"`, a real
  `task_id`, empty `theories`, and tells you it'll appear in the Theories
  panel in 5–15 minutes — **not** presented as a failure or "no results."
- [ ] Confirm the run does **not** set `do_novelty=True` unless you
  explicitly asked for a novelty evaluation and accepted the 30–60 minute
  wait.
- [ ] Come back later (or ask "check on that theory run" with the same
  `task_id`) and confirm `status="completed"` carries over every theory's
  `laws`, `supporting_papers`, `conflicting_papers` unmodified, plus
  `knowledge_gaps` and `papers_reviewed`.
- [ ] If the run legitimately fails, confirm `status="failed"` with a real
  reason in `knowledge_gaps` — not fabricated theories.

### 5.8 `data_voyager`

**Does:** Runs Asta DataVoyager (`asta analyze-data`) against a real
tabular dataset — async, `SubmitBeforeReporting` guarded.

**Prompt (note all four rules from its own system prompt — name datasets,
name methods, ask for numbers, say "run it"):**
> Use the `data_voyager` subagent to train and evaluate regression models
> predicting `yield_t_ha` from the numeric covariates in
> `potato_soc_yield_clean.csv`. Fit and compare at least two candidate
> models with cross-validation, report R² and RMSE, name the most important
> predictor, and produce a chart. Actually run the code and report the
> numbers.

**Checks:**
- [ ] Started with **no `context_id`** (fresh session) since this is a new
  analysis, not a continuation.
- [ ] First reply: `DataAnalysisResults.status="running"` with real
  `task_id`/`context_id`, empty `findings` — not presented as failure.
- [ ] On a later explicit check (same `task_id`/`context_id`):
  `status="completed"` with concrete `findings[]` (each with `chart_path`
  when a chart exists), `hypotheses_tested`, and `charts`.
- [ ] Follow-up test: ask a continuation question in the **same**
  conversation ("now also check whether irrigation changes that
  relationship") and confirm this time `context_id` **is** reused.
- [ ] Separately, confirm that asking about a *different, unrelated*
  question in a fresh conversation does **not** carry over the old
  `context_id` (this was a documented failure mode — DataVoyager reasoning
  over unrelated prior history in the same session).

### 5.9 `pdf_librarian`

**Does:** OCR/index/search a persistent local PDF library via the `asta`
CLI. `RunBeforeReporting` guarded.

**Prompt (with `soc_factsheet.pdf` attached, §2.2):**
> Use the `pdf_librarian` subagent to index `soc_factsheet.pdf` into the
> library with a good summary, then search the library for "soil organic
> carbon".

**Checks:**
- [ ] Confirms the PDF is actually on disk before claiming anything was
  extracted (never claims to have read a paper with no local file).
- [ ] `LibraryArtifact.action` reflects what actually happened (`"index"`
  then effectively a search), `paper_count` increments correctly,
  `index_path` stays at the default `.asta/documents` (no `--root`
  override, nothing written to `/data` or `/tmp`).
- [ ] The indexed document's summary is content-derived (mentions something
  specific from the PDF), not a generic placeholder.
- [ ] Search returns the just-indexed document with a real relevance score
  when queried for "soil organic carbon."
- [ ] Regression check: ask it to index a **URL-only** reference (no
  attached file) and confirm it does *not* claim extraction — recording a
  URL is not the same as fetching and OCR'ing the PDF.

### 5.10 `autodiscovery`

**Does:** Prepares (drafts only) an Asta AutoDiscovery run. **Cannot start
one** — this is enforced, not a limitation to route around.

**Prompt:**
> Use the `autodiscovery` subagent to explore what's in
> `potato_soc_yield_clean.csv` — I don't have a specific hypothesis, I want
> to see what the data itself suggests.

**Checks:**
- [ ] Tool call is `draft_discovery_run` only — never `execute`, never a
  direct `asta` CLI submission.
- [ ] `description`, `domain`, `intent`, `dataset_description` are all
  filled in with real content (not boilerplate) — `description` should
  reflect the file's actual header row and row count, `domain` something
  like "soil science / agronomy," `intent` steers without stating an
  answer (e.g. "focus on how soil and climate variables relate to yield" —
  **not** "temperature above X reduces yield by Y%").
- [ ] `DiscoveryRunResults.status = "awaiting_approval"` with a real
  `run_id`; the reply says plainly that **nothing has run and nothing has
  been spent**, and that it's waiting on you to approve the budget in the
  app.
- [ ] **Hard gate check:** confirm no experiment result, finding, or belief
  shift appears anywhere in this reply — there can't be any yet.
- [ ] If you *do* approve the budget in the UI, come back later and confirm
  results eventually populate from `discovery/<run_id>.md`/`.json` rather
  than being re-fabricated by the model.
- [ ] Negative test: try passing an absolute Windows/host path (e.g.
  `C:\Users\...\potato_soc_yield_clean.csv`) instead of the relative
  attached-file path and confirm the agent either resolves it via the
  filename or clearly tells you the file isn't visible to the run — not a
  silent failure.

### 5.11 `report_writer`

**Does:** Synthesizes prior findings into a full markdown report.
`ArtifactCaptureMiddleware` only — no run-before-reporting guard, since it
has nothing of its own to run.

**Prompt (run after at least 5.3–5.6 in the same conversation so there's
real prior work to synthesize):**
> Use the `report_writer` subagent to write a report on the SOC–yield
> investigation, including the cleaning, EDA, and modeling results so far.

**Checks:**
- [ ] `ReportWriterOutput.markdown` contains the **full** report body
  (sections, findings, any table, image references, code appendix) — not a
  short summary and not a "see the chat above" placeholder. This is called
  out explicitly in the prompt as the #1 failure mode to watch for.
- [ ] Any image reference (e.g. `![...](./eda_distributions.png)`) points
  at a file that actually exists in the sandbox/Outputs — open the PDF/Report
  card and confirm the image actually renders, not a broken link.
- [ ] Chat message and structured response are **both** present and
  distinct — chat isn't just "I saved a report."
- [ ] `derived_from` lists the earlier steps this report actually draws on
  (analyses/datasets/hypotheses), each `ref` quoted verbatim against the
  originating artifact. If it can't quote one exactly, that entry should be
  **omitted**, not guessed.
- [ ] No invented results — every number in the report should trace to an
  earlier subagent's structured output in this same conversation.

### 5.12 `research_planner`

**Does:** Produces a 3–7 step `ResearchPlan` for the P5 autonomous run
loop. **Runs nothing** — planning only.

**Prompt:**
> Use the `research_planner` subagent to plan the investigation into
> whether soil organic carbon affects potato yield, given that I already
> have `potato_soc_yield.csv`.

**Checks:**
- [ ] `ResearchPlan.goal` restates the mission in one line.
- [ ] `steps` has 3–7 entries, each single-subagent, sequenced sensibly:
  evidence/data work (cleaning, EDA, maybe literature) before synthesis
  (diagnostic/predictive/hypothesis/DataVoyager) before **report_writer
  last**.
- [ ] Does **not** propose a step for work you've already told it is done
  (since the prompt says "I already have the CSV," it should not propose a
  dataverse-search or upload step for that same data).
- [ ] Each step's `action` uses one of the exact friendly labels ('Data
  Cleaning', 'Exploratory Data Analysis', 'Diagnostic Analytics',
  'Predictive Analytics', 'Report Writer', etc.) and `prompt` uses the
  snake_case subagent name in "Use the `<name>` subagent to …" phrasing —
  confirm a step's `prompt`, pasted into a fresh message, actually routes
  to the subagent it names (this is the coupling that makes the whole
  planner useful — verify it, don't assume it).
- [ ] Confirm **nothing actually ran** — no subagent besides
  `research_planner` appears in the activity trace for this turn.

---

## 6. Pipeline tests (chained / automatic)

Individual `Use the X subagent` prompts isolate one agent at a time. These
tests instead give the **coordinator** a single, natural request and check
whether it correctly chains multiple subagents *on its own*, in the right
order, within one turn (or one guided conversation) — which is the
realistic way a researcher will actually use the app.

Not every agent chains automatically. From the code:

- **Synchronous agents** (`academic_researcher`, `dataverse_explorer`,
  `data_cleaning`, `exploratory_data_analysis`, `diagnostic_analytics`,
  `predictive_analytics`, `report_writer`) can be legitimately chained by
  the coordinator inside a single turn, since each finishes before the
  next needs its output.
- **Async agents** (`hypothesis_generator`, `data_voyager`) submit and
  return immediately — the coordinator **cannot** chain a next step off
  their output in the same turn, because the result doesn't exist yet. A
  correct pipeline treats these as a checkpoint: the coordinator should
  tell you it's running and wait for you to come back, not fabricate a
  "next step" against results that don't exist.
- **`autodiscovery`** is a hard stop behind a credit-approval gate — no
  pipeline should ever assume it continues past the draft step
  automatically.
- **`research_planner`** produces a plan for *you* to run step by step
  (P5) — it is not itself a link in an automatic chain.

### 6.1 Pipeline A — the data pipeline (should auto-chain)

**Prompt (single message, fresh conversation, `potato_soc_yield.csv`
attached):**
> I have `potato_soc_yield.csv` with potato yield and soil data from four
> regions. Clean it, explore it, and tell me whether soil organic carbon
> predicts yield — then write me a short report.

**Expected chain:** `data_cleaning` → `exploratory_data_analysis` →
`diagnostic_analytics` and/or `predictive_analytics` → `report_writer`,
all within the conversation, without you re-prompting between steps.

**Checks:**
- [ ] Activity trace shows the subagents firing in that dependency order —
  cleaning strictly before EDA/modeling, and `report_writer` strictly
  last.
- [ ] The EDA step reads the **cleaned** file, not the original — verify by
  checking which filename its tool calls reference.
- [ ] The final report's `derived_from` correctly references the
  intermediate analyses, meaning the coordinator actually passed context
  forward rather than each subagent working in isolation.
- [ ] No step re-does work an earlier step in the same turn already did
  (e.g. EDA re-profiling issues `data_cleaning` already fixed and reported).

### 6.2 Pipeline B — the literature-to-evidence pipeline (partially async)

**Prompt (fresh conversation):**
> Find literature on whether soil organic carbon affects potato yield,
> generate testable theories from it, and once I have `potato_soc_yield.csv`
> attached, test the most promising one against the data.

**Expected chain:** `academic_researcher` → `hypothesis_generator`
(submits, **stops** — background) → *(you wait / come back)* → once
theories complete, a follow-up turns into `data_voyager` (submits,
**stops** again) → *(you wait / come back)* → `report_writer` once both are
done.

**Checks:**
- [ ] The coordinator does **not** try to fake `data_voyager` results in
  the same turn `hypothesis_generator` was just submitted — confirm it
  correctly tells you theories are generating and that testing them against
  data is a next step once they land.
- [ ] When you return and ask it to continue, it reads the completed
  `HypothesisOutput` (from the Theories panel / `derived_from` reference)
  rather than re-deriving hypotheses from scratch.
- [ ] `data_voyager`'s question, when the coordinator writes it for you,
  actually follows the four rules from §5.8 (names the dataset, names
  methods, asks for numbers, says "run it") — a coordinator-authored
  question is just as capable of degrading into a vague one as a
  human-written one.
- [ ] Final `report_writer` output's `derived_from` links both the
  hypothesis and the analysis by their exact `ref` strings.

### 6.3 Pipeline C — planner-driven, one confirmed step at a time (P5)

**Prompt:**
> Use the `research_planner` subagent to plan the SOC–yield investigation
> for `potato_soc_yield.csv`, then let's run it step by step.

**Checks:**
- [ ] The plan appears for review; confirm you can **edit** a step's prompt
  before running it and the edit takes effect (not silently reverted to the
  planner's original wording).
- [ ] Each step only runs when you explicitly confirm it — the app never
  auto-advances through the whole plan unattended.
- [ ] Step statuses (`pending` → `active` → `done`/`skipped`) update
  correctly in the UI as you go.
- [ ] If you **skip** a step (e.g. skip a redundant literature search), the
  later report step doesn't claim to have used it.

---

## 7. Results log

Copy this table when you run a pass; one row per subagent/pipeline test,
dated.

| Date | Test | Result (✅/❌) | Notes / filed issue |
|---|---|---|---|
| | 5.1 academic_researcher | | |
| | 5.2 dataverse_explorer | | |
| | 5.3 data_cleaning | | |
| | 5.4 exploratory_data_analysis | | |
| | 5.5 diagnostic_analytics | | |
| | 5.6 predictive_analytics | | |
| | 5.7 hypothesis_generator | | |
| | 5.8 data_voyager | | |
| | 5.9 pdf_librarian | | |
| | 5.10 autodiscovery | | |
| | 5.11 report_writer | | |
| | 5.12 research_planner | | |
| | 6.1 Pipeline A (data) | | |
| | 6.2 Pipeline B (literature→evidence) | | |
| | 6.3 Pipeline C (planner-driven) | | |

## 8. If something fails

- A subagent fabricating a claim, citation, ID, or file path → check
  whether its guard middleware actually fired (§3 "No fabrication") before
  assuming it's a prompt-wording problem.
- A subagent producing output but nothing shows in Outputs → check
  `DISK_WRITING_SUBAGENTS` in `subagents.py` and the relevant
  `FileSyncMiddleware`/`ArtifactCaptureMiddleware` wiring, not the model.
- `execute` running without an approval prompt → this is a
  `NoSpendingWithoutApproval` regression; treat as a stop-the-line bug, not
  a flaky test.
- An async agent (`hypothesis_generator`, `data_voyager`) reported as
  "failed" when it was actually still running → re-check the exact
  `status` string the tool returned before concluding the agent
  mishandled it; these two are explicitly forbidden from presenting
  "running" as failure.
