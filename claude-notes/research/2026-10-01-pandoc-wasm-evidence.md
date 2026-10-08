# Research notes: pandoc.wasm in hub-client

**Date:** 2026-10-01. Evidence behind the pandoc-wasm design ([`../designs/pandoc-wasm-architecture.md`](../designs/pandoc-wasm-architecture.md)) and its two epics ([request](../plans/2026-10-01-pandoc-request-epic.md), [host](../plans/2026-10-01-pandoc-host-epic.md)). Code refs are to this branch (`feature/pandoc-wasm`,
from main `142cb4045`). Items marked *(inferred)* were read from source, not built or run. The spike harness is in `2026-10-01-pandoc-wasm-spike/` next to this file (called `spike/` below).

## 1. The Pandoc wasm artifact

- Official assets: `pandoc-<ver>.wasm.zip` on each jgm/pandoc release since 3.9 (`gh release download <ver> -R jgm/pandoc -p 'pandoc-<ver>.wasm.zip'`;
  the zip contains `pandoc-wasm/pandoc.wasm`).

| Version | pandoc.wasm raw | zip (deflate) | Needs exnref? | Node 24 (V8 13.6) | Chromium (Playwright) | WebKit 26.4 (Playwright) |
|---|---|---|---|---|---|---|
| 3.9 | 58.4 MB | 16.2 MB | no | ok | not run | not run |
| 3.10 (also bundled in npm `pandoc-wasm@1.1.0`) | 58.6 MB | 16.2 MB | no | ok | ok | not run |
| **3.11** (our native floor, `PANDOC_PIN`) | 59.2 MB | 16.4 MB | **yes** | needs `--experimental-wasm-exnref` | ok | ok (also Firefox 157 ok) |
| 3.12 (latest, released 2026-09-29; not used, the wasm is pinned to 3.11) | n/a | 16.7 MB | **yes** | needs the flag | not run | not run |

