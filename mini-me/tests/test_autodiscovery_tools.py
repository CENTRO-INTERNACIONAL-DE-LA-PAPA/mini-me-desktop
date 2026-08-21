"""Guards for the Asta AutoDiscovery tool (`backend.autodiscovery_tools`).

Two jobs. The first is the one the DataVoyager and theorizer tests already do: pin
the real CLI contract so a flag drifting is caught here rather than in a live run.
The second is specific to this module — **prove that no model decision can spend a
credit.** 1 credit = 1 experiment out of a fixed grant, the CLI's own confirmation
prompt is unusable from a TTY-less sandbox, so the only guard is that the tool
surface stops one call short of `submit`.

Every field name and status word asserted here came off a captured response from
the probe in docs §247, not from the documentation — which was wrong or incomplete
in nine places.
"""

from __future__ import annotations

from types import SimpleNamespace

import asyncio
import json
import os
import shlex
import subprocess
import tempfile

import pytest

from backend.autodiscovery_tools import (
    DEFAULT_EXPERIMENTS,
    MAX_EXPERIMENTS,
    _FIGURES_PY,
    _RESOLVE_PY,
    _build_create_command,
    _build_credits_command,
    _build_experiment_command,
    _build_experiments_command,
    _build_metadata_command,
    _build_status_command,
    _build_submit_command,
    _build_upload_command,
    _configure_shell,
    _upload_shell,
    _extract_json,
    _figures_shell,
    _sizes_shell,
    build_metadata,
    cost_of,
    discovery_markdown,
    draft_discovery_run,
    draft_run,
    fetch_experiment_figures,
    is_valid_experiment_id,
    is_valid_run_id,
    MetadataNotSaved,
    metadata_path,
    normalise_status,
    persist_discovery_outputs,
    poll_discovery_status,
    read_credits,
    read_metadata,
    submit_run,
    update_metadata,
)

_RUN = "8e5d2eaa-f067-4193-9400-d555e4607c41"


class _Sandbox:
    """A sandbox double that answers commands by substring match, in order."""

    def __init__(self, answers: list[tuple[str, str]], work_dir: str = "/workspace"):
        self.answers = answers
        self.commands: list[str] = []
        self.writes: dict[str, str] = {}
        self._work_dir = work_dir

    async def aexecute(self, command: str, timeout: int | None = None):
        self.commands.append(command)
        for needle, output in self.answers:
            if needle in command:
                return SimpleNamespace(output=output, exit_code=0)
        return SimpleNamespace(output="", exit_code=0)

    async def aget_work_dir(self) -> str:
        return self._work_dir

    async def awrite(self, path: str, content: str):
        self.writes[path] = content
        return SimpleNamespace(error=None)


# ---------------------------------------------------------------------------
# The CLI contract
# ---------------------------------------------------------------------------


def test_the_cli_subcommands_are_the_ones_the_cli_has():
    assert _build_create_command() == ["asta", "autodiscovery", "create"]
    assert _build_upload_command(_RUN, ["/w/a.csv", "/w/b.csv"]) == [
        "asta", "autodiscovery", "upload", _RUN, "/w/a.csv", "/w/b.csv",
    ]
    # `--file` is required; there is no inline metadata form. And the staged path is passed in
    # rather than defaulted, because a default is how the write and the read came to disagree.
    assert _build_metadata_command(_RUN, "/w/.staged.json")[-2:] == ["--file", "/w/.staged.json"]
    for argv in (
        _build_status_command(_RUN),
        _build_experiments_command(_RUN),
        _build_experiment_command(_RUN, "node_2_0"),
        _build_credits_command(),
    ):
        assert argv[-2:] == ["--format", "json"], argv


def test_submit_always_carries_yes_because_a_sandbox_has_no_tty():
    """Without `-y` the CLI calls `click.confirm` and blocks until its timeout."""
    argv = _build_submit_command(_RUN)
    assert argv == ["asta", "autodiscovery", "submit", _RUN, "-y"]


def test_ids_are_validated_before_they_reach_a_shell():
    assert is_valid_run_id(_RUN)
    assert not is_valid_run_id("")
    assert not is_valid_run_id(f"{_RUN}; rm -rf /")
    assert not is_valid_run_id("8e5d2eaa_f067_4193_9400_d555e4607c41")

    assert is_valid_experiment_id("node_2_0")
    assert is_valid_experiment_id("node_12_3")
    assert not is_valid_experiment_id("node_2_0; cat /etc/passwd")
    assert not is_valid_experiment_id("node_2")
    assert not is_valid_experiment_id("../../etc/passwd")


def test_the_configure_step_writes_uses_and_removes_its_staging_file():
    """§256: `awrite` is create-only, so the second write to a run's staging path was refused —
    which is how a budget change failed and nothing ran. `>` truncates, which is the semantics a
    transient file actually needs, and the process that writes it is the one that reads it."""
    shell = _configure_shell(_RUN, "/w/.staged.json", {"n_experiments": 5})
    assert shell.index("printf") < shell.index("autodiscovery")
    assert "/w/.staged.json" in shell
    # Cleaned up after use, and after a *failed* use too — `;` not `&&` before the rm.
    assert shell.rstrip().endswith("rm -f " + shlex.quote("/w/.staged.json"))
    assert "; rm -f" in shell
    # Keeps stderr, or a failure has no reason (§255).
    assert "2>&1" in shell
    # A staging write that fails says so instead of silently running the CLI on a missing file.
    assert "could not stage" in shell


