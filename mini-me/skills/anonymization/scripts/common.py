from __future__ import annotations

import json
import math
from pathlib import Path
from typing import Any

import pandas as pd


def load_dataframe(path: str | Path, *, sheet_name: str | int | None = None) -> pd.DataFrame:
    """Read a table with every column as text, so IDs and phone numbers keep their digits."""
    file_path = Path(path)
    suffix = file_path.suffix.lower()

    if suffix == ".csv":
        return pd.read_csv(file_path, dtype=str, keep_default_na=False, na_values=[""])
    if suffix == ".tsv":
        return pd.read_csv(file_path, sep="\t", dtype=str, keep_default_na=False, na_values=[""])
    if suffix == ".parquet":
        return pd.read_parquet(file_path)
    if suffix in {".xlsx", ".xls"}:
        return pd.read_excel(file_path, sheet_name=sheet_name or 0, dtype=str)

    raise ValueError(f"Unsupported input format: {file_path}")


def write_dataframe(df: pd.DataFrame, path: str | Path) -> None:
    file_path = Path(path)
    file_path.parent.mkdir(parents=True, exist_ok=True)
    suffix = file_path.suffix.lower()

    if suffix == ".csv":
        df.to_csv(file_path, index=False)
    elif suffix == ".tsv":
        df.to_csv(file_path, sep="\t", index=False)
    elif suffix == ".parquet":
        df.to_parquet(file_path, index=False)
    elif suffix in {".xlsx", ".xls"}:
        df.to_excel(file_path, index=False)
    else:
        raise ValueError(f"Unsupported output format: {file_path}")


def load_structured_file(path: str | Path) -> Any:
    file_path = Path(path)
    text = file_path.read_text(encoding="utf-8")
    if file_path.suffix.lower() == ".json":
        return json.loads(text)
    if file_path.suffix.lower() in {".yaml", ".yml"}:
        try:
            import yaml
        except ImportError as exc:
            raise RuntimeError("PyYAML is required to read YAML config files.") from exc
        return yaml.safe_load(text)
    raise ValueError(f"Unsupported config format: {file_path}")


def write_json(data: Any, path: str | Path) -> None:
    file_path = Path(path)
    file_path.parent.mkdir(parents=True, exist_ok=True)
    file_path.write_text(json.dumps(to_jsonable(data), indent=2, ensure_ascii=True), encoding="utf-8")


def to_jsonable(value: Any) -> Any:
    if value is None or isinstance(value, (str, int, bool)):
        return value
    if isinstance(value, float):
        return None if math.isnan(value) or math.isinf(value) else value
    if isinstance(value, Path):
        return str(value)
    if isinstance(value, dict):
        return {str(key): to_jsonable(val) for key, val in value.items()}
    if isinstance(value, (list, tuple, set)):
        return [to_jsonable(item) for item in value]
    if hasattr(value, "item"):
        return to_jsonable(value.item())
    return str(value)


def parse_sheet_name(value: str | None) -> str | int | None:
    return int(value) if value and value.isdigit() else value


def refuse_to_overwrite(input_path: str | Path, output_path: str | Path) -> None:
    if Path(input_path).resolve() == Path(output_path).resolve():
        raise SystemExit("Refusing to overwrite the raw dataset: choose a different --output path.")
