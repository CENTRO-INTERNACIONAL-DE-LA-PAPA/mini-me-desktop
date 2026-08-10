---
name: data_voyager
description: >-
  Use the Asta DataVoyager pipeline to generate and test hypotheses against a
  local tabular dataset. Use when the task is to analyze a dataset, explore what
  is in a CSV, find patterns, or test theories/questions against the user's own
  data with an autonomous, code-executing analysis.
---

# DataVoyager Data Analysis Guidelines

Use this skill when the goal is to analyze the user's **own tabular dataset** —
generating and testing hypotheses against it with code — rather than searching
the literature or scoping a single bounded analysis step yourself.

This skill is **tool-first**. It uses the `analyze_data` tool, which runs the
Asta DataVoyager pipeline end-to-end (upload data → write and execute code in a
sandboxed notebook → answer the question) and returns the result already parsed.
You do **not** call the `asta` CLI directly and you do **not** poll with
`execute` — the tool owns the whole long-running job.

## When to use this skill

Use when the user asks:

- "Analyze this dataset / this CSV"
- "Run DataVoyager" / "explore this data"
- "What patterns / correlations are in this data?"
- "Test these theories against my data" (the loop from a generated theory to
  evidence in the data)

## Boundaries

This skill does **not** own:

- literature discovery or theory synthesis (use `academic_researcher` /
  `hypothesis_generator`)
- a specific, bounded analysis step you scope yourself — a single profiling pass,
  one regression, one forecast (use `exploratory_data_analysis`,
  `diagnostic_analytics`, or `predictive_analytics`)
- final report writing (use `report_writer`)

Prefer DataVoyager for open-ended, agentic "analyze / test against my data"
requests where the agent should decide the analysis itself.

## Step 1 — Draft a tightened question

Before running anything, turn the user's request into a **tightened analytical
question** that:

- Names the specific dataset(s) to analyze
- States the decision or insight the user is after — not just "analyze X"
- Is phrased as a question DataVoyager can answer with code

Uploaded files are already on disk at the relative paths in the user's
"Attached files" blockquote. Use those exact paths; **never** ask the user to
re-upload — the tool uploads them to the DataVoyager workspace.

## Tool: `analyze_data`

The pipeline is asynchronous and slow — a few minutes for a simple EDA, up to
20–40 minutes for multi-step modelling. `analyze_data` submits the run and then
waits, returning the finished result or `status="running"` if the run outlasts
the wait budget.

Parameters:

- `question` — the tightened analytical question (required to start).
- `dataset_paths` — the local dataset path(s), comma-separated for several. The
  exact relative paths from the "Attached files" blockquote.
- `context_id` — an existing DataVoyager session id, to ask a **follow-up**
  against the same workspace (reuses the uploaded data; attach new files via
  `dataset_paths`).
- `resume_task_id` — an existing task id to keep **waiting** on a run this tool
  already started (use only when a prior call returned `running`).

### Flow

- **Start**: `analyze_data(question=<tightened>, dataset_paths=<paths>)`.
- **Follow-up** (same data): pass the prior `context_id`.
- **Keep waiting** (only after a `running` result):
  `analyze_data(resume_task_id=<task id>, context_id=<context id>)`.

Set the `DataAnalysisResults.status` field to match the tool result:

1. `"completed"` → build `DataAnalysisResults` from the result: a tight `summary`,
   the concrete `findings` (each with a `chart_path` when a figure backs it), the
   `hypotheses_tested` with their verdicts, and the `charts` produced. Ground
   every claim in the returned `analysis_text` and the files the run wrote to
   disk — read them with your filesystem tools if you need detail. Set `question`,
   `dataset_paths`, `task_id`, `context_id`.
2. `"running"` → still generating. Return `status="running"` with the
   `task_id`/`context_id`, the `question`, and empty findings. Tell the user it is
   still running and they can ask again to keep waiting. Do not call the tool
   again in a loop.
3. `"input-required"` → relay the tool's `prompt`, ask the user for the missing
   input, and return `status="input-required"` with the `task_id`/`context_id`.
4. `"failed"`/`"error"` → `DataAnalysisResults` with `status="failed"`, the
   `question`, empty findings, and the reason in `summary`.

### The one rule that matters

**Never fabricate findings, numbers, or charts.** If the run failed or is still
running, say so honestly — the charts/notebook the run wrote to disk also appear
in the user's Images/Files tabs on their own.

## Expected output

A structured `DataAnalysisResults`:

- `question` — the analytical question answered
- `dataset_paths` — the dataset(s) analyzed
- `summary` — short narrative synthesis of what the data said
- `findings` — key insights, each with a `title`, `detail`, and optional `chart_path`
- `hypotheses_tested` — the hypotheses generated/evaluated and their verdicts
- `charts` — relative paths to the figures produced
- `status`, `task_id`, `context_id`

If the data is thin or the analysis is inconclusive, say so rather than
overstating confidence. These are data-driven findings on the user's own dataset,
to be reviewed by a subject-matter expert.
