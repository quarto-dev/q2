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

- [x] 4.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [x] 4.2 `mjs` => `application/javascript` and `gif` => `image/gif` in `content_type_for` and in the mirror `scripts/manifest-dist.mjs` (was 0.5); unit assertions; `rust_generator_matches_the_npm_written_viewer_manifest` stays green (the viewer manifest hash changes; harmless).
- [x] 4.3 `fetchPdfjsViewer({fromZip, dests})` over a list of `{dir, pruneLocales}` (hub-client with `pruneLocales: false`, `q2-preview-spa/public/pdfjs` with `pruneLocales: true`), one `.stamp` per destination, the same keep-filter for both (`locale.json` is kept); `--pdfjs-only` per P4-D2. Vitest beside the `patchWorker` tests: the two destinations are byte-identical apart from the pruned locale directories, and hub-client's keeps all 114.
- [x] 4.4 `q2-preview-spa/package.json`: `"prebuild": "node ../scripts/fetch-pandoc-wasm.mjs --pdfjs-only --require"` (the SPA has no `predev`); `.gitignore` `q2-preview-spa/public/pdfjs/`. Each SPA-build site (the four above) gains a network dependency on a cold cache (cached in `.cache/pdfjs` afterwards): check each, including that the root `npm run build` (`--workspaces`) now runs the SPA prebuild and that the `ts-packages/*` build loops in `ts-test-suite.yml` and `hub-client-e2e.yml` (which do not build the SPA) are unaffected.
- [x] 4.5 Extract `pdfViewer.ts` into `ts-packages/pdf-viewer/{package.json,tsconfig.json,src/index.ts,src/pdfViewer.test.ts}`; `base` becomes a required parameter (no `vite/client` types in the package) with a one-line hub-client wrapper passing `import.meta.env.BASE_URL`; the SPA passes `'./'` (its `vite.config.ts:38` base; this works only because the SPA page is always served at `/`); `hub-client/src/pandoc/pdfViewer.test.ts` moves or changes accordingly; keep a re-export at the old path so `PdfPreviewPane.test.tsx:31`\'s mock and `test-hooks.ts:30` stay valid; add `@quarto/pdf-viewer` to the SPA's dependencies; update `package-lock.json` (`npm ci` must pass); add an `npm test -w @quarto/pdf-viewer` line to `ts-test-suite.yml`.
- [x] 4.6 Serving: add `pdfjs_response(rest_path)` (P4-D1) and call it from `spa_handler` (`lib.rs:849`), `serve_from_disk` (`lib.rs:963`, which today falls back to `index.html`) and `join_frontend.rs`\'s `lookup_embedded` route, so the built `q2` serves `/pdfjs/web/viewer.html` and the three `.mjs` files with a JavaScript content type; a missing `/pdfjs/...` path returns 404, except that any `/pdfjs/web/locale/<x>/viewer.ftl` is answered with the `en-US` file. Unit tests of `pdfjs_response` over an injected in-memory tree (these run in `test-suite.yml`, where `EMBEDDED_SPA` is the placeholder). Integration test over the real SPA dist GETs `/pdfjs/web/viewer.html`, `/pdfjs/build/pdf.mjs`, a pruned locale's `viewer.ftl` (expects the en-US body) and a nonexistent path (404); it skips with a clear reason on the placeholder and so effectively runs in the SPA e2e job. Record the binary-size delta. Check that the embed still holds no byte-identical duplicates of viewer files (`editor_embed_holds_no_byte_identical_duplicates_of_viewer_files`).
- [x] 4.7 Plan end: workspace nextest against 4.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- pdf.js bumps need `patchWorker` updates; it throws loudly when its anchor is missing.
- The SPA prebuild adds a network step to every SPA build on a cold cache.

## Handoff log

Landed on `q2-preview/p04-pdfjs-packaging`: one implementation commit (`2f2f9f665`) plus this checklist/Handoff commit, on top of `67a1471d2` (the integration tip did not move, so no rebase and no conflicts).

