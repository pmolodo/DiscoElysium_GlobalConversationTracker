#!/usr/bin/env bash
# The census driver lives in tools/measure-census.py; this is the name every note, log header
# and beads issue in this repository already spells - and the name tools/measure-matrix.py
# invokes when it has to take a census of its own.
#
# KEPT RATHER THAN DELETED, for the two reasons tools/measure-matrix.sh gives: the recorded
# incantations keep working, and tools/stop-measurements.sh finds a run by matching
# `measure-census` on the command line, which both this and the Python it runs carry.
#
# It passes everything through unchanged: arguments, environment, and the exit status.
exec python "$(dirname "$0")/measure-census.py" "$@"
