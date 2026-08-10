"""LangSmith Sandbox backend for the deepagents virtual filesystem.

A thread's code execution and file I/O all run inside a per-thread LangSmith
Sandbox. ``LazyLangsmithSandbox`` defers acquiring that sandbox until the first
async I/O call, so read-only requests (history fetches, thread switches, page
reloads) never pay sandbox startup cost. It implements the deepagents
``BaseSandbox`` protocol so the filesystem middlewares (``FileSyncMiddleware``,
``SandboxSyncMiddleware``) and MCP helpers work without knowing the backend.
"""

import asyncio
import os
import re
import shlex
from datetime import datetime
from pathlib import PurePosixPath
from typing import Any

from langsmith.sandbox import (
    AsyncSandbox,
    AsyncSandboxClient,
    ResourceNotFoundError,
)

from deepagents.backends.protocol import (
    EditResult,
    ExecuteResponse,
    FileData,
    FileDownloadResponse,
    FileInfo,
    FileUploadResponse,
    GlobResult,
    GrepMatch,
    GrepResult,
    LsResult,
    ReadResult,
    WriteResult,
)
from deepagents.backends.sandbox import BaseSandbox


EXECUTE_OUTPUT_MAX_BYTES = 32_000
EXECUTE_OUTPUT_HEAD_BYTES = 16_000
EXECUTE_OUTPUT_TAIL_BYTES = 12_000


def _truncate_execute_response(response):
    """Cap aexecute output so chatty libraries cannot poison agent state.

    PyMC samplers, scikit-learn verbose modes, and other numerical libs
    can emit MB-scale stdout/stderr even with progressbar/verbose flags.
    That output is captured verbatim and returned to the LLM — and then
    persisted in LangGraph state, re-serialized on every stream tick.
    A multi-MB tool message crashes the browser renderer (SIGILL in V8)
    long before ReactMarkdown gets involved.

    We keep head + tail so the model still sees the start (which usually
    indicates what ran) and the end (which usually contains the final
    summary / error), and we mark the response as truncated so the model
    knows the middle was elided.
    """
    output = getattr(response, "output", None)
    if not isinstance(output, str) or len(output.encode("utf-8")) <= EXECUTE_OUTPUT_MAX_BYTES:
        return response

    encoded = output.encode("utf-8")
    head = encoded[:EXECUTE_OUTPUT_HEAD_BYTES].decode("utf-8", errors="ignore")
    tail = encoded[-EXECUTE_OUTPUT_TAIL_BYTES:].decode("utf-8", errors="ignore")
    dropped_kb = (len(encoded) - EXECUTE_OUTPUT_HEAD_BYTES - EXECUTE_OUTPUT_TAIL_BYTES) // 1024
    response.output = (
        f"{head}\n\n"
        f"...[output truncated — {dropped_kb} KB elided to protect agent state; "
        f"redirect verbose library output to a log file in the work dir if you need the full trace]...\n\n"
        f"{tail}"
    )
    response.truncated = True
    return response


SANDBOX_WORK_DIR = "/workspace"
SANDBOX_NAME_PREFIX = "minime-"
SANDBOX_SNAPSHOT_NAME = os.getenv("MINIME_SANDBOX_SNAPSHOT", "mini-me-base")
SANDBOX_IDLE_TTL_SECONDS = 600  # stop after 10 min idle
SANDBOX_DELETE_AFTER_STOP_SECONDS = 14 * 24 * 3600  # delete 14 days after stop


def _sandbox_name_for_thread(thread_id: str) -> str:
    """Map a LangGraph thread_id to a valid LangSmith sandbox name.

    LangSmith requires lowercase, digits, hyphens, not ending with hyphen,
    max 63 chars. Thread IDs are usually UUIDs which already satisfy this
    after lowercasing; non-conforming characters are replaced with hyphens.
    """
    raw = f"{SANDBOX_NAME_PREFIX}{thread_id}".lower()
    cleaned = re.sub(r"[^a-z0-9-]+", "-", raw)[:63].strip("-")
    return cleaned or f"{SANDBOX_NAME_PREFIX}fallback"


