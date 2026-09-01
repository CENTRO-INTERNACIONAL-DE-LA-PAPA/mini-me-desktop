"""The record of what a conversation ran, and what each command named outside it.

The property under test is not "the regex is clever". It is that **the failure which actually
happened would have been visible on the day it happened**: §160's sixteen files went to `/tmp` and
the Outputs panel was empty, and nobody knew until they were told to look.

So the first test is that command, and the rest are the ways a report becomes one nobody reads.
"""

from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path

import pytest

from minime_local import ledger

WORK = "/mnt/c/Users/piero/Documents/Mini-Me/019ff651-0cd7-71c1"


def test_the_command_from_160_names_the_place_the_files_went():
    """The defect this exists for, as it was actually written."""
    command = (
        "python3 -c \"import pandas as pd, matplotlib.pyplot as plt; "
        "df = pd.read_csv('data.csv'); df.describe().to_csv('/tmp/summary.csv'); "
        "plt.savefig('/tmp/hist.png')\""
    )
    assert ledger.outside(command, WORK) == ["/tmp/summary.csv", "/tmp/hist.png"]


def test_what_lands_in_the_conversation_is_not_reported():
    """The Outputs panel already shows those. Repeating them buries the ones it cannot show."""
    command = f"python3 analyse.py --out {WORK}/results.csv && cp fig.png {WORK}/fig.png"
    assert ledger.outside(command, WORK) == []
    # The folder itself is not "outside" itself.
    assert ledger.outside(f"ls {WORK}", WORK) == []


def test_system_locations_are_left_out_so_the_report_stays_readable():
    """Noise, not safety.

    `/usr/bin/python3` and `/dev/null` are in a large share of commands and are not where results
    land. A report that names them every time is one that stops being read — §116 and §132 record
    exactly that happening to a diagnostic that was correct.
    """
    command = "/usr/bin/python3 -c 'pass' 2>/dev/null; cat /etc/hostname; ls /proc/self"
    assert ledger.outside(command, WORK) == []
    # And the exclusion is not a claim that those are unwritable — only that they are not shown.
    assert "/usr/" in ledger.SYSTEM_PREFIXES


def test_a_url_is_not_a_path():
    """Reporting the host of every download as an escaped write is how this becomes noise."""
    command = "curl -sSL https://example.org/data/potato.csv -o potato.csv"
    assert ledger.outside(command, WORK) == []
    assert ledger.named_paths("git clone file:///srv/repo") == []


def test_paths_come_out_of_quotes_and_in_the_order_written():
    command = "cp '/tmp/second.csv' \"/root/first.csv\" /tmp/second.csv"
    # Order as written, and each named once however often it appears.
    assert ledger.outside(command, WORK) == ["/tmp/second.csv", "/root/first.csv"]


def test_an_entry_says_what_ran_and_what_it_named():
    record = ledger.entry(
        "python3 -c \"open('/tmp/x','w')\"",
        exit_code=0,
        seconds=1.234,
        work_dir=WORK,
        at="2026-08-25T09:14:03Z",
    )
    assert record["exit"] == 0
    assert record["seconds"] == 1.23
    assert record["at"] == "2026-08-25T09:14:03Z"
    assert record["outside"] == ["/tmp/x"]
    assert record["clipped"] is False


def test_a_generated_script_is_clipped_and_says_so():
    """A record meant to be read cannot carry a forty-kilobyte one-liner."""
    command = "python3 -c \"" + ("x = 1; " * 5_000) + "\""
    record = ledger.entry(command, exit_code=0, seconds=0.1, work_dir=WORK, at="t")
    assert record["clipped"] is True
    assert len(record["command"]) == ledger.TEXT_CAP + 1  # the ellipsis
    assert record["command"].endswith("…")


def test_the_record_is_bounded_so_a_loop_cannot_fill_a_disk(tmp_path: Path):
    """A bounded file that only grows is not bounded."""
    for i in range(ledger.MAX_ENTRIES + 25):
        ledger.append(tmp_path, ledger.entry(f"echo {i}", exit_code=0, seconds=0, work_dir=tmp_path, at="t"))
    written = (tmp_path / ledger.RECORD_DIR / ledger.RECORD_NAME).read_text().splitlines()
    assert len(written) == ledger.MAX_ENTRIES
    # The newest are kept, because the question is always what just happened.
    assert json.loads(written[-1])["command"] == f"echo {ledger.MAX_ENTRIES + 24}"


