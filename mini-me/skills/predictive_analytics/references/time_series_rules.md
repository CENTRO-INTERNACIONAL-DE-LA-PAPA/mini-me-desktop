# Time Series Rules

Use these rules whenever the prediction target is time-ordered.

## Validation

- never shuffle observations across time
- train on the past, validate on the future
- use rolling or forward-chaining validation

## Feature engineering

Useful features may include:

- lags
- rolling means or sums
- rolling standard deviations or min/max summaries
- expanding-window summaries
- seasonality indicators
- holiday or event indicators
- weather or intervention covariates known at prediction time

Use lag and rolling features only when they can be constructed from information
available at the prediction timestamp.

## Leakage checks

Do not include:

- future values
- future-derived aggregates
- labels from after the prediction timestamp
- rolling or expanding features that were computed using future rows

## Forecast framing

Clarify:

- forecast horizon
- one-step vs multi-step
- update frequency
- whether exogenous covariates are known in advance

Feature engineering must match the forecasting setup. Multi-step forecasting may
need different lag structures, recursive logic, or direct-horizon models.

## Baselines

Always compare against a naive baseline when possible.

Examples:

- last observed value
- seasonal naive
- rolling average baseline
