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

This file exists so the directory itself is tracked by git (git does not
track empty directories) — both `include_dir!` call sites above require
the directory to exist at compile time, even before any real subtree is
vendored.

No subtree is registered here yet. To add one, see
`claude-notes/instructions/extension-subtrees.md`.
