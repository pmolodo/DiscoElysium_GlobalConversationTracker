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
#   tools/measure-residue-arms.sh [--budget-mb MB] [--searches N] [--arms "a b"] [runs-per-arm]
#
# Defaults to the counts the original table used, which is what makes a new table comparable
# with it. A single number overrides all four, for a quicker look that is NOT comparable.
#
#   --budget-mb   the diagram budget; 6144 reproduces the matrix, and search_residue's own
#                 default of 512 does not
#   --searches    searches per run
#   --arms        which arrangements to run, space separated; default all four
#
# THE SETTINGS ARE PASSED TO THE DRIVER AS ARGUMENTS, which is what it takes - see de-3dx9.
# They used to be exported as DEGCT_ variables, and when search_residue stopped reading those
# every arm would have run at its own defaults: four identical arms, and a death-rate table
# that says the arrangement makes no difference.
#
# Writes one log per run under the run folder, and prints the table at the end.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# DEGCT_ ON THE LOCALS TOO, which is the rule in CLAUDE.md rather than an oversight: the
# collision that started it was a local, and a script whose every variable carries the prefix
# cannot shadow anything the shell owns.
DEGCT_BUDGET_MB=6144
DEGCT_SEARCHES=5
DEGCT_ARMS='main one each one-manager'
DEGCT_OVERRIDE=""

while [ "$#" -gt 0 ]; do
    case "$1" in
        --budget-mb)
            DEGCT_BUDGET_MB="$2"
            shift 2
            ;;
        --searches)
            DEGCT_SEARCHES="$2"
            shift 2
            ;;
        --arms)
            DEGCT_ARMS="$2"
            shift 2
            ;;
        -h | --help)
            sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        -*)
            echo "unknown option $1; see --help" >&2
            exit 2
            ;;
        *)
            DEGCT_OVERRIDE="$1"
            shift
            ;;
    esac
done

# The counts behind the recorded table, per arm. An arm not named here falls back to 20.
runs_for() {
    if [ -n "${2:-}" ]; then
        echo "$2"
        return
    fi
    case "$1" in
        one) echo 45 ;;
        one-manager) echo 35 ;;
        *) echo 20 ;;
    esac
}

DEGCT_OUT="$ROOT/performance/logs/$(date +%Y-%m-%d_%H,%M,%S)_residue-arms"
mkdir -p "$DEGCT_OUT"

echo "budget ${DEGCT_BUDGET_MB} MB, ${DEGCT_SEARCHES} searches per run"
echo "logs in $DEGCT_OUT"
echo

cd "$ROOT" || exit 1
cargo build --release --example search_residue || exit 1

printf '%-14s %8s %8s\n' arrangement runs died
for degct_arm in $DEGCT_ARMS; do
    degct_runs="$(runs_for "$degct_arm" "$DEGCT_OVERRIDE")"
    degct_died=0
    for degct_run in $(seq 1 "$degct_runs"); do
        degct_log="$DEGCT_OUT/${degct_arm}-${degct_run}.txt"
        if ! ./target/release/examples/search_residue \
            --budget-mb "$DEGCT_BUDGET_MB" \
            --searches "$DEGCT_SEARCHES" \
            --thread "$degct_arm" >"$degct_log" 2>&1; then
            degct_died=$((degct_died + 1))
        fi
    done
    printf '%-14s %8s %8s\n' "$degct_arm" "$degct_runs" "$degct_died"
done
