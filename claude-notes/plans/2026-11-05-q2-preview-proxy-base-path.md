---
title: 'q2 preview: work behind a path-prefix proxy'
date: 2026-11-05
description: 'Make the live viewer and `--static` preview work when a proxy serves them under a stripped path prefix such as `/p/<token>/`, with no flags.'
status: in-progress
---

**Branch:** `preview-proxy-base-path`, pushed as `seeM:feature/preview-proxy-base-path` (based on `main` @ `a177a07a4`)

## Overview

Posit Assistant's Canvas shows report previews in an iframe at
`/p/<token>/...` on its own server, and strips the prefix before
forwarding to the preview's loopback port. Posit Workbench, JupyterHub
(`/user/<name>/proxy/<port>/`) and code-server (`/proxy/<port>/`) do the
same. Quarto 1's preview already works this way; `q2 preview` does not,
in either mode, because both build URLs from the server root, so behind
a prefix those requests reach the proxy's root instead of q2.

The prefix is not known when `q2 preview` starts (Canvas creates the
capability after the URL is printed), so a `--base-path` flag does not
help. The fix is the "relative URLs, resolved in the browser" shape:
every runtime request is resolved against the page's own URL, and every
URL the server emits is relative.

Scope: the default viewer UI (with or without `--allow-edit`) and
`--static`. Out of scope: `--ui editor` (hub-client, which has its own
build-time `VITE_HUB_BASE_PATH`), `--join` and `--share`.

## What breaks today

Live viewer (`q2-preview-spa`):

- `wsUrlFromLocation` in `PreviewApp.tsx` builds `${host}/ws`.
- The boot health probe builds `${host}/health`.
- `previewConfig.ts` fetches `${host}/api/preview/config`.
- `PreviewApp.tsx` fetches `/api/preview/deps` and `/api/preview/diagnostics`.
- `StaleCaptureOverlay.tsx` posts `/api/preview/re-execute`.
- `PdfPane.tsx` posts `PDF_ROUTE` (`/api/preview/pdf`).

The native render and import routes in `hub-client/src/pandoc/` already
resolve against `document.baseURI` and need nothing.

`--static` (`crates/quarto-preview/src/static_mode/`):

- `client.js` opens `EventSource("/__q2-preview/events")`.
- `reload` events carry a root-absolute `target` (`/s.html`), and the
  client navigates to it.
- `client.js` compares `location.pathname` against the absolute
  `VIEWER_PATH` and the `target`.
- The server sends root-absolute `Location` headers: `/` to the
  default file, the directory slash redirect, and the PDF to
  `viewer.html?file=/x.pdf` redirect.
- The no-`index.html` listing links pages as `/page.html`.
- The pdf.js viewer page loads `<script src="/__q2-preview/client.js">`.

## Design

**Live viewer.** One helper, `serverUrl(route)`, resolves a route
against `document.baseURI` (the directory of the SPA page, since vite
builds with `base: './'`). WebSocket URLs are derived from it by swapping
the scheme. This matches what `nativeRender.ts` already does.

**Static.** The server knows the site-relative path of every page it
serves, so at injection time it writes the relative way back to the
site root (`./`, `../`, `../../`, …) into the script tag as
`data-q2-root`. The client resolves `new URL(root, location.href)` once
to get the site root URL, then:

- opens `EventSource` on `<root>__q2-preview/events`;
- maps a root-absolute `target` to `<root><target without leading />`;
- compares paths relative to the site root, so the viewer check and the
  "same page, just reload" check still work under a prefix.

The pdf.js viewer page lives at a fixed depth, so its script tag gets a
fixed relative `src` and `data-q2-root`. This needs no proxy-specific
regex, unlike Quarto 1, and works for any stripping proxy.

Every `Location` the static server sends becomes relative to the
request URL; the listing's hrefs become relative to `/`.

A request URL without its trailing slash (`/p/abc123` rather than
`/p/abc123/`) can't resolve relative URLs correctly in either mode. The
SPA's own assets already have this limitation, and proxies redirect to
the slash form, so it is not handled here.

## Checklist

### Phase 1: tests first

