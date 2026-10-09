---
title: 'Source-pointing diagnostics for resource-path errors'
date: 2026-05-21
---

## Overview

When a `resources:` entry in `_quarto.yml` or in a document YAML
header truly does point outside the project root (a legitimate user
error after we fix the leading-`/` bug — see plan
`2026-05-21-resource-path-leading-slash.md`), the current error is
plain text:

```
Error: resource path '/docs/download/_download.json' resolves outside
the project root '/.../quarto-web'. Project resources must live
within the project directory.
```

This tells the user *what* but not *where*. In `_quarto.yml` the
offending pattern could be one of dozens; in a document header it
could be one of many docs in the project. We want a tidyverse-style
diagnostic with an Ariadne source snippet pointing at the offending
YAML scalar, the same way other Q2 errors do.

## Why this is feasible now

The data is already there:

- `ConfigValue` carries a `SourceInfo` per scalar
  (`crates/quarto-pandoc-types/src/config_value.rs:155`).
- `quarto-error-reporting` already supports source-spanned
  diagnostics with Ariadne rendering (used across `quarto-core` —
  see `crates/quarto-core/src/error.rs`, `project/listing/sort.rs`,
  `project/dependency_graph.rs`).
- The current loss happens at two spots:
  1. `extract_resource_patterns` in
     `crates/quarto-core/src/project_resources.rs:118` flattens
     `ConfigValue` items into `Vec<String>`, discarding source.
  2. The orchestrator wraps `ResourceError` in
     `QuartoError::other(e.to_string())` at
     `crates/quarto-core/src/project/orchestrator.rs:627`, dropping
     even the structured error variant.

## Design

### 1. Preserve `SourceInfo` per pattern

Replace the flat `Vec<String>` with a small struct:

```rust
pub struct RawResourcePattern {
    pub pattern: String,
    pub source_info: SourceInfo,
}
```

Threaded through:
- `ProjectConfig::resources: Vec<RawResourcePattern>`
- `DocumentProfile::resources: Vec<RawResourcePattern>` (matching the
  same change on the doc side; see `crates/quarto-core/src/document_profile.rs`)
- `extract_resource_patterns` returns `Vec<RawResourcePattern>`,
  copying the per-item `source_info`.
- `expand_patterns` takes `&[RawResourcePattern]` and propagates the
  source info into the `ResourceError` variants.

This is intentionally a small, localized refactor — no public
surface beyond these structs needs to change.

### 2. Carry `SourceInfo` into `ResourceError`

```rust
pub enum ResourceError {
    OutOfProject {
        pattern: String,
        project_root: PathBuf,
        source_info: SourceInfo,
    },
    InvalidGlob { pattern: String, source: glob::PatternError, source_info: SourceInfo },
    GlobWalk    { pattern: String, source: glob::GlobError,    source_info: SourceInfo },
}
```

### 3. Render through `quarto-error-reporting`

In the orchestrator, instead of `QuartoError::other(e.to_string())`,
build a `DiagnosticMessage` using
`DiagnosticMessageBuilder::error(...)` with a label at
`source_info`\'s span, e.g.:

```
error: resource path resolves outside the project root
   ╭─[_quarto.yml:5:7]
 5 │     - "/docs/download/_download.json"
   ·       ─────────────────┬──────────────
   ·                        ╰── this pattern resolves to '/docs/...'
   ·                            which is outside '/path/to/quarto-web'
   ╰─

i Use a project-relative path. A leading `/` is treated as
  project-root-relative — e.g. `/docs/download/_download.json` means
  `<project>/docs/download/_download.json`.
```

The `info` bullet is exactly the kind of guidance that closes the
loop with the fix from
`2026-05-21-resource-path-leading-slash.md`: once that lands, the
*only* way to hit this error is to have a real out-of-project
pattern, and the `i` line tells the user how to write the right one.

The renderer goes through `ParseError` (in
`crates/quarto-core/src/error.rs`) so we get Ariadne snippets in
text output and structured JSON for `--diagnostics-format json`.

### 4. Test infrastructure

Borrow the snapshot-test pattern already used elsewhere in
`quarto-core`. A fixture project under
`crates/quarto-core/tests/fixtures/resource-out-of-project/` with a
crafted `_quarto.yml` containing a `../escape.csv` entry. The
snapshot captures the rendered diagnostic (without hyperlinks —
`TextRenderOptions { enable_hyperlinks: false, .. }`) so the
filename doesn't leak absolute paths.

## Test plan (TDD)

1. **Unit: `ResourceError::OutOfProject` carries SourceInfo** —
   construct `expand_patterns` directly with a `../escape.csv`
   pattern + a synthesized `SourceInfo`, assert the error variant's
   `source_info` matches.
2. **Unit: project-level YAML preserves source info end-to-end** —
   parse a tiny `_quarto.yml` string via the existing config-parse
   path, run `collect_static_resources`, assert the resulting
   `ResourceError` has line/col matching the YAML scalar.
3. **Unit: document-level YAML preserves source info end-to-end** —
   same shape, but the `resources:` entry lives in a doc header.
   Asserts the diagnostic points into the `.qmd` file, not
   `_quarto.yml`.
