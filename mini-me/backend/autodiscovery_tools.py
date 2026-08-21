"""Asta AutoDiscovery: draft a run, poll it, and read one back.

AutoDiscovery (`asta autodiscovery`) is not a question with an answer. It runs an
MCTS search over hypotheses it writes itself, each experiment executing code
against an uploaded dataset and reporting how far the result moved a prior
belief. The output is a tree of measured belief shifts.

**Submitting is not a tool, and that is most of the design of this module.**
1 credit = 1 experiment, 1–500 per run, out of a grant this account cannot
top up. The CLI's own `submit` asks for confirmation with `click.confirm`, which
a sandbox with no TTY cannot answer — so the backend has to pass `-y`, and the
guard has to live somewhere else. It lives in the desktop app: the agent can only
ever *draft* a run (`draft_discovery_run` — create, upload, save metadata, all
free), and `submit_run` is a plain function reachable only from the route the
approval modal posts to. There is no *tool* a model can call that spends a credit.

**Which is not the same as "no path", and a review found the difference.**
`execute` is a general shell every agent keeps, `ASTA_TOKEN` is injected into
every command it runs, and `asta autodiscovery submit <id> -y` is a shell command
— so with `approve_execute = false` a model could have spent the whole grant with
no press at all. `middleware/no_spending.py` refuses those commands now, the route
requires a one-shot token the app issues when the modal opens, and `submit_run`
re-reads the budget before spending. None of that is airtight against a model
actively working around it; all of it closes the path a confused one would take.
The residual is written down in docs §252 rather than claimed away.

Every field name here was read off a captured response, not the documentation.
The probe in docs §247 found nine places where the documented shape was wrong or
incomplete; the ones that matter to this module are noted at their use sites. The
frozen payloads live in `crates/app/tests/fixtures/autodiscovery-*.json`.

One hard fact about the service worth stating up front: **uploaded datasets
expire seven days after creation** (`dataset_expires_at`). The service is not an
archive, so `persist_discovery_outputs` is not a convenience — it is the only
place a run's results survive.
"""

import json
import logging
import shlex
from typing import Any

from langchain_core.tools import tool

from backend import diagnostics
from backend.runtime import _active_sandbox

#: Reaches the log at INFO — see `backend/diagnostics.py` for why that needs saying.
logger = diagnostics.arriving(__name__)

_DRAFT_TIMEOUT_S = 600  # upload of a real dataset goes to GCS through a presigned URL
_SUBMIT_TIMEOUT_S = 180
_STATUS_TIMEOUT_S = 90
_FIGURE_TIMEOUT_S = 180

#: What a run costs, in experiments, when nobody says otherwise.
#:
#: Settled by the researcher who owns the credits: *"A normal run size is about 15 experiments."*
#: Inside the skill's own 10–20 "explore" band, and 33 runs' worth of a 500-credit grant rather
#: than 10.
DEFAULT_EXPERIMENTS = 15

#: The service's own bounds. 1 credit each, so the ceiling is also a bill.
MIN_EXPERIMENTS = 1
MAX_EXPERIMENTS = 500

#: Filename the drafted metadata is staged under, **inside the working directory**.
#:
#: Not `/tmp`, and that was a real failure rather than a style preference. The sandbox adapter's
#: `_resolve_for_write` rewrites *any* write target outside `SANDBOX_WORK_DIR` to
#: `<work_dir>/<basename>` — deliberately, because deepagents treats a leading `/` as the project
#: root while the sandbox's `/` is a POSIX root the run cannot write to. So `awrite` reported
#: success, the file landed in the work dir, and the shell command that followed read
#: `/tmp/asta-autodiscovery-metadata.json` and found nothing:
#:
#:     Error: Invalid value for '--file' / '-f': Path '/tmp/asta-autodiscovery-metadata.json'
#:     does not exist.
#:
#: A leading dot so it does not show up as one of the researcher's own files in the panel.
_METADATA_NAME = ".asta-autodiscovery-metadata"


def metadata_path(work_dir: str, run_id: str = "") -> str:
    """Where the staged metadata lives, for whoever writes it *and* whoever reads it.

    One function so the two cannot drift — the failure above was a write and a read naming
    different paths.

    **Per run**, which is the second half of that lesson. A single shared staging file is a race: a
    modal showing 5 experiments for run A writes the file, a concurrent draft for run B overwrites
    it with 500, and A's `metadata` command reads whatever is there and saves *B's* budget onto A.
    Then A's "spend 5" press starts a 500-credit run. Found in review rather than in production
    (§252), and the fix is to stop sharing the file.
    """
    base = (work_dir or ".").rstrip("/")
    if run_id:
        return f"{base}/{_METADATA_NAME}.{run_id}.json"
    return f"{base}/{_METADATA_NAME}.json"

#: Cap on how much of an experiment's narrative crosses back to the model. The full response is
#: 8–12KB per experiment, dominated by `code` and `code_output`; a 100-experiment run is ~1MB and
#: the durable file carries the detail either way.
_TEXT_CAP = 12_000


# ---------------------------------------------------------------------------
# CLI contract — pure argv builders (unit-tested; kept out of the model's hands)
# ---------------------------------------------------------------------------


def _build_create_command() -> list[str]:
    """`asta autodiscovery create` — prints a bare run id, and costs nothing."""
    return ["asta", "autodiscovery", "create"]


def _build_upload_command(run_id: str, dataset_paths: list[str]) -> list[str]:
    """`asta autodiscovery upload RUNID FILES...` — variadic, and free."""
    return ["asta", "autodiscovery", "upload", run_id, *dataset_paths]


