# Embed preview SPA bundles as tar.zst archives

## Overview

`quarto-preview` embeds the viewer SPA (`q2-preview-spa/dist`) and the
editor bundle (`hub-client/dist-preview-embed`, post-dedupe) via
`include_dir!` — identity bytes **plus** per-file `.gz` siblings. That
is ~107 MiB of the 181.8 MiB release binary (measured 2026-08-13:
`__TEXT,__const` = 106.8 MiB).

This plan swaps the embed format to **one tar.zst archive per bundle,
identity files only**, decompressed lazily on first asset request.
Precomputed `.gz` siblings leave the binary; gzip responses are
generated at runtime (flate2, level 9, cached per file) — flate2 is
already a quarto-preview dependency.

Measured on the real dists (2026-08-13, this machine):

| | identity tar | tar.zst -19 | tar.gz -9 |
|---|---|---|---|
| both dists (no `.gz`) | 96,610,816 B | **20,001,748 B** | 26,855,766 B |

- Decompress + tar parse: **94 ms** (zstd), one time per process.
- Worst-case lazy gzip: the 27 MB wasm, **1.53 s** at `-9` (0.47 s at
  `-6`), once per process per file.
- Expected binary: 181.8 → ~95 MiB (before the separate
  `strip = "symbols"` lever, −21.6 MiB more).

## Design decisions

1. **Identity-only archive.** `.gz` siblings stay out of the binary.
   Runtime gzip is the price; the alternative (archive identity + gz)
   costs ~26 MiB more for zero runtime CPU. Chosen per user direction
   after reviewing the numbers above.
2. **Lazy `OnceLock` decompression, per UI bundle.** `q2 render` never
   touches the preview code paths (verified: only
   `crates/quarto/src/commands/preview.rs` calls into quarto-preview),
   so render pays zero — no CPU, no RSS. Editor mode decompresses the
   viewer bundle too on first shared-asset fallback, same as today's
   page-in behavior.
3. **Runtime gzip mirrors the precompress skip set**
   (`scripts/precompress-dist.mjs` `SKIP_EXTENSIONS`): already-
   compressed containers (woff/woff2/png/…) are never gzipped at
   runtime, matching today's wire behavior file-for-file. Level 9 to
   match `Z_BEST_COMPRESSION`.
4. **Deterministic archives** (reproducible builds, coding.md): tar
   entries sorted by rel path, forward-slash normalized (Windows),
   mtime 0, fixed mode. zstd level 19.
5. **Placeholders archive identically.** Fresh-clone placeholder dists
   go through the same tar.zst path — no second embed mechanism.
6. **Dedupe + manifest machinery unchanged.** build.rs still strips
   editor/viewer duplicates and writes the editor `spa-manifest.json`
   into the editor embed dir **before** archiving, so the manifest
   rides inside the archive. `spa-manifest.json` is identity content
   (never had a `.gz`).
7. **Disk-override path untouched.** `SPA_DIR_OVERRIDE` serving still
   reads dist dirs from disk, including their `.gz` siblings — the npm
   precompress post-pass keeps its job there.

## Work items

### Phase 1 — tests first

- [x] Unit: embedded archives contain no `.gz` entries (pins the size
      win's source)
- [x] Unit: `gz_compressible` predicate mirrors the precompress skip
      set (js/css/wasm/html/svg/ttf compress; woff/woff2/png/… don't)
- [x] Unit: `embedded_gz` output gunzips to the identity bytes
- [x] Existing HTTP contract tests (`asset_serving.rs`,
      `editor_ui.rs`, `smoke.rs`, `asset_manifest.rs`) pass unchanged —
      they are the regression net

### Phase 2 — implementation

- [x] Workspace deps: add `tar`, `zstd` to `[workspace.dependencies]`
- [x] build.rs: archive both embed dirs (post-dedupe, post-manifest)
      to `$OUT_DIR/*.tar.zst`, expose `*_EMBED_ARCHIVE` env vars
- [x] lib.rs: `EmbeddedBundle` (archive bytes + `OnceLock` map),
      slice-based `lookup_embedded`, runtime `embedded_gz`
- [x] Update `asset_manifest.rs`, `join_frontend.rs`, and lib.rs unit
      tests to the new accessors
- [x] Drop `include_dir` from quarto-preview's Cargo.toml

### Phase 3 — verification

- [x] `cargo nextest run -p quarto-preview` — 139/139 green
- [x] `cargo xtask verify --skip-hub-build` — all 14 steps, workspace
      suite 11,939 passed
- [x] End-to-end: `q2 preview target/e2e-embed/index.qmd --port 4599`;
      `/` 200 no-cache; `assets/main-*.js` identity 67,791 B vs
      `Accept-Encoding: gzip` 21,546 B with `Content-Encoding: gzip`;
      gunzip roundtrip byte-identical (output inspected)
- [x] Release build; sizes recorded below

## Results (2026-08-13)

- **Embedded preview payload: ~107 MiB → 12.1 MiB**
  (viewer-embed.tar.zst 7.4 MiB + editor-embed.tar.zst 4.7 MiB,
  measured in `target/release/build/quarto-preview-*/out/`).
- **Release binary: 181.8 → 118.9 MiB (−62.9 MiB, −34.6%)**
  (190,648,272 → 124,674,896 B). The binary delta is smaller than the
  embed delta because `cargo xtask verify` rebuilt two other embedded
  artifacts between the baseline and final measurements: the q2-mcp
  `dist-bundle` (10.6 MiB, previously a placeholder) and the
  trace-viewer dist (0.2 MiB). Unrelated drift, not the change
  leaking.
- Remaining `__const` (47.4 MiB): the two archives (12.1), mcp bundle
  (10.6), resources (5.3), trace viewer (0.2), ~19 MiB Rust/crypto
  const data predating this change.
- New dependencies: `tar 0.4`, `zstd 0.13` (zstdmt feature for
  multithreaded build-time compression). `include_dir` dropped from
  quarto-preview (still used by quarto-trace-server and others).

## Follow-ups (not this plan)

- Background gzip warm-up at server start (erases the 1.5 s worst-case
  first-hit on the wasm).
- `strip = "symbols"` in `[profile.release]` (−21.6 MiB, measured).
- `quarto-trace-server`'s viewer embed is a separate, smaller
  `include_dir!` — same treatment if it grows.
- Decompress-to-tempfile + mmap if the ~95 MB heap resident set
  matters (today's embed is demand-paged; the archive trades that for
  binary size).
