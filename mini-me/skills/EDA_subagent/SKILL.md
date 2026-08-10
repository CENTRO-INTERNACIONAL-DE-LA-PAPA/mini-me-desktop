---
name: eda_subagent
description: >-
  Perform exploratory data analysis to understand what happened in tabular data
  through profiling, descriptive summaries, multilevel visualizations,
  correlation and trend analysis, and anomaly detection.
  Use before cleaning to assess interpretability and caveats, or after cleaning
  to extract clearer insights without duplicating the cleaning subagent's work.
---

# Exploratory Data Analysis Guidelines

Use this skill when the goal is to understand **what happened** in the data through
profiling, summarization, visualization, trend detection, and anomaly review.
This skill supports two modes:

- **Before cleaning**: produce cautious insights, quantify caveats, and decide whether the data is interpretable enough to proceed.
- **After cleaning**: produce stronger descriptive summaries, clearer plots, and more reliable insights for downstream analysis.

## Goals

- Understand the dataset structure, major distributions, and salient patterns.
- Quantify what appears unusual, concentrated, sparse, or highly variable.
- Produce visual summaries that help explain the data clearly.
- Identify caveats that materially affect interpretation.
- Recommend the next step: continue with EDA, hand off to cleaning, or move to deeper analysis.

## Boundaries

This skill does **not** own deterministic cleaning, rule-based validation plans,
or ontology harmonization. It may inspect missingness, duplicates, invalid
values, or anomalies as analytical caveats, but it should not silently fix them.

If severe data quality problems materially compromise interpretation, say so
explicitly and recommend the `data_cleaning` skill instead of duplicating its
work.

## Workflow

1. Profile the dataset structure and variable types.
2. Review missingness, basic distinct counts, and potential caveats.
3. Run univariate summaries and plots.
4. Run bivariate summaries and plots.
5. Run multivariate structure and anomaly analysis when justified.
6. Summarize what happened, what is uncertain, and what should happen next.

## Preferred execution path

Use the bundled scripts as the default workflow:

- `scripts/profile_eda.py`: create a pointblank `DataScan` profile, missingness summary, and optional `missingno` plots.
- `scripts/univariate_eda.py`: generate descriptive summaries and plots for numeric, categorical, and datetime variables.
- `scripts/bivariate_eda.py`: generate correlations, grouped summaries, scatterplots, grouped boxplots, and contingency views.
- `scripts/multivariate_eda.py`: generate PCA, optional UMAP embeddings, optional HDBSCAN clusters, and `IsolationForest` anomaly scans.
- `scripts/eda_report.py`: combine outputs into a concise markdown report for the agent or user.

Prefer these scripts over ad hoc notebook code when the task should be
reproducible or production-aligned.

## Plotting rules

When generating plots, follow the **visualization skill**.

Do not generate plots just because you can. Prefer the smallest set of clear
plots that explains the main patterns.

## Analytical expectations

### Univariate EDA

- Numeric variables: central tendency, spread, skew, tails, outliers, missingness.
- Categorical variables: cardinality, top categories, imbalance, rare levels.
- Datetime variables: coverage, gaps, frequency, and temporal concentration.
- Always pair important tabular summaries with at least one relevant plot.

### Bivariate EDA

- Numeric-numeric: correlations, monotonic patterns, nonlinear shapes, clusters.
- Categorical-numeric: grouped summaries, median shifts, spread differences.
- Categorical-categorical: contingency tables, concentration, imbalance.
- Use scatterplots, boxplots, violin plots, heatmaps, and grouped bar plots as needed.

### Multivariate EDA

- Use correlation structure and dimensionality reduction to understand global shape.
- Use `PCA` as the standard structure-discovery tool for numeric data.
- Use `UMAP` as an optional nonlinear exploratory embedding when installed and useful.
- Use `IsolationForest` for multivariate anomaly detection.
- Use `HDBSCAN` as an optional density-based cluster and noise detector when installed.
- Treat multivariate outlier methods as exploratory signals, not automatic evidence for deletion.

## When to hand off to cleaning

Read [references/analysis_decision_rules.md](references/analysis_decision_rules.md)
when deciding whether EDA should proceed or defer to data cleaning.

In general, recommend `data_cleaning` first when:

- key variables are mostly missing
- duplicates materially distort counts or rates
- joins are unreliable
- invalid values dominate important variables
- unit/category inconsistencies invalidate comparison

## Implementation notes

- Use the local `pointblank` Python package for dataset profiling when useful.
- Use `missingno` plots as an optional support for missingness interpretation.
- Use pandas and seaborn/matplotlib for summaries and plotting.
- Use scikit-learn for PCA and `IsolationForest`.
- Treat UMAP and HDBSCAN as optional enhancements. If the packages are not installed, report that they were skipped.
- Do not overwrite raw data or perform cleaning beyond lightweight analysis-only preparation required for multivariate methods.

## Expected output

Return a concise report with:

- dataset inspected
- key descriptive findings
- most relevant plots or visual insights
- notable anomalies or unusual structure
- caveats that limit interpretation
- recommended next step