def test_the_upload_happens_before_the_configure():
    """Metadata naming a file the upload did not deliver configures a run against nothing, so the
    caller sequences them and can tell the two failures apart."""
    import inspect

    from backend.autodiscovery_tools import draft_run as _draft

    body = inspect.getsource(_draft)
    assert body.index("_upload_shell") < body.index("_configure_shell")
    assert "2>&1" in _upload_shell(_RUN, ["/w/a.csv"])


def test_a_path_with_a_semicolon_in_it_cannot_start_a_second_command():
    """Dataset paths come from the model, so they are quoted rather than trusted.

    Run through a real shell rather than inspected as a string: `_sizes_shell` quotes twice —
    each path, then the whole script — and a regex over the result mis-reads the nesting. What
    matters is not how the command looks but whether `bash` executes the injected half.
    """
    with tempfile.TemporaryDirectory() as work:
        marker = f"{work}/pwned"
        nasty = f"{work}/absent.csv; touch {marker}"
        run = subprocess.run(
            ["bash", "-c", _sizes_shell([nasty])], capture_output=True, text=True, timeout=60
        )
        assert not os.path.exists(marker), "the injected command ran"
        # And the path was passed through whole, so it is simply reported as missing.
        assert json.loads(run.stdout or "{}") == {}, run.stdout

    # The figure decoder is `shlex.quote`d whole, so an apostrophe anywhere in it would end the
    # quoting and hand the rest of the script to the shell.
    assert "'" not in _FIGURES_PY
    # And the experiment id reaches the command, having been validated on the way in.
    assert "node_2_0" in _figures_shell(_RUN, "node_2_0", "/w/discovery")


# ---------------------------------------------------------------------------
# The credit gate
# ---------------------------------------------------------------------------


def test_the_model_facing_surface_cannot_submit():
    """The one guard that matters: no path from a model decision to a spent credit."""
    from backend import autodiscovery_tools as module

    tools = [
        getattr(module, name)
        for name in dir(module)
        if hasattr(getattr(module, name), "name") and hasattr(getattr(module, name), "description")
    ]
    exported = {getattr(t, "name", "") for t in tools}
    assert "draft_discovery_run" in exported
    assert not any("submit" in name for name in exported), exported
    # And `submit_run` is a plain coroutine, not a tool.
    assert not hasattr(submit_run, "description")


def test_a_drafted_run_reports_the_cost_against_the_balance_and_does_not_start():
    sandbox = _Sandbox(
        [
            ("_RESOLVE", json.dumps({"/w/soc.csv": {"path": "/w/soc.csv", "size": 665894}})),
            ("autodiscovery create", _RUN),
            ("autodiscovery upload", "Uploading soc.csv... done (665894 bytes)"),
            ("autodiscovery metadata ", "Metadata saved: gs://x"),
            ("autodiscovery credits", json.dumps({"credits": {"available": 495, "granted": 500}})),
        ]
    )
    answer = asyncio.run(_draft_via_tool(sandbox))
    assert _RUN in answer
    assert "5 credit(s) of 495 available" in answer
    assert "has NOT started" in answer
    assert not any("submit" in command for command in sandbox.commands), sandbox.commands


def _draft_via_tool(sandbox):
    """Run the tool with `_active_sandbox` bound, the way a turn would."""
    from backend.runtime import _active_sandbox

    async def go():
        token = _active_sandbox.set(sandbox)
        try:
            return await draft_discovery_run.coroutine(
                name="SOC covariates",
                description="synthetic control",
                domain="Soil science",
                intent="find the covariates that matter",
                dataset_paths="/w/soc.csv",
                dataset_description="113 covariates and a target",
                n_experiments=5,
            )
        finally:
            _active_sandbox.reset(token)

    return go()


def test_a_budget_outside_the_services_bounds_is_refused_before_anything_is_created():
    one = [{"path": "/w/a.csv", "size": 10}]
    for bad in (0, -1, MAX_EXPERIMENTS + 1, True, 2.5):
        with pytest.raises(ValueError):
            build_metadata(
                name="x", description="", domain="", intent="", datasets=one, n_experiments=bad
            )
    assert cost_of(build_metadata(
        name="x", description="", domain="", intent="", datasets=one
    )) == DEFAULT_EXPERIMENTS == 15


def test_a_dataset_name_is_derived_from_its_path_so_the_two_cannot_disagree():
    """The service validates `datasets[].name` against the uploaded filename."""
    metadata = build_metadata(
        name="run",
        description="d",
        domain="soil",
        intent="i",
        datasets=[{"path": "/workspace/nested/dir/soc.csv", "size": 12, "description": "cols"}],
    )
    entry = metadata["datasets"][0]
    assert entry["name"] == "soc.csv"
    assert entry["is_preloaded"] is False
    assert entry["file_size_bytes"] == 12
    assert entry["content_type"] == "text/csv"
    # The four tuning knobs are absent on purpose, so the service applies its own defaults.
    for knob in ("exploration_weight", "mcts_selection", "surprisal_width", "evidence_weight"):
        assert knob not in metadata


def test_a_missing_dataset_stops_the_draft_rather_than_configuring_an_empty_run():
    sandbox = _Sandbox([("_RESOLVE", json.dumps({}))])
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=["/w/gone.csv"],
            dataset_description="cols",
        )
    )
    assert "error" in drafted
    assert not any("autodiscovery create" in c for c in sandbox.commands)


