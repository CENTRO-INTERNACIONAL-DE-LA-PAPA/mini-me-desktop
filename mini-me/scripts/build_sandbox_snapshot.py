"""Build the `mini-me-base` LangSmith Sandbox snapshot.

Spins up a fresh sandbox from the default Ubuntu 24.04 base image, installs
the numerical / reporting stack that our subagents rely on, then captures
the result as a reusable snapshot. Idempotent: if `mini-me-base` already
exists it short-circuits (delete it manually with `--rebuild` if needed).

Usage:
    LANGSMITH_API_KEY=... uv run python scripts/build_sandbox_snapshot.py
    LANGSMITH_API_KEY=... uv run python scripts/build_sandbox_snapshot.py --rebuild

The build runs `apt update` + Python package installs inside the sandbox.
PyMC compiles pytensor on first import, so expect ~10–15 min wall time end
to end. Free-tier LangSmith allows 1 concurrent sandbox; this script will
fail with QuotaExceededError if another sandbox is already running.
"""

from __future__ import annotations

import argparse
import asyncio
import os
import sys
import time
from pathlib import Path

from langsmith.sandbox import AsyncSandboxClient, ResourceNotFoundError

SNAPSHOT_NAME = "mini-me-base"
BUILD_SANDBOX_NAME = "minime-snapshot-builder"

# Each entry is (label, command). They run in order; any non-zero exit
# aborts the build (the sandbox is still deleted on exit).
APT_PACKAGES = "python3-pip pandoc curl build-essential ca-certificates"

CORE_PY_PACKAGES = " ".join(
    [
        "numpy",
        "pandas",
        "scipy",
        "matplotlib",
        "seaborn",
        "scikit-learn",
        "statsmodels",
        "pyarrow",
        "openpyxl",
        "pyreadr",
        "dabest",
        "pointblank",
        "typst",  # Python bindings to the typst typesetter (no CLI)
    ]
)

# PyMC is heavy (pulls in pytensor, jax, etc.) and benefits from a separate
# pip step so failures are easier to diagnose.
BAYESIAN_PY_PACKAGES = "pymc arviz"

BUILD_STEPS: list[tuple[str, str, int]] = [
    # (label, command, timeout_seconds)
    ("apt update", "apt-get update -y", 180),
    ("apt install", f"DEBIAN_FRONTEND=noninteractive apt-get install -y {APT_PACKAGES}", 600),
    # Skip pip self-upgrade — system pip is debian-packaged and can't be
    # uninstalled cleanly. Pip 24.x is sufficient for the installs below.
    (
        "pip core",
        f"pip3 install --break-system-packages --no-cache-dir {CORE_PY_PACKAGES}",
        1200,
    ),
    (
        "pip bayesian",
        f"pip3 install --break-system-packages --no-cache-dir {BAYESIAN_PY_PACKAGES}",
        1500,
    ),
    (
        "pip asta",
        "pip3 install --break-system-packages --no-cache-dir "
        "'git+https://github.com/allenai/asta-plugins.git@v0.101.0'",
        600,
    ),
    (
        "asta smoke",
        "asta --version",
        30,
    ),
    (
        "smoke imports",
        (
            "python3 -c \""
            "import numpy, pandas, scipy, sklearn, statsmodels, "
            "matplotlib, seaborn, dabest, pointblank, pyarrow, openpyxl; "
            "print('core ok')\""
        ),
        60,
    ),
    (
        "pymc smoke",
        "python3 -c \"import pymc, arviz; print('pymc', pymc.__version__)\"",
        180,
    ),
    ("pandoc smoke", "pandoc --version | head -1", 30),
    (
        "typst smoke",
        "python3 -c \"import typst; print('typst-py', typst.__version__)\"",
        30,
    ),
    (
        "workspace dir",
        "mkdir -p /workspace && chmod 755 /workspace",
        10,
    ),
    (
        "apt clean",
        "apt-get clean && rm -rf /var/lib/apt/lists/*",
        60,
    ),
]


def _api_key_from_env_or_file() -> str:
    key = os.getenv("LANGSMITH_API_KEY")
    if key:
        return key
    env_path = Path(__file__).resolve().parent.parent / ".env"
    if env_path.is_file():
        for line in env_path.read_text().splitlines():
            if line.startswith("LANGSMITH_API_KEY="):
                return line.split("=", 1)[1].strip().strip('"').strip("'")
    raise SystemExit("LANGSMITH_API_KEY not set and not found in .env")


