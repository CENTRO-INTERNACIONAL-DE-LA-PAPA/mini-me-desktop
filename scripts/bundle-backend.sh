#!/usr/bin/env bash
# Put a copy of the Mini-Me backend inside this repo, so the app can install itself
# on a machine with no GitHub credentials.
#
# WHY THIS EXISTS
#
# Mini-Me is a *private* repository. `git clone` therefore demands credentials, and
# GitHub stopped accepting account passwords for git in 2021 — what the password
# prompt actually wants is a personal access token. Asking a potato scientist to
# create one before they can open an app is not a setup step, it is a wall.
#
# So the backend travels *with* the app. Whoever prepares a build runs this once;
# every install after that provisions with a local copy and never contacts GitHub.
#
# This does NOT fork Mini-Me. It is a pinned, unmodified checkout — the locked
# decision is "bundled, never forked" (docs §5), and `vendor/` is gitignored so the
# copy never enters this repository's history.
#
#   Usage:  bash bundle-backend.sh [git-ref]
#           default ref: main
#
#   MINIME_REPO_URL   override the remote
#   MINIME_VENDOR     override where the copy lands

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_URL="${MINIME_REPO_URL:-https://github.com/CENTRO-INTERNACIONAL-DE-LA-PAPA/Mini-Me.git}"
VENDOR="${MINIME_VENDOR:-$HERE/../vendor/Mini-Me}"
REF="${1:-main}"

say() { printf '\n==> %s\n' "$1"; }
ok()  { printf '    ok  %s\n' "$1"; }

mkdir -p "$(dirname "$VENDOR")"

# A checkout already on this machine, if there is one.
#
# Cloning *from that path* is what avoids GitHub entirely. An earlier version used
# `git clone --reference <local> <url>`, which does not: `--reference` only reuses
# objects, so git still contacts the remote for refs and asks for credentials — which is
# exactly the prompt this script exists to avoid, and it failed that way on first use.
find_local_checkout() {
  local candidate
  for candidate in \
      "$HOME/Documents/Mini-Me" "$HOME/Documents/GitHub/Mini-Me" "$HOME/Mini-Me" \
      /mnt/c/Users/*/Documents/GitHub/Mini-Me /mnt/c/Users/*/Documents/Mini-Me; do
    if [ -d "$candidate/.git" ]; then
      printf '%s' "$candidate"
      return 0
    fi
  done
  return 1
}

if [ -d "$VENDOR/.git" ]; then
  say "Updating the bundled copy in $VENDOR"
  # Best-effort: on a bundle cloned from a local path there may be no reachable remote,
  # and that is fine — the pin below works from what is already here.
  git -C "$VENDOR" fetch --tags origin 2>/dev/null || ok "no remote to fetch from (using what is here)"
else
  # A failed attempt can leave an empty directory behind, which blocks the clone.
  [ -d "$VENDOR" ] && [ -z "$(ls -A "$VENDOR" 2>/dev/null)" ] && rmdir "$VENDOR"

  if SOURCE="$(find_local_checkout)"; then
    say "Copying Mini-Me from $SOURCE"
    echo "    Local, so this needs no GitHub account and no token."
    git clone "$SOURCE" "$VENDOR"
    # Point at the real remote so a later `git fetch` can reach it, without having
    # needed it now.
    git -C "$VENDOR" remote set-url origin "$REPO_URL" 2>/dev/null || true
  else
    say "Downloading Mini-Me from GitHub"
    echo "    No local checkout found, so this needs repo access. GitHub does NOT"
    echo "    accept your account password — the prompt wants a personal access token"
    echo "    (https://github.com/settings/tokens), or run 'gh auth login' first."
    git clone "$REPO_URL" "$VENDOR"
  fi
fi

say "Pinning to $REF"
# `$REF` may only exist as a remote-tracking branch after a clone from a local path.
if git -C "$VENDOR" rev-parse --verify --quiet "$REF" >/dev/null; then
  git -C "$VENDOR" checkout --detach "$REF"
elif git -C "$VENDOR" rev-parse --verify --quiet "origin/$REF" >/dev/null; then
  git -C "$VENDOR" checkout --detach "origin/$REF"
else
  ok "no ref '$REF' here — bundling whatever is checked out"
fi
PINNED="$(git -C "$VENDOR" rev-parse --short HEAD)"
ok "pinned at $PINNED"

# A venv from whoever built this is dead weight and would be wrong on the target
# machine anyway — the provisioning script builds a native one.
if [ -d "$VENDOR/.venv" ]; then
  rm -rf "$VENDOR/.venv"
  ok "removed the build machine's virtual environment"
fi
# Never ship someone's keys.
if [ -f "$VENDOR/.env" ]; then
  rm -f "$VENDOR/.env"
  ok "removed .env (keys belong in the app's keychain, not in a bundle)"
fi

# Record what was bundled, so a support question has an answer.
cat > "$VENDOR/../BUNDLED.txt" <<EOF
Mini-Me backend bundled for the desktop app.
  ref:    $REF
  commit: $PINNED
EOF

say "Done."
echo "    The app will now provision from $VENDOR"
echo "    with no GitHub access needed on the user's machine."