def test_a_failed_metadata_save_says_nothing_was_spent():
    sandbox = _Sandbox(
        [
            ("_RESOLVE", json.dumps({"/w/a.csv": {"path": "/w/a.csv", "size": 5}})),
            ("autodiscovery create", _RUN),
            ("autodiscovery upload", "Uploading a.csv... done (5 bytes)"),  # nothing saves after
        ]
    )
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=["/w/a.csv"],
            dataset_description="cols",
        )
    )
    assert "nothing was spent" in drafted["error"]
    assert drafted["run_id"] == _RUN


def test_submit_refuses_a_run_id_it_did_not_recognise():
    sandbox = _Sandbox([])
    assert "error" in asyncio.run(submit_run(sandbox, "not-a-run"))
    assert sandbox.commands == []


# ---------------------------------------------------------------------------
# Status: the service shouts in caps and the vocabulary is open
# ---------------------------------------------------------------------------


def test_the_uppercase_statuses_map_to_the_words_the_routes_already_speak():
    assert normalise_status("SUCCEEDED") == "completed"
    assert normalise_status("COMPLETED") == "completed"
    assert normalise_status("FAILED") == "failed"
    assert normalise_status("ERROR") == "failed"
    assert normalise_status("CANCELLED") == "canceled"
    assert normalise_status("CANCELED") == "canceled"
    assert normalise_status("DELETED") == "canceled"
    for running in ("CREATED", "PENDING", "RUNNING", "IN_PROGRESS"):
        assert normalise_status(running) == "running", running
    # Case is not load-bearing either way.
    assert normalise_status("succeeded") == "completed"


def test_an_unknown_status_keeps_polling_rather_than_abandoning_a_live_run():
    """§242 was a run nobody polled. An unfamiliar word must not recreate it."""
    assert normalise_status("REHYDRATING") == "running"
    assert normalise_status("") == "running"
    # Unless the experiments response says the work is over, which is the stronger signal.
    assert normalise_status("REHYDRATING", job_completed=True) == "completed"


def test_a_poll_counts_finished_experiments_and_trusts_the_services_surprise_flag():
    experiments = {
        "has_job_completed": True,
        "experiments": [
            {"experiment_id": "node_2_0", "is_surprising": False, "surprise": -0.67},
            {"experiment_id": "node_2_1", "is_surprising": True, "surprise": 0.2},
        ],
    }
    sandbox = _Sandbox(
        [
            ("autodiscovery status", json.dumps({"run_details": {"status": "SUCCEEDED"}})),
            ("autodiscovery experiments", json.dumps(experiments)),
        ]
    )
    result = asyncio.run(poll_discovery_status(sandbox, _RUN))
    assert result["status"] == "completed"
    assert result["completed"] == 2
    # Read off `is_surprising`, never recomputed from `surprise` — the probe had a 0.67 shift
    # flagged false at a 0.5 width.
    assert result["surprising"] == 1


def test_an_unreadable_status_response_is_not_a_finished_run():
    sandbox = _Sandbox([("autodiscovery status", "Error: service unavailable")])
    result = asyncio.run(poll_discovery_status(sandbox, _RUN))
    assert result["status"] == "running"


def test_the_run_status_is_read_not_the_cloud_run_phase():
    """The probe saw these disagree inside one second."""
    payload = {
        "run_details": {"status": "SUCCEEDED"},
        "execution_status": {"phase": "PENDING"},
    }
    sandbox = _Sandbox(
        [
            ("autodiscovery status", json.dumps(payload)),
            ("autodiscovery experiments", json.dumps({"experiments": []})),
        ]
    )
    assert asyncio.run(poll_discovery_status(sandbox, _RUN))["status"] == "completed"


# ---------------------------------------------------------------------------
# Durability — the service keeps the data for a week
# ---------------------------------------------------------------------------


def _probe_experiments() -> list[dict]:
    """The five experiments the §247 probe returned, trimmed to what the writer reads."""
    return [
        {"experiment_id": "node_2_0", "id_in_run": 1, "surprise": -0.6705146629422446,
         "prior": 0.7916666666666666, "posterior": 0.4318181818181818, "is_surprising": False,
         "status": "SUCCEEDED", "runtime_ms": 128220.0,
         "hypothesis": "Elastic Net will select a broader set of signal covariates",
         "analysis": "Lasso selected 11 features, Elastic Net 13."},
        {"experiment_id": "node_3_1", "id_in_run": 4, "surprise": 0.3881926995981416,
         "prior": 0.2917, "posterior": 0.5, "is_surprising": False, "status": "SUCCEEDED",
         "hypothesis": "PLS will capture the variance in the first components",
         "analysis": "cov_000, cov_005 and cov_011 separate from the noise."},
    ]


def test_the_written_report_ranks_by_how_much_a_belief_moved():
    metadata = build_metadata(
        name="SOC covariates",
        description="synthetic control",
        domain="Soil science",
        intent="find the covariates that matter",
        datasets=[{"path": "/w/soc.csv", "size": 10}],
        n_experiments=5,
    )
    text = discovery_markdown(
        metadata,
        {"run_id": _RUN, "surprising": 0, "completed": 2, "experiments": _probe_experiments()},
    )
    # Ranked by magnitude, so the 0.67 comes before the 0.39 despite being created first.
    assert text.index("node_2_0") < text.index("node_3_1")
    # The sign is reported as a direction, not as a subtraction of the beliefs.
    assert "belief moved away" in text
    assert "belief moved toward" in text
    assert "-0.6705" in text
    # Org policy: machine-generated content is disclosed and flagged for expert review.
    assert "subject-matter expert" in text
    assert "AutoDiscovery" in text


