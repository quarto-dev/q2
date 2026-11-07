---
title: "q2-preview theme in an opaque-origin sandbox"
---

## Overview

When a host frames the q2 live preview with `sandbox="allow-scripts"` and no
`allow-same-origin` (for example, a host that serves the preview from its own
origin and isolates untrusted document content this way), the document
renders but its theme never applies.

Cause: `Q2PreviewIframe` reads the compiled theme CSS from the VFS, mints a
`blob:` URL in the parent, and posts the URL to the nested `q2-preview.html`
frame. Under an opaque-origin parent the nested frame is opaque too, with a
different opaque origin, and a `blob:` URL only loads in the origin that minted
it. The AST is posted by value, which is why the content arrives.

Fix: post the CSS **text** (`UPDATE_THEME {cssText, fingerprint}`), and let the
iframe mint its own `blob:` URL for the `<link data-q2-theme>`. That's the
transport hub-client's cross-origin sandboxed preview already uses
(`hub-client/quarto-hub-sandboxed-preview/src/entry.tsx`).

No braid strand: braid isn't set up on this machine, so the user chose a
manual worktree. File the strand (and the follow-ups below) when braid is
available.

## Checklist

### Phase 1: tests (red)

- [x] `Q2PreviewIframe.integration.test.tsx`: the three-way walk posts
  `cssText` (the VFS bytes) and never mints a theme blob URL in the parent
- [x] `entry.integration.test.tsx`: `UPDATE_THEME {cssText}` creates one
  `<link data-q2-theme>` whose `href` the iframe minted; replacing or clearing
  the theme revokes the previous URL
- [x] `entry-slide-theme.integration.test.tsx`: same transport on a slide deck
- [x] Run and confirm the new assertions fail

### Phase 2: implementation (green)

- [x] `Q2PreviewIframe.tsx`: post `cssText`; drop the parent-side theme blob
  URL and its revocation bookkeeping
- [x] `q2-preview/entry.tsx`: mint and revoke the theme blob URL in the iframe
- [x] Update doc comments (`Q2PreviewIframe`, `artifactPaths.ts`,
  `PreviewApp.tsx`)
- [x] preview-renderer, q2-preview-spa, and hub-client tests pass;
  `npm run build:all` in hub-client passes

### Phase 3: end-to-end

- [x] Rebuild the SPA and `q2`, run `q2 preview`, frame it from another origin
  with `sandbox="allow-scripts"`, and check the theme applies
- [x] Record the invocation and observed result below
- [x] Playwright regression spec `q2-preview-spa/e2e/sandboxed-frame-theme.spec.ts`
  (red against the `main` binary, green with the fix)

## Out of scope (follow-ups)

- Images: `buildAssetManifest` also mints `blob:` URLs in the parent, so VFS
  images likely break under an opaque parent the same way.
- Scroll and click sync read the nested frame's `contentDocument`, which is
  `null` when the frame is cross-origin; they silently do nothing.
  hub-client's sandboxed preview already does both over `postMessage`.
- Seen while testing, unrelated to the transport: a second `IFRAME_READY`
  resets `Q2PreviewIframe`\'s theme dedup, but `iframeReady` is already
  `true`, so the theme effect doesn't re-run and nothing is re-posted. The
  old unit test only covered the parent-side revoke on that path, which no
  longer exists, so it was removed.

## End-to-end record

Inspected by hand on 2026-11-05 in headless Chromium (Playwright).

Setup, mirroring a sandboxing host: `q2 preview doc.qmd --no-browser --port
4801`; a small Node proxy on `:4802` that adds `Access-Control-Allow-Origin`
(a host that proxies the preview has to do the same; without it the
opaque-origin SPA can't load its module scripts at all); a host page on
`:4900` with
`<iframe sandbox="allow-scripts" src="http://127.0.0.1:4802/?page=doc.qmd">`.
The "after" run served the rebuilt SPA with `--preview-dir
q2-preview-spa/dist`.

Before (`main` SPA), inside the nested `q2-preview.html` frame:

```text
origin: "null"
themeHref: "blob:null/7fa9b084-…"   themeRuleCount: null
bodyFont: "Times"   --bs-body-font-family: (unset)
console: Not allowed to load local resource: blob:null/7fa9b084-…
```

After (fixed SPA), same page:

```text
origin: "null"
themeHref: "blob:null/56908242-…"   themeRuleCount: 2116
bodyFont: "system-ui, -apple-system, …"   console errors: none
```

Screenshots matched: unstyled Times before; Bootstrap headings, code block
and callout after. Also checked:

- Unsandboxed frame: theme still applies (`blob:http://127.0.0.1:4802/…`,
  2116 rules).
- Live theme swap (`theme: darkly` added while previewing): a new `blob:null`
  URL replaces the old one, still one `<link data-q2-theme>`, 2135 rules,
  no errors. darkly's body colours don't show in the preview, but `main`
  behaves the same unsandboxed, so that's existing behaviour.