def _build_metadata_command(run_id: str, staged: str) -> list[str]:
    """`asta autodiscovery metadata RUNID --file PATH`.

    `--file` is *required* by the CLI; there is no inline form. `staged` must come from
    [`metadata_path`] — the path is not defaulted here, because a default is exactly how the write
    and the read came to disagree.
    """
    return ["asta", "autodiscovery", "metadata", run_id, "--file", staged]


def _build_submit_command(run_id: str) -> list[str]:
    """`asta autodiscovery submit RUNID -y` — the one call that spends credits.

    `-y` is not an optimisation. Without it the CLI calls `click.confirm`, and a sandbox has no
    TTY to answer with, so the command would block until its timeout and spend nothing while
    looking like a failure. Which is exactly why the human gate is upstream in the app rather than
    here (see the module docstring).
    """
    return ["asta", "autodiscovery", "submit", run_id, "-y"]


def _build_status_command(run_id: str) -> list[str]:
    """`asta autodiscovery status RUNID --format json`."""
    return ["asta", "autodiscovery", "status", run_id, "--format", "json"]


def _build_experiments_command(run_id: str) -> list[str]:
    """`asta autodiscovery experiments RUNID --format json`.

    Returns *whole* experiments — hypothesis, analysis, review, code, code_output, parent_id and
    child_ids — in one request, so the tree costs one call and not one per node. The single thing
    it does not return is `rich_outputs`, which comes back `null` here even for experiments that
    have figures.
    """
    return ["asta", "autodiscovery", "experiments", run_id, "--format", "json"]


def _build_experiment_command(run_id: str, experiment_id: str) -> list[str]:
    """`asta autodiscovery experiment RUNID EXPERIMENT_ID --format json`.

    The only place figures exist — ~458KB for a single experiment — so this is called per
    experiment and on demand. Two warnings about the response, both from §247: its `child_ids` is
    **empty** even when the list endpoint reports children for the same node, so it must never be
    used to build the tree; and its `rich_outputs` is a Jupyter display bundle carrying the same
    figure as PNG, JPEG, SVG and a text repr.
    """
    return ["asta", "autodiscovery", "experiment", run_id, experiment_id, "--format", "json"]


def _build_metadata_get_command(run_id: str) -> list[str]:
    """`asta autodiscovery metadata-get RUNID` — JSON on stdout, no `--format` flag of its own.

    Note the asymmetry, which is the CLI's and not ours: saving is `metadata --file`, reading is
    `metadata-get`, and only the first takes a flag.
    """
    return ["asta", "autodiscovery", "metadata-get", run_id]


def _build_credits_command() -> list[str]:
    """`asta autodiscovery credits --format json`.

    Returns `granted`, `consumed`, `pending` and `available`. **Only `available` is safe to show a
    researcher**: submitting moves credits to `pending` immediately and to `consumed` on
    completion, and `available` is the one that already nets both off. A gate quoting `granted`
    while three runs held 60 in flight would be wrong by sixty.
    """
    return ["asta", "autodiscovery", "credits", "--format", "json"]


def is_valid_run_id(run_id: str) -> bool:
    """Whether a string is plausibly a run id, before it reaches a shell or a URL path."""
    text = (run_id or "").strip()
    if len(text) != 36 or text.count("-") != 4:
        return False
    return all(c in "0123456789abcdefABCDEF-" for c in text)


def is_valid_experiment_id(experiment_id: str) -> bool:
    """Whether a string is plausibly an `experiment_id` (`node_2_0`).

    Deliberately narrow: this value is interpolated into a shell command, and the ids the service
    issues are `node_<int>_<int>`. Anything else is refused rather than quoted and hoped for.
    """
    text = (experiment_id or "").strip()
    if not text.startswith("node_"):
        return False
    parts = text[len("node_") :].split("_")
    return len(parts) == 2 and all(part.isdigit() for part in parts)


# ---------------------------------------------------------------------------
# Metadata — pure, because it is the thing a human approves
# ---------------------------------------------------------------------------


def build_metadata(
    *,
    name: str,
    description: str,
    domain: str,
    intent: str,
    datasets: list[dict[str, Any]],
    n_experiments: int = DEFAULT_EXPERIMENTS,
) -> dict[str, Any]:
    """Assemble the metadata the service stores for a run.

    Pure, and that is deliberate: this dict is what the approval modal renders, so it has to be
    constructible and assertable without a sandbox.

    `datasets` entries are `{"path", "description", "size", "content_type"}`. **The name the
    service stores is derived from the path's basename here and nowhere else** — the docs require
    `datasets[].name` to match the uploaded filename exactly, and letting a caller pass a name
    alongside a path is an invitation to have them disagree.

    Four fields the skill documents as tunable (`exploration_weight`, `mcts_selection`,
    `surprisal_width`, `evidence_weight`) are deliberately absent, so the service applies its own
    defaults. The researcher edits `intent` and the budget; a form with six tuning knobs is a form
    nobody fills in correctly.
    """
    if not (name or "").strip():
        raise ValueError("a discovery run needs a name")
    if not datasets:
        raise ValueError("a discovery run needs at least one dataset")
    if not isinstance(n_experiments, int) or isinstance(n_experiments, bool):
        raise ValueError("n_experiments must be a whole number of experiments")
    if not MIN_EXPERIMENTS <= n_experiments <= MAX_EXPERIMENTS:
        raise ValueError(
            f"n_experiments must be between {MIN_EXPERIMENTS} and {MAX_EXPERIMENTS} "
            f"(1 credit each); got {n_experiments}"
        )

    entries = []
    for dataset in datasets:
        path = str(dataset.get("path") or "").strip()
        if not path:
            raise ValueError("every dataset needs a path")
        entries.append(
            {
                "name": path.rsplit("/", 1)[-1],
                "description": str(dataset.get("description") or "").strip(),
                "content_type": str(dataset.get("content_type") or "text/csv"),
                "file_size_bytes": int(dataset.get("size") or 0),
                # Always false for a CLI upload, per the service's own field guide.
                "is_preloaded": False,
            }
        )

    return {
        "name": name.strip(),
        "description": (description or "").strip(),
        "domain": (domain or "").strip(),
        "intent": (intent or "").strip(),
        "datasets": entries,
        "n_experiments": n_experiments,
    }