def test_a_completed_run_is_written_to_disk_because_the_service_forgets_in_a_week():
    sandbox = _Sandbox([])
    metadata = build_metadata(
        name="run", description="d", domain="soil", intent="i",
        datasets=[{"path": "/w/a.csv", "size": 5}],
    )
    written = asyncio.run(
        persist_discovery_outputs(
            sandbox, _RUN, metadata,
            {"status": "completed", "run_id": _RUN, "completed": 2,
             "surprising": 0, "experiments": _probe_experiments()},
        )
    )
    assert written == [f"/workspace/discovery/{_RUN}.md", f"/workspace/discovery/{_RUN}.json"]
    assert "node_2_0" in sandbox.writes[f"/workspace/discovery/{_RUN}.md"]
    stored = json.loads(sandbox.writes[f"/workspace/discovery/{_RUN}.json"])
    assert stored["metadata"]["n_experiments"] == 15
    assert len(stored["experiments"]) == 2


def test_a_failed_run_still_records_what_happened():
    sandbox = _Sandbox([])
    metadata = build_metadata(
        name="run", description="", domain="", intent="",
        datasets=[{"path": "/w/a.csv", "size": 5}],
    )
    written = asyncio.run(
        persist_discovery_outputs(
            sandbox, _RUN, metadata, {"status": "failed", "completed": 1, "experiments": []}
        )
    )
    assert written == [f"/workspace/discovery/{_RUN}.error.log"]
    assert "status: failed" in sandbox.writes[written[0]]


def test_figures_are_fetched_per_experiment_and_never_by_the_poll():
    """They exist only in the detail response, at ~458KB for one node."""
    sandbox = _Sandbox(
        [("autodiscovery experiment", json.dumps({"ok": True, "figures": ["figure-01.png"]}))]
    )
    outcome = asyncio.run(fetch_experiment_figures(sandbox, _RUN, "node_2_0"))
    assert outcome == {"figures": [f"/workspace/discovery/{_RUN}/node_2_0/figure-01.png"]}

    # §260: a failed fetch is an error, not an experiment that drew nothing — the app caches the
    # second and retries the first, so conflating them turns an auth error into a missing plot.
    broken = _Sandbox([("autodiscovery experiment", "Usage: asta autodiscovery experiment")])
    failed = asyncio.run(fetch_experiment_figures(broken, _RUN, "node_2_0"))
    assert failed["figures"] == []
    assert "Usage: asta autodiscovery experiment" in failed["error"]

    # And a genuine none is an answer with no error attached.
    none = _Sandbox([("autodiscovery experiment", json.dumps({"ok": True, "figures": []}))])
    assert asyncio.run(fetch_experiment_figures(none, _RUN, "node_2_0")) == {"figures": []}
    # A poll must not touch the expensive endpoint.
    poll_sandbox = _Sandbox(
        [
            ("autodiscovery status", json.dumps({"run_details": {"status": "RUNNING"}})),
            ("autodiscovery experiments", json.dumps({"experiments": []})),
        ]
    )
    asyncio.run(poll_discovery_status(poll_sandbox, _RUN))
    assert not any(
        "autodiscovery experiment " in command for command in poll_sandbox.commands
    ), poll_sandbox.commands


def test_the_figure_decoder_prefers_png_and_survives_a_bundle_it_cannot_read():
    """`rich_outputs` carries the same figure four ways; only the PNG is wanted."""
    import base64, subprocess, sys, tempfile, os

    png = base64.b64encode(b"\x89PNG\r\n\x1a\n" + b"x" * 200).decode()
    payload = json.dumps(
        {"experiment": {"rich_outputs": [
            {"image/png": png, "image/jpeg": "not base64 at all", "text/plain": "<Figure>"},
            {"text/plain": "no image here"},
            "not even a dict",
        ]}}
    )
    with tempfile.TemporaryDirectory() as work:
        out = subprocess.run(
            [sys.executable, "-c", _FIGURES_PY, f"{work}/node_2_0", payload],
            capture_output=True, text=True, timeout=60,
        )
        assert out.returncode == 0, out.stderr
        answered = json.loads(out.stdout)
        assert answered["ok"] is True
        assert answered["figures"] == ["figure-01.png"]
        assert os.path.isfile(f"{work}/node_2_0/figure-01.png")

    # A payload that is not a response at all reports why rather than reporting no figures.
    with tempfile.TemporaryDirectory() as work:
        out = subprocess.run(
            [sys.executable, "-c", _FIGURES_PY, f"{work}/node_2_0", "Usage: asta autodiscovery"],
            capture_output=True, text=True, timeout=60,
        )
        refused = json.loads(out.stdout)
        assert refused["ok"] is False
        assert "Usage: asta autodiscovery" in refused["reason"]

    # And a warning printed before the JSON — which keeping stderr makes likely — still parses.
    with tempfile.TemporaryDirectory() as work:
        noisy = "WARNING stale token\n" + json.dumps({"experiment": {"rich_outputs": []}})
        out = subprocess.run(
            [sys.executable, "-c", _FIGURES_PY, f"{work}/node_2_0", noisy],
            capture_output=True, text=True, timeout=60,
        )
        assert json.loads(out.stdout) == {"ok": True, "figures": [], "bundles": 0}


