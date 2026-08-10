"""Cross-cutting agent middleware, split by concern.

Re-exports the middleware classes and builder helpers so callers can import
them from ``backend.middleware`` without knowing which submodule each lives in.
"""

from backend.middleware.artifacts import ArtifactCaptureMiddleware
from backend.middleware.dataverse_first import (
    FixedSearchFilename,
    SearchBeforeRecommending,
)
from backend.middleware.guardrails import (
    _build_filesystem_permissions,
    _build_guardrail_middleware,
    _build_pii_middleware,
)
from backend.middleware.project import ProjectSpineMiddleware
from backend.middleware.search_first import SearchBeforeCiting
from backend.middleware.tool_gate import Step, ToolsBeforeAnswering
from backend.middleware.sync import (
    FileSyncMiddleware,
    SandboxSyncMiddleware,
    _collect_sandbox_files,
)

__all__ = [
    "ArtifactCaptureMiddleware",
    "FileSyncMiddleware",
    "FixedSearchFilename",
    "ProjectSpineMiddleware",
    "SearchBeforeCiting",
    "SearchBeforeRecommending",
    "SandboxSyncMiddleware",
    "Step",
    "ToolsBeforeAnswering",
    "_collect_sandbox_files",
    "_build_filesystem_permissions",
    "_build_pii_middleware",
    "_build_guardrail_middleware",
]
