---
title: 'Treat leading-`/` resource paths as project-root-relative'
date: 2026-05-21
description: 'Makes leading-slash `project.resources` patterns in `_quarto.yml` resolve project-root-relative instead of filesystem-absolute, which currently fails the containment check during `q2 render` of quarto-web.'
---

## Overview

`project.resources:` patterns in `_quarto.yml` that begin with `/` —
e.g. `"/docs/download/_download.json"` — are intended in TS Quarto to
mean *project-root-relative*, not *filesystem-absolute*. In Q2 today
they are interpreted as filesystem-absolute paths, which then fail
the project-containment check with:

```
Error: resource path '/docs/download/_download.json' resolves
outside the project root '/.../quarto-web'.
```

`external-sources/quarto-web/_quarto.yml` exercises this on line 4–10
(six leading-`/` patterns). It is the immediate blocker for trying
to render the live `quarto-web` site under Q2.

## Reproduction (2026-05-21)

```
$ cargo run --quiet --bin q2 -- render external-sources/quarto-web
Error: resource path '/docs/download/_download.json' resolves outside
the project root '/Users/cscheid/rooms/room-1/q2/external-sources/quarto-web'.
Project resources must live within the project directory.
```

Time-to-error: \~4.8s (this is the same render run we will profile in
the third issue, see plan
`2026-05-21-q2-render-website-profile.md`).

## Root cause

`crates/quarto-core/src/project_resources.rs::expand_one()` builds
the on-disk path via `anchor.join(pattern)`. When `pattern` is
absolute (starts with `/`), Rust's `Path::join` *replaces* the base
with the absolute path. The resulting path is then canonicalized and
rejected by `canonicalize_within_project` because it doesn't live
under `project_root`.

The same shape exists in `resolve_reported_resources()` (engine /
Lua-filter channel), where `entry.raw_path.is_absolute()` is checked
explicitly and the path is used as-is. That path is *intentional*
for engine outputs that already live on disk — those paths really
are filesystem-absolute. So the fix must distinguish:

- **YAML-declared** patterns (project `resources:` and document
  `resources:`): leading `/` means *project-root-relative*, matching
  TS Quarto.
- **Engine / Lua-filter contributed** paths: leading `/` continues
  to mean filesystem-absolute (engine paths are real on-disk paths).

## Design

Introduce a small normalization step in `expand_one()` (and in the
YAML-driven callers of `expand_patterns`) that, before the
`anchor.join(pattern)` call, rewrites a leading-`/` pattern to be
relative to `project_root`. Concretely:

```rust
fn normalize_yaml_pattern<'a>(pattern: &'a str) -> &'a str {
    pattern.strip_prefix('/').unwrap_or(pattern)
}
```

…applied at the boundary where YAML patterns enter `expand_one`. The
glob branch and the literal branch both need it. On Windows, `/`
inside a YAML string is still the project-root marker — we do *not*
strip drive prefixes (`C:\`) or backslashes; this is a Quarto
convention, not a filesystem convention.

`resolve_reported_resources()` is untouched: engine and Lua-filter
channels keep absolute-path semantics.

## Test plan (TDD)

Tests go in `crates/quarto-core/src/project_resources.rs` alongside
the existing `expand_*` tests. All tests use a `TempDir` for
`project_root` so they're platform-portable.

1. **`expand_leading_slash_literal_is_project_relative`** — pattern
   `"/data/a.txt"` with `anchor == project_root`; file exists at
   `<root>/data/a.txt`; assert `output_relative == "data/a.txt"`.
2. **`expand_leading_slash_glob_is_project_relative`** — pattern
   `"/data/*.csv"`; two CSVs present under `<root>/data/`; assert
   both resolve under the project root.
3. **`expand_leading_slash_doc_pattern_anchors_to_project_root_not_doc_dir`**
   — anchor is a doc dir under `<root>/posts/`; pattern `/shared.js`;
   file at `<root>/shared.js`; assert resolution succeeds and
   `output_relative == "shared.js"` (i.e. not `posts/shared.js`).
4. **`engine_report_absolute_path_still_filesystem_absolute`** —
   regression test: `resolve_reported_resources` with an absolute
   path inside the project root continues to resolve correctly. This
   guards against accidentally applying the YAML-normalization rule
   to the engine channel.
5. **Existing `out_of_project_literal_is_error`** — must still pass
   (a literal `"../outside.csv"` is still an error).

End-to-end (must be recorded in the PR per CLAUDE.md
"End-to-end verification before declaring success"):

```
$ cargo run --bin q2 -- render external-sources/quarto-web
```

…should no longer fail on the resource-path check. (It will likely
fail later for *other* reasons since quarto-web isn't a Q2 project
yet — that's expected and is what the profiling plan investigates.)

## Work Items

- [x] Add the five tests above; confirmed tests (1)–(3) fail before
  the fix and (4)–(5) pass before the fix.
- [x] Implement `strip_prefix('/')` normalization in `expand_one`\'s
  YAML entry point.
- [x] Re-run all five tests; all five pass.
- [x] `cargo nextest run -p quarto-core` clean (2080 tests pass).
- [x] `cargo xtask verify --skip-hub-build` clean.
- [x] End-to-end: re-ran `q2 render external-sources/quarto-web`.
  The resource-path error no longer fires. The first four
  leading-`/` file entries (`_download.json`, `_prerelease.json`,
  `_redirects`, `netlify.toml`) all resolved as
  project-root-relative and were copied to `_site/`. The render
  then fails further along on a *different* bug — the fifth
  resource (`/docs/blog/posts/.../demo`) is a directory, and the
  literal-path branch of `expand_one` doesn't filter directories
  (whereas the glob branch does). Filed as **bd-47w7o**
  (discovered-from bd-wlza2).
- [x] Update the doc-comment on `expand_one` / `expand_patterns` to
  describe leading-`/` semantics explicitly.

## End-to-end transcript (2026-05-21)

```
$ cargo run --quiet --bin q2 -- render external-sources/quarto-web
Error: Failed to copy resource /…/external-sources/quarto-web/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo
  → /…/external-sources/quarto-web/_site/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo
  : I/O error: the source path is neither a regular file nor a symlink to a regular file

$ ls external-sources/quarto-web/_site/docs/download/
_download.json
_prerelease.json
…
$ ls external-sources/quarto-web/_site/_redirects
external-sources/quarto-web/_site/_redirects
$ ls external-sources/quarto-web/_site/netlify.toml
external-sources/quarto-web/_site/netlify.toml
```

The leading-`/` patterns resolve correctly; the failure is the
directory-resource issue tracked separately.

## Out of scope

- The diagnostic / source-pointing aspect of the error message lives
  in a separate issue (plan
  `2026-05-21-resource-path-diagnostic.md`). The first goal here is
  to stop rejecting *valid* YAML; the second issue improves what we
  say when YAML really is wrong.
- Profile-mode investigation lives in
  `2026-05-21-q2-render-website-profile.md`.
