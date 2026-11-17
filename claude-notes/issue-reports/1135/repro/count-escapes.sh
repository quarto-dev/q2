#!/usr/bin/env bash
# Count ANSI SGR (ESC[) and OSC 8 (ESC]8;) sequences on q2's stderr in
# each output mode. Run from a directory OUTSIDE any q2 project (the
# claude-notes project would otherwise claim index.qmd):
#
#   cp -r claude-notes/issue-reports/1135/repro /some/tmp/dir && cd /some/tmp/dir
#   Q=/path/to/q2 ./count-escapes.sh
set -u
Q=${Q:-q2}
count() { python3 -I -c "import sys; d=open(sys.argv[1],errors='replace').read(); print('SGR', d.count('\x1b['), 'OSC8', d.count('\x1b]8;'))" "$1"; }
printf -- '---\ntitle: ok\n---\n\nhi\n' > ok.qmd
run() { local name=$1; shift; "$@" 2>"$name.err" >/dev/null; printf '%-28s ' "$name"; count "$name.err"; }
run diag-plain               "$Q" render index.qmd
run diag-NO_COLOR            env NO_COLOR=1 "$Q" render index.qmd
run diag-json-errors         "$Q" render index.qmd --json-errors
run tracing-plain            "$Q" render ok.qmd -vv
run tracing-NO_COLOR         env NO_COLOR=1 "$Q" render ok.qmd -vv
"$Q" render --help > help.out; printf '%-28s ' help-piped; count help.out
