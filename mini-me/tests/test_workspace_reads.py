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


def test_a_write_into_a_subfolder_stays_in_that_subfolder(tmp_path, monkeypatch):
    """`./papers/notes.md` arrives as `/papers/notes.md`; it must not be flattened to the root."""
    backend = _backend(tmp_path, monkeypatch, "thread-d")

    result = backend.write(validate_path("./papers/notes.md"), "hello")

    assert not result.error, result.error
    assert (tmp_path / "thread-d" / "papers" / "notes.md").read_text() == "hello"
    assert not (tmp_path / "thread-d" / "notes.md").exists()


def test_text_never_overwrites_an_attached_pdf(tmp_path, monkeypatch):
    """How an attached 7.9 MB report became a one-line placeholder."""
    backend = _backend(tmp_path, monkeypatch, "thread-e")
    attached = tmp_path / "thread-e" / "report 1.pdf"
    attached.write_bytes(b"%PDF-1.7 the real report")

    result = backend.write(validate_path("./report 1.pdf"), "This is a placeholder")

    assert result.error and "Refusing to overwrite" in result.error
    assert attached.read_bytes() == b"%PDF-1.7 the real report"


def test_a_real_absolute_path_keeps_only_its_name(tmp_path, monkeypatch):
    """`/tmp/x.csv` is a real place on this machine; rebuilding `tmp/` inside the workspace buries it."""
    backend = _backend(tmp_path, monkeypatch, "thread-f")
    elsewhere = tmp_path / "outside" / "out.csv"
    elsewhere.parent.mkdir()

    result = backend.write(str(elsewhere), "a,b")

    assert not result.error, result.error
    assert (tmp_path / "thread-f" / "out.csv").read_text() == "a,b"
    assert not elsewhere.exists(), "a write never lands outside the workspace"
