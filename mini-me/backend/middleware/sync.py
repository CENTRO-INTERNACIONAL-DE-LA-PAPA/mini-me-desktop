"""Sandbox<->store sync middleware: surface artifacts, mirror skills+memories.

``FileSyncMiddleware`` surfaces newly produced sandbox files as artifacts after
every subagent turn; ``SandboxSyncMiddleware`` uploads skills + memories into
the sandbox before a run and mirrors mutated memories back to the LangGraph
store afterwards.
"""

import asyncio
import mimetypes
from pathlib import PurePosixPath
from typing import TYPE_CHECKING, Any

from langgraph.runtime import Runtime
from langchain.agents.middleware import AgentMiddleware
from deepagents.backends.utils import create_file_data

from backend.runtime import (
    _asearch_all,
    _memory_namespace,
    _require_server_identity,
    _safe_relative_path,
    _skills_namespace,
    _thread_skills_synced,
)
from backend.schemas import (
    ArtifactState,
    FileArtifactPayload,
    _infer_artifact_description,
    _is_supported_artifact_file,
    _normalize_artifact_path,
)

if TYPE_CHECKING:
    from backend.sandbox import LazyLangsmithSandbox


async def _collect_sandbox_files(
    *,
    sandbox_backend: "LazyLangsmithSandbox",
    work_dir: PurePosixPath,
    skills_dir: PurePosixPath,
    memories_dir: PurePosixPath,
) -> list[FileArtifactPayload]:
    """List supported artifact files in the sandbox work dir."""
    result = await sandbox_backend.aglob("**/*", str(work_dir))
    if result.error or not result.matches:
        return []

    files: list[FileArtifactPayload] = []
    excluded_roots = {
        skills_dir.as_posix().rstrip("/") + "/",
        memories_dir.as_posix().rstrip("/") + "/",
    }

    for match in result.matches:
        if match.get("is_dir"):
            continue

        absolute_path = _normalize_artifact_path(work_dir, match["path"])
        if absolute_path.startswith(tuple(excluded_roots)):
            continue
        if not _is_supported_artifact_file(absolute_path):
            continue

        relative_path = PurePosixPath(absolute_path).relative_to(work_dir).as_posix()
        if (
            relative_path.startswith(".")
            or "/." in relative_path
            or "__pycache__" in relative_path
        ):
            continue

        media_type, _ = mimetypes.guess_type(absolute_path)
        files.append(
            {
                "name": PurePosixPath(absolute_path).name,
                "path": absolute_path,
                "relative_path": relative_path,
                "media_type": media_type,
                "description": _infer_artifact_description(absolute_path),
            }
        )

    return files


class FileSyncMiddleware(AgentMiddleware[ArtifactState, Any, Any]):
    """Surface sandbox files as artifacts after every subagent turn.

    SandboxSyncMiddleware only fires when the *coordinator's* agent run
    ends, which in a multi-subagent flow is the very end of the chain.
    This lighter middleware runs after each subagent so the user sees
    files appear as they are produced (EDA plots, cleaned CSVs, model
    checkpoints) instead of waiting until the whole pipeline completes.

    File-collection only — no skill / memory sync.
    """

    state_schema = ArtifactState

    def __init__(self, sandbox_backend: "LazyLangsmithSandbox"):
        super().__init__()
        self.sandbox_backend = sandbox_backend
        self._work_dir: PurePosixPath | None = None
        self._skills_dir: PurePosixPath | None = None
        self._memories_dir: PurePosixPath | None = None

    async def _ensure_paths(self) -> None:
        if self._work_dir is not None:
            return
        if hasattr(self.sandbox_backend, "aget_work_dir"):
            work_dir_str = await self.sandbox_backend.aget_work_dir()
        else:
            work_dir_str = await asyncio.to_thread(
                self.sandbox_backend._sandbox.get_work_dir
            )
        self._work_dir = PurePosixPath(work_dir_str)
        self._skills_dir = self._work_dir / "skills"
        self._memories_dir = self._work_dir / "memories"

    async def aafter_agent(self, state: ArtifactState, runtime: Runtime) -> dict[str, Any] | None:
        await self._ensure_paths()
        assert self._work_dir is not None
        assert self._skills_dir is not None
        assert self._memories_dir is not None
        files = await _collect_sandbox_files(
            sandbox_backend=self.sandbox_backend,
            work_dir=self._work_dir,
            skills_dir=self._skills_dir,
            memories_dir=self._memories_dir,
        )
        if not files:
            return None
        return {
            "artifacts": {
                "datasets": [],
                "sources": [],
                "reports": [],
                "files": files,
            }
        }


