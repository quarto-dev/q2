#!/usr/bin/env bash
#
# Start a local, no-auth Quarto hub watching the example project, then print
# the project's index-document id and the exact URLs/commands for the other
# two processes (hub-client dev server + q2 provide-hub executor).
#
# See README.md in this directory for the full walkthrough.
#
# Usage:
#   ./start-local-hub.sh          # hub on port 3031
#   PORT=4000 ./start-local-hub.sh

set -euo pipefail

PORT="${PORT:-3031}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
PROJECT="$HERE/project"
Q2="$REPO/target/debug/q2"

echo "Building q2 (if needed)…"
( cd "$REPO" && cargo build --bin q2 )

echo "Starting hub on 127.0.0.1:$PORT (no auth), watching $PROJECT …"
"$Q2" hub --project "$PROJECT" --port "$PORT" &
HUB_PID=$!
trap 'kill "$HUB_PID" 2>/dev/null || true' EXIT

# Wait for the hub to answer /health, then read the project's index doc id.
until curl -sf "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; do
  sleep 0.3
done
ID="$(curl -s "http://127.0.0.1:$PORT/health" \
  | sed 's/.*"index_document_id":"\([^"]*\)".*/\1/')"

cat <<EOF

────────────────────────────────────────────────────────────────────────
Hub is ready on ws://127.0.0.1:$PORT  (no auth — this is local only)
Project index-document id:
    $ID

Terminal 2 — hub-client dev server (point it at the local hub):
    cd "$REPO/hub-client"
    VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:$PORT npm run dev

Then open this URL in the browser (opens the example project):
    http://localhost:5173/#/share/$ID?server=ws://127.0.0.1:$PORT&file=hello.qmd&name=Local%20demo

Terminal 3 — the execution provider (offers THIS machine to run the code):
    cd "$REPO"
    cargo run --bin q2 -- provide-hub --server ws://127.0.0.1:$PORT --allow-all --token dev $ID

Then in the browser: open hello.qmd, switch to the preview, and click Run.
────────────────────────────────────────────────────────────────────────

Hub running (pid $HUB_PID). Press Ctrl-C to stop.
EOF

wait "$HUB_PID"
