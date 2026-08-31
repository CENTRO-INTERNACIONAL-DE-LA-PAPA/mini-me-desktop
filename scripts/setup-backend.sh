#!/usr/bin/env bash
# Provision the Mini-Me backend for the desktop app, natively (macOS/Linux dev use, CI).
#
# There is no WSL step here any more: the backend runs on this machine directly, and
# Asta is a normal Python dependency of this checkout (a path dependency on the
# `asta-plugins` submodule, resolved by `uv sync` below) rather than a separately
# installed CLI. See `scripts/setup-backend.ps1` for the native-Windows equivalent —
# the two are kept in step; a change to one almost always belongs in the other too.
#
# The desktop app runs this itself, from the Setup pane, and shows the output line
# by line. So every message here is written for someone who does not write code:
# no unexplained jargon, and no step that ends in "now go and edit this file".
#
# Safe to re-run. It never overwrites a checkout it did not create.
#
#   Usage:  bash setup-backend.sh [target-dir]
#           default target: ~/.local/share/mini-me-desktop/backend
#           asta-plugins lands as a SIBLING of target-dir (../asta-plugins), because
#           that is the relative path mini-me/pyproject.toml's `[tool.uv.sources]`
#           expects — the same layout this repo itself uses at the root.
#
#   MINIME_BUNDLED_SOURCE   a Mini-Me copy shipped with the app, to copy from
#                           instead of cloning
#   MINIME_REPO_URL         override the Mini-Me git remote
#   ASTA_BUNDLED_SOURCE     an asta-plugins copy shipped with the app, to copy from
#                           instead of cloning
#   ASTA_REPO_URL           override the asta-plugins git remote

set -euo pipefail

REPO_URL="${MINIME_REPO_URL:-https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me.git}"
ASTA_REPO_URL="${ASTA_REPO_URL:-git@github.com:allenai/asta-plugins.git}"
DIR="${1:-$HOME/.local/share/mini-me-desktop/backend}"
# Expand a leading ~ if the caller passed one through as a literal.
DIR="${DIR/#\~/$HOME}"
ASTA_DIR="$(dirname "$DIR")/asta-plugins"

# Resolved here, *before* anything cds anywhere. `${BASH_SOURCE[0]}` is whatever the
# caller typed, so after `cd "$DIR"` a relative invocation resolves against the wrong
# directory — which silently skipped the overlay install the first time this was run.
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

say() { printf '\n==> %s\n' "$1"; }
ok()  { printf '    ok  %s\n' "$1"; }
bad() { printf '    !!  %s\n' "$1"; }

# ------------------------------------------------------------------------ tools
say "Checking what is already installed"
if ! command -v git >/dev/null 2>&1; then
  bad "git is missing. Install it with your system's package manager."
  exit 1
fi
ok "git $(git --version | awk '{print $3}')"

# Make ~/.local/bin reachable by the shell the *app* launches the backend with.
#
# That shell is `bash -lc` — a login shell that is NOT interactive — and it reads
# ~/.profile, never ~/.bashrc: Ubuntu's .bashrc returns in its first few lines when
# `$-` has no `i`. A PATH line written only to .bashrc would be invisible to the
# backend, which is where `uv` has to be found when a command runs.
ensure_local_bin_on_path() {
  local file
  for file in "$HOME/.profile" "$HOME/.bashrc"; do
    [ -e "$file" ] || : > "$file"
    if ! grep -qsF '.local/bin' "$file"; then
      printf '\nexport PATH="$HOME/.local/bin:$PATH"\n' >> "$file"
      ok "added ~/.local/bin to PATH in $(basename "$file")"
    fi
  done
}

if ! command -v uv >/dev/null 2>&1; then
  say "Installing uv (the Python package manager)"
  curl -LsSf https://astral.sh/uv/install.sh | sh
  # Visible to the rest of *this* script, too.
  export PATH="$HOME/.local/bin:$PATH"
