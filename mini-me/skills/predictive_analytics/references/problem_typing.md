# Problem Typing

Before modeling, identify the predictive problem precisely.

## Binary classification

Use when the target has two states, such as:

- event vs no event
- disease vs no disease
- churn vs no churn

Typical baselines:

- logistic regression
- simple decision tree

## Multiclass classification

Use when the target has more than two categories.

Typical baselines:

- multinomial logistic regression
- simple tree baseline

## Continuous regression

Use when the target is approximately continuous and unconstrained.

Examples:

- yield
- temperature
- price

Typical baselines:

- linear regression
- elastic net

## Count regression

Use when the target is a non-negative count and the count interpretation
matters.

Examples:

- number of pests
- number of disease cases
- number of visits

Default path:

- Poisson GLM first
- Negative Binomial when overdispersion is present
- zero-inflated models only when there is a real structural reason for excess
  zeros

Do not jump straight to generic regression if the count process matters.

## Proportion or rate prediction

Use when the target is a probability, proportion, or count with an exposure
term or denominator.

Examples:

- germination rate
- infection rate
- success proportion

Prefer count-aware/binomial-aware formulations when the data supports them.

## Time-series forecasting

Use when the target is indexed by time and the goal is future prediction.

Examples:

- next-week demand
- next-month yield estimate
- future rainfall class

Use time-aware validation and forecasting logic. Never use random shuffles.
