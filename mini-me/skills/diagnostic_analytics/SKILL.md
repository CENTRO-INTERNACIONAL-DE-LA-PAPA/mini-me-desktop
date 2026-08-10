---
name: diagnostic_analytics
description: >-
  Diagnose why an outcome happened in tabular data using interpretable
  inference, group comparisons, regression, confounding checks, time-change
  analysis, and diagnostic visualizations.
  Use when the task asks why a pattern, difference, anomaly, or change may
  have occurred and the goal is explanation rather than prediction.
---

# Diagnostic Analytics Guidelines

Use this skill when the goal is to explain why something happened in the data.
This skill focuses on interpretable inference and disciplined explanatory
reasoning, not broad exploration or predictive modeling.

## Goals

- Identify plausible drivers of an observed outcome.
- Quantify differences, associations, and changes.
- Check whether explanations remain stable after adding controls.
- Visualize the evidence clearly.
- Communicate assumptions, caveats, and the strength of the explanation.

## Boundaries

This skill does **not** own data cleaning or broad descriptive exploration.
If the data is too messy for trustworthy inference, recommend the
`data_cleaning` skill first.

By default, use associative language. Do not make strong causal claims unless
the design genuinely supports them.

## Human clarification

When the research question, hypothesis, outcome, candidate drivers, unit of
analysis, time window, or confounders are unclear, call the
`request_diagnostic_context` tool before running inference.

Use it to clarify:

- research question or decision goal
- primary hypothesis
- outcome variable
- candidate drivers or exposures
- unit of analysis
- time window
- candidate confounders
- whether the goal is associative or causal

## Workflow

1. Clarify the diagnostic question if the target, hypothesis, or design is underspecified.
2. Choose the simplest defensible analysis design.
3. Compare groups, estimate driver associations, or evaluate change over time.
4. Check assumptions, outlier influence, collinearity, and confounding risk.
5. Visualize the evidence.
6. Return the most plausible explanations with caveats.

## Preferred execution path

Use the bundled scripts as the default workflow:

- `scripts/compare_groups.py`: use DABEST first for effect sizes, estimation plots, optional forest plots, and supplemental classical tests on request.
- `scripts/driver_regression.py`: fit interpretable regression models, produce coefficient tables, confidence intervals, and VIF diagnostics.
- `scripts/confounding_checks.py`: compare unadjusted and adjusted estimates and inspect subgroup or stratified caveats.
- `scripts/time_change_analysis.py`: evaluate pre/post changes and temporal patterns.
- `scripts/diagnostic_report.py`: combine outputs into a concise markdown report.

## Plotting rules

When generating plots, follow the visualization skill.

Use only the plots needed to support the explanation:

- grouped boxplots or violin plots
- coefficient plots
- residual or fitted-value diagnostics
- before/after trend plots
- effect-size comparison plots

## Method guidance

### Group comparison

Use when the question is about differences between categories, treatments,
segments, sites, or periods.

Use DABEST as the default comparison framework. Prioritize effect sizes,
confidence intervals, and estimation plots over p-values alone. Add classical
tests only when the user requests them or they are needed as a supplemental
check.

### Driver regression

Use when the question is about which variables are associated with the outcome
after accounting for others.

### Confounding checks

Use when a focal driver may be entangled with controls, grouping structure, or
selection effects.

### Time-change analysis

Use when the question is about what changed around a date, event, intervention,
or period shift.

## Diagnostics and assumptions

- Check missingness in the outcome and key predictors.
- Check group imbalance and sample sizes.
- Check outlier influence before overinterpreting results.
- Check collinearity in regression models with `VIF`.
- Treat `VIF` as a regression stability diagnostic, not as a confounding test.
- Check whether the focal estimate changes materially after adding controls.
- For time analyses, check whether temporal ordering and intervention timing are plausible.

## Interpretation rules

Read these references when forming conclusions:

- `references/analysis_decision_rules.md`
- `references/assumption_checklist.md`
- `references/interpretation_rules.md`

## Expected output

Return a concise report with:

- diagnostic question addressed
- methods used and why they were chosen
- strongest findings
- uncertainty and caveats
- how strong the explanation is
- recommended next step
