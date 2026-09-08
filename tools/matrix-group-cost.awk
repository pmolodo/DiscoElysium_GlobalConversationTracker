# What one group cost, read back off the rows a run recorded for it.
#
# Prints one line: "<search_ms> <peak_nodes> <complete>", which is what
# tools/measure-matrix.sh decides the serial/parallel split on. See the note above
# SETTLE_GROUPS there for what it does with them.
#
# READ FROM THE TSV RATHER THAN TIMED BY THE SHELL, and that is the point. Wall clock is
# not available for a row the resume skipped, and a resumed whole-game run is mostly
# skipped rows - so a split decided on wall clock would see a run of instant groups and
# conclude the cost had bottomed out before it had measured anything at all. What the file
# records is the cost of that group whenever it was measured, which is the same evidence
# either way.
#
# THE LAST ROW PER PROFILE WINS, because the files are appended to and a retried row sits
# after the one it replaces - the same rule the resume itself reads by.
#
# `complete` IS ZERO WHERE ANY CELL IS NOT A NUMBER, which is how a group holding a
# CRASHED, NOT-MEASURED or otherwise unfilled row declines to be evidence of anything. A
# group whose rows are all NO-ROWS reports zero as well: it measured nothing, so it says
# nothing about whether the measuring has got cheap.
#
#   awk -f tools/matrix-group-cost.awk performance-matrix-625.tsv

BEGIN { FS = "\t" }

$1 == "conv" {
    for (i = 1; i <= NF; i++) {
        if ($i ~ /_ms$/) ms[i] = 1
        else if ($i ~ /_nodes$/) held[i] = 1
        else if ($i ~ /_verdict$/) verdict[i] = 1
        else if ($i == "profile") profile_column = i
    }
    next
}

profile_column && NF > 1 { last[$profile_column] = $0 }

END {
    complete = 1
    for (profile in last) {
        n = split(last[profile], cell, FS)

        # A row with nothing in it is not a cost. Skipped rather than counted as zero: a
        # zero would drag the group's total towards the floor and make an empty group look
        # like a cheap one.
        empty = 1
        for (i in verdict) if (cell[i] != "NO-ROWS") empty = 0
        if (empty) continue
        rows++

        for (i in ms) {
            if (cell[i] ~ /^[0-9]+$/) total_ms += cell[i]
            else complete = 0
        }
        for (i in held) {
            if (cell[i] !~ /^[0-9]+$/) complete = 0
            else if (cell[i] + 0 > peak_nodes) peak_nodes = cell[i] + 0
        }
    }
    if (rows == 0) complete = 0
    print total_ms + 0, peak_nodes + 0, complete
}