def test_the_record_lands_where_the_conversation_can_carry_it(tmp_path: Path):
    ledger.append(tmp_path, ledger.entry("echo hi", exit_code=0, seconds=0, work_dir=tmp_path, at="t"))
    target = tmp_path / ledger.RECORD_DIR / ledger.RECORD_NAME
    assert target.is_file()
    # A dot-folder, so `workspace::outputs` does not offer it as something the research produced.
    assert ledger.RECORD_DIR.startswith(".")
    assert json.loads(target.read_text().splitlines()[0])["command"] == "echo hi"


def test_an_unwritable_record_costs_the_researcher_nothing(tmp_path: Path):
    """The trade `_say_where_it_ran` already makes: a lost diagnostic beats a broken `execute`."""
    blocked = tmp_path / "a-file-not-a-folder"
    blocked.write_text("in the way")
    # `blocked/.mini-me/` cannot be created. This must return, not raise.
    ledger.append(blocked, ledger.entry("echo hi", exit_code=0, seconds=0, work_dir=blocked, at="t"))


def test_the_cwd_scan_does_not_claim_to_see_another_directory(tmp_path: Path):
    """Observation has a boundary; a file outside the scanned cwd stays unknown."""
    import time

    command_dir = tmp_path / "command"
    command_dir.mkdir()
    elsewhere = tmp_path / "elsewhere.csv"
    started = time.time()
    elsewhere.write_text("outside the observation")
    finished = time.time()

    found, truncated = ledger.observed_writes(command_dir, started, finished)
    assert found == []
    assert not truncated


def test_a_real_command_through_the_real_backend_lands_in_the_record(tmp_path, monkeypatch):
    """The join, walked rather than asserted.

    A recorder nothing calls is the defect this project has hit six times (§254, §257, §258, §259,
    §261, §262), so this drives `LocalWorkspaceBackend.aexecute` — the one function every command
    passes through — with a real command that writes to a real `/tmp`, and reads the record back
    off disk.

    `monkeypatch` for the workspace variable, and pytest runs these serially, so nothing else is
    reading it while it is redirected — the property §271 cost a day to learn about the Rust side.
    """
    from minime_local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    outside_target = tmp_path.parent / "outside-the-conversation.csv"

    backend = LocalWorkspaceBackend("thread-x")
    asyncio.run(backend.aexecute(f"python3 -c \"open('{outside_target}','w').write('x')\""))

    record = tmp_path / "thread-x" / ledger.RECORD_DIR / ledger.RECORD_NAME
    assert record.is_file(), "every command must reach the record, not only the failures"

    written = [json.loads(line) for line in record.read_text().splitlines()]
    assert len(written) == 1
    assert written[0]["exit"] == 0
    assert written[0]["outside"] == [str(outside_target)], "the file it put outside must be named"
    assert written[0]["seconds"] is not None, "how long it took is part of what ran"
    assert outside_target.is_file(), "and the command really did write there"


def test_a_command_that_stays_inside_reports_nothing_outside(tmp_path, monkeypatch):
    """The other half: a well-behaved run must not fill the record with false alarms."""
    from minime_local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    backend = LocalWorkspaceBackend("thread-y")
    asyncio.run(backend.aexecute("python3 -c \"open('inside.csv','w').write('x')\""))

    record = tmp_path / "thread-y" / ledger.RECORD_DIR / ledger.RECORD_NAME
    written = [json.loads(line) for line in record.read_text().splitlines()]
    assert written[0]["outside"] == []
    assert (tmp_path / "thread-y" / "inside.csv").is_file(), "it ran in the conversation folder"


#: The record as the app must read it. Same discipline as `test_artifact_contract.py`: the producer
#: writes the fixture from its own code, so a field added here fails a test until the client's
#: authors have decided about it, rather than arriving unread for as long as the feature exists.
FIXTURE = (
    Path(__file__).resolve().parent.parent.parent
    / "crates" / "app" / "tests" / "fixtures" / "command-record.jsonl"
)


