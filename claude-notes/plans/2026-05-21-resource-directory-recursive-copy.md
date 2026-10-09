---
title: 'Directory resources should expand to recursive file copies (bd-47w7o)'
date: 2026-05-21
---

## Overview

After bd-wlza2 landed (leading-`/` patterns interpreted as
project-root-relative), `q2 render external-sources/quarto-web`
gets past the original error and copies four of the six declared
resources into `_site/`. It then fails on the fifth:

```
Error: Failed to copy resource
  /…/external-sources/quarto-web/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo
→ /…/_site/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo
: I/O error: the source path is neither a regular file nor a symlink to a regular file
```

`…/demo` is a directory. The current literal-path branch of
`expand_one` returns it as a single `ResolvedResource`, and the
later `copy_resources_to_output_dir` step tries to `file_copy` a
directory and fails.

This issue makes Q2 match TS Quarto: a literal directory resource
recursively expands to every file inside it.

## TS Quarto reference behavior

`external-sources/quarto-cli/src/core/path.ts:227–280` (in
`resolveGlobs`):

```ts
if (glob.endsWith("/")) {
  glob = glob + "**/*";
} else {
  // literal relative reference to any directory means everything in the dir
  const fullPath = join(root, glob);
  try {
    if (Deno.statSync(fullPath).isDirectory) {
      glob = glob + "/**/*";
    }
  } catch {
    // Leave the glob alone, this must not be a directory
  }
}
```

So TS Quarto rewrites both `dir/` (trailing slash) and a bare
literal that resolves to an existing directory into the recursive
glob `dir/**/*`. The quarto-web entry hits the second branch
(`/docs/blog/posts/.../demo` resolves to a real directory).

## Current Q2 behavior

`crates/quarto-core/src/project_resources.rs`:

- **Glob branch (`expand_one`, \~line 196)**: already filters
  `path.is_dir() { continue; }` — only files come through. A
  user-written `/data/**/*` works correctly today.
- **Literal branch (`expand_one`, \~line 230)**: resolves the path,
  validates project-containment, and returns a single PathBuf. No
  `is_dir` check; downstream `file_copy` fails.

The existing in-code comment ("Q1 requires explicit `dir/**/*` for
recursive copy") is wrong about TS Quarto's actual behavior. The
comment needs to be revised alongside the fix.

## Design

In the literal branch, after `canonicalize_within_project`
succeeds, check whether the canonical path is a directory. If so,
recursively enumerate every file under it. Implement by reusing
the existing `glob::glob` machinery with the `<canonical>/**/*`
pattern — same skip-directories filter, same project-containment
check.

```rust
// literal branch — pseudocode
let absolute = base.join(pat);
let canonical = canonicalize_within_project(project_root, &absolute, pattern)?;
if canonical.is_dir() {
    expand_directory_recursive(project_root, &canonical, pattern)
} else {
    Ok(vec![canonical])
}
```

Where `expand_directory_recursive` is a small helper that builds
`format!("{}/**/*", canonical.display())`, runs `glob::glob`,
filters directories, runs each match through
`canonicalize_within_project` (defense in depth — symlinks could
re-escape; the existing prefix check is what enforces containment),
and returns the file list.

The `is_dir` check happens **after** canonicalization — if the user
wrote a relative path that doesn't exist on disk yet,
`canonicalize` falls back to `lexical_normalize`, the resulting
path probably doesn't exist, `is_dir` returns false, and we fall
through to the single-path return. The downstream "Declared
resource '…' does not exist on disk" error in
`copy_resources_to_output_dir` then fires — same diagnostic users
get today for typos.

### Why glob, not `walkdir`

- `glob` is already a workspace dep, used by the sibling branch.
- The `<dir>/**/*` rewrite is exactly TS Quarto's rewrite (line
  273 of `path.ts`), so the parity argument is explicit at the
  code level.
- `walkdir` is a heavier hammer — symlink loops, error semantics,
  options like `follow_links`. We don't want any of that here;
  `glob` already gives us the project-containment guard for free.

### Symlink consideration

`canonicalize_within_project` resolves symlinks via
`std::fs::canonicalize`, so a symlinked directory inside the
project that points *outside* the project will fail the
`starts_with(project_root)` check, same as today. After fix this
check fires on each enumerated file individually, not just the
top-level dir. That's safer, not less safe.

### What about glob-pattern directories?

