# q2-sandboxed-preview Separate Origin Design

> Updated 2026-09-10 as part of the q2-preview → sandboxed-preview port
> (`claude-notes/plans/2026-09-01-port-q2-preview-into-sandboxed-preview.md`,
> epic bd-r9yr0hbe). The original document described a prototype that
> rendered raw AST JSON; the sandboxed frame now runs the full
> `@quarto/preview-renderer` renderer.

## Problem

The sandboxed preview renders untrusted document content (including user
TSX components) in an iframe. For security isolation it is served from a
**separate origin** (cross-origin) rather than the same origin as the
main hub-client application.

## Architecture

```
Main app:   https://your-hub-domain.com/
            ├─ Serves the main React app (owns WASM + VFS)
            └─ Q2SandboxedPreviewIframe (parent side of the protocol)

Sandbox:    https://quarto-dev.github.io/q2/          (GitHub Pages)
            ├─ index.html + q2-preview-assets/* (renderer bundle, KaTeX fonts)
            ├─ serviceWorker.js (asset proxy)
            └─ Communicates ONLY via postMessage
```

- Deployment: `.github/workflows/deploy-sandboxed-preview.yml` publishes
  `hub-client/quarto-hub-sandboxed-preview/dist/` to GitHub Pages (see
  `.github/workflows/github-pages.md`).
- Local dev fallback: the build also copies dist/ to
  `hub-client/public/q2-sandboxed-preview/` (gitignored); point
  `VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html` at it.
- local-prod: `scripts/q2-sandboxed-preview-server.mjs` serves the dist
  dir on port 8081 to simulate the separate origin
  (`VITE_Q2_SANDBOXED_PREVIEW_URL=http://127.0.0.1:8081/`).

### Security posture

1. **Origin isolation**: the frame runs on a separate origin — no
   cookies, no localStorage, no DOM reach into the parent, no WASM/VFS
   access.
2. **`sandbox="allow-scripts allow-same-origin"` + separate origin**:
   `allow-same-origin` is load-bearing (an opaque-origin frame cannot
   register the service worker); the isolation comes from the separate
   origin, not the sandbox attribute.
3. **Source checks both ways**: the parent ignores messages whose
   `event.source` is not the iframe's `contentWindow`; the frame ignores
   messages whose `event.source` is not `window.parent` (including the
   page bridge's `url_response` listener — a forged response would
   inject attacker bytes as document assets).
4. **Pinned target origins**: the parent posts to the sandbox origin
   derived from the iframe URL; the frame pins the parent origin from
   the first accepted message. Only the pre-contact `IFRAME_READY` and
   SW-bridge `url` posts use `'*'` (they go to `window.parent`, which
   only the embedder receives, and carry no document content).
5. **`allow="clipboard-write"`** is delegated so the renderer's
   code-copy button works cross-origin.
6. **No strict CSP yet**: user TSX components load as blob-URL module
   imports inside the frame, which a `script-src` without `blob:` would
   break. A CSP for the Pages origin is deferred (see the port plan's
   "Deferred" section).

## Communication protocol

All parent-side handling lives in
`hub-client/src/components/render/q2-sandboxed-preview/Q2SandboxedPreviewIframe.tsx`;
all frame-side handling in `hub-client/quarto-hub-sandboxed-preview/src/`
(`entry.tsx`, `registerServiceWorker.ts`, `serviceWorker.ts`).

### Parent → iframe

