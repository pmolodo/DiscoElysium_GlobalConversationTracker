#!/usr/bin/env bash
# Every unit test and only those, in the debug profile: the loop to run WHILE ITERATING.
#
# THE FULL SUITE IS STILL THE GATE. Run `tools/run-logged.sh --kind testing cargo full-suite --
# cargo test --profile release-incremental` before committing, every time. This does not change what "the tests
# pass" means; it adds a second, weaker question that can be answered in seconds.
#
# WHAT IT COSTS, measured 2026-09-19 by touching a library source and asking for a verdict:
#
#   tools/smoke.sh - debug, --lib      6 s      471 unit tests
#   release, --lib                    58 s      the same 471
#   cargo test --release            4m50s       46 binaries, 57 s of it running tests
#
# SO THE LEVER IS NOT FEWER TESTS, IT IS FEWER LINKS - AND BOTH LEVERS ARE NEEDED. Every test in
# this repository finishes in under twelve seconds and most in under one: the whole of test
# EXECUTION is a minute, and the other four are compiling and linking twenty-nine integration
# binaries. `--lib` needs one binary, which is the first ten-fold; the debug profile is the
# second, and the middle row above is what skipping it would cost.
#
# A SUBSET BY CONSTRUCTION, NOT A LIST. A smoke run that passes while the full suite fails is
# worse than no smoke run, because it is trusted. "Every unit test" is a category `--lib` names
# exactly, so a test added to the library is in this from the moment it is written and nobody
# has to remember to add it. A hand-picked list would rot the first time somebody forgot.
#
# WHAT IT CANNOT TELL YOU: anything the integration tests ask. The engine's agreement with the
# oracle, the committed saves, the wire schema, the scenario suites and the kept-value checks are
# all integration binaries, and none of them runs here.
#
#   tools/smoke.sh                  every unit test
#   tools/smoke.sh guard            the unit tests whose name holds "guard"
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec "$ROOT/tools/run-logged.sh" --kind testing cargo smoke -- cargo test --lib "$@"