**Interfaces (as shipped)**
- `spa_manifest::content_type_for`: `mjs` => `application/javascript`, `gif` => `image/gif`; mirrored in `scripts/manifest-dist.mjs`.
- `scripts/fetch-pandoc-wasm.mjs`: `fetchPdfjsViewer({ fromZip, dests = pdfjsDests })` returns `{ dirs, dir }`; `pdfjsDests` is `[{dir: hub-client/public/pdfjs, pruneLocales: false}, {dir: q2-preview-spa/public/pdfjs, pruneLocales: true}]`; one `.stamp` per destination (the stamp covers `pruneLocales` and the keep-filters). `--pdfjs-only` skips pandoc and typst; `--require` exits 1 on failure. `pdfjsDir`/`pdfjsStamp` are still exported (hub-client's destination). The pruned copy keeps `web/locale/locale.json` and `web/locale/en-US/`.
- `q2-preview-spa/package.json` has `prebuild` (`node ../scripts/fetch-pandoc-wasm.mjs --pdfjs-only --require`); `q2-preview-spa/.gitignore` ignores `public/pdfjs/`. Every SPA build site runs `npm run build` inside `q2-preview-spa`, so each picks the prebuild up (the root `npm run build` runs it too, a no-op after the first fetch). The `ts-packages/*` build loops do not build the SPA and are unaffected.
- `@quarto/pdf-viewer` (`ts-packages/pdf-viewer`): `mountPdfViewer(container: HTMLElement, base: string): PdfViewerHandle`, `viewerUrl(base: string, file?: string, restore = false): string`, `PDF_HISTORY_KEY`, type `PdfViewerHandle` (`show(pdf, {key, fileName})`, `dispose()`). `base` is required; the preview SPA passes `'./'`. hub-client keeps `hub-client/src/pandoc/pdfViewer.ts` as a wrapper defaulting `base` to `import.meta.env.BASE_URL` (same exports), so `PdfPreviewPane.test.tsx`\'s mock and `test-hooks.ts` are unchanged. `hub-client/vitest.config.ts` aliases the package; `@quarto/pdf-viewer` is a dependency of hub-client and q2-preview-spa; `ts-test-suite.yml` runs `npm test -w ts-packages/pdf-viewer`. The SPA does not import it yet (Plan 11 does).
- `quarto_preview::resolve_pdfjs_rel(rel: &str, has: impl Fn(&str) -> bool) -> Option<String>` (pub, `rel` is dist-relative, e.g. `pdfjs/web/viewer.html`): existing path => itself; `pdfjs/web/locale/<x>/viewer.ftl` missing => the `en-US` file when present; anything else under `pdfjs/` => `None`, which the caller answers 404. Used by `spa_handler` (via `pdfjs_embedded_response`), `serve_from_disk` (`--preview-dir`) and `join_frontend::route`. **Plan 8's static server must call it too** (and 404 on `None`); there is no separate `pdfjs_response(rest_path)` function (see Deviations).

**Deviations**
- The plan named a shared `pdfjs_response(rest_path)`. The shared piece is the pure path decision `resolve_pdfjs_rel` instead, because the embedded, disk and join paths build their responses differently (sync embed lookup, async disk read, raw TCP head); each calls the resolver and keeps its own response builder.
- The join frontend serves a locale alias locally only when the en-US file is in the guest's manifest; every other non-manifest `/pdfjs/` path still tunnels to the host, which answers 404.
- The plan's "hub-client's keeps all 114" is 113 locale directories plus `locale.json`; the vitest asserts 113 directories and that `locale.json` is kept.
- `package-lock.json` was edited by hand to the four additive entries (`npm install --package-lock-only` also pruned unrelated `@esbuild/*` platform entries on this machine).

**Tests:** `scripts/fetch-pdfjs.test.mjs` (destinations; skips without the cached release zip), `ts-packages/pdf-viewer/src/pdfViewer.test.ts`, hub-client `pdfViewer.test.ts` (wrapper default base), 1 spa-manifest unit test, 4 `quarto-preview` unit tests over an in-memory tree, and `tests/integration/pdfjs_serving.rs` (2; they run against the real SPA dist, which exists in this worktree, and skip with a message on the placeholder embed, so `test-suite.yml` skips them and the SPA e2e job runs them). `editor_embed_holds_no_byte_identical_duplicates_of_viewer_files` and `rust_generator_matches_the_npm_written_viewer_manifest` stay green.

**Binary size:** the preview SPA's pruned pdf.js tree is 8.2 MB raw and about 2.1 MB as tar.zst (hub-client's full tree is 11 MB raw), so the `q2` binary grows by roughly that archive size; the embed is not duplicated by the editor embed.

**Nextest (workspace):** 4.1 predates the epic's ledger rule, so I took my own baseline at the Plan 3 tip (`67a1471d2`): the listing was 16013 tests, 202 skipped; that run aborted (fail-fast) at 14892 passed, 2 failed, 1119 not run. The two failures (`config_endpoint::config_reports_embedded_asset_manifest_hashes`, `asset_manifest::rust_generator_matches_the_npm_written_viewer_manifest`) were my own contamination, from rebuilding the SPA `dist/` mid-run, and pass on the final tree; so that baseline is a listing, not a full green run. First `--no-fail-fast` run on the plan branch, before Plan 2 landed: 16020 passed, 0 failed, 202 skipped (delta +7 = 1 spa-manifest + 4 `quarto-preview` unit + 2 integration tests, skipped unchanged). Plan 2 then landed first, so I rebased onto `3b5fb2fc7` (one conflict, the 4.1 checklist line, resolved to the epic's new wording), reran clippy on `spa-manifest` and `quarto-preview` (clean) and the workspace: 16032 passed, 0 failed, 202 skipped; the extra 12 passed against my first run are Plan 2's tests (its crates are disjoint from mine).

Plan 5 then landed too; after a second rebase (onto `f02ad368d`, no conflicts; Plan 5 touched only the wasm crate and hub-client, disjoint from mine) the hub-client download and PDF tests, clippy and the workspace were rerun with the same counts.

Ledger: fdd98e521: 16032 passed, 202 skipped

**For later plans:** Plan 8 (static viewer) and Plan 11 (SPA PDF pane) can rely on `/pdfjs/web/viewer.html` with the `en-US` alias in the preview binary and on `@quarto/pdf-viewer`; Plan 12 (embed) is served `/pdfjs/` through the viewer-bundle fallback (the embed holds no pdf.js files). A cold SPA build now needs network once for pdf.js (cached in `.cache/pdfjs`). The e2e workflow's SPA build step already runs `npm run build` in `q2-preview-spa`, so it gets the prebuild with no workflow change.