def cost_of(metadata: dict[str, Any]) -> int:
    """What submitting this metadata will spend, in credits. One per experiment."""
    return int(metadata.get("n_experiments") or 0)


# ---------------------------------------------------------------------------
# Shell — one place that quotes, so nothing else has to remember to
# ---------------------------------------------------------------------------


#: In-sandbox resolver for the dataset paths the model was given.
#:
#: **Why a resolver and not a `getsize`.** The sandbox is a container with its own filesystem, not
#: a view of the researcher’s machine. An attachment reaches it by being synced from the
#: conversation folder into the working directory — so a path like
#: ``/mnt/c/Users/.../Downloads/soc.csv`` is a real file on their laptop and nothing at all in
#: here. The app hands the model that absolute form whenever the file has not been copied into the
#: conversation folder yet, which is the normal state of the first turn of a new conversation
#: (§236): the folder does not exist until the backend has issued a thread id.
#:
#: So each path is tried as given, then by its basename in the working directory and in the
#: current directory. The file the researcher attached is almost always sitting there under exactly
#: that name, and failing on the prefix would be refusing to look in the one place it is.
#:
#: Reports where each one was found, so the caller can say so rather than silently substituting.
#: Apostrophe-free: the whole script is `shlex.quote`d into a single-quoted shell string.
_RESOLVE_PY = """
# _RESOLVE marks this script in a sandbox transcript and in the test doubles that match on it.
import json, os, sys
work = sys.argv[1]
found = {}
for given in sys.argv[2:]:
    name = os.path.basename(given)
    for candidate in (given, os.path.join(work, name), name):
        if candidate and os.path.isfile(candidate):
            found[given] = {"path": os.path.abspath(candidate), "size": os.path.getsize(candidate)}
            break
print(json.dumps(found))
"""


def _sizes_shell(dataset_paths: list[str], work_dir: str = ".") -> str:
    """Resolve each dataset path and print `{given: {path, size}}` as JSON.

    One round trip rather than one per file, and a path that resolves nowhere is simply absent
    from the result instead of an error — `draft_run` reports which ones it could not find, and
    where it looked, which is more useful to a researcher than a shell diagnostic.
    """
    script = shlex.quote(_RESOLVE_PY)
    args = " ".join(shlex.quote(part) for part in [work_dir, *dataset_paths])
    return f"python3 -c {script} {args}"


def _configure_shell(run_id: str, staged: str, metadata: dict[str, Any]) -> str:
    """Write the metadata, hand it to the CLI, and delete it — in one command.

    **Staged through the shell rather than through `awrite`, and each clause is a bug it fixes.**

    `awrite` is create-only. The draft wrote this path; the edit wrote it again and the deepagents
    filesystem refused:

        Cannot write to …/.asta-autodiscovery-metadata.<run>.json because it already exists.
        Read and then make an edit, or write to a new path.

    So the researcher pressed "Run 5 and spend 5", the budget change could not be staged, and
    nothing ran (§256). `>` truncates, which is the semantics this actually needs — the file is
    transient, written and read seconds apart, and never wanted again.

    It also removes §251's whole failure mode at the root: `_resolve_for_write` silently relocates a
    write outside the work dir, and the process that writes the file here is the one that reads it,
    so there is nothing left to disagree.

    And it cleans up. The staging file lands in the conversation's own folder — in local execution
    that is a directory the researcher browses — so it is removed after use rather than left as a
    dotfile nobody can explain. `;` not `&&` before the `rm`, so a failed save is still tidied.
    """
    payload = shlex.quote(json.dumps(metadata, indent=2, ensure_ascii=False))
    target = shlex.quote(staged)
    configure = " ".join(
        shlex.quote(part) for part in _build_metadata_command(run_id, staged)
    )
    return (
        f"printf %s {payload} > {target} "
        f"|| echo 'could not stage the configuration file'; "
        f"{configure} 2>&1; rm -f {target}"
    )


def _upload_shell(run_id: str, dataset_paths: list[str]) -> str:
    """Upload the datasets, keeping stderr so a failure says why.

    Separate from the configure step now that staging is a shell write too. They used to be chained
    with `&&` — metadata naming a file the upload did not deliver would configure a run against data
    that is not there — and the ordering is preserved by the caller instead, which also gets to tell
    the two failures apart.
    """
    upload = " ".join(shlex.quote(part) for part in _build_upload_command(run_id, dataset_paths))
    return f"{upload} 2>&1"


def _json_shell(argv: list[str]) -> str:
    """Run a `--format json` command, with stderr dropped so a warning cannot break the parse."""
    return " ".join(shlex.quote(part) for part in argv) + " 2>/dev/null"


