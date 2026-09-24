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

## Verification

See the record at the end of this file once the WASM build finishes.
