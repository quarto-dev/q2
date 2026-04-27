# tree-sitter-language-wasm-shim

Local drop-in replacement for the upstream
[`tree-sitter-language`](https://crates.io/crates/tree-sitter-language)
crate (version 0.1.7). Used via `[patch.crates-io]` in
`wasm-quarto-hub-client`'s `Cargo.toml`.

## Why this exists

See the full rationale in
`claude-notes/plans/2026-04-20-wasm-shim-merge.md`.

Short version: two of the tree-sitter grammar crates we depend on
(`tree-sitter-lua`, `tree-sitter-css`) embed upstream
`tree-sitter-language`'s `wasm/src/*.c` files directly, which define C
stdlib stubs that collide with the ones `wasm-quarto-hub-client`'s
`c_shim.rs` has been providing for the Lua runtime since before
`tree-sitter-language` existed. Both implementations have feature gaps
relative to the other, so neither side can silently win without a
regression.

This crate:

- Mirrors upstream's Rust API (`LanguageFn`) verbatim so the grammar
  crates link against a compatible type.
- Mirrors upstream's `wasm/include/*.h` headers verbatim (static-inline
  helpers only; no link-level symbols).
- Ships **empty** `wasm/src/stdio.c`, `wasm/src/stdlib.c`, and
  `wasm/src/string.c`. `c_shim.rs` provides all the symbols those files
  would have contributed, extended to cover the format specifiers Lua
  needs.

## Keeping in sync with upstream

Pinned upstream version: **0.1.7**.

Three surfaces can drift:

1. `src/language.rs` — the Rust API. Stable since 0.1.0 at 23 lines.
2. `wasm/include/` — static-inline C helpers. Occasional upstream
   additions.
3. `build.rs` — publishes `wasm-headers` / `wasm-src` metadata.
   Trivially small.

When upgrading the pin, diff each surface against upstream and copy new
additions. Do **not** copy new `.c` files into `wasm/src/` — those
remain empty by design.
