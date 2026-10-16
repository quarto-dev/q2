# Plan 7: document import (docx/odt/rtf/epub/pptx to qmd) in the `q2 preview --ui editor` embed

Part of the epic [`2026-10-08-q2-preview-hub-parity-epic.md`](2026-10-08-q2-preview-hub-parity-epic.md) (E6); the execution model and landing procedure are there.

## Execution

- **Lane:** C (`workspace-3`), binding: one agent per plan, in this order: Plan 1, Plan 4, Plan 7, Plan 8, Plan 11. This plan is number 3 of 5 in the lane.
- **Waits on:** Plan 1 merged into `feature/q2-preview-hub-parity` (the shared child runner and the public `resolve_and_gate_pandoc`). Nothing inside this plan waits on another plan. The import end-to-end and `--join` checks are in Plan 13 (they need the embed harness).
- **Start gate:** every plan under "Waits on" is merged. Check from your worktree: `git -C ../workspace-7 branch --merged feature/q2-preview-hub-parity | grep -q 'q2-preview/p01-child-runner' && echo ready`. If it does not print `ready`, do not start: report which plans are missing and stop (plan branches are never deleted after landing, so `--merged` is the test). Before coding, read the Handoff log at the end of each plan you wait on.
- **Branch:** `q2-preview/p07-embed-import`, cut from the tip of `feature/q2-preview-hub-parity` when you start.
- **Merges to:** `feature/q2-preview-hub-parity`, by the landing procedure in the epic.
- **Unlocks:** Plan 13.
- **Next in lane:** Plan 8 (`2026-10-08-q2-preview-p08-static-pdf-viewer`); the next agent starts only when its own start gate passes.

## Goal

The Import button, file picker, drop handling and placement dialog work in `q2 preview --ui editor` exactly as in hub-client, with native pandoc doing the reading.

## Background facts

