# What a matrix run established, read back off its TSVs.
#
#   awk -f tools/matrix-summary.awk <run>/performance-matrix-*.tsv
#
# BY HEADER NAME, not by column position, which is the only way to read these: the columns
# have changed several times (a `real` column added and removed, `fwdbwd` split into
# `ingame` and `nolimit`, `nodes` added to the portfolio columns), and older folders are
# still worth reading. See "READING AN OLDER RUN" in measurements/README.md.
#
# It separates the three things a row can be - measured, skipped by a profile's own rule, or
# a group with nothing to measure - because a total that mixes them says nothing. The
# skipped ones are not failures: a deepest-unreach profile declines a group with nothing
# unreachable in it, and the rule that declined is in the verdict column.
BEGIN { FS = "\t" }

/^conv\t/ {
    delete ms; delete verdict; profile_col = 0
    for (i = 1; i <= NF; i++) {
        if ($i == "profile") profile_col = i
        else if ($i ~ /_ms$/) { ms[i] = 1; engine[i] = $i; sub(/_ms$/, "", engine[i]) }
        else if ($i ~ /_verdict$/) { verdict[i] = 1; vengine[i] = $i; sub(/_verdict$/, "", vengine[i]) }
    }
    next
}

profile_col == 0 { next }

{
    profile = $profile_col
    for (i in verdict) {
        v = $i
        if (v == "NO-ROWS") { norows[profile]++; next }
        if (v ~ /^SKIPPED/) { skipped[profile]++; rule[v]++; next }
        break
    }
    measured[profile]++
    for (i in verdict) { outcome[vengine[i], profile, $i]++; seen_verdict[$i] = 1 }
    for (i in ms) {
        if ($i ~ /^[0-9]+$/) {
            total[engine[i], profile] += $i
            cells[engine[i], profile]++
            if ($i + 0 > peak[engine[i], profile]) {
                peak[engine[i], profile] = $i + 0
                peak_at[engine[i], profile] = $1
            }
        }
    }
}

END {
    printf "ROWS\n  %-22s %9s %9s %9s\n", "profile", "measured", "skipped", "no-rows"
    for (p in measured) hit[p] = 1
    for (p in skipped) hit[p] = 1
    for (p in norows) hit[p] = 1
    for (p in hit)
        printf "  %-22s %9d %9d %9d\n", p, measured[p] + 0, skipped[p] + 0, norows[p] + 0

    if (length(rule)) {
        printf "\n  why rows were skipped\n"
        for (r in rule) printf "    %-30s %d\n", r, rule[r]
    }

    printf "\nVERDICTS over measured rows\n"
    for (k in outcome) {
        split(k, part, SUBSEP)
        printf "  %-10s %-22s %-20s %d\n", part[1], part[2], part[3], outcome[k]
    }

    printf "\nENGINE TIME over measured rows\n"
    printf "  %-10s %-22s %8s %11s %11s %10s\n",
        "engine", "profile", "rows", "mean ms", "peak ms", "peak group"
    for (k in cells) {
        split(k, part, SUBSEP)
        if (cells[k] > 0)
            printf "  %-10s %-22s %8d %11.0f %11d %10s\n",
                part[1], part[2], cells[k], total[k] / cells[k], peak[k], peak_at[k]
    }
}
