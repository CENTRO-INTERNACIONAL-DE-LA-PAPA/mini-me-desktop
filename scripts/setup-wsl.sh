#!/usr/bin/env bash
# Provision the Mini-Me backend inside WSL2 (or any Linux) for the desktop app.
#
# Why WSL: the agent stack shells out with POSIX commands and needs bash/python3/
# the asta CLI, none of which behave under cmd.exe. Inside WSL the backend simply
# is on Linux, so nothing upstream changes — and the desktop app reaches it over
# localhost, which WSL2 forwards.
#
# Safe to re-run: it never overwrites an existing checkout or .env.
#
#   Usage:  bash setup-wsl.sh [checkout-dir]     (default: ~/Mini-Me)

set -euo pipefail

REPO_URL="${MINIME_REPO_URL:-https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me.git}"
DIR="${1:-$HOME/Mini-Me}"

say() { printf '\n\033[1;33m==>\033[0m %s\n' "$1"; }
ok()  { printf '    \033[0;32mok\033[0m %s\n' "$1"; }

# ---------------------------------------------------------------- sanity checks
if grep -qiE "(microsoft|wsl)" /proc/version 2>/dev/null; then
  ok "running inside WSL"
else
  say "Note: this doesn't look like WSL. That's fine on a native Linux box."
fi

# ------------------------------------------------------------------------ tools
say "Checking prerequisites"
if ! command -v git >/dev/null 2>&1; then
  echo "    git is missing. Install it first:  sudo apt-get update && sudo apt-get install -y git"
  exit 1
fi
ok "git $(git --version | awk '{print $3}')"

if ! command -v uv >/dev/null 2>&1; then
  say "Installing uv (Python package manager)"
  curl -LsSf https://astral.sh/uv/install.sh | sh
  # uv lands in ~/.local/bin; make it visible to this script and future shells.
  export PATH="$HOME/.local/bin:$PATH"
  if ! grep -qs '\.local/bin' "$HOME/.bashrc"; then
    echo 'export PATH="$HOME/.local/bin:$PATH"' >> "$HOME/.bashrc"
    ok "added ~/.local/bin to PATH in ~/.bashrc"
  fi
fi
ok "uv $(uv --version | awk '{print $2}')"

# ------------------------------------------------------------------- checkout
# Find an existing checkout on the Windows side. Most users already cloned the
# backend there, and copying it avoids GitHub authentication entirely — which
# matters because GitHub removed password auth for git in 2021, so the password
# prompt actually wants a personal access token and rejects the account password.
find_windows_checkout() {
  local candidate
  for candidate in /mnt/c/Users/*/Documents/GitHub/Mini-Me /mnt/c/Users/*/Documents/Mini-Me; do
    if [ -f "$candidate/langgraph.json" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

if [ -d "$DIR/.git" ]; then
  ok "checkout already present at $DIR (leaving it alone)"
else
  # A failed clone can leave an empty directory behind; it would block the copy.
  [ -d "$DIR" ] && [ -z "$(ls -A "$DIR" 2>/dev/null)" ] && rmdir "$DIR"

  if WIN_CHECKOUT="$(find_windows_checkout)"; then
    say "Copying the existing Windows checkout from $WIN_CHECKOUT"
    echo "    (avoids needing GitHub credentials inside WSL)"
    cp -r "$WIN_CHECKOUT" "$DIR"
    # A Windows venv holds Scripts/*.exe and Windows-built wheels — unusable from
    # Linux. Drop it so uv rebuilds a native one below.
    if [ -d "$DIR/.venv" ]; then
      rm -rf "$DIR/.venv"
      ok "removed the copied Windows .venv (Linux needs its own)"
    fi
    ok "copied to $DIR"
  else
    say "Cloning the Mini-Me backend into $DIR"
    echo "    This is a private repository. Note that GitHub does NOT accept your"
    echo "    account password here — Git over HTTPS needs a personal access token."
    echo "    Easiest: run 'gh auth login' first. Otherwise create a token at"
    echo "    https://github.com/settings/tokens and paste it at the password prompt."
    git clone "$REPO_URL" "$DIR"
  fi
fi

cd "$DIR"

# ------------------------------------------------------------------ dependencies
# --extra dev is REQUIRED: langgraph-cli lives in an optional extra, so a plain
# `uv sync` leaves you with the server libraries but no `langgraph` entry point.
say "Installing Python dependencies (uv sync --extra dev)"
echo "    First run pulls PyMC/scikit-learn and friends — expect a few minutes."
uv sync --extra dev
if [ -x .venv/bin/langgraph ]; then
  ok "langgraph CLI installed"
else
  echo "    WARNING: .venv/bin/langgraph is missing even after --extra dev."
  echo "    The desktop app cannot spawn the backend without it."
  exit 1
fi

# -------------------------------------------------------------------------- env
if [ -f .env ]; then
  ok ".env already exists (leaving it alone)"
else
  say "Creating a .env template"
  cat > .env <<'ENV'
# Your own API keys — these stay on your machine.
OPENAI_API_KEY=
# Asta (research tools). Mint the token once with: asta auth print-token --raw --refresh
ASTA_API_KEY=
ASTA_TOKEN=
# Required only while execution still uses the remote sandbox. Once local
# execution lands you can delete this line.
LANGSMITH_API_KEY=
ENV
  ok "wrote $DIR/.env — open it and paste your keys"
fi

# ------------------------------------------------------------------------- done
say "Done."
cat <<EOF
    Backend ready at: $DIR

    1) Put your API keys in    $DIR/.env
    2) On the WINDOWS side, launch the app with WSL mode on:

         \$env:MINIME_BACKEND_WSL=1
         \$env:MINIME_BACKEND_WSL_DIR="${DIR/#$HOME/~}"
         cargo run -p mini-me-desktop-app

       The app starts the backend inside WSL itself — you don't need to run it
       here. To check by hand:  .venv/bin/langgraph dev --host 0.0.0.0 --port 2024

    Tip: to run 'git pull' in here later without re-entering credentials (this is
    what a future "click to update" will need), reuse Windows' credential manager:

      git config --global credential.helper \\
        "/mnt/c/Program Files/Git/mingw64/libexec/git-core/git-credential-manager.exe"
EOF