- `importAvailable() = pandocWasmEnabled() && !isPreviewEmbed()` (`importService.ts:291-293`), so the feature is hidden in the embed (`Editor.tsx:504,1230,1598`); `importService.test.ts:294-305` asserts exactly that. The embed is built with `VITE_PANDOC_WASM=0` (`hub-client/package.json:17`), so `pandocWasmEnabled()` is false there too: dropping `!isPreviewEmbed()` alone would not enable the feature.
- The flow is split: the hub Rust wasm owns the request, paths, format table, transforms and the qmd (`prepareImport`, `finishImport`, `getImportFormatTable`, `classifyImportFailure`); pandoc.wasm does one thing, `deps.runner.run(prepared.request, prepared.share_tree, {inputs: {[source_path]: bytes}})` (`importService.ts:252-254`). The embed ships the hub Rust wasm, so only that runner call lacks an implementation. The seam: `ImportDeps.runner` is `{run(request, shareTree, options?) => Promise<RunOutcome>}` (`:139-141`), `createImportService(deps)` is injectable, `defaultDeps` wires `getImportRunner().run` (`:300`).
- `import_argv(format, source_path)` (`import/request.rs:85`) hardcodes the wasm's virtual paths (`import_dir()` = `/__q2_share__/import`, `request.rs:51-66`, called at `:137`); `prepare_import` (which returns only the format id string) builds `collect_dirs: vec![extract_dir()]` (`:158`); `format_for_file_name` is `pub` and gives the `&ImportFormat` the argv builder needs; `rewrite_image_targets` hardcodes the extract prefix (`media.rs:192`) and raises fatal Q-24-12 for an unmanifested virtual-prefix image (`:200,209-211`). A second, parameterized argv builder exists in `crates/xtask/src/capture_import_recordings.rs:126` with a different signature; xtask has no `quarto-core` dependency (`xtask/Cargo.toml:13-31`), and `quarto-core/tests/integration/import_request.rs:3` already asserts the core builder equals every recording's `argv.json`, so drift is caught.
- Source files can be 25 MB (I19). Extracted media must return as bytes; EMF/WMF-to-SVG, the 10 MB image rule and content-hash naming are TS-side (I8/I12/I16). `RunSuccess` = `ExecuteSuccess & {notices}` with `output`, `outputPath`, `collected`, `stderr`, `stdout`, `diagnostics` and `stats{mountMs, instanceMs, runMs, memoryBytes, mountedBytes}` (`pandocRunner.ts:46-62`).
- `resolve_and_gate_pandoc` (made public by Plan 1) returns `PipelineError` and spawns `pandoc --version` on every call; the route accepts that.
- `axum` 0.8 (`quarto-preview/Cargo.toml:13`); `DefaultBodyLimit` per route is fine and unused elsewhere (axum's implicit 2 MB applies to other routes).
- **Spike results (native pandoc 3.11).** Native output equals the recorded output for all 11 import fixtures (`crates/quarto-core/tests/fixtures/import-recordings/`: docx x9, epub, odt, rtf, pptx) after rewriting `<tmp>/media` to `/__q2_share__/import/media` (JSON, media sha256 and stderr identical; `corrupt-docx` exits 63); the recordings are native captures, so native-versus-wasm parity rests on the repo's wasm tests (not run). **`--sandbox` is mandatory:** without it a crafted epub makes pandoc copy `/etc/hosts` into the extracted media and fetch `//127.0.0.1:<port>/x.png`; with it neither happens, and it works with all five readers plus `--extract-media`. A docx with external image relationships is inert either way. `+RTS -M512M -RTS` works on all fixtures; at the cap pandoc exits 251 with `Heap exhausted`; a 1.6 MB docx expanding to about 360 MB hit it in 3.4 s at about 647 MB RSS (about 25% over the cap), so a wall-clock timeout is also required. Build argv as separate arguments.

## Decisions

- **P7-D1** An allow-listed import route (a generic "run this `PandocRequest`" route is rejected: pandoc's `--filter`/`--lua-filter` would give every `--share` guest code execution on the host). `POST /api/preview/import` takes the source bytes and `file_name` (used only to select the format by extension); the server calls `prepare_import` and a directory-parameterized `import_argv` itself and runs native `pandoc` in a per-request temp dir removed afterwards. Mandatory hardening, appended by the route after the shared builder (not part of it: it would change the recordings\' `argv.json` and the wasm path's request job id): `--sandbox`; `+RTS -M512M -RTS` (a distro pandoc built without RTS options would reject it: fail with a clear message, UNVERIFIED); a wall-clock timeout; kill-on-abort; a cap on total returned media bytes; the pandoc version floor (`pandoc_filters/version.rs:76-100`). The child runs through the shared `quarto-core` runner (E7) inside `spawn_blocking`, with a cancel flag set by a drop guard on the handler future. The route validates the upload with `prepare_import(file_name, size, "")` (validate-only), takes the `&ImportFormat` from `format_for_file_name`, and does not build the wasm request. On Windows `canonicalize` yields `\\?\` paths: strip that prefix before building argv and the remap needle (UNVERIFIED; test with both forms).
- **P7-D2** Transport: the request body is the source bytes; the `file_name` header is the basename of the runner's `source_path` (`source.<ext>`: the runner interface carries no original file name, and the server uses the name only to select the format by extension), so it is always ASCII and needs no encoding; the response is `application/octet-stream` framed `[u32 header_len][JSON header][concatenated payloads]`, the header holding `{status, stderr, stdout, outputLen, collected:[{path, len}]}`; the body limit rises to the I19 cap (25 MB) plus slack on this route only.
- **P7-D3** Diagnostics parity: the route returns pandoc's stderr and exit status so `classify_import_failure` (wasm) writes the user-facing text; the runner invents none. The route reports pandoc's raw exit status and stderr plus `timedOut` and `aborted` flags; the client runner (7.10) does the one mapping onto `ExecuteFailureKind` (exit 251 with `Heap exhausted` to oom; `timedOut` to `timeout`; `aborted` to `aborted`; other exits to the pandoc-exit and crash kinds). A body over the cap is 413. Missing pandoc is `Q-20-1`, too old `Q-20-2` (`Q-21-*` are typst). `failureOutcome` (`importService.ts:205-221`, `ImportDialog.tsx:314-318`) offers no kind that shows runner diagnostics cleanly: `invalid-request` and `superseded` prepend `Q-24-12`; every other kind, including `timeout`, discards `out.diagnostics` for `classify`; `load-failed` and `worker-blocked` show them but `ImportReport` also prints `download.failed[uiState]` (a misleading "download failed" headline when `loadError` is absent). So task 7.10 adds a dedicated diagnostics-only failure path (no `uiState` headline) for the `Q-20-*` and `Q-24-2` cases. Dropped collected files carry `collect-limit` diagnostics with `path` and `size`, using the wasm limits (`ts-packages/pandoc-host/src/limits.ts:24-25`: 26214400 per file, 314572800 total).
- **P7-D4** The dialog's cancel kills the child and maps to `aborted`. Missing or too-old pandoc shows the existing `Q-20-*` message, as for downloads.
- **P7-D5** (derived from the spike; Gordon may veto) The route runs pandoc with the real request directory in argv (canonicalized first, as the xtask does: macOS `/var` versus `/private/var`), then replaces the whole directory string with the virtual `import_dir()` everywhere pandoc output reaches a consumer: string values of the parsed JSON (not the raw text, where Windows paths hold `\\`), stderr (which becomes `Q-24-4` warning text in `finish_import`), stdout and collected-file paths. It fails closed: if any occurrence of the real directory survives, the route errors rather than returning it (the xtask's `relocate` does the same, `capture_import_recordings.rs:~138-147`). `finish_import` and the TS manifest builder stay unchanged, and a `--join` guest never sees the host's temp path.

## Tasks

Per task: `cargo clippy -p <crate> --all-targets -- -D warnings` plus `cargo nextest run -p <crate>`; `tsc` plus `vitest` in `hub-client`. Workspace nextest at the end of the plan. New Rust integration tests go in `crates/<crate>/tests/integration/`.

### Part A: baseline and remaining checks

- [x] 7.1 Baseline: take the workspace pass/skip counts recorded for the current integration tip (epic, "Workspace nextest ledger"); run `cargo nextest run --workspace` only if the tip has no recorded entry, and then say so in the Handoff log.
- [x] 7.2 Confirm which parts of the import epic (P0-P6, close-out) are in this branch (the plan assumes P0-P5: `crates/quarto-core/src/import/`, `hub-client/src/pandoc/importService.ts`, `ImportDialog.tsx`, commits `f116b3e59`..`f541ebbe7`).
- [x] 7.3 Run `finish_import` on a native-run result (the spikes compared JSON and media only) and confirm acceptance.
- [x] 7.4 Run `pandocHost.wasm.test.ts` to settle native-versus-wasm reader parity.
- [x] 7.5 Extend the crafted-input matrix with HTML, SVG and font references inside epubs, with `--sandbox`.

### Part B: quarto-core and the server route

- [x] 7.6 `import_argv_in(format, source_name, dir)` in `quarto-core`; `import_argv` delegates with `import_dir()`; the xtask's own builder stays, untouched (its tests `:681-697` and the recordings stay valid). The shared builder adds neither `--sandbox` nor `+RTS`. Tests: argv for each format with a virtual and a real directory; the existing `import_request.rs` recording check stays green.
- [x] 7.7 `crates/quarto-preview/src/import_route.rs` (new, sibling of `render_download.rs`; registered next to `/api/preview/render`, `lib.rs:737`): a per-route `.layer(DefaultBodyLimit::max(..))`; a per-request temp dir cleaned up by a guard inside the blocking closure; argv from the shared builder, then `--sandbox` and `+RTS -M512M -RTS`; the Plan 1 child runner with deadline and cancel, taking the pandoc path and timeout as parameters of the blocking function (the test seam; no `QUARTO_PANDOC` env racing); media collection with the byte caps (stat before read, no symlinks; the per-file and total limits and the sort order duplicate `ts-packages/pandoc-host/src/limits.ts:24-25` and `shared.ts:72-95`, so a test pins them); the P7-D5 remap; the P7-D2 framing.
- [x] 7.8 Handler tests with real fixtures: docx with an image; 400 for an unsupported extension; 413 over the cap; missing pandoc; a crafted epub (`file:` and `//127.0.0.1` references) yields no host data; an expanding docx hits the heap cap (the cap is a parameter of the blocking function, like the pandoc path and timeout, so the test uses a small docx expanding past a `-M64M` cap rather than a 650 MB one); `native_media_paths_are_remapped_to_virtual_prefix`; the remap covers stderr and stdout and a surviving real path is an error; timeout and abort tests use a fake pandoc script through the injected path (a `#[cfg(unix)]` script helper with a `#[cfg(not(unix))]` counterpart), not a real pandoc race.
- [x] 7.9 `tests/integration/import_route.rs` + `main.rs`: real server and pandoc; round-trip the fixtures and compare with the recordings.

### Part C: hub-client

- [x] 7.10 `importAvailable()` is `isPreviewEmbed() || pandocWasmEnabled()` (true in the embed although `VITE_PANDOC_WASM=0`; update `importService.test.ts:294-305`); `hub-client/src/pandoc/nativeImportRunner.ts` implements `ImportDeps.runner` through the route and fills `RunSuccess` (zeros for the stats fields); failures map per P7-D3 (the single exit-status mapping lives here) and use the new diagnostics-only failure path in `importService.ts`; `defaultDeps` picks the native runner when `isPreviewEmbed()`. Tests: `parses frames and rejects truncated bodies`; each status mapping; abort; `Q-20-*` and `Q-24-2` render on their own.
- [x] 7.11 Progress: the runner calls `options.onStage('running')` when it posts (maps to `converting`); `loading-pandoc` never stalls the dialog (`ImportDialog` test); `onLoadProgress` is unused.
- [x] 7.12 Vitest: import service with the native runner and a fake fetch (including the changed `importService.test.ts` assertion); `Editor` shows the button in embed mode; drop handling unchanged.
- [x] 7.13 hub-client changelog entry.
- [x] 7.14 Plan end: workspace nextest against 7.1; reconcile this checklist; write the Handoff log; land per the epic.

## Risks

- Native pandoc may differ slightly from pandoc.wasm in reader output (both 3.11; native floor 3.11).
- Memory: image bytes in the response (the browser path measured 14-16x payload growth; native avoids it on the client, not the framing cost).
- RSS can exceed `-M` by about 25%; the timeout is the backstop.

## Handoff log

Landed on `q2-preview/p07-embed-import`: four commits on top of `c4ce01a3c` (rebased once, no conflicts): `import_argv_in` (7.6), the route and its tests (7.7-7.9), the hub-client runner (7.10-7.12), the changelog (7.13).

**Interfaces (as shipped)**
- `quarto_core::import::request::import_argv_in(format: &ImportFormat, source_path: &str, dir: &str) -> Vec<String>`; `import_argv` delegates with `import_dir()`. No `--sandbox` or `+RTS` in it.
- `POST /api/preview/import` (`quarto_preview::import_route`, `BODY_LIMIT` = 25 MB + 64 KB on this route only). Request: body = source bytes, header `file_name` = name whose extension picks the format. 200 body: `[u32 BE header_len][JSON {status, stderr, stdout, timedOut, aborted, outputLen, collected:[{path,len}], diagnostics}][output][media...]`. Refusals carry `{diagnostics:[...]}`: 400 (`Q-24-1`), 413 (`Q-24-2`), 422 (`Q-20-1`/`Q-20-2`). Past the body limit axum's own 413 has no body. Hardening: `--sandbox`, `+RTS -M512M -RTS`, 120 s timeout, kill on handler drop, the wasm collection caps (a test pins them to `limits.ts`).
- hub-client: `nativeImportRunner.ts` (`createNativeImportRunner`, `parseFrame`); new `RunFailureKind` `'refused'` (maps to `invalid-request` in `uiStateFor`; the import service shows its diagnostics with no `uiState`). `importAvailable()` = `isPreviewEmbed() || pandocWasmEnabled()`; `defaultDeps` uses the native runner in the embed.

**Deviations / notes**
- 400/413/422 bodies are JSON diagnostics (the plan left 400/413 as text) so `Q-24-1`/`Q-24-2` render from Rust's wording.
- Missing-pandoc is tested through an injected resolver closure, not a real absent pandoc.
- 7.3: a native run through `finish_import` reproduces every fixture's `expected.qmd`. 7.4: `pandocHost.wasm.test.ts` 59 passed. 7.5: the crafted epub now has HTML, CSS (font-face, import) and SVG references to a host marker file and loopback; removing `--sandbox` makes it fail.
- No test renders `Editor` in embed mode; the wiring is `importEnabled = importAvailable()`, covered via `importAvailable()` and the sidebar tests.
- Windows `\\?\` stripping and separator handling are unit-tested on strings only (UNVERIFIED on a real Windows pandoc).
- Plan 13 owns the end-to-end and `--join` checks.

Baseline: c4ce01a3c, 16033 passed, 202 skipped (Plan 2's entry). Delta +20 passed, 0 skipped ( = 1 quarto-core, 17 route unit tests, 2 integration). Also green: wasm32 check, hub-client vitest 1622 + integration 218, changelog wasm test.

Ledger: 2c023038a: 16053 passed, 202 skipped