def _loud_shell(argv: list[str]) -> str:
    """Run a command and keep stderr, for the ones whose *failure* is the interesting part.

    `2>/dev/null` is right for a JSON parse and wrong here. The metadata read and save are the two
    calls that stand between a researcher's press and their run, and when one failed the app said
    "your changes did not save" while the reason — a `Usage:` line, an auth error, a path — went to
    /dev/null. That is §249's paraphrased-away failure in a different costume (§255).
    """
    return " ".join(shlex.quote(part) for part in argv) + " 2>&1"


#: In-sandbox decoder for one experiment's figures.
#:
#: `rich_outputs` is a list of Jupyter display bundles, each carrying the *same* figure under
#: several MIME types — `image/png`, `image/jpeg`, `image/svg+xml` and a `text/plain` repr. We take
#: the PNG, exactly as §242's DataVoyager decoder does, and write `figure-NN.png`. Runs entirely in
#: the sandbox so 458KB of base64 per experiment never crosses back.
#:
#: Apostrophe-free on purpose: the whole script is `shlex.quote`d into a single-quoted shell string.
_FIGURES_PY = """
import base64, json, os, sys
run_dir, payload = sys.argv[1], sys.argv[2]
try:
    record = json.loads(payload)
except Exception:
    record = {}
experiment = record.get("experiment") or record
bundles = experiment.get("rich_outputs") or []
os.makedirs(run_dir, exist_ok=True)
written = []
for index, bundle in enumerate(bundles, start=1):
    if not isinstance(bundle, dict):
        continue
    encoded = bundle.get("image/png") or bundle.get("image/jpeg")
    if not isinstance(encoded, str) or len(encoded) < 64:
        continue
    suffix = "png" if bundle.get("image/png") else "jpg"
    name = "figure-%02d.%s" % (index, suffix)
    try:
        raw = base64.b64decode(encoded, validate=True)
    except Exception:
        continue
    with open(os.path.join(run_dir, name), "wb") as handle:
        handle.write(raw)
    written.append(name)
print(json.dumps(written))
"""


def _figures_shell(run_id: str, experiment_id: str, run_dir: str) -> str:
    """Fetch one experiment and decode its figures into `run_dir`, printing the names written.

    The fetch and the decode are one command so the base64 stays inside the sandbox. Separated by
    `;` rather than `&&` after the capture, so a decode still runs on a response the CLI wrote to
    stdout alongside a non-zero exit.
    """
    fetch = _json_shell(_build_experiment_command(run_id, experiment_id))
    script = shlex.quote(_FIGURES_PY)
    target = shlex.quote(f"{run_dir}/{experiment_id}")
    return (
        f"OUT=$({fetch}); "
        f"printf %s \"$OUT\" | python3 -c {script} {target} \"$(cat)\" 2>/dev/null"
    )


async def _run(sandbox: Any, command: str, timeout: int) -> str:
    """Run a command in the sandbox and return its stdout, whichever shape it answers in.

    Prefers the untruncated path: an experiments response is tens of kilobytes of JSON and a
    truncated one is unparseable. Both response shapes are handled for the reason §224 taught the
    hard way — a dict-shaped response read only for attributes yields an empty string, which is
    indistinguishable from a command that printed nothing.
    """
    runner = getattr(sandbox, "aexecute_untruncated", None) or sandbox.aexecute
    resp = await runner(command, timeout=timeout)
    if isinstance(resp, dict):
        return resp.get("output") or ""
    return getattr(resp, "output", "") or ""


def _extract_json(output: str) -> Any:
    """Pull the first JSON value out of command output that may carry log lines around it."""
    text = (output or "").strip()
    if not text:
        return None
    for opener, closer in (("{", "}"), ("[", "]")):
        start = text.find(opener)
        end = text.rfind(closer)
        if start != -1 and end > start:
            try:
                return json.loads(text[start : end + 1])
            except json.JSONDecodeError:
                continue
    return None


# ---------------------------------------------------------------------------
# Status — the service shouts in caps and this is where that stops
# ---------------------------------------------------------------------------

#: Service statuses that mean the run is still going. Uppercase, because they arrive that way.
#:
#: `CREATED` is in here and is not in the CLI's own icon table, which is how §247 learned the
#: vocabulary is open rather than documented.
_RUNNING_STATUSES = frozenset({"CREATED", "PENDING", "RUNNING", "IN_PROGRESS", "QUEUED"})

#: Service statuses that mean it has stopped, mapped to the words this backend's routes already
#: speak — the same four `analyze_data_status` returns, so the frontend needs no new vocabulary.
_TERMINAL_STATUSES = {
    "SUCCEEDED": "completed",
    "COMPLETED": "completed",
    "FAILED": "failed",
    "ERROR": "failed",
    "CANCELLED": "canceled",
    "CANCELED": "canceled",
    "DELETED": "canceled",
}


def normalise_status(raw: str, *, job_completed: bool | None = None) -> str:
    """Map a service status to `completed` / `failed` / `canceled` / `running`.

    Two sources, on purpose. `run_details.status` is the run's own — and note it is **not**
    `execution_status.phase`, which reports the Cloud Run job and disagreed with it inside one
    second during the probe: `submit` printed `RUNNING` while `status` returned `PENDING`.

    An unrecognised status counts as running *unless* `has_job_completed` says otherwise, and is
    logged either way. The alternative — treating an unknown word as terminal — would abandon a
    live run, and abandoning a run is how §242 ended up with six charts nobody could see. A new
    in-progress word therefore costs some polling and a log line; it cannot lose a result.
    """
    status = (raw or "").strip().upper()
    if status in _TERMINAL_STATUSES:
        return _TERMINAL_STATUSES[status]
    if job_completed:
        # The experiments response says the work is over even though the status is a word we do
        # not know. Believe the stronger signal, and say so.
        logger.warning(
            "autodiscovery reported has_job_completed with an unfamiliar status %r", status
        )
        return "completed"
    if status not in _RUNNING_STATUSES:
        logger.warning("autodiscovery returned an unfamiliar status %r; treating as running", status)
    return "running"


