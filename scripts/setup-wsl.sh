#!/usr/bin/env bash
# Provision the Mini-Me backend for the desktop app, inside WSL2 (or any Linux).
#
# Why WSL: the agent stack shells out with POSIX commands and needs bash/python3/
# the asta CLI, none of which behave under cmd.exe. Inside WSL the backend simply
# is on Linux, so nothing upstream changes — and the desktop app reaches it over
# localhost, which WSL2 forwards.
#
# The desktop app runs this itself, from the Setup pane, and shows the output line
# by line. So every message here is written for someone who does not write code:
# no unexplained jargon, and no step that ends in "now go and edit this file".
#
# Safe to re-run. It never overwrites an existing checkout, and never touches a
# checkout it did not create.
#
#   Usage:  bash setup-wsl.sh [target-dir]
#           default target: ~/.local/share/mini-me-desktop/backend
#
#   MINIME_BUNDLED_SOURCE   a Mini-Me copy shipped with the app, to copy from
#                           instead of cloning (see "where the source comes from")
#   MINIME_REPO_URL         override the git remote

set -euo pipefail

REPO_URL="${MINIME_REPO_URL:-https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me.git}"
DIR="${1:-$HOME/.local/share/mini-me-desktop/backend}"
# Expand a leading ~ if the caller passed one through as a literal.
DIR="${DIR/#\~/$HOME}"

# Resolved here, *before* anything cds anywhere. `${BASH_SOURCE[0]}` is whatever the
# caller typed, so after `cd "$DIR"` a relative invocation resolves against the wrong
# directory — which silently skipped the overlay install the first time this was run.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

say() { printf '\n==> %s\n' "$1"; }
ok()  { printf '    ok  %s\n' "$1"; }
bad() { printf '    !!  %s\n' "$1"; }

if grep -qiE "(microsoft|wsl)" /proc/version 2>/dev/null; then
  ok "running inside WSL"
else
  ok "running on Linux (not WSL — that's fine)"
fi

# ------------------------------------------------------------------------ tools
say "Checking what is already installed"
if ! command -v git >/dev/null 2>&1; then
  bad "git is missing. Install it with:  sudo apt-get update && sudo apt-get install -y git"
  exit 1
fi
ok "git $(git --version | awk '{print $3}')"

if ! command -v uv >/dev/null 2>&1; then
  say "Installing uv (the Python package manager)"
  curl -LsSf https://astral.sh/uv/install.sh | sh
  # uv lands in ~/.local/bin; make it visible to this script and to future shells.
  export PATH="$HOME/.local/bin:$PATH"
  if ! grep -qs '\.local/bin' "$HOME/.bashrc"; then
    echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
    ok "added ~/.local/bin to PATH in ~/.bashrc"
  fi
fi
ok "uv $(uv --version | awk '{print $2}')"

