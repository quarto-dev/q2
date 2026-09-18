# Pandoc Filters

This directory contains Lua filters from Quarto 1 that are used with Pandoc to produce non-HTML output formats (docx, pptx, etc.). These files are copied from the TypeScript Quarto repository (`quarto-cli`) at the pinned tag and maintained locally.

## Contents

- `filters/` - Quarto's pandoc filter suite
  - `main.lua` - Main filter entry point (171 `import()` directives, one of
    which — `quarto2-shim.lua` — is a Task 8 patch, not from `v1.11.3`)
  - `ast/` - AST manipulation utilities
  - `common/` - Common functionality
  - `crossref/` - Cross-reference processing
  - `customnodes/` - Custom node handling
  - `customwriter/` - Custom writer implementation
  - `layout/` - Layout processing
  - `llms/` - LLM-related filters
  - `modules/` - Module system
  - `normalize/` - Normalization filters
  - `quarto-finalize/` - Final processing
  - `quarto-init/` - Initialization
  - `quarto-internals/` - Internal utilities
  - `quarto-post/` - Post-processing
  - `quarto-pre/` - Pre-processing
  - `rmarkdown/` - R Markdown compatibility

- `pandoc/datadir/` - Pandoc data directory with Lua utilities
  - `init.lua` - Main initialization script
  - `_base64.lua`, `_format.lua`, `_json.lua`, `_utils.lua` - Utility modules
  - `logging.lua`, `lpegfenceddiv.lua`, `lpegshortcode.lua` - Parsing utilities
  - `profiler.lua`, `readqmd.lua` - Performance and parsing tools
  - `luacov/` - Code coverage utility

### Required runtime layout

At runtime these two subtrees must be materialized on disk as **siblings
under one shared root**, not extracted independently:

```
<share>/
├── filters/
│   └── main.lua
└── pandoc/
    └── datadir/
        └── init.lua
```

`init.lua:257` builds pandoc's Lua filter search path as
`pandoc.path.normalize(PANDOC_STATE.user_data_dir .. '/../../filters/?.lua')`,
where `PANDOC_STATE.user_data_dir` is set by pandoc's own `--data-dir` flag.
Climbing two levels up from `<share>/pandoc/datadir` lands at `<share>`, so
`filters/` must sit exactly two directories above `pandoc/datadir/`. Two
independent extractions (e.g. two separate `ResourceBundle`s, each with its
own temp directory) cannot produce this layout. `--data-dir` alone is also
not sufficient: `init.lua` additionally requires `QUARTO_SHARE_PATH` set to
`<share>` (so its own `require '_format'` etc. resolve) and
`QUARTO_FILTER_DEPENDENCY_FILE` set to a writable file path. See
`crates/quarto-core/src/pandoc_filters/bundle.rs` (materializes the layout)
and `crates/quarto-core/src/pandoc_filters/harness.rs` (the env contract and
the `pandoc` invocation shape), with tests in
`crates/quarto-core/tests/integration/pandoc_transport.rs`.

## Source

These files are copied from:
- quarto-cli tag: `v1.11.3`
- pandoc version: `3.10`

To update: Check the original repository at that tag for any changes.

## Updating

To re-vendor these files when quarto-cli updates:

1. Ensure v1.11.3 (or newer tag as needed) is available in the quarto-cli checkout
2. Extract the subtrees:
   ```bash
   cd /path/to/quarto-cli
   git archive v1.11.3 src/resources/filters src/resources/pandoc/datadir | \
     tar -xf - -C /path/to/q2/resources/pandoc-filters
   ```
3. Move extracted files to correct locations:
   ```bash
   mv src/resources/filters/* resources/pandoc-filters/filters/
   mv src/resources/pandoc/datadir/* resources/pandoc-filters/pandoc/datadir/
   ```
4. **IMPORTANT:** After re-vendor, read the "Ours vs. pinned" section below and restore any Q2 customizations

## Why Local Copy?

These files are maintained as a local copy rather than referencing `external-sources/` directly because:

1. **Build reproducibility**: The build should work without external-sources being checked out
2. **Embedded at compile time**: Files are embedded into the binary via `include_dir!`
3. **Version control**: Changes to these resources are tracked in the repository
4. **CI/CD compatibility**: CI builds don't need to check out quarto-cli

## Ours vs. pinned

The following files and modifications are *not* from `v1.11.3` and should be preserved on re-vendor:

**Format for entries:** List items must be shaped exactly as `` - `<path/from/repo/root>` — description `` (backtick immediately after `- `) so the `cargo xtask lint` rule `vendored-pandoc-filters` can verify the path exists. The rule checks that every path listed here actually exists on disk.

- `resources/pandoc-filters/filters/main.lua` — patched twice, independently.
  (1) Task 8: two edits marked `QUARTO2-PATCH`: an
  `import("./quarto2-shim.lua")` line after the `customnodes/*.lua` import
  block, and a `tappend(quarto_filter_list, quarto_pandoc_shim_filters)`
  line spliced between `quarto_init_filters` and `quarto_normalize_filters`.
  Both are anchored by group name in the patch comments, not by line
  number, since `main.lua`'s group contents have been refactored twice in
  two years. (2) P3 Task 6, upstream PR quarto-dev/quarto-cli#14913: edits
  marked `QUARTO-PATCH` convert the old `if enableCrossRef then` gate to
  `_quarto.modules.crossref_numbering.assign_crossref_numbers()`, and add a
  fail-fast guard rejecting `crossref-numbering: external` combined with a
  LaTeX-family or Typst target.
- `resources/pandoc-filters/filters/quarto2-shim.lua` — new (ours, Task 8).
  P4 ships a placeholder `quarto_pandoc_shim_filters` group (an empty
  filter, so `main.lua` loads and runs unchanged); P5 replaces the body
  with the real wire-format-to-Q1-scaffold conversion.
- `resources/pandoc-filters/filters/modules/crossref_numbering.lua` — new
  (upstream, carried early via P3 Task 6, upstream PR
  quarto-dev/quarto-cli#14913). Defines `crossref_present()` /
  `assign_crossref_numbers()`, separating "should captions/refs still show
  numbers" from "should quarto itself compute those numbers" — the second
  question is false under `crossref-numbering: external`.
- `resources/pandoc-filters/filters/modules/import_all.lua` — patched (P3
  Task 6, upstream PR quarto-dev/quarto-cli#14913). One line marked
  `QUARTO-PATCH`: registers `crossref_numbering` in `_quarto.modules`.
- `resources/pandoc-filters/filters/customnodes/floatreftarget.lua` —
  patched (P3 Task 6, upstream PR quarto-dev/quarto-cli#14913). Four call
  sites marked `QUARTO-PATCH`: converted from `param("enable-crossref",
  true)` to `_quarto.modules.crossref_numbering.crossref_present()`.
- `resources/pandoc-filters/filters/modules/callouts.lua` — patched (P3
  Task 6, upstream PR quarto-dev/quarto-cli#14913). Two sites marked
  `QUARTO-PATCH`: one call site converted the same way as
  `floatreftarget.lua`; the other adds a guard against a missing
  `callout.order` field.
- `resources/pandoc-filters/filters/crossref/format.lua` — patched (P3
  Task 6, upstream PR quarto-dev/quarto-cli#14913). One site marked
  `QUARTO-PATCH`: `refNumberOption` guards against `crossref.startAppendix`
  being nil.
- `resources/pandoc-filters/filters/layout/ipynb.lua` — comment-only
  addition (P3 Task 6, upstream PR quarto-dev/quarto-cli#14913, not
  marked `QUARTO-PATCH` since no logic changed). Explains why the two
  `param("enable-crossref", true)` reads here are deliberately *not*
  converted to `crossref_present()`: both predicates dispatch to the same
  `render_ipynb_layout` callback, so the pair is a verified no-op
  regardless of which flag it reads.

## License

These filters are part of Quarto, licensed under the GPL v2 License (with Quarto-specific exceptions). See the original quarto-cli repository for full licensing information.