def test_json_is_found_even_when_the_cli_prints_around_it():
    assert _extract_json('WARNING: stale\n{"a": 1}\n') == {"a": 1}
    assert _extract_json('["figure-01.png"]') == ["figure-01.png"]
    assert _extract_json("no json here") is None
    assert _extract_json("") is None


# ---------------------------------------------------------------------------
# The edits the modal offers, and only those
# ---------------------------------------------------------------------------


def test_only_the_budget_and_the_intent_can_be_edited():
    """The modal offers two fields; a request carrying more is not honoured quietly."""
    current = {
        "name": "SOC covariates",
        "intent": "original",
        "n_experiments": 15,
        "datasets": [{"name": "soc.csv"}],
        # A field this backend has never heard of, which must survive the round trip.
        "n_warmstart": 3,
    }
    sandbox = _Sandbox(
        [
            ("metadata-get", json.dumps(current)),
            ("autodiscovery metadata ", "Metadata saved: gs://x"),
        ]
    )
    updated = asyncio.run(
        update_metadata(
            sandbox,
            _RUN,
            {"intent": "steered differently", "n_experiments": 5, "name": "renamed", "datasets": []},
        )
    )
    assert updated["intent"] == "steered differently"
    assert updated["n_experiments"] == 5
    assert updated["name"] == "SOC covariates", "an unapproved edit was applied"
    assert updated["datasets"] == [{"name": "soc.csv"}], "an unapproved edit was applied"
    # Read-modify-write, so a field we do not model is not erased.
    assert updated["n_warmstart"] == 3
    # Staged inside the work dir, and staged *by the command that reads it* — no `awrite`, so
    # nothing can relocate it (§251) and nothing refuses to overwrite it (§256).
    where = metadata_path("/workspace", _RUN)
    assert where == f"/workspace/.asta-autodiscovery-metadata.{_RUN}.json"
    configure = next(c for c in sandbox.commands if "autodiscovery metadata " in c)
    assert where in configure
    assert '"n_experiments": 5' in configure


def test_an_edited_budget_outside_the_bounds_is_refused_before_it_is_staged():
    sandbox = _Sandbox([("metadata-get", json.dumps({"n_experiments": 15}))])
    for bad in (0, MAX_EXPERIMENTS + 1, True, "five"):
        with pytest.raises(ValueError):
            asyncio.run(update_metadata(sandbox, _RUN, {"n_experiments": bad}))
    assert metadata_path("/workspace", _RUN) not in sandbox.writes


def test_a_metadata_save_that_did_not_confirm_raises_with_the_reason():
    """The researcher approved a number. If the edit did not land, submitting would charge for
    whatever the service still had stored — so an unconfirmed save must not look like success.

    And it has to say *why*: "your changes did not save" is true and useless, which is how §255's
    dead end reached the researcher with the reason in /dev/null."""
    sandbox = _Sandbox(
        [
            ("metadata-get", json.dumps({"n_experiments": 15, "intent": "x"})),
            ("autodiscovery metadata ", "Usage: asta autodiscovery metadata [OPTIONS] RUNID"),
        ]
    )
    with pytest.raises(MetadataNotSaved) as raised:
        asyncio.run(update_metadata(sandbox, _RUN, {"n_experiments": 5}))
    assert "Usage: asta autodiscovery metadata" in str(raised.value)

    # A command that printed nothing at all still gets a message rather than an empty one.
    silent = _Sandbox(
        [
            ("metadata-get", json.dumps({"n_experiments": 15})),
            ("autodiscovery metadata ", ""),
        ]
    )
    with pytest.raises(MetadataNotSaved) as quiet:
        asyncio.run(update_metadata(silent, _RUN, {"n_experiments": 5}))
    assert "printed nothing" in str(quiet.value)

    # And a configuration read that fails is its own reason, not a silent falsy return.
    unreadable = _Sandbox([("metadata-get", "Error: not found")])
    with pytest.raises(MetadataNotSaved):
        asyncio.run(update_metadata(unreadable, _RUN, {"n_experiments": 5}))


def test_a_configuration_that_already_matches_is_not_rewritten():
    """The common case — the researcher changes nothing — must not depend on the two calls most
    likely to fail."""
    sandbox = _Sandbox([("metadata-get", json.dumps({"n_experiments": 15, "intent": "x"}))])
    same = asyncio.run(update_metadata(sandbox, _RUN, {"n_experiments": 15, "intent": "x"}))
    assert same["n_experiments"] == 15
    assert sandbox.writes == {}, "nothing staged"
    assert not any("autodiscovery metadata " in c for c in sandbox.commands), sandbox.commands


def test_the_metadata_calls_keep_stderr_so_a_failure_has_a_reason():
    """`2>/dev/null` is right for a JSON parse and wrong for the two calls that stand between a
    press and a run."""
    from backend.autodiscovery_tools import _loud_shell, _json_shell

    assert _loud_shell(["asta", "x"]).endswith("2>&1")
    assert _json_shell(["asta", "x"]).endswith("2>/dev/null")

    sandbox = _Sandbox([("metadata-get", json.dumps({"n_experiments": 1}))])
    asyncio.run(read_metadata(sandbox, _RUN))
    assert sandbox.commands[0].endswith("2>&1"), sandbox.commands[0]