# ------------------------------------------------- where the source comes from
#
# In preference order, and the order matters:
#
#   1. A copy bundled with the app. THIS IS THE ONE THAT MATTERS for a real
#      install: Mini-Me is a *private* repository, so `git clone` demands
#      credentials that GitHub only issues as a personal access token — something
#      no scientist should have to create in order to open an app. Whoever builds
#      the installer runs scripts/bundle-backend.sh once, and this path is free.
#   2. A checkout already on this machine — copied, not downloaded again.
#   3. A checkout on the Windows side, same.
#   4. git clone. The developer path, and the only one that can ask for a
#      password, which is why it is last.
find_source() {
  local candidate
  if [ -n "${MINIME_BUNDLED_SOURCE:-}" ] && [ -f "${MINIME_BUNDLED_SOURCE}/langgraph.json" ]; then
    printf '%s' "$MINIME_BUNDLED_SOURCE"
    return 0
  fi
  for candidate in \
      "$HOME/Mini-Me" "$HOME/mini-me" "$HOME/Documents/Mini-Me" \
      /mnt/c/Users/*/Documents/GitHub/Mini-Me /mnt/c/Users/*/Documents/Mini-Me; do
    if [ -f "$candidate/langgraph.json" ] && [ "$candidate" != "$DIR" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

if [ -f "$DIR/langgraph.json" ]; then
  ok "Mini-Me is already here: $DIR"
else
  mkdir -p "$(dirname "$DIR")"
  # A failed clone can leave an empty directory behind; it would block the copy.
  if [ -d "$DIR" ] && [ -z "$(ls -A "$DIR" 2>/dev/null)" ]; then rmdir "$DIR"; fi

  if SOURCE="$(find_source)"; then
    say "Copying Mini-Me from $SOURCE"
    echo "    (faster than downloading, and it needs no password)"
    cp -r "$SOURCE" "$DIR"
    # A copied .venv holds the *other* machine's compiled packages — Windows
    # Scripts/*.exe, or wheels built for a different Python. Unusable here.
    if [ -d "$DIR/.venv" ]; then
      rm -rf "$DIR/.venv"
      ok "removed the copied environment (this machine needs to build its own)"
    fi
    ok "copied to $DIR"
  else
    say "Downloading Mini-Me"
    echo "    This is a private repository, so git will ask who you are."
    echo "    GitHub does NOT accept your account password here — it wants a"
    echo "    personal access token. If you are reading this inside the app, the"
    echo "    backend was not bundled with your copy: ask whoever gave it to you."
    git clone "$REPO_URL" "$DIR"
  fi
fi

cd "$DIR"

# ------------------------------------------------------------------ the overlay
#
# Copied *into the distro* rather than left on the Windows filesystem. Host
# execution works by putting this directory on the backend's PYTHONPATH, and a
# path under /mnt/c is reachable only while the app's own folder still exists and
# the drive is still mounted. Three small files; copying them removes a whole
# class of silent failure (docs §25).
OVERLAY_SRC="$(cd "$HERE/../overlay" 2>/dev/null && pwd || true)"
if [ -n "$OVERLAY_SRC" ] && [ -f "$OVERLAY_SRC/sitecustomize.py" ]; then
  say "Installing the local-execution overlay"
  rm -rf "$DIR/.desktop-overlay"
  cp -r "$OVERLAY_SRC" "$DIR/.desktop-overlay"
  # Bytecode from the source copy would be stale here and is never wanted.
  find "$DIR/.desktop-overlay" -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true
  ok "overlay installed here, so it no longer depends on the Windows drive"
else
  bad "could not find the overlay next to this script — the app will use its own copy"
fi

# ------------------------------------------------------------------ dependencies
# --extra dev is REQUIRED: langgraph-cli lives in an optional extra, so a plain
# `uv sync` leaves you with the server libraries but no `langgraph` entry point.
say "Installing Python packages (a few minutes the first time)"
echo "    This pulls the scientific stack — PyMC, scikit-learn and friends."
uv sync --extra dev
if [ -x .venv/bin/langgraph ]; then
  ok "the backend can be started"
else
  bad ".venv/bin/langgraph is missing even after --extra dev."
  bad "The desktop app cannot start the backend without it."
  exit 1
fi

# -------------------------------------------------------------------------- env
# Deliberately NOT a key template any more. Keys live in the desktop app's
# settings panel and travel with each request from the OS keychain (docs §20/§22),
# so nobody has to edit a file inside a Linux distro to get started. An empty
# .env is still written because `langgraph dev` auto-loads one when present, and
# its absence has made people think they missed a step.
if [ ! -f .env ]; then
  cat > .env <<'ENV'
# Nothing needs to go in here.
#
# Your API keys live in the desktop app: open Settings and paste them there. They
# are kept in your operating system's keychain and sent with each request, so they
# are never written to a file on disk.
ENV
  ok "wrote an (intentionally empty) .env"
fi

# ------------------------------------------------------------------------- done
say "Done — Mini-Me is ready."
echo "    Location: $DIR"
echo
echo "    Back in the app, press Re-check. If a model key is still missing, open"
echo "    Settings and paste one. Nothing else needs doing here."