`/data/*` (existing glob branch) is intentionally *files-only* —
matching directories would be ambiguous (Q1 had this same
behavior). Out of scope for this issue. Only the *literal*
directory case (no glob chars, resolves to a directory) gains
recursive semantics. The in-code comment ("a directory match would
be ambiguous") on the glob branch's `path.is_dir() { continue; }`
remains accurate and stays.

## Test plan (TDD)

Tests go in `crates/quarto-core/src/project_resources.rs` alongside
existing `expand_*` tests. All use `TempDir`.

1. **`expand_literal_directory_recursively_enumerates_files`** —
   Create `<root>/demo/a.html`, `<root>/demo/sub/b.css`,
   `<root>/demo/sub/c.png`. Pattern `"demo"` (no leading slash,
   no glob). Assert 3 ResolvedResources, with the expected
   `output_relative` values (`demo/a.html`, `demo/sub/b.css`,
   `demo/sub/c.png`). Sort first; ordering is filesystem-dependent.
2. **`expand_literal_directory_with_trailing_slash_works`** —
   Same fixture, pattern `"demo/"`. Asserts same 3 results.
   Important because TS Quarto handles trailing slash and bare
   literal symmetrically.
3. **`expand_leading_slash_directory_recursively_enumerates_files`** —
   Pattern `"/demo"` (leading slash). Confirms the bd-wlza2 path
   composes correctly with this fix: leading-`/` strips to
   project-relative, then directory expansion fires.
4. **`expand_literal_directory_skips_nested_dirs_keeps_their_files`** —
   Single subdir, no files in it. Result should be empty (the
   subdir itself is filtered out; no files inside to include).
   Guards against accidentally including the directory entry as a
   "resource".
5. **`expand_literal_nonexistent_path_returns_single_entry`** —
   Regression: pattern `"missing.txt"` (no file on disk).
   Asserts 1 result (the unresolved path); downstream error
   surfaces as before, not silently swallowed.
6. **`expand_literal_file_unchanged`** — Regression: pattern
   `"a.txt"` with a real file. Asserts 1 result, matching the
   existing `expand_literal_path` test exactly. Belt-and-braces;
   already covered.

End-to-end (required per CLAUDE.md before declaring success):

```
cargo run --release --bin q2 -- render external-sources/quarto-web
```

After the fix, the demo-directory copy step should succeed; the
render may still fail further along for unrelated Q2-not-yet-Q1
reasons (sidebar deep customization, shortcodes, etc.). Record:

- the previous failure point (this issue, demo dir);
- the new failure point (whatever fires next, or "render
  completes" if quarto-web actually goes all the way through);
- a sample of files copied into
  `_site/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo/`
  to confirm recursive copy actually happened.

Each new downstream issue gets its own bd issue, *not* folded
into this one (CLAUDE.md "isolate and fix one bug at a time").

## Work Items

- [x] Add the 6 tests above. Confirmed tests 1–4 fail pre-fix;
  tests 5–6 pass pre-fix.
- [x] Implement the directory-detect + recursive-expand path in
  the literal branch of `expand_one`. Factored the glob-expansion
  loop into a shared `expand_glob_files` helper used by both the
  glob branch and the synthesized-from-directory case.
- [x] Update doc comment on `expand_patterns` to describe
  directory semantics; revise the in-branch comment so it no
  longer claims `dir/**/*` is the only way (it was the only way
  pre-bd-47w7o).
- [x] Re-run the 6 tests; all pass (2086 quarto-core tests total).
- [x] `cargo nextest run -p quarto-core` clean.
- [x] `cargo xtask verify --skip-hub-build` clean.
- [x] End-to-end on quarto-web (see transcript below).
- [x] Updated plan with e2e transcript and follow-up issue.

## End-to-end transcript (2026-05-21)

```
$ rm -rf external-sources/quarto-web/_site
$ cargo run --quiet --bin q2 -- render external-sources/quarto-web
error: …/website-blog.qmd: Invalid theme configuration: theme must be a string or array of strings
error: …/website-llms.qmd: Invalid theme configuration: theme must be a string or array of strings
error: …/index.qmd: Invalid theme configuration: theme must be a string or array of strings
error: …/license.qmd: Invalid theme configuration: theme must be a string or array of strings
error: …/trademark.qmd: Invalid theme configuration: theme must be a string or array of strings

$ find external-sources/quarto-web/_site/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo -type f | wc -l
70
$ find external-sources/quarto-web/docs/blog/posts/2024-07-02-beautiful-tables-in-typst/demo -type f | wc -l
70
```

Demo directory: 70 source files, 70 destination files. The
recursive-copy path works. The render then fails on an unrelated
issue — quarto-web's `_quarto.yml` uses the dark-mode form
`theme: { light: [cosmo, theme.scss], dark: [cosmo, theme-dark.scss] }`,
which Q2 doesn't yet parse. Filed as **bd-0pic6**
(discovered-from bd-47w7o).

## Out of scope

- Changing the glob branch's `path.is_dir() { continue; }`
  behavior — that's a separate semantic question (TS Quarto's
  `expandGlobSync` does include dirs with `includeDirs: true`,
  but they're filtered downstream; we'd need to verify exactly
  what they become before changing ours).
- Performance: copying a huge resource directory is one-by-one
  today. If quarto-web's `demo/` turns out to be large (it isn't;
  it's a small demo), we can optimize later. Filed under bd-9eltv
  (profile work).
- Adding source-info to the directory-resolved entries — the
  diagnostic plan (bd-c1et2) covers the source-info threading; it
  applies here too, but lands as part of that issue, not this one.

## Dependencies

- Blocked by: **bd-wlza2** (already landed). Without the
  leading-`/` fix, the directory-resource case is shadowed by
  the prior error.
- Sibling: **bd-9eltv** (profile work) is itself blocked by this
  issue (or by accepting an intermediate failure as the
  profiling baseline). The profile plan calls out that further
  blockers are *expected* and become their own issues.
