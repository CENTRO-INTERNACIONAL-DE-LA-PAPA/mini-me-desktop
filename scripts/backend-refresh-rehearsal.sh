#!/usr/bin/env bash
# Prove that an already-provisioned backend is updated when the bundle changes — in seconds,
# against throwaway directories, using the real `setup-wsl.sh`.
#
# WHY THIS EXISTS
#
# `setup-wsl.sh` said "Mini-Me is already here" and copied nothing, so a backend fix could
# never reach a machine that had been provisioned once. The app updated weekly; the Python
# under it did not. A researcher's dataverse explorer ran a `read_search_results` that had
# been fixed nine days earlier, and the claims recorder written for it was never on the
# machine at all (docs §283).
#
# That is a shell path on somebody else's Windows box, which is exactly the shape of failure
# §275 spent four release round-trips on before building an instrument. This is the
# instrument, written first this time.
#
#   Usage:  bash scripts/backend-refresh-rehearsal.sh
#
# It runs the real script with MINIME_SETUP_STOP_AFTER_SOURCE=1, so nothing is installed and
# no package manager is touched.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SETUP="$HERE/setup-wsl.sh"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

pass=0
fail=0
check() {
  if [ "$2" = "$3" ]; then
    printf '    ok  %s\n' "$1"; pass=$((pass + 1))
  else
    printf '    !!  %s\n        expected %s\n        got      %s\n' "$1" "$3" "$2"; fail=$((fail + 1))
  fi
}

# A minimal backend: enough for `find_source` and the pyproject guard to be satisfied.
make_bundle() {
  local dir="$1" marker="$2" stamp="$3"
  mkdir -p "$dir/backend/middleware"
  printf '{}\n'            > "$dir/langgraph.json"
  printf '[project]\n'     > "$dir/pyproject.toml"
  printf '%s\n' "$marker"  > "$dir/backend/middleware/marker.txt"
  [ -n "$stamp" ] && printf '%s\n' "$stamp" > "$dir/.bundled-backend"
  return 0
}

run_setup() {
  MINIME_SETUP_STOP_AFTER_SOURCE=1 MINIME_BUNDLED_SOURCE="$1" \
    bash "$SETUP" "$2" >"$WORK/log" 2>&1 || {
      printf '    !!  setup-wsl.sh exited non-zero:\n'; sed 's/^/        /' "$WORK/log"; exit 1
    }
}

printf '\n==> A machine provisioned once, then handed a newer bundle\n'
BUNDLE="$WORK/bundle"; INSTALL="$WORK/install"
make_bundle "$BUNDLE" old aaaa1111
run_setup "$BUNDLE" "$INSTALL"
check "the first run installs it" "$(cat "$INSTALL/backend/middleware/marker.txt")" "old"

# The venv is what makes a blind re-copy unaffordable, so plant one and watch it survive.
mkdir -p "$INSTALL/.venv/lib"; printf 'built here\n' > "$INSTALL/.venv/lib/keep.txt"
printf 'KEY=secret\n' > "$INSTALL/.env"

rm -rf "$BUNDLE"; make_bundle "$BUNDLE" new bbbb2222
printf 'gone upstream\n' > "$INSTALL/backend/middleware/removed_upstream.py"
run_setup "$BUNDLE" "$INSTALL"
check "a newer bundle replaces the backend" "$(cat "$INSTALL/backend/middleware/marker.txt")" "new"
check "the stamp follows it"                "$(cat "$INSTALL/.bundled-backend")"            "bbbb2222"
check "a module deleted upstream goes"      "$([ -e "$INSTALL/backend/middleware/removed_upstream.py" ] && echo present || echo gone)" "gone"
check "the virtualenv survives"             "$(cat "$INSTALL/.venv/lib/keep.txt")"          "built here"
check "the machine's own .env survives"     "$(cat "$INSTALL/.env")"                        "KEY=secret"

printf '\n==> The same bundle again does nothing\n'
printf 'edited\n' > "$INSTALL/backend/middleware/marker.txt"
run_setup "$BUNDLE" "$INSTALL"
check "an unchanged stamp is left alone" "$(cat "$INSTALL/backend/middleware/marker.txt")" "edited"

printf '\n==> A developer checkout is never overwritten\n'
DEV="$WORK/dev"; DEVINSTALL="$WORK/dev-install"
make_bundle "$DEV" mine ""     # no stamp: somebody's working tree
run_setup "$DEV" "$DEVINSTALL"
printf 'my work in progress\n' > "$DEVINSTALL/backend/middleware/marker.txt"
rm -rf "$DEV"; make_bundle "$DEV" theirs ""
run_setup "$DEV" "$DEVINSTALL"
check "an unstamped source never overwrites" "$(cat "$DEVINSTALL/backend/middleware/marker.txt")" "my work in progress"

printf '\n==> %d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
