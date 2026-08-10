#!/bin/zsh
# Stress loop for bd-u0tldu4z: capture failure output of the
# admin_collect_lifecycle tests (flaky ~3%/run per bd-eb2wnxkp measurement).
cd /Users/cscheid/rooms/room-1/q2 || exit 1
OUTDIR=/private/tmp/claude-501/-Users-cscheid-rooms-room-1-q2/32600a45-9169-48c8-a48a-c5d18af39218/scratchpad/stress-out
mkdir -p "$OUTDIR"
FAILS=0
for i in $(seq 1 150); do
  LOG="$OUTDIR/run.log"
  if ! cargo nextest run -p quarto-hub -E 'test(admin_collect_lifecycle)' --no-fail-fast > "$LOG" 2>&1; then
    FAILS=$((FAILS+1))
    cp "$LOG" "$OUTDIR/failure-$FAILS-iter-$i.log"
    echo "FAILURE $FAILS at iteration $i"
    [ "$FAILS" -ge 3 ] && break
  fi
done
echo "done: $FAILS failures in $i iterations"
