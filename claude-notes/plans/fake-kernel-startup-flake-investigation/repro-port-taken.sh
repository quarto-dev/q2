#!/usr/bin/env bash
# bd-0w8rgjdf repro: run the fake kernel on a connection file whose shell port
# is already bound by another process, the way a port peeked by the daemon can
# be taken before the kernel binds it. Expected with the bug: exit 0, empty
# stderr (TestKernel::run's bind error is discarded by `let _ = handle.await`).
# Run from the repo root (macOS/Linux; needs python3).
set -euo pipefail
exe=$(cargo test -p quarto-core --lib --no-run --message-format=json 2>/dev/null \
  | python3 -c 'import sys,json
for l in sys.stdin:
    m=json.loads(l)
    if m.get("reason")=="compiler-artifact" and m.get("executable") and m["target"]["name"]=="quarto_core" and m["profile"]["test"]:
        print(m["executable"])' | tail -1)
work=$(mktemp -d)
# Hold one port open in a background python process; print it.
python3 - "$work/port" <<'PY' &
import socket, sys, time
s = socket.socket(); s.bind(("127.0.0.1", 0)); s.listen()
open(sys.argv[1], "w").write(str(s.getsockname()[1])); time.sleep(30)
PY
holder=$!
while [ ! -s "$work/port" ]; do sleep 0.1; done
taken=$(cat "$work/port")
free=$(python3 -c 'import socket
ps=[]
for _ in range(4):
    s=socket.socket(); s.bind(("127.0.0.1",0)); ps.append(s)
print(" ".join(str(s.getsockname()[1]) for s in ps))')
set -- $free
cat > "$work/conn.json" <<JSON
{"ip":"127.0.0.1","transport":"tcp","shell_port":$taken,"iopub_port":$1,"stdin_port":$2,
 "control_port":$3,"hb_port":$4,"signature_scheme":"hmac-sha256","key":"k","kernel_name":"fake"}
JSON
set +e
QUARTO_FAKE_KERNEL_SCRIPT='{}' perl -e 'alarm shift; exec @ARGV' 20 "$exe" --exact \
  engine::jupyter::fake_kernel::tests::fake_kernel_process_entry --nocapture "$work/conn.json" \
  >"$work/out" 2>"$work/err"
code=$?
set -e
kill $holder 2>/dev/null || true; wait $holder 2>/dev/null || true
echo "shell port $taken held elsewhere -> kernel exit code: $code"
echo "--- stderr ---"; cat "$work/err"
echo "--- stdout ---"; cat "$work/out"
