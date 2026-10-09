---
title: 'Single-file `q2 preview`: resolve sibling assets the deck references'
date: 2026-06-16
description: 'Makes `q2 preview` on a standalone `.qmd` sync the sibling images it references into the preview''s virtual filesystem, without walking the whole directory, so images render as they do in project mode.'
braid:
  strand: bd-kpuweafo  # follow-up to bd-y259zb57 / bd-ggvq1j68; found 2026-06-16
  priority: P2
  labels: [preview, revealjs]
---

## Problem

`q2 preview deck.qmd` on a bare file (no `_quarto.yml` ancestor) runs single-file
mode (`bd-tnm3k`), which **does not walk the directory** — only the target `.qmd`
(plus, now, `_brand.yml` siblings) reaches the preview VFS. So an image
`![](./sibling-image.png)` renders broken: `vfsReadBinaryFile` fails, the asset
manifest has no entry, and the `<img>` falls through to the SPA `index.html`
(verified E2E: `naturalWidth = 0`, response `content-type: text/html`). In
**project mode** (`_quarto.yml` present) the dir-walk syncs the image →
automerge → VFS → blob URL, and it works (`naturalWidth = 320`).

## Constraint

Single-file mode must **not** walk the whole directory (`bd-tnm3k`): a bare
`q2 preview ~/Downloads/note.qmd` must not index all of `~/Downloads`. So
"match project behavior" = sync the **specific sibling assets the deck
references**, not every sibling.

## Approach (reference-driven VFS sync)

Mirror project mode's *mechanism* (sync into VFS → blob URL), not a new
disk-serving HTTP route (keeps the deliberate VFS-only security posture,
`bd-teh4hbli`; avoids a single-file↔project mechanism divergence). Mirror the
existing `resource_files` injection pattern (resolved in the preview layer,
injected via `HubConfig`, synced by `reconcile_files_with_index`).

## Work items

- [x] **A. quarto-core — extract referenced asset URLs.** `transforms::
      collect_referenced_asset_urls(blocks) -> Vec<String>` (reuses the
      `ResourceCollectorTransform` traversal via empty anchors, so nested images
      in lists/Divs/figures/tables are found; external/absolute URLs dropped).
      Test: `collect_referenced_asset_urls_returns_relative_images_only`.
- [x] **B. quarto-preview — resolve to on-disk siblings.**
      `config::resolve_single_file_assets(root, rel, runtime)`: parse deck via
      `pampa::readers::qmd::read`, collect URLs, keep relative binary assets that
      exist and canonicalize **under** `root` (no `../` escape). Returns
      root-relative paths. Tests: `single_file_assets_resolves_referenced_image_
      siblings_only`, `single_file_assets_rejects_parent_escape`.
- [x] **C. config plumbing.** Resolved in `build_hub_config` (quarto-preview has
      the parser) → new `HubConfig.single_file_assets`. No `PreviewConfig` field
      (8 literal sites) — resolved at the mapping point instead.
- [x] **D. quarto-hub — sync them.** `ProjectFiles::with_binary_files(...)`; the
      single-file branch in `context.rs` appends `config.single_file_assets` to
      `binary_files`, so `reconcile_files_with_index`\'s binary loop syncs them.
      Test: `test_single_file_with_binary_files_adds_referenced_assets`.
- [x] **E. E2E.** `q2 preview page.qmd` (no `_quarto.yml`) with sibling
      `./sibling-image.png`: before = broken (`naturalWidth 0`, raw HTTP url →
      SPA index.html); after = **blob URL, `naturalWidth 320`** — identical to
      project mode. Screenshot showed the rendered Quarto logo.

## Known v1 limitations (document, don't fix now)

- **Load-time only.** Assets are resolved when the session starts. An image
  reference *added while editing* won't sync until reload (project mode catches
  it via the dir watcher). Extending the single-file watch loop to re-extract on
  deck change is a follow-up.
- **Direct references only** (not transitively through `{{< include >}}`).
- **Images only** for v1 (the dominant case); other binary refs (e.g. linked
  PDFs/video) can extend `collect_referenced_asset_urls` later.
- **Non-conventional brand paths** (`brand: ../shared/foo.yml`) remain
  project-only (bd-ggvq1j68).