# ---------------------------------------------------------------------------
# Operations
# ---------------------------------------------------------------------------


async def _work_dir(sandbox: Any) -> str:
    try:
        return await sandbox.aget_work_dir()
    except Exception:  # noqa: BLE001
        return "/workspace"


async def draft_run(
    sandbox: Any,
    *,
    name: str,
    description: str,
    domain: str,
    intent: str,
    dataset_paths: list[str],
    dataset_description: str,
    n_experiments: int = DEFAULT_EXPERIMENTS,
) -> dict[str, Any]:
    """Create a run, upload its datasets, and save its metadata. Spends nothing.

    Returns `{"run_id", "metadata", "cost", "missing"}`, or `{"error"}`. Deliberately stops one
    call short of `submit`: everything here is free and reversible, and the next step is not.
    """
    work_dir = await _work_dir(sandbox)
    resolved_out = await _run(sandbox, _sizes_shell(dataset_paths, work_dir), _STATUS_TIMEOUT_S)
    resolved = _extract_json(resolved_out)
    resolved = resolved if isinstance(resolved, dict) else {}
    missing = [path for path in dataset_paths if path not in resolved]
    # The path the upload will actually read, which may not be the one the model named — see
    # `_RESOLVE_PY`. Kept in the order the caller gave them.
    present = [
        (given, resolved[given]["path"], int(resolved[given].get("size") or 0))
        for given in dataset_paths
        if given in resolved and isinstance(resolved.get(given), dict)
    ]
    for given, actual, _ in present:
        if given != actual:
            # Said out loud rather than silently substituted: the researcher attached a file and
            # the model was told a path that only exists on their own machine.
            logger.info("resolved discovery dataset %s to %s", given, actual)
    if not present:
        # The most likely cause by far, and worth naming in the message rather than leaving a
        # researcher to guess: the sandbox is a container, and a path under /mnt/c or C:\\ is on
        # their laptop and nowhere the run can reach.
        logger.warning(
            "no discovery dataset resolved; looked for %s under %s",
            ", ".join(dataset_paths),
            work_dir,
        )
        return {
            "error": (
                "none of those files exist in the sandbox. Looked for "
                + ", ".join(dataset_paths)
                + f" and for their filenames in {work_dir}. A path on the researcher's own "
                "machine (anything under /mnt/c or C:) is not visible to a run — attach the file "
                "to the conversation so it is copied in, then try again."
            ),
            "missing": missing,
        }

    try:
        metadata = build_metadata(
            name=name,
            description=description,
            domain=domain,
            intent=intent,
            datasets=[
                {
                    "path": actual,
                    "description": dataset_description,
                    "size": size,
                    "content_type": "text/csv" if actual.lower().endswith(".csv") else None,
                }
                for _, actual, size in present
            ],
            n_experiments=n_experiments,
        )
    except ValueError as exc:
        return {"error": str(exc), "missing": missing}

    created = await _run(sandbox, _json_shell(_build_create_command()), _STATUS_TIMEOUT_S)
    # `create` prints a bare id, not JSON — so this is the one response that is parsed by looking
    # at the text rather than through `_extract_json`.
    run_id = (created or "").strip().splitlines()[-1].strip() if created.strip() else ""
    if not is_valid_run_id(run_id):
        logger.warning("autodiscovery create returned no run id in %d char(s)", len(created or ""))
        return {"error": "could not create a discovery run; the service returned no id"}

    uploads = [actual for _, actual, _ in present]
    # Upload first: metadata naming a file the upload did not deliver configures a run against data
    # that is not there, and the service validates the two against each other.
    uploaded = await _run(sandbox, _upload_shell(run_id, uploads), _DRAFT_TIMEOUT_S)
    if "done" not in (uploaded or "").lower():
        logger.warning("autodiscovery upload run=%s did not confirm: %.300s", run_id, uploaded)
        return {
            "error": (
                "the datasets did not upload, so nothing was configured and nothing was spent: "
                + (" ".join((uploaded or "").split())[:300] or "the command printed nothing")
            ),
            "run_id": run_id,
        }

    staged = metadata_path(work_dir, run_id)
    out = await _run(
        sandbox, _configure_shell(run_id, staged, metadata), _STATUS_TIMEOUT_S
    )
    if "Metadata saved" not in (out or ""):
        logger.warning("autodiscovery draft run=%s did not confirm metadata: %.400s", run_id, out)
        return {
            "error": (
                "the datasets uploaded but the run configuration did not save, so nothing was "
                "spent: "
                + (" ".join((out or "").split())[:300] or "the command printed nothing")
            ),
            "run_id": run_id,
        }

    logger.info(
        "drafted a discovery run=%s experiments=%d datasets=%s",
        run_id,
        cost_of(metadata),
        ", ".join(uploads),
    )
    return {
        "run_id": run_id,
        "metadata": metadata,
        "cost": cost_of(metadata),
        "missing": missing,
    }


async def read_credits(sandbox: Any) -> dict[str, Any]:
    """The credit balance, so the approval modal can state the cost against it."""
    out = await _run(sandbox, _json_shell(_build_credits_command()), _STATUS_TIMEOUT_S)
    payload = _extract_json(out)
    credits = payload.get("credits") if isinstance(payload, dict) else None
    if not isinstance(credits, dict):
        credits = payload if isinstance(payload, dict) else {}
    # `available` is the only one that nets off runs already in flight (see `_build_credits_command`).
    return {
        "available": credits.get("available"),
        "granted": credits.get("granted"),
        "consumed": credits.get("consumed"),
        "pending": credits.get("pending"),
    }