def _sample() -> list[dict]:
    """Three entries covering every shape the app has to render.

    Real-ish rather than minimal: a command that stayed inside, §160's command that did not, and
    one that failed — because "exit is null or a number" and "outside is empty or not" are the two
    branches the client gets wrong if it never sees them.
    """
    work = "/mnt/c/Users/piero/Documents/Mini-Me/019ff651-0cd7-71c1"
    return [
        ledger.entry(
            "python3 -c \"import pandas as pd; pd.read_csv('data.csv').describe().to_csv('summary.csv')\"",
            exit_code=0,
            seconds=1.4,
            work_dir=work,
            at="2026-08-25T09:14:03Z",
        ),
        ledger.entry(
            "python3 -c \"import matplotlib.pyplot as plt; plt.savefig('/tmp/hist.png')\"",
            exit_code=0,
            seconds=2.1,
            work_dir=work,
            at="2026-08-25T09:14:07Z",
        ),
        ledger.entry(
            "Rscript missing.R",
            exit_code=127,
            seconds=0.05,
            work_dir=work,
            at="2026-08-25T09:15:00Z",
        ),
        # The live defect: the command named no absolute path, but the bounded cwd observation saw
        # the relative output. `outside` and `wrote` must therefore be able to disagree in both
        # directions rather than one being forced into a subset of the other.
        ledger.entry(
            "python analysis.py",
            exit_code=0,
            seconds=3.2,
            work_dir=work,
            cwd="/tmp/background-worker",
            at="2026-08-25T09:15:20Z",
            wrote=["/tmp/background-worker/missingness.png"],
        ),
        # The strong case: named *and* confirmed written, which is the only one a copy button may
        # ever act on. Passed explicitly because deciding it needs a filesystem and this is a
        # fixture, not a run.
        ledger.entry(
            "python3 -c \"open('/tmp/late-blight.csv','w').write(rows)\"",
            exit_code=0,
            seconds=0.8,
            work_dir=work,
            at="2026-08-25T09:16:11Z",
            wrote=["/tmp/late-blight.csv"],
        ),
    ]


def test_the_committed_record_fixture_matches_what_this_module_writes():
    """Regenerate with `MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_ledger.py`."""
    generated = "\n".join(json.dumps(e, ensure_ascii=False, sort_keys=True) for e in _sample()) + "\n"
    if os.environ.get("MINIME_WRITE_CONTRACT"):
        FIXTURE.parent.mkdir(parents=True, exist_ok=True)
        FIXTURE.write_text(generated, encoding="utf-8")
        pytest.skip("fixture regenerated; read the diff")

    assert FIXTURE.exists(), f"{FIXTURE} is missing — regenerate with MINIME_WRITE_CONTRACT=1"
    assert FIXTURE.read_text(encoding="utf-8") == generated, (
        "the command record changed shape. Regenerate with "
        "`MINIME_WRITE_CONTRACT=1 pytest mini-me/tests/test_ledger.py`, then decide whether the "
        "app should read the new field."
    )


def test_the_fixture_covers_both_branches_the_client_can_get_wrong():
    entries = _sample()
    assert any(e["outside"] for e in entries), "a command that named something outside"
    assert any(not e["outside"] for e in entries), "and one that did not"
    assert any(e["exit"] != 0 for e in entries), "and one that failed"
    assert any(e["wrote"] for e in entries), "and one confirmed to have written there"
    assert any(e["outside"] and not e["wrote"] for e in entries), (
        "and one that named a path without writing it — the read case, which is the whole reason "
        "`wrote` exists and the one a copy button must never touch"
    )
    assert any(e["wrote"] and not e["outside"] for e in entries), (
        "and one relative write found from the cwd rather than the command string"
    )


def test_both_execute_tools_reach_the_record(tmp_path, monkeypatch):
    """deepagents registers two execute tools; a command must appear whichever one ran it.

    `middleware/filesystem.py` builds a synchronous tool that calls `execute` and an async one that
    calls `aexecute`, and which is used depends on how the graph was built. The first version of
    this recorded in `aexecute` alone — a correct component wired to one of two paths, which is the
    sixth time this project has made that exact mistake.
    """
    from minime_local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    backend = LocalWorkspaceBackend("both-paths")
    record = tmp_path / "both-paths" / ledger.RECORD_DIR / ledger.RECORD_NAME

    backend.execute("echo synchronous")
    asyncio.run(backend.aexecute("echo asynchronous"))

    written = [json.loads(line) for line in record.read_text().splitlines()]
    ran = [entry["command"] for entry in written]
    assert "echo synchronous" in ran, "the sync tool's commands must be recorded"
    assert "echo asynchronous" in ran, "and the async tool's"
    assert len(written) == 2, f"exactly once each, not twice: {ran}"
    # **Recorded is not the same as ran**, and the first version of this test only checked the
    # former. It passed while every synchronous command was dying with `FileNotFoundError`,
    # because `_record` records failures too — only `aexecute` created the workspace, so the sync
    # path ran with a `cwd` that did not exist.
    assert [entry["exit"] for entry in written] == [0, 0], (
        f"both tools must actually work, not merely be recorded: {written}"
    )


