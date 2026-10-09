---
title: 'bd-ql55q — Preview navbar brand link points to artifacts VFS root'
date: 2026-05-20
description: 'Makes the navbar brand link in `q2 preview` resolve to the project home page instead of a bare artifacts directory URL that the preview iframe cannot route, so clicking it stays inside the app.'
---

**Issue:** bd-ql55q (child of bd-lk66 — Hub-client website rendering UX issues)
**Type:** bug · **Priority:** 1

## Summary

In `q2 preview` (and the hub-client preview iframe), the navbar brand
anchor — the website title rendered at the top-left — links to the VFS
artifacts directory:

```html
<a class="navbar-brand" href="/.quarto/project-artifacts/">Quarto 2</a>
```

Clicking it navigates the preview iframe to that URL. The iframe's link
handler does not recognize this pattern (`reverseMapArtifactHref` requires
a `.html` suffix), so the click escapes the SPA and dead-ends. The native
`q2 render` is unaffected: it emits `href="./"`, which the browser
resolves to `index.html` via the static-file server.

## Reproduction

1. `cd docs/` (the docs-site skeleton).
2. `q2 preview`, navigate the iframe to the index page.
3. Inspect the link wrapping the navbar title element.

Native render comparison (for evidence):

```bash
cd docs && q2 render
grep "navbar-brand" _site/index.html
# <a class="navbar-brand" href="./">Quarto 2</a>      ← correct
```

The bug is preview-only. The native HTML is correct.

**Live repro captured 2026-05-20** (chrome-devtools MCP against
`q2 preview docs/`):

- The brand element renders as
  `<a class="navbar-brand" href="/.quarto/project-artifacts/">Quarto 2</a>`.
- Clicking it: `beforeunload` fires on the iframe; iframe location
  changes from `/q2-preview.html` to `/.quarto/project-artifacts/`;
  **no `NAVIGATE_TO_DOCUMENT` postMessage fires** (confirming the SPA
  link handler did not intercept). The iframe is now stranded on a URL
  the preview server does not serve.
- Note: the same gate also breaks `nav-link` items pointing to
  `/.quarto/project-artifacts/index.html` and
  `/.quarto/project-artifacts/about.html` in the *postProcessor*
  surface — but those *do* match `.html` and *would* be intercepted
  by `installLinkHandlers::parseArtifactHref` on click. The bare
  trailing-slash directory URL is the unique escape.

## Root cause

Two places line up to produce the bug:

1. **Rust side — `crates/quarto-core/src/resource_resolver.rs`.**
   `ResourceResolverContext::page_url_for_site_root_dir()` returns
   `"/.quarto/project-artifacts/"` in VFS-root mode (the mode used by
   hub-client / preview). The contract in the doc comment is *"Always
   ends with `/`, so HTML attributes can use it as a directory href
   that the browser resolves against the host's index document."* The
   existing test `page_url_for_site_root_dir_vfs_root_mode`
   (resource_resolver.rs:592) pins this output.

2. **Rust side — `crates/quarto-core/src/transforms/navbar_render.rs:114-118`.**
   `NavbarRenderTransform` uses that URL as the brand-anchor fallback
   when no explicit `logo-href` is set:

   ```rust
   let home_url = ctx
       .resource_resolver
       .as_ref()
       .map(|r| r.page_url_for_site_root_dir())
       .unwrap_or_else(|| "./".to_string());
   ```

3. **TS side — two parallel link handlers, same gate.**
   - **`ts-packages/preview-renderer/src/utils/iframeLinkHandlers.ts::parseArtifactHref`**
     (lines 160-171) — used by the **q2-preview** SPA's React-rendered
     iframe (event-delegated body click). This is the path that
     actually runs in `q2 preview docs/`.
   - **`ts-packages/preview-renderer/src/utils/iframePostProcessor.ts::reverseMapArtifactHref`**
     (lines 84-100) — used by hub-client's one-shot HTML
     iframe (per-element click listeners attached at post-process
     time).

   Both helpers reject hrefs whose stem doesn't end in `.html`:

   ```ts
   if (!stem.endsWith('.html')) return null;
   ```

   A bare directory URL (`/.quarto/project-artifacts/`) fails the
   suffix check, the click handler returns early, `preventDefault` is
   not called, and the browser performs native navigation away from
   the SPA iframe.

   The constant `ARTIFACT_ROOT = '/.quarto/project-artifacts/'` is
   duplicated across both files (bd-msp0 tracks the future hoist).

