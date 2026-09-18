#!/usr/bin/env bash
# Run search_residue's four arrangements many times each and count the deaths.
#
# The arrangement table in src/symbolic/isolated.rs is what the one-manager-per-thread rule
# rests on, and it is a table of DEATH RATES rather than of timings: a stack overflow on
# Windows is STATUS_STACK_OVERFLOW rather than a panic, so a run that dies takes its process
# with it and prints no closing line. One clean run therefore says nothing - the failure is
# intermittent, and the original counts were 20, 45, 20 and 35 runs.
#
# Usage:
#   tools/measure-residue-arms.sh [runs-per-arm]
#
# Defaults to the counts the original table used, which is what makes a new table comparable
# with it. A single number overrides all four, for a quicker look that is NOT comparable.
#
# Env:
#   BUDGET_MB   the diagram budget; 6144 reproduces the matrix, and the default 512 does not
#   SEARCHES    searches per run
#   ARMS        which arrangements to run, space separated; default all four
#
# Writes one log per run under the run folder, and prints the table at the end.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# The prefix is applied by the helper rather than typed - see CLAUDE.md.
. "$ROOT/tools/degct-env.sh"
DEGCT_BUDGET_MB="$(degct_env BUDGET_MB 6144)"
DEGCT_SEARCHES="$(degct_env SEARCHES 5)"
DEGCT_ARMS="$(degct_env ARMS 'main one each one-manager')"

# The counts behind the recorded table, per arm. An arm not named here falls back to 20.
runs_for() {
    if [ "$#" -ge 2 ] && [ -n "${2:-}" ]; then
        echo "$2"
        return
    fi
    case "$1" in
        one) echo 45 ;;
        one-manager) echo 35 ;;
        *) echo 20 ;;
    esac
}

OVERRIDE="${1:-}"
OUT="$ROOT/performance/logs/$(date +%Y-%m-%d_%H,%M,%S)_residue-arms"
mkdir -p "$OUT"

echo "budget ${DEGCT_BUDGET_MB} MB, ${DEGCT_SEARCHES} searches per run"
echo "logs in $OUT"
echo

cd "$ROOT" || exit 1
cargo build --release --example search_residue || exit 1

printf '%-14s %8s %8s\n' arrangement runs died
for arm in $DEGCT_ARMS; do
    runs="$(runs_for "$arm" "$OVERRIDE")"
    died=0
    for run in $(seq 1 "$runs"); do
        log="$OUT/${arm}-${run}.txt"
        if ! DEGCT_BUDGET_MB="$DEGCT_BUDGET_MB" DEGCT_SEARCHES="$DEGCT_SEARCHES" DEGCT_THREAD="$arm" \
            ./target/release/examples/search_residue >"$log" 2>&1; then
            died=$((died + 1))
        fi
    done
    printf '%-14s %8s %8s\n' "$arm" "$runs" "$died"
done