class SandboxSyncMiddleware(AgentMiddleware[ArtifactState, Any, Any]):
    """Mirror skills + memories between the LangGraph store and the sandbox.

    Skills live in the store namespaced per assistant_id; memories live in
    the store namespaced per (assistant_id, user_id). Both are copied to
    the sandbox real filesystem so Python code executed via ``aexecute``
    can read/write them with ordinary file operations; mutations are
    mirrored back to the store on agent exit.
    """

    state_schema = ArtifactState

    def __init__(self, sandbox_backend: "LazyLangsmithSandbox"):
        super().__init__()
        self.sandbox_backend = sandbox_backend
        self.sandbox_work_dir: PurePosixPath | None = None
        self.sandbox_skills_dir: PurePosixPath | None = None
        self.sandbox_memories_dir: PurePosixPath | None = None
        self._uploaded_memory_contents: dict[str, bytes] = {}

    async def _ensure_paths(self) -> None:
        if self.sandbox_work_dir is not None:
            return
        work_dir_str = await self.sandbox_backend.aget_work_dir()
        self.sandbox_work_dir = PurePosixPath(work_dir_str)
        self.sandbox_skills_dir = self.sandbox_work_dir / "skills"
        self.sandbox_memories_dir = self.sandbox_work_dir / "memories"

    async def abefore_agent(self, state: ArtifactState, runtime: Runtime) -> None:
        """Upload skill scripts and memories into the sandbox.

        Skills are static during a process lifetime, so they are uploaded
        only once per (assistant_id, thread_id) pair — subsequent user
        messages skip the (54 files × hundreds of KB) sandbox round-trip
        entirely. Memories are mutable per turn and are always synced.
        """
        await self._ensure_paths()
        store = runtime.store
        if store is None:
            raise ValueError("A LangGraph store is required for sandbox sync")

        assistant_id, user_id = _require_server_identity(runtime)
        thread_id = getattr(self.sandbox_backend, "_thread_id", "") or ""
        skills_key = (assistant_id, thread_id)
        skip_skills = bool(thread_id) and skills_key in _thread_skills_synced

        self._uploaded_memory_contents = {}
        files: list[tuple[str, bytes]] = []
        if not skip_skills:
            for item in await _asearch_all(store, _skills_namespace(assistant_id)):
                rel_path = _safe_relative_path(item.key)
                files.append(
                    (str(self.sandbox_skills_dir / rel_path), item.value["content"].encode())
                )

        for item in await _asearch_all(store, _memory_namespace(assistant_id, user_id)):
            rel_path = _safe_relative_path(item.key)
            sandbox_path = str(self.sandbox_memories_dir / rel_path)
            content = item.value["content"].encode()
            files.append((sandbox_path, content))
            self._uploaded_memory_contents[sandbox_path] = content

        if files:
            await self.sandbox_backend.aupload_files(files)
        if not skip_skills and thread_id:
            _thread_skills_synced.add(skills_key)

    async def aafter_agent(self, state: ArtifactState, runtime: Runtime) -> dict[str, Any] | None:
        """Sync updated memories from the sandbox back to the LangGraph store.

        File collection is handled per-subagent by :class:`FileSyncMiddleware`;
        this hook is intentionally limited to memory persistence.
        """
        await self._ensure_paths()
        store = runtime.store
        if store is None:
            raise ValueError("A LangGraph store is required for sandbox sync")

        assistant_id, user_id = _require_server_identity(runtime)

        ls_result = await self.sandbox_backend.als(str(self.sandbox_memories_dir))
        if not ls_result.error and ls_result.entries:
            memory_paths = [entry["path"] for entry in ls_result.entries if not entry.get("is_dir")]
            if memory_paths:
                results = await self.sandbox_backend.adownload_files(memory_paths)
                for result in results:
                    if result.content is None:
                        continue
                    if self._uploaded_memory_contents.get(result.path) == result.content:
                        continue
                    rel_prefix = f"{self.sandbox_memories_dir.as_posix().rstrip('/')}/"
                    rel_path = _safe_relative_path(result.path.removeprefix(rel_prefix))
                    await store.aput(
                        _memory_namespace(assistant_id, user_id),
                        rel_path,
                        create_file_data(result.content.decode("utf-8")),
                    )

        return None