- [x] Static, Rust integration tests (`tests/integration/static_mode.rs`):
  the injected script carries `data-q2-root` matching the request depth
  (`/`, `/a.html`, `/a/`, `/a/b.html`, a 404 under `/x/y/`); every
  redirect `Location` is relative and resolves to the old absolute target
  against the request URL; listing hrefs are relative; the viewer page's
  script tag is relative.
- [x] Static, client behavior: a test that runs `client.js` against a
  page under `/p/abc123/` and checks the `EventSource` URL and the
  navigation target for a `reload` event
  (`q2-preview-spa/src/staticModeClient.test.ts`).
- [x] Live viewer, vitest: `serverUrl` and `serverWsUrl` resolve
  under a prefixed base; a source scan fails on any `location.host` or
  `fetch('/...')` left in the SPA (`q2-preview-spa/src/serverUrl.test.ts`).
- [x] Run them and confirm they fail (11 static Rust tests, 5 client
  tests, and the missing `serverUrl` module).

### Phase 2: static

- [x] Server: compute the root for each served HTML page and inject it;
  relative `Location` headers; relative listing hrefs; relative viewer
  script tag.
- [x] `client.js`: resolve events, targets and the viewer check against
  the injected root.
- [x] Keep the test that pins the client's literal endpoint in sync.

### Phase 3: live viewer

- [x] Add `serverUrl` (`q2-preview-spa/src/serverUrl.ts`) and route every call site listed above through it.

### Phase 4: verify

- [x] `cargo nextest run -p quarto-preview`, clippy, `npm run test` and
  the build for `q2-preview-spa`.
- \[\~\] Windows cross-check
  (`cargo clippy -p quarto-preview --all-targets --target x86_64-pc-windows-msvc`):
  can't run from macOS, since `ring` and `aws-lc-sys` compile C for the target. The
  change adds no platform-specific code.
- [x] Rebuild the WASM and SPA embed chain and `q2`.
- [x] Document it for users: a "Behind a proxy" section in
  `docs/guides/projects/preview.qmd`.
- [x] End to end: a small path-stripping proxy at `/p/abc123/` in front
  of both modes; page renders, edits re-render, and every request in the
  network panel stays under the prefix. Record the invocation and
  observations below.

## End-to-end record

Run on 2026-11-05 with `target/debug/q2` built after
`npm run build:wasm` and `cargo xtask build-q2-preview-spa`, against a
two-page website (`doc.qmd`, `other.qmd`). The proxy is a ~70-line Node
script that serves `http://127.0.0.1:<port>/p/abc123/...` from the
preview with the prefix stripped, forwards WebSocket upgrades, and
answers anything outside the prefix with a 404. Headless Chromium
(Playwright) loaded the page, edited the source, waited for the new
text, and recorded every request. Output inspected.

`q2 preview --static --no-browser --port 4711 .` behind the proxy at
`/p/abc123/`:

```text
load http://127.0.0.1:8091/p/abc123/ ... ok
shows "First version." ... ok
edit doc.qmd -> "Second version." ... ok
edit other.qmd -> browser follows ... ok
final URL: http://127.0.0.1:8091/p/abc123/other.html
31 requests, 0 outside /p/abc123/
  http://127.0.0.1:8091/p/abc123/
  http://127.0.0.1:8091/p/abc123/doc.html
  http://127.0.0.1:8091/p/abc123/__q2-preview/events
  http://127.0.0.1:8091/p/abc123/other.html
```

`/p/abc123/` answered `302` with `location: ./doc.html`, and the page
carried `<script data-q2-root="./">`.

`q2 preview --no-browser --port 4712 .` (live viewer) behind the proxy:

```text
load http://127.0.0.1:8092/p/abc123/?page=doc.qmd ... ok
shows "First version." ... ok
edit doc.qmd -> "Second version." ... ok
23 requests, 0 outside /p/abc123/
  http://127.0.0.1:8092/p/abc123/health
  http://127.0.0.1:8092/p/abc123/api/preview/config
  ws://127.0.0.1:8092/p/abc123/ws
  http://127.0.0.1:8092/p/abc123/api/preview/deps?page=doc.qmd
  http://127.0.0.1:8092/p/abc123/api/preview/diagnostics?page=doc.qmd
  http://127.0.0.1:8092/p/abc123/q2-preview.html
```
