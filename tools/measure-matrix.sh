#!/usr/bin/env bash
# The matrix driver lives in tools/measure-matrix.py; this is the name every note, memory,
# log header and beads issue in this repository already spells.
#
# KEPT RATHER THAN DELETED, and not for sentiment. Two reasons, both practical:
#
#   1. The recorded incantations. Dozens of places - CLAUDE.md, the measurement README, the
#      beads history, the run logs themselves - say `MATRIX_OUT=... tools/measure-matrix.sh
#      all`. Every one of those keeps working, which is the whole point of a driver being
#      resumable by the same command that started it.
#   2. tools/stop-measurements.sh finds a run by matching `measure-matrix` on the command
#      line. Both this and the Python it runs carry that, so a run started either way is
#      stoppable either way.
#
# It passes everything through unchanged: arguments, environment, and the exit status.
exec python "$(dirname "$0")/measure-matrix.py" "$@"
