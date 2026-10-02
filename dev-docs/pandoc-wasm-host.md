# pandoc.wasm host: local setup and tests

The host core is `ts-packages/pandoc-host` (`@quarto/pandoc-host`): DOM-free,
`execute(request, shareTree, { module, fault? })`, one fresh WASI instance per run
(command mode, `_start`). hub-client owns the worker shell and the UI (host phases H2+).

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
