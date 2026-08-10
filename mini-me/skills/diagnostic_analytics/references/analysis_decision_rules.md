# Diagnostic Analysis Decision Rules

Use these rules to choose the right explanatory analysis path.

## Use group comparisons when

- the outcome is being compared across treatments, sites, regions, or categories
- the question is fundamentally "how different are these groups?"
- the user needs a simple, interpretable explanation

## Use regression when

- multiple candidate drivers may explain the same outcome
- you need adjusted estimates rather than raw comparisons
- the goal is to understand conditional associations

## Use confounding checks when

- a focal relationship may disappear or change after controls
- subgroup imbalance is likely
- the same variable may proxy for several mechanisms

## Use time-change analysis when

- there is a date, intervention, season, or period shift
- the question is about change over time rather than static differences

## Escalate to cleaning first when

- the outcome variable is too incomplete to support inference
- duplicates distort the outcome or exposure definitions
- key joins are unreliable
- invalid values dominate important variables

## Escalate to predictive or Bayesian work when

- the question becomes forecasting-oriented
- uncertainty modeling is central
- the user needs probabilistic decision support beyond standard diagnostics