The Rust convention is correct *for a static-server world.* In the
preview iframe there is no HTTP server resolving a directory URL to
`index.html`; the iframe is a sandboxed surface driven by the SPA
parent. The TS link handler is where the convention needs to be
honored.

## Fix approach (TS side, per user direction)

Teach **both** iframe link helpers to recognize the trailing-slash
form of an artifact-rooted URL as "the project home" and route it
through the same intercept path as ordinary cross-doc links.

**Edit 1 — `iframeLinkHandlers.ts::parseArtifactHref`** (the path that
runs in q2-preview's React iframe). When `stem === ''` (i.e. the href
is exactly `ARTIFACT_ROOT` ± `#anchor`), return
`{ qmdCandidate: 'index.qmd', anchor }` directly. This matches the
"directory URL = index" web convention; if `index.qmd` doesn't exist
in the project, `PreviewApp`\'s render attempt surfaces the existing
missing-page error overlay — consistent with the docstring policy
*"always intercept artifact-rooted hrefs"* on lines 16-20.

**Edit 2 — `iframePostProcessor.ts::reverseMapArtifactHref`** (the
hub-client HTML iframe path). When the stem is empty, try each
`RENDERABLE_EXTS` prefix `'index' + ext` and return the first match
in `projectFilePaths` (or `null` if none match — the existing strict
policy on this surface is preserved).

The behavioral asymmetry between the two surfaces (always-intercept
vs strict-list) is preserved on purpose; the docstrings already pin
that difference (`iframeLinkHandlers.ts` lines 51-60). The new code
in each helper follows its surface's existing policy.

Resolver semantics in `quarto-core` stay unchanged. This keeps the
"directory URL = index" convention consistent with the rest of the
web platform and only adapts the SPA-specific interception layer.

## Test plan (TDD — tests first, then fix)

### Unit tests for `iframeLinkHandlers.ts` (q2-preview surface)

Add to
`ts-packages/preview-renderer/src/utils/iframeLinkHandlers.integration.test.ts`.

Because `parseArtifactHref` is not exported, the tests exercise it via
`installLinkHandlers` + a synthetic click on a fixture document
(matching the pattern already used in that file).

- [ ] **L-A:** `<a href="/.quarto/project-artifacts/">` triggers
  `onQmdLinkClick({ path: 'index.qmd', anchor: null })`; default is
  prevented.
- [ ] **L-B:** `<a href="/.quarto/project-artifacts/#intro">` triggers
  `onQmdLinkClick({ path: 'index.qmd', anchor: 'intro' })`.
- [ ] **L-C:** Existing behavior preserved — `<a href="/.quarto/project-artifacts/about.html">`
  still triggers `onQmdLinkClick({ path: 'about.qmd', anchor: null })`.
- [ ] **L-D:** Non-artifact href (`<a href="other.qmd">`) routes through
  the existing `.qmd`-suffix branch (regression sanity).

### Unit tests for `iframePostProcessor.ts` (hub-client HTML surface)

Add to
`ts-packages/preview-renderer/src/utils/iframePostProcessor.test.ts`
(this file already exists and unit-tests `reverseMapArtifactHref`
directly):

- [ ] **P-A:** `reverseMapArtifactHref('/.quarto/project-artifacts/', ['index.qmd', 'about.qmd'])`
  returns `{ path: 'index.qmd', anchor: null }`.
- [ ] **P-B:** `reverseMapArtifactHref('/.quarto/project-artifacts/#intro', ['index.qmd'])`
  returns `{ path: 'index.qmd', anchor: 'intro' }`.
