# Extension subtrees

This directory holds whole extension repos vendored as `git subtree`s via
`cargo xtask pull-extension-subtree` (a port of Quarto 1's hidden
`pull-git-subtree` dev command), mirroring Quarto 1's
`src/resources/extension-subtrees/`.

Each subtree lives at `<name>/`, a full checkout of the vendored repo
(source, tests, CI config — everything, kept in sync via `git subtree`).
Only each subtree's `<name>/_extensions/` payload is embedded into the
binary — see `crates/quarto-core/src/extension/mod.rs`
(`builtin_extension_subtree_roots`) and
`crates/wasm-quarto-hub-client/src/lib.rs` (`populate_extension_subtrees`).
Embedding the whole vendored repo would bloat the binary with source,
tests, and CI config that never needs to ship.

This file also exists so the directory itself is tracked by git (git does
not track empty directories) — the `include_dir!` call sites above require
the directory to exist at compile time, even before any real subtree is
vendored.

## Vendored subtrees

- **`orange-book/`** — the default Typst book extension, subtreed from
  `quarto-ext/orange-book`, pinned to upstream tag `0.2.0` (book-projects
  P2, plan item 80). Refresh with
  `cargo xtask pull-extension-subtree orange-book`.
- **`julia-engine/`** — the [Julia engine](https://github.com/PumasAI/quarto-julia-engine),
  subtreed from the `q2-static-declarations` branch of
  `gordonwoodhull/quarto-julia-engine` (q2's upstream of record until the
  Q1 `external-engine` schema change ships in a stable Quarto 1 release;
  see `claude-notes/plans/2026-09-03-julia-engine-static-declarations-epic.md`,
  Step 2c pivot). Refresh with `cargo xtask pull-extension-subtree julia-engine`.

To add another, see `claude-notes/instructions/extension-subtrees.md`.
