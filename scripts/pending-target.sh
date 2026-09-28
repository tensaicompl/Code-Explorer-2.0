#!/usr/bin/env bash
# Stands in for a make target whose owning task has not landed yet.
#
#   pending-target.sh <target> <owning task>
#
# It says so and succeeds, so the nightly run fails only on something real. It
# never runs a partial version of the target: skipped means nothing was checked.
# scripts/plan/plan-progress.py fails once the owning task is done while its target
# still points here, so a stand-in cannot outlive the task that replaces it.
set -euo pipefail
[ $# -eq 2 ] || { echo "usage: pending-target.sh <target> <owning task>" >&2; exit 2; }
echo "SKIPPED $1: not yet implemented; arrives with $2"
