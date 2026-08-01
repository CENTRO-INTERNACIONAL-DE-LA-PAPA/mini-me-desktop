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

if [ -d "$VENDOR/.git" ]; then
  say "Updating the bundled copy in $VENDOR"
  git -C "$VENDOR" fetch --tags origin
else
  say "Cloning Mini-Me into $VENDOR"
  echo "    You need GitHub access for this step — but only you, once."
  # A local checkout is a much cheaper source than the network, when there is one.
  for candidate in "$HOME/Documents/Mini-Me" "$HOME/Documents/GitHub/Mini-Me" "$HOME/Mini-Me"; do
    if [ -d "$candidate/.git" ]; then
      say "Found a local checkout at $candidate — cloning from it"
      git clone --reference "$candidate" --dissociate "$REPO_URL" "$VENDOR"
      break
    fi
  done
  [ -d "$VENDOR/.git" ] || git clone "$REPO_URL" "$VENDOR"
fi

say "Pinning to $REF"
git -C "$VENDOR" checkout --detach "$REF"
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
