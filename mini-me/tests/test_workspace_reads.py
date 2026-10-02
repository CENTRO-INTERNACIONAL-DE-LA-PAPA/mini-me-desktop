"""An attached file has to be readable at the path the app announces.

The app tells the coordinator `./report.pdf`; deepagents' `validate_path` turns that into
`/report.pdf` before the backend sees it. Without the workspace fallback in
`LocalWorkspaceBackend._resolve_path`, `read_file` looked at the root of the Linux filesystem and
answered "File '/report.pdf' not found" with the file sitting in the conversation's folder.
"""

from __future__ import annotations

from pathlib import Path

from deepagents.backends.utils import validate_path


def _backend(tmp_path: Path, monkeypatch, thread: str):
    from backend.local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    backend = LocalWorkspaceBackend(thread)
    (tmp_path / thread).mkdir(parents=True, exist_ok=True)
    return backend


def test_an_attachment_announced_as_dot_slash_is_read_from_the_workspace(tmp_path, monkeypatch):
    backend = _backend(tmp_path, monkeypatch, "thread-a")
    (tmp_path / "thread-a" / "notes (1).txt").write_text("hello\n")

    # Exactly what the read_file tool hands the backend.
    result = backend.read(validate_path("./notes (1).txt"))

    assert not result.error, result.error
    assert "hello" in str(result.file_data)


def test_a_real_absolute_path_is_left_alone(tmp_path, monkeypatch):
    backend = _backend(tmp_path, monkeypatch, "thread-b")
    elsewhere = tmp_path / "elsewhere.txt"
    elsewhere.write_text("outside\n")

    assert backend._resolve_path(str(elsewhere)) == elsewhere


def test_a_missing_file_is_still_reported_missing(tmp_path, monkeypatch):
    backend = _backend(tmp_path, monkeypatch, "thread-c")

    result = backend.read(validate_path("./not-there.txt"))

    assert result.error