def test_metadata_is_read_from_whichever_endpoint_it_came_from():
    """`metadata-get` returns it bare; the run listing nests it under `run_metadata`."""
    bare = _Sandbox([("metadata-get", json.dumps({"n_experiments": 15, "name": "a"}))])
    assert asyncio.run(read_metadata(bare, _RUN))["name"] == "a"

    nested = _Sandbox([("metadata-get", json.dumps({"run_metadata": {"name": "b"}}))])
    assert asyncio.run(read_metadata(nested, _RUN))["name"] == "b"

    wrapped = _Sandbox([("metadata-get", json.dumps({"metadata": {"name": "c"}}))])
    assert asyncio.run(read_metadata(wrapped, _RUN))["name"] == "c"


def test_only_available_credits_are_reported_as_spendable():
    """Submitting moves credits to `pending`, so `granted` overstates what is left."""
    sandbox = _Sandbox(
        [("autodiscovery credits", json.dumps(
            {"credits": {"granted": 500, "consumed": 5, "pending": 60, "available": 435}}
        ))]
    )
    credits = asyncio.run(read_credits(sandbox))
    assert credits["available"] == 435
    assert credits["granted"] == 500 and credits["pending"] == 60


# ---------------------------------------------------------------------------
# The sandbox is a container, not a view of the researcher's machine (§249)
# ---------------------------------------------------------------------------


def test_a_path_only_the_researchers_laptop_has_resolves_by_filename():
    """The failure that actually happened.

    The app hands the model an absolute host path whenever the attachment has not been copied into
    the conversation folder yet — the normal state of a new conversation's first turn. Inside the
    sandbox that path is nothing, and the run failed on its first step. The resolver looks for the
    filename in the working directory, which is where the file actually is.
    """
    import subprocess
    import sys
    import tempfile

    with tempfile.TemporaryDirectory() as work:
        real = os.path.join(work, "SOC_Covariables_TrainValV5.csv")
        # `newline=""` so the size is the same on Windows, where text mode would translate each
        # \n to \r\n and make this 10 bytes. Windows is the platform ~98% of users are on, so a
        # Unix-only byte count in a test is a defect rather than a detail.
        with open(real, "w", newline="") as handle:
            handle.write("a,b\n1,2\n")
        host_path = "/mnt/c/Users/LENOVO/Downloads/SOC_Covariables_TrainValV5.csv"
        assert not os.path.exists(host_path), "the point is that this path is not here"

        out = subprocess.run(
            [sys.executable, "-c", _RESOLVE_PY, work, host_path],
            capture_output=True, text=True, timeout=60,
        )
        assert out.returncode == 0, out.stderr
        resolved = json.loads(out.stdout)
        # Keyed by what it was given, valued by what it found — so the caller can say it substituted.
        assert resolved[host_path]["path"] == real
        assert resolved[host_path]["size"] == 8

        # A path that resolves nowhere is simply absent, not an error.
        missing = subprocess.run(
            [sys.executable, "-c", _RESOLVE_PY, work, "/mnt/c/nope/absent.csv"],
            capture_output=True, text=True, timeout=60,
        )
        assert json.loads(missing.stdout) == {}


def test_a_path_that_is_already_right_is_left_alone():
    import subprocess
    import sys
    import tempfile

    with tempfile.TemporaryDirectory() as work:
        real = os.path.join(work, "soc.csv")
        with open(real, "w") as handle:
            handle.write("x\n")
        out = subprocess.run(
            [sys.executable, "-c", _RESOLVE_PY, work, real],
            capture_output=True, text=True, timeout=60,
        )
        assert json.loads(out.stdout)[real]["path"] == real


def test_the_upload_reads_the_resolved_path_not_the_one_the_model_named():
    """Uploading the path the model gave would upload nothing; the metadata name comes from the
    resolved one, which is also what the service validates the upload against."""
    host = "/mnt/c/Users/LENOVO/Downloads/soc.csv"
    sandbox = _Sandbox(
        [
            ("_RESOLVE", json.dumps({host: {"path": "/workspace/soc.csv", "size": 12}})),
            ("autodiscovery create", _RUN),
            ("autodiscovery upload", "Uploading soc.csv... done (665894 bytes)"),
            ("autodiscovery metadata ", "Metadata saved: gs://x"),
        ]
    )
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=[host],
            dataset_description="cols",
        )
    )
    assert "error" not in drafted, drafted
    assert drafted["metadata"]["datasets"][0]["name"] == "soc.csv"
    assert drafted["metadata"]["datasets"][0]["file_size_bytes"] == 12
    upload = next(c for c in sandbox.commands if "autodiscovery upload" in c)
    assert "/workspace/soc.csv" in upload
    assert "/mnt/c" not in upload, upload


def test_a_file_the_run_cannot_see_says_what_to_do_about_it():
    """The message a researcher reads. It has to name the cause, not just the symptom."""
    sandbox = _Sandbox([("_RESOLVE", json.dumps({}))])
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=["/mnt/c/Users/LENOVO/Downloads/soc.csv"],
            dataset_description="cols",
        )
    )
    error = drafted["error"]
    assert "/mnt/c" in error, "it names the path that failed"
    assert "not visible to a run" in error, "and why"
    assert "attach the file" in error, "and what to do"
    # Nothing was created, so nothing needs cleaning up.
    assert not any("autodiscovery create" in c for c in sandbox.commands)


