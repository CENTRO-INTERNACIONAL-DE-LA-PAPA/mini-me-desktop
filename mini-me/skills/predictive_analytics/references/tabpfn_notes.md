# TabPFN Notes

Use this reference before choosing TabPFN.

## When TabPFN is a good candidate

Use TabPFN when:

- the task is supervised tabular classification or regression
- the user wants a strong default model and does not know what to use
- the dataset is within practical size limits
- mixed feature types or missing values are present

## When not to treat it as the first model

Do not make TabPFN the first model for:

- count processes where Poisson/Negative Binomial structure matters
- time-series tasks without proper temporal reformulation
- cases where the package, license, or runtime requirements are not available

## Practical constraints

- Confirm the package is installed or can be installed.
- Confirm the license terms are acceptable before trying to download gated
  weights.
- Confirm the sandbox resources are large enough.
- Do not assume GPU access exists.

## Recommendation rule

When the user does not know what to use:

1. fit a simple baseline
2. evaluate whether TabPFN is feasible
3. compare baseline vs TabPFN fairly
4. recommend it only if performance and operational constraints justify it
