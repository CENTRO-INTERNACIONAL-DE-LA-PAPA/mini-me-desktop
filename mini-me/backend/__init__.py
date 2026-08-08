"""Mini-Me backend package.

Importing this package eagerly loads :mod:`backend.runtime` so that
``load_dotenv()`` and the shared process-wide state (the active-sandbox
ContextVar, the MCP client caches) are initialised before any other backend
module reads the environment. This preserves the import-time ordering the
codebase relied on when everything lived in ``ask_the_data.py``.
"""

from . import runtime as _runtime  # noqa: F401  (imported for load_dotenv ordering)
