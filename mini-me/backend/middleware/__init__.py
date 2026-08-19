"""Cross-cutting agent middleware, split by concern.

Re-exports the middleware classes and builder helpers so callers can import
them from ``backend.middleware`` without knowing which submodule each lives in.
"""

from backend.middleware.artifacts import ArtifactCaptureMiddleware
from backend.middleware.claims import ClaimsRecorder
from backend.middleware.dataverse_first import (
    SearchBeforeRecommending,
    SearchResultsFile,
)
from backend.middleware.library_first import RunBeforeReporting
from backend.middleware.submit_first import (
    SubmitBeforeReporting,
    TheorizeBeforeReporting,
)
from backend.middleware.guardrails import (
    _build_filesystem_permissions,
    _build_guardrail_middleware,
    _build_pii_middleware,
)
from backend.middleware.project import ProjectSpineMiddleware
from backend.middleware.search_first import KeepSources, SearchBeforeCiting
from backend.middleware.tool_gate import Step, ToolsBeforeAnswering
from backend.middleware.sync import (
    FileSyncMiddleware,
    SandboxSyncMiddleware,
    _collect_sandbox_files,
)

__all__ = [
    "ArtifactCaptureMiddleware",
    "ClaimsRecorder",
    "FileSyncMiddleware",
    "KeepSources",
    "ProjectSpineMiddleware",
    "RunBeforeReporting",
    "SubmitBeforeReporting",
    "TheorizeBeforeReporting",
    "SearchBeforeCiting",
    "SearchBeforeRecommending",
    "SearchResultsFile",
    "SandboxSyncMiddleware",
    "Step",
    "ToolsBeforeAnswering",
    "_collect_sandbox_files",
    "_build_filesystem_permissions",
    "_build_pii_middleware",
    "_build_guardrail_middleware",
]
