#!/usr/bin/env python
"""Report which columns of a table may hold personal data, without printing any value."""
from __future__ import annotations

import argparse
import json
import re
import unicodedata
from typing import Any

import pandas as pd

from common import load_dataframe, parse_sheet_name, write_json

# Column-name hints, in English and Spanish. Matched on whole words of the normalized name.
NAME_HINTS: dict[str, tuple[str, list[str]]] = {
    "person_name": ("direct", ["name", "nombre", "nombres", "apellido", "apellidos", "surname", "respondent", "encuestado", "farmer", "agricultor", "productor", "jefe", "head"]),
    "phone": ("direct", ["phone", "telefono", "celular", "mobile", "movil", "whatsapp", "tel"]),
    "email": ("direct", ["email", "correo", "mail"]),
    "national_id": ("direct", ["dni", "nin", "passport", "pasaporte", "cedula", "ruc", "ssn", "aadhaar", "documento"]),
    "address": ("direct", ["address", "direccion", "domicilio", "street", "calle"]),
    "coordinates": ("direct", ["lat", "latitude", "latitud", "lon", "lng", "long", "longitude", "longitud", "gps", "coord", "coordinates", "coordenadas", "utm", "easting", "northing"]),
    "birth_date": ("quasi", ["birth", "nacimiento", "dob", "birthdate"]),
    "age": ("quasi", ["age", "edad"]),
    "sex": ("quasi", ["sex", "sexo", "gender", "genero"]),
    "small_place": ("quasi", ["village", "comunidad", "community", "caserio", "anexo", "centro", "poblado", "localidad", "aldea", "barrio", "sector"]),
    "place": ("quasi", ["district", "distrito", "province", "provincia", "municipality", "municipio", "county", "ward", "parish"]),
    "identifier": ("quasi", ["id", "code", "codigo", "identifier", "identificador"]),
    "ethnicity": ("quasi", ["ethnicity", "etnia", "ethnic", "tribe", "lengua", "language", "religion"]),
    "sensitive": ("sensitive", ["income", "ingreso", "ingresos", "salary", "salario", "debt", "deuda", "health", "salud", "disease", "enfermedad", "disability", "discapacidad", "hiv", "vih"]),
    "free_text": ("direct", ["comment", "comments", "comentario", "comentarios", "observacion", "observaciones", "notes", "notas", "remarks", "specify", "especifique"]),
}

# Checked in this order; a value counts for the first pattern it matches, so a date is never
# also counted as a phone number.
VALUE_PATTERNS: dict[str, re.Pattern[str]] = {
    "email": re.compile(r"^[\w.+-]+@[\w-]+\.[\w.-]+$"),
    "url": re.compile(r"^https?://", re.I),
    "ip_address": re.compile(r"^\d{1,3}(\.\d{1,3}){3}$"),
    "date": re.compile(r"^\d{4}-\d{1,2}-\d{1,2}|^\d{1,2}[/-]\d{1,2}[/-]\d{2,4}"),
    "coordinate": re.compile(r"^-?\d{1,3}\.\d{3,}$"),
    "decimal": re.compile(r"^-?\d+[.,]\d+$"),
    "national_id": re.compile(r"^\d{8}$|^\d{11}$|^[A-Z]{1,2}\d{6,9}$"),
    "phone": re.compile(r"^\+?(?:[\s().-]*\d){9,15}$"),
}
IDENTIFYING_PATTERNS = {"email", "ip_address", "national_id", "phone"}

