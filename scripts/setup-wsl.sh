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
#   MINIME_BUNDLED_SOURCE   the Mini-Me backend shipped with the app — the only
#                           source this script installs from

set -euo pipefail

DIR="${1:-$HOME/.local/share/mini-me-desktop/backend}"
# Expand a leading ~ if the caller passed one through as a literal.
DIR="${DIR/#\~/$HOME}"


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

# Make ~/.local/bin reachable by the shell the *app* launches the backend with.
#
# That shell is `bash -lc` — a login shell that is NOT interactive — and it reads
# ~/.profile, never ~/.bashrc: Ubuntu's .bashrc returns in its first few lines when
# `$-` has no `i`. So a PATH line written only to .bashrc is invisible to the backend,
# which is where `asta` has to be found when a command runs. Ubuntu's default .profile
# happens to add this directory already; that is luck, and this makes it a guarantee.
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

# ------------------------------------------------- where the source comes from
#
# Only ever the copy bundled with the app. `scripts/package.sh` ships `mini-me/` inside
# the app, so there is nothing to download and nothing to ask a password for. A copy the
# script merely *found* elsewhere on the machine is not a source: it may be someone's
# working tree, and it is not the version this app was built against.
find_source() {
  if [ -n "${MINIME_BUNDLED_SOURCE:-}" ] && [ -f "${MINIME_BUNDLED_SOURCE}/langgraph.json" ]; then
    printf '%s' "$MINIME_BUNDLED_SOURCE"
    return 0
  fi
  return 1
}

# The stamp `scripts/package.sh` writes into the bundled copy. Empty for a developer
# checkout, which is the signal that this source is somebody's working tree and must not be
# copied over anything.
stamp_of() { [ -f "$1/.bundled-backend" ] && cat "$1/.bundled-backend" || true; }

# Copy the source's *contents* into $DIR, leaving out what belongs to the machine it came from.
#
# A released bundle has none of these — `scripts/package.sh` prunes them. Running from source,
# `MINIME_BUNDLED_SOURCE` is the developer's own `mini-me/`, and its Windows `.venv` alone is
# over a gigabyte in ~40k files. Copied across `/mnt/c`, that held onboarding on "Copying
# Mini-Me…" for ten minutes, only for the fresh-install path to delete it straight after. tar
# rather than `cp`, because it can exclude, and the trailing `.` copies contents either way.
copy_source() {
  local dest="${2:-$DIR}"
  mkdir -p "$dest"
  tar -C "$1" \
      --exclude=./.venv --exclude=./.env --exclude=./.git --exclude=./.langgraph_api \
      --exclude=node_modules --exclude=__pycache__ --exclude=.pytest_cache \
      -cf - . | tar -C "$dest" -xf -
}

if [ -f "$DIR/langgraph.json" ]; then
  # **Already installed is not the same as up to date**, and for a long time this script
  # treated them as the same thing: it said "already here" and copied nothing, so the app
  # could update every week while the Python underneath it stayed at whatever shipped the
  # first time the machine was provisioned. The researcher's dataverse explorer ran a
  # version of `read_search_results` that had been fixed nine days earlier, and the claims
  # recorder built for it was not on the machine at all (docs §283).
  #
  # Only when the source carries a stamp: a developer running from source has an unstamped
  # `mini-me/`, and that is not a newer build to copy over anything.
  UPDATED=no
  if SOURCE="$(find_source)"; then
    BUNDLED="$(stamp_of "$SOURCE")"
    INSTALLED="$(stamp_of "$DIR")"
    if [ -n "$BUNDLED" ] && [ "$BUNDLED" != "$INSTALLED" ]; then
      say "Updating Mini-Me from the copy bundled with this app"
      echo "    installed ${INSTALLED:-unstamped} -> bundled ${BUNDLED:0:12}"
      # **Replaced per entry, not merged.** A merge leaves a module that upstream deleted
      # sitting importable on the machine, which is the same class of ghost this whole fix
      # is about. Only what the bundle carries is removed: `.venv` costs fifteen minutes to
      # rebuild, and `.env` and the server's state directory are this machine's.
      for entry in "$SOURCE"/* "$SOURCE"/.[!.]*; do
        [ -e "$entry" ] || continue
        name="$(basename "$entry")"
        case "$name" in
          .venv|.env|.git|.langgraph_api) continue ;;
        esac
        rm -rf "${DIR:?}/$name"
      done
      copy_source "$SOURCE"
      if [ ! -f "$DIR/pyproject.toml" ]; then
        bad "the update did not bring pyproject.toml — $SOURCE may be incomplete"
        exit 1
      fi
      rm -rf "$DIR/.venv/lib"/*/site-packages/__pycache__ 2>/dev/null || true
      ok "updated from $SOURCE"
      UPDATED=yes
    fi
  fi
  [ "$UPDATED" = no ] && ok "Mini-Me is already here and current: $DIR"
