# Offline Support

Quarto Hub includes offline support via Vite PWA plugin powered by Workbox.

## How it works

The service worker is automatically generated at build time by `vite-plugin-pwa` and provides:

1. **App shell precaching**: HTML and icon are precached on first visit
2. **Runtime caching**: JS, CSS, and fonts are cached as they're loaded
3. **Automatic updates**: Service worker auto-updates when new versions are deployed
4. **Google Fonts caching**: External fonts are cached for offline use

## Caching strategy

### Precached (available immediately on first load)
Everything is precached during service worker installation (~44 MB total):
- All HTML pages (`index.html`, `debug.html`, `ast-renderer.html`)
- All JavaScript bundles (including 3MB+ main bundles)
- All CSS files
- All WASM files (including 32MB quarto parser + 2MB Automerge WASM)
- All local fonts (.woff, .woff2)
- Icons and SVG assets

### Runtime cached (external resources)

- **Google Fonts CSS** - `CacheFirst`
  - Cached permanently once loaded
  - 1-year expiration, max 10 entries
  
- **Google Fonts files** - `CacheFirst`
  - Cached permanently once loaded
  - 1-year expiration, max 10 entries

## Why precache everything?

Service workers don't control a page until the **second navigation**. On first visit:
1. Page loads normally from network
2. Service worker installs in background
3. Service worker is "waiting" and doesn't intercept requests yet

This means runtime caching (NetworkFirst, etc.) **doesn't work** until the second visit.

By precaching all assets during service worker installation:
- ✅ First visit: installs in background while page loads
- ✅ Refresh (even offline): everything loads from cache immediately
- ✅ No "second visit" needed - works offline right away

## Limitations

This is **basic offline support** - it provides:
- ✅ App shell loads when offline (after first visit)
- ✅ Previously loaded pages/assets work offline
- ❌ No offline editing (requires server connection for Automerge sync)
- ❌ No background sync
- ❌ No push notifications

## Development

The service worker only registers in production builds. During development (`npm run dev`), it's disabled to avoid caching issues.

## Testing offline behavior

1. Build for production: `npm run build`
2. Serve the build: `npm run preview`
3. Open http://localhost:4173 in browser
4. Open DevTools → Application → Service Workers to verify registration
5. Navigate around the app to populate cache
6. Enable "Offline" mode in DevTools → Network tab
7. Reload the page - it should load from cache

## Configuration

PWA settings are in `vite.config.ts`:

```typescript
VitePWA({
  registerType: 'autoUpdate',
  workbox: {
    globPatterns: ['**/*.{html,svg}'],
    maximumFileSizeToCacheInBytes: 3 * 1024 * 1024,
    runtimeCaching: [/* ... */]
  }
})
```

## Cache management

The service worker automatically:
- Updates when new versions are deployed
- Cleans up old caches
- Enforces size limits (max entries, max age)
- Handles cache storage quota exceeded errors

No manual cache version bumping is required - Workbox handles versioning automatically.
