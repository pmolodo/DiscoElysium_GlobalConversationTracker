# What a whole-game census says, read back off census.tsv.
#
#   awk -f tools/census-summary.awk <run>/census.tsv
#
# THREE THINGS WORTH KNOWING BEFORE AN UNREACHABLE SWEEP IS STARTED:
#
#   - the skip-rule split, which says how many rows the sweep will actually run. A group
#     with nothing unreachable poses no such question; one with exactly one would put a
#     single hard question in a set named for five.
#   - how many groups reported any UNDECIDED candidate. An undecided pass proves nothing
#     either way, so those groups' lists are lower bounds and nothing says so in the row.
#   - how many groups were CAPPED. Where `exact` is `at-least` the scan stopped at
#     CENSUS_WANTED and everything past it was never examined - so `unreachable` is a floor
#     rather than a count. A census taken with CENSUS_ALL=1 should show none of these.
BEGIN { FS = "\t" }

NR == 1 { next }

$2 == "CRASHED" { crashed++; next }

{
    groups++
    n = $3 + 0
    if (n == 0) none++
    else if (n == 1) one++
    else many++

    if ($4 + 0 > 0) { undecided_groups++; undecided_total += $4 }
    if ($5 == "at-least") capped++
    seconds += $6 / 1000
    if ($6 + 0 > slowest_ms) { slowest_ms = $6 + 0; slowest = $1 }
}

END {
    printf "groups censused      %d\n", groups
    printf "  crashed            %d\n", crashed + 0
    printf "\nskip-rule split\n"
    printf "  none unreachable   %-6d  both profiles skipped\n", none + 0
    printf "  exactly one        %-6d  runs -1, skips -5\n", one + 0
    printf "  two or more        %-6d  runs both\n", many + 0
    printf "  rows the sweep runs %d\n", (one + 0) + 2 * (many + 0)
    printf "\nhow far to trust it\n"
    printf "  groups with any undecided  %-6d (%d candidates in all)\n",
        undecided_groups + 0, undecided_total + 0
    printf "  groups capped              %-6d  'unreachable' is a floor for these\n",
        capped + 0
    printf "\ncost\n"
    printf "  total census time   %.1f minutes\n", seconds / 60
    printf "  slowest group       %s at %.1fs\n", slowest, slowest_ms / 1000
}