def test_a_nested_worker_records_with_its_conversation_without_offering_visible_files(
    tmp_path, monkeypatch
):
    """The worker owns the cwd; the researcher-facing conversation owns the record.

    A correctly pinned worker is nested under the conversation, so its relative file is already in
    Outputs and must not get a redundant recovery offer. Its command still belongs in the
    conversation's record rather than a hidden `.mini-me` folder under the worker.
    """
    from minime_local import workspace as ws

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    monkeypatch.setattr(
        ws,
        "_configurable",
        lambda: {ws.WORKSPACE_THREAD_KEY: "conversation"},
    )
    ws._PINNED_BY_THREAD.clear()
    ws._PROJECT_BY_THREAD.clear()

    worker = ws.LocalWorkspaceBackend("worker-thread")
    assert worker._conversation_dir == tmp_path / "conversation"
    assert worker._work_dir == tmp_path / "conversation" / "worker-thread"
    result = worker.execute("python3 -c \"open('plot.png','w').write('plot')\"")
    assert result.exit_code == 0, result.output

    entries = ledger.read(worker._conversation_dir)
    assert len(entries) == 1, "the command joins the conversation's record"
    assert entries[0]["cwd"] == str(worker._work_dir), "where it really ran stays observable"
    assert entries[0]["wrote"] == [], "the plot is already inside the conversation"
    assert (worker._work_dir / "plot.png").is_file()
    assert not (worker._work_dir / ledger.RECORD_DIR).exists(), "no hidden worker-only record"


def test_the_worker_pin_comes_from_its_tool_runtime_when_context_is_empty(monkeypatch):
    """The launch tool is handed the parent config even when `get_config()` cannot see it.

    Losing this pin is the second half of the live complaint: the worker then gets a sibling folder
    that is "inside" to itself and invisible to the researcher's conversation.
    """
    from minime_local import async_agents
    from minime_local.workspace import WORKSPACE_THREAD_KEY

    runtime_config = {
        "recursion_limit": 37,
        "configurable": {
            "thread_id": "researcher-conversation",
            "model_config": {"default": "openrouter::test-model"},
        },
    }
    forwarded = async_agents._forwarded_config(runtime_config)

    assert forwarded["recursion_limit"] == 37
    assert forwarded["configurable"][WORKSPACE_THREAD_KEY] == "researcher-conversation"
    assert forwarded["configurable"]["model_config"] == runtime_config["configurable"][
        "model_config"
    ]


def test_a_command_that_raises_is_still_timed_out_of_the_record(tmp_path, monkeypatch):
    """A failure inside `execute` must not leave the record silently short.

    It may legitimately record nothing — there is no result to describe — but it must not take the
    command down with it, and the researcher must still get the exception.
    """
    from minime_local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    backend = LocalWorkspaceBackend("raising")
    # A timeout far in the past is the cheapest way to make the underlying call unhappy without
    # reaching into deepagents' internals.
    backend.execute("echo fine")
    record = tmp_path / "raising" / ledger.RECORD_DIR / ledger.RECORD_NAME
    assert record.is_file(), "the ordinary path still records"


# --- what the command wrote, as against what it named -----------------------------------------


def test_a_file_written_during_the_command_is_reported_as_written(tmp_path: Path):
    import time

    target = tmp_path / "made-now.csv"
    start = time.time()
    target.write_text("col1\n1\n")
    end = time.time()
    assert ledger.written_during([str(target)], start, end) == [str(target)]


def test_a_file_the_command_only_read_is_not_reported_as_written(tmp_path: Path):
    """**The distinction the whole feature turns on.**

    `pd.read_csv('/tmp/input.csv')` names a file the researcher owns. Treating a named path as
    output is how a well-meant tidy-up steals somebody's data, and the command's text cannot tell
    the two apart — the file's own mtime can.
    """
    import os
    import time

    existing = tmp_path / "the-researchers-own.csv"
    existing.write_text("theirs")
    long_ago = time.time() - 3600
    os.utime(existing, (long_ago, long_ago))

    start = time.time()
    end = start + 0.5
    assert ledger.written_during([str(existing)], start, end) == []


def test_paths_that_are_not_files_are_left_alone(tmp_path: Path):
    import time

    start, end = time.time() - 1, time.time() + 1
    missing = tmp_path / "never-existed.csv"
    a_directory = tmp_path / "a-folder"
    a_directory.mkdir()
    assert ledger.written_during([str(missing), str(a_directory)], start, end) == []


