# Evaluation And Validation

Validation design is part of model choice.

## General rules

- Never compare models under mismatched splits.
- Watch for leakage through identifiers, time, groups, or target-derived
  features.
- Use a held-out test set when a final unbiased estimate is needed.

## IID tabular data

Use:

- train/validation/test split
- or cross-validation when data is limited

## Grouped data

Use grouped splits when observations from the same entity should not be split
across train and validation.

Examples:

- farmer
- field
- location
- experiment batch

## Time-dependent data

Use:

- rolling-origin evaluation
- forward chaining
- horizon-specific backtesting

Never randomize time order.

## Metrics

### Regression

- MAE
- RMSE
- sometimes R-squared

### Classification

- ROC AUC
- PR AUC for imbalanced problems
- log loss
- thresholded precision/recall/F1 when operational thresholds matter
- calibration when predicted probabilities will be acted upon

### Forecasting

- horizon-specific MAE/RMSE/MAPE-like metrics if appropriate
- compare against naive time baselines

### Counts

- MAE / RMSE
- calibration against observed count behavior
- check whether the model preserves non-negativity and plausible dispersion

## Model comparison

- Compare baseline vs stronger candidate fairly.
- Prefer simpler models when performance is similar.
- Prefer better-calibrated models when probabilities or intervals matter.
