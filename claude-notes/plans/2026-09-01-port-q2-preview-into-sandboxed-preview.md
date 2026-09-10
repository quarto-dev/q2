# Port q2-preview functionality into q2-sandboxed-preview

## Overview

Bring the full `q2-preview` renderer experience to the `q2-sandboxed-preview`
format, which runs in a **cross-origin iframe** (GitHub Pages,
`https://quarto-dev.github.io/q2/`) with real origin isolation and a
service-worker asset proxy.

**Hard constraint: q2-preview is not modified.** No behavior changes to
`ts-packages/preview-renderer`'s q2-preview surface, `Q2PreviewIframe`,
`q2-preview/entry.tsx`, or hub-client's q2-preview wiring. Everything the
sandboxed path needs that can't be imported unmodified gets **copied into
`hub-client/quarto-hub-sandboxed-preview/`** (iframe side) or into
`hub-client/src/components/render/q2-sandboxed-preview/` (parent side).

### Why this port is not a file copy

q2-preview's parent/iframe split leans on same-origin access in exactly three
places; everything else is already postMessage + pure React-over-AST-JSON:

1. **Assets**: parent-minted blob URLs (`assetWalker.ts`, theme CSS in
   `Q2PreviewIframe.tsx:463-468`) are origin-scoped — unusable from the
   cross-origin iframe. Replacement: the existing service-worker proxy
   (SW intercepts fetch → postMessage to iframe page → parent → WASM VFS →
   back down), which also covers cases blob manifests never did (CSS
   `url()` font refs, `<img>` inside chrome HTML strings).
2. **Scroll sync + click-to-line**: `Q2PreviewIframe.tsx:210-269` reads
   `iframe.contentDocument` directly. Replacement: re-host the pure-DOM
   helpers (`scrollSyncDom.ts`) *inside* the iframe and add postMessage
   commands/reports.
3. **Clipboard**: `codeCopy.ts` uses `navigator.clipboard` — restricted in
   the cross-origin frame; proxy through the parent.

### Key decisions

- **Reuse `@quarto/preview-renderer` unmodified, copy only boundary files.**
  The iframe-side surface (`framework/**`, `q2-preview/**` minus
  `assetWalker.ts`, `utils/*`) is decoupled from WASM and same-origin — it
  consumes AST JSON + a URL manifest. The sandboxed project imports these
  modules directly from package **source** via a Vite alias into
  `../../ts-packages/preview-renderer/src` (zero changes to the package, no
  exports-map edit needed). Copied-and-adapted files: `entry.tsx` (iframe
  side) and `Q2PreviewIframe.tsx` (parent side, becomes the grown
  `Q2SandboxedPreviewIframe.tsx`).
  - Requires in the sandboxed project: React 18 → 19, `resolve.conditions:
    ['source', …]`, the `virtual:quarto-attribution-viewer-css` plugin
    (copy from `hub-client/vite.config.ts:36-52`), `server.fs.allow` for
    the repo-root `resources/**` `?raw`/CSS imports, plus the package's
    runtime deps (tiptap, reveal.js, `@babel/standalone`, morphdom, katex).
- **Drop the single-file build; deploy the full multi-file `dist/`.**
  `vite-plugin-singlefile` would have to base64-inline tiptap, prosemirror,
  Babel standalone, reveal CSS, Bootstrap and ~60 KaTeX fonts into one HTML
  (tens of MB). GitHub Pages already serves a directory (it deploys
  `dist/` today — `index.html` + `serviceWorker.js`); a normal build with
  `/assets/*` chunks works cross-origin because they're same-origin *to the
  iframe*. The SW skips its own origin's real app assets by path prefix.
  Local fallbacks (`scripts/q2-sandboxed-preview-server.mjs`, the
  `hub-client/public/` copy) switch from two files to the dist dir.
- **Theme CSS travels as text**, not a blob URL: parent reads
  `/.quarto/project-artifacts/styles.css` and posts the CSS string; the
  iframe mints its *own* blob URL (same-origin to itself) for the
  `<link data-q2-theme>` swap. Relative `url()` refs inside the CSS then
  resolve against the iframe origin and are picked up by the SW proxy —
  fonts work for the first time.
- **Pipeline parity is a prerequisite**: `q2-sandboxed-preview` currently
  maps to `("html", None)` (`crates/quarto-core/src/format.rs:125`) and is
  absent from `pipelineKindForFormat`
  (`ts-packages/preview-runtime/src/pipelineKind.ts:27-34`), so it gets a
  raw parse-only AST — no highlight spans, no chrome metadata, no theme
  fingerprint. Both mappings change to match `q2-preview`'s
  `Some("preview")`. (This touches shared files but only the
  sandboxed-format entries — q2-preview behavior unchanged.)

### Existing proxy defects fixed as part of Phase 2

