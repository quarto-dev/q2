# Configurable Default Sync Server

**Issue:** bd-1g5f
**Priority:** Medium (2)
**Type:** Feature

## Overview

The hub-client currently hardcodes `wss://sync.automerge.org` as the default Automerge sync server URL. For internal deployments with private sync servers, beta users must manually type a custom `wss://` URL every time they create or connect to a project. This change uses Vite's built-in environment variable support to make the default configurable at build time.

## Work Items

- [x] Update `hub-client/src/utils/routing.ts` to read `DEFAULT_SYNC_SERVER` from `import.meta.env.VITE_DEFAULT_SYNC_SERVER` with fallback to `wss://sync.automerge.org`
- [x] Update `hub-client/src/components/ProjectSelector.tsx` to import `DEFAULT_SYNC_SERVER` from `routing.ts` instead of hardcoding the URL
- [x] Add `hub-client/.env` with documented default value
- [x] Add TypeScript type declaration for the new env variable (`env.d.ts` or `vite-env.d.ts`)
- [x] Verify `npm run build:all` works with and without the env variable set
- [x] Verify `npm run dev` works with `.env.local` override (covered by vitest — `.env.local` is loaded by Vite automatically; `*.local` is already gitignored)

## Details

**Approach:** Use Vite's native `import.meta.env` support. Variables prefixed with `VITE_` are automatically replaced at build time and exposed to client code.

**Usage for internal deployments:**
```bash
# At build time
VITE_DEFAULT_SYNC_SERVER=wss://123.123.123.123/ws npm run build:all

# Or via .env.local (gitignored)
echo 'VITE_DEFAULT_SYNC_SERVER=wss://123.123.123.123/ws' > hub-client/.env.local
npm run build:all
```

**Files to modify:**
1. `hub-client/src/utils/routing.ts` — single source of truth for the default
2. `hub-client/src/components/ProjectSelector.tsx` — use imported constant
3. `hub-client/.env` — document the default (committed to repo)
4. `hub-client/src/vite-env.d.ts` — add type for `VITE_DEFAULT_SYNC_SERVER`
