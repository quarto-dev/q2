#!/usr/bin/env bash
set -euo pipefail

# Local dev mode for Quarto Hub
# Runs the hub binary + q2-sandboxed-preview server in the background and the
# Vite dev server (hot reload) in the foreground. Vite proxies /auth and /ws
# to the hub. Unlike local-prod there is no build step for the app itself;
# run `npm run preflight` (or `npm run build:wasm` / `build:sandboxed`)
# first if the WASM module or sandboxed preview is missing or stale.
# Extra arguments are passed through to Vite (e.g. --host).

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HUB_CLIENT_DIR="$PROJECT_ROOT/hub-client"
DATA_DIR="$PROJECT_ROOT/.local-prod-data"
HUB_PORT=3000
Q2_SANDBOXED_PREVIEW_PORT=8081

GREEN='\033[0;32m'
RED='\033[0;31m'
NC='\033[0m'

log_info() { echo -e "${GREEN}[local-dev]${NC} $1"; }
log_error() { echo -e "${RED}[local-dev]${NC} $1"; }

cleanup() {
    trap - SIGINT SIGTERM EXIT
    log_info "Shutting down..."
    if [ -n "${HUB_PID:-}" ]; then
        kill "$HUB_PID" 2>/dev/null || true
    fi
    if [ -n "${Q2_SANDBOXED_PREVIEW_PID:-}" ]; then
        kill "$Q2_SANDBOXED_PREVIEW_PID" 2>/dev/null || true
    fi
}

trap cleanup SIGINT SIGTERM EXIT

HUB_BINARY="$PROJECT_ROOT/target/debug/hub"
if [ ! -f "$HUB_BINARY" ]; then
    HUB_BINARY="$PROJECT_ROOT/target/release/hub"
fi
if [ ! -f "$HUB_BINARY" ]; then
    log_error "hub binary not found. Run 'cargo build --bin hub' first."
    exit 1
fi

if [ ! -d "$HUB_CLIENT_DIR/public/q2-sandboxed-preview" ]; then
    log_error "hub-client/public/q2-sandboxed-preview not found. Run 'cd hub-client && npm run build:sandboxed' first."
    exit 1
fi

for port in "$HUB_PORT" "$Q2_SANDBOXED_PREVIEW_PORT"; do
    if lsof -Pi :"$port" -sTCP:LISTEN -t >/dev/null 2>&1; then
        log_error "Port $port is already in use. Stop the other process first."
        exit 1
    fi
done

mkdir -p "$DATA_DIR"

log_info "Starting hub server on http://127.0.0.1:$HUB_PORT (data: $DATA_DIR)"
"$HUB_BINARY" \
    --data-dir "$DATA_DIR" \
    -P "$HUB_PORT" \
    -H 127.0.0.1 \
    --allow-insecure-auth \
    > "$DATA_DIR/hub.log" 2>&1 &
HUB_PID=$!

sleep 2
if ! kill -0 "$HUB_PID" 2>/dev/null; then
    log_error "Hub failed to start. Check $DATA_DIR/hub.log for details."
    tail -20 "$DATA_DIR/hub.log"
    exit 1
fi

log_info "Starting q2-sandboxed-preview server on http://127.0.0.1:$Q2_SANDBOXED_PREVIEW_PORT"
Q2_SANDBOXED_PREVIEW_PORT=$Q2_SANDBOXED_PREVIEW_PORT \
    node "$SCRIPT_DIR/q2-sandboxed-preview-server.mjs" > "$DATA_DIR/q2-sandboxed-preview.log" 2>&1 &
Q2_SANDBOXED_PREVIEW_PID=$!

sleep 1
if ! kill -0 "$Q2_SANDBOXED_PREVIEW_PID" 2>/dev/null; then
    log_error "q2-sandboxed-preview server failed to start. Check $DATA_DIR/q2-sandboxed-preview.log for details."
    tail -20 "$DATA_DIR/q2-sandboxed-preview.log"
    exit 1
fi

log_info "Logs: $DATA_DIR/hub.log, $DATA_DIR/q2-sandboxed-preview.log"
log_info "Starting Vite dev server (Ctrl-C to stop everything)"

cd "$HUB_CLIENT_DIR"
# .env defaults the sync server to the public wss://sync.automerge.org; point new
# projects at the local hub instead (a relative /ws resolves against the page
# origin, so it works on whatever port Vite picks, via Vite's /ws proxy).
VITE_DEFAULT_SYNC_SERVER=/ws \
VITE_HUB_SERVER="http://127.0.0.1:$HUB_PORT" \
VITE_Q2_SANDBOXED_PREVIEW_URL="http://127.0.0.1:$Q2_SANDBOXED_PREVIEW_PORT/" \
    npx vite "$@"