fi
ensure_local_bin_on_path
ok "uv $(uv --version | awk '{print $2}')"

# ------------------------------------------------------------- generic checkout
#
# Both Mini-Me and asta-plugins are private repositories bundled with the app so a
# from-scratch install needs no GitHub credentials (whoever builds the installer runs
# scripts/bundle-backend.sh once, and that path is free). In preference order:
#
#   1. A copy bundled with the app (env var below).
#   2. A checkout already on this machine — copied, not downloaded again.
#   3. git clone. The developer path, and the only one that can ask for a
#      password, which is why it is last.
#
# $1 name (for messages), $2 target dir, $3 marker file relative to the checkout root,
# $4 bundled-source env var value, $5 repo url, $6.. candidate local checkout dirs.
find_checkout_source() {
  local marker="$3" bundled="$4"
  shift 5
  if [ -n "$bundled" ] && [ -f "$bundled/$marker" ]; then
    printf '%s' "$bundled"
    return 0
  fi
  local candidate
  for candidate in "$@"; do
    if [ -f "$candidate/$marker" ] && [ "$candidate" != "$2" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

# The stamp `scripts/package.sh` writes into a bundled copy. Empty for a developer
# checkout, which is the signal that this source is somebody's working tree and must
# not be copied over anything.
stamp_of() { [ -f "$1/.bundled-backend" ] && cat "$1/.bundled-backend" || true; }

provision_checkout() {
  local name="$1" dir="$2" marker="$3" bundled="$4" repo_url="$5"
  shift 5
  say "Checking $name ($dir)"
  if [ -f "$dir/$marker" ]; then
    # **Already installed is not the same as up to date.** Only overwrite when the
    # source carries a stamp, so a developer checkout adopted with "use the one I
    # have" is never overwritten — it may hold real work.
    local updated=no source bundled_stamp installed_stamp entry entry_name
    if source="$(find_checkout_source "$name" "$dir" "$marker" "$bundled" "$@")"; then
      bundled_stamp="$(stamp_of "$source")"
      installed_stamp="$(stamp_of "$dir")"
      if [ -n "$bundled_stamp" ] && [ "$bundled_stamp" != "$installed_stamp" ]; then
        say "Updating $name from the copy bundled with this app"
        echo "    installed ${installed_stamp:-unstamped} -> bundled ${bundled_stamp:0:12}"
        for entry in "$source"/* "$source"/.[!.]*; do
          [ -e "$entry" ] || continue
          entry_name="$(basename "$entry")"
          case "$entry_name" in
            .venv|.env|.git|.desktop-overlay|.langgraph_api) continue ;;
          esac
          rm -rf "${dir:?}/$entry_name"
        done
        cp -r "$source/." "$dir/"
        if [ ! -f "$dir/$marker" ]; then
          bad "the update did not bring $marker — $source may be incomplete"
          exit 1
        fi
        rm -rf "$dir/.venv/lib"/*/site-packages/__pycache__ 2>/dev/null || true
        ok "updated from $source"
        updated=yes
      fi
    fi
    [ "$updated" = no ] && ok "$name is already here and current: $dir"
    return 0
  fi

  mkdir -p "$(dirname "$dir")"
  # A failed clone/copy can leave an empty directory behind; it would block the copy.
  if [ -d "$dir" ] && [ -z "$(ls -A "$dir" 2>/dev/null)" ]; then rmdir "$dir"; fi

  local source
  if source="$(find_checkout_source "$name" "$dir" "$marker" "$bundled" "$@")"; then
    say "Copying $name from $source"
    echo "    (faster than downloading, and it needs no password)"
    # Trailing `/.` copies the *contents*, which is the only sane meaning of `cp -r`
    # here — without it, a `$dir` that already exists gains a nested `$dir/$(basename
    # $source)` instead, and the step after this reports a missing pyproject.toml with
    # no indication the copy itself claimed success.
    mkdir -p "$dir"
    cp -r "$source/." "$dir/"
    if [ ! -f "$dir/$marker" ]; then
      bad "the copy did not bring $marker — $source may be incomplete"
      exit 1
    fi
    # A copied .venv holds the *other* machine's compiled packages. Unusable here.
    if [ -d "$dir/.venv" ]; then
      rm -rf "$dir/.venv"
      ok "removed the copied environment (this machine needs to build its own)"
    fi
    ok "copied to $dir"
  else
    say "Downloading $name"
    echo "    This is a private repository, so git will ask who you are."
    echo "    If you are reading this inside the app, $name was not bundled with"
    echo "    your copy: ask whoever gave it to you."
    git clone "$repo_url" "$dir"
  fi
}

provision_checkout "Mini-Me" "$DIR" "langgraph.json" "${MINIME_BUNDLED_SOURCE:-}" "$REPO_URL" \
  "$HOME/Mini-Me" "$HOME/mini-me" "$HOME/Documents/Mini-Me"

provision_checkout "asta-plugins" "$ASTA_DIR" "pyproject.toml" "${ASTA_BUNDLED_SOURCE:-}" \
  "$ASTA_REPO_URL" "$HOME/asta-plugins" "$HOME/Documents/asta-plugins"

cd "$DIR"

# ------------------------------------------------------------------ the overlay
#
# Copied *into the checkout* rather than left wherever this script lives. Host
# execution works by putting this directory on the backend's PYTHONPATH, and a path
# next to the app's own install is reachable only while the app's own folder still
# exists. Three small files; copying them removes a whole class of silent failure.
OVERLAY_SRC="$(cd "$HERE/../overlay" 2>/dev/null && pwd || true)"
if [ -n "$OVERLAY_SRC" ] && [ -f "$OVERLAY_SRC/sitecustomize.py" ]; then
  say "Installing the local-execution overlay"
  rm -rf "$DIR/.desktop-overlay"
  cp -r "$OVERLAY_SRC" "$DIR/.desktop-overlay"
  # Bytecode from the source copy would be stale here and is never wanted.
  find "$DIR/.desktop-overlay" -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true
  ok "overlay installed here"
else
  bad "could not find the overlay next to this script — the app will use its own copy"
fi

# **A stopping point, so the step above can be rehearsed.**
if [ -n "${MINIME_SETUP_STOP_AFTER_SOURCE:-}" ]; then
  say "Stopping after the source step (MINIME_SETUP_STOP_AFTER_SOURCE)"
  exit 0
fi

# ------------------------------------------------------------------ dependencies
# --extra dev is REQUIRED: langgraph-cli lives in an optional extra, so a plain
# `uv sync` leaves you with the server libraries but no `langgraph` entry point.
# This also pulls in `asta` itself, per `[tool.uv.sources]` in pyproject.toml,
# from the sibling checkout provisioned above — no separate CLI install step.
say "Installing Python packages (a few minutes the first time)"
echo "    This pulls the scientific stack — PyMC, scikit-learn and friends — and Asta."
uv sync --extra dev

# Durable conversation storage, installed by default and not left to a checkbox.
say "Installing durable conversation storage"
uv pip install langgraph-checkpoint-sqlite || \
  bad "could not install langgraph-checkpoint-sqlite - Setup will offer it again"

if [ -x .venv/bin/langgraph ]; then
  ok "the backend can be started"
else
  bad ".venv/bin/langgraph is missing even after --extra dev."
  bad "The desktop app cannot start the backend without it."
  exit 1
fi

# -------------------------------------------------------------------------- env
# Deliberately NOT a key template. Keys live in the desktop app's settings panel and
# travel with each request from the OS keychain, so nobody has to edit a file to get
# started. An empty .env is still written because `langgraph dev` auto-loads one when
# present, and its absence has made people think they missed a step.
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
