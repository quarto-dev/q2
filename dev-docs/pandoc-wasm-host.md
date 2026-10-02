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
