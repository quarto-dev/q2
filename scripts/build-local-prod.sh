#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
HUB_CLIENT_DIR="$(cd "$SCRIPT_DIR/../hub-client" && pwd)"
STATIC_PORT="$(node "$SCRIPT_DIR/local-prod-port.mjs" "$@")"

cd "$HUB_CLIENT_DIR"

# --client-only: skip the slow prerequisites (pandoc wasm fetch, Rust WASM
# build, sandboxed preview build). They all write into public/ or the wasm
# package dir, which vite build copies rather than rebuilds, so for a
# frontend-only change (src/, ts-packages/) the Vite step alone is enough.
# Use the full build after touching crates/, the sandboxed preview, or
# after a fresh clone.
CLIENT_ONLY=0
for arg in "$@"; do
    [ "$arg" = "--client-only" ] && CLIENT_ONLY=1
done

if [ "$CLIENT_ONLY" = "1" ]; then
    echo "[build-local-prod] --client-only: skipping pandoc/wasm/sandboxed builds"
else
    npm run fetch:pandoc
    npm run build:wasm
    npm run build:sandboxed
fi
# Optional auth: when OIDC_CLIENT_ID is set, bake it into the client so the
# sign-in button appears. The hub reads the same variable (local-prod.sh).
VITE_DEFAULT_SYNC_SERVER="ws://127.0.0.1:$STATIC_PORT/ws" \
VITE_Q2_SANDBOXED_PREVIEW_URL=http://127.0.0.1:8081/ \
VITE_GOOGLE_CLIENT_ID="${OIDC_CLIENT_ID:-}" \
NODE_OPTIONS=--max-old-space-size=4096 vite build
