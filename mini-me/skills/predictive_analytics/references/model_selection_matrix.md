# Model Selection Matrix

Choose methods based on task, objective, and constraints.

## Start with baselines

Always fit at least one simple baseline unless the user explicitly forbids it.

- regression: linear regression or elastic net
- binary classification: logistic regression
- multiclass: multinomial logistic regression or simple tree baseline
- counts: Poisson GLM
- overdispersed counts: Negative Binomial GLM

## Stronger tabular candidates

Use stronger candidates when accuracy matters and the data justifies them.

- tree ensembles
- boosted trees
- TabPFN for supported tabular classification/regression use cases

## When interpretability is primary

Prefer:

- GLMs
- regularized linear models
- monotonic or constrained models when applicable
- shallow decision trees only when a simple rule-based model is explicitly
  desired

Do not treat tree ensembles or boosted trees as white-box models.

## When the user does not know what to use

For supervised tabular classification/regression:

- propose a baseline
- propose TabPFN as a strong default candidate when package, license, size, and
  hardware constraints allow it
- compare rather than assume it will win

## When counts matter scientifically

Prefer count-aware models over generic regression:

- Poisson
- Negative Binomial
- zero-inflated variants only with strong justification

## When the user needs explicit uncertainty

Prefer:

- Bayesian models with PyMC
- quantile or interval-aware approaches
- bootstrap-based intervals if a full Bayesian model is not justified

## When data is small

Consider:

- regularized simpler models
- partial pooling / Bayesian models when domain knowledge exists
- TabPFN when the task is supported and resources allow it

## When time ordering matters

Do not use iid validation logic.

Use:

- lag/rolling/calendar features for tabular forecasting
- rolling or forward-chaining validation
- horizon-aware evaluation
