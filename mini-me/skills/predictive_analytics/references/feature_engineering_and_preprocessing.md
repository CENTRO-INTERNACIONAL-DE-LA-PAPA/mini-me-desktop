# Feature Engineering And Preprocessing

Use preprocessing only when it matches the model family and the predictive
problem.

## Transformations

### Box-Cox

Use when:

- a continuous predictor or target is strictly positive
- variance stabilization or skew reduction helps a linear-style model

Do not use when zeros or negative values are present.

### Yeo-Johnson

Use when:

- a continuous variable may include zeros or negative values
- you need a power transform with fewer positivity constraints

## Missing-data handling

Use missing-data methods that match both the model family and the predictive
goal.

### Simple imputation

Use simple strategies when:

- missingness is limited
- speed and robustness matter more than complex imputation
- the model can work well with simple fill values plus indicators

Examples:

- median or mean imputation for numeric variables
- mode or constant-category imputation for categorical variables

### Missingness indicators

Consider adding missingness indicators when the fact that a value is missing may
itself be predictive.

### MICE-style imputation with sklearn

For predictive preprocessing, sklearn `IterativeImputer` is the practical
MICE-style option.

Use it when:

- multivariable relationships are informative for imputing missing values
- the extra compute and complexity are justified
- a pipeline-based train/validation workflow can be enforced cleanly

Rules:

- fit the imputer only on training data
- apply the fitted imputer to validation and test data
- do not fit imputation on the full dataset before splitting

Important caveat:

- sklearn `IterativeImputer` is a practical MICE-like method for prediction
  workflows, but it is not the same thing as a full multiple-imputation
  inference workflow with Rubin-style pooling

When not to default to it:

- missingness is extreme
- a model already handles missing values well
- simple imputation plus indicators is sufficient
- runtime or stability constraints make iterative imputation too expensive

## Scaling

### Standardization

Use for:

- linear models
- regularized models
- distance-based methods
- neural models

### Normalization

Use when vector magnitude needs to be comparable across rows, but do not assume
it is the default for tabular prediction.

### MaxAbsScaler

Useful for sparse or sign-preserving scaled data.

### RobustScaler

Useful when outliers would distort mean/standard-deviation scaling.

## Binning

Use sparingly.

Good reasons:

- domain thresholds matter
- monotonic or piecewise effects are easier to model or explain
- rare extreme values need more stable grouping

Do not bin automatically if it throws away signal.

## Encoding

### One-hot encoding

Safe default for low- to moderate-cardinality categorical variables.

### Rare category encoding

Useful when long-tail categories create unstable estimates or too many dummy
columns.

### Frequency encoding

Useful for high-cardinality categories when count information is predictive.

### Target encoding

Useful for high-cardinality categories, but high leakage risk.

Rules:

- fit only on training folds
- never compute using the full dataset before splitting
- use smoothing or shrinkage when possible

### Multi-hot encoding

Use for set-valued or multi-label categorical inputs where one row may contain
multiple categories.

## Temporal features

For forecasting or time-aware prediction, consider:

- lags
- rolling summaries
- expanding summaries
- seasonality indicators
- holiday/event flags

All must be computed without access to future information.

## VIF

Use VIF when:

- coefficient stability matters
- the model family is linear or GLM-style
- multicollinearity may make interpretation unstable

Do not treat VIF as a mandatory preprocessing step for all predictive models.

## Model-specific rule

Do not apply the same preprocessing pipeline to every model family.

- GLMs and regularized linear models often benefit from transformation and
  scaling
- tree models usually do not need scaling
- count-aware GLMs should preserve count interpretation
- TabPFN should not be burdened with unnecessary handcrafted preprocessing
