# Choosing a treatment

Pick the least destructive treatment that removes the risk. Ask the researcher
which columns the analysis needs before proposing to drop anything.

| Column holds | Needed for analysis? | Treatment | Action type |
| --- | --- | --- | --- |
| Name, phone, email, national ID | No | Remove | `drop` |
| Identifier that links records or tables | Yes | Replace with a code, no key kept | `pseudonymize` |
| Exact coordinates | Yes, as location | Round to 1 decimal (about 11 km) or replace with district | `round_coordinates` |
| Exact coordinates | No | Remove | `drop` |
| Birth date, interview date | Yes | Keep the year, or year and month | `generalize_date` |
| Exact age, household size | Yes | Bands of 5 or 10 | `bin_numeric` |
| Farm size, income with extreme values | Yes | Cap the top values | `top_code` |
| Village or other small place | Yes | Keep places with enough records, merge the rest | `keep_top_categories` |
| Free text | No | Remove | `drop` |
| Free text | Yes | Redact emails, phones, IDs, and numbers | `redact_text` |
| Rare combinations left after all of the above | n/a | Remove those rows | `suppress_small_groups` |

## Coordinate precision

| Decimals | Approximate precision | Use |
| --- | --- | --- |
| 4 | 11 m | Identifies a plot. Never share for household data |
| 2 | 1.1 km | Identifies a village. Rarely enough |
| 1 | 11 km | District scale. Default for shared survey data |
| 0 | 111 km | Regional scale |

Trial-station coordinates are not personal data and can stay exact.

## k-anonymity

A dataset is k-anonymous when every combination of quasi-identifier values is
shared by at least k records.

- Default threshold: k = 5. Use k = 10 for sensitive topics or small places.
- Measure on the quasi-identifiers that stay in the output, all together.
- When k is too low, generalize the column with the most distinct values
  first. Suppress rows only for what is left.
- Report how many rows suppression removed. If it is more than about 5% of
  the dataset, generalize more instead.

## What this skill does not do

- It does not judge legal basis, consent, or data-sharing agreements.
- It does not detect names in free text reliably without `--presidio`, and
  Presidio's models are weakest for names outside English and Spanish.
- It does not protect against linkage with other datasets the recipient holds.

These are the data steward's checks.