async def read_metadata(sandbox: Any, run_id: str) -> dict[str, Any]:
    """The metadata the service currently holds for a run.

    What the approval modal renders. Read back from the service rather than remembered locally,
    because the run may have been drafted in an earlier session — or forked — and the server's copy
    is the one `submit` will act on.
    """
    if not is_valid_run_id(run_id):
        return {}
    out = await _run(sandbox, _loud_shell(_build_metadata_get_command(run_id)), _STATUS_TIMEOUT_S)
    payload = _extract_json(out)
    if isinstance(payload, dict):
        # The run listing nests this under `run_metadata`; `metadata-get` returns it bare. Accept
        # both so a caller does not have to know which endpoint it came from.
        inner = payload.get("metadata") or payload.get("run_metadata")
        return inner if isinstance(inner, dict) else payload
    logger.warning("autodiscovery metadata-get run=%s unreadable: %.200s", run_id, out)
    return {}


class MetadataNotSaved(RuntimeError):
    """A drafted run's configuration could not be changed, with the service's own words attached.

    An exception rather than a falsy return so the reason cannot be dropped on the way out. The
    first version returned `{}` and the route turned that into "your changes did not save", which
    is true and useless — the researcher cannot act on it and neither can anyone reading a bug
    report (§255).
    """


async def update_metadata(
    sandbox: Any, run_id: str, changes: dict[str, Any]
) -> dict[str, Any]:
    """Apply the researcher's edits to a drafted run, before it is submitted.

    Only `intent` and `n_experiments` may be changed, and this is where that is enforced rather
    than in the route: the modal offers exactly those two, and a request carrying anything else is
    either a mistake or something nobody agreed to. Unknown keys are dropped and logged.

    Read-modify-write against the service's own copy, so a field this backend has never heard of
    survives the round trip instead of being erased by a rewrite.
    """
    allowed = {"intent", "n_experiments"}
    if not is_valid_run_id(run_id):
        raise MetadataNotSaved("not a run id")
    rejected = sorted(set(changes) - allowed)
    if rejected:
        logger.warning("ignoring unapproved discovery metadata edits: %s", ", ".join(rejected))
    patch = {key: value for key, value in changes.items() if key in allowed}
    if not patch:
        return await read_metadata(sandbox, run_id)

    current = await read_metadata(sandbox, run_id)
    if not current:
        raise MetadataNotSaved(
            "could not read this run's configuration back from the service, so nothing was changed"
        )

    # **Nothing to do is not a failure.** If the stored configuration already matches what was
    # approved — the common case when the researcher changes nothing — there is no write and no CLI
    # call, so the two things most likely to fail simply do not happen.
    if all(current.get(key) == value for key, value in patch.items()):
        logger.info("discovery run=%s already configured as approved", run_id)
        return current

    updated = {**current, **patch}
    if "n_experiments" in patch:
        budget = patch["n_experiments"]
        if not isinstance(budget, int) or isinstance(budget, bool):
            raise ValueError("n_experiments must be a whole number of experiments")
        if not MIN_EXPERIMENTS <= budget <= MAX_EXPERIMENTS:
            raise ValueError(
                f"n_experiments must be between {MIN_EXPERIMENTS} and {MAX_EXPERIMENTS} "
                f"(1 credit each); got {budget}"
            )

    staged = metadata_path(await _work_dir(sandbox), run_id)
    out = await _run(
        sandbox, _configure_shell(run_id, staged, updated), _STATUS_TIMEOUT_S
    )
    if "Metadata saved" not in (out or ""):
        logger.warning("edited metadata for %s did not save: %.400s", run_id, out)
        detail = " ".join((out or "").split())[:300] or "the command printed nothing"
        raise MetadataNotSaved(f"the service did not save the change: {detail}")
    logger.info(
        "updated a drafted discovery run=%s experiments=%s", run_id, updated.get("n_experiments")
    )
    return updated


async def submit_run(sandbox: Any, run_id: str, *, approved: int | None = None) -> dict[str, Any]:
    """Spend the credits and start the run. **Only ever called for an approved draft.**

    Not a tool, not exported to the model, and not reachable from a turn: the single caller is the
    route the desktop app's approval modal posts to. If this function ever acquires a second
    caller, the credit gate has been removed.
    """
    if not is_valid_run_id(run_id):
        return {"error": "not a run id"}

    # **Read back what will actually be charged.** The researcher approved a number; between their
    # press and this call another client, another app instance or a concurrent draft could have
    # changed the run's stored budget. Verifying here rather than trusting the earlier save closes
    # that window, and it costs one cheap request on a call that spends money (§252).
    if approved is not None:
        stored = await read_metadata(sandbox, run_id)
        actual = stored.get("n_experiments")
        if actual != approved:
            logger.warning(
                "refusing to submit run=%s: approved %s experiments, service holds %s",
                run_id,
                approved,
                actual,
            )
            return {
                "error": (
                    f"this run is now configured for {actual} experiments, not the {approved} "
                    "you approved — nothing was submitted. Open it again to approve the new number."
                )
            }

    out = await _run(
        sandbox, _json_shell(_build_submit_command(run_id)), _SUBMIT_TIMEOUT_S
    )
    text = out or ""
    if "Submitted" not in text:
        logger.warning("autodiscovery submit run=%s did not confirm: %.300s", run_id, text)
        return {"error": "the service did not confirm the submission", "output": text[:500]}
    logger.info("submitted a discovery run=%s", run_id)
    # The printed status is the *submit* response's, which the probe saw disagree with the status
    # endpoint one second later. Report what it said and let the poll be authoritative.
    return {"status": "running", "run_id": run_id, "output": text[:500]}


