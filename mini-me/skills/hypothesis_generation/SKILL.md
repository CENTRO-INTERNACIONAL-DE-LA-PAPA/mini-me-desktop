---
name: hypothesis_generation
description: >-
  Use the Asta Theorizer to generate literature-grounded scientific theories and
  hypotheses for a research question. Use when the task is to explain a phenomenon,
  generate candidate mechanisms, or synthesize what the literature proposes about
  a causal or mechanistic question.
---

# Hypothesis Generation Guidelines

Use this skill when the goal is to produce candidate scientific theories or
hypotheses for a research question, grounded in the actual literature rather
than free-form speculation.

This skill is **tool-first**. It uses the `generate_theories` tool, which runs
the Asta Theorizer end-to-end (find papers → extract evidence → write theories →
score novelty) and returns finished, structured theories. You do **not** call the
`asta` CLI directly and you do **not** poll with `execute` — the tool owns the
whole long-running job.

## When to use this skill

Use when the user asks:

- "What theories/hypotheses explain X?"
- "What are the proposed mechanisms for Y?"
- "Generate hypotheses about Z"
- "What does the literature propose about why A causes B?"

## Boundaries

This skill does **not** own:

- plain literature search or evidence synthesis (use `academic_researcher`)
- analysis of a local dataset (use the EDA / diagnostic / predictive subagents)
- final report writing (use `report_writer`)

If the task is to *find and summarize papers* rather than *propose theories*,
hand off to `academic_researcher`. If it becomes about a local dataset, hand off
to the analysis subagents.

## Tool: `generate_theories`

The theorizer is asynchronous and slow — **5–15 minutes** (paper search →
per-paper extraction → theory synthesis → self-reflection). The
`generate_theories` tool hides that: it submits the run, waits for it by polling
internally, and returns the finished theories already parsed into the
`HypothesisOutput` shape (each theory's `laws`, `supporting_papers` and
`conflicting_papers` — every paper with a `citation` and a resolvable `url` when
one exists — plus `knowledge_gaps` and `papers_reviewed`).

Parameters:

- `question` — the research question (required on the first call).
- `resume_task_id` — an existing task id to keep waiting on (used to resume).
- `max_papers` — papers to retrieve (default 30; 20–30 keeps runs faster).
- `do_novelty` — run qualified-novelty evaluation (adds 30–60 min; default off).
  Only pass `do_novelty=True` when the user explicitly asks for novelty scores.

### Flow

- **Start**: call `generate_theories(question=<the research question>)`. The tool
  submits the run and returns right away — it does not wait for the theories.
- **Check** (only if the user explicitly asks about a specific run): call
  `generate_theories(resume_task_id=<that task id>)`.

Set the `HypothesisOutput.status` field to match the tool result:

1. `"running"` → the run was submitted and is generating in the background.
   Return a `HypothesisOutput` with the user's `question`, `status="running"`,
   the `task_id`, and empty `theories`. Tell the user their theories are being
   generated and will appear in the Theories panel automatically. Do not call the
   tool again.
2. `"completed"` → build the `HypothesisOutput` from the result: copy `theories`
   (keep every paper's link), `knowledge_gaps`, and `papers_reviewed`; set
   `question`; set `status="completed"`.
3. `"failed"`/`"canceled"`/`"error"` → `HypothesisOutput` with the user's
   question, empty `theories`, `status="failed"`, and the reason in
   `knowledge_gaps`.

Leave `do_novelty` off (the default). Only set it when the user explicitly asks
for a novelty evaluation and accepts a 30–60 minute run — a question that mentions
"novelty" or asks for a novelty score does not count.

### The one rule that matters

**Never present a running run as a failure or as "no theories".** It is still
generating and the panel fills in on its own. Never fabricate theories or
citations.

## Expected output

A structured `HypothesisOutput`:

- `question` — the research question addressed
- `theories` — each with `laws`, `supporting_papers`, `conflicting_papers` (each
  paper a `PaperRef` with `citation` and, when resolvable, `url`/`doi`/`corpus_id`),
  and `novelty_score` (only when `do_novelty` was run)
- `knowledge_gaps` — open questions, or the failure reason on a non-completed run
- `papers_reviewed` — how many papers grounded the theories

If the literature is thin or mixed, say so rather than overstating confidence.
These are literature-grounded *hypotheses*, not established fact.
