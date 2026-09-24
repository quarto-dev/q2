# Preview SPA rebuild chain

When you change Rust code and want to verify the change in `q2 preview`,
the rebuild chain is **not** captured by `cargo build`. This note
explains the chain, the reasons it exists, and the minimal commands to
force a fresh preview.

## The trap, in one paragraph

`q2 preview` boots a server that serves an embedded React SPA. The SPA
runs in a sandboxed iframe and renders documents in the browser via a
WASM build of `wasm-quarto-hub-client`. When you change Rust code in
`quarto-core`, `pampa`, or any crate the WASM transitively depends on,
none of the following are sufficient:

- `cargo build --bin q2`
- `cargo build --workspace`
- `cargo nextest run --workspace`
- `cargo xtask verify --skip-hub-build`

All of them succeed, the preview command starts, the page loads, the
document renders — but the iframe is executing **pre-change WASM**.
Tests pass, end-to-end of `q2 render` looks right, and yet the preview
silently shows old behavior. This is exactly the situation
"End-to-end verification before declaring success" in `CLAUDE.md`
warns about — verifying through the binary a user runs is necessary,
and for preview the *real* user binary loads a sub-bundle that your
build didn't refresh.

## The artifact chain

```
crates/wasm-quarto-hub-client/  ──build:wasm──▶  hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm
                                                                │
                                                                ▼
                                          q2-preview-spa/  (Vite, alias 'wasm-quarto-hub-client')
                                                                │
                                            build-q2-preview-spa
                                                                ▼
                                                  q2-preview-spa/dist/
                                                                │
                                                  include_dir!  (build.rs)
                                                                ▼
                                                  crates/quarto-preview/  (EMBEDDED_SPA)
                                                                │
                                                                ▼
                                                    cargo build --bin q2
                                                                │
                                                                ▼
                                                  target/debug/q2
```

Each arrow is its own build step. None of them cascade automatically:

- `cargo build --bin q2` only re-embeds `q2-preview-spa/dist/` if a
  file inside that directory changed (see
  `crates/quarto-preview/build.rs`\'s `rerun-if-changed` directives).
  Files inside that dist are themselves Vite output — they only change
  when you re-run the SPA build.
- `cargo xtask build-q2-preview-spa` runs `npm run build` inside
  `q2-preview-spa/`, which is `tsc -b && vite build`. Vite picks up
  the WASM via an alias that resolves to
  `hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm`.
  If that file is stale, the SPA build will *successfully* bundle the
  stale WASM.
- `hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm`
  is the output of `npm run build:wasm` (a wrapper around
  `wasm-pack build` + `wasm-bindgen`). It only refreshes when you
  explicitly run that script.

So there are three caches in series, and a Rust change in `quarto-core`
has to march through all three before it reaches the preview iframe.

## The minimal command sequence

```bash
cd hub-client && npm run build:wasm
cd ..
cargo xtask build-q2-preview-spa
cargo build --bin q2
```

Then restart any running preview (`q2 preview` does not hot-reload its
embedded SPA on file change — the dist is baked in at compile time).

## When `cargo xtask verify` does and doesn't help

- `cargo xtask verify --skip-hub-build`: skips both the WASM rebuild
  and the SPA rebuild. **The preview will be stale.**
- `cargo xtask verify` (no skip flags): runs `npm run build:all` in
  `hub-client`, which rebuilds the WASM, and also runs
  `cargo xtask build-q2-preview-spa`. After verify finishes you still
  need `cargo build --bin q2` to re-embed the fresh dist before the
  next `q2 preview` invocation sees the change. (Verify does not
  rebuild the q2 binary as its final step.)

## How to recognize stale-WASM symptoms

If you've made a Rust change you expect to see in the preview and
don't, check this *first* before assuming the pipeline is buggy:

1. Note the timestamp of
   `hub-client/wasm-quarto-hub-client/wasm_quarto_hub_client_bg.wasm`.
   If it predates your Rust edit, the WASM is stale.
2. Note the timestamp of
   `q2-preview-spa/dist/assets/wasm_quarto_hub_client_bg-*.wasm`
   (the hashed file). If it predates the file above, the SPA bundle
   is stale.
3. Note the timestamp of `target/debug/q2`. If it predates the dist
   file above, the binary is stale.

The first stale link in this chain is your culprit. Run the
corresponding step in §"The minimal command sequence" and re-check.

## Why the chain isn't automated

Two structural reasons:

1. **`cargo` doesn't see across the npm boundary.** The WASM artifact
   lives in a directory that npm writes to and Vite reads from; cargo
   has no idea that a change to `crates/quarto-core/src/transforms/x.rs`
   should re-emit `hub-client/wasm-quarto-hub-client/*.wasm`. Wiring
   this in via a build script is possible but would couple every
   `cargo build` to a node-side build, which is expensive and noisy.
2. **The dev-server path doesn't have this problem.** When the
   hub-client UI is being iterated on via `cd hub-client && npm run
   dev`, Vite watches the WASM file and the hot-reload covers it.
   `q2 preview` is the production-style boot — it intentionally
   freezes the SPA at compile time so the binary is self-contained.

The trade-off, then, is: a self-contained binary the user can run
without npm — at the cost of a manual rebuild chain when iterating on
the Rust→preview path.

## Related

- `crates/quarto-preview/build.rs` — the `include_dir!` build script
  that wires `q2-preview-spa/dist/` into the embedded SPA.
- `crates/xtask/src/build_q2_preview_spa.rs` — the `cargo xtask`
  command that runs Vite for the SPA.
- `hub-client/scripts/build-wasm.js` — the script `npm run build:wasm`
  invokes.
- `q2-preview-spa/vite.config.ts` — the alias that pins the WASM path.
- bd-kw93 (q2 preview epic) — the broader feature this all supports.
