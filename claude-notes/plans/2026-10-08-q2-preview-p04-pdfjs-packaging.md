# Plan 4: pdf.js packaging for the preview binary and the viewer SPA

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E5, E12); the execution model and landing procedure are there.

## Execution

- **Lane:** C (`workspace-3`), binding: one agent per plan, in this order: Plan 1, Plan 4, Plan 7, Plan 8, Plan 11. This plan is number 2 of 5 in the lane.
- **Waits on:** nothing. Nothing inside this plan waits on another plan.
- **Start gate:** none; start immediately.
- **Branch:** `q2-preview/p04-pdfjs-packaging`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 8 (static viewer), Plan 11 (SPA PDF pane), Plan 12 (embed PDF preview).
- **Next in lane:** Plan 7 (`2026-10-08-q2-preview-p07-embed-import`); the next agent starts only when its own start gate passes.

## Goal

The built `q2` serves a bundled pdf.js viewer tree at `/pdfjs/`, with correct MIME types, and the SPA and hub-client share one `pdf-viewer` package. No PDF feature is delivered here; the pieces PDF previews build on are.

## Background facts

- `resources/pdfjs-viewer.json` pins pdf.js 6.3.289; `fetchPdfjsViewer` (`scripts/fetch-pandoc-wasm.mjs:219-251`) is SHA-checked and `patchWorker` already pins the document fingerprint so zoom and scroll survive recompiles. 11 MB raw, 2.4 MB as tar.zst. `pdfjsDir`/`pdfjsStamp` are single constants (`:193-194`); the CLI block `:262-283` always runs all three fetches.
- `spa_manifest::content_type_for` (`spa-manifest/src/lib.rs:97`) has no `mjs` entry, so `.mjs` is served as `application/octet-stream` and browsers refuse the module worker. `spa_handler` answers any missing path with `index.html` and status 200 (`lib.rs:849-858`). `lookup_embedded(Editor)` falls back to the viewer bundle (`lib.rs:804-810`). `mime_guess` already knows `.mjs`.
- pdf.js with a non-`en-US` browser: when `locale/<x>/viewer.ftl` is missing, pdf.js does not fall back to English; toolbar labels and titles are empty (ran, `de-DE`). A 404 does not help.
- `hub-client/src/pandoc/pdfViewer.ts` (138 lines) imports nothing from hub-client; `PdfPreviewPane.tsx` is coupled to the download controller and is not reusable. `ts-packages/preview-runtime/package.json` is the package model (`"source"` export, `"types": "src/index.ts"`).
- The SPA `npm run build` runs in `release-pipeline.yml:251`, `xtask build_all.rs:168`, `verify.rs:714` (step 13, gated by its own flag, not `--skip-hub-build`) and the e2e workflow; `ts-test-suite.yml` lists per-package `npm test -w` lines (`:324-373`) and builds hub-client through `build:all`.
- `quarto-preview/build.rs:57-60,173` dedupes embed files against the viewer bundle; `scripts/prune-embed-dist.mjs:19` excludes `pdfjs` from the embed, and the embed falls back to the viewer bundle.

## Decisions

- **P4-D1** In the preview binary's copy (`q2-preview-spa/public/pdfjs`), prune pdf.js `web/locale` to `en-US` (saves 2.9 MB raw), and answer any other `locale/<x>/viewer.ftl` request with the `en-US` file through one shared function in `quarto-preview` (`pdfjs_response(rest_path)`), which every server of that tree calls: the SPA handler, the `--preview-dir` disk path, the `--join` frontend and Plan 8's static server. Other missing `/pdfjs/` paths return 404, not `index.html`. hub-client's copy (`hub-client/public/pdfjs`) keeps all locales: it is served by Vite or a static host with no alias, and pruning it would leave non-English users with empty toolbar labels.
- **P4-D2** `--pdfjs-only` is new, skips pandoc and typst assets, and `--require` still exits 1 on failure (without `--require` a failed fetch exits 0).

## Tasks

Per task: `cargo clippy -p <crate> --all-targets -- -D warnings` plus `cargo nextest run -p <crate>` for Rust; `tsc` plus `vitest` in the touched package. Workspace nextest at the end of the plan. Integration tests go in `crates/<crate>/tests/integration/<name>.rs` plus `pub mod` in `main.rs`.

