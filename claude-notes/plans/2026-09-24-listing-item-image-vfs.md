# Listing item images break the hub preview (bd-yqlbfrln)

Branch `braid/bd-yqlbfrln-listing-item-image-copy` off main at `ccbdc441`.
Discovered from bd-q33ylfxf while Andrew tried the grouped New menu
locally: creating a project from the Blog template and opening
`index.qmd` shows

```
Pass 2 failed for /project/index.qmd: output destination
/project/_site/posts/post-with-code/image.jpg is not under any
allowed root (/.quarto/project-artifacts)
```

## Cause

`ListingGenerateTransform` (bd-qv2lsab0) registers a `ResourceCopyIntent`
for every listing item's front-matter `image:` so the file lands in the
output tree even though no page body references it. The destination is
`ctx.project.output_dir.join(img)`, which is right natively but, in the
hub-client's VFS-root resolver mode, points at `/project/_site/...`.
The only allowed write root there is `/.quarto/project-artifacts`, so
`OutputSink` rejects the copy and `flush_resource_copies` fails the
whole Pass-2 render of the host page.

`ResourceCollectorTransform` already handles the same situation for body
images by skipping copy intents when `resolver.is_vfs_root_mode()`: the
hub's parent-side asset walker reads `<img src>` straight from the VFS,
resolved against the page's path (`iframePostProcessor` /
`buildAssetManifest`). Listing thumbnails are host-relative `<img>` tags
in the rendered HTML, so the same walker serves them.

## Fix

- [x] Test first, `listing_generate.rs`: `resource_copies_with_resolver`
      helper; `item_image_copy_intent_targets_output_dir_natively`
      (website resolver, one intent `/project/posts/cover.png` to
      `/project/_site/posts/cover.png`) and
      `item_image_copy_intent_skipped_in_vfs_root_mode` (vfs_root
      resolver, no intents). Red: the second failed with one intent.
- [x] `listing_generate.rs`: compute `vfs_root_mode` from the attached
      resolver and skip the copy-intent loop body when set. Native
      behaviour unchanged (38 unit tests, 26 `listing_pipeline`
      integration tests green).
- [x] WASM bridge test `hub-client/src/services/listingItemImage.wasm.test.ts`:
      Blog-shaped project (website, `index.qmd` listing over `posts/`,
      one post with `image: "image.jpg"` and real bytes) renders through
      `render_page_in_project_with_attribution` both with
      `prefer_preview_format` on (hub default, `ast_json`) and off
      (`html`), and the thumbnail src stays `posts/post-with-code/image.jpg`.

## Verification (2026-09-24)

- Rust: `cargo test -p quarto-core --lib listing_generate` 38 passed;
  `--test integration listing_pipeline` 26 passed.
- WASM: worktree `npm run build:wasm`, then `npm run test:wasm` 26 files /
  150 tests passed (includes the new bridge test with the Blog template's
  `feed: true` and `categories: true`, and the changelog render gate).
- Browser: `npm run build:local-prod` from this worktree, stack on
  :3000/:8080/:8081 (hub binary reused from the main checkout's
  `target/debug/hub`; it is unaffected by this change). Fresh Playwright
  profile: ＋ New → Blog → name → create lands on `index.qmd`; the preview
  shows the listing (both starter posts, categories, dates), no "Render
  Error", no "not under any allowed root". The thumbnail `<img>` keeps
  `src="posts/post-with-code/image.jpg"`.

## Found on the way

- **Thumbnail bytes do not load in q2-preview** (bd-960vrfbm). The listing
  is a `RawBlock html`; `assetWalker.ts` only manifests structured `Image`
  nodes and `RawBlock.tsx` injects the HTML as is, so the iframe fetches
  `http://127.0.0.1:8080/posts/post-with-code/image.jpg` and gets a 404.
  The thumbnail column renders blank. Same gap for any author-written raw
  `<img>`. Separate strand, not part of this fix.
- The local-prod static proxy (`scripts/local-prod-server.mjs`) dies on an
  unhandled `ECONNRESET` when a browser disconnects, leaving :8080 dead
  while :3000/:8081 keep running. Flagged as a background task.

Commits: `dce0c5a5` (fix + tests + plan), `5d3ff890` (changelog),
`b4299ec8` (test mirrors template options).