- [ ] **P-C:** `reverseMapArtifactHref('/.quarto/project-artifacts/', ['about.qmd'])`
  returns `null` — no `index.qmd` in the project, leave the click
  alone (strict policy on this surface).
- [ ] **P-D:** Existing behavior preserved — `reverseMapArtifactHref('/.quarto/project-artifacts/about.html', ['index.qmd', 'about.qmd'])`
  still returns `{ path: 'about.qmd', anchor: null }`.

Run with:

```bash
cd hub-client && npm run test -- iframeLinkHandlers
cd hub-client && npm run test -- iframePostProcessor
```

### End-to-end verification

Per CLAUDE.md, tests-pass is necessary but not sufficient. Before
closing the issue, reproduce the *2026-05-20 live repro* signature in
the inverse:

1. Run `q2 preview` against `docs/` (the skeleton site).
2. Open the index page in the preview iframe.
3. Use the chrome-devtools MCP to inspect the navbar brand anchor's
   `href`. It should still read `/.quarto/project-artifacts/` (the
   resolver didn't change).
4. Click the brand. **Expected after fix:**
   - `beforeunload` does **not** fire on the iframe.
   - The iframe location stays at `/q2-preview.html`.
   - A `NAVIGATE_TO_DOCUMENT` postMessage fires with
     `{ path: 'index.qmd', anchor: null }`.
   - The preview app re-renders the index page (no actual surface
     change since we were already on it — repeat from `about.qmd` for
     a visible change).
5. Record the captured inspection + click flow in the issue's close
   note.

## Work items

- [x] Reproduce the bug deterministically — DONE (live repro
      captured 2026-05-20 via chrome-devtools MCP; bug behavior
      matches the code-reading prediction).
- [x] Write failing tests L-A, L-B (iframeLinkHandlers integration)
      and P-A, P-B, P-C (iframePostProcessor unit). L-C, L-D and P-D
      were pre-existing.
- [x] Run tests, confirmed all 4 new tests fail with the expected
      "no callback fire" / "null" messages.
- [x] Implement the `parseArtifactHref` empty-stem branch in
      `iframeLinkHandlers.ts` — always-intercept policy (no
      projectFilePaths consultation).
- [x] Implement the `reverseMapArtifactHref` empty/null-stem branch
      in `iframePostProcessor.ts` — strict policy (returns null if
      no `index.<ext>` is in projectFilePaths).
- [x] Run unit tests — all green (155 integration + 181 unit in
      preview-renderer).
- [x] Run `npm run build:all` from `hub-client/` — clean.
- [x] End-to-end verify against `docs/`:
      - Brand href still renders `/.quarto/project-artifacts/` (resolver
        unchanged, as designed).
      - Click from `about.qmd`: `defaultPrevented: true`, no
        `beforeunload`, iframe stays on `/q2-preview.html`,
        `NAVIGATE_TO_DOCUMENT` fires with
        `{ path: 'index.qmd', anchor: null }`, SPA route updates to
        `?page=index.qmd`, visible heading after click is "Quarto 2"
        (the index title). All six bullets exactly invert the
        2026-05-20 repro signature.
- [ ] Close bd-ql55q with the e2e evidence captured in the close reason.

## Out of scope

- No change to `page_url_for_site_root_dir()` or `NavbarRenderTransform`.
  The Rust output is correct for the static-server world; the
  preview-only adaptation is the right scope here.
- No change to the sidebar / footer brand fallback — they reuse the
  same resolver call, but the iframe-side fix is centralized in
  `reverseMapArtifactHref`, so all three navigation surfaces inherit
  the fix automatically.
- A future `q2 render`-served preview (i.e. a real HTTP serve of the
  rendered site) would not need this adaptation at all; the directory
  URL would naturally resolve to `index.html`. The fix is specifically
  for the WASM-driven sandboxed-iframe preview architecture.

## Related

- bd-lk66 (parent epic): Hub-client website rendering UX issues.
- bd-lnd3: introduced the artifact-rooted `.html` reverse-mapping that
  this plan extends.
- bd-jgeu: introduced the `page_url_for_site_root_dir()` brand fallback.