def test_the_metadata_is_staged_by_the_command_that_reads_it():
    """§251 and §256, both closed by the same move.

    §251: `awrite` silently relocated a write outside the work dir, so the write and the read named
    different paths. §256: `awrite` is create-only, so the *second* write to a run's path was
    refused outright — which is how a budget change failed and nothing ran.

    Staging through the shell ends both. One command writes the file, hands it to the CLI and
    removes it, so there is no second process to disagree and no create-only semantics to trip on.
    """
    sandbox = _Sandbox(
        [
            ("_RESOLVE", json.dumps({"/w/a.csv": {"path": "/workspace/a.csv", "size": 5}})),
            ("autodiscovery create", _RUN),
            ("autodiscovery upload", "Uploading a.csv... done (5 bytes)"),
            ("autodiscovery metadata ", "Metadata saved: gs://x"),
        ]
    )
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=["/w/a.csv"],
            dataset_description="cols",
        )
    )
    assert "error" not in drafted, drafted

    # Nothing went through the virtual filesystem at all.
    assert sandbox.writes == {}, sandbox.writes

    where = metadata_path("/workspace", _RUN)
    configure = next(c for c in sandbox.commands if "autodiscovery metadata " in c)
    # Written and read in one command, at one path, and removed afterwards.
    assert configure.count(shlex.quote(where)) >= 2, configure
    assert configure.index("printf") < configure.index("autodiscovery metadata")
    assert configure.rstrip().endswith("rm -f " + shlex.quote(where))
    assert "/tmp/" not in configure, configure


def test_an_edit_stages_to_the_same_place_as_the_draft():
    sandbox = _Sandbox(
        [
            ("metadata-get", json.dumps({"n_experiments": 15, "intent": "x"})),
            ("autodiscovery metadata ", "Metadata saved: gs://x"),
        ]
    )
    asyncio.run(update_metadata(sandbox, _RUN, {"n_experiments": 5}))
    assert sandbox.writes == {}
    configure = next(c for c in sandbox.commands if "autodiscovery metadata " in c)
    assert metadata_path("/workspace", _RUN) in configure


def test_a_path_only_the_researchers_laptop_has_resolves_by_filename():
    """The failure that actually happened.

    The app hands the model an absolute host path whenever the attachment has not been copied into
    the conversation folder yet — the normal state of a new conversation's first turn. Inside the
    sandbox that path is nothing, and the run failed on its first step. The resolver looks for the
    filename in the working directory, which is where the file actually is.
    """
    import subprocess
    import sys
    import tempfile

    with tempfile.TemporaryDirectory() as work:
        real = os.path.join(work, "SOC_Covariables_TrainValV5.csv")
        # `newline=""` so the size is the same on Windows, where text mode would translate each
        # \n to \r\n and make this 10 bytes. Windows is the platform ~98% of users are on, so a
        # Unix-only byte count in a test is a defect rather than a detail.
        with open(real, "w", newline="") as handle:
            handle.write("a,b\n1,2\n")
        host_path = "/mnt/c/Users/LENOVO/Downloads/SOC_Covariables_TrainValV5.csv"
        assert not os.path.exists(host_path), "the point is that this path is not here"

        out = subprocess.run(
            [sys.executable, "-c", _RESOLVE_PY, work, host_path],
            capture_output=True, text=True, timeout=60,
        )
        assert out.returncode == 0, out.stderr
        resolved = json.loads(out.stdout)
        # Keyed by what it was given, valued by what it found — so the caller can say it substituted.
        assert resolved[host_path]["path"] == real
        assert resolved[host_path]["size"] == 8

        # A path that resolves nowhere is simply absent, not an error.
        missing = subprocess.run(
            [sys.executable, "-c", _RESOLVE_PY, work, "/mnt/c/nope/absent.csv"],
            capture_output=True, text=True, timeout=60,
        )
        assert json.loads(missing.stdout) == {}


def test_a_path_that_is_already_right_is_left_alone():
    import subprocess
    import sys
    import tempfile

    with tempfile.TemporaryDirectory() as work:
        real = os.path.join(work, "soc.csv")
        with open(real, "w") as handle:
            handle.write("x\n")
        out = subprocess.run(
            [sys.executable, "-c", _RESOLVE_PY, work, real],
            capture_output=True, text=True, timeout=60,
        )
        assert json.loads(out.stdout)[real]["path"] == real


def test_the_upload_reads_the_resolved_path_not_the_one_the_model_named():
    """Uploading the path the model gave would upload nothing; the metadata name comes from the
    resolved one, which is also what the service validates the upload against."""
    host = "/mnt/c/Users/LENOVO/Downloads/soc.csv"
    sandbox = _Sandbox(
        [
            ("_RESOLVE", json.dumps({host: {"path": "/workspace/soc.csv", "size": 12}})),
            ("autodiscovery create", _RUN),
            ("autodiscovery upload", "Uploading soc.csv... done (665894 bytes)"),
            ("autodiscovery metadata ", "Metadata saved: gs://x"),
        ]
    )
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=[host],
            dataset_description="cols",
        )
    )
    assert "error" not in drafted, drafted
    assert drafted["metadata"]["datasets"][0]["name"] == "soc.csv"
    assert drafted["metadata"]["datasets"][0]["file_size_bytes"] == 12
    upload = next(c for c in sandbox.commands if "autodiscovery upload" in c)
    assert "/workspace/soc.csv" in upload
    assert "/mnt/c" not in upload, upload


