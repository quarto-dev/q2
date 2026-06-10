# Preview: resolve embedded static-asset iframes from the VFS source path

**Strand:** bd-kjrpya2d (discovered-from bd-z1smhvuo)
**Date:** 2026-06-09
**Status:** DESIGN — approach chosen with user; ready to implement on go-ahead.
**Builds on:** the embed feature (`2026-06-09-website-example-iframe-embed.md`,
Phases 1–2) and crossref (`2026-06-09-crossreferenceable-examples.md`).

## Goal

Make `.embed-example-iframe` iframes resolve under **`q2 preview`** *and* in
real **hub-client** (Automerge/VFS-backed) projects — the VFS-native way.

`q2 render` already works (the resource is copied to `_site/examples/` via
`project.resources`, and the iframe `src` is page-relative). Preview is the gap.

## Why this, and why not the alternatives

In preview the page is rendered **in-browser via WASM**; there is no disk server
for the rendered page. The iframe `src` is the artifact-prefixed
`/.quarto/project-artifacts/examples/…/slides.html` (what `page_url_for`
produces in `vfs_root` mode). The TS **iframe post-processor**
(`ts-packages/preview-renderer/src/utils/iframePostProcessor.ts`) already
"Replaces `/.quarto/` resource links with data URIs from VFS" — it reads the
artifact path from the VFS via `vfsReadFile`. But the staged deck lives at its
VFS **source** path (`examples/…/slides.html`), **not** the rendered-output
artifact path — so the read misses and the iframe breaks. The copy that *would*
place it at the artifact path (`copy_resources_to_output_dir`) is
`#[cfg(not(target_arch = "wasm32"))]` — native-only.

Rejected alternatives:

- **Disk HTTP route (CLI-only).** Prototyped (a `project_or_spa_handler` in
  `quarto-preview` + a `vfs_root`-mode resolver branch emitting `/examples/…`)
  and **reverted**. It worked for `q2 preview docs/` but is fundamentally
  disk-bound: a real hub-client project populates `/examples/` only in Automerge
  (paste-in / programmatic doc edits), with no disk and no native server — so
  the iframe could never resolve there. CLI-only shortcut.
- **WASM resource-copy to the artifact path (option A).** Port the native
  resource copy into the WASM render so `resources:` assets land at
  `/.quarto/project-artifacts/…`. Rejected: the artifact flush
  (`wasm-quarto-hub-client/src/lib.rs:1422`) runs **per render**, so every
  edit-triggered re-render would re-copy the full `resources:` set (~6 MB of
  decks) into the VFS artifact tree — and in a hub-client project that tree is
  **inside the Automerge document**, bloating the doc and its sync traffic with
  duplicated copies. Confirmed with the user; this is the cost we're avoiding.

## Chosen design (option B): post-processor source-path fallback

When the iframe/asset post-processor encounters
`src="/.quarto/project-artifacts/X"` and the **artifact path misses** in the
VFS, strip the `ARTIFACT_ROOT` prefix and read **`X` from the VFS source path**.
No copy, no Automerge duplication — the bytes are read on demand from where they
already live. (`ARTIFACT_ROOT = '/.quarto/project-artifacts/'` is already a
constant in `iframePostProcessor.ts`.)

This is symmetric with how `assetWalker.ts` (`buildAssetManifest`) already reads
**images** from the VFS at their source-relative path — referenced static
assets are present in the VFS source, which is exactly what the fallback relies
on.

### Two parts

1. **Post-processor fallback (TS).**
   `ts-packages/preview-renderer/src/utils/iframePostProcessor.ts`: where it
   resolves a `/.quarto/…` `src`/`href` to a data URI, add: on a VFS miss at the
   artifact path, retry at `ARTIFACT_ROOT`-stripped source path before giving
   up. Likely the same for the asset walker if iframes flow through it. Existing
   tests (`iframePostProcessor.integration.test.ts`) give the harness to extend.
2. **VFS source availability.**
   - hub-client: the uploaded `/examples/…` files are already in Automerge/VFS
     source — nothing to do.
   - `q2 preview` (CLI): the deck is on disk; the preview's VFS sync filter is
     `.qmd` + config (`WatchFilter`, `crates/quarto-hub/src/watch.rs`). Confirm
     whether referenced static assets are already loaded into the VFS (images
     are, per the asset walker) — if `resources:`/static `.html` are **not**,
     broaden the sync (or the initial VFS population) to include them.

## Open questions

1. Does the `q2 preview` initial VFS population already include non-`.qmd`
   static files (the asset walker reading images suggests yes for *referenced*
   images — confirm for `resources:` decks)? Determines whether part 2 needs any
   change for CLI.
2. Inline strategy for the iframe: data-URI `src` vs `srcdoc` vs blob URL — match
   whatever the post-processor does today for `/.quarto/` resources; a full
   self-contained deck (~750 KB) as a data URI is large but workable. Decide
   during implementation.
3. Does the deck's own relative asset loading (it's self-contained, so likely
   none) need anything? Confirmed earlier the decks embed their assets.

