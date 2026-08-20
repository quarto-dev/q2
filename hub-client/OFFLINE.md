# Offline Support

Quarto Hub includes offline support via Vite PWA plugin powered by Workbox.

## How it works

The service worker is automatically generated at build time by `vite-plugin-pwa` and provides:

1. **App shell precaching**: HTML, JS/CSS bundles, fonts, and icons are precached on first visit
2. **Runtime caching**: WASM, Monaco editor workers, and the sass chunk are cached as they're loaded
3. **Automatic updates**: when a new version activates, hidden tabs reload themselves immediately and visible tabs show an "Update available" toast
4. **Google Fonts caching**: External fonts are cached for offline use

## Caching strategy

### Precached (available immediately on first load)
The app shell is precached atomically during service worker installation (about 14 MB total):
- All HTML pages (`index.html`, `debug.html`, and friends)
- All JavaScript bundles, including the ~7.5 MB `main.js` entry
- All CSS files
- All local fonts (.woff, .woff2)
- Icons and SVG assets

`main.js` stays precached deliberately: the precache installs atomically per
service worker version, so the shell and the entry chunk can never skew.

### Runtime cached (loaded on demand)

These assets are content-hashed, so `CacheFirst` is correct: the URL changes
when the bytes change. They were moved out of the precache (GH #447) because an
all-or-nothing ~56 MB install meant one flaky fetch discarded the new service
worker entirely.

- **WASM files** (`/assets/*.wasm`) — `CacheFirst`, `wasm-cache`
  - Quarto parser (~26 MB), Automerge (~3.5 MB), tree-sitter
  - 30-day expiration, max 8 entries
- **Monaco workers + sass chunk** (`/assets/*.worker-*.js`, `sass.default-*.js`) — `CacheFirst`, `ondemand-assets`
  - 30-day expiration, max 12 entries

The entry caps matter: hashed URLs accumulate across deploys, and unlike the
precache, runtime caches have no automatic old-revision cleanup.

### Runtime cached (external resources)

- **Google Fonts CSS** - `CacheFirst`
  - Cached permanently once loaded
  - 1-year expiration, max 10 entries

- **Google Fonts files** - `CacheFirst`
  - Cached permanently once loaded
  - 1-year expiration, max 10 entries

## Update flow

The app registers the service worker through `virtual:pwa-register`
(`src/pwa.ts`) with `registerType: 'autoUpdate'`, so a new service worker
activates immediately (`skipWaiting()` + `clientsClaim()`). What happens next
depends on the tab:

- **Tab hidden** — it reloads itself immediately. A tab left open for hours
  silently heals onto the new version.
- **Tab visible** — an "Update available" toast appears and the tab reloads on
  its next transition to hidden, or immediately via the toast's Reload button.
  A visible tab is never reloaded against the user's will.

Update checks happen on page load, on an hourly interval, and on
hidden → visible transitions, so long-lived tabs discover deploys without a
reload. Focus-triggered checks are throttled to one per 5 minutes; the
hourly interval is not throttled, so the hidden-tab self-heal always runs
on schedule. A failed check (e.g. offline right after waking from sleep) is
swallowed and retried on the next trigger. Each check is a cheap
conditional request (nginx serves `sw.js` with `no-cache`).

## Limitations

This is **basic offline support** - it provides:
- ✅ App shell loads when offline (after first visit)
- ✅ Previously loaded pages/assets work offline
- ❌ No offline editing (requires server connection for Automerge sync)
- ❌ No background sync
- ❌ No push notifications

## Development

The service worker only registers in production builds. During development (`npm run dev`), it's disabled to avoid caching issues. E2E (`VITE_E2E=1`) and preview-embed (`VITE_DISABLE_PWA=1`) builds disable it too; in those builds the `virtual:pwa-register` import resolves to a no-op stub.

## Testing offline behavior

1. Build for production: `npm run build`
2. Serve the build: `npm run preview`
3. Open http://localhost:4173 in browser
4. Open DevTools → Application → Service Workers to verify registration
5. Navigate around the app to populate cache (the WASM lands in `wasm-cache` on first render)
6. Enable "Offline" mode in DevTools → Network tab
7. Reload the page - it should load from cache

## Configuration

PWA settings are in `vite.config.ts`:

```typescript
VitePWA({
  disable: isE2E || disablePwa,
  registerType: 'autoUpdate',
  workbox: {
    globPatterns: ['**/*.{html,js,css,svg,woff,woff2}'],
    globIgnores: ['**/*.worker-*.js', '**/sass.default-*.js'],
    maximumFileSizeToCacheInBytes: 16 * 1024 * 1024,
    runtimeCaching: [/* wasm-cache, ondemand-assets, google fonts */]
  }
})
```

The `maximumFileSizeToCacheInBytes` ceiling is a deliberate guard: past it the
build fails loudly, so a large asset can't silently find its way back into the
precache.

## Cache management

The service worker automatically:
- Updates when new versions are deployed
- Cleans up old precaches on activation (`cleanupOutdatedCaches`)
- Enforces size limits (max entries, max age) on runtime caches
- Handles cache storage quota exceeded errors

No manual cache version bumping is required - Workbox handles versioning automatically.