def test_a_file_the_run_cannot_see_says_what_to_do_about_it():
    """The message a researcher reads. It has to name the cause, not just the symptom."""
    sandbox = _Sandbox([("_RESOLVE", json.dumps({}))])
    drafted = asyncio.run(
        draft_run(
            sandbox,
            name="run",
            description="d",
            domain="soil",
            intent="i",
            dataset_paths=["/mnt/c/Users/LENOVO/Downloads/soc.csv"],
            dataset_description="cols",
        )
    )
    error = drafted["error"]
    assert "/mnt/c" in error, "it names the path that failed"
    assert "not visible to a run" in error, "and why"
    assert "attach the file" in error, "and what to do"
    # Nothing was created, so nothing needs cleaning up.
    assert not any("autodiscovery create" in c for c in sandbox.commands)


# ---------------------------------------------------------------------------
# §252: the approval token, and re-reading the budget before spending
# ---------------------------------------------------------------------------


def test_an_approval_token_authorises_one_run_at_one_budget():
    from backend.routes.artifacts import _check_approval, _issue_approval, _spend_approval

    token = _issue_approval("thread-1", _RUN, 5)
    # Bound to the thread and the run it was issued for, or the check is a formality.
    assert _check_approval(token, "thread-2", _RUN) is None
    assert _check_approval(token, "thread-1", "other-run") is None
    assert _check_approval("not-a-token", "thread-1", _RUN) is None
    assert _check_approval("", "thread-1", _RUN) is None

    # Checking does NOT consume, so a submit that fails before spending can be retried. §255: the
    # first version consumed on arrival, and a failed save left the next press with no approval.
    assert _check_approval(token, "thread-1", _RUN) == 5
    assert _check_approval(token, "thread-1", _RUN) == 5, "checking is repeatable"

    # Spending consumes it, immediately before the charge — so a replay cannot pay twice.
    _spend_approval(token)
    assert _check_approval(token, "thread-1", _RUN) is None


def test_submitting_re_reads_the_budget_and_refuses_a_number_nobody_approved():
    """The race a review found: the modal said 5, something else moved the run to 500, and the
    press would have started a 500-credit run."""
    sandbox = _Sandbox(
        [
            ("metadata-get", json.dumps({"n_experiments": 500})),
            ("autodiscovery submit", "Submitted. Execution ID: x"),
        ]
    )
    refused = asyncio.run(submit_run(sandbox, _RUN, approved=5))
    assert "error" in refused
    assert "500 experiments, not the 5 you approved" in refused["error"]
    assert not any("autodiscovery submit" in c for c in sandbox.commands), sandbox.commands

    # And when the stored budget is the approved one, it goes ahead.
    agreeing = _Sandbox(
        [
            ("metadata-get", json.dumps({"n_experiments": 5})),
            ("autodiscovery submit", "Submitted. Execution ID: x"),
        ]
    )
    assert asyncio.run(submit_run(agreeing, _RUN, approved=5))["status"] == "running"


def test_two_runs_stage_their_metadata_in_different_files():
    """A shared staging file is a race: A writes 5, B overwrites with 500, and A's `metadata`
    command saves B's budget onto A's run."""
    other = "11111111-2222-3333-4444-555555555555"
    assert metadata_path("/workspace", _RUN) != metadata_path("/workspace", other)
    assert _RUN in metadata_path("/workspace", _RUN)


# ---------------------------------------------------------------------------
# §258: the service knows whether a run was approved; our artifact may not
# ---------------------------------------------------------------------------


def test_only_created_means_a_run_was_never_submitted():
    """A discovery artifact is written by a turn and approving is not a turn, so an approved run's
    record can sit at `awaiting_approval` indefinitely. Re-offering the budget gate for a run that
    is already finished is worse than not offering it, so the service is asked."""
    from backend.autodiscovery_tools import already_submitted

    def answering(status):
        return _Sandbox(
            [("autodiscovery status", json.dumps({"run_details": {"status": status}}))]
        )

    # The one status that means drafted-and-never-started.
    assert asyncio.run(already_submitted(answering("CREATED"), _RUN)) is False
    assert asyncio.run(already_submitted(answering("created"), _RUN)) is False

    # Everything else means it has been started — including a word nobody has seen, because the
    # vocabulary is open (§247) and erring toward *not* asking for money twice is the safe side.
    for started in ("PENDING", "RUNNING", "SUCCEEDED", "FAILED", "CANCELLED", "REHYDRATING"):
        assert asyncio.run(already_submitted(answering(started), _RUN)) is True, started


def test_an_unreadable_status_says_nothing_rather_than_guessing():
    """`None` is a third answer, and the caller must not treat it as either: it does not know
    whether a press would be a second charge."""
    from backend.autodiscovery_tools import already_submitted

    assert asyncio.run(already_submitted(_Sandbox([("autodiscovery status", "boom")]), _RUN)) is None
    assert (
        asyncio.run(
            already_submitted(
                _Sandbox([("autodiscovery status", json.dumps({"run_details": {}}))]), _RUN
            )
        )
        is None
    )
    # And an id it will not touch.
    assert asyncio.run(already_submitted(_Sandbox([]), "not-a-run")) is None
