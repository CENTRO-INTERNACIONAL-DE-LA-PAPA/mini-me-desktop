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


def test_nothing_here_claims_to_know_what_was_written():
    """The one thing this module must never be read as saying.

    A command can write somewhere it never names — a script it invokes, a library's cache, a
    relative path after a `cd`. `execute_rule.py` refuses to make a guard out of string matching
    for exactly this reason, and a docstring here that promised containment would be the §252
    mistake in a third place.
    """
    source = Path(ledger.__file__).read_text()
    assert "what the command wrote" in source, "the limit has to be stated in the module itself"
    assert "write somewhere it never names" in source, "and the way it is incomplete, named"
    # A command that writes without naming: this reports nothing, and that is correct behaviour.
    assert ledger.outside("bash prepared-script.sh", WORK) == []


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
