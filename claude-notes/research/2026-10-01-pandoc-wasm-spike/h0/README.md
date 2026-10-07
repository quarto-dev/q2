# H0 browser harness (pandoc-wasm host phase H0)

Replays R0's recordings (`crates/quarto-core/tests/fixtures/pandoc-recordings/recordings/`) through the pinned
`pandoc.wasm` in **WASI command mode** (`_start`) inside a module Web Worker, with
`@bjorn3/browser_wasi_shim` 0.4.2. Throwaway verification code; the production host is H1.

```bash
npm install                                   # here: only the shim
gh release download 3.11 -R jgm/pandoc -p 'pandoc-3.11.wasm.zip' && unzip pandoc-3.11.wasm.zip   # sha256 in resources/pandoc-wasm.json
cargo build --release -p quarto-output-extract    # compare.mjs uses target/release/quarto-output-extract
export WASM=/path/to/pandoc-wasm/pandoc.wasm
node drive.mjs chromium                       # or webkit; [only] filters recording names; writes out/<browser>/
node compare.mjs chromium                     # wasm output vs each recording's native reference, via the extractor
node summarize.mjs chromium                   # timings, probes
# by hand (Firefox, or any real browser):
node serve.mjs                                # then open http://localhost:8140/?b=firefox ; results land in out/firefox/
node compare.mjs firefox
```

`compare.mjs` rewrites `/tmp/q2-pandoc-replay/<rec>/__q2_` to `/__q2_` in the extraction text before comparing, because the
native references ran at the replay path and the wasm run at the placeholder roots (docx image descriptions embed the path);
that is the only normalization beyond the extractor's own. `out/` is gitignored; `out/<browser>/svg-docx/` holds the docx files for
the manual Word check.