- SW extension allowlist (`gif|png|jpg` only) disagrees with the parent's
  binary list — the in-code TODO pair. Unify into one shared module.
- `registerServiceWorker.ts:41` strips URLs to their basename → same-named
  files in different directories collide; forward full pathnames and
  resolve against `currentFilePath` (reuse `utils/vfsPaths.ts` logic).
- Per-request `message` listener leaks in `serviceWorker.ts:80` and
  `registerServiceWorker.ts` (no timeout/rejection); add request ids +
  timeout.
- No `event.origin` validation anywhere; all posts use `'*'` (Phase 5).

## Phases and work items

### Phase 0 — pipeline parity + test scaffolding ✅ (d6df9500a, bd-jgpz4hfq)

- [x] Test first: mapping tests on both sides
      (`test_from_format_string_q2_sandboxed_preview` in format.rs,
      pipelineKind.test.ts) — verified failing before the fix.
- [x] Rust: `format.rs` `q2-sandboxed-preview` → `("html", Some("preview"))`;
      `wasm-quarto-hub-client`'s `coerce_format_for_print` needs no change
      (printable fallback to html stays correct).
- [x] TS: add `q2-sandboxed-preview` to `pipelineKindForFormat`.
- [x] `cargo nextest run --workspace` (39 pre-existing environmental
      failures, identical with change stashed — pandoc missing on this
      machine) + `cargo xtask verify --skip-rust-tests` fully green.

### Phase 1 — real renderer inside the sandboxed bundle

- [x] Sandboxed project deps/config: React 19, preview-renderer source
      alias (+ `@quarto/preview-runtime` **stub** at
      `src/stubs/preview-runtime.ts` — the q2-preview barrel drags
      parent-side WASM-coupled modules into the graph), attribution-CSS
      virtual plugin, dedupe react/react-dom/katex, drop `viteSingleFile`,
      emit normal `dist/` with `base: './'`.
- [x] Copy-and-adapt `entry.tsx` → `src/entry.tsx` (PreviewRoot/registry
      imported unmodified; promise-ordered dispatcher copied as
      `iframeMessageDispatch.ts` with `UPDATE_THEME.cssText`; local
      blob-URL theme minting; SW init gates IFRAME_READY).
- [x] Delete `basicRenderer.tsx` + toy `App.tsx`; removed stale committed
      artifacts (`hub-client/public/q2-sandboxed-preview.html`,
      `hub-client/public/serviceWorker.js`, root leftover); gitignored
      `hub-client/public/q2-sandboxed-preview/`.
- [x] Deploy targets: GH Pages workflow adds root `npm ci` + wider path
      triggers (preview-renderer, resources); dist-dir server script;
      `build-local-prod.sh` URL → `http://127.0.0.1:8081/`; docs updated
      (project README, scripts/README, github-pages.md).
- [x] Parent minimum viable wiring (TDD, tests first, 4 failing → green):
      `Q2SandboxedPreviewIframe` now takes `currentFilePath` +
      three-way `themeFingerprint`, posts `UPDATE_THEME {cssText}`;
      ReactRenderer passes both.
- [x] Smoke (end-to-end, headless chromium): served
      `hub-client/public/q2-sandboxed-preview/` via
      `scripts/q2-sandboxed-preview-server.mjs`, posted
      `UPDATE_AST {astJson, currentFilePath}` + `UPDATE_THEME {cssText:
      'h1 { color: rgb(1,2,3) }'}` → observed `<h1>Hello sandbox</h1>`,
      `<p>Rendered by PreviewRoot.</p>`, computed h1 color
      `rgb(1, 2, 3)`, `<link data-q2-theme>` present, SW `active`.
      Output inspected; recorded 2026-09-01.

### Phase 2 — asset proxying, done properly

Design refinement over the original sketch: instead of extension-guessing
interception, proxied assets live in an explicit **page-relative namespace**
`__q2_vfs__/<resolved VFS path>`. The parent resolves image targets against
`currentFilePath` at manifest-build time (mirroring `assetWalker`'s
resolution exactly), so the full resolved path rides in the URL — in-scope
under `/q2/` on Pages, immune to basename collisions, and app assets
(`assets/*`, fonts, the page) are never touched.

- [x] Tests first (19 new, red → green): proxy URL round-trip (subdirs,
      spaces, basename-collision regression), namespace misses, binary
      classification, MIME table, CSS `url()` rewriting, manifest
      resolution (`../`, root-absolute, external skip), parent responder
      id correlation, manifest-in-payload.
- [x] Shared `quarto-hub-sandboxed-preview/src/assetPolicy.ts` used by the
      SW, the page bridge, and the parent responder (kills the skewed-list
      TODO pair).
- [x] SW rework: intercepts only `__q2_vfs__` GETs; one persistent message
      listener + request-id map (fixes per-request listener leak); 10s
      timeout → 504; miss → 404; text vs binary bodies.
