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
#     vendor/Mini-Me/             the backend, so no GitHub account is needed
#
# `resource()` in backend.rs looks beside the executable first, which is what makes this
# layout work without any configuration.
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
if [ -f "$ROOT/vendor/Mini-Me/langgraph.json" ]; then
  mkdir -p "$OUT/vendor"
  # --exclude would be nicer, but cp is what every platform here has. Copy then prune.
  cp -r "$ROOT/vendor/Mini-Me" "$OUT/vendor/Mini-Me"
  for junk in .venv .env node_modules .git; do
    rm -rf "$OUT/vendor/Mini-Me/$junk"
  done
  ok "vendor/Mini-Me ($(du -sh "$OUT/vendor/Mini-Me" | cut -f1)) — no GitHub account needed"
else
  bad "vendor/Mini-Me is missing, so this bundle CANNOT install itself."
  bad "Mini-Me is a private repository: without a bundled copy the user is asked"
  bad "for a GitHub token they do not have. Fix it with:"
  bad "    bash scripts/bundle-backend.sh"
  bad "Continuing anyway — this bundle is only usable by someone with repo access."
fi

# `.git` is dropped above, so record what went in.
if [ -f "$ROOT/vendor/BUNDLED.txt" ]; then
  cp "$ROOT/vendor/BUNDLED.txt" "$OUT/vendor/BUNDLED.txt"
fi

cat > "$OUT/README.txt" <<'TXT'
Mini-Me Desktop
===============

1. Double-click mini-me-desktop-app.exe
2. The Setup pane opens and tells you what is missing. Press the buttons.
3. When it asks for a model API key, open Settings and paste one.

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
