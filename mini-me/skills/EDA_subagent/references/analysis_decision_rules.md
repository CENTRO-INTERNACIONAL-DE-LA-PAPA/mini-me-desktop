# EDA Decision Rules

Use these rules to decide whether EDA should proceed, proceed with caveats, or
recommend the `data_cleaning` skill first.

## Proceed with EDA

Proceed when:

- the data is structurally readable
- key variables exist and are interpretable
- missingness is present but does not erase the main analytical signal
- outliers or odd values exist but do not dominate the dataset

In this case:

- report caveats explicitly
- avoid overclaiming
- keep the focus on descriptive patterns

## Proceed cautiously

Proceed with caution when:

- important variables have moderate missingness
- categories are messy but still understandable
- duplicates exist but do not overwhelm the key counts
- timestamps or joins look imperfect but the main summaries are still useful

In this case:

- state which findings are robust versus fragile
- highlight which summaries may change after cleaning
- recommend a cleaning handoff if the user needs higher confidence

## Hand off to data cleaning first

Recommend the `data_cleaning` skill first when:

- key variables are largely missing
- duplicate records materially distort totals or rates
- identifiers or join keys are broken
- invalid or impossible values dominate a key field
- unit inconsistencies make comparisons unreliable
- category labels are too inconsistent for trustworthy grouping

In this case:

- do not pretend the EDA findings are reliable
- summarize the blockers
- explain why cleaning is a prerequisite

## Outlier interpretation

- Univariate outliers are descriptive signals, not automatic errors.
- Multivariate outlier flags from `IsolationForest` or noise labels from `HDBSCAN` are exploratory.
- Do not recommend deleting outliers from the EDA skill unless the user explicitly asks for cleaning and the evidence is strong.

## Dimensionality reduction interpretation

- `PCA` is useful for global structure and feature contribution.
- `UMAP` is exploratory and sensitive to parameters.
- Treat cluster-like separation in reduced-dimensional plots as a hypothesis, not proof.