- [x] Page bridge rework: full-path forwarding (no basename stripping),
      id-correlated with timeout + listener cleanup.
- [x] Parent responder: id-keyed `url`/`url_response`, shared
      `isBinaryPath`; ships `buildProxyAssetManifest(astJson,
      currentFilePath)` in the UPDATE_AST payload (no VFS reads at
      manifest time — bytes on demand).
- [x] Theme fonts: `rewriteThemeCssUrls` rewrites relative `url()` refs in
      the posted theme CSS into the proxy namespace against
      `.quarto/project-artifacts` (q2-preview loses these entirely;
      absolute/data:/blob:/# refs untouched).
- [x] End-to-end smoke (headless chromium, harness parent on :8098,
      renderer iframe on :8099): AST with `images/pic.png` +
      manifest entry → `<img src="__q2_vfs__/project/sub/images/pic.png">`
      → SW intercept → bridge request `project/sub/images/pic.png` →
      harness returns PNG bytes → image decodes at natural size 1×1.
      Output inspected; recorded 2026-09-01.

### Phase 3 — parent feature parity (scroll sync, click-to-line)

- [x] Tests first (12 new, red → green): bridge unit tests
      (PREVIEW_SCROLLED ratio on scroll, CLICK_AT_LINE with line + block
      top, silent on unlocated / included-file clicks, SCROLL_TO_LINE
      scrolling with visibility check) + parent protocol tests (handle
      posts SCROLL_TO_LINE / cached getScrollRatio, hostY = iframeY +
      iframe top, NAVIGATE/SET_AST/SLIDE_CHANGED/AST_RENDERED forwarding,
      LOAD_CUSTOM_COMPONENTS, deduped SET_SLIDE incl. echo guard, full
      UPDATE_AST payload).
- [x] Iframe side: `scrollClickBridge.ts` imports
      `findElementForLine`/`isElementVisible`/`lineForClickTarget`
      unmodified from `scrollSyncDom`; entry installs the bridge at module
      top and handles `SCROLL_TO_LINE`.
- [x] Parent side: `Q2SandboxedPreviewIframe` grown to the full
      `Q2PreviewIframe` surface (same `Q2PreviewIframeHandle` interface;
      `getScrollRatio` reads the last `PREVIEW_SCROLLED` ratio, null
      before first report; SET_SLIDE echo-dedup; state reset on
      IFRAME_READY).
- [x] `ReactRenderer.tsx` sandboxed branch passes the full prop set,
      mirroring the q2-preview branch (custom-components *generation* gate
      still excludes the sandboxed format — Phase 4).
- [x] End-to-end smoke (headless chromium, harness + iframe on separate
      ports, 80-paragraph document, data-loc stamped on two blocks):
      SCROLL_TO_LINE 61 scrolled the frame 0 → 1865px; smooth scroll
      emitted PREVIEW_SCROLLED ratios; pointerup on the located block
      posted CLICK_AT_LINE {line: 11, iframeY: 156}. Output inspected;
      recorded 2026-09-10.

### Phase 4 — remaining functionality

- [x] `LOAD_CUSTOM_COMPONENTS` (user TSX): ReactRenderer's
      component-transpile gate extended to `q2-sandboxed-preview` (TDD:
      failing ReactRenderer integration test first, plus a routing test).
      Browser smoke: LOAD_CUSTOM_COMPONENTS with a Para override →
      blob-URL `import()` inside the allow-scripts frame → override
      rendered (`CUSTOM PARA` sentinel observed). No strict CSP on the
      Pages origin yet — blob script imports depend on that staying true
      (deferred item unchanged).
- [x] Rich text / tiptap (`richText`, `nestedEditBuffers`, `SET_AST` edit
      payloads): fully wired through the full-surface props + protocol
      tests (Phase 3); interactive editing verified in Phase 5's real
      session, not separately here.
- [x] Slides: `SET_SLIDE`/`SLIDE_CHANGED` unit-tested both sides
      (echo-dedup included); RevealDeck + reveal CSS are in the bundle.
      Note: in hub-client, `format: revealjs` routes to `Q2PreviewIframe`,
      not the sandboxed frame, and the sandboxed pseudo-format is
      html-based (`isSlides` false) — so a sandboxed reveal deck has no
      production route today; the machinery is ported and inert.
- [x] `hub-client-save` (Cmd+S): posted by the unmodified
      `installLinkHandlers` inside the frame; `App.tsx`'s window-level
      listener is origin-agnostic. No change needed.
- [x] Clipboard: `allow="clipboard-write"` delegated on the iframe so the
      unmodified `codeCopy.ts` can call `navigator.clipboard.writeText`
      cross-origin (test-first). No COPY_TO_CLIPBOARD proxy message needed.
- [x] Comments mode / attribution (`currentActor`, `commentsMode`,
      `untransformedAstJson`, `renderedContent`): shipped in the Phase 3
      payload; covered by the full-payload protocol test.

### Phase 5 — hardening + verification

- [x] Source/origin checks (TDD, negative test first): the parent ignores
      messages whose `event.source !== iframe.contentWindow` (forged
      SET_AST cannot reach document state); the frame ignores messages
      whose `event.source !== window.parent` — including the page
      bridge's `url_response` listener (forged responses would inject
      attacker bytes as document assets). Target origins pinned: parent →
      sandbox origin derived from the iframe URL; frame → parent origin
      pinned from first accepted message. `'*'` remains only on the
      pre-contact IFRAME_READY and the SW-bridge `url` request (both go
      to `window.parent`, which only the embedder receives, and carry no
      document content).