def test_a_relative_write_from_an_outside_cwd_is_offered_for_recovery(tmp_path: Path):
    """The eight orphaned plots, across the whole producer seam.

    The command text names no absolute output. It runs in a worker directory outside the
    researcher's conversation and writes beside its script. The command record must live with the
    conversation, keep the empty named-path list honest, and still offer the file the filesystem
    observed appearing during the command.
    """
    import subprocess
    import sys
    import time

    from minime_local.workspace import _record

    conversation = tmp_path / "conversation"
    command_dir = tmp_path / "background-worker"
    conversation.mkdir()
    command_dir.mkdir()
    script = command_dir / "analysis.py"
    script.write_text("from pathlib import Path\nPath('missingness.png').write_text('plot')\n")
    old = time.time() - 3600
    os.utime(script, (old, old))

    started = time.monotonic()
    completed = subprocess.run(
        [sys.executable, "analysis.py"], cwd=command_dir, check=False, capture_output=True, text=True
    )
    elapsed = time.monotonic() - started
    assert completed.returncode == 0, completed.stderr

    _record(
        "python analysis.py",
        {"exit_code": completed.returncode, "output": completed.stdout},
        command_dir,
        elapsed,
        conversation_dir=conversation,
    )

    entries = ledger.read(conversation)
    assert len(entries) == 1, "the worker's record must join the conversation that can recover it"
    assert entries[0]["outside"] == [], "a relative write did not name an absolute path"
    assert entries[0]["wrote"] == [str(command_dir / "missingness.png")]
    assert ledger.collectable(conversation) == [str(command_dir / "missingness.png")]


def test_a_relative_write_after_shell_cd_is_offered_for_recovery(tmp_path: Path, monkeypatch):
    """The exact live miss: the process starts inside, then its shell changes directory.

    A parent process cannot read its child's final cwd. The absolute directory named by ``cd`` is
    therefore the observation root, while the file's mtime — not the path string — is what makes
    the relative PNG safe to offer. An older file beside it proves the directory is not treated as
    a bag of outputs.
    """
    import shlex
    import time

    from minime_local.workspace import LocalWorkspaceBackend

    workspaces = tmp_path / "workspaces"
    external = tmp_path / "temporary-job"
    external.mkdir()
    existing = external / "the-researchers-own.csv"
    existing.write_text("theirs")
    long_ago = time.time() - 3600
    os.utime(existing, (long_ago, long_ago))
    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(workspaces))

    backend = LocalWorkspaceBackend("shell-cd")
    completed = backend.execute(
        f"cd {shlex.quote(str(external))} && "
        "python3 -c \"from pathlib import Path; "
        "Path('missingness.png').write_text('plot')\""
    )
    assert completed.exit_code == 0, completed.output

    conversation = workspaces / "shell-cd"
    entries = ledger.read(conversation)
    assert len(entries) == 1
    assert entries[0]["cwd"] == str(conversation), "the parent-visible cwd stays honest"
    assert entries[0]["outside"] == [str(external)]
    assert entries[0]["wrote"] == [str(external / "missingness.png")]
    assert ledger.collectable(conversation) == [str(external / "missingness.png")]


def test_observed_writes_are_kept_distinct_from_named_paths():
    """A relative output may be observed without weakening what `outside` means."""
    record = ledger.entry(
        "python3 -c \"open('/tmp/named.csv','w')\"",
        exit_code=0,
        seconds=0.1,
        work_dir=WORK,
        at="t",
        wrote=["/tmp/named.csv", "/tmp/never-mentioned.csv"],
    )
    assert record["outside"] == ["/tmp/named.csv"]
    assert record["wrote"] == ["/tmp/named.csv", "/tmp/never-mentioned.csv"]


def test_the_cwd_scan_says_when_either_safety_limit_bites(tmp_path: Path, caplog):
    import time

    for name in ("a.csv", "b.csv", "c.csv"):
        (tmp_path / name).write_text(name)
    nested = tmp_path / "nested"
    nested.mkdir()
    (nested / "deep.csv").write_text("deep")
    start, end = time.time() - 1, time.time() + 1

    with caplog.at_level("WARNING", logger="minime_local.ledger"):
        found, count_cut = ledger.observed_writes(
            tmp_path, start, end, max_entries=2, max_depth=3
        )
        shallow, depth_cut = ledger.observed_writes(
            tmp_path, start, end, max_entries=20, max_depth=0
        )

    assert count_cut and len(found) <= 2, "the entry budget is a real bound"
    assert depth_cut and str(nested / "deep.csv") not in shallow, "depth zero never descends"
    warnings = [record.getMessage() for record in caplog.records]
    assert len([message for message in warnings if "safety limit" in message]) == 2


def test_named_directory_scans_share_one_entry_budget(tmp_path: Path):
    """Naming more roots must not multiply the hot-path filesystem allowance."""
    import time

    first = tmp_path / "a-job"
    second = tmp_path / "b-job"
    first.mkdir()
    second.mkdir()
    (first / "one.png").write_text("one")
    (second / "two.png").write_text("two")
    start, end = time.time() - 1, time.time() + 1

    found, truncated = ledger.observed_writes_under(
        [str(first), str(second)], start, end, max_entries=1, max_depth=3
    )

    assert truncated
    assert found == [str(first / "one.png")], "one shared entry budget stops before root two"


