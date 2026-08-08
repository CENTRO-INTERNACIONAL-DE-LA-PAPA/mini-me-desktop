from __future__ import annotations

import json
import math
import warnings
from pathlib import Path
from typing import Any

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns
import statsmodels.api as sm
from statsmodels.stats.outliers_influence import variance_inflation_factor


def load_dataframe(path: str | Path, *, sheet_name: str | int | None = None) -> pd.DataFrame:
    file_path = Path(path)
    suffix = file_path.suffix.lower()

    if suffix == ".csv":
        return pd.read_csv(file_path)
    if suffix == ".tsv":
        return pd.read_csv(file_path, sep="\t")
    if suffix == ".parquet":
        return pd.read_parquet(file_path)
    if suffix in {".json", ".jsonl", ".ndjson"}:
        return pd.read_json(file_path, lines=suffix in {".jsonl", ".ndjson"})
    if suffix in {".xlsx", ".xls"}:
        return pd.read_excel(file_path, sheet_name=sheet_name or 0)

    raise ValueError(f"Unsupported input format: {file_path}")


def ensure_parent(path: str | Path) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)


def ensure_directory(path: str | Path) -> Path:
    directory = Path(path)
    directory.mkdir(parents=True, exist_ok=True)
    return directory


def write_json(data: Any, path: str | Path) -> None:
    file_path = Path(path)
    ensure_parent(file_path)
    file_path.write_text(json.dumps(to_jsonable(data), indent=2, ensure_ascii=True), encoding="utf-8")


def to_jsonable(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, bool)):
        return value
    if isinstance(value, float):
        if math.isnan(value) or math.isinf(value):
            return None
        return value
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, pd.DataFrame):
        return [to_jsonable(record) for record in value.to_dict(orient="records")]
    if isinstance(value, pd.Series):
        return {str(key): to_jsonable(val) for key, val in value.to_dict().items()}
    if isinstance(value, dict):
        return {str(key): to_jsonable(val) for key, val in value.items()}
    if isinstance(value, (list, tuple, set)):
        return [to_jsonable(item) for item in value]
    if isinstance(value, pd.Timestamp):
        return value.isoformat()
    if hasattr(value, "tolist") and not isinstance(value, str):
        return to_jsonable(value.tolist())
    if hasattr(value, "item"):
        return to_jsonable(value.item())
    if pd.isna(value):
        return None
    return str(value)


def setup_plotting() -> None:
    sns.set_theme(style="whitegrid", context="notebook")
    plt.rcParams["figure.figsize"] = (8, 5)


def save_current_figure(path: str | Path) -> None:
    file_path = Path(path)
    ensure_parent(file_path)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        plt.tight_layout()
    plt.savefig(file_path, dpi=150, bbox_inches="tight")
    plt.close()


def safe_name(value: str) -> str:
    return "".join(ch if ch.isalnum() or ch in "._-" else "_" for ch in value).strip("_") or "item"


def maybe_parse_datetimes(df: pd.DataFrame, *, max_parse_fraction: float = 0.85) -> pd.DataFrame:
    updated = df.copy()
    for column in updated.columns:
        if pd.api.types.is_object_dtype(updated[column]) or pd.api.types.is_string_dtype(updated[column]):
            with warnings.catch_warnings():
                warnings.simplefilter("ignore", UserWarning)
                parsed = pd.to_datetime(updated[column], errors="coerce")
            non_null = updated[column].notna().sum()
            if non_null == 0:
                continue
            parsed_fraction = parsed.notna().sum() / non_null
            if parsed_fraction >= max_parse_fraction:
                updated[column] = parsed
    return updated


def is_binary_series(series: pd.Series) -> bool:
    values = set(series.dropna().astype(float).unique().tolist())
    return values.issubset({0.0, 1.0}) and bool(values)


def prepare_model_frame(
    df: pd.DataFrame,
    outcome: str,
    predictors: list[str],
    *,
    categorical: list[str] | None = None,
) -> tuple[pd.DataFrame, pd.Series, pd.DataFrame, list[str]]:
    frame = df[[outcome, *predictors]].dropna().copy()
    inferred_categorical = list(categorical or [
        column
        for column in predictors
        if not pd.api.types.is_numeric_dtype(frame[column]) and not pd.api.types.is_bool_dtype(frame[column])
    ])
    inferred_categorical = [column for column in inferred_categorical if column in predictors]
    X = pd.get_dummies(frame[predictors], columns=inferred_categorical, drop_first=True, dtype=float)
    y = frame[outcome]
    if is_binary_series(y):
        y = y.astype(int)
    else:
        y = pd.to_numeric(y, errors="coerce")
    valid_rows = y.notna() & X.notna().all(axis=1)
    frame = frame.loc[valid_rows].copy()
    y = y.loc[valid_rows].copy()
    X = X.loc[valid_rows].copy()
    return frame, y, X, inferred_categorical


def fit_regression(
    df: pd.DataFrame,
    outcome: str,
    predictors: list[str],
    *,
    categorical: list[str] | None = None,
) -> tuple[str, Any, pd.DataFrame, pd.Series, pd.DataFrame]:
    frame, y, X, _ = prepare_model_frame(df, outcome, predictors, categorical=categorical)
    X_const = sm.add_constant(X, has_constant="add")

    if is_binary_series(y):
        model = sm.GLM(y.astype(float), X_const, family=sm.families.Binomial())
        result = model.fit()
        model_type = "logit"
    else:
        model = sm.OLS(y.astype(float), X_const)
        result = model.fit()
        model_type = "ols"

    return model_type, result, frame, y, X_const


def coefficient_table(result: Any) -> pd.DataFrame:
    conf_int = result.conf_int()
    table = pd.DataFrame(
        {
            "term": result.params.index,
            "coef": result.params.values,
            "std_err": result.bse.values,
            "stat": result.tvalues.values if hasattr(result, "tvalues") else result.pvalues.values,
            "p_value": result.pvalues.values,
            "ci_low": conf_int.iloc[:, 0].values,
            "ci_high": conf_int.iloc[:, 1].values,
        }
    )
    return table


def compute_vif(X_const: pd.DataFrame) -> pd.DataFrame:
    if "const" in X_const.columns:
        design = X_const.drop(columns=["const"])
    else:
        design = X_const.copy()
    if design.shape[1] < 2:
        return pd.DataFrame(columns=["feature", "vif"])
    values = design.astype(float).values
    vif_rows = []
    for index, feature in enumerate(design.columns):
        vif_rows.append({"feature": feature, "vif": float(variance_inflation_factor(values, index))})
    return pd.DataFrame(vif_rows)


def grouped_numeric_summary(df: pd.DataFrame, group_col: str, outcome_col: str) -> pd.DataFrame:
    return (
        df.groupby(group_col)[outcome_col]
        .agg(["count", "mean", "median", "std", "min", "max"])
        .reset_index()
    )


def effect_size_cohens_d(group_a: pd.Series, group_b: pd.Series) -> float | None:
    a = group_a.dropna().astype(float)
    b = group_b.dropna().astype(float)
    if len(a) < 2 or len(b) < 2:
        return None
    pooled = math.sqrt(((len(a) - 1) * a.var(ddof=1) + (len(b) - 1) * b.var(ddof=1)) / (len(a) + len(b) - 2))
    if pooled == 0:
        return None
    return float((a.mean() - b.mean()) / pooled)
