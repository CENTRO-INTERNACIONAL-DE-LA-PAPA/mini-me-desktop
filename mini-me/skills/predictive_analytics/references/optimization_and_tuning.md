# Optimization And Tuning

Hyperparameter optimization is a late-stage step, not the starting point.

## When to tune

Tune only after:

- the predictive task is typed correctly
- the validation scheme is correct
- a baseline exists
- one or two strong candidate model families have been identified

Do not tune a model family that is a bad fit for the problem.

## Preferred tuning strategy

Start modestly.

Prefer:

- sensible defaults
- a small targeted random search
- a modest Bayesian optimization workflow if the environment supports it

Avoid large brute-force grid searches unless the search space is tiny and well
justified.

## Resource awareness

Sandbox compute is usually the wrong place for expensive tuning campaigns unless
the runtime is explicitly provisioned for it.

If the runtime is constrained:

- run only a small exploratory tuning budget
- report that the result is provisional
- generate reproducible code for larger local or dedicated compute

## What to tune

Tune only the parameters most likely to matter.

Examples:

- regularization strength
- tree depth / leaf size
- learning rate and number of estimators
- sampling-related settings
- decision threshold when the use case is classification

## Comparison rule

Use the same validation design and selection metric across all tuned candidates.

Do not compare:

- one model with random split
- another model with grouped split

Do not compare tuned vs untuned models without saying so.

## Output rule

When optimization is requested, return:

- the tuned model family
- the search strategy
- the search space
- the tuning budget
- the validation design
- the best settings found
- whether the search was limited by sandbox compute
- whether a larger external run is recommended