- [ ] 4.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [ ] 4.2 `mjs` => `application/javascript` and `gif` => `image/gif` in `content_type_for` and in the mirror `scripts/manifest-dist.mjs` (was 0.5); unit assertions; `rust_generator_matches_the_npm_written_viewer_manifest` stays green (the viewer manifest hash changes; harmless).
- [ ] 4.3 `fetchPdfjsViewer({fromZip, dests})` over a list of `{dir, pruneLocales}` (hub-client with `pruneLocales: false`, `q2-preview-spa/public/pdfjs` with `pruneLocales: true`), one `.stamp` per destination, the same keep-filter for both (`locale.json` is kept); `--pdfjs-only` per P4-D2. Vitest beside the `patchWorker` tests: the two destinations are byte-identical apart from the pruned locale directories, and hub-client's keeps all 114.
- [ ] 4.4 `q2-preview-spa/package.json`: `"prebuild": "node ../scripts/fetch-pandoc-wasm.mjs --pdfjs-only --require"` (the SPA has no `predev`); `.gitignore` `q2-preview-spa/public/pdfjs/`. Each SPA-build site (the four above) gains a network dependency on a cold cache (cached in `.cache/pdfjs` afterwards): check each, including that the root `npm run build` (`--workspaces`) now runs the SPA prebuild and that the `ts-packages/*` build loops in `ts-test-suite.yml` and `hub-client-e2e.yml` (which do not build the SPA) are unaffected.
- [ ] 4.5 Extract `pdfViewer.ts` into `ts-packages/pdf-viewer/{package.json,tsconfig.json,src/index.ts,src/pdfViewer.test.ts}`; `base` becomes a required parameter (no `vite/client` types in the package) with a one-line hub-client wrapper passing `import.meta.env.BASE_URL`; the SPA passes `'./'` (its `vite.config.ts:38` base; this works only because the SPA page is always served at `/`); `hub-client/src/pandoc/pdfViewer.test.ts` moves or changes accordingly; keep a re-export at the old path so `PdfPreviewPane.test.tsx:31`'s mock and `test-hooks.ts:30` stay valid; add `@quarto/pdf-viewer` to the SPA's dependencies; update `package-lock.json` (`npm ci` must pass); add an `npm test -w @quarto/pdf-viewer` line to `ts-test-suite.yml`.
- [ ] 4.6 Serving: add `pdfjs_response(rest_path)` (P4-D1) and call it from `spa_handler` (`lib.rs:849`), `serve_from_disk` (`lib.rs:963`, which today falls back to `index.html`) and `join_frontend.rs`'s `lookup_embedded` route, so the built `q2` serves `/pdfjs/web/viewer.html` and the three `.mjs` files with a JavaScript content type; a missing `/pdfjs/...` path returns 404, except that any `/pdfjs/web/locale/<x>/viewer.ftl` is answered with the `en-US` file. Unit tests of `pdfjs_response` over an injected in-memory tree (these run in `test-suite.yml`, where `EMBEDDED_SPA` is the placeholder). Integration test over the real SPA dist GETs `/pdfjs/web/viewer.html`, `/pdfjs/build/pdf.mjs`, a pruned locale's `viewer.ftl` (expects the en-US body) and a nonexistent path (404); it skips with a clear reason on the placeholder and so effectively runs in the SPA e2e job. Record the binary-size delta. Check that the embed still holds no byte-identical duplicates of viewer files (`editor_embed_holds_no_byte_identical_duplicates_of_viewer_files`).
- [ ] 4.7 Plan end: workspace nextest against 4.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- pdf.js bumps need `patchWorker` updates; it throws loudly when its anchor is missing.
- The SPA prebuild adds a network step to every SPA build on a cold cache.

## Handoff log

Written by the implementing agent in the plan-end task, committed with the checklist: what landed (commit range), any task that deviated from the plan and why, names and signatures of the interfaces later plans consume exactly as they ended up, the workspace nextest baseline and final counts, and anything the next agent in the lane or a plan that waits on this one must know. Empty until the plan lands.
