---
name: predictive_analytics
description: >-
  Choose and execute predictive models for tabular and time-ordered data based
  on the user's goal, target type, data constraints, validation design, and
  required uncertainty. Use when the task asks what will happen, which model to
  use, how to forecast or predict an outcome, or how to compare predictive
  approaches such as GLMs, tree-based models, TabPFN, or Bayesian models.
---

# Predictive Analytics Guidelines

Use this skill when the question is predictive: `what will happen?`

This skill is recommendation-first. Do not default to one fixed script. Choose
methods that match:

- the target type
- the user's preferred tradeoff: accuracy, interpretability, uncertainty, speed
- the data size and structure
- the validation design needed to avoid leakage
- the packages and hardware actually available in the sandbox

## Goals

- Match the model family to the prediction problem.
- Train at least one sensible baseline before recommending a stronger model.
- Use validation that matches the data-generating structure.
- Report predictive performance, assumptions, and deployment caveats.
- Recommend next steps only after comparing candidates fairly.

## Boundaries

This skill does **not** own:

- data cleaning
- broad descriptive exploration
- causal explanation

If the data is too noisy, leaked, or structurally broken for trustworthy
prediction, recommend the `data_cleaning` skill first.

## Workflow

1. Identify the predictive task.
2. Clarify the target, prediction unit, horizon, and evaluation metric.
3. Choose a baseline and one or more stronger candidates.
4. Pick a leakage-safe validation design.
5. Train, validate, compare, and calibrate if needed.
6. Return predictions, model performance, assumptions, risks, and next steps.

## First classify the task

Read [references/problem_typing.md](references/problem_typing.md) first.

Classify the problem into one of:

- binary classification
- multiclass classification
- continuous regression
- count regression
- proportion or rate prediction
- time-series forecasting

Do not treat all numeric targets as the same problem. Counts, proportions, and
time-dependent targets often require different model families and validation.

## Method selection

Use [references/model_selection_matrix.md](references/model_selection_matrix.md)
to choose the model family.

Default approach:

- Start with a simple baseline.
- Add a stronger candidate only when the data and objective justify it.
- Prefer interpretable models when the user asks for interpretability or policy
  decisions depend on the model.
- Prefer stronger tabular foundation or ensemble models when the user wants a
  robust default and predictive accuracy matters more than coefficient-level
  interpretation.

Examples:

- Continuous regression:
  - baseline: linear regression or elastic net
  - stronger candidates: tree-based models or TabPFN regression when suitable
- Count targets:
  - baseline/default: Poisson GLM
  - if overdispersion is present: Negative Binomial GLM
  - only use zero-inflated count models when excess zeros are a real structural
    feature of the process
- Binary classification:
  - baseline: logistic regression
  - stronger candidates: calibrated tree-based models or TabPFN classification
- Multiclass classification:
  - baseline: multinomial logistic regression or simple tree baseline
  - stronger candidates: strong tabular models, including TabPFN when suitable
- Proportions/rates:
  - use binomial/count-aware formulations when numerator-denominator structure
    exists
- Time series:
  - use forecasting-aware validation and feature engineering
  - do not use random splits

## Feature engineering and preprocessing

Read [references/feature_engineering_and_preprocessing.md](references/feature_engineering_and_preprocessing.md)
before applying transformations or encodings.

Use preprocessing conditionally, not mechanically.

- scaling and transformation matter for linear, distance-based, and some neural
  methods
- many tree models do not need scaling
- target encoding must be fit only inside training folds
- VIF is useful for coefficient stability in linear/GLM-style models, not as a
  universal predictive preprocessing step

## Class imbalance

Read [references/class_imbalance.md](references/class_imbalance.md) for
imbalanced classification work.

When the target is imbalanced:

- choose metrics that reflect the real decision problem
- consider class weights, threshold tuning, and resampling methods
- apply resampling only on training folds, never before the split
- prefer simpler imbalance-aware methods before stacking multiple resamplers

## TabPFN

Read [references/tabpfn_notes.md](references/tabpfn_notes.md) before choosing it.

Use TabPFN as a strong default candidate when:

- the task is supervised tabular classification or regression
- the user does not know what model to use
- the dataset size is within practical limits
- package access, license acceptance, and hardware are acceptable

Do **not** use TabPFN blindly for:

- forecasting without proper temporal feature construction and time-aware splits
- problems where a count-aware GLM is the correct first model family
- settings where the sandbox cannot reasonably support the package/runtime cost

## Bayesian models and PyMC

Read [references/bayesian_modeling.md](references/bayesian_modeling.md) when
uncertainty, priors, hierarchical structure, or decision-risk quantification
are central.

Use PyMC when:

- interval estimates and posterior uncertainty are central to the task
- hierarchical or partial-pooling structure matters
- priors from domain knowledge should influence the model
- small data or high-stakes decisions benefit from explicit uncertainty

Do **not** default to PyMC when:

- a simpler predictive baseline already answers the user's question
- the task is latency-sensitive and posterior sampling cost is unjustified
- the sandbox resources are too limited for the proposed model

If using priors, document how they were chosen. If the user or researcher can
provide prior knowledge, use that before browsing for generic priors.

## Validation and metrics

Read [references/evaluation_and_validation.md](references/evaluation_and_validation.md).

Always match validation to the problem:

- iid tabular data: train/validation/test or cross-validation
- grouped entities: grouped splits
- time-dependent data: rolling or forward-chaining validation

Always report metrics that match the task:

- regression: RMSE, MAE, R-squared when useful
- counts: MAE/RMSE plus calibration against count behavior
- classification: ROC AUC, PR AUC, log loss, calibration, threshold-aware
  metrics when relevant
- forecasting: horizon-aware error metrics

Do not compare models using incompatible validation schemes.

## Hyperparameter optimization

Read [references/optimization_and_tuning.md](references/optimization_and_tuning.md)
before running tuning.

Treat optimization as a late-stage step, after:

- the problem type is correct
- the validation design is leakage-safe
- baseline and strong candidate models already exist

Do not default to large hyperparameter searches in the sandbox.

If optimization is requested:

- start with a small, defensible search budget
- tune only the most plausible model families
- prefer random search or modest Bayesian optimization over brute-force grids
- stop if compute cost is disproportionate to expected gain

If the search is too expensive for the current runtime:

- provide reproducible code, parameter ranges, and evaluation design
- explain what should be run on larger local or dedicated compute
- do not pretend that an untuned sandbox run is a full optimization study

## Uncertainty

Read [references/uncertainty_and_intervals.md](references/uncertainty_and_intervals.md).

If the user needs uncertainty:

- prefer models or procedures that produce intervals or calibrated
  uncertainty
- distinguish predictive intervals from confidence intervals
- say clearly whether uncertainty comes from bootstrap, Bayesian posterior,
  quantile models, or another method

## Time series

Read [references/time_series_rules.md](references/time_series_rules.md) for all
forecasting work.

Do not:

- shuffle time
- leak future information into training
- evaluate one-step and multi-step forecasts as if they were the same problem

## Package and sandbox discipline

Assume only installed packages are safe by default.

Before choosing a method that needs extra dependencies or hardware:

- check whether the package is installed
- check whether the sandbox resources are sufficient
- justify the installation cost if you need to add a package

When GPU-sensitive methods are proposed, do not assume a GPU exists. Verify the
runtime first.

## Expected output

Return:

- the predictive task type
- the candidate models considered
- why the chosen model family fits the problem
- the validation design
- the main performance metrics
- uncertainty or calibration notes when relevant
- key assumptions and deployment caveats
- the recommended next step
