# Class Imbalance

Use this reference whenever the classification target is materially imbalanced.

## First choose the right metrics

Do not optimize plain accuracy when the classes are imbalanced.

Prefer:

- precision
- recall
- F1 when a balance is needed
- PR AUC when positive cases are rare
- ROC AUC when ranking quality matters
- log loss and calibration when predicted probabilities are used operationally

## Validation rules

- use stratified splits when appropriate
- preserve grouped or temporal structure when required
- never resample before the train/validation split

All oversampling or undersampling steps must happen only on the training fold.

## Handling options

### Class weights

Good first option for many linear and tree-based classifiers.

### Threshold tuning

Useful when the model probabilities are reasonable but the operational tradeoff
needs a different decision threshold.

### SMOTE

Useful when the minority class needs synthetic support and the feature space is
appropriate for interpolation.

Do not treat SMOTE as a default when:

- the data is time-ordered
- grouped structure must be preserved
- categorical structure dominates and interpolation would be unrealistic

### ADASYN

Useful when you want adaptive synthetic oversampling focused on harder minority
examples.

Use the same cautions as SMOTE. It is not a safe default for temporal or
group-structured data.

### Tomek links

Useful for cleaning ambiguous boundary examples, often as a post-processing step
with oversampling.

### Combined methods

Possible combinations include:

- SMOTE + Tomek links
- SMOTE + undersampling
- ADASYN + Tomek Links

Do not stack resampling methods blindly. Compare them under the same validation
design.

## Model choice rule

Some models handle imbalance more naturally than others. Prefer a simpler
imbalance-aware model before adding complex resampling pipelines if performance
is already adequate.

For temporal, grouped, or heavily categorical data, prefer class weights,
threshold tuning, stratified/group-safe validation, or category-aware methods
before generic synthetic oversampling.

## Reporting rule

Always state:

- the class prevalence
- the metric used for model selection
- whether resampling, weights, or threshold tuning were applied
- whether calibration was checked
