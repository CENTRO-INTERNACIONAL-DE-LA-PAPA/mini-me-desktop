# Uncertainty And Intervals

Use uncertainty-aware methods when decisions depend on risk, not just point
predictions.

## Distinguish the outputs

- confidence interval: uncertainty about an estimated parameter
- predictive interval: uncertainty about a future observation
- posterior interval: uncertainty under a Bayesian model

Do not conflate them.

## When uncertainty matters

Use interval-aware workflows when:

- high-stakes decisions depend on the prediction
- the user explicitly asks for uncertainty
- small samples make point estimates fragile
- forecasting ranges matter more than point forecasts

## Practical approaches

- bootstrap intervals for model performance or simple predictive summaries
- quantile models when interval prediction is needed
- Bayesian posterior predictive intervals with PyMC when full uncertainty
  modeling is justified

## Reporting rule

Always state:

- what kind of interval is being reported
- how it was obtained
- what assumptions it relies on
