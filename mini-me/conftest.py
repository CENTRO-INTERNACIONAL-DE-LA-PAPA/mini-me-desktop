"""Pytest bootstrap: ensure the project root is importable and tests use the
source tree (not any previously-installed copy of the ``backend`` package)."""

import sys
from pathlib import Path

_ROOT = str(Path(__file__).resolve().parent)
if _ROOT not in sys.path:
    sys.path.insert(0, _ROOT)

# The desktop overlay (`overlay/minime_local`), which ships in the bundle and is injected onto the
# backend's PYTHONPATH at launch. It sits beside this checkout rather than inside it, and until now
# had no tests at all — a package that rewrites how every command runs and that nothing asserted.
_OVERLAY = Path(__file__).resolve().parent.parent / "overlay"
if _OVERLAY.is_dir() and str(_OVERLAY) not in sys.path:
    sys.path.insert(0, str(_OVERLAY))