def _emit_sandbox_status(state: str, message: str = "") -> None:
    """Emit a custom 'sandbox_status' event to the LangGraph stream.

    Best-effort: when no stream writer is available (outside a graph run,
    e.g. during HTTP-route resolution), this is a no-op. The frontend
    listens on the 'custom' stream-mode channel for these events.

    Args:
        state: one of 'preparing', 'ready', 'error'.
        message: human-readable detail shown in the UI.
    """
    try:
        from langgraph.config import get_stream_writer
    except Exception:
        return
    try:
        writer = get_stream_writer()
    except Exception:
        return
    if writer is None:
        return
    try:
        writer({"sandbox_status": {"state": state, "message": message}})
    except Exception:
        # Never let a status emission failure break agent execution.
        pass


class LazyLangsmithSandbox(BaseSandbox):
    """Defers LangSmith Sandbox acquisition until the first async I/O call.

    Lets the LangGraph factory return immediately without paying sandbox
    startup cost on read-only requests (history fetches, thread switches,
    page reloads). The real sandbox is created on first node execution.

    Subclasses ``BaseSandbox`` so ``isinstance(lazy, SandboxBackendProtocol)``
    is true. Without this, ``FilesystemMiddleware.supports_execution`` returns
    False and the ``execute`` tool is stripped from the agent and subagents.

    The class mirrors the deepagents ``BaseSandbox`` protocol so middlewares
    (``FileSyncMiddleware``, ``SandboxSyncMiddleware``) and helpers
    (``_save_mcp_to_sandbox``) work without knowing the backend.
    """

    def __init__(self, thread_id: str):
        self._thread_id = thread_id
        self._sandbox_name = _sandbox_name_for_thread(thread_id)
        self._client: AsyncSandboxClient | None = None
        self._sandbox: AsyncSandbox | None = None
        self._lock = asyncio.Lock()

    async def aresolve(self) -> AsyncSandbox:
        if self._sandbox is not None and self._sandbox.status == "ready":
            return self._sandbox
        async with self._lock:
            if self._sandbox is not None and self._sandbox.status == "ready":
                return self._sandbox
            client = self._client or AsyncSandboxClient()
            self._client = client
            try:
                created_fresh = False
                try:
                    sandbox = await client.get_sandbox(self._sandbox_name)
                except ResourceNotFoundError:
                    _emit_sandbox_status("preparing", "Creating sandbox…")
                    sandbox = await self._create_sandbox(client)
                    created_fresh = True

                # If the sandbox exists but was idled / stopped, restart it
                # before issuing any commands. start_sandbox polls until ready.
                if not created_fresh and sandbox.status != "ready":
                    _emit_sandbox_status(
                        "preparing",
                        f"Resuming sandbox (was {sandbox.status})…",
                    )
                    sandbox = await client.start_sandbox(
                        self._sandbox_name, timeout=120
                    )

                self._sandbox = sandbox
                # Ensure work dir exists (cheap if already there).
                await sandbox.run(
                    f"mkdir -p {shlex.quote(SANDBOX_WORK_DIR)}",
                    timeout=10,
                )
                # Persist the caller's Asta token into the sandbox shell profile
                # so an interactive shell the user opens (to debug a run) is
                # authenticated too — not just the per-`execute` commands, which
                # get ASTA_TOKEN injected on each call. Without this, opening the
                # sandbox terminal always shows `asta` as logged-out even right
                # after refreshing the token. Best-effort.
                await self._persist_asta_token(sandbox)
                _emit_sandbox_status("ready", "Sandbox ready")
                return sandbox
            except Exception as exc:
                _emit_sandbox_status("error", f"Sandbox unavailable: {exc}")
                raise

    async def _persist_asta_token(self, sandbox: AsyncSandbox) -> None:
        """Write the active Asta token into the sandbox shell profile.

        Per-`execute` commands get ``ASTA_TOKEN`` injected on each call
        (``_aexecute_core``), but an interactive shell the user opens inherits
        nothing — so ``asta`` looks logged-out there. This writes the token to a
        root-only ``/etc/profile.d`` file plus a one-line loader in ``~/.bashrc``
        so both login and interactive shells pick it up. Re-run every resolve so a
        refreshed token propagates; best-effort and never blocks readiness.
        """
        from backend.runtime import _active_asta_token  # noqa: PLC0415 — avoid cycle

        token = _active_asta_token.get()
        if not token:
            return
        loader = "[ -f /etc/profile.d/asta_token.sh ] && . /etc/profile.d/asta_token.sh"
        script = (
            "umask 077; "
            f"printf 'export ASTA_TOKEN=%s\\n' {shlex.quote(token)} "
            "> /etc/profile.d/asta_token.sh; "
            "chmod 600 /etc/profile.d/asta_token.sh; "
            f"grep -qF {shlex.quote(loader)} /root/.bashrc 2>/dev/null "
            f"|| printf '%s\\n' {shlex.quote(loader)} >> /root/.bashrc"
        )
        try:
            await sandbox.run(script, timeout=10)
        except Exception:  # noqa: BLE001 — never block sandbox readiness on this
            pass

    async def try_resolve(self) -> AsyncSandbox | None:
        """Return the underlying sandbox if it already exists; do NOT create.

        Used by routes (file download, image fetch, delete) that should 404
        rather than spin up a fresh sandbox for a thread the user never ran.
        """
        if self._sandbox is not None:
            return self._sandbox
        async with self._lock:
            if self._sandbox is not None:
                return self._sandbox
            client = AsyncSandboxClient()
            try:
                sandbox = await client.get_sandbox(self._sandbox_name)
            except ResourceNotFoundError:
                return None
            self._client = client
            self._sandbox = sandbox
            return sandbox

    async def aresume(self) -> bool:
        """Resume an existing (possibly idled) sandbox; do NOT create.

        Returns True once the sandbox is ready, False if no sandbox exists
        for this thread (expired / never created). Used by the
        ``POST /sandboxes/{thread_id}/start`` route so the frontend can
        bring a past conversation's files back without starting a run.
        """
        sb = await self.try_resolve()
        if sb is None:
            return False
        if sb.status != "ready":
            assert self._client is not None
            async with self._lock:
                # Re-check under the lock; another caller may have resumed it.
                if self._sandbox is not None and self._sandbox.status == "ready":
                    return True
                self._sandbox = await self._client.start_sandbox(
                    self._sandbox_name, timeout=120
                )
        return True

    async def adelete(self) -> bool:
        """Delete the underlying sandbox. Returns True if one was deleted."""
        sb = await self.try_resolve()
        if sb is None:
            return False
        assert self._client is not None
        await self._client.delete_sandbox(sb.id)
        self._sandbox = None
        return True

    async def _create_sandbox(self, client: AsyncSandboxClient) -> AsyncSandbox:
        """Create a sandbox; fall back to the default image if no snapshot."""
        kwargs = dict(
            name=self._sandbox_name,
            idle_ttl_seconds=SANDBOX_IDLE_TTL_SECONDS,
            delete_after_stop_seconds=SANDBOX_DELETE_AFTER_STOP_SECONDS,
        )
        if SANDBOX_SNAPSHOT_NAME:
            try:
                return await client.create_sandbox(
                    snapshot_name=SANDBOX_SNAPSHOT_NAME, **kwargs
                )
            except ResourceNotFoundError:
                pass  # snapshot not built yet; fall through to default image
        return await client.create_sandbox(**kwargs)

    async def aget_work_dir(self) -> str:
        await self.aresolve()
        return SANDBOX_WORK_DIR

    def _resolve_for_write(self, path: str) -> str:
        """Reroute write-bound paths into the sandbox work dir.

        The deepagents virtual filesystem treats ``/foo.md`` as "at the project
        root". Within the sandbox, ``/`` is the literal POSIX root, where the
        non-root user cannot write. Rewrite any write target outside
        ``SANDBOX_WORK_DIR`` to ``<work_dir>/<basename>``. Read paths are
        handled separately by :meth:`_resolve_for_read`.
        """
        if not isinstance(path, str) or not path:
            return path
        work_dir = PurePosixPath(SANDBOX_WORK_DIR)
        candidate = PurePosixPath(path)
        try:
            candidate.relative_to(work_dir)
            return str(candidate)
        except ValueError:
            return str(work_dir / candidate.name)

    def _resolve_for_read(self, path: str) -> str:
        """Resolve relative read paths against the sandbox work dir.

        LangSmith's ``sb.read`` / ``sb.run`` resolve bare relative paths
        against ``/home/user/`` (per the SDK), but uploads land in
        ``SANDBOX_WORK_DIR`` (``/workspace``) and the ``execute`` tool runs
        with ``cwd=SANDBOX_WORK_DIR``. Without this helper, ``read_file
        ("./data.csv")`` and the like would chase ``/home/user/data.csv``
        and 404 even though the file is sitting in ``/workspace``.
        """
        if not isinstance(path, str) or not path:
            return SANDBOX_WORK_DIR
        candidate = PurePosixPath(path)
        if candidate.is_absolute():
            return str(candidate)
        return str(PurePosixPath(SANDBOX_WORK_DIR) / path)

    @property
    def id(self) -> str:
        if self._sandbox is not None:
            return self._sandbox.id
        return f"lazy-{self._thread_id}"

    # ---------------------------------------------------------------
    # File operations
    # ---------------------------------------------------------------

    async def aread(
        self, file_path: str, offset: int = 0, limit: int = 2000
    ) -> ReadResult:
        sb = await self.aresolve()
        target = self._resolve_for_read(file_path)
        try:
            raw = await sb.read(target)
        except ResourceNotFoundError as exc:
            return ReadResult(error=str(exc))
        except Exception as exc:
            return ReadResult(error=f"{type(exc).__name__}: {exc}")
        text = raw.decode("utf-8", errors="replace") if isinstance(raw, bytes) else str(raw)
        lines = text.splitlines(keepends=True)
        end = (offset + limit) if limit and limit > 0 else None
        sliced = "".join(lines[offset:end])
        return ReadResult(file_data=FileData(content=sliced, encoding="utf-8"))

    async def awrite(self, file_path: str, content: str) -> WriteResult:
        sb = await self.aresolve()
        target = self._resolve_for_write(file_path)
        try:
            parent = str(PurePosixPath(target).parent)
            if parent and parent not in ("/", "."):
                await sb.run(f"mkdir -p {shlex.quote(parent)}", timeout=10)
            data: Any = content if isinstance(content, (str, bytes)) else str(content)
            await sb.write(target, data)
            return WriteResult(error=None, path=target)
        except Exception as exc:
            return WriteResult(error=f"{type(exc).__name__}: {exc}", path=None)

    async def aedit(
        self,
        file_path: str,
        old_string: str,
        new_string: str,
        replace_all: bool = False,
    ) -> EditResult:
        sb = await self.aresolve()
        target = self._resolve_for_read(file_path)
        try:
            raw = await sb.read(target)
        except Exception as exc:
            return EditResult(
                error=f"{type(exc).__name__}: {exc}",
                path=None,
                occurrences=None,
            )
        text = raw.decode("utf-8", errors="replace") if isinstance(raw, bytes) else str(raw)
        if old_string not in text:
            return EditResult(
                error=f"old_string not found in {file_path}",
                path=None,
                occurrences=0,
            )
        if replace_all:
            updated = text.replace(old_string, new_string)
            occurrences = text.count(old_string)
        else:
            if text.count(old_string) > 1:
                return EditResult(
                    error=(
                        "old_string is not unique; use replace_all=True or a "
                        "more specific old_string"
                    ),
                    path=None,
                    occurrences=text.count(old_string),
                )
            updated = text.replace(old_string, new_string, 1)
            occurrences = 1
        try:
            await sb.write(target, updated)
        except Exception as exc:
            return EditResult(
                error=f"{type(exc).__name__}: {exc}",
                path=None,
                occurrences=None,
            )
        return EditResult(error=None, path=target, occurrences=occurrences)

    async def aupload_files(
        self, files: list[tuple[str, bytes]]
    ) -> list[FileUploadResponse]:
        sb = await self.aresolve()
        results: list[FileUploadResponse] = []
        for path, data in files:
            target = self._resolve_for_write(path)
            try:
                parent = str(PurePosixPath(target).parent)
                if parent and parent not in ("/", "."):
                    await sb.run(f"mkdir -p {shlex.quote(parent)}", timeout=10)
                await sb.write(target, data)
                results.append(FileUploadResponse(path=target, error=None))
            except Exception:
                # Best-effort classification — surface generic invalid_path
                # rather than leak SDK exception types into the protocol.
                results.append(FileUploadResponse(path=target, error="invalid_path"))
        return results

    async def adownload_files(
        self, paths: list[str]
    ) -> list[FileDownloadResponse]:
        sb = await self.aresolve()
        results: list[FileDownloadResponse] = []
        for path in paths:
            target = self._resolve_for_read(path)
            try:
                content = await sb.read(target)
                if not isinstance(content, bytes):
                    content = str(content).encode("utf-8")
                results.append(FileDownloadResponse(path=target, content=content, error=None))
            except ResourceNotFoundError:
                results.append(
                    FileDownloadResponse(path=target, content=None, error="file_not_found")
                )
            except Exception:
                results.append(
                    FileDownloadResponse(path=target, content=None, error="invalid_path")
                )
        return results

    # ---------------------------------------------------------------
    # Discovery (shell-shimmed because LangSmith has no native APIs)
    # ---------------------------------------------------------------

    async def als(self, path: str) -> LsResult:
        info = await self.als_info(path)
        return LsResult(entries=info)

    async def als_info(self, path: str) -> list[FileInfo]:
        sb = await self.aresolve()
        target = self._resolve_for_read(path)
        # -F appends '/' to dirs, '*' to executables, etc. We use stat-based
        # listing for richer info: format <type> <size> <mtime> <name>.
        # `find -maxdepth 1 -mindepth 1` lists immediate children.
        cmd = (
            f"find {shlex.quote(target)} -maxdepth 1 -mindepth 1 -printf "
            r"'%y\t%s\t%T@\t%p\n'"
        )
        try:
            result = await sb.run(cmd, timeout=20)
        except Exception:
            return []
        if result.exit_code != 0:
            return []
        return _parse_find_printf(result.stdout)

    async def aglob(self, pattern: str, path: str = "/") -> GlobResult:
        matches = await self.aglob_info(pattern, path)
        return GlobResult(matches=matches)

    async def aglob_info(self, pattern: str, path: str = "/") -> list[FileInfo]:
        sb = await self.aresolve()
        target = self._resolve_for_read(path)
        # find supports -name (shell-glob) and -path (full path glob).
        # Use -name for simple patterns; treat patterns containing '/' as -path.
        flag = "-path" if "/" in pattern else "-name"
        cmd = (
            f"find {shlex.quote(target)} {flag} {shlex.quote(pattern)} -printf "
            r"'%y\t%s\t%T@\t%p\n'"
        )
        try:
            result = await sb.run(cmd, timeout=30)
        except Exception:
            return []
        if result.exit_code != 0:
            return []
        return _parse_find_printf(result.stdout)

    async def agrep(
        self,
        pattern: str,
        path: str | None = None,
        glob: str | None = None,
    ) -> GrepResult:
        matches = await self.agrep_raw(pattern, path, glob)
        if isinstance(matches, str):
            return GrepResult(error=matches)
        return GrepResult(matches=matches)

    async def agrep_raw(
        self,
        pattern: str,
        path: str | None = None,
        glob: str | None = None,
    ) -> list[GrepMatch] | str:
        sb = await self.aresolve()
        target = self._resolve_for_read(path) if path else SANDBOX_WORK_DIR
        # -r recursive, -n line numbers, -H always show filename, -I skip binary
        include = f"--include={shlex.quote(glob)} " if glob else ""
        cmd = (
            f"grep -rnHI {include}{shlex.quote(pattern)} {shlex.quote(target)} "
            f"2>/dev/null"
        )
        try:
            result = await sb.run(cmd, timeout=30)
        except Exception as exc:
            return f"{type(exc).__name__}: {exc}"
        # grep exits 1 when no matches; that's not an error for us.
        if result.exit_code not in (0, 1):
            return result.stderr or f"grep exited with code {result.exit_code}"
        matches: list[GrepMatch] = []
        for line in result.stdout.splitlines():
            # Format: <path>:<lineno>:<text>
            parts = line.split(":", 2)
            if len(parts) != 3:
                continue
            try:
                line_no = int(parts[1])
            except ValueError:
                continue
            matches.append(GrepMatch(path=parts[0], line=line_no, text=parts[2]))
        return matches

    # ---------------------------------------------------------------
    # Code execution
    # ---------------------------------------------------------------

    async def _aexecute_core(
        self, command: str, *, timeout: int | None = None
    ) -> ExecuteResponse:
        """Run a command and return the FULL merged output (no truncation)."""
        sb = await self.aresolve()
        run_timeout = timeout if timeout is not None else 300
        # Surface ASTA_TOKEN into the command environment so the `asta` CLI
        # (theory generation, DataVoyager, PDF extraction) can authenticate.
        # Read at call time, not import time: .env is loaded after this module
        # is imported, so an eager module-level read would see an empty value.
        # env vars are added on top of the /bin/bash profile, so PATH etc. are
        # preserved; subprocesses spawned by the executed code inherit them.
        # Prefer the caller's own token (per-user, paste-and-store), set into a
        # ContextVar by agent(); fall back to the process-wide env var.
        from backend.runtime import _active_asta_token  # noqa: PLC0415 — avoid import cycle at module load

        run_env = None
        asta_token = _active_asta_token.get() or os.getenv("ASTA_TOKEN")
        if asta_token:
            run_env = {"ASTA_TOKEN": asta_token}
        try:
            result = await sb.run(
                command, timeout=run_timeout, cwd=SANDBOX_WORK_DIR, env=run_env
            )
        except Exception as exc:
            return ExecuteResponse(
                output=f"{type(exc).__name__}: {exc}",
                exit_code=None,
                truncated=False,
            )
        # Merge stdout + stderr in a single channel. stderr is prefixed so
        # callers (and the model) can tell them apart when diagnosing failures.
        merged = result.stdout
        if result.stderr:
            if merged and not merged.endswith("\n"):
                merged += "\n"
            merged += f"[stderr]\n{result.stderr}"
        return ExecuteResponse(
            output=merged, exit_code=result.exit_code, truncated=False
        )

    async def aexecute(
        self, command: str, *, timeout: int | None = None
    ) -> ExecuteResponse:
        # Truncated: this output can flow into the agent's context, so it is
        # capped to protect the model's state window.
        return _truncate_execute_response(
            await self._aexecute_core(command, timeout=timeout)
        )

    async def aexecute_untruncated(
        self, command: str, *, timeout: int | None = None
    ) -> ExecuteResponse:
        """Run a command WITHOUT the agent-state truncation cap.

        For server-side callers (HTTP routes) that parse the output themselves
        and never feed it to the model — e.g. polling an Asta Theorizer task,
        whose completed record is ~500 KB and would otherwise be clipped to
        ``EXECUTE_OUTPUT_MAX_BYTES`` and become unparseable JSON.
        """
        return await self._aexecute_core(command, timeout=timeout)

    # ---------------------------------------------------------------
    # Sync stubs (deepagents middleware uses async only).
    # ---------------------------------------------------------------

    def _no_sync(self, *_args, **_kwargs):
        raise RuntimeError(
            "LazyLangsmithSandbox supports only async methods; sync operations "
            "would block on async sandbox creation. Use the a*-prefixed APIs."
        )

    upload_files = _no_sync
    download_files = _no_sync
    ls = _no_sync
    read = _no_sync
    write = _no_sync
    edit = _no_sync
    grep = _no_sync
    glob = _no_sync
    execute = _no_sync


def _parse_find_printf(raw: str) -> list[FileInfo]:
    """Parse `find ... -printf '%y\\t%s\\t%T@\\t%p\\n'` output into FileInfo."""
    entries: list[FileInfo] = []
    for line in raw.splitlines():
        parts = line.split("\t")
        if len(parts) != 4:
            continue
        ftype, size_str, mtime_str, path = parts
        info: FileInfo = {"path": path, "is_dir": ftype == "d"}
        try:
            info["size"] = int(size_str)
        except ValueError:
            pass
        try:
            info["modified_at"] = datetime.fromtimestamp(float(mtime_str)).isoformat()
        except (ValueError, OSError):
            pass
        entries.append(info)
    return entries
