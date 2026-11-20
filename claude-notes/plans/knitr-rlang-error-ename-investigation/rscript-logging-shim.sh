#!/bin/bash
# log which nextest test started R, then run the real binary
# Usage: put this file on PATH first as both Rscript and R (adjust the exec line), set RSHIM_LOG, run nextest.
p=$$; anc=""; for i in 1 2 3 4 5 6; do p=$(ps -o ppid= -p $p | tr -d ' '); [ -z "$p" ] && break; a=$(ps -o args= -p $p); case "$a" in *--exact*) anc="$a"; break;; esac; done
echo "${NEXTEST_TEST_NAME:-?}|${NEXTEST_BINARY_ID:-?}|Rscript|$anc" >> "${RSHIM_LOG:?set RSHIM_LOG}"
exec /usr/local/bin/Rscript "$@"
