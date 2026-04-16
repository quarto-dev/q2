# wasm-qmd-parser

> **Status: Dormant** — This crate is not actively maintained but kept for
> future use as a lightweight parsing-only WASM module.

Lightweight WASM wrapper around `pampa` for QMD parsing in browser/extension
contexts. Unlike `wasm-quarto-hub-client` (which bundles the full rendering
stack), this crate exposes only the parser — smaller output, faster load times.

## History

Originally built with `wasm-pack` and its own C stdlib shim. The C shim has
since moved to the shared `wasm-c-shim` crate, and the build toolchain has
shifted to `cargo build` + `wasm-bindgen` CLI (see `dev-docs/wasm.md`).

The crate will need updates before it can build again — the `pampa` API has
evolved significantly since this was last maintained.

## License

Licensed under either of

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE_APACHE))
* MIT license ([LICENSE-MIT](LICENSE_MIT))

at your option.