4. **Snapshot: rendered diagnostic** — a project-fixture test that
   runs `render_project` to error and snapshots the rendered text.
   Confirms the Ariadne span, the `i` hint, and the tidyverse-bullet
   shape are all stable.
5. **Regression: leading-`/` no longer errors** — depends on
   `2026-05-21-resource-path-leading-slash.md`. A pattern
   `"/data/x.txt"` with file at `<root>/data/x.txt` succeeds.

## Work Items

- [x] Define `RawResourcePattern` + thread through `ProjectConfig`,
  `DocumentProfile`, `extract_resource_patterns`, `expand_patterns`.
- [x] Add `source_info: SourceInfo` to `ResourceError` variants
  (`OutOfProject`, `InvalidGlob`, `GlobWalk`).
- [x] Add `ResourceError::source_info()` and `pattern()` accessors
  so callers don't have to match every variant by hand.
- [x] Add `config_path: Option<PathBuf>` to `ProjectConfig` so the
  diagnostic helper can locate the YAML file for source-context
  loading. Populated by `parse_config`.
- [x] Bump `DOCUMENT_PROFILE_VERSION` from 5 → 6 with a `bd-c1et2`
  changelog note (`resources` shape changed; v5 profiles fail to
  deserialize and the cache layer regenerates).
- [x] Write tests T1 (extract preserves source_info per item; both
  array and scalar shorthand), T2 (`expand_patterns` errors carry
  the SourceInfo we passed in), T3 (full diagnostic round-trip:
  rendered text contains code, pattern, leading-`/` hint, and the
  Ariadne span resolves against a `SourceContext` built from a real
  on-disk YAML file).
- [x] Build `resource_error_to_parse_error(err, &source_file)` —
  loads the YAML file into a `SourceContext` under the SourceInfo's
  FileId and constructs a tidyverse-shaped `DiagnosticMessage` with
  `.with_code("Q-RSC-…").with_location(source_info).problem(…).add_info(…)`.
- [x] Build `collect_static_resources_with_diagnostics` that splits
  the project-level and per-document calls so each error is
  attributed to the right source file (`_quarto.yml` vs. the
  declaring `.qmd`).
- [x] Wire orchestrator to use the diagnostic-aware variant; map
  the returned `ParseError` into `QuartoError::Parse(…)` instead of
  `QuartoError::other(e.to_string())`.
- [x] All quarto-core tests pass (2090 total, +4 new from this
  issue), full `cargo xtask verify --skip-hub-build` clean.
- [x] End-to-end demo: tiny project at `/tmp/q2-bd-c1et2-demo` with
  an out-of-project resource entry. Rendered diagnostic captured
  below.

## End-to-end transcript (2026-05-21)

`/tmp/q2-bd-c1et2-demo/_quarto.yml`:

```yaml
project:
  type: website
  resources:
    - "/docs/legit.json"
    - "../escape.csv"

website:
  title: "demo"
```

`cargo run --bin q2 -- render /tmp/q2-bd-c1et2-demo` (ANSI stripped).
Codes follow the `Q-<subsystem>-<number>` convention from
`CONTRIBUTING-ERRORS.md`; `5` is the "Projects and Structure"
subsystem. The three codes Q-5-1 (out-of-project), Q-5-2 (invalid
glob), and Q-5-3 (glob walk failure) are registered in
`crates/quarto-error-reporting/error_catalog.json`:

```
Error: [Q-5-1] Resource path resolves outside the project root
   ╭─[ /private/tmp/q2-bd-c1et2-demo/_quarto.yml:5:7 ]
   │
 5 │     - "../escape.csv"
   │       ──────┬──────
   │             ╰──────── Pattern `../escape.csv` resolves outside
   │                       `/private/tmp/q2-bd-c1et2-demo`. Project
   │                       resources must live within the project
   │                       directory.
───╯
ℹ A leading `/` is project-root-relative — e.g. `/docs/foo.json`
  means `<project>/docs/foo.json`. To reference files outside the
  project, copy them in or use `copy:` (Q1: not yet supported).
```

The Ariadne span correctly underlines `../escape.csv` on line 5
where it appears in `_quarto.yml`. The "i" hint guides the user
toward the leading-`/` fix from bd-wlza2 — the diagnostic and the
earlier bug fix work as a pair.

## Follow-up filed during this work

- **bd-z2j7o** (discovered-from bd-c1et2, priority 3): audit the
  codebase for `String → { String, SourceInfo }` refactors and
  decide whether to unify them on a parametric
  `WithSourceInfo<T>` wrapper. Worth considering as more
  diagnostics get source-info-attached.

## Open questions

- Should the document-YAML diagnostic also include a span into
  `_quarto.yml` when `output_dir` settings affect what counts as
  "inside the project root"? Probably no; the user wrote the bad
  pattern in the doc header, not in `_quarto.yml`. Mentioning it in
  the `i` line is enough.

## Dependencies

- Blocked by: `2026-05-21-resource-path-leading-slash.md` (the bug
  fix is what makes this diagnostic land on *actual* errors instead
  of false positives).
- Related: `2026-05-21-q2-preview-diagnostics-ariadne.md` (recent
  work establishing the same Ariadne-rendering pattern in the
  preview surface).
