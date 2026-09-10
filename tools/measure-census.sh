#!/usr/bin/env bash
# The census driver lives in tools/measure-census.py; this is the name that notes, log headers
# and beads issues in this repository already spell.
#
# KEPT RATHER THAN DELETED so those recorded incantations keep working. Nothing in the
# repository calls it: Windows cannot execute a shell script as a program, so a caller that
# needs a census runs measure-census.py with its own interpreter.
#
# It passes everything through unchanged: arguments, environment, and the exit status.
exec python "$(dirname "$0")/measure-census.py" "$@"