async def _existing_snapshot(client: AsyncSandboxClient) -> bool:
    snapshots = await client.list_snapshots(name_contains=SNAPSHOT_NAME)
    return any(getattr(s, "name", "") == SNAPSHOT_NAME for s in snapshots)


async def _cleanup_leftovers(client: AsyncSandboxClient) -> None:
    for box in await client.list_sandboxes():
        try:
            await client.delete_sandbox(box.id)
            print(f"  deleted leftover sandbox {box.id} ({getattr(box, 'name', '')})")
        except Exception as exc:  # noqa: BLE001
            print(f"  could not delete {box.id}: {exc}")


async def build_snapshot(rebuild: bool) -> int:
    os.environ["LANGSMITH_API_KEY"] = _api_key_from_env_or_file()
    # The SDK client defaults to a 10s HTTP timeout on every request. That is
    # far too short for the snapshot-capture POST, which blocks server-side
    # while the full scientific stack (+ asta) is imaged — it fires an
    # httpx.ReadTimeout long before the server responds. Raise the per-request
    # timeout generously; it is a ceiling, so fast calls still return promptly.
    client = AsyncSandboxClient(timeout=600.0)

    if not rebuild and await _existing_snapshot(client):
        print(f"snapshot '{SNAPSHOT_NAME}' already exists — nothing to do")
        print("  pass --rebuild to delete and rebuild it")
        return 0

    if rebuild:
        # Best-effort delete of any existing snapshot with this name.
        for s in await client.list_snapshots(name_contains=SNAPSHOT_NAME):
            if getattr(s, "name", "") == SNAPSHOT_NAME:
                try:
                    await client.delete_snapshot(s.id)
                    print(f"deleted existing snapshot {s.id}")
                except Exception as exc:  # noqa: BLE001
                    print(f"could not delete existing snapshot: {exc}")

    print("cleaning up any leftover sandboxes...")
    await _cleanup_leftovers(client)
    await asyncio.sleep(2)

    print(f"creating builder sandbox '{BUILD_SANDBOX_NAME}'...")
    sandbox = await client.create_sandbox(
        name=BUILD_SANDBOX_NAME,
        idle_ttl_seconds=3600,
        delete_after_stop_seconds=3600,
    )
    print(f"  sandbox id: {sandbox.id}")

    failed = False
    started = time.monotonic()
    try:
        for label, cmd, timeout in BUILD_STEPS:
            t0 = time.monotonic()
            print(f"\n>>> {label} (timeout {timeout}s)")
            print(f"    $ {cmd}")
            try:
                result = await sandbox.run(cmd, timeout=timeout)
            except Exception as exc:  # noqa: BLE001
                print(f"    !! step raised: {type(exc).__name__}: {exc}")
                failed = True
                break
            elapsed = time.monotonic() - t0
            tail_stdout = "\n".join(result.stdout.splitlines()[-8:])
            tail_stderr = "\n".join(result.stderr.splitlines()[-8:])
            print(f"    exit {result.exit_code} in {elapsed:.1f}s")
            if tail_stdout:
                print(f"    stdout (tail):\n      " + tail_stdout.replace("\n", "\n      "))
            if tail_stderr:
                print(f"    stderr (tail):\n      " + tail_stderr.replace("\n", "\n      "))
            if result.exit_code != 0:
                print(f"    !! step '{label}' failed; aborting build")
                failed = True
                break

        if not failed:
            print(f"\ncapturing snapshot '{SNAPSHOT_NAME}'...")
            snapshot = await sandbox.capture_snapshot(SNAPSHOT_NAME, timeout=600)
            elapsed = time.monotonic() - started
            print(f"  snapshot id: {snapshot.id} (total build time: {elapsed/60:.1f} min)")
            return 0
        return 1
    finally:
        print("\ndeleting builder sandbox...")
        try:
            await client.delete_sandbox(sandbox.id)
            print("  deleted")
        except Exception as exc:  # noqa: BLE001
            print(f"  delete failed: {exc}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--rebuild",
        action="store_true",
        help="Delete existing snapshot (if any) and rebuild from scratch",
    )
    args = parser.parse_args()
    return asyncio.run(build_snapshot(rebuild=args.rebuild))


if __name__ == "__main__":
    # Force line-buffered stdout so progress shows up live when redirected.
    sys.stdout.reconfigure(line_buffering=True)
    sys.exit(main())
