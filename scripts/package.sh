#!/usr/bin/env bash
# Assemble a folder you can hand to a colleague.
#
# The result is deliberately a **folder, not an installer**. There is no MSI, no code
# signing and no notarization — the target is a few dozen colleagues at CIP, and an
# unsigned installer is a SmartScreen warning that teaches people to click through
# security dialogs. A folder they unzip and a shortcut they make is honest about what
# this is.
#
#   dist/mini-me-desktop/
#     mini-me-desktop-app(.exe)   the app
#     overlay/                    host execution (docs §18)
#     scripts/                    setup-wsl.sh, run from the Setup pane
#     mini-me/                    the backend, so no GitHub account is needed
#
# `resource()` in backend.rs looks beside the executable first, which is what makes this
# layout work without any configuration.
#
# **The backend is `mini-me/`, and this script used to ship `vendor/Mini-Me` instead.**
# `bundled_backend_dir()` has preferred `mini-me/` since the monorepo move — *"from now I
# want a mono repo in mini me desktop"* — and this script never copied it, so every bundle
# carried a clone of the separate private repo instead. Four middleware modules and a route
# were simply absent from every release, and the dataverse reader shipped with the argument
# name that had been fixed a week earlier (docs §283).
#
#   Usage:  bash scripts/package.sh            (release build)
#           bash scripts/package.sh --debug
#
# Run it from Git Bash or WSL on the machine that built the .exe. On WSL, build on the
# Windows side first — a Linux binary in a Windows bundle helps nobody.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
PROFILE=release
[ "${1:-}" = "--debug" ] && PROFILE=debug

say() { printf '\n==> %s\n' "$1"; }
ok()  { printf '    ok  %s\n' "$1"; }
bad() { printf '    !!  %s\n' "$1"; }

OUT="$ROOT/dist/mini-me-desktop"

# Either name — the same bundle script is useful on both platforms.
BIN=""
for candidate in \
    "$ROOT/target/$PROFILE/mini-me-desktop-app.exe" \
    "$ROOT/target/$PROFILE/mini-me-desktop-app"; do
  [ -f "$candidate" ] && BIN="$candidate" && break
done
if [ -z "$BIN" ]; then
  bad "no $PROFILE binary. Build it first:"
  bad "    cargo build --$PROFILE -p mini-me-desktop-app"
  exit 1
fi

say "Packaging the $PROFILE build"
rm -rf "$OUT"
mkdir -p "$OUT"
cp "$BIN" "$OUT/"
ok "$(basename "$BIN") ($(du -h "$BIN" | cut -f1))"

for dir in overlay scripts; do
  cp -r "$ROOT/$dir" "$OUT/$dir"
  ok "$dir/"
done
# Bytecode belongs to whichever interpreter made it, never to a bundle.
find "$OUT" -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true

# ------------------------------------------------------------------- the backend
#
# `mini-me/` first, because that is the source this repository tracks and the directory
# `bundled_backend_dir()` looks for before anything else. `vendor/Mini-Me` remains as the
# fallback for a checkout that predates the monorepo move and still bundles from the
# private repo — but it is no longer what a build here ships.
BACKEND_SRC=""
BACKEND_DEST=""
if [ -f "$ROOT/mini-me/langgraph.json" ]; then
  BACKEND_SRC="$ROOT/mini-me"
  BACKEND_DEST="$OUT/mini-me"
elif [ -f "$ROOT/vendor/Mini-Me/langgraph.json" ]; then
  BACKEND_SRC="$ROOT/vendor/Mini-Me"
  BACKEND_DEST="$OUT/vendor/Mini-Me"
  bad "shipping vendor/Mini-Me — mini-me/ is missing, so this is the pre-monorepo layout"
fi

