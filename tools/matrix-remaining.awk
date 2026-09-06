# How long a matrix run has left, weighted by what each row has cost before.
#
# The flat estimate this replaces - the mean row so far, spread over the rows that remain -
# assumes the rows are alike, and they are not. Each conversation's adversarial profiles
# run first and are the slowest, so a flat estimate reads long early and short late; and a
# run that narrows the grid to one engine would be scaled from rows measured with three.
#
# WHAT THIS DOES INSTEAD. Past runs already record what every (engine, conversation,
# profile) cost, in the per-engine ms columns of every performance-matrix-*.tsv. Average
# those into a WEIGHT per row, then let this run's completed rows say how fast this machine
# is running today:
#
#     ratio     = seconds actually spent on the rows already done
#                 / the summed weight of those same rows
#     estimate  = ratio * the summed weight of the rows not yet run
#
# So three rows that have cost 5, 3 and 2 minutes before, whose first row has just taken
# 10 rather than 5, have 10 minutes left rather than 5: the pace has changed and the SHAPE
# of what remains has not.
#
# Usage:
#     awk -v engines=fwd,bwd,fwdbwd \
#         -v done='14:deepest-1=126;28:deepest-5=310' \
#         -v left='14:deepest-5;14:deepest-10' \
#         -f tools/matrix-remaining.awk <past run TSVs>
#
# Prints whole seconds, or nothing at all when it cannot answer - no history, no completed
# row to calibrate against, or nothing left to estimate. The caller falls back to the flat
# mean and should say that it has.

function mean(key,    n) {
    n = count[key]
    return n > 0 ? total[key] / n : 0
}

# What one row should cost, summed over the engines this run measures.
#
# FOUR FALLBACKS, because a tuple with no history must not silently weigh nothing - a row
# dropped from the total is a row the estimate says is free. A conversation is a better
# proxy for an unmeasured profile than a profile is for an unmeasured conversation, so
# (engine, conversation) is tried before (engine, profile).
function weight(conv, profile,    i, engine, key, sum) {
    sum = 0
    for (i = 1; i <= engine_count; i++) {
        engine = engines_wanted[i]
        key = engine SUBSEP conv SUBSEP profile
        if (count[key] > 0) {
            sum += mean(key)
        } else if (count[engine SUBSEP conv] > 0) {
            sum += mean(engine SUBSEP conv)
        } else if (count[engine SUBSEP profile] > 0) {
            sum += mean(engine SUBSEP profile)
        } else if (count[engine] > 0) {
            sum += mean(engine)
        } else {
            sum += mean("")
        }
    }
    return sum
}

BEGIN {
    FS = "\t"
    OFS = "\t"

    engine_count = split(engines, engines_wanted, ",")
    for (i = 1; i <= engine_count; i++) {
        gsub(/^[ \t]+|[ \t]+$/, "", engines_wanted[i])
    }
}

# A new file: read its header, because a run holds only the engines it was asked for and
# older runs spell two of them differently.
FNR == 1 {
    delete column_engine
    conv_column = 0
    profile_column = 0

    # WHICH ERA THIS FILE IS FROM, decided before anything is renamed, because `fwd` has
    # meant two different searches and only the company it keeps says which.
    #
    #   oldest      fwd, bwd                 - fwd is a state-at-a-time search, bwd is the
    #                                          symbolic forward one.
    #   middle      explicit, symfwd, symbwd - symfwd is today's fwd, symbwd today's bwd.
    #   current     fwd, bwd, fwdbwd         - direction is what tells them apart.
    era_current = 0
    era_middle = 0
    for (i = 1; i <= NF; i++) {
        if ($i == "fwdbwd_ms") era_current = 1
        if ($i == "explicit_ms") era_middle = 1
    }

    for (i = 1; i <= NF; i++) {
        name = $i
        if (name == "conv") {
            conv_column = i
        } else if (name == "profile") {
            profile_column = i
        } else if (name ~ /_ms$/) {
            engine = substr(name, 1, length(name) - 3)
            if (era_middle) {
                # de-zovl's names for the two that survive.
                if (engine == "symfwd") engine = "fwd"
                else if (engine == "symbwd") engine = "bwd"
            } else if (!era_current) {
                # The oldest runs. `bwd` was the symbolic forward search and is a usable
                # weight for today's `fwd`. Its `fwd` was a state-at-a-time search that
                # nothing measures now, so it keeps a name nothing asks about rather than
                # poisoning the column that bears that name today.
                if (engine == "bwd") engine = "fwd"
                else if (engine == "fwd") engine = "explicit"
            }
            column_engine[i] = engine
        }
    }
    next
}

conv_column > 0 && profile_column > 0 {
    conv = $conv_column
    profile = $profile_column

    for (i in column_engine) {
        # A crashed or never-run row leaves "?" here, and a "?" is not a duration.
        if ($i !~ /^[0-9]+$/) {
            continue
        }
        seconds = $i / 1000
        engine = column_engine[i]

        total[engine SUBSEP conv SUBSEP profile] += seconds
        count[engine SUBSEP conv SUBSEP profile]++
        total[engine SUBSEP conv] += seconds
        count[engine SUBSEP conv]++
        total[engine SUBSEP profile] += seconds
        count[engine SUBSEP profile]++
        total[engine] += seconds
        count[engine]++
        total[""] += seconds
        count[""]++
    }
}

END {
    # Nothing measured anywhere: the caller's flat mean is the honest answer.
    if (count[""] == 0) {
        exit 1
    }

    done_count = split(done, done_rows, ";")
    spent = 0
    spent_weight = 0
    for (i = 1; i <= done_count; i++) {
        if (done_rows[i] == "") {
            continue
        }
        split(done_rows[i], row, "=")
        split(row[1], where, ":")
        spent += row[2]
        spent_weight += weight(where[1], where[2])
    }

    left_count = split(left, left_rows, ";")
    left_weight = 0
    for (i = 1; i <= left_count; i++) {
        if (left_rows[i] == "") {
            continue
        }
        split(left_rows[i], where, ":")
        left_weight += weight(where[1], where[2])
    }

    # No row finished yet, or every row that did weighs nothing, so there is no pace to
    # measure. Nothing left to estimate is not an error either, but it is not a number.
    if (spent <= 0 || spent_weight <= 0 || left_weight <= 0) {
        exit 1
    }

    printf "%d\n", (spent / spent_weight) * left_weight
}
