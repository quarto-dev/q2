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

- `typst-template/` - the 8-partial typst doctemplate (pandoc-hybrid-typst
  Phase 1), vendored **unmodified** from
  `src/resources/formats/typst/pandoc/quarto/` (a different upstream
  subtree than `filters/`/`pandoc/datadir/` above, but the same pin — see
  Source below). `template.typ` is the orchestrator passed to pandoc's
  `--template`; the other 7 (`numbering.typ`, `definitions.typ`,
  `typst-template.typ`, `page.typ`, `typst-show.typ`, `notes.typ`,
  `biblio.typ`) are `$partial.typ()$`-included by it and by each other, and
  must stay flat siblings — Pandoc resolves a partial relative to the
  *including* template's own directory, so nesting them would break the
  chain. Materialized by
  `crate::pandoc_filters::bundle::extract_typst_template`, independent of
  the `filters/`/`pandoc/datadir/` layout above (`--template` and
  `--data-dir`/`-L` are unrelated pandoc mechanisms). No Q2 customizations
  exist here yet, so there is no corresponding "Ours vs. pinned" entry
  below — if that changes, add one.

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
- pandoc version: `3.11`

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
- `resources/pandoc-filters/filters/quarto2-shim.lua` — new (ours, Task 8;
  body implemented across P5 Tasks 1-8, plus a post-review fix wave).
  Decodes Q2's wire-format `CustomNode` scaffold back into real Q1 nodes,
  so Q1's own render handlers run: Route R (Callout, Tabset, Theorem,
  Proof, FloatRefTarget — a real Q1 constructor exists) and Route N
  (CrossrefResolvedRef, Equation — no Q1 constructor; the shim calls Q1's
  render functions directly). The `routes` R/N classification is derived
  from `route_handlers`'s keys, not hand-maintained as a second table.
- `resources/pandoc-filters/filters/quarto2-shim-probe.lua` — new (ours, P5
  Task 1). A test-only observer `-L` filter appended after `main.lua` by the
  harness's `run_main_lua_capturing_ast`, capturing the post-filter AST for
  assertions without displacing the real writer. Never part of the
  production filter chain.
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
- `resources/pandoc-filters/filters/quarto-pre/book-numbering.lua` — new
  (ours, book-projects P2; no upstream PR — marked `QUARTO2-PATCH`, not
  `QUARTO-PATCH`, per the `main.lua` Task 8 convention for Q2-authored
  changes). The `Header` handler reads `quarto-book-item-*` Pandoc
  attributes (stamped by Q2's single-file merge step,
  `crates/quarto-core/src/project/book/merge.rs`) directly, instead of
  `currentFileMetadataState().file` (Q1's own mechanism, populated from
  the paired `<!-- quarto-file-metadata: ... -->` comment markers by
  `common/filemetadata.lua`). Additive, not a replacement: the merge step
  still emits both forms from the same data, since the comment markers
  are the *only* channel the unpatched `orange-book` Typst extension's own
  filter reads via `quarto.doc.file_metadata()`. Along the way, this fixes
  a read that was dead in Q1 too — Q1's `bookItemMetadata`
  (`book-render.ts`) never sets a `file.appendix` field, so the "mark
  appendix chapters for epub" rule was unreachable; the new
  `quarto-book-item-appendix` attribute (set only on appendix chapters —
  kind `Appendix`, `file: Some`) makes it live. Tests:
  `crates/quarto-core/tests/integration/book_numbering_lua.rs`.

## License

These filters are part of Quarto, licensed under the GPL v2 License (with Quarto-specific exceptions). See the original quarto-cli repository for full licensing information.
