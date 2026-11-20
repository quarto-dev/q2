#!/usr/bin/env bash
# RIU-6 Phase A: check, with no Positron window, that what `q2` exports for Positron satisfies the
# `positron_loader` mirror. RIU-5 owns these tests; this script only calls them, so the RIU-6 demo
# has one repeatable command to run before anyone opens the window.
#
#   scripts/riu-6-check-export-mirror.sh
#
# It runs, in `quarto-core`'s integration binary:
#   - positron_export::    the export function's cases (Python, duplicate hashes, Julia plot and
#                          error), including "an export the mirror would delete cannot be produced"
#   - positron_conformance::  the mirror itself: cache_version 1, and a code cell without outputs
#                          is deleted by Positron's loader
# A pass here says nothing about the Positron window; the commands are checked there by hand.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

# Not piped through anything: nextest output is left alone (it can hang when piped).
exec cargo nextest run -p quarto-core \
    -E 'binary(integration) & (test(positron_export::) | test(positron_conformance::))'
