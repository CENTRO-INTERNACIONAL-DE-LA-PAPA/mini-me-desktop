from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any

import pandas as pd

DEFAULT_MISSING_MARKERS = ["", "NA", "N/A", "NULL", "null", "nan", "NaN", "-999", "unknown"]


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


def write_dataframe(df: pd.DataFrame, path: str | Path, *, index: bool = False) -> None:
    file_path = Path(path)
    ensure_parent(file_path)
    suffix = file_path.suffix.lower()

    if suffix == ".csv":
        df.to_csv(file_path, index=index)
        return
    if suffix == ".tsv":
        df.to_csv(file_path, sep="\t", index=index)
        return
    if suffix == ".parquet":
        df.to_parquet(file_path, index=index)
        return
    if suffix in {".json", ".jsonl", ".ndjson"}:
        df.to_json(file_path, orient="records", lines=suffix in {".jsonl", ".ndjson"})
        return
    if suffix in {".xlsx", ".xls"}:
        df.to_excel(file_path, index=index)
        return

    raise ValueError(f"Unsupported output format: {file_path}")


def load_structured_file(path: str | Path) -> Any:
    file_path = Path(path)
    suffix = file_path.suffix.lower()
    text = file_path.read_text(encoding="utf-8")

    if suffix == ".json":
        return json.loads(text)
    if suffix in {".yaml", ".yml"}:
        try:
            import yaml
        except ImportError as exc:
            raise RuntimeError("PyYAML is required to read YAML config files.") from exc
        return yaml.safe_load(text)

    raise ValueError(f"Unsupported config format: {file_path}")


def ensure_parent(path: str | Path) -> None:
    Path(path).parent.mkdir(parents=True, exist_ok=True)


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
    if hasattr(value, "item"):
        return to_jsonable(value.item())
    if pd.isna(value):
        return None
    return str(value)


def missingness_by_column(df: pd.DataFrame) -> list[dict[str, Any]]:
    n_rows = max(len(df), 1)
    results: list[dict[str, Any]] = []
    for column in df.columns:
        missing_count = int(df[column].isna().sum())
        results.append(
            {
                "column": column,
                "missing_count": missing_count,
                "missing_pct": missing_count / n_rows,
            }
        )
    return results


def normalize_missing_markers(
    df: pd.DataFrame,
    *,
    columns: list[str] | None = None,
    markers: list[Any] | None = None,
) -> tuple[pd.DataFrame, int]:
    target_columns = columns or list(df.columns)
    target_markers = markers or DEFAULT_MISSING_MARKERS
    updated = df.copy()
    replacements = 0

    for column in target_columns:
        mask = updated[column].isin(target_markers)
        replacements += int(mask.sum())
        updated.loc[mask, column] = pd.NA

    return updated, replacements