if [ -n "$BACKEND_SRC" ]; then
  mkdir -p "$(dirname "$BACKEND_DEST")"
  # --exclude would be nicer, but cp is what every platform here has. Copy then prune.
  cp -r "$BACKEND_SRC" "$BACKEND_DEST"
  for junk in .venv .env node_modules .git .langgraph_api; do
    rm -rf "$BACKEND_DEST/$junk"
  done
  find "$BACKEND_DEST" -name __pycache__ -type d -exec rm -rf {} + 2>/dev/null || true

  # **A stamp, so an installed copy can tell it is out of date.**
  #
  # `setup-wsl.sh` copies the backend once and reports "already here" ever after, so before
  # this a backend fix could never reach a machine that had already been provisioned — the
  # app updated and the Python underneath it did not (docs §283). The stamp is content-
  # derived rather than a version number: a hand-built bundle has no version to quote, and
  # what matters is whether these files differ from the installed ones, not what they are
  # called.
  STAMP="$(cd "$BACKEND_DEST" && find . -type f \( -name '*.py' -o -name '*.json' -o -name '*.toml' -o -name '*.md' \) \
    | LC_ALL=C sort | xargs sha256sum 2>/dev/null | sha256sum | cut -d' ' -f1)"
  printf '%s\n' "$STAMP" > "$BACKEND_DEST/.bundled-backend"
  ok "$(basename "$BACKEND_DEST")/ ($(du -sh "$BACKEND_DEST" | cut -f1)) — no GitHub account needed"
  ok "stamped ${STAMP:0:12} — an older install will notice and re-copy"
else
  bad "no backend to bundle, so this bundle CANNOT install itself."
  bad "Expected mini-me/langgraph.json in this repository. If this is a"
  bad "pre-monorepo checkout, fix it with:"
  bad "    bash scripts/bundle-backend.sh"
  bad "Continuing anyway — this bundle is only usable by someone with repo access."
fi

# `.git` is dropped above, so record what went in.
# ------------------------------------------------- a folder the old app insists on
#
# **Kept for one reason, and it is not a good one.** `update.rs` shipped with `vendor` as a
# *required* marker of a bundle, so an install from before v0.3.15 refuses any download
# without one: `unpack` answers "the download does not contain a bundle", nothing is staged,
# and the Restart to Update button never appears. There is no way to fix that remotely —
# the check lives in the copy already on the researcher's machine.
#
# So every bundle carries a `vendor/`, whatever else it carries, until nobody is running a
# build older than v0.3.15. Newer apps accept `mini-me` or `vendor` (BUNDLE_BACKENDS), which
# is what this should have been from the start.
mkdir -p "$OUT/vendor"
cat > "$OUT/vendor/README.txt" <<'VENDOR'
The backend moved to ../mini-me in v0.3.14.

This folder is kept only so that installs older than v0.3.15 accept this download:
their updater requires a folder named `vendor` beside the executable and refuses
the bundle without one. Nothing reads what is in here.
VENDOR
ok "vendor/ (empty — so an older install still accepts this bundle)"

if [ -f "$ROOT/vendor/BUNDLED.txt" ]; then
  cp "$ROOT/vendor/BUNDLED.txt" "$OUT/vendor/BUNDLED.txt"
fi

cat > "$OUT/README.txt" <<'TXT'
Mini-Me Desktop
===============

1. Double-click mini-me-desktop-app.exe

2. Windows will say "Windows protected your PC" and offer only a "Don't run"
   button. This is expected: the app is not code-signed yet, and Windows says
   this about every program it has not seen before.

   Click "More info", then "Run anyway".

3. The Setup pane opens and tells you what is missing. Press the buttons.
4. When it asks for a model API key, open Settings and paste one.

That is all. Nothing here needs a terminal.

The first run installs a Linux environment (WSL) if you do not already have one.
That step asks for administrator rights and may need a restart — the app says so
before you press it.

Keys you paste are stored in Windows Credential Manager, not in any file.

If something looks wrong, the Setup pane has a "Re-check" button and shows the
location of the log file at the bottom.
TXT
ok "README.txt"

say "Done."
echo "    $OUT"
echo "    Total: $(du -sh "$OUT" | cut -f1)"
echo
echo "    Zip that folder and send it. To make a Start Menu entry, right-click the"
echo "    .exe and choose 'Create shortcut'."
