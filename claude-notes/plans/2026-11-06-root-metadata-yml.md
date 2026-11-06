---
title: "Apply a project-root `_metadata.yml` like any other directory's"
---

## Overview

A `_metadata.yml` next to `_quarto.yml` should apply to the documents in the
project root and its subdirectories, like any other directory's
`_metadata.yml`. q2 ignores it: `toc`, `theme`,
`embed-resources`, `code-fold` and so on set there never reach `q2 render`
or `q2 preview`. A `_metadata.yml` in a subdirectory already works.

This matters for any tool that scaffolds a small single-directory project:
a `_quarto.yml` with `project: type: default`, a tool-owned `_metadata.yml`
with the house theme and settings, and the document. The tool cannot just
move those settings into `_quarto.yml`: when the folder is later bundled
under a parent project, its `_quarto.yml` becomes a nested project file,
which Quarto 1 ignores, while a nested `_metadata.yml` still applies. So the
settings have to stay in `_metadata.yml`, and q2 has to read it.

## Root cause

`directory_metadata_paths_for_document`
(`crates/quarto-core/src/project/mod.rs`) walks
`relative_path.components()` from the project directory to the document\'s
directory, joining one component at a time. For a document in the project
root the relative path is empty, so the loop never runs and the root is
never checked.

That walk was ported from Quarto 1\'s `directoryMetadataForInputFile`
(`src/project/project-shared.ts`), which does
`relative(projectDir, inputDir).split(SEP_PATTERN)`. For a root-level
document that is `"".split(...)`, which is `[""]`, not `[]`; one iteration
runs, `join(projectDir, "")` is the project directory, and the root
`_metadata.yml` is read. The Rust port (and the comments in its unit tests,
`test_directory_metadata_hierarchy` and
`test_directory_metadata_document_at_root`) read this as "the root is never
included".

## Quarto 1 behaviour, and where q2 differs

Checked with Quarto 1.10.19: a project with `_quarto.yml`
(`project: type: default`), `toc: true` in a `_metadata.yml`, and the same
document at several depths.

| `_metadata.yml` in | `doc.qmd` | `sub/doc.qmd` | `sub/foo/doc.qmd` |
| --- | --- | --- | --- |
| project root | yes | no | no |
| `sub/` | n/a | yes | yes |

Every directory's `_metadata.yml` reaches its subdirectories except the
root's. The Quarto 1 docs describe directory metadata as applying to
"files within a directory" and never mention the root, and the root-only
reach falls out of `"".split()` rather than any rule in the code, so we
treat it as accidental.

**Decision (2026-11-06):** q2 treats the root like every other directory:
a root `_metadata.yml` applies to the whole project, between `_quarto.yml`
and any deeper `_metadata.yml`. This is a deliberate difference from
Quarto 1, documented in the user docs. No quarto-cli issue will be filed.

## Checklist

### Phase 1: tests (red)

- [x] Unit (`project/mod.rs`): `test_directory_metadata_document_at_root`
  expects one layer, the root `_metadata.yml`, instead of none.
- [x] Unit: `test_directory_metadata_hierarchy` and
  `test_directory_metadata_skips_missing` expect the root layer first for
  a document in a subdirectory (replacing the "root is NOT included"
  comments).
- [x] Unit: the `.yaml` spelling at the root, and no layer when the root
  has no file.
- [x] Integration (`tests/integration/root_directory_metadata.rs`, through
  `ProjectPipeline`): `toc: true` in a root `_metadata.yml`, top level and
  under `format: html:`, puts a TOC in both `doc.html` and `sub/doc.html`.
- [x] Integration: `theme: [t.scss]` in a root `_metadata.yml` reaches the
  stylesheets both pages link, so `t.scss` resolves against the root for
  `sub/doc.qmd` too.
- [x] Integration: precedence, `_quarto.yml` < root `_metadata.yml` <
  `sub/_metadata.yml` < front matter.
- [x] Run the new and changed tests; confirm they fail for the expected
  reason.

### Phase 2: fix (green)

- [x] In `directory_metadata_paths_for_document`, check the project
  directory itself before walking the components. Keep the path spelling
  the walk already uses (`project.dir` joined form), since layer `FileId`s
  are hashed from it (bd-x113wg9v).