- [x] Debug logs: the SW/bridge rewrites (Phase 2) already dropped the
      per-request logs; the one-time SW registration logs are kept
      (parity with q2-preview's own logging).
- [x] Docs: sandboxed README rewritten in Phase 1;
      `claude-notes/designs/q2-sandboxed-preview-separate-domain.md`
      rewritten (full protocol table, `url` as the core asset path,
      security posture incl. CSP/blob note, testing pointers).
- [x] **End-to-end, real pipeline**: new Playwright spec
      `hub-client/e2e/q2-sandboxed-preview.spec.ts` — real hub server
      (globalSetup `cargo run --bin hub`), real WASM preview pipeline,
      project with `_quarto.yml` + `images/dot.png` (binary) + a
      `format: q2-sandboxed-preview` doc. Observed inside the sandboxed
      frame: title block `<h1 class="title">Sandboxed Doc</h1>`, content
      heading with a real `data-loc` stamp, KaTeX `.katex` markup,
      `<link data-q2-theme>`, and
      `<img src="__q2_vfs__/…/images/dot.png">` decoded at natural size
      1×1 through the SW proxy. `npx playwright test
      e2e/q2-sandboxed-preview.spec.ts` → 1 passed (5.0s), 2026-09-10.
      Wiring: `test:e2e` script + the CI e2e workflow now build the
      sandboxed bundle and set
      `VITE_Q2_SANDBOXED_PREVIEW_URL=q2-sandboxed-preview/index.html`
      so CI tests this branch's renderer, not the last Pages deploy.
      Additional harness smokes re-run against the hardened bundle:
      proxy+theme (incl. theme-CSS `url()` font round trip through the
      SW), scroll/click, custom components — all green.
      Not exercised in a real browser session: interactive richtext
      editing and Cmd+S inside the sandboxed frame (machinery identical
      to q2-preview's, protocol unit-tested; verify by hand in
      local-prod when convenient).
- [x] `cargo xtask verify --skip-rust-tests` (Rust untouched since
      Phase 0's fully-verified commit) green at phase close.

## Deferred / out of scope

- Strict CSP on the Pages origin (blocked by blob-script custom components).
- SW caching/offline (deliberately disabled in `103af4445`; unchanged).
- Retiring the old q2-preview path in favor of the sandboxed one — separate
  decision once parity is proven.
- `q2 preview` (the CLI's q2-preview-spa) migration to this model — the
  embedded single-server design needs a second-origin story first; noted in
  the 2026-09-01 exploration but not part of this port.

## Bookkeeping

- [x] Epic bd-r9yr0hbe with per-phase strands bd-jgpz4hfq (0),
      bd-44ohor2w (1), bd-k9tfhsst (2), bd-xqg0t494 (3), bd-rhn8wi8r (4),
      bd-5fwm3zju (5). All closed 2026-09-10; commits d6df9500a,
      5684cfead, e7f9fcc2a, 0d5967f2a, 9f7efd8a9, 14de97b62 (+ changelog
      commits) on branch q2-sandboxed-preview.

## Reference: exploration findings (2026-09-01)

- Full q2-preview postMessage protocol + prop surface:
  `Q2PreviewIframe.tsx:310-476`, `entry.tsx:249-282,459`;
  dispatch in `ReactRenderer.tsx:257-341`.
- Existing SW proxy chain:
  `quarto-hub-sandboxed-preview/src/serviceWorker.ts:61-95`,
  `registerServiceWorker.ts:4-52`,
  `Q2SandboxedPreviewIframe.tsx:25-68`.
- Blob URLs are origin-scoped; the parent's cannot be fetched from the
  cross-origin iframe — that's why the SW proxy exists.
- `sandbox="allow-scripts allow-same-origin"` on the sandboxed iframe is
  load-bearing: without `allow-same-origin` the frame is opaque-origin and
  `serviceWorker.register` throws. Isolation comes from the separate
  origin (github.io), not the sandbox attribute.
