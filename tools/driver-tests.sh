#!/usr/bin/env bash
# Every test of the measurement drivers in tools/, and only those.
#
# WHY THERE IS A SEPARATE SUITE AT ALL. The drivers carry real logic - the settle rule, the
# resume matching, the cold-run decision, the binary-digest guard, the folding of several runs
# into one table - and none of it is Rust or C#, so neither existing suite can reach it. What
# they have in common is that they are decisions rather than measurements: right or wrong on
# their inputs, with no machine and no game in the way. The whole suite is a fraction of a
# second, and it does not build anything.
#
# THE STANDARD LIBRARY RUNS IT, because a test suite that needs a dependency installed is one
# that does not get run. unittest discovery is in every Python this repository already requires;
# pytest would be nicer to write and one more thing to have.
#
#   tools/driver-tests.sh              every driver test
#   tools/driver-tests.sh -v           naming each one as it runs
#   tools/driver-tests.sh -k Drift     the ones whose name holds "Drift" - matched case-sensitively
#
# A test goes in tools/tests/test_<driver>.py, and reaches its driver through
# `drivers.load("measure-menus")` - the drivers are named with hyphens, so a plain import
# cannot see them.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TESTS="$ROOT/tools/tests"

# Discovery always, with anything given appended to it: -s and -t are what nobody remembers, and
# leaving them to the caller is how a suite ends up half run.
exec "$ROOT/tools/run-logged.sh" --kind testing python driver-tests -- \
    python -m unittest discover -s "$TESTS" -t "$TESTS" "$@"
