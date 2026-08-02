#!/usr/bin/env bash
# Turn the packaged folder into a download link.
#
# The last gap between this app and being used: today "install it" means `git clone` and
# `cargo build`, which rules out every researcher this was built for. This produces a zip
# and attaches it to a GitHub release, so the answer becomes a URL.
#
#   Usage:  bash scripts/release.sh              # draft release for the version in Cargo.toml
#           bash scripts/release.sh --dry-run    # check + zip, touch nothing on GitHub
#           bash scripts/release.sh v0.1.1       # explicit tag
#           bash scripts/release.sh --publish    # publish immediately instead of drafting
#
# **Run this on Windows**, in Git Bash, after:
#     cargo build --release -p mini-me-desktop-app
#     bash scripts/bundle-backend.sh
#     bash scripts/package.sh
#
# It is a *draft* by default. Nobody has ever installed this from a zip, and a draft can be
# deleted without anyone having downloaded a broken one; publishing is one command, printed
# at the end.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"

# A path as *Windows* spells it.
#
# This script runs from three different shells and each has its own idea of a path: WSL
# says `/mnt/c/…`, Git Bash says `/c/…`, PowerShell wants `C:\…`. Anything handed to
# `powershell.exe` has to be translated first — passing WSL's spelling straight through is
# what broke the first real release attempt (docs §46).
#
# Translated via the parent directory, because `wslpath` resolves a path that exists and
# the zip does not exist yet.
win_path() {
  local path="$1"
  if command -v wslpath >/dev/null 2>&1; then
    wslpath -w "$path" 2>/dev/null && return 0
  fi
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$path" 2>/dev/null && return 0
  fi
  # Last resort, for a shell that offers neither: /mnt/c/x → C:\x, /c/x → C:\x.
  printf '%s' "$path" | sed -E 's|^/mnt/([a-zA-Z])/|\1:/|; s|^/([a-zA-Z])/|\1:/|' \
    | sed -E 's|^(.)|\U\1|; s|/|\\|g'
}

say()  { printf '\n==> %s\n' "$1"; }
ok()   { printf '    ok  %s\n' "$1"; }
bad()  { printf '    !!  %s\n' "$1"; }
die()  { bad "$1"; exit 1; }

PUBLISH=no
DRY=no
TAG=""
for arg in "$@"; do
  case "$arg" in
    --publish) PUBLISH=yes ;;
    --dry-run) DRY=yes ;;
    v*)        TAG="$arg" ;;
    *)         die "unknown argument: $arg" ;;
  esac
done

if [ -z "$TAG" ]; then
  VERSION="$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
  [ -n "$VERSION" ] || die "could not read the version from Cargo.toml"
  TAG="v$VERSION"
fi

BUNDLE="$ROOT/dist/mini-me-desktop"
ZIP="$ROOT/dist/mini-me-desktop-$TAG-windows-x64.zip"

# ------------------------------------------------------- refuse to ship a broken bundle
#
# Every check here is a way a colleague's first ten minutes get wasted. Finding out from
# them is the expensive way to find out.
say "Checking the bundle"
[ -d "$BUNDLE" ] || die "no bundle. Run: bash scripts/package.sh"

if [ -f "$BUNDLE/mini-me-desktop-app.exe" ]; then
  ok "mini-me-desktop-app.exe"
elif [ -f "$BUNDLE/mini-me-desktop-app" ]; then
  die "this bundle holds a Linux binary. Build and package on Windows — a colleague
       on Windows cannot run it, and that is the whole audience."
else
  die "no executable in the bundle"
fi

# Without this the installer asks for a GitHub token for a *private* repo, which is
# exactly the wall this bundle exists to remove (docs §25).
[ -f "$BUNDLE/vendor/Mini-Me/langgraph.json" ] \
  || die "vendor/Mini-Me is missing — the bundle cannot install itself.
       Fix: bash scripts/bundle-backend.sh && bash scripts/package.sh"
ok "vendor/Mini-Me (the backend, so no GitHub account is needed)"

[ -f "$BUNDLE/overlay/minime_local/workspace.py" ] \
  || die "overlay/ is missing — host execution would not work"
ok "overlay/ (host execution)"

[ -f "$BUNDLE/scripts/setup-wsl.sh" ] || die "scripts/setup-wsl.sh is missing"
ok "scripts/ (first-run provisioning)"

if [ "$DRY" = no ]; then
  command -v gh >/dev/null 2>&1 || die "the GitHub CLI is needed: https://cli.github.com"
  gh auth status >/dev/null 2>&1 || die "not signed in. Run: gh auth login"
fi

if [ -n "$(git -C "$ROOT" status --porcelain)" ]; then
  bad "the working tree is dirty — the tag will not describe what is in the zip."
  bad "Commit first, or accept that this release is not reproducible."
