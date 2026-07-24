#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HUB_CLIENT_DIR="$(cd "$SCRIPT_DIR/../hub-client" && pwd)"
STATIC_PORT="$(node "$SCRIPT_DIR/local-prod-port.mjs" "$@")"

cd "$HUB_CLIENT_DIR"
npm run build:wasm
npm run build:sandboxed
VITE_DEFAULT_SYNC_SERVER="ws://127.0.0.1:$STATIC_PORT/ws" \
VITE_Q2_SANDBOXED_PREVIEW_URL=http://127.0.0.1:8081/q2-sandboxed-preview.html \
NODE_OPTIONS=--max-old-space-size=4096 vite build
