# pandoc.wasm host: local setup and tests

The host core is `ts-packages/pandoc-host` (`@quarto/pandoc-host`): DOM-free,
`execute(request, shareTree, { module, fault? })`, one fresh WASI instance per run
(command mode, `_start`). hub-client owns the loader, the worker shell and the UI.

## Loader and worker lifecycle (hub-client `src/pandoc/`)

| File | Role |
|---|---|
| `pandocLoader.ts` | main thread: fetch `pandoc/pandoc.wasm.gz` (absolute URL from `document.baseURI`), Cache API, `DecompressionStream`, SHA-256 of the *decompressed* wasm against the request's `expected_pandoc_wasm_sha256`, compile once, keep the `Module` resident (dropped after 5 min idle). Every environment dependency is injectable (`LoaderEnv`) |
| `pandocRunner.ts` | one short-lived worker per render, the `Module` posted in; abort = terminate; 120 s wall timeout; a new `run()` supersedes the old; typed `RunOutcome` and `uiStateFor` (one UI state per failure class) |
| `pandoc.worker.ts` | thin shell over `createHandler`. The name matters: `pandoc.worker-<hash>.js` matches the PWA `globIgnores` and the `ondemand-assets` route, keeping it out of the precache |
| `pandocService.ts` | the app-wide loader + runner (`getPandoc()`) |
| `smokeJob.ts` | a tiny markdown-to-plain request for tests of the lifecycle (no format behaviour) |

Behaviours worth knowing: the cache key carries the SHA (`<url>?sha256=<sha>`) so a stale entry is
never used and old keys are evicted after a verified write; the body is sniffed (`1f 8b` gzip,
`\0asm` raw wasm) because some servers add `Content-Encoding: gzip` to `*.gz`; a load in flight is
shared by the next click and aborted only when no waiter is left; `Cache` problems proceed
uncached with a notice, but a missing `DecompressionStream`, `crypto.subtle` or exnref is an error with
its own message; `navigator.storage.persist()` is requested after the first verified write.

The wall timeout and idle period are parameters (`RunnerConfig`, `LoaderConfig`), so tests shorten them.
The `VITE_E2E` page hook is `window.__quartoTest.pandoc` (`src/test-hooks.ts`).

## Getting the wasm

```bash
node scripts/fetch-pandoc-wasm.mjs            # skips with a message if it cannot fetch/verify
node scripts/fetch-pandoc-wasm.mjs --require  # fails instead (CI, `cargo xtask verify`)
node scripts/fetch-pandoc-wasm.mjs --from-wasm /path/to/pandoc.wasm   # offline: SHA-checked copy
```

It downloads the release zip named in `resources/pandoc-wasm.json`, verifies the zip
SHA-256 and the decompressed wasm SHA-256, extracts with `fflate` (no `unzip`), and writes
`.cache/pandoc-wasm/pandoc.wasm` (what vitest reads) and
`hub-client/public/pandoc/pandoc.wasm.gz` (the served asset). Both are gitignored.
`npm run test:wasm` runs it (non-`--require`) as `pretest:wasm`.

## Shipping the asset (H4)