| type | payload |
|---|---|
| `UPDATE_AST` | `{ payload: { astJson, currentFilePath, assetManifest, projectFilePaths?, pendingAnchor?, pendingAnchorEpoch?, renderedContent?, untransformedAstJson?, currentActor?, commentsMode?, unlockNestingCursor?, richText?, nestedEditBuffers? } }` — post-pipeline AST (the format maps to the preview `pipeline_kind`) |
| `UPDATE_THEME` | `{ cssText: string \| null, fingerprint }` — compiled theme as **text** (blob URLs are origin-scoped; the frame mints its own, and rewrites relative `url()` refs into the proxy namespace) |
| `LOAD_CUSTOM_COMPONENTS` | `{ componentsCode: Record<path, jsCode> }` |
| `SET_SLIDE` | `{ index }` |
| `SCROLL_TO_LINE` | `{ line }` — the frame does the `data-loc` lookup itself |
| `url_response` | `{ id, path, success, content?, error?, isBinary }` — answer to a VFS proxy request |

### Iframe → parent

| type | payload |
|---|---|
| `IFRAME_READY` | `{}` (posted after the service worker is registered) |
| `AST_RENDERED` | `{}` |
| `NAVIGATE_TO_DOCUMENT` | `{ path, anchor }` |
| `SET_AST` | `{ ast }` (block edits / richtext) |
| `SLIDE_CHANGED` | `{ index }` |
| `PREVIEW_SCROLLED` | `{ ratio }` (preview→editor scroll sync) |
| `CLICK_AT_LINE` | `{ line, iframeY }` (parent adds its iframe rect top for `hostY`) |
| `url` | `{ id, path }` — **the core of the asset path**: VFS proxy request relayed from the service worker |
| `hub-client-save` | `{}` (Cmd+S, from the shared link handlers) |

### Asset proxying (any relative path, bd-00bgt5cy)

**Any in-scope path outside the frame's own app files is served from the
VFS.** The service worker exempts only the page itself, `serviceWorker.js`,
and `q2-preview-assets/*` (the hashed renderer chunks + KaTeX fonts — the
dir is deliberately not `assets/` so a project's own `assets/` folder is
proxied normally); every other same-origin GET is relayed `SW → page → parent → WASM VFS → back`,
correlated by request id with timeouts at each hop. The URL path relative
to the SW scope IS the VFS path.

The parent still resolves AST image targets against `currentFilePath`
(mirroring q2-preview's `assetWalker`) and ships a manifest of
`origPath → <bare resolved path>` page-relative URLs — that's what keeps
`../` and subdirectory images correct, and same-named files in different
directories distinct. Paths the manifest never saw (an `<img>` in raw
HTML or navbar chrome) are proxied too; the parent retries them against
the current document's directory on a VFS miss. Theme CSS relative
`url()` refs are rewritten to absolute page URLs against
`.quarto/project-artifacts`, so theme fonts resolve (they don't in
q2-preview's blob-based `<link>`).

Known limitation, deliberate: project files literally named `index.html`,
`serviceWorker.js`, or under `q2-preview-assets/` at the VFS root are
shadowed by the app-file exemption (they fall through to the network).
The asset dir is named `q2-preview-assets` precisely so no real project
trips over this.

Policy (interception namespace, binary classification, MIME table,
CSS rewriting) is a single module shared by the SW, the page bridge,
and the parent responder:
`quarto-hub-sandboxed-preview/src/assetPolicy.ts`.

## Testing

- Unit/protocol: `hub-client/src/components/render/q2-sandboxed-preview/*.test.ts(x)`
  (policy round-trips, manifest resolution, protocol forwarding, source
  checks) and `scrollClickBridge.test.ts`.
- End-to-end: `hub-client/e2e/q2-sandboxed-preview.spec.ts` — real hub +
  WASM pipeline; asserts themed render, KaTeX, and an image decoded
  through the SW proxy inside the frame.
- local-prod: `npm run local-prod:fresh` (or `:nginx`), open a document
  with `format: q2-sandboxed-preview`.

## Future enhancements

1. Strict CSP on the Pages origin (needs a story for blob-URL module
   imports used by custom components).
2. SW caching/offline (disabled in commit `103af4445`).
3. Version pinning / cache-busting for the Pages deployment.
4. Routing `format: revealjs` through the sandboxed frame (RevealDeck is
   bundled but has no production route today).