async def poll_discovery_status(sandbox: Any, run_id: str) -> dict[str, Any]:
    """Where a run has got to, cheaply enough to call on a timer.

    Two calls, because they answer different questions: `status` gives the run's own state and
    `experiments` gives how many are done — which is the only progress number with an honest
    denominator, since `n_experiments` was fixed at submit.
    """
    if not is_valid_run_id(run_id):
        return {"status": "error", "message": "not a run id"}

    status_out = await _run(sandbox, _json_shell(_build_status_command(run_id)), _STATUS_TIMEOUT_S)
    status_payload = _extract_json(status_out)
    if not isinstance(status_payload, dict):
        # No readable answer is not the same as a finished run. Say running so the next tick tries
        # again, and log it so a persistent failure is visible rather than silent.
        logger.warning("autodiscovery status run=%s unreadable: %.200s", run_id, status_out)
        return {"status": "running", "run_id": run_id, "completed": 0}

    details = status_payload.get("run_details")
    details = details if isinstance(details, dict) else {}

    experiments_out = await _run(
        sandbox, _json_shell(_build_experiments_command(run_id)), _STATUS_TIMEOUT_S
    )
    experiments_payload = _extract_json(experiments_out)
    experiments_payload = experiments_payload if isinstance(experiments_payload, dict) else {}
    experiments = experiments_payload.get("experiments")
    experiments = experiments if isinstance(experiments, list) else []
    job_completed = experiments_payload.get("has_job_completed")

    status = normalise_status(
        str(details.get("status") or ""),
        job_completed=bool(job_completed) if isinstance(job_completed, bool) else None,
    )
    return {
        "status": status,
        "run_id": run_id,
        "completed": len(experiments),
        "surprising": sum(1 for item in experiments if isinstance(item, dict) and item.get("is_surprising")),
        "finished_at": details.get("finished_at"),
        # The whole experiments payload, for the caller that persists it. Never handed to a model.
        "experiments": experiments,
    }


# ---------------------------------------------------------------------------
# Durability — the service keeps the data for a week, so this is the archive
# ---------------------------------------------------------------------------


def discovery_markdown(metadata: dict[str, Any], result: dict[str, Any]) -> str:
    """A run's results as something a person reads, ranked by how much each moved a belief.

    Ordered by `|surprise|` rather than by creation, because the reason to read this file is to
    find the experiments that changed the picture. The sign is kept as a direction word — it *is*
    the direction, and the magnitudes of `surprise` and of the belief move are different numbers
    (§247).
    """
    experiments = [item for item in (result.get("experiments") or []) if isinstance(item, dict)]

    def magnitude(item: dict[str, Any]) -> float:
        value = item.get("surprise")
        return abs(value) if isinstance(value, (int, float)) else 0.0

    ranked = sorted(experiments, key=magnitude, reverse=True)
    lines = [
        f"# {metadata.get('name') or 'Discovery run'}",
        "",
        f"- **Run**: `{result.get('run_id', '')}`",
        f"- **Experiments**: {len(experiments)} of {metadata.get('n_experiments', '?')} requested",
        f"- **Flagged surprising by the service**: {result.get('surprising', 0)}",
    ]
    if metadata.get("domain"):
        lines.append(f"- **Domain**: {metadata['domain']}")
    if metadata.get("intent"):
        lines.append(f"- **Intent**: {metadata['intent']}")
    lines += [
        "",
        "> Generated by Asta AutoDiscovery, an AI system that writes and runs its own",
        "> experiments. Every hypothesis and every number below is machine-produced and needs a",
        "> subject-matter expert before it is relied on.",
        "",
    ]
    for item in ranked:
        surprise = item.get("surprise")
        direction = ""
        if isinstance(surprise, (int, float)) and surprise != 0:
            direction = " (belief moved toward)" if surprise > 0 else " (belief moved away)"
        lines.append(f"## {item.get('experiment_id', '?')} — experiment {item.get('id_in_run', '?')}")
        lines.append("")
        lines.append(f"**Hypothesis.** {item.get('hypothesis') or '(none recorded)'}")
        lines.append("")
        score = f"{surprise:+.4f}" if isinstance(surprise, (int, float)) else "—"
        lines.append(
            f"- Surprise `{score}`{direction}; "
            f"flagged surprising: {'yes' if item.get('is_surprising') else 'no'}"
        )
        prior, posterior = item.get("prior"), item.get("posterior")
        if isinstance(prior, (int, float)) and isinstance(posterior, (int, float)):
            lines.append(f"- Belief {prior:.3f} → {posterior:.3f}")
        runtime = item.get("runtime_ms")
        if isinstance(runtime, (int, float)):
            lines.append(f"- Ran for {runtime / 1000:.0f}s, status {item.get('status', '?')}")
        for label, key in (("Analysis", "analysis"), ("Review", "review")):
            body = item.get(key)
            if isinstance(body, str) and body.strip():
                lines += ["", f"**{label}.** {body.strip()[:_TEXT_CAP]}"]
        lines.append("")
    return "\n".join(lines)