- [x] Update the doc comment on `directory_metadata_for_document` (it says
  the root is not included and the example tree marks it so).
- [x] Check the other callers of the path list pick it up without changes:
  `metadata_layers.rs`, `compile_theme_css.rs`, `project_resources.rs`,
  `website_post_render.rs`.
- [x] Run the Phase 1 tests; confirm green. Run all of `quarto-core` for
  tests that relied on the root being skipped.

### Phase 2b: lone documents (decided 2026-11-06)

Quarto 1 also applies a `_metadata.yml` to a document rendered outside any
project, when the file sits in the document's own directory (its stand-in
project is rooted there; the same `"".split()` path as the root case). A
parent directory's file does not apply. q2 skipped directory metadata for
single-file projects entirely, a guard from the original port. We match
Quarto 1.

- [x] Tests (red first): `test_directory_metadata_single_file_reads_its_own_directory`
  and `..._ignores_parent_directories` (`project/mod.rs`, through
  `ProjectContext::discover` on the file); `single_file_project_reads_only_its_own_directory`
  (`metadata_layers.rs`, replacing a test that set `is_single_file` on a
  real project and expected no layers); `lone_document_*` in
  `root_directory_metadata.rs` (rendered through `ProjectPipeline`).
- [x] Drop the `is_single_file` guards in `directory_metadata_for_document`,
  `directory_metadata_paths_for_document` and `collect_metadata_layers`.
  A single-file context's `dir` is the document's directory, so the walk
  reads that directory only.
- [x] Docs: state the rule for documents outside a project.
- [x] `cargo nextest run -p quarto-core` (only the `bsicons` failure) and
  `-p quarto` (only `smoke_all`: 14 Typst fixtures with R cells failing on
  `failed to load cairo DLL`; none has a `_metadata.yml`).

### Phase 3: preview and docs

- [ ] `q2 preview` on the reproduction project: the TOC and theme apply,
  and editing the root `_metadata.yml` re-renders. Rebuild the WASM and SPA
  first (`npm run build:wasm`, `cargo xtask build-q2-preview-spa`,
  `cargo build --bin q2`), or the preview runs stale code.
- [x] Docs: say that a root `_metadata.yml` applies to the whole project,
  with a "Differences from Quarto 1" entry. No page documented directory
  metadata, so added `docs/guides/projects/directory-metadata.qmd` (sidebar
  entry after "Paths in websites").

### Phase 4: verification

- [x] `cargo nextest run -p quarto-core`: 6730 of 6731 pass. The one
  failure, `dashboard_goldens::dashboard_skeletons_match_goldens`, needs
  the R package `bsicons`, which is not installed on this machine.
- [x] `cargo clippy -p quarto-core --all-targets -- -D warnings`
- [ ] `cargo xtask verify` (the change is in `quarto-core`, which the
  hub-client WASM depends on)
- [x] End-to-end with the binary (below), output inspected and recorded
  here.

## End-to-end check

```sh
mkdir t && cd t
printf 'project:\n  type: default\n' > _quarto.yml
printf 'toc: true\n' > _metadata.yml
printf -- '---\ntitle: T\n---\n\n## Section\n\ntext\n' > doc.qmd
mkdir sub && cp doc.qmd sub/
cargo run --bin q2 -- render && grep -c 'id="TOC"' doc.html sub/doc.html   # expect 1 each
```

Before the fix both print 0.

### Result (2026-11-06)

Run in a scratch directory with `target/debug/q2` built from this branch,
with the fixture above plus `format: html: theme: [t.scss]` in the root
`_metadata.yml` and `$body-bg: #abcdef` in `t.scss`:

```text
$ q2 render
Rendered 2 of 2 files to .../e2e
$ grep -c 'id="TOC"' doc.html sub/doc.html
sub/doc.html:1
doc.html:1
$ grep -o '<nav id="TOC"[^>]*>' doc.html
<nav id="TOC" role="doc-toc" class="toc-active">
```

Both pages link `doc_files/quarto/quarto-theme-8927f9b7bf58e8ce.css`, which
contains `--bs-body-bg: #abcdef`. `q2 render doc.qmd` (one file) also
gets the TOC. Output inspected.
