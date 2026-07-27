#!/usr/bin/env bash
set -euo pipefail

# Local production mode with nginx (native)
# Runs hub binary + nginx on host for full production parity

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
HUB_CLIENT_DIR="$PROJECT_ROOT/hub-client"
DATA_DIR="$PROJECT_ROOT/.local-prod-data"
HUB_PORT=3000
NGINX_PORT=8080
Q2_SANDBOXED_PREVIEW_PORT=8081

# Color output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[local-prod-nginx]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[local-prod-nginx]${NC} $1"
}

log_error() {
    echo -e "${RED}[local-prod-nginx]${NC} $1"
}

log_step() {
    echo -e "${BLUE}[local-prod-nginx]${NC} $1"
}

# Cleanup function for graceful shutdown
cleanup() {
    log_info "Shutting down..."

    # Stop nginx
    if [ ! -z "${NGINX_STARTED:-}" ]; then
        log_step "Stopping nginx..."
        nginx -s stop -c "$DATA_DIR/nginx.conf" 2>/dev/null || true
    fi

    # Stop q2-sandboxed-preview server
    if [ ! -z "${Q2_SANDBOXED_PREVIEW_PID:-}" ]; then
        log_step "Stopping q2-sandboxed-preview server..."
        kill "$Q2_SANDBOXED_PREVIEW_PID" 2>/dev/null || true
    fi

    # Stop hub
    if [ ! -z "${HUB_PID:-}" ]; then
        log_step "Stopping hub..."
        kill "$HUB_PID" 2>/dev/null || true
    fi

    # Clean up temp config
    rm -f "$DATA_DIR/nginx.conf" 2>/dev/null || true

    log_info "Cleanup complete"
    exit 0
}

trap cleanup SIGINT SIGTERM EXIT

# Check prerequisites
log_step "Checking prerequisites..."

# Check nginx
if ! command -v nginx &> /dev/null; then
    log_error "nginx not found. Install via: brew install nginx"
    exit 1
fi

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

# Check if ports are available
if lsof -Pi :$HUB_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
    log_error "Port $HUB_PORT is already in use. Stop the other process first."
    exit 1
fi

if lsof -Pi :$NGINX_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
    log_error "Port $NGINX_PORT is already in use. Stop the other process first."
    exit 1
fi

if lsof -Pi :$Q2_SANDBOXED_PREVIEW_PORT -sTCP:LISTEN -t >/dev/null 2>&1; then
    log_error "Port $Q2_SANDBOXED_PREVIEW_PORT is already in use. Stop the other process first."
    exit 1
fi

# Create data directory
mkdir -p "$DATA_DIR"

# Generate nginx config with absolute paths
log_step "Generating nginx configuration..."
DIST_PATH_ABSOLUTE="$HUB_CLIENT_DIR/dist"
sed "s|DIST_PATH|$DIST_PATH_ABSOLUTE|g" "$PROJECT_ROOT/config/local-nginx.conf" > "$DATA_DIR/nginx.conf"

# Add required nginx directives (pid, error_log, events, http wrapper)
cat > "$DATA_DIR/nginx.conf.tmp" << EOF
pid $DATA_DIR/nginx.pid;
error_log $DATA_DIR/nginx-error.log;

events {
    worker_connections 1024;
}

http {
    include /opt/homebrew/etc/nginx/mime.types;
    default_type application/octet-stream;

    access_log $DATA_DIR/nginx-access.log;

$(cat "$DATA_DIR/nginx.conf")
}
EOF
mv "$DATA_DIR/nginx.conf.tmp" "$DATA_DIR/nginx.conf"

# Start q2-sandboxed-preview server first (needed before nginx)
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
log_step "Waiting for hub to become ready..."
sleep 2

if ! kill -0 "$HUB_PID" 2>/dev/null; then
    log_error "Hub failed to start. Check $DATA_DIR/hub.log for details."
    tail -20 "$DATA_DIR/hub.log"
    exit 1
fi

# Wait for hub health endpoint
for i in {1..10}; do
    if curl -f http://127.0.0.1:$HUB_PORT/health >/dev/null 2>&1; then
        log_info "Hub is ready (PID: $HUB_PID)"
        break
    fi
    if [ $i -eq 10 ]; then
        log_error "Hub health check failed after 10 attempts"
        tail -20 "$DATA_DIR/hub.log"
        exit 1
    fi
    sleep 1
done

# Start nginx
log_step "Starting nginx..."
nginx -c "$DATA_DIR/nginx.conf"

if [ $? -ne 0 ]; then
    log_error "Failed to start nginx"
    cat "$DATA_DIR/nginx-error.log"
    exit 1
fi

NGINX_STARTED=1

# Wait for nginx to be ready
log_step "Waiting for nginx to become ready..."
for i in {1..10}; do
    if curl -f http://127.0.0.1:$NGINX_PORT/ >/dev/null 2>&1; then
        break
    fi
    if [ $i -eq 10 ]; then
        log_error "Nginx failed to become ready"
        cat "$DATA_DIR/nginx-error.log"
        exit 1
    fi
    sleep 1
done

log_info "nginx started successfully"
echo ""
log_info "============================================"
log_info "Local production mode (with nginx) running!"
log_info "============================================"
log_info "Main app:  ${GREEN}http://127.0.0.1:$NGINX_PORT${NC}"
log_info "q2-sandboxed-preview:    ${GREEN}http://127.0.0.1:$Q2_SANDBOXED_PREVIEW_PORT${NC}"
log_info ""
log_info "Architecture:"
log_info "  Browser → nginx:8080 (native)"
log_info "    ├─ /ws → hub:3000 (WebSocket)"
log_info "    ├─ /auth → hub:3000"
log_info "    └─ /* → static files"
log_info "  Browser → nginx:8081 → q2-sandboxed-preview:8081 (sandboxed)"
log_info ""
log_info "Logs:"
log_info "  Hub:    $DATA_DIR/hub.log"
log_info "  q2-sandboxed-preview: $DATA_DIR/q2-sandboxed-preview.log"
log_info "  Nginx access:  $DATA_DIR/nginx-access.log"
log_info "  Nginx error:   $DATA_DIR/nginx-error.log"
log_info ""
log_info "Press Ctrl-C to stop"
echo ""

# Follow nginx access log
tail -f "$DATA_DIR/nginx-access.log"
