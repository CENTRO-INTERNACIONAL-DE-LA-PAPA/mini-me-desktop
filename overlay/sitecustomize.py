"""Arms the desktop overlay at interpreter startup.

Python imports ``sitecustomize`` automatically if it can find one on ``sys.path``, so
putting this directory on ``PYTHONPATH`` is the whole injection mechanism — no edits
to the Mini-Me checkout, no wrapper script, no patched entry point.

**Caveat, deliberately not hidden:** Python imports only the *first* ``sitecustomize``
it finds. If something else in the environment ever ships one, whichever comes first on
``PYTHONPATH`` wins and the other is skipped. Nothing in the Mini-Me venv does today
(checked 2026-07-31), and the log line below is how you would notice.
"""

from __future__ import annotations

import sys

try:
    import minime_local
except Exception as error:  # pragma: no cover - only on a broken PYTHONPATH
    print(f"minime_local: overlay not importable ({error!r})", file=sys.stderr)
else:
    if minime_local.install():
        print("minime_local: overlay armed (host execution)", file=sys.stderr)
