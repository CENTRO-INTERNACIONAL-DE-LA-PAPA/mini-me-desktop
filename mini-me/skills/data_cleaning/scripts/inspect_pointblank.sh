#!/usr/bin/env bash
set -euo pipefail

python - <<'PY'
import inspect
import pointblank as pb

print(f"pointblank version: {getattr(pb, '__version__', 'unknown')}")
print()

exports = [
    "DataScan",
    "Validate",
    "Schema",
    "col_summary_tbl",
    "missing_vals_tbl",
    "get_validation_summary",
]

for name in exports:
    obj = getattr(pb, name, None)
    if obj is None:
        continue
    try:
        signature = inspect.signature(obj)
    except Exception:
        signature = "(signature unavailable)"
    print(f"{name}{signature}")

print()
print("Validate methods:")
for name in sorted(n for n in dir(pb.Validate) if not n.startswith("_")):
    print(f"  - {name}")
PY
