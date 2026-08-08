"""Pytest bootstrap: ensure the project root is importable and tests use the
source tree (not any previously-installed copy of the ``backend`` package)."""

import sys
from pathlib import Path

_ROOT = str(Path(__file__).resolve().parent)
if _ROOT not in sys.path:
    sys.path.insert(0, _ROOT)