async def persist_discovery_outputs(
    sandbox: Any, run_id: str, metadata: dict[str, Any], result: dict[str, Any]
) -> list[str]:
    """Write a terminal run to disk as `discovery/<run_id>.md` + `.json`.

    **Not a convenience.** `dataset_expires_at` is creation plus seven days, so the service is not
    an archive: after a week the run's own inputs are gone. If this does not run, the only copy of
    a result the researcher paid credits for is a web page.

    Figures are *not* fetched here. They live only in the per-experiment response at roughly 458KB
    each, so a 15-experiment run would be 7MB of base64 on a code path that fires from a status
    poll. `fetch_experiment_figures` gets them one at a time, when something asks.

    Best-effort, like its DataVoyager counterpart: a write failure is logged, never raised into a
    poll response.
    """
    awrite = getattr(sandbox, "awrite", None)
    if awrite is None:
        return []
    base = f"{await _work_dir(sandbox)}/discovery"
    status = result.get("status")
    written: list[str] = []
    try:
        if status == "completed":
            targets = {
                f"{base}/{run_id}.md": discovery_markdown(metadata, result),
                f"{base}/{run_id}.json": json.dumps(
                    {"run_id": run_id, "metadata": metadata, **result},
                    indent=2,
                    ensure_ascii=False,
                ),
            }
        else:
            targets = {
                f"{base}/{run_id}.error.log": (
                    f"run_id: {run_id}\n"
                    f"name: {metadata.get('name', '')}\n"
                    f"status: {status}\n"
                    f"experiments completed: {result.get('completed', 0)}\n"
                )
            }
        for path, content in targets.items():
            res = await awrite(path, content)
            err = getattr(res, "error", None)
            if err:
                logger.warning("failed to persist %s: %s", path, err)
            else:
                written.append(path)
    except Exception as exc:  # noqa: BLE001
        logger.warning("persist_discovery_outputs failed for %s: %s", run_id, exc)
    return written


async def fetch_experiment_figures(sandbox: Any, run_id: str, experiment_id: str) -> list[str]:
    """Decode one experiment's figures into `discovery/<run_id>/<experiment_id>/`.

    On demand and one experiment at a time, because this is the expensive call: the figures exist
    only in the per-experiment response and that response is ~458KB for a single node. The base64
    never leaves the sandbox — the decode runs there and only the filenames come back.
    """
    if not is_valid_run_id(run_id) or not is_valid_experiment_id(experiment_id):
        return []
    run_dir = f"{await _work_dir(sandbox)}/discovery/{run_id}"
    out = await _run(sandbox, _figures_shell(run_id, experiment_id, run_dir), _FIGURE_TIMEOUT_S)
    names = _extract_json(out)
    if not isinstance(names, list):
        logger.warning(
            "no figures decoded for run=%s experiment=%s from %d char(s)",
            run_id,
            experiment_id,
            len(out or ""),
        )
        return []
    return [f"{run_dir}/{experiment_id}/{name}" for name in names if isinstance(name, str)]


# ---------------------------------------------------------------------------
# The one tool the model gets, and it cannot spend anything
# ---------------------------------------------------------------------------


@tool
async def draft_discovery_run(
    name: str,
    description: str,
    domain: str,
    intent: str,
    dataset_paths: str,
    dataset_description: str,
    n_experiments: int = DEFAULT_EXPERIMENTS,
) -> str:
    """Prepare an Asta AutoDiscovery run over a local dataset, for the researcher to approve.

    AutoDiscovery decides its own hypotheses: it searches over experiments, writes and runs code
    for each, and reports how far the result moved its prior belief. Use it when the question is
    "what is in this data" rather than a specific analytical question — `analyze_data` is the tool
    for a question you can already state.

    This DRAFTS the run and does not start it. Each experiment costs one credit from the
    researcher's grant, so starting it needs their explicit approval, which happens in the desktop
    app and not here. You will get back a run id and the drafted configuration; report those and
    stop. Do not claim any results — there are none yet.

    Args:
        name: a short title for the run.
        description: what the data is, where it came from, and its known gaps. The service
            conditions its hypotheses on this, so write it as if for a collaborator who has never
            seen the file.
        domain: the research field, e.g. "Soil science" or "Plant pathology".
        intent: how to steer the exploration, without naming the answer. "Focus on how temperature
            affects yield", not "test whether temperature above 30C cuts yield by 20%".
        dataset_paths: one or more sandbox paths to tabular files, comma- or newline-separated.
        dataset_description: what the columns mean and their units.
        n_experiments: how many experiments to run, 1–500, one credit each. Leave at the default
            unless the researcher asked for a different number.
    """
    sandbox = _active_sandbox.get()
    if sandbox is None:
        return "No sandbox is available, so a discovery run cannot be prepared."

    paths = [
        part.strip()
        for chunk in (dataset_paths or "").replace("\n", ",").split(",")
        if (part := chunk.strip())
    ]
    if not paths:
        return "No dataset paths were given, so there is nothing to run discovery over."

    drafted = await draft_run(
        sandbox,
        name=name,
        description=description,
        domain=domain,
        intent=intent,
        dataset_paths=paths,
        dataset_description=dataset_description,
        n_experiments=n_experiments,
    )
    if error := drafted.get("error"):
        return f"Could not prepare the discovery run: {error}"

    credits = await read_credits(sandbox)
    cost = drafted["cost"]
    available = credits.get("available")
    lines = [
        f"Drafted discovery run {drafted['run_id']}.",
        f"It will run {cost} experiment(s), costing {cost} credit(s)"
        + (f" of {available} available." if available is not None else "."),
        "It has NOT started. The researcher approves the budget in the app before anything runs.",
    ]
    if drafted.get("missing"):
        lines.append("Not found, and left out: " + ", ".join(drafted["missing"]))
    return "\n".join(lines)