def test_a_real_command_records_what_it_wrote_and_not_what_it_read(tmp_path, monkeypatch):
    """The join, with both halves in one turn.

    One command reads a file it did not create and writes another. The record must name both as
    *outside* — they are — and only the second as *written*, because only the second is this
    command's to offer to move.
    """
    import os
    import time

    from minime_local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    theirs = tmp_path.parent / "their-input.csv"
    theirs.write_text("a,b\n1,2\n")
    long_ago = time.time() - 3600
    os.utime(theirs, (long_ago, long_ago))
    ours = tmp_path.parent / "our-output.csv"
    ours.unlink(missing_ok=True)

    backend = LocalWorkspaceBackend("reads-and-writes")
    backend.execute(
        f"python3 -c \"open('{ours}','w').write(open('{theirs}').read())\""
    )

    record_path = tmp_path / "reads-and-writes" / ledger.RECORD_DIR / ledger.RECORD_NAME
    written = json.loads(record_path.read_text().splitlines()[0])

    assert set(written["outside"]) == {str(theirs), str(ours)}, "both are outside the conversation"
    assert written["wrote"] == [str(ours)], (
        "only the file this command created may be offered as its output; the other is the "
        "researcher's own input and moving it would be theft"
    )


# --- bringing them back -------------------------------------------------------------------------


def test_only_what_was_written_is_offered_and_never_what_was_read(tmp_path: Path):
    """`collectable` reads `wrote`, never `outside`. That difference is the whole safety property."""
    theirs, ours = tmp_path.parent / "their-input.csv", tmp_path.parent / "our-output.csv"
    theirs.write_text("theirs")
    ours.write_text("ours")
    ledger.append(
        tmp_path,
        ledger.entry(
            f"python3 -c \"open('{ours}','w').write(open('{theirs}').read())\"",
            exit_code=0,
            seconds=0.1,
            work_dir=tmp_path,
            at="t",
            wrote=[str(ours)],
        ),
    )
    assert ledger.collectable(tmp_path) == [str(ours)]


def test_a_swept_file_is_reported_as_gone_rather_than_silently_dropped(tmp_path: Path):
    """An empty answer must be able to say *why* it is empty.

    The first version returned only what could be copied, so a conversation whose files had been
    swept from `/tmp` produced an empty list indistinguishable from one that never wrote anything
    — and the button reported `brought=0 refused=0`, which is a sentence with no information in it.
    A caller cannot explain what it was not told (§279).
    """
    here, gone = tmp_path.parent / "still-here.csv", tmp_path.parent / "swept.csv"
    here.write_text("x")
    gone.unlink(missing_ok=True)
    for path in (gone, here):
        ledger.append(
            tmp_path,
            ledger.entry(
                f"python3 -c \"open('{path}','w')\"",
                exit_code=0, seconds=0.1, work_dir=tmp_path, at="t", wrote=[str(path)],
            ),
        )
    report = ledger.outside_files(tmp_path)
    assert report["present"] == [str(here)]
    assert report["gone"] == [str(gone)], "the swept one is named, not dropped"


def test_a_file_that_has_since_gone_is_not_offered(tmp_path: Path):
    gone = tmp_path.parent / "cleaned-up.csv"
    ledger.append(
        tmp_path,
        ledger.entry(
            f"python3 -c \"open('{gone}','w')\"",
            exit_code=0, seconds=0.1, work_dir=tmp_path, at="t", wrote=[str(gone)],
        ),
    )
    assert ledger.collectable(tmp_path) == [], "/tmp is swept; a stale record is not an offer"


def test_collecting_copies_and_leaves_the_original_where_it_was(tmp_path: Path):
    """**Copy, not move.**

    A script often writes a file and reads it back later in the same run, and a later turn may
    expect it where it was left. Moving breaks work that is still going; copying costs disk.
    """
    source = tmp_path.parent / "results.csv"
    source.write_text("col1\n1\n")
    outcome = ledger.collect(tmp_path, [str(source)])

    assert outcome["brought"] == [{"path": str(source), "as": "results.csv"}]
    assert (tmp_path / "results.csv").read_text() == "col1\n1\n"
    assert source.is_file(), "the original must still be there for whatever reads it next"