`hub-client/public/pandoc/pandoc.wasm.gz` is served from `/pandoc/` (outside `assets/`, so the
service worker's `/assets/*.wasm` route never sees it). Build entry points and the asset:

| Entry point | Behaviour |
|---|---|
| `npm run build:all` (production, built by `quarto-hub-deployment`), `test:e2e*`, `test:harness`, `scripts/build-local-prod.sh` | run `npm run fetch:pandoc` first (`fetch-pandoc-wasm.mjs --require`): the build fails when the asset cannot be fetched or verified |
| `npm run build`, `vite dev` alone | no fetch; a missing asset turns the feature off and prints a `[pandoc]` warning (restart `vite dev` after fetching) |
| `npm run build:preview-embed` | builds with `VITE_PANDOC_WASM=0` and then `scripts/prune-embed-dist.mjs` removes `pandoc/` (and `typst/`, `pdfjs/` once they exist) from `dist-preview-embed`, which is `include_dir!`-ed into the `q2` binary; the embed does its downloads through the native pandoc (D7) |

The flag is `__PANDOC_WASM_ENABLED__` (`src/pandoc/buildFlag.ts` decides, `vite.config.ts` defines it,
`src/pandoc/featureFlag.ts` reads it): on iff the asset exists and `VITE_PANDOC_WASM` is not `0`.
The "Download as" UI (H5) must check it. It is also the kill switch: build with `VITE_PANDOC_WASM=0`.

`.gz` is in `scripts/gzip-skip-extensions.txt`, so `precompress-dist.mjs` skips it, and `vite preview`'s
`compression` filter leaves it alone (`mime-db` has `application/gzip` as not compressible).

Production serving (nginx in `quarto-hub-deployment`) needs a `location /pandoc/` that 404s a missing
file, serves `application/octet-stream` with `Cache-Control: public, max-age=31536000, immutable`
and repeats the server-level security headers; see the H4 plan for the block.

CSP: hub-client sets none (only `quarto-hub/server.rs` does, with auth on), so nothing changes
here. A deployment that adds a CSP needs `worker-src` (the render worker is a same-origin module worker)
and `script-src 'wasm-unsafe-eval'` (compiling the module).

## Tests

| Where | What |
|---|---|
| `ts-packages/pandoc-host/src/*.test.ts` (`npm test -w ts-packages/pandoc-host`) | path normalizer, mount rules, limits, request type vs the schema; no wasm needed |
| `hub-client/src/pandoc/pandocHost.wasm.test.ts` (`npm run test:wasm` in hub-client) | R0's recordings and the host behaviours through the real wasm |
| `hub-client/src/pandoc/pandocLoader.test.ts`, `pandocRunner.test.ts` (`npm test`) | loader and runner against fakes (`Cache`, `Worker`, `fetch`): SHA/cache/sniffing cases, abort/timeout/supersede, the failure taxonomy |
| `hub-client/src/pandoc/pandocLoader.wasm.test.ts` | the compressed asset through the loader to a real `Module`; a render through a real `worker_threads` worker; hang/oom/crash; memory over 20 renders |
| `hub-client/e2e/pandoc-loader.harness.spec.ts` (`playwright.harness.config.ts`) | Chromium smoke against the `VITE_E2E=1` bundle: no request before first use, exnref-absent message, abort mid-fetch, cache hit, fault injection, subpath URL. Needs the gz in `public/pandoc/` at build time |

Do not compare two 16 MB typed arrays with vitest's `toEqual` (it exhausts the heap); compare with `Buffer.equals`.

The wasm tests need Node's `--experimental-wasm-exnref` (set in `vitest.wasm.config.ts`
as `execArgv`; `NODE_OPTIONS` rejects it) and the fork pool. A missing wasm skips
locally and fails when `CI` is set. Docx and typst replays are byte-compared with the
native recording; epub (random UUIDs) and any output that embeds the native run's path
are compared with `quarto-output-extract` (`cargo build --release -p quarto-output-extract`,
or point `QUARTO_OUTPUT_EXTRACT` at a binary).

## Limits and the share root

`DEFAULT_LIMITS` and `DEFAULT_SHARE_ROOT` in `ts-packages/pandoc-host/src/limits.ts` mirror
`resources/pandoc-wasm.json`; `request.test.ts` fails if they drift. The host takes
`limits`/`shareRoot` as options so hub-client can supply them from its own build.

See `dev-docs/pandoc-wasm-bump.md` for changing the pinned version.

## Harness and parity net (H3)

`window.q2PandocDownload(path, { format = 'docx', save = true, sourceDateEpoch })` (`vite dev` and
`VITE_E2E` builds only) runs the whole chain on the document in the VFS: Rust `render_pandoc_request`,
the share tree, `PandocRunner`, then a download. It returns `{ ok, output, fileName, failure,
diagnostics, timings }`; `timings` has the request build, share-tree fetch, runner wall time, and the
host core's `instanceMs`, `runMs`, `memoryBytes`, `mountedBytes`. Tests use
`window.__quartoTest.pandoc.download`.

The parity net renders the ten P7 docx golden fixtures (`crates/quarto-core/tests/fixtures/pandoc-goldens/fixtures.json`,
the committed export of `quarto_output_extract::FIXTURES`; regenerate with
`quarto-output-extract fixtures`) through that chain and compares the extraction of each output
with the Q1 golden (`.snap`) using the Rust extractor CLI:

| Run | Command | Needs |
|---|---|---|
| Node | `npm run test:wasm -- goldenParity` (in `hub-client`) | built wasm pkg, `fetch-pandoc-wasm.mjs`, `cargo build -p quarto-output-extract` |
| Chromium | `VITE_E2E=1 npm run build && npx playwright test --config playwright.harness.config.ts e2e/pandoc-parity.harness.spec.ts` | the same, plus `hub-client/public/pandoc/pandoc.wasm.gz` |

Typst, pptx and epub are covered by R0's recorded documents (`recordingParity.wasm.test.ts`, `e2e/pandoc-formats.harness.spec.ts`; the same two commands with `recordingParity` / the formats spec): each is seeded at `/__q2_doc__`, rendered through the Rust request and compared with the recording's native `reference/`.

`Q2_PARITY_OUT=<dir>` keeps the rendered files (to open in Word). The mermaid fixture's reference
is native q2's output, not Q1's (`pandoc-goldens/DIVERGENCES.md`).

## "Download as" UI (H5)

`DocumentTopBar` renders `DownloadAsControl` (a menu button and a status panel) when
`downloadAvailable()` (`pandocWasmEnabled() || isPreviewEmbed()`). The pieces, all under
`hub-client/src/pandoc/`:

| File | Role |
|---|---|
| `downloadController.ts` | The state machine: click id (a stale `render_pandoc_request` or runner response is dropped), cancel, throttled progress, classification of the run through `classifyPandocCompletion`. A document with an error diagnostic or a failed run produces no Blob. Two executors: pandoc.wasm, or `renderNatively` in the `q2 preview` embed. |
| `downloadService.ts` | The singleton controller and `menuFormats()`. `MENU_FORMATS` is the list of entries the UI has been reviewed for (docx now); each request phase that lands adds its entry in its own commit. |
| `downloadName.ts` | `<doc-stem>.<ext>` with directory parts, control and bidi characters, Windows-reserved characters and device names removed. |
| `useDownloadAs.ts` | The React binding; status lives outside the editor's diagnostics array (which every live-preview render replaces). |

`PreviewRouter` has four modes (`PreviewMode` in `components/render/getQ2Format.ts`): `react`,
`dom`, `download` (the document's own format is downloadable but not previewable: a "Download
<type>" button, rendering only on click) and `neither` (nothing is mounted; the control is
`aria-disabled` and described by the explanation). The class comes from the Rust resolver
(`resolvePandocFormats`), which reads the document from the VFS.

### What filters and extensions can do in the browser (D9)

The docx download runs the document's Lua filters in two places. Filters in the default position
run in the browser's own Lua with a synthetic `io`/`os` and `quarto.*`. Post-position and entry-point
filters run inside pandoc.wasm with Quarto 1's vendored Lua, which has these limits:

- Not available: `os.tmpname`, `io.tmpfile`, `io.popen`, `pandoc.pipe`, network `mediabag.fetch`,
  `require 'lfs'`. `os.execute` does nothing, so a filter that runs a command and reads its
  output finds no file.
- The working directory is `/`, so a relative `io.open('data.csv')` finds nothing: only the
  filter's own directory, images and path-valued options (a reference doc, a template) are mounted.
- `os.getenv` sees only `QUARTO_*` and `SOURCE_DATE_EPOCH`; `HOME`, `PATH` and `TMPDIR` are nil.
- A filter's `print` output is not shown; a Lua error shows pandoc's message in the `Q-20-3` error.
- SVG images in docx get alt text instead of the picture (no `rsvg-convert`).
- Documents inside a `_quarto.yml` project cannot be downloaded yet (request phase R7).

## Measuring (H3)

`e2e/pandoc-measure.harness.spec.ts` (opt-in: `Q2_MEASURE=1`, with a `VITE_E2E=1` build) launches Chromium and
WebKit itself and records first-download, cached-load and per-render latency, filter startup, `mountMs`, pandoc's
linear memory and process RSS with image-heavy documents, to `Q2_MEASURE_OUT/measure-<browser>.json`. The numbers and
what they mean for the limits are in `claude-notes/research/2026-10-01-pandoc-wasm-evidence.md` §12.

The loader rejects a download that gunzips to more than `MAX_DECOMPRESSION_RATIO` (8) times its compressed size; the
real asset is 3.55x.

## Browser matrix (H6)

The pandoc harness specs (`hub-client/e2e/pandoc-*.harness.spec.ts`) run under two projects of
`playwright.harness.config.ts`: `chromium` (every harness spec, `npm run test:harness`) and
`webkit` (the pandoc specs only, `npm run test:harness:webkit`, one worker). The app E2E
(`pandoc-download.spec.ts`, real hub) stays Chromium-only. In CI (`hub-client-e2e.yml`) the
WebKit job first runs `pandoc-browser-probe.harness.spec.ts` (exnref, `DecompressionStream`,
`crypto.subtle`, Cache API, workers). If the probe fails on Playwright's Linux WebKit the
suite is skipped with a workflow warning instead of failing the job; once a run shows the probe
passing, remove the `continue-on-error`/`if:` guards so WebKit gates.

| Browser | How checked | Result |
|---|---|---|
| Chromium 148 (Playwright, macOS and Linux CI) | every pandoc spec | pass |
| WebKit 26.4 (Playwright, macOS) | all pandoc harness specs, 2026-10-02 | pass (38 of 38; the opt-in measure spec skips) |
| WebKit (Playwright, Linux CI) | probe, then the suite | **unverified until the first CI run** (not runnable from the macOS lane; Docker was not running) |
| Firefox 157 | `spike/ff/` by hand (H0) | pass; CI Firefox is braid `bd-phu943t7` |
| Real Safari | by hand | **not yet checked**: Playwright's WebKit loses the Cache API entry across navigations, so cache behaviour (and ITP's 7-day cap on script-writable storage) needs a real Safari |

Minimum versions the loader's message names: Chrome/Edge 137, Firefox 131, Safari 18.4 (MDN
exnref data). Bumping `PANDOC_PIN` means re-running the probe in each browser (upstream is
still changing its exception-handling encoding, design D4).
