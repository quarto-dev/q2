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

- [ ] **A — Post-processor fallback (TS).** Extend
  `iframePostProcessor.integration.test.ts`: an artifact-rooted `src` whose
  artifact path misses but whose source path hits resolves to the source bytes.
  Implement the fallback.
- [ ] **B — VFS source availability (CLI).** Confirm/extend the `q2 preview` VFS
  population so the staged decks are present at their source path. Test through
  the hub VFS layer.
- [ ] **C — End-to-end.** `q2 preview docs/` in a browser: the `#demo-fragments`
  iframe loads the real deck from the VFS; crossref ("Demo 1") already works.
- [ ] **D — Hub-client parity.** A test (or manual) where `/examples/` is
  populated only in the VFS/Automerge (no disk) and the iframe still resolves —
  the case the reverted disk route could never handle.

## Out of scope / unaffected

- `q2 render` (native): already works; `page_url_for` stays uniform (no
  `vfs_root` special-case — the reverted resolver branch is gone).
- The crossref feature (bd-t3cert81): orthogonal; works in preview already
  ("Demo 1" caption + `@demo` xref verified in-browser).