def test_an_existing_name_is_suffixed_rather_than_overwritten(tmp_path: Path):
    """The turn's own output must survive a file arriving with its name."""
    (tmp_path / "results.csv").write_text("what the turn produced")
    source = tmp_path.parent / "results.csv"
    source.write_text("what was outside")

    outcome = ledger.collect(tmp_path, [str(source)])
    assert outcome["brought"] == [{"path": str(source), "as": "results-2.csv"}]
    assert (tmp_path / "results.csv").read_text() == "what the turn produced"
    assert (tmp_path / "results-2.csv").read_text() == "what was outside"
    # Suffixed before the extension, or the file stops being openable.
    assert not (tmp_path / "results.csv-2").exists()


def test_one_bad_file_does_not_lose_the_others(tmp_path: Path):
    """A partial result is the normal case, and reporting only a count would hide it."""
    good = tmp_path.parent / "good.csv"
    good.write_text("fine")
    missing = tmp_path.parent / "not-there.csv"

    outcome = ledger.collect(tmp_path, [str(missing), str(good)])
    assert [b["as"] for b in outcome["brought"]] == ["good.csv"]
    assert outcome["refused"][0]["path"] == str(missing)
    assert "no longer there" in outcome["refused"][0]["reason"], "and it says why"


def test_a_file_already_inside_is_refused_rather_than_duplicated(tmp_path: Path):
    inside = tmp_path / "already-here.csv"
    inside.write_text("x")
    outcome = ledger.collect(tmp_path, [str(inside)])
    assert outcome["brought"] == []
    assert "already in this conversation" in outcome["refused"][0]["reason"]


# --- one conversation, one folder ---------------------------------------------------------------


def test_a_route_finds_the_folder_a_project_conversation_actually_lives_in(tmp_path, monkeypatch):
    """**The bug that made the button say the opposite of the panel.**

    `workspace_project()` reads the live run's config, and a route has none — so a conversation
    filed in a project had its runs writing to `root/<project>/<thread>` while every route computed
    `root/<thread>`. The app counted two files written outside; the backend, reading the other
    folder, counted none. Both were confident and one was looking at nothing (§280).
    """
    from minime_local.workspace import LocalWorkspaceBackend, existing_project

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    # What a run in a project leaves behind.
    filed = tmp_path / "Late blight" / "thread-42"
    filed.mkdir(parents=True)

    assert existing_project(tmp_path, "thread-42") == "Late blight"
    # And the backend built outside a run now resolves to it, rather than to `root/thread-42`.
    assert LocalWorkspaceBackend("thread-42")._work_dir == filed


def test_an_ungrouped_conversation_is_left_where_it_is(tmp_path, monkeypatch):
    """The folder that already exists wins, and for an ungrouped conversation that is the root."""
    from minime_local.workspace import LocalWorkspaceBackend, existing_project

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    (tmp_path / "thread-7").mkdir()
    # A project folder exists too, and must not be picked for a thread that is not inside it.
    (tmp_path / "Some project").mkdir()

    assert existing_project(tmp_path, "thread-7") == ""
    assert LocalWorkspaceBackend("thread-7")._work_dir == tmp_path / "thread-7"


def test_a_conversation_with_no_folder_yet_lands_where_it_would_be_created(tmp_path, monkeypatch):
    from minime_local.workspace import LocalWorkspaceBackend, existing_project

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    assert existing_project(tmp_path, "brand-new") == ""
    assert LocalWorkspaceBackend("brand-new")._work_dir == tmp_path / "brand-new"


def test_the_record_a_route_reads_is_the_one_the_run_wrote(tmp_path, monkeypatch):
    """The join: a run in a project writes the record, and a route built later finds it.

    This is the assertion the whole arc was missing. Both sides were tested; that they agreed about
    *which folder* was never asserted, and they did not.
    """
    from minime_local.workspace import LocalWorkspaceBackend

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    filed = tmp_path / "Late blight" / "thread-99"
    filed.mkdir(parents=True)
    outside_file = tmp_path.parent / "from-a-project-run.csv"
    outside_file.unlink(missing_ok=True)

    # The run: a command that writes outside the conversation.
    running = LocalWorkspaceBackend("thread-99")
    assert running._work_dir == filed
    running.execute(f"python3 -c \"open('{outside_file}','w').write('x')\"")

    # The route: a *fresh* backend for the same thread, as a request would build.
    from_route = LocalWorkspaceBackend("thread-99")
    report = ledger.outside_files(from_route._work_dir)
    assert report["present"] == [str(outside_file)], (
        "a route must read the record the run wrote, or it reports the opposite of the panel"
    )