SUGGESTED = {
    "direct": "drop, or pseudonymize if it links records",
    "quasi": "generalize (bin, round, or merge small categories), then check k-anonymity",
    "sensitive": "keep only if the analysis needs it; check l-diversity",
    "none": "keep",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input_path", help="Path to the input table.")
    parser.add_argument("--output", required=True, help="Path to write the detection report JSON.")
    parser.add_argument("--sheet-name", help="Excel sheet name or zero-based index.")
    parser.add_argument("--sample", type=int, default=5000, help="Rows to scan for value patterns (default 5000).")
    parser.add_argument("--presidio", action="store_true", help="Also run Presidio named-entity detection on text columns.")
    return parser.parse_args()


def _words(column: str) -> set[str]:
    plain = unicodedata.normalize("NFKD", str(column)).encode("ascii", "ignore").decode()
    plain = re.sub(r"([a-z])([A-Z])", r"\1 \2", plain)
    return set(re.split(r"[^a-z0-9]+", plain.lower())) - {""}


def _name_hints(column: str) -> list[str]:
    words = _words(column)
    return [hint for hint, (_, keys) in NAME_HINTS.items() if words & set(keys)]


def _pattern_rates(values: pd.Series) -> dict[str, float]:
    rates: dict[str, float] = {}
    if values.empty:
        return rates
    first_match = values.map(lambda v: next((name for name, pattern in VALUE_PATTERNS.items() if pattern.search(v)), ""))
    for name, count in first_match.value_counts().items():
        rate = count / len(values)
        if name not in {"", "decimal"} and rate >= 0.05:
            rates[str(name)] = round(float(rate), 3)
    return rates


def _presidio_rates(values: pd.Series, limit: int = 200) -> dict[str, float]:
    try:
        from presidio_analyzer import AnalyzerEngine
    except ImportError as exc:
        raise SystemExit("--presidio needs the presidio-analyzer package and a spaCy model.") from exc
    engine = _presidio_rates.__dict__.setdefault("engine", AnalyzerEngine())
    sample = values.head(limit)
    hits: dict[str, int] = {}
    for text in sample:
        for entity in {r.entity_type for r in engine.analyze(text=text, language="en") if r.score >= 0.6}:
            hits[entity] = hits.get(entity, 0) + 1
    return {entity: round(count / len(sample), 3) for entity, count in hits.items() if count / len(sample) >= 0.1}


def scan_column(series: pd.Series, n_rows: int, sample: int, presidio: bool) -> dict[str, Any]:
    values = series.dropna().astype(str).str.strip()
    values = values[values != ""].head(sample)
    hints = _name_hints(str(series.name))
    patterns = _pattern_rates(values)
    numeric_rate = float(pd.to_numeric(values, errors="coerce").notna().mean()) if len(values) else 0.0
    unique_ratio = float(series.nunique(dropna=True) / max(series.notna().sum(), 1))
    mean_length = float(values.str.len().mean()) if len(values) else 0.0
    free_text = numeric_rate < 0.5 and mean_length >= 30 and unique_ratio > 0.5

    reasons: list[str] = [f"name suggests {hint}" for hint in hints]
    reasons += [f"{int(rate * 100)}% of values look like {name}" for name, rate in patterns.items()]
    if free_text:
        reasons.append("long free text, mostly unique")

    entities: dict[str, float] = {}
    if presidio and numeric_rate < 0.5 and len(values):
        entities = _presidio_rates(values)
        reasons += [f"Presidio finds {entity} in {int(rate * 100)}% of sampled values" for entity, rate in entities.items()]

    categories = {NAME_HINTS[hint][0] for hint in hints}
    if IDENTIFYING_PATTERNS & patterns.keys():
        categories.add("direct")
    if {"PERSON", "EMAIL_ADDRESS", "PHONE_NUMBER"} & entities.keys() or free_text:
        categories.add("direct")
    if "coordinate" in patterns and "coordinates" in hints:
        categories.add("direct")
    if "date" in patterns and not categories:
        categories.add("quasi")
    # A text column where almost every value is unique can single out a person even with no other clue.
    if not categories and unique_ratio > 0.9 and numeric_rate < 0.5 and n_rows >= 20:
        categories.add("quasi")
        reasons.append("text values are almost all unique")

    category = next((c for c in ("direct", "quasi", "sensitive") if c in categories), "none")
    return {
        "column": str(series.name),
        "category": category,
        "reasons": reasons,
        "unique_ratio": round(unique_ratio, 3),
        "missing_pct": round(float(series.isna().mean()), 3),
        "suggested_treatment": SUGGESTED[category],
    }


def main() -> None:
    args = parse_args()
    df = load_dataframe(args.input_path, sheet_name=parse_sheet_name(args.sheet_name))
    columns = [scan_column(df[column], len(df), args.sample, args.presidio) for column in df.columns]
    flagged = [c for c in columns if c["category"] != "none"]
    report = {
        "input_path": args.input_path,
        "n_rows": len(df),
        "n_columns": len(df.columns),
        "presidio": args.presidio,
        "summary": {
            "direct": [c["column"] for c in flagged if c["category"] == "direct"],
            "quasi": [c["column"] for c in flagged if c["category"] == "quasi"],
            "sensitive": [c["column"] for c in flagged if c["category"] == "sensitive"],
        },
        "columns": columns,
        "note": "This report holds column names, counts and rates only. No cell value is included.",
    }
    write_json(report, args.output)
    print(json.dumps(report["summary"], indent=2))


if __name__ == "__main__":
    main()
