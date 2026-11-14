---
name: anonymization
description: >-
  Detect and remove personal data from tabular research data before it is
  analysed, shared, or published. Use when the task involves anonymization,
  de-identification, pseudonymization, PII, personal data, farmer or household
  surveys, names, phone numbers, national IDs, GPS coordinates of homes or
  plots, k-anonymity, or preparing a dataset for Dataverse or for use with AI
  tools.
---

# Anonymization Guidelines

Use this skill when a dataset may hold personal data and must be made safe
before analysis, sharing, or publication. It covers the anonymization step of
the FAIR data cycle: detect, decide, transform, measure, document.

## The one rule that shapes everything

**Raw values of a suspect column never enter the conversation.** The scripts
run locally and report column names, counts, and rates only. Do not print,
`head`, sample, or quote cell values from a dataset that has not been
anonymized yet. Decide from the detection report, not from reading rows.

## Goals

- Find direct identifiers, quasi-identifiers, and sensitive columns.
- Propose the least destructive treatment for each one and get it confirmed.
- Apply the treatments to a new file. Never overwrite the raw dataset.
- Measure the remaining re-identification risk (k-anonymity).
- Leave a report a data steward can review and sign off.

## Workflow

1. Run `scripts/detect_pii.py` to get a value-free report of suspect columns.
2. Classify each flagged column with `references/pii_categories.md`.
3. Propose an actions file (one action per column) using
   `references/treatment_rules.md`. Show it to the researcher and wait for
   confirmation: which columns are needed for the analysis is their decision.
4. Run `scripts/anonymize_dataset.py` with the confirmed actions file.
5. Run `scripts/check_k_anonymity.py` on the output with the remaining
   quasi-identifiers. If k is below the agreed threshold (default 5),
   generalize further or suppress the small groups, then measure again.
6. Run `scripts/detect_pii.py` on the output to confirm nothing is left.
7. Return the report described under "Expected output".

## Scripts

- `scripts/detect_pii.py`: scans every column by name hints and by value
  patterns (email, phone, national ID, coordinates, dates, URLs, IPs), plus
  uniqueness and free-text checks. `--presidio` adds named-entity detection
  with Microsoft Presidio when it is installed. The report holds no raw values.
- `scripts/anonymize_dataset.py`: applies an actions file (JSON or YAML) and
  writes the anonymized table and a report of what changed.
- `scripts/check_k_anonymity.py`: measures k-anonymity over the
  quasi-identifiers, and l-diversity for a sensitive column when one is given.

Prefer these scripts over ad hoc code. They are deterministic and auditable,
and they keep values out of the conversation.

## Actions file

```json
{
  "actions": [
    {"type": "drop", "columns": ["farmer_name", "phone", "dni"]},
    {"type": "pseudonymize", "column": "household_id", "prefix": "HH"},
    {"type": "round_coordinates", "columns": ["latitude", "longitude"], "decimals": 1},
    {"type": "generalize_date", "column": "birth_date", "to": "year"},
    {"type": "bin_numeric", "column": "age", "width": 10},
    {"type": "top_code", "column": "farm_size_ha", "upper": 20},
    {"type": "keep_top_categories", "column": "village", "min_count": 10, "other": "Other"},
    {"type": "redact_text", "column": "comments"},
    {"type": "suppress_small_groups", "columns": ["district", "sex", "age"], "k": 5}
  ]
}
```

Supported types: `drop`, `pseudonymize`, `round_coordinates`,
`generalize_date`, `bin_numeric`, `top_code`, `keep_top_categories`,
`redact_text`, `suppress_small_groups`.

## Rules

- Never overwrite the raw dataset. Save outputs as new versioned files.
- Never write a pseudonym key (original to code) next to the anonymized file.
  `--key-file` exists for the data owner and must point outside any folder that
  will be shared.
- Pseudonymized data is still personal data. Say so in the report.
- Do not drop a column the analysis needs when generalizing it is enough.
- Do not claim a dataset is anonymous. Report the k reached, the
  quasi-identifiers it was measured on, and what was not checked.
- Free-text columns are the highest risk and the hardest to clean. Prefer
  dropping them; redact only when the text is needed.
- If uncertain whether a column identifies people, treat it as if it does and
  flag it for the data steward.
- A human data steward approves the result before the data is shared or
  published. This skill prepares that decision; it does not replace it.

## Expected output

Return a concise report with:

- dataset inspected and output files created
- columns flagged, their category, and the treatment applied
- rows and cells changed, dropped, or suppressed
- k-anonymity before and after, and the quasi-identifiers used
- residual risks and columns that need a data steward's review