def test_the_empty_folder_the_bug_left_behind_does_not_win(tmp_path, monkeypatch):
    """**Exactly what was on the researcher's disk**, and what the first fix walked straight into.

        Mini-Me\\01a03ab8-…                 <- empty, created by a route resolving the wrong path
        Mini-Me\\test3\\01a03ab8-…           <- the real one, holding the record

    Every earlier press had created the top-level folder through `aresolve`, so a check for
    `is_dir()` found it and answered "ungrouped" — and the fix for reading the wrong folder went on
    reading the wrong folder (§280).
    """
    from minime_local.workspace import LocalWorkspaceBackend, existing_project

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    thread = "01a03ab8-2049-7363-9864-25d40e644180"
    (tmp_path / thread).mkdir()                       # the debris
    real = tmp_path / "test3" / thread                # where the run actually writes
    real.mkdir(parents=True)
    (real / "notes.md").write_text("the conversation's own work")

    assert existing_project(tmp_path, thread) == "test3"
    assert LocalWorkspaceBackend(thread)._work_dir == real


def test_an_ungrouped_conversation_with_files_still_wins(tmp_path, monkeypatch):
    """The rule must not simply prefer projects: a real ungrouped conversation keeps its folder."""
    from minime_local.workspace import existing_project

    thread = "ungrouped-thread"
    (tmp_path / thread).mkdir()
    (tmp_path / thread / "results.csv").write_text("mine")
    (tmp_path / "some project" / thread).mkdir(parents=True)  # an empty decoy inside a project

    assert existing_project(tmp_path, thread) == ""


def test_the_project_lookup_walks_the_root_once_per_thread(tmp_path, monkeypatch):
    """A constructor is not a place to be slow.

    `LocalWorkspaceBackend` is built for every run *and* every request, and this walks the
    workspace root over a `/mnt/c` mount. LangGraph's dev server flags synchronous work in an
    async handler for exactly this reason, and the honest floor is: at most one listing per
    thread, per process.
    """
    from minime_local import workspace as ws

    monkeypatch.setenv("MINIME_LOCAL_WORKSPACE", str(tmp_path))
    ws._PROJECT_BY_THREAD.clear()
    (tmp_path / "proj" / "thread-cached").mkdir(parents=True)
    (tmp_path / "proj" / "thread-cached" / "x.txt").write_text("x")

    walks = {"count": 0}
    real = ws._look_for_project

    def counted(root, thread_id):
        walks["count"] += 1
        return real(root, thread_id)

    monkeypatch.setattr(ws, "_look_for_project", counted)

    for _ in range(5):
        assert ws.existing_project(tmp_path, "thread-cached") == "proj"
    assert walks["count"] == 1, f"the root was walked {walks['count']} times"


def test_the_route_does_its_filesystem_work_off_the_event_loop():
    """Directory walks, `stat` and `copy2` in an async handler block every other request.

    Asserted on the source because the property is *where the call happens*, not what it returns —
    and a test that called the route would pass either way.
    """
    from pathlib import Path as P

    route = P(__file__).resolve().parent.parent / "backend" / "routes" / "artifacts.py"
    body = route.read_text().split("async def collect_outside_files")[1].split("\nasync def ")[0]
    for call in ("ledger.outside_files", "ledger.collect", "ledger.read"):
        assert f"asyncio.to_thread({call}" in body, f"{call} still runs on the event loop"


def test_append_answers_where_it_wrote(tmp_path):
    """**Never raising and never saying are different promises**, and this keeps only the first.

    A caller could not tell a written record from a failed one, so when the claims record silently
    failed to appear there were two invisible paths and no way to choose between them (§285).
    """
    written = ledger.append(tmp_path, {"command": "echo one"})
    assert written == ledger.record_path(tmp_path)
    assert Path(written).is_file()


def test_append_answers_nothing_when_it_could_not_write(tmp_path):
    """Still never raises — a diagnostic must not be what takes `execute` down."""
    (tmp_path / ledger.RECORD_DIR).write_text("not a folder", encoding="utf-8")
    assert ledger.append(tmp_path, {"command": "echo one"}) is None


def test_a_failed_write_says_why_and_still_does_not_raise(tmp_path, caplog):
    """**"It returned None" is not a diagnosis.**

    The claims record failed to appear on a real machine and this path was `pass`: the caller could
    report *that* it failed and nothing could report *why*, so the next step was another release
    (§286).
    """
    (tmp_path / ledger.RECORD_DIR).write_text("not a folder", encoding="utf-8")
    with caplog.at_level("WARNING", logger="minime_local.ledger"):
        assert ledger.append(tmp_path, {"command": "echo one"}) is None
    assert any("could not write" in r.getMessage() for r in caplog.records)
    # The traceback, not only the sentence — the exception type is the whole answer here.
    assert any(r.exc_info for r in caplog.records), "the reason has to survive to the log"
