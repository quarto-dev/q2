#!/usr/bin/env bash
set -euo pipefail

# Local production mode for Quarto Hub
# Runs the hub binary + serves built hub-client to mirror production setup

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HUB_CLIENT_DIR="$PROJECT_ROOT/hub-client"
DATA_DIR="$PROJECT_ROOT/.local-prod-data"
HUB_PORT=3001
STATIC_PORT=8080
Q2_SANDBOXED_PREVIEW_PORT=8081

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[local-prod]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[local-prod]${NC} $1"
}

log_error() {
    echo -e "${RED}[local-prod]${NC} $1"
}

# Cleanup function for graceful shutdown
cleanup() {
    log_info "Shutting down..."
    if [ ! -z "${HUB_PID:-}" ]; then
        kill "$HUB_PID" 2>/dev/null || true
    fi
    if [ ! -z "${STATIC_PID:-}" ]; then
        kill "$STATIC_PID" 2>/dev/null || true
    fi
    if [ ! -z "${Q2_SANDBOXED_PREVIEW_PID:-}" ]; then
        kill "$Q2_SANDBOXED_PREVIEW_PID" 2>/dev/null || true
    fi
    exit 0
}

trap cleanup SIGINT SIGTERM EXIT

# Check if hub-client is built
if [ ! -d "$HUB_CLIENT_DIR/dist" ]; then
    log_error "hub-client/dist not found. Run 'cd hub-client && npm run build:local-prod' first."
    exit 1
fi

# Check if hub binary exists
if [ ! -f "$PROJECT_ROOT/target/debug/hub" ] && [ ! -f "$PROJECT_ROOT/target/release/hub" ]; then
    log_error "hub binary not found. Run 'cargo build --bin hub' first."
    exit 1
fi

# Use release build if available, otherwise debug
HUB_BINARY="$PROJECT_ROOT/target/release/hub"
if [ ! -f "$HUB_BINARY" ]; then
    HUB_BINARY="$PROJECT_ROOT/target/debug/hub"
fi

# Create data directory if needed
mkdir -p "$DATA_DIR"

# Check if ports are available
if lsof -Pi :$HUB_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
    log_error "Port $HUB_PORT is already in use. Stop the other process first."
    exit 1
fi

if lsof -Pi :$STATIC_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
    log_error "Port $STATIC_PORT is already in use. Stop the other process first."
    exit 1
fi

if lsof -Pi :$Q2_SANDBOXED_PREVIEW_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
    log_error "Port $Q2_SANDBOXED_PREVIEW_PORT is already in use. Stop the other process first."
    exit 1
fi

log_info "Starting hub server on http://127.0.0.1:$HUB_PORT"
log_info "Using data directory: $DATA_DIR"

# Start hub binary in background
"$HUB_BINARY" \
    --data-dir "$DATA_DIR" \
    -P "$HUB_PORT" \
    -H 127.0.0.1 \
    --allow-insecure-auth \
    > "$DATA_DIR/hub.log" 2>&1 &

HUB_PID=$!

# Wait for hub to start
sleep 2
if ! kill -0 "$HUB_PID" 2>/dev/null; then
    log_error "Hub failed to start. Check $DATA_DIR/hub.log for details."
    tail -20 "$DATA_DIR/hub.log"
    exit 1
fi

log_info "Hub started (PID: $HUB_PID)"
log_info "Starting static file server + proxy on http://127.0.0.1:$STATIC_PORT"

# Start Node.js static server with proxy
STATIC_PORT=$STATIC_PORT HUB_PORT=$HUB_PORT \
    node "$SCRIPT_DIR/local-prod-server.mjs" > "$DATA_DIR/static.log" 2>&1 &
STATIC_PID=$!

# Wait for static server to start
sleep 1
if ! kill -0 "$STATIC_PID" 2>/dev/null; then
    log_error "Static server failed to start. Check $DATA_DIR/static.log for details."
    tail -20 "$DATA_DIR/static.log"
    exit 1
fi

log_info "Static server started (PID: $STATIC_PID)"

# Start q2-sandboxed-preview server
log_info "Starting q2-sandboxed-preview server on http://127.0.0.1:$Q2_SANDBOXED_PREVIEW_PORT"
Q2_SANDBOXED_PREVIEW_PORT=$Q2_SANDBOXED_PREVIEW_PORT \
    node "$SCRIPT_DIR/q2-sandboxed-preview-server.mjs" > "$DATA_DIR/q2-sandboxed-preview.log" 2>&1 &
Q2_SANDBOXED_PREVIEW_PID=$!

# Wait for q2-sandboxed-preview server to start
sleep 1
if ! kill -0 "$Q2_SANDBOXED_PREVIEW_PID" 2>/dev/null; then
    log_error "q2-sandboxed-preview server failed to start. Check $DATA_DIR/q2-sandboxed-preview.log for details."
    tail -20 "$DATA_DIR/q2-sandboxed-preview.log"
    exit 1
fi

log_info "q2-sandboxed-preview server started (PID: $Q2_SANDBOXED_PREVIEW_PID)"
echo ""
log_info "============================================"
log_info "Local production mode running!"
log_info "============================================"
log_info "Main app:              ${GREEN}http://127.0.0.1:$STATIC_PORT${NC}"
log_info "q2-sandboxed-preview:  ${GREEN}http://127.0.0.1:$Q2_SANDBOXED_PREVIEW_PORT${NC}"
log_info ""
log_info "Proxying /auth and /ws to hub (http://127.0.0.1:$HUB_PORT)"
log_info ""
log_info "Logs:"
log_info "  Hub:                  $DATA_DIR/hub.log"
log_info "  Static:               $DATA_DIR/static.log"
log_info "  q2-sandboxed-preview: $DATA_DIR/q2-sandboxed-preview.log"
log_info ""
log_info "Press Ctrl-C to stop"
echo ""

# Wait for either process to exit
wait
