---
name: data_cleaning
description: >-
  Clean, validate, and document tabular data before analysis.
  Use when the task involves data cleaning, data validation, data quality
  assessment, missing values, duplicates, invalid values, schema checks,
  joins, or unit/category harmonization in scientific tabular data.
---

# Data Cleaning Guidelines

Use this skill when data must be profiled, validated, cleaned, and documented
before downstream analysis or modeling.

## Goals

- Preserve raw data.
- Detect and quantify data quality issues.
- Apply the minimum necessary cleaning steps.
- Save cleaned outputs as new versioned files.
- Report what changed, what was dropped, and what remains unresolved.

## Workflow

1. Profile the dataset structure and missingness patterns.
2. Validate schema, types, keys, and expected domains.
3. Measure data quality issues with pointblank and pandas.
4. Use AGROVOC and CropOntology MCP tools only when semantic harmonization is needed.
5. Apply cleaning steps with pandas.
6. Re-run validation checks on the cleaned output.
7. Return a concise cleaning report with metrics, actions, and remaining risks.

## Preferred execution path

Use the bundled scripts as the default workflow:

- `scripts/profile_dataset.py`: create a pointblank `DataScan` profile plus missingness and duplicate summaries.
- `scripts/validate_dataset.py`: run pointblank validations from a rules file or from common CLI checks such as non-null keys and distinct keys.
- `scripts/validate_joins.py`: evaluate join-key quality, unmatched rows, duplicate keys, and null inflation after a join.
- `scripts/clean_from_findings.py`: apply deterministic pandas cleaning actions from a JSON or YAML actions file.
- `scripts/inspect_pointblank.sh`: inspect the installed pointblank version and available methods before writing custom validation logic.

Prefer these scripts over ad hoc notebook code when the task is repetitive, production-bound, or should be reproducible.

## Data Quality and Validation Guidelines

You follow the six dimensions of data quality:

- **Data Accuracy**: Does the data plausibly reflect reality? Example: real age vs. recorded age. Only claim accuracy issues when the data supports that assessment.
- **Data Completeness**: Measure missingness and empty-value patterns.
- **Data Uniqueness**: Check for duplicated rows, duplicated keys, and near-duplicates when relevant.
- **Data Consistency**: Detect contradictions across columns, tables, joins, and repeated measurements.
- **Data Validity**: Check type, format, unit, domain, and rule compliance. Example: negative ages, invalid dates, impossible measurements.
- **Data Timeliness**: Check whether timestamps, collection periods, or refresh cadence are fit for the intended use.

## What to inspect first

- Row count and column count
- Column names and semantic meaning
- Declared and inferred types
- Primary keys or identifier columns
- Join keys and referential dependencies
- Value domains, units, and category vocabularies
- Missing-value markers beyond NULL, such as `""`, `"NA"`, `"N/A"`, `-999`, or `"unknown"`

## Metrics to check

- % NULL or missing values per column
- % duplicated rows
- % duplicated keys
- % out of range values
- % records that violate business or scientific rules
- % invalid category values
- % failed joins or unmatched foreign keys
- % inconsistent units or malformed formats

## Cleaning actions

Use only the actions justified by the data and document each one:

- Standardize column names only when needed for reliability.
- Cast columns to correct types.
- Normalize missing-value markers.
- Deduplicate rows or records using explicit rules.
- Standardize category labels, spelling, and casing.
- Harmonize units before comparing or combining values.
- Flag, remove, or set aside impossible values according to domain rules.
- Validate join keys before merging tables.
- Create derived validation flags when rows must be reviewed rather than silently changed.

## Ontology harmonization

Use ontology MCP tools only when the task needs domain vocabulary alignment:

- `Agrovoc MCP`: harmonize agricultural concepts, crop names, and controlled vocabulary terms.
- `CropOntology MCP`: harmonize crop- and trait-specific terms, variables, or ontology-backed identifiers.

Do not use ontology MCP tools for generic missingness checks, duplicate detection, type casting, or row-level cleaning. Those tasks belong to the local pointblank and pandas workflow.

## Cleaning rules

- Never overwrite the raw dataset.
- Save cleaned outputs as new versioned files.
- Prefer reversible and auditable transformations.
- Do not invent values unless imputation is explicitly requested or justified.
- If imputation is used, state the method, columns affected, and assumptions.
- If rows are removed, report how many and why.
- Avoid overly aggressive cleaning that makes the dataset unusably small or biased.
- If a proposed cleaning rule would remove a large fraction of rows, reassess whether the threshold is too strict and prefer conservative handling.
- If applying a rule would remove most of the dataset, do not apply it silently. Report the tradeoff explicitly and consider flagging, partial retention, or column-specific handling instead of dropping the rows.
- If uncertainty remains, keep the data and flag the issue instead of making an unsupported correction.

## Validation and cleaning implementation notes

- Use the local `pointblank` Python package as the primary validation engine.
- Use `pointblank.DataScan` for broad dataset profiling.
- Use `pointblank.Validate` for rule-based validation checks.
- Use pandas for deterministic cleaning, duplicate handling, joins, and post-validation fixes.
- If a validation requires a specific pointblank method, inspect the installed API with `scripts/inspect_pointblank.sh` before writing custom code.
- If the rule set is stable, encode it in a JSON or YAML rules file and pass it to `scripts/validate_dataset.py`.
- If the cleaning actions are stable, encode them in a JSON or YAML actions file and pass it to `scripts/clean_from_findings.py`.

## Expected output

Return a concise report with:

- dataset inspected
- key quality issues found
- validation checks performed
- cleaning actions applied
- output files created
- metrics before and after cleaning
- remaining risks, caveats, or rows requiring manual review
