# Bayesian Modeling With PyMC

Use this reference when priors, hierarchical structure, or uncertainty are
central to the predictive task.

## When PyMC is a strong option

Use PyMC when:

- posterior predictive uncertainty is a core deliverable
- partial pooling or multilevel structure matters
- domain knowledge should be encoded through priors
- data is small and regularization through priors is useful
- the user explicitly asks for Bayesian modeling

## When PyMC is not the default

Do not default to PyMC when:

- a simpler predictive baseline already solves the task
- the question is mainly about maximizing predictive accuracy quickly
- the sandbox resources are tight
- runtime latency matters more than posterior richness

## Priors

Prefer domain-informed priors when available.

If priors are elicited:

- document the source
- justify the scale
- show sensitivity if priors materially affect conclusions

## Computational caution

PyMC can be computationally expensive, especially with:

- many parameters
- hierarchical structure
- slow likelihoods
- long chains and many draws

If GPU/JAX acceleration is unavailable, prefer smaller models or simpler
alternatives unless the user explicitly needs Bayesian posterior inference.