- Brotli -q11 on 3.11: **10.9 MB** (gzip -9 on 3.10: 16.0 MB). Brotli is the smaller wire size, but no serving path in this repo does Brotli (see §7), so the working baseline is gzip (~16 MB).
- **exnref:** 3.11+ wasm uses the new WebAssembly exception-handling encoding (opcode 0x1f). Node 24 rejects it without
  `--experimental-wasm-exnref`; Chromium and WebKit 26.4 accept it with no flag. **Firefox 157 (macOS, the user's default browser) also
  passes the full spike-3 suite** (`spike/ff/`, 2026-10-01): exnref probe, compile (~250 ms), instantiate, docx/pptx/epub, Lua filter with nested
  `require` + env var, 20 repeated conversions (wasm memory 119 MB), 3 fresh instances (~310-330 ms create+docx each). Playwright's own
  Firefox build cannot launch on this machine (`RenderCompositorSWGL failed mapping default framebuffer`), so Firefox was tested by opening the page in the real browser.
  Older Firefox versions are untested (MDN lists exnref from 131).
- The npm `pandoc-wasm@1.1.0` ships pandoc 3.10, **below our native floor 3.11**
  (`crates/quarto-core/src/pandoc_filters/mod.rs:28`, `PANDOC_PIN = "3.11"`; vendored Q1 pin `QUARTO_CLI_PIN = "v1.11.3"`). So we cannot
  just depend on that npm package; we download the official 3.11 asset.
- Toolchain: standalone `wasm32-wasi` (GHC wasm backend), no JS FFI; not linked with our Rust wasm; hosted by any WASI shim
  (`@bjorn3/browser_wasi_shim`). Reactor protocol used by the npm host: `hs_init_with_rtsopts`, then exports `query` / `convert`.

## 2. Host behaviour measured in the spike

| Behaviour | Result |
|---|---|
| Stock npm host file store | **Flat** map: `filters/sub/x.lua` is not found. Our host needs nested directories (`spike/host-patched.js`, ~10 lines). |
| Env vars | **Fixed at instance init.** Changing the env array between `convert()` calls is invisible (`spike/env-reuse.mjs`). Files are re-read every call. |
| Per-render params options | (a) new instance per render from a pre-compiled `WebAssembly.Module`: ~40 ms to create, but the first docx then costs ~630 ms vs ~40-130 ms warm, memory stable (GC reclaims old instances; `spike/instance-cost.mjs`). (b) keep one warm instance and deliver the params blob via a file read by a small Lua preamble that overrides `os.getenv` for `QUARTO_*` (fits the planned P5 shim). |
| FS cleared per `convert()` | The host clears the file map every call, so the share tree (~250 files) must be re-added each call; cost unmeasured, expected small. |
| `-f json` + `filters:` (Lua) + nested `require` | Works. The option key is `filters`, not `lua-filter`. |
| Memory | Linear memory 39 MB fresh, plateaus at ~105-118 MB after many conversions (no leak). Chromium process-tree RSS about +340 MB once pandoc is loaded (mostly compiled code). |
| Latency | Compile+init ~140-200 ms; first docx 150-650 ms; later 30-130 ms. |
| Coexistence with Rust wasm | Works in Node and Chromium (chain spike: `spike/chain.spike.test.ts`, `chain-browser.mjs`). Rust AST goes straight into `-f json`; docx `document.xml` byte-identical to native for the preview-format AST. |
| Command mode (`_start`) | Not exercised by any committed spike; all spike measurements here are reactor-mode (`hs_init`/`convert`/`query`). The 3.11 wasm does export `_start`. The 2026-10-01 runtime audit (§9) later ran command mode in Node; the browser run is host phase H0. |
| Where Rust wasm runs today | Main thread: `initWasm()` in `ts-packages/preview-runtime/src/wasmRenderer.ts:150`; hub-client has no application Workers (Monaco only). Pandoc in its own Worker is a clean fit. |

## 3. What the native Pandoc leg does (so a wasm "request" can reproduce it)

From `crates/quarto-core/src/stage/stages/pandoc_write.rs` (`PandocWriteStage`) and `pandoc_filters/*`:

- **Before the call:** coerce title/subtitle MetaBlocks -> MetaInlines (`:452`); typst-only section numbering and
  `--shift-heading-level-by` (`:462-468`); pandoc version gate (`:473`, `:102-117`).
- **Argument order** (`:927-957`): `-f json -t <writer> --data-dir <share>/pandoc/datadir -L <share>/filters/main.lua`, then epub extra args,
  then `Format::pandoc_invocation_args()`, then `--resource-path <doc_dir>`, then typst extras, then `-o <output>`, then
  `forwarded_args` (reference-doc, template, highlight-style, toc, toc-depth, reference-location, slide-level, default-image-extension), then the input JSON path.
- **Env vars:** `QUARTO_SHARE_PATH`, `QUARTO_FILTER_PARAMS` (base64 blob via `encode_params_blob`), `QUARTO_FILTER_DEPENDENCY_FILE` (must exist and be writable).
- **Files pandoc reads/writes:** `pandoc-input.json` (wire JSON, `raw: false`; a file, not stdin); the share tree
  (`filters/`, `pandoc/datadir/`, `formats/docx/` callout PNGs); epub `formats/{html/styles-callout.html,epub/styles.html}`; doc-relative resources
  (images, `reference-doc`, `epub-cover-image`, `epub-metadata`, `epub-embed-font`, `css`); writable results file, deps file, crossref index
  (paths named in the params blob).
- **Output:** written directly to `output_path`; binary bytes never pass through `PipelineData`. Only markdown-family outputs get a post-step (shortcode unescape).
- **Stderr** is captured unconditionally; `classify_pandoc_completion` (`:62`) yields `Q-11-1` warnings on success or the `Q-20-3` nonzero-exit error. These live in the pure
  `diagnostics.rs` and are reusable in wasm.

## 4. What is already wasm-ready, and what is not

- **Already compiled for wasm32 (ungated):** `params`, `params_codec`, `meta_coerce`, `format_defaults`, `typst_brand`, `typst_params`, `version`,
  `crossref_params`, the `include_dir!` statics for the vendored trees (`pandoc_filters/mod.rs:31-61`), `PipelineProfile` (`format.rs:499`),
  `Format::pandoc_writer_name`/`pandoc_invocation_args` (`format.rs:1181-1190`). *(inferred from cfg gates; `quarto-core` was not built for wasm32 with `pandoc_write` ungated)*
- **Gated native-only:** `bundle` (extracts to disk), `harness` (spawns pandoc), `diagnostics` (pure; only gated because its sole consumer is native — can just be ungated),
  the `pandoc_write`, `resource_copy_flush`, `typst_compile` stages (`stage/stages/mod.rs:74-147`), `render_qmd_to_pandoc` and the pandoc stage-list builders
  (`pipeline.rs:427, 560, 654, 670, 1258`; wasm fallback `:1274` always picks the HTML list).
- **Everything before PandocWrite already runs in wasm:** the pandoc stage list is the HTML list minus `PANDOC_STAGE_EXCLUDED` (`pipeline.rs:526-566`); crossref, callouts,
  `AstTransformsStage` (dispatches on `PipelineProfile` at runtime), citeproc (pampa, in Rust), and pampa's JSON writer are shared.
- **wasm entry:** `render_page_in_project_with_attribution` (`crates/wasm-quarto-hub-client/src/lib.rs:1271`) picks pipeline by `format.pipeline_kind`
  (`"preview"` -> preview AST, else HTML); there is **no `PipelineProfile::Pandoc` branch** in the wasm crate (format override: `:1538-1546`).
- **Runtime reads in the prepare path that need the VFS/`SystemRuntime`:**
  `epub_extra_args` (`:152`, uses `std::fs` + extracts `FORMATS_DIR`); `resolve_user_template_path` (`:1095`) and `stage_typst_template_partials` (`:1118`) (`std::fs::copy`, typst);
  `stage_typst_brand_fonts` (`:329`, `fetch_url` via `pollster::block_on`, typst); `resolve_typst_available_fonts` (`:364`, runs `typst fonts`, typst, must return `None` in wasm);
  `resolve_typst_brand` (`:260`) goes through `SystemRuntime` and is probably fine. `quarto-system-runtime` already has a VFS-backed `WasmRuntime` (`src/wasm.rs`): file read/write,
  `dir_create`, `fetch_url`, `temp_dir` -> `/tmp/<name>-N` in the VFS; `exec_command`/`exec_pipe` unsupported; `env_get` empty.

## 5. Vendored trees and resources

| Tree | Files | Raw | gzip -9 |
|---|---|---|---|
| `resources/pandoc-filters/filters/` | 220 | 1.12 MB | 280 KB |
| `resources/pandoc-filters/pandoc/datadir/` | 27 | 212 KB | 61 KB |
| `resources/formats/` (docx PNGs, epub/html snippets) | 7 | 18.7 KB | 12.7 KB |
| `typst-template/` (typst only) | 8 | 16.6 KB | 5.8 KB |
| `resources/typst-packages/` (typst only; 5 packages, fonts 1.7 MB of 2.5 MB) | | 2.5 MB | |

- Filters + datadir + formats together: ~1.35 MB raw, ~353 KB gzipped. A docx/pptx/epub subset is **not** meaningfully smaller: `main.lua` has 171 unconditional
  `import()`s; only `dashboard`/`rmarkdown`/`llms`/`luacov` could be pruned (<10%, untested). `datadir` carries luacov/profiler files not needed at runtime.
- Per-format extras: docx 5 PNGs (5.5 KB); epub 2 HTML snippets (~13 KB).
- **By-path inputs for docx/pptx/epub:** images (found through `--resource-path <doc_dir>`; a missing image is a warning `Q-11-1` and pandoc substitutes the alt text);
  `--reference-doc`/`--template` (a missing file is the hard error `Q-5-30`, `format_defaults.rs:163-189`); epub `--epub-cover-image`, `--epub-metadata`, repeated
  `--epub-embed-font`, repeated `--css`. **Bibliography/CSL are not read by pandoc** — pampa's citeproc consumes them in Rust before JSON is produced.
- docx/pptx/epub **do not need `ResourceCopyFlushStage`** (pandoc embeds image bytes at write time); the output-dir resource copy is irrelevant for them. No Rust-side zip/epub post-processing exists.
- **Hub VFS:** `vfs_add_file`, `vfs_add_binary_file`, `vfs_read_binary_file` (base64) at `wasm-quarto-hub-client/src/lib.rs:399-576`; `ts-packages/preview-runtime/src/automergeSync.ts:102,112`
  already pushes Automerge binary docs (images) into the VFS. The VFS lives *inside the Rust wasm*, so TS has to copy files out (`vfs_list_files` + `vfs_read_binary_file`) into pandoc's FS.
  Non-doc files like `reference-doc` only exist there if they are in the Automerge project.

## 6. Lua calls the wasm sandbox lacks (grep of vendored `filters/`, `datadir/`)

- **Happy path for docx/pptx/epub is fine** given the FS/env setup: `os.getenv` (`QUARTO_SHARE_PATH`, `QUARTO_FILTER_PARAMS`, `QUARTO_FILTER_DEPENDENCY_FILE`, soft-failing `QUARTO_PROJECT_DIR`, shortcode `env`),
  `io.open` writes to paths we choose (deps file, results file, crossref index), `pandoc.read`, `pandoc.path.*`, `pandoc.system.get_working_directory`.
- **Not on the happy path:** `pandoc.pipe("rsvg-convert")` (`pdf-images.lua:15`, pdf only), `pandoc.system.with_temporary_directory` (`modules/mediabag.lua:21`, pdf-gated),
  `pandoc.pipe quarto run juice.ts` (`normalize/astpipeline.lua:47-78`, raw HTML tables; gated to typst output at `:113`; only the `pandoc.pipe` is `pcall`-guarded, the `with_temporary_directory` around it is not (§9); line numbers are pre-#766, which is the in-process replacement), `os.execute("quarto ...")` (profiling/trace), `shiny.lua` pipes, `email.lua`, `manuscript.lua`, typst-only mediabag code,
  debug env hooks. **`pandoc.mediabag.fetch` (network)** runs only for remote-URL images — those will fail in wasm.
- Not yet checked: format gating of `mediabag_filter` (`main.lua:598`) and `writeIndex` (`main.lua:714`); `pandoc.write`/`os.date`.
- Unknowns that only running it answers: whether wasm pandoc's Lua has full `io`/`os` for these writes (the spike's filter used `io.open` read and `os.getenv` successfully), working-directory semantics, Lua startup time for ~1.3 MB of filters in wasm (native extraction ~27 ms).

## 7. Delivery and offline

- hub-client is a Vite PWA (`hub-client/OFFLINE.md`): the app shell is precached (~14 MB); WASM files (`/assets/*.wasm`) are runtime-cached `CacheFirst` in `wasm-cache`,
  30-day expiry, **max 8 entries** (pandoc.wasm stays off this route; its loader owns its Cache API entry, design doc D6) (Rust wasm ~26-30 MB, Automerge ~3.5 MB, tree-sitter). Large assets were deliberately moved out of the precache (GH #447: an all-or-nothing ~56 MB
  install let one flaky fetch discard the new service worker). So pandoc.wasm is a runtime-cached, on-demand asset, never precached, and the entry cap is unchanged.
- `scripts/precompress-dist.mjs` and `scripts/gzip-skip-extensions.txt` exist for dist precompression; mime-db marks `application/wasm` non-compressible in Vite's compress config
  (`vite.config.ts:101-107`), so a Brotli path would need an explicit allowance. In practice there is no Brotli anywhere: `precompress-dist.mjs` and the q2 preview embed are `.gz`-only (decided 2026-08-13; `quarto-preview/src/lib.rs:858-883`), nginx and `vite preview` gzip only. The embed also does `file.to_vec()` per request and caches gzip output in memory (`GZ_CACHE`), and the release pipeline does not build the editor embed (`release-pipeline.yml` `web-payloads`).
- Native `q2 preview` embeds a built hub-client (`hub-client/package.json` `build:preview-embed`, `crates/quarto-preview`; `crates/xtask/src/build_hub_client_embed.rs` only runs that script) and renders non-HTML formats with real pandoc natively. The embed therefore carries no pandoc.wasm and uses the native pandoc (design doc D7). The PWA service worker is disabled in the embed.

## 8. Typst for PDF (see the host epic's PDF phases H7-H9)

- Native q2 **shells out to an external `typst` binary** (`typst_compile.rs:236`, `--root`, `--font-path`; floor 0.8, unpinned; local 0.14.2). Typst crates in `Cargo.lock` come only via `typst-gather` (package fetching), not the compiler.
- Vendored typst packages: showybox, fontawesome, theorion, octique, marginalia (2.5 MB incl. 1.7 MB Font Awesome OTFs), extracted to `packages/preview/<name>/<ver>/` + `fonts/` (`bundle.rs:106`).
  Other `@preview` packages are fetched by `typst_gather::gather_packages` (`typst_compile.rs:157`). `--font-path` order is load-bearing: vendored fonts, document `font-paths`, brand file fonts, Google font cache.
  `typst-available-fonts` filter param is computed from `typst fonts`.
- Browser options: **typst.ts** (`@myriaddreamin/typst-ts-web-compiler@0.7.0`, targets typst 0.14.2 = native here; compiler wasm 28.3 MB raw / ~10.8 MB gzip; active; `0.8.0-rc` targets typst 0.15 rc);
  `typst-wasm` (1.0.0, younger, 37 stars, not evaluated in depth); embedding the `typst` crate ourselves (we would write the `World`: VFS, fonts, package registry, PDF export; ~10 MB+ estimated; we own version bumps).
  typst.ts bundles **no default fonts**; the host supplies them (`typst-assets` ~4-8 MB, estimated). API: `map_shadow(path, bytes)` virtual FS, `add_raw_font`/`add_lazy_font`, JS package-registry callback, PDF export; works in a Worker.
- pdf.js: `pdfjs-dist@6.3.289`; main 459 KB (132 KB gz) + worker 1.27 MB (375 KB gz); Vite `?url` worker import; lazy `await import()`.
- Pitfalls: pandoc 3.10/3.11 typst writer targets Typst 0.14-era syntax; fonts differ -> layout differs from native; package versions must match; `--root` must map to the virtual FS.

## 9. Runtime audit (2026-10-01, run in Node 24.5 with `--experimental-wasm-exnref`, shim 0.4.2)

Native runs were captured with a `QUARTO_PANDOC` wrapper and replayed through the 3.11 wasm in command mode (`_start`), after rewriting native temp paths to fixed ones.
- **Parity with native pandoc 3.11 on identical inputs:** typst `.typ` identical; pptx 1 of 54 zip entries differs (`core.xml` timestamp); epub 2 of 12 differ (`content.opf`, `toc.ncx`: random `urn:uuid` + modified time); docx differs only through an SVG image (native embeds a PNG via `rsvg-convert`; wasm warns and emits an empty `<a:blip>`).
- **Determinism:** `SOURCE_DATE_EPOCH` gives a byte-identical whole docx; epub is not identical without a document `identifier`.
- **Command mode:** exit codes come back from `wasi.start()` (0; 83 Lua filter error; 6 bad option; 1 missing input; 97 unknown data file; 251 for `+RTS -M5m`); `+RTS` in argv works; stderr needs a custom `Fd` (`ConsoleStdout.lineBuffered` drops an unterminated last line); the shim logs every path open unless `{debug:false}`; no `poll_oneoff` NOTSUP occurred; a nested `require` through `datadir/init.lua` and `../../filters` works; a user filter that `require`s and `io.open`s siblings matched native.
- **Timings (Node, module precompiled, fresh instance):** compile ~91 ms; docx ~425 ms, pptx ~490 ms, epub ~440 ms, typst ~305 ms; a bare docx without filters ~90-165 ms (Lua init is ~250-300 ms); linear memory afterwards ~46-50 MB.
- **Wasm Lua:** `os.getenv`, `os.date` (UTC), `os.time`, `io.open` read/write in the mounted tree, `pandoc.read`/`write`, `pandoc.system.make_directory`/`list_directory`/`with_working_directory`, the mediabag, `lpeg` work; `os.tmpname`, `io.tmpfile`, `io.popen`, `pandoc.pipe`, network `mediabag.fetch`, `require 'lfs'` fail; `os.execute` is a stub returning `true`; cwd is `/`.
- **Fatal without `/tmp`:** `pandoc.system.with_temporary_directory` with no `/tmp` in the mount is an uncatchable Haskell exception (exit 1); before PR #766 the vendored Lua reached it at `normalize/astpipeline.lua:49` for a typst raw-HTML table; #766 replaced that call, so the `/tmp` mount stays only for `mediabag.lua:21` and `runemulation.lua:143`.
- **Remote images:** docx/pptx/epub warn ("compiled without HTTP support") and fall back to alt text; typst fails hard (exit 83) via `modules/mediabag.lua:30` / `quarto-post/typst.lua:260`.
- **Locale:** unset `LANG` is fine for UTF-8; `LANG=C` breaks non-ASCII file names.
- **Projects:** `QUARTO_PROJECT_DIR` is not set natively either; `writeIndex` did not write `.quarto/crossref-index.json` in the docx project replay (unconfirmed why).
- **Pandoc data files:** embedded in the wasm and used when `--data-dir` lacks them (missing or empty `--data-dir` is silent, exit 0): `reference.docx`, `reference.pptx`, `epub.css`, `templates/default.{html5,typst,epub3,latex}`, `abbreviations`, `translations`, `default.csl`, `init.lua`, highlight styles. Not embedded: `sample.lua`, `styles/*.csl`. The vendored `datadir/` holds only Lua helpers (and trimmable `luacov*`/`profiler`), so docx/pptx/epub use pandoc's embedded defaults as native does. The mount must keep `<share>/pandoc/datadir` and `<share>/filters` in the same relative layout (`init.lua:233` adds `user_data_dir/../../filters/?.lua`) plus `<share>/formats/docx/*.png` (their absolute paths are in the params blob and the AST).

## 10. Pre-plan experiments (2026-10-01, review round 4; scratch code was discarded, scripts live in the session scratchpad)

- **Command mode in browsers** (Playwright 1.60 builds: Chromium 148 headless, WebKit 26.4; module Web Worker; shim 0.4.2, `{debug:false}`, `args_sizes_get` overridden; captured native docx with callout, table, crossrefs, `doc 数据.qmd` and `résumé.png`; share tree 255 files mounted at its original absolute paths under a `/` preopen). Docx byte-identical to native under `SOURCE_DATE_EPOCH` in both; exit codes 0/83/6/251 return from `wasi.start()`; a custom `Fd` keeps an unterminated stderr line (`ConsoleStdout.lineBuffered` loses it); no `poll_oneoff` calls over 15 runs; non-ASCII output and image names work, and a long non-ASCII argv traps without the UTF-8 `args_sizes_get` override (a normal fixture passes without it by luck); `_start` traps on a second call. The replay needs `argv[0]` (the wrapper log starts at `-f json`) and the empty dependency file (exit 83 without it). Chromium: wasm compile ~0.3 s, instance ~25 ms, first docx ~295 ms, later ~160 ms, bare docx run ~62 ms, linear memory ~53 MB with Lua and ~47 MB without; WebKit: compile ~1.2 s, instance ~125-300 ms, first docx ~0.6-0.8 s, later ~0.3-0.4 s. Not run: Firefox, SVG, pptx/epub/typst, hang injection.
- **wasm32 ungate** (`cargo check --target wasm32-unknown-unknown` in `crates/wasm-quarto-hub-client`, Homebrew llvm clang): ungating `pandoc_write`, `bundle` and `diagnostics` gave 8 errors (`ResourceError` and the `typst_compile` helpers); then `typst_compile` needed ungating, which hits `typst-gather` → `openssl-sys` (does not build for wasm32, so the stage stays native-gated); `tempfile` had to move to shared dependencies. About six edits, three error layers, then dead-code warnings until the helpers are split from the stage. `css-inline` 0.21 with default features off type-checks for wasm32.
- **typst.ts 0.7.0 in Node 24.5:** compile ~28 ms, `getModule()` with a precompiled `Module` works including inside a `worker_threads` Worker; `mapShadow` plus compile root `/` resolves images, `#import` and `#include`; wasm 28,325,178 bytes, 10,733,225 gzip -9, 7,099,357 Brotli -q11; first compile 0.19-0.45 s, later ~4 ms; PDF 36,188 bytes against native typst 0.14.2's 36,198 for the same `.typ` and fonts. Fonts, package registry, missing-package diagnostics and PDF dates are in H7 and H9. Not checked: that the wasm embeds typst 0.14.2 exactly, and browsers.
- **prepare/execute split sketch** (docx, `pandoc_write.rs` plus a new request module and `RenderContext` bridge): compiled, clippy clean, 344 pandoc/docx/epub/typst tests passed unchanged; the details are in R1.
- **Static review:** default-position (`Pre`) user filters run in pampa's Lua and only `Post`/entry-point filters in pandoc.wasm (`user_filters.rs:140-160`); the vfs-root gate leaves site-root image targets as written (`link_rewrite.rs:285-291`); the R0 wrapper is POSIX-only and `.gitattributes` pins `eol=lf` for one directory only.

## 11. H0 runtime gates (2026-10-01, host phase H0; harness `2026-10-01-pandoc-wasm-spike/h0/`)

Setup: pandoc 3.11 wasm (SHA-256 `44829227...`, 59,163,604 bytes, verified against `resources/pandoc-wasm.json`), shim 0.4.2 with `{debug:false}`, module Worker, `_start` on a fresh instance per run, R0's 24 recordings (6 fixtures x docx/pptx/epub/typst) mounted at their placeholder roots (`/__q2_share__`, `/__q2_tmp__`, `/__q2_doc__`) under a `/` preopen. Playwright 1.60, macOS: Chromium 148.0.7778.96 (headless) and WebKit 26.4. Firefox (below, run by hand by Gordon) and the Word check are manual.

- **Command mode works in both engines.** All 24 recordings exit 0 in Chromium and in WebKit, each twice (second pass = new instance, warm byte cache). The exnref probe, `WebAssembly.compile` (Chromium 127 ms, WebKit 163 ms from a localhost fetch of 24 ms) and `_start` need no flag. The wasm imports only `wasi_snapshot_preview1`.
- **Lua-under-wasm gate: 24 of 24 equal in both browsers.** `quarto-output-extract` reports each browser output equal to the native reference for all 6 fixtures x 4 formats; 17 of 24 are also byte-identical (docx except callouts, pptx, typst), the other 7 are callouts-docx (the only difference is the path inside the docx image `descr`: the native run was at `/tmp/q2-pandoc-replay/...`, the wasm one at `/__q2_share__`, normalized by the harness) and the six epubs (random `urn:uuid:`, xhtml attribute order, normalized by the extractor). Negative control: callouts vs tables outputs compare unequal. This repeats the Node result (§9) in browsers on the pandoc-profile pipeline with the full vendored Lua.
- **Exit codes** come back through `WASIProcExit` and match Node: 0 ok; 83 Lua filter error (dependency file removed); 6 unknown option; 1 missing input file; 251 for `+RTS -M8m -RTS` (heap exhausted; `-M5m` also exits 251 but with the "smaller than -A" message, and `-M12m` and up complete). The RTS options are honoured when they are in argv after `argv[0]`.
- **stderr**: a `ConsoleStdout` with a raw `write` callback captures everything including an unterminated last line (a Lua filter writing `no-newline-tail`: whole capture 15 chars, `ConsoleStdout.lineBuffered` returned 0 chars).
- **`poll_oneoff`**: wrapped on every run; **0 calls** over all runs in both browsers, so 0 non-zero returns (nothing in this workload sleeps or polls).
- **Non-ASCII**: with `args_sizes_get` overridden to UTF-8 byte lengths, an output name `数据 résumé.docx` plus a 600-character non-ASCII `--metadata title=` value runs and writes the file; with the shim's own `args_sizes_get` (UTF-16 units) the same argv traps with "memory access out of bounds" in both engines. The short non-ASCII output name alone passes with the shim's counts, so the override is only needed for long non-ASCII argv; keep it unconditionally.
- **`_start` twice on one instance traps** (`unreachable`) in both engines: one instance per run, as the plan assumes.
- **Latency, fresh instance, module precompiled (instantiate + run):** Chromium docx ~140 ms with the Lua filters vs ~66 ms bare, so the vendored filters\' startup is **~70 ms** (Node was ~250-300 ms); first run after load 293 ms (callouts-docx: instance 25 ms + 268 ms). Per recording run time (Chromium, `_start` only): docx 77-268 ms (median 90), pptx 108-176 (median 121), epub 66-154 (median 83), typst 67-133 (median 81). WebKit: instance 115-260 ms, first run 390 ms, medians docx 114, pptx 129, epub 99, typst 98 ms; Lua startup ~70 ms (bare ~185 ms vs ~250 ms with filters). Informational only (the reactor numbers are not a comparable baseline).
- **Memory** (`instance.exports.memory.buffer.byteLength` after the run): 47-49 MB with filters over all recordings (bare docx 44 MB), identical in both engines; a bad-option run is 42 MB.

Open questions settled:
- **`writeIndex` / crossref-index**: `crossref-writeIndex` sits in `quarto_crossref_filters`, which `main.lua:756` only appends when `crossref_numbering.assign_crossref_numbers()` is true; q2 renders with `crossref-numbering: external` (Rust numbers), so the group does not run, natively or under wasm. Separately, `crossref-index-file` is put in the params blob only for non-single-file projects (`pandoc_filters/params.rs:276`), and none of the 24 recordings has it. Replaying with the key added to the params blob wrote nothing natively (`pandoc-recording replay --env`, index path inside the doc dir; the run stayed byte-equal) and nothing under wasm (index path in a pre-created `.quarto/`, exit 0, empty stderr), which fits the gate above; I did not separately confirm that the override reached Lua. Wasm parity either way; nothing to do in H0. A multi-file project render (R7) must not rely on this file.
- **`mediabag_filter`** (`quarto-finalize/mediabag.lua`, used at `main.lua:598`): skipped for docx and pptx by format; for epub and typst it is a no-op unless `pandoc.mediabag.lookup(src)` finds an entry, which needs a fetched remote or data-URI image, none of which wasm pandoc can fetch. `mediabag-dir` in the params blob is `/__q2_doc__/all-docx_files/mediabag` (already a mounted root); no recording writes there. Remote images stay an R6 matter (§9).
- **Files the run writes**: besides `-o`, `/__q2_tmp__/pandoc-results.json` is written (by a filter), so `/__q2_tmp__` must stay writable.
- **SVG in docx (D9)**: wasm pandoc logs `[WARNING] Could not convert image ...: check that rsvg-convert is in path` (exit 0) and embeds the `.svg` as `word/media/rIdN.svg` with an `asvg:svgBlip` extension and an empty `<a:blip>` with no PNG fallback. **Word check (Gordon, 2026-10-01): `svg.docx` opens without a repair prompt and shows the text before, then the alt text where the image should be (no image), then the text after.** Per D9's default (alt text if Word opens the file), accept alt text for SVG in docx for now; rasterizing remains a later option. The files were `h0/out/chromium/svg-docx/svg.docx` (with an SVG image) and `png-control.docx` (same document with a PNG). 

- **Firefox 157 (macOS, real browser, by hand): same results.** 24 of 24 recordings equal to native (17 byte-identical, same 7 exceptions), exit codes 0/83/6/1/251 (`-M8m` 251, `-M12m` 0), unterminated stderr line kept (`lineBuffered` loses it), `poll_oneoff` never called, long non-ASCII argv works with the UTF-8 `args_sizes_get` override and traps without it, a second `_start` traps, memory 47-49 MB. Wasm compile 252 ms, first run 8 ms instance + 184 ms; run-time medians docx 111, pptx 142, epub 98, typst 98 ms; Lua startup ~90 ms (bare ~65 ms vs ~155 ms).

Not run: hang injection, a machine with less memory.


## 12. H3 parity net and browser measurements (2026-10-02, host phase H3)

Setup: the real chain (Rust `render_pandoc_request` in the built `wasm-quarto-hub-client`, share tree from Rust, `PandocRunner`, worker, pandoc.wasm 3.11) driven by the dev harness (`hub-client/src/pandoc/devHarness.ts`) in the `VITE_E2E=1` production bundle served by `vite preview` on localhost; Playwright 1.60 Chromium 148.0.7778.96 and WebKit 26.4, one Mac, other work running. The script is `hub-client/e2e/pandoc-measure.harness.spec.ts` (opt-in, `Q2_MEASURE=1`); raw JSON is not committed. Single runs on a shared machine: read the numbers as magnitudes. Localhost removes network time, so the download is arithmetic: the asset is 16,652,658 bytes, i.e. 13.3 s at 10 Mbit/s, 2.7 s at 50, 1.3 s at 100.

**Parity net.** The ten P7 docx fixtures render equal to their Q1 goldens (the `.snap` extraction text) in Node (`goldenParity.wasm.test.ts`) and in Chromium (`pandoc-parity.harness.spec.ts`, saved through Playwright's `download` event, the suggested file name checked). The one accepted-divergent fixture (mermaid) is compared with native q2's output instead (`pandoc-goldens/parity/`): wasm equals native there too. A changed reference makes the test fail (mutation-checked).

**Load and render latency** (callouts.qmd to docx, 252 share-tree files, 1.34 MB):

| | Chromium | WebKit |
|---|---:|---:|
| first click, empty cache (localhost): runner total | 640 ms | 969 ms |
| of which: download / unpack + SHA-256 / compile | ~97 / ~83 / ~130 ms | ~71 / ~44 / ~162 ms |
| cached copy, module not resident (reload): runner total | 472 ms | not measurable (see below) |
| module resident, fresh instance (median of 6) | 165 ms | 299 ms |
| of which: instantiate / run | 22 / 138 ms | 123 / 165 ms |
| Rust request build, warm (the first click is ~50 ms) | 4 ms | 7 ms |
| mount ~252 files into a fresh instance (`mountMs`) | 1 ms | 1 ms |
| filter chain: one-paragraph docx `runMs` vs trivial-filter `runMs` | 80 vs 10 ms | 96 vs 69 ms |
| pandoc linear memory after a callouts docx | 51.2 MB | 51.2 MB |

So the Node baseline (0.3-0.5 s per render, ~250-300 ms of it Lua) is beaten: Lua/filter startup is ~70 ms in Chromium (H0: ~70 ms) and ~27 ms over a slower base in WebKit; mounting the share tree is not a cost (D2a holds). The first run after a load is slower than a warm one (run ~285 ms vs 138 ms in Chromium). With the asset cached the click-to-docx time is under half a second in Chromium; the first ever click adds the download (16.7 MB).

**WebKit and the Cache API.** Under Playwright, WebKit's Cache API entry is gone after a navigation to the same origin, before any of our code runs (a probe listed the cache empty and `usage` 59 MB), so every reload re-downloads. This is the automation context's ephemeral storage, not the loader (the entry was present after the first load and the same code caches in Chromium); real Safari (persistent storage, ITP's 7-day cap on script-writable storage) needs the by-hand WebKit check, which belongs with the other manual browser checks.

**Image-heavy documents** (12 x 24 MB, 10 x 10 MB and 4 x 5 MB PNGs of incompressible noise, one document each; process memory is the summed RSS of the Playwright browser processes, sampled every 100 ms, growth measured from after the images were seeded into the page's VFS and garbage-collected):

| Images (total) | docx | pandoc linear memory | run | Chromium RSS growth | WebKit RSS growth |
|---|---:|---:|---:|---:|---:|
| 4 x 5 MB (21 MB) | 21 MB | 92 MB | 0.67 s / 0.77 s | +0.43 GB | +0.14 GB |
| 10 x 10 MB (105 MB) | 105 MB | 213 MB | 2.6 s / 2.6 s | +1.46 GB | +1.15 GB |
| 12 x 24 MB (302 MB, at the 300 MB total limit) | 302 MB | 853 MB | 8.1 s / 6.8 s | **+5.1 GB** | **+3.2 GB** |

(Runner totals 0.95/1.3 s, 2.8/3.1 s, 8.4/7.3 s, Chromium/WebKit; the request build is 0.07-0.9 s of it.) Pandoc's own memory is ~2.8x the payload and is the small part; the browser process grows 11-17x the payload, from the copies the chain makes (VFS in the Rust wasm, the request's `Uint8Array` copies, the worker's tree, the output file, the transferred result) plus the heap those leave behind. The WebKit baseline in these runs was inflated by earlier steps (RSS 5-6 GB before the render), so only its growth is meaningful.

**Limits against the numbers (a miss goes to a human, not a silent change).**
- 120 s wall timeout: the largest legal job takes 8.4 s, 14x headroom; even at 5x slower hardware it fits. No change proposed.
- Image limit 25 MB each: a 24 MB image renders. Fine.
- **Total mounted bytes 300 MB: a miss on memory, not on time.** It renders, but a job at the limit grows the browser process by 3-5 GB, so a device with 4 GB of RAM would lose the tab long before the host's limit rejects the job. Decision needed from a human; the candidates: lower `limits.total_bytes` (the number is shared with Rust, R1/R2, so it is a constants-file change plus the mirrored tests; ~100 MB would cost +1.1-1.5 GB), or keep it and treat large-image documents as desktop-only. Nothing was changed.
- **Decompression-ratio guard: set to 8x** (`MAX_DECOMPRESSION_RATIO` in `pandocLoader.ts`), from the measured 3.55x (59,163,604 B from 16,652,658 B). The loader reads the gunzipped stream with a cap and cancels it past 8x the compressed size, so a gzip bomb or a wrong file is rejected before it is buffered whole, uncompiled and uncached.

**Memory budget.** Loading pandoc for the first time (Chromium, one small docx, including initialising the hub's own Rust wasm if the page had not yet): process RSS 448 MB before, 1.02 GB at the peak (+0.57 GB: download buffers, 59 MB wasm, SHA, compile, the worker's 51 MB instance); WebKit 640 MB to 2.03 GB (+1.4 GB). The resident `Module` is dropped after 5 minutes idle. Budget to state in the UI copy and to design H5/H6 around: **~0.6 GB (Chromium) to ~1.4 GB (WebKit) transient for a first download, then ~50 MB per live render plus ~11-17x the size of the images in the document.** The epic's earlier "~+340 MB page RSS once pandoc loads" (reactor mode) is below these; the measured numbers replace it.

**Not measured.** Firefox (manual; `spike/ff/`). The typst, pptx and epub recordings through the harness wait for R4/R5 (the plan's own condition). `performance.measureUserAgentSpecificMemory` needs cross-origin isolation, which the app does not have, so RSS is the memory measure.

## 13. H6 memory sweep and browser matrix (2026-10-02, host phase H6)

Same setup as section 12 (dev harness, `VITE_E2E=1` bundle, `vite preview`, one Mac with other work running, Playwright Chromium 148.0.7778.96 and WebKit 26.4), re-run with the sweep widened to the candidate values for `limits.total_bytes` and with the main thread's Rust wasm memory added (`window.__quartoTest.pandoc.rustWasmMemoryBytes()`, `memory.buffer.byteLength`). Spec: `hub-client/e2e/pandoc-measure.harness.spec.ts` (opt-in, `Q2_MEASURE=1`). Single runs: magnitudes, not benchmarks.

| Images (total) | pandoc linear memory | Rust wasm memory after (never shrinks) | Chromium RSS growth during the render | run (total) |
|---|---:|---:|---:|---:|
| 4 x 5 MB (20 MB) | 88 MB | 130 MB | +0.34 GB | 1.0 s |
| 10 x 10 MB (100 MB) | 203 MB | 494 MB | +1.54 GB | 3.6 s |
| 6 x 24 MB (144 MB) | 446 MB | 729 MB | +2.02 GB | 5.2 s |
| 8 x 24 MB (192 MB) | 478 MB | 921 MB | +2.93 GB | 6.7 s |
| 12 x 24 MB (288 MB, the 300 MB limit) | 814 MB | 1338 MB | +4.75 GB | 11.4 s |

Both wasm memories are identical in WebKit and Chromium (they are the module's own). WebKit's RSS growth is not reported: its baseline was 5-7 GB in this run (RSS summed over every `ms-playwright` process) and the growth column came out non-monotonic (+0.8 GB at 144 MB, +0.4 GB at 192 MB), so only the section 12 magnitude (+1.2 GB at 100 MB, +3.2 GB at 300 MB) stands.

What the numbers say:
- **The browser process grows about 14-16x the image payload during a render, linearly** (14.7x at 100 MB, 15.7x at 288 MB). Lowering `total_bytes` buys memory in proportion: 100 MB is about +1.5 GB, 150 MB about +2.0 GB, 200 MB about +2.9 GB, 300 MB about +4.7 GB.
- **Worker memory is already recycled.** The runner makes one worker per render and terminates it when the render settles (`pandocRunner.ts`), and each render instantiates pandoc anew, so pandoc's 2-3x lives only for the render. Nothing in the worker needs a recycle policy; the compiled module is dropped after 5 minutes idle.
- **The Rust wasm on the main thread is the part that stays.** Its linear memory grows to about 4.4-4.7x the payload during request building (copies of the files on the way out) and cannot shrink, so after one 100 MB render the page keeps about 0.5 GB until it is reloaded, and after one 288 MB render about 1.3 GB. It plateaus: a second and third render of the same document add nothing (`pandoc-memory.harness.spec.ts`). It cannot be recycled without re-initialising the module that holds the VFS and project state, so no recycle policy is implemented; the lever is the payload limit, or fewer copies in request building (a Rust change, R-lane territory).
- The JS heap plus ArrayBuffer backing stores (CDP `Runtime.getHeapUsage` after a forced GC, Chromium) is 26 MB before and after three 100 MB renders: a finished render retains no copy of the images or the output on the JS side.

**The CI test** (`hub-client/e2e/pandoc-memory.harness.spec.ts`, Chromium and WebKit): ten 10 MB images, three renders; asserts pandoc linear memory <= 64 MB + 3.5x payload (measured 1.9x), Rust wasm memory <= 128 MB + 6x payload (measured 4.7x), the docx carries the payload, no growth in either memory between render 1 and render 3, and (Chromium) the JS heap plus backing stores within 64 MB of the pre-render reading after GC.

**Failure-taxonomy audit** (design, Failure taxonomy; all ten classes have a diagnostic, a UI state and a test): two gaps found and closed. (1) `WebAssembly` missing altogether (iOS Lockdown Mode, a browser policy) threw a `ReferenceError` out of the loader and was classified as a failed download; it is now `no-wasm` with its own message and the `unsupported` UI state. (2) A memory failure on the main thread (the Rust request build, or the hand-off to the worker) threw into the controller's catch-all and showed as `crashed`; it is now `out-of-memory` when the error looks like an allocation failure (`looksLikeOom`, now exported by `@quarto/pandoc-host`). A third item is documentation only: after a main-thread Rust wasm trap the module is not usable until the page reloads.

## 14. H7 typst worker: download budget, memory, prior art (2026-10-02, host phase H7)

### First-use download against the 40 MB budget

The budget is `first_use_download_gzip_bytes` = 41,943,040 (40 MiB) in `resources/typst-wasm.json`, set before measuring. Exact sizes of the files the browser fetches (`stat`, the `.gz` files in `hub-client/public/`):

| Piece | Gzip bytes | MiB |
|---|---:|---:|
| `pandoc/pandoc.wasm.gz` | 16,652,658 | 15.88 |
| `typst/typst.wasm.gz` (typst.ts 0.7.0 compiler) | 11,068,415 | 10.56 |
| `typst/fonts.bin.gz` (typst-assets 0.14.2 default fonts, 8,757,072 raw) | 5,857,677 | 5.59 |
| `get_typst_assets()` export (41 files, 2,497,763 raw), gzipped as one stream in the page | 862,794 (Chromium) / 877,834 (WebKit) | 0.82 |
| **Total** | **34,441,544** | **32.85** |

That is 7.5 MB (18%) under the budget. Not in the sum: `index.json` (2.3 MB), which is fetched only if a use needs it and otherwise has left the budget; the `pdfjs-dist` pair (~0.5 MB, H9's lazy fetch); and the Rust and Automerge wasm modules, which load with the app. The export row is the Rust wasm's own payload (it ships inside the Rust wasm, so it is not a separate request); it is counted because the budget text counts it. Gzip of the concatenated bytes, not a tarball, so it is a close estimate (`tar | gzip -9` of `resources/typst-packages` is 860,826).

### Latency and memory (Chromium 148.0.7778.96, WebKit 26.4)

Spec: `hub-client/e2e/typst-measure.harness.spec.ts` (opt-in, `Q2_MEASURE=1`; same setup as sections 12 and 13: `VITE_E2E=1` bundle, `vite preview`, a Mac with other work running). A 13-page document (table, math, a vendored package) through the production runner. Single runs; magnitudes, not benchmarks. Latency is local, so the first-use number has no network in it.

| | Chromium | WebKit |
|---|---:|---:|
| first compile, fresh context (wasm + fonts fetched, compiled; total) | 354 ms | 383 ms |
| cached compile after reload (Cache API holds the gz; module gone) | 352 ms | 1077 ms |
| warm compile, median of 5 (module resident, fresh worker) | 67 ms | 87 ms |
| Rust wasm memory after (shared with the pandoc measurements) | 32 MB | 32 MB |

Process RSS (summed over every `ms-playwright` process; the harness's browser keeps earlier contexts\' memory, so read the deltas, not the absolute values):

| Step | Chromium start -> peak | WebKit start -> peak |
|---|---:|---:|
| first typst compile (after boot) | 624 -> 839 MB | 868 -> 1286 MB |
| cached typst compile after reload | 879 -> 1079 MB | 1364 -> 1549 MB |
| pandoc docx render first (Rust wasm booted) | peak 1286 MB (boot 1057) | peak 2385 MB (boot 1512) |
| typst compile with pandoc and Rust resident | 1234 -> 1273 MB | 2386 -> 2431 MB |
| second pandoc, then second typst, all resident | 1331, then 1279 MB | 2436, then 2437 MB |

What the numbers say:
- **A typst compile adds about 200-400 MB on its first use and a further 40-50 MB when pandoc and the Rust wasm are already resident**, in both engines; the second round of each adds nothing. The compile worker is short-lived (one per compile, terminated when it settles), so its compiler instance is not retained; the compiled `Module` stays on the main thread for the idle window.
- **All four modules in play (Rust, Automerge, pandoc, typst) fit comfortably** at these sizes: peak 1.3 GB Chromium, 2.4 GB WebKit for the whole browser including the harness. WebKit's baseline is higher on this Mac for the same pages; no WebKit-only failure appeared in the 13-page run, and Safari-class memory pressure on a phone is not covered here.
- WebKit's cached compile is slower than its first (1077 vs 383 ms): the Cache API read and decompress of 17 MB, not the compile (134 ms).

### CSP and registry notes

- `connect-src https://packages.typst.org` is allowed in `crates/quarto-hub/src/server.rs` (Gordon's change, 2026-10-02, with a test). `script-src` has no `wasm-unsafe-eval` and it was not changed. Whether the hub's policy blocks `WebAssembly.instantiate` for the pandoc and typst modules is unverified: the harness runs against `vite preview`, which sends no CSP, so no test here exercises it. Check it against a real hub before shipping.
- Tarballs are pinned by exact version in the import spec only. `packages.typst.org` publishes no hashes, so there is nothing to check a download against; a hash would have to be vendored by us per package, which only works for the vendored five (already in the Rust assets).

### Prior art: pandoc's own web app (`~/src/pandoc/wasm/index.js`, 3.9+)

How it makes a PDF: pandoc writes `typst` (standalone) in the wasm; the page then runs `$typst` (`@myriaddreamin/typst-all-in-one.ts@0.7.0-rc2`, loaded lazily from jsDelivr as a script on the main thread), calls `resetShadow()`, `mapShadow` for every input file under `/<path>` and `<path>`, maps the `.typ` at `/main.typ` and calls `$typst.pdf({mainFilePath})`. Errors are scraped from the typst.ts message string with a regex. Nothing else: default fonts and packages come from typst.ts's own CDN defaults.

What changes in our design:
- **Nothing structural.** Same shape as ours (pandoc then typst.ts, `map_shadow`), same typst.ts line (we pin 0.7.0 final and its wasm sha256); ours differs by design where the web app cannot go: a worker per compile with a wall timeout, vendored fonts and packages (offline, no CDN at run time), a checksummed asset script, structured diagnostics.
- **Images (pandoc#11584).** Pandoc's wasm cannot write the typst writer's extracted images into a temp directory, so the web app injects a Lua filter that rewrites every mediabag image to a `data:` URI before the typst writer runs. Our request mounts `resource_refs` at the same absolute paths for both stages (H8), so we do not need it; it is the fallback if an image that goes through the mediabag (remote or embedded) fails to resolve in the typst stage. Noted for H8's image tests.
- The web app has no package-registry or font-list handling, so nothing there informs `typst_available_fonts`.