## Phasing (TDD-first)

- [x] **A — Post-processor fallback (TS).** DONE (commit `bfff2d3e`). The
  post-processor had **no `<iframe>` handling at all** (only `link`/`img`/`a`);
  added it. `iframePostProcessor.ts` now inlines an artifact-rooted iframe
  `src` via `srcdoc`, reading the VFS with `readArtifactOrSource` — artifact
  path first, then the `ARTIFACT_ROOT`-stripped source path. 4 jsdom tests
  (`iframePostProcessor.embed.test.ts`); existing 7 tests green; typecheck clean.
- [ ] **B — VFS source availability (the gate for e2e).** The staged deck must
  be present in the preview VFS source — it currently is **not**.
  - **Scope decided (user, 2026-06-09):** sync the `.html` files **visible via
    `resources:`** (resources-scoped, not all project `.html`). Flagged as a
    trust-boundary footgun — `resources:` is a *publish* control, not an
    *upload* control — and split into a hardening strand **bd-teh4hbli**
    (decouple "what may upload to the sync server" from `resources:`; safe
    defaults; the local/remote sync-server seam). This resources-scoped sync is
    the interim that strand tightens.
  - **Integration point found:** `ProjectFiles::discover`
    (`crates/quarto-hub/src/discovery.rs`) classifies qmd/config/binary(images,
    PDFs)/extension/`.tsx`. `.html` is **not a binary extension**, so it falls
    through and is never discovered → never synced. Add a resources-scoped
    `.html` category: resolve the project's `resources:` (via
    `quarto-core::project_resources` + `ProjectConfig`) and include matched
    `.html` files. (Watch filter `is_preview_relevant` is a *separate* event
    gate — discovery is the load path.)
  - Remaining cross-layer wiring: (a) hub discovery emits the resource `.html`
    set; (b) the sync/storage carries it into the VFS at the **source** path
    (binary resources already flow this way — mirror that); (c) the SPA's VFS
    population `vfsAddFile`s them. Verify needs a full hub+WASM+q2 rebuild +
    browser e2e (+ a hub-client/Automerge-only test).
- [x] **B — VFS source availability.** DONE. Resolved in `quarto-preview`
  (option chosen with user 2026-06-09: keep `quarto-hub` lean rather than add a
  `quarto-core` dep there). `config::resolve_project_resource_html` resolves the
  `resources:`-scoped `.html` via `ProjectContext::discover` + `expand_patterns`;
  `PreviewConfig.resource_html_files` → `HubConfig.resource_files` →
  `ProjectFiles::with_resource_files` (new text-synced category in
  `crates/quarto-hub/src/discovery.rs`). Verified through the real binary:
  `q2 preview docs/` logs `resource_count=8` and `Reconciled … count=163`.
- [x] **C — End-to-end.** DONE + browser-verified. **Plan correction:** the
  `q2 preview` SPA does **not** use `iframePostProcessor.ts` (that is the
  *hub-client* preview pane — `MorphIframe`/`DoubleBufferedIframe`). q2-preview
  renders via `Q2PreviewIframe` + React `<Ast>`; the embed `<iframe>` is a
  `RawBlock(html)` and asset resolution is the parent-side **`assetWalker.ts`**
  (which previously only walked `Image` nodes → blob URLs). Fix: `assetWalker`
  now also collects `.embed-example-iframe` srcs from `RawBlock(html)`, reads the
  deck text from the VFS, and mints a `text/html` blob URL into the asset
  manifest; `blocks/RawBlock.tsx` rewrites the deck `<iframe src>` to that blob
  URL (shared scan/rewrite helpers in `q2-preview/embedIframe.ts`). Playwright
  e2e against `q2 preview docs/`: all **8/8** `.embed-example-iframe` decks
  resolve to `blob:` srcs, `#demo-fragments` loads the real Fragments deck
  (reveal "FRAGMENTS" slide rendered), the "Demo 1:" caption + `@demo-fragments`
  xref work, and **zero console errors** (the 8 prior `/examples/…` MIME errors
  are gone). VFS key note: `vfs_add_file`/`vfs_read_file` use the path verbatim
  (no `/project/` prefix), so the absolute `/examples/…` src strips to the bare
  index key it was synced under.
  - The part-1 `iframePostProcessor.ts` generalization (page-relative `/X` →
    VFS source) was **kept** — it is correct for the *hub-client* preview pane,
    just not the renderer `q2 preview` uses (19 post-processor tests pass).
- [ ] **D — Hub-client parity.** Not yet exercised: a hub-client/Automerge run
  where `/examples/` is populated only in the VFS (no disk). The q2-preview fix
  is VFS-native (no disk assumption), so it should carry, but this path is
  unverified. Tracked as remaining work.

## Out of scope / unaffected

- `q2 render` (native): already works; `page_url_for` stays uniform (no
  `vfs_root` special-case — the reverted resolver branch is gone).
- The crossref feature (bd-t3cert81): orthogonal; works in preview already
  ("Demo 1" caption + `@demo` xref verified in-browser).