fi

# ------------------------------------------------------------------------------ the zip
say "Building $(basename "$ZIP")"
rm -f "$ZIP"
if command -v zip >/dev/null 2>&1; then
  ( cd "$ROOT/dist" && zip -qr "$(basename "$ZIP")" "$(basename "$BUNDLE")" )
else
  # Neither Git Bash nor a default WSL ships `zip`. Windows has had Compress-Archive
  # since PowerShell 5, and it is already there on every machine this targets.
  DIST_WIN="$(win_path "$ROOT/dist")"
  DIST_WIN="${DIST_WIN%\\}"
  # Naming the folder (not `dir\*`) keeps `mini-me-desktop/` as the root inside the zip,
  # so unzipping produces one folder rather than scattering 30 files into Downloads.
  powershell.exe -NoProfile -NonInteractive -Command \
    "Compress-Archive -Path '$DIST_WIN\\$(basename "$BUNDLE")' \
     -DestinationPath '$DIST_WIN\\$(basename "$ZIP")' -Force" \
    || die "could not create the zip. Path handed to PowerShell: $DIST_WIN"
fi
[ -f "$ZIP" ] || die "the zip was not created"
ok "$(du -h "$ZIP" | cut -f1)"

# A checksum is what lets someone verify the download is the one we built — the only
# integrity signal an *unsigned* executable has.
if command -v sha256sum >/dev/null 2>&1; then
  SHA="$(sha256sum "$ZIP" | cut -d' ' -f1)"
else
  SHA="$(certutil.exe -hashfile "$(win_path "$ZIP")" SHA256 | sed -n 2p | tr -d ' \r')"
fi
ok "sha256 ${SHA:0:16}…"

PINNED="unknown"
[ -f "$BUNDLE/vendor/BUNDLED.txt" ] \
  && PINNED="$(grep -m1 'commit:' "$BUNDLE/vendor/BUNDLED.txt" | awk '{print $2}')"

# -------------------------------------------------------------------------- the release
#
# Written for the person downloading it, not for us. They do not know what a sidecar is,
# and the first thing they will meet is a SmartScreen warning.
NOTES="$(mktemp)"
cat > "$NOTES" <<TXT
Mini-Me Desktop for Windows — a research workbench that runs on your own machine.

## Install

1. Download **mini-me-desktop-$TAG-windows-x64.zip** below and unzip it anywhere.
2. Double-click **mini-me-desktop-app.exe**.
3. Windows will say *"Windows protected your PC"*. This build is not code-signed yet.
   Click **More info**, then **Run anyway**.
4. The Setup pane opens and tells you what is missing. Press the buttons it shows.
5. When it asks for a model API key, open Settings (ctrl-,) and paste one.

No terminal, no GitHub account, no Python install. The backend is included.

The first run installs a Linux environment (WSL) if you do not have one. That step
needs administrator rights and may ask for a restart — the app says so before you press it.

## Notes

- Your API keys go in Windows Credential Manager, never in a file.
- Files the agent creates land in **Documents\\Mini-Me**, one folder per conversation.
- Commands the agent wants to run on your machine wait for your approval first.
- Backend pinned at \`$PINNED\`.
- sha256: \`$SHA\`

## Known limits in $TAG

- **Not code-signed** — hence the SmartScreen warning above.
- WSL provisioning has not yet been tested on a machine that never had WSL.
- Text in the transcript cannot be selected with the mouse; use *Copy last answer*
  from the command palette (ctrl-p).
- The composer is single-line: Enter sends.
TXT

if [ "$DRY" = yes ]; then
  say "Dry run — this is the release note a colleague would read:"
  echo
  sed 's/^/    | /' "$NOTES"
  rm -f "$NOTES"
  echo
  echo "    tag  : $TAG"
  echo "    zip  : $ZIP"
  echo "    Nothing was sent to GitHub. Drop --dry-run to create the release."
  exit 0
fi

say "Creating the release $TAG"
if gh release view "$TAG" >/dev/null 2>&1; then
  bad "$TAG already exists — uploading the zip to it and replacing any previous one."
  gh release upload "$TAG" "$ZIP" --clobber
else
  DRAFT=(--draft)
  [ "$PUBLISH" = yes ] && DRAFT=()
  gh release create "$TAG" "$ZIP" \
    --title "Mini-Me Desktop $TAG" \
    --notes-file "$NOTES" \
    "${DRAFT[@]}"
fi
rm -f "$NOTES"

say "Done."
gh release view "$TAG" --json url --jq .url 2>/dev/null || true
if [ "$PUBLISH" != yes ]; then
  echo
  echo "    This is a DRAFT — the link works only for you. When you have installed it"
  echo "    from the zip yourself and it worked, publish with:"
  echo
  echo "        gh release edit $TAG --draft=false"
fi
