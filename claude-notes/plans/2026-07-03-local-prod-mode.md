# Local Production Mode

**Created**: 2026-07-03  
**Updated**: 2026-07-07  
**Status**: ✅ Complete (All Phases)

## Overview

Add `npm run local-prod` command that mirrors production deployment architecture locally. Goal is dev/prod parity: catch nginx config issues, reverse proxy behavior, WebSocket routing, and header handling early.

## Current State vs. Production

**Current dev (`npm run dev`):**
- Vite dev server on :5173
- Proxies `/auth` and `/ws` to `http://localhost:3000`
- Hot module reload
- No nginx layer

**Production:**
- nginx :443 (TLS termination, reverse proxy)
- Routes `/ws` → hub :3000 (WebSocket upgrade)
- Routes `/auth`, `/api`, `/health` → hub :3000
- Routes `/assets/` → static files (cached indefinitely)
- Routes `/` → static files (no-cache, SPA fallback)
- hub binary serves sync protocol + auth endpoints

**Key differences to bridge:**
- No nginx layer in dev
- Vite HMR vs. static build
- HTTP vs. HTTPS
- Different cache headers

## Implementation Phases

### Phase 1: Basic orchestration ✅ COMPLETE
**Goal**: Get hub + built client working together locally

- [x] Add `scripts/local-prod.sh` orchestration script
- [x] Script starts hub binary in background
- [x] Script serves built hub-client with Node.js proxy for `/auth` and `/ws`
- [x] Graceful shutdown on Ctrl-C
- [x] Update `package.json` with `local-prod` script
- [x] Document in CLAUDE.md
- [x] Add `.local-prod-data/` to `.gitignore`

**Implementation notes:**
- Created `scripts/local-prod-server.mjs` - Node.js server that serves static files AND proxies `/auth` and `/ws` to hub
- Implements same cache headers as production (`/assets/` immutable, others no-cache)
- Handles WebSocket upgrades for `/ws` route
- Hub runs with `--allow-insecure-auth` for local development
- Port 3000 for hub, 8080 for static server
- Logs written to `.local-prod-data/{hub,static}.log`
- Added `npm run build:local-prod` script that sets `VITE_DEFAULT_SYNC_SERVER=ws://127.0.0.1:8080/ws` at build time
  - This is required because Vite bakes env vars into the bundle
  - Without this, the client would try to connect to `wss://sync.automerge.org` instead of the local hub

**Hub setup:**
```bash
cargo build --bin hub
./target/debug/hub \
  --data-dir ./.local-prod-data \
  -P 3000 \
  -H 127.0.0.1
```

**Static server options:**
- `python3 -m http.server 8080 --directory hub-client/dist`
- OR `npx serve hub-client/dist -l 8080`
- OR write simple Node.js server

### Phase 2: Add nginx via Docker Compose ✅ COMPLETE
**Goal**: Full parity with production proxy layer

- [x] Create `docker-compose.local-prod.yml`
- [x] Service: nginx (alpine image)
- [x] Service: hub-health-check (waits for hub before nginx starts)
- [x] Create `config/local-nginx.conf` (adapted from production)
  - HTTP only (no TLS for local)
   - Routes `/ws` → `host.docker.internal:3000` with WebSocket upgrade
  - Routes `/auth`, `/api`, `/health` → hub
  - Routes `/assets/` with immutable cache headers
  - Routes `/` to static files with no-cache
  - Same security headers as production
  - Gzip compression including WASM files
- [x] Script `local-prod-nginx.sh` starts hub + compose stack
- [x] Update `package.json` with `local-prod:nginx` script
- [x] Document nginx mode in CLAUDE.md and scripts/README.md

**Implementation notes:**
- Hub runs on HOST (not Docker) to avoid Rust/WASM build complexity in containers
- Hub binds to `0.0.0.0:3000` (not `127.0.0.1`) so Docker can reach it via `host.docker.internal`
- Health check container waits for hub before nginx starts (prevents startup race)
- nginx uses `alias` for `/assets/` to avoid path prefix issues
- Script follows nginx logs with `docker compose logs -f nginx`

**Config template:**
```nginx
server {
    listen 8080;
    server_name localhost;
    
    location /ws {
        proxy_pass http://host.docker.internal:3000;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection $connection_upgrade;
        # ... other headers
    }
    
    location /assets/ {
        root /var/www/hub-client;
        add_header Cache-Control "public, max-age=31536000, immutable";
    }
    
    location / {
        root /var/www/hub-client;
        try_files $uri $uri/ /index.html;
        add_header Cache-Control "no-cache";
    }
}
```

### Phase 3: Documentation & polish ✅ COMPLETE

- [x] Document differences from production (HTTP vs HTTPS, no OAuth, etc.)
- [x] Add troubleshooting section
- [x] Add `npm run local-prod:fresh` (clean build + data dir)
- [x] Update hub-client/README.md with usage examples

**Implementation notes:**
- Added comprehensive troubleshooting section to `scripts/README.md` covering:
  - Port conflicts, hub failures, Docker issues
  - WebSocket connection problems
  - CSS/MIME type console warnings (expected, harmless)
  - Stale build recovery
- Added `local-prod:fresh` and `local-prod:fresh:nginx` scripts that:
  - Rebuild client with `build:local-prod`
  - Clean `.local-prod-data/` directory
  - Start the chosen mode
- Updated `hub-client/README.md` with:
  - Full script reference table
  - Local production mode section
  - Prerequisites and usage
  - Phase 1 vs Phase 2 comparison
  - Differences from production

## Open Questions

1. **Data persistence**: Should `.local-prod-data/` be gitignored? (Yes, probably)
2. **Port conflicts**: What if :3000 or :8080 are taken? (Document or auto-detect)
3. **OAuth/auth**: Production uses OIDC. Do we need a mock? (Phase 1: skip auth)
4. **TLS in dev**: Worth the cert complexity? (No, HTTP is fine for local)
5. **Service worker**: Disable in local-prod like E2E tests? (Yes, cache interference)

## Success Criteria ✅

- [x] `npm run local-prod` starts cleanly
- [x] Browser connects to `http://localhost:8080`
- [x] WebSocket connection succeeds to hub
- [x] Static assets load with correct cache headers
- [x] Ctrl-C shuts down cleanly
- [x] Works on macOS (Windows/Linux untested but scripts are portable)
- [x] Phase 2 nginx mode works with Docker Compose
- [x] `.quarto/` paths properly proxied to hub
- [x] Comprehensive documentation and troubleshooting

## Future Enhancements (not in scope)

- Mock OIDC provider for auth testing
- HTTPS with self-signed cert
- PostgreSQL service for future persistence
- Redis for future caching
- Health check endpoints
- Metrics/monitoring setup
