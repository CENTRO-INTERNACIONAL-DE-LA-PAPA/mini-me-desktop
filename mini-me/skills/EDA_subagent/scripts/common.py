from __future__ import annotations

import json
import math
import re
import warnings
from pathlib import Path
from typing import Any

import matplotlib.pyplot as plt
import numpy as np
import pandas as pd
import seaborn as sns


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
    plt.rcParams["axes.titlesize"] = 12
    plt.rcParams["axes.labelsize"] = 10


def save_current_figure(path: str | Path) -> None:
    file_path = Path(path)
    ensure_parent(file_path)
    with warnings.catch_warnings():
        warnings.simplefilter("ignore", UserWarning)
        plt.tight_layout()
    plt.savefig(file_path, dpi=150, bbox_inches="tight")
    plt.close()


def safe_name(value: str) -> str:
    cleaned = re.sub(r"[^A-Za-z0-9_.-]+", "_", value.strip())
    return cleaned.strip("_") or "item"


def infer_column_groups(df: pd.DataFrame) -> dict[str, list[str]]:
    numeric = list(df.select_dtypes(include=[np.number]).columns)
    boolean = list(df.select_dtypes(include=["bool"]).columns)
    datetime = list(df.select_dtypes(include=["datetime", "datetimetz"]).columns)
    categorical = [
        column
        for column in df.columns
        if column not in set(numeric + boolean + datetime)
    ]
    return {
        "numeric": numeric,
        "boolean": boolean,
        "datetime": datetime,
        "categorical": categorical,
    }


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


def missingness_summary(df: pd.DataFrame) -> list[dict[str, Any]]:
    denominator = max(len(df), 1)
    results: list[dict[str, Any]] = []
    for column in df.columns:
        missing_count = int(df[column].isna().sum())
        results.append(
            {
                "column": column,
                "missing_count": missing_count,
                "missing_pct": missing_count / denominator,
            }
        )
    return results


def top_correlated_pairs(df: pd.DataFrame, *, max_pairs: int = 8) -> list[dict[str, Any]]:
    numeric_df = df.select_dtypes(include=[np.number])
    if numeric_df.shape[1] < 2:
        return []

    corr = numeric_df.corr(numeric_only=True)
    pairs: list[dict[str, Any]] = []
    columns = list(corr.columns)
    for i, left in enumerate(columns):
        for right in columns[i + 1 :]:
            value = corr.loc[left, right]
            if pd.isna(value):
                continue
            pairs.append(
                {
                    "left": left,
                    "right": right,
                    "correlation": float(value),
                    "abs_correlation": float(abs(value)),
                }
            )

    pairs.sort(key=lambda item: item["abs_correlation"], reverse=True)
    return pairs[:max_pairs]