else
  mkdir -p "$(dirname "$DIR")"
  # A failed clone can leave an empty directory behind; it would block the copy.
  if [ -d "$DIR" ] && [ -z "$(ls -A "$DIR" 2>/dev/null)" ]; then rmdir "$DIR"; fi

  if SOURCE="$(find_source)"; then
    say "Copying Mini-Me from the copy bundled with this app"
    # `cp -r SRC DEST` means two different things depending on whether DEST exists: it *becomes*
    # SRC when it does not, and gains a `DEST/<basename SRC>` when it does. So a `$DIR` left
    # behind non-empty by an interrupted run — or by anything else — turned the copy into
    # `$DIR/mini-me/pyproject.toml`, and `uv sync` two steps later reported
    # "No `pyproject.toml` found in current directory or any parent directory" while the copy
    # above it said `ok`. `copy_source` copies the *contents*, which means one thing only.
    #
    # **Staged, then moved into place.** The slow part — reading across `/mnt/c` — used to write
    # straight into `$DIR`, and the app's backend launch mirrors `backend/` into `$DIR` too, by
    # deleting and replacing it. A launch during the copy (at app start, or after a retry) swapped
    # `backend/` out from under it: tar failed with "Cannot mkdir: No such file or directory" and
    # "Cannot open: File exists", and a half-copied tree with `langgraph.json` passed for an
    # install on the next run. Until the move, `$DIR` does not exist, so a launch's mirror has
    # nothing to write into, and an interrupted copy leaves only the staging folder behind.
    STAGE="$DIR.partial"
    rm -rf "$STAGE"
    copy_source "$SOURCE" "$STAGE"
    # Said out loud, because the failure above was silent for exactly as long as it took to
    # reach a step that needed a file: the copy reported success either way.
    if [ ! -f "$STAGE/pyproject.toml" ]; then
      bad "the copy did not bring pyproject.toml — $SOURCE may be incomplete"
      exit 1
    fi
    if [ -e "$DIR" ]; then
      # Left by an interrupted run of an older version of this script, and it may hold this
      # machine's `.env` or conversations — so merged into, never deleted. Local disk to local
      # disk, so this takes a second, not the minutes the staging copy did.
      cp -a "$STAGE/." "$DIR/"
      rm -rf "$STAGE"
    else
      mv "$STAGE" "$DIR"
    fi
    # A copied .venv holds the *other* machine's compiled packages — Windows
    # Scripts/*.exe, or wheels built for a different Python. Unusable here.
    if [ -d "$DIR/.venv" ]; then
      rm -rf "$DIR/.venv"
      ok "removed the copied environment (this machine needs to build its own)"
    fi
    ok "copied to $DIR"
  else
    bad "this copy of the app has no backend bundled with it"
    echo "    Reinstall the app from the latest release, or ask whoever gave it to you."
    exit 1
  fi
fi

cd "$DIR"

# A checkout copied from /mnt/c carries Windows' CRLF working files, but not Git for Windows'
# *global* `core.autocrlf=true`. WSL Git therefore used to call every tracked file modified the
# moment provisioning finished, which made the checkout unusable for any later Git operation.
#
# Make the policy local to the checkout because Windows is the source we deliberately support,
# not an exceptional environment to tell the researcher to repair. `input` normalises CRLF when
# Git reads it and keeps future checkouts inside WSL at LF. Git also caches the old clean filter in
# its index, so a guarded `--renormalize` is required once after changing the policy. It runs only
# when every unstaged difference is a CR at end-of-line; a real edit leaves the tree untouched.
# Do not `reset --hard`: a developer's bundle may be a working tree with real work (§144).
if git -C "$DIR" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  git -C "$DIR" config core.autocrlf input
  if ! git -C "$DIR" diff --quiet -- && \
      git -C "$DIR" diff --ignore-cr-at-eol --quiet --; then
    git -C "$DIR" add --renormalize -- .
    ok "normalised the copied checkout's Windows line endings"
  else
    ok "configured the copied checkout for Windows line endings"
  fi
fi

# Host execution lives in mini-me/backend/local/ — part of the backend package copied
# above, not a separate directory to stage (docs §303).

# **A stopping point, so the step above can be rehearsed.**
#
# Everything below installs packages and takes minutes; everything above decides *which
# backend this machine runs*, which is the part that was wrong for a fortnight and could not
# be observed from the machine it was written on. `scripts/backend-refresh-rehearsal.sh`
# sets this and checks what landed, in seconds, against throwaway directories — the lesson
# of §275, applied one script earlier this time.
if [ -n "${MINIME_SETUP_STOP_AFTER_SOURCE:-}" ]; then
  say "Stopping after the source step (MINIME_SETUP_STOP_AFTER_SOURCE)"
  exit 0
fi

# ------------------------------------------------------------------ dependencies
# --extra dev is REQUIRED: langgraph-cli lives in an optional extra, so a plain
# `uv sync` leaves you with the server libraries but no `langgraph` entry point.
say "Installing Python packages (a few minutes the first time)"
echo "    This pulls the scientific stack — PyMC, scikit-learn and friends."
uv sync --extra dev

# Durable conversation storage, installed by default and not left to a checkbox.
#
# Without it the backend keeps `langgraph dev`'s pickle checkpointer, which loads every
# conversation in the installation before answering anything, and which — on a load that
# fails after a dependency change — flushes an empty dict over the real file ten seconds
# later (docs §90/§94). Neither cost is one a researcher can be expected to opt out of;
# they would have to know the failure exists to go looking for the switch.
#
# `|| true` because this is an improvement, not a requirement: a machine that cannot reach
# the index still gets a working backend, and Setup will offer the install again.
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
