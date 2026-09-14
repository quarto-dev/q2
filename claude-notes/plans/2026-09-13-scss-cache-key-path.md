# SCSS cache key hashes the document-relative theme path (bd-79c4do6g)

**Date:** 2026-09-13
**Braid:** bd-79c4do6g (P1, bug, label `perf`)
**Branch:** `braid/bd-79c4do6g-scss-cache-key-path` in the main checkout (based on `main` @ `35bc11415`; topic branch, no worktree, per user request)
**Status:** Design agreed 2026-09-14; implementing.

## Triage verdict

**Ready to design.** The root cause is confirmed at HEAD with a 3-document
repro (see below), the fix is a few lines in one function, and the only
real decision is *which* spelling-independent identity to hash (see the
design questions — there is a subtle correctness reason not to simply
drop the path).

One soft dependency: the `perf.sass` gauge the strand says to verify
with is **not on `main`** — it is uncommitted work on the parent strand's
branch (`braid/bd-fq44dlnm-connect-docs-profile`, room-5). Verification
here can use a cache-directory file count instead (the repro does), so
this is not blocking.

## Issue context

Filed 2026-09-13 by Carlos from the Connect-docs render profile
(bd-fq44dlnm). `compile_theme_css::cache_key` hashes, for each
`ThemeSpec::Custom(path)`, the string form of
`theme_context.resolve_path(path)` — i.e. `document_dir.join(path)` —
*and* the file contents. Because the merged metadata carries a
**document-relative** spelling of the theme path, the string differs
per document directory even though it names the same file. On the
Connect docs (352 docs across 349 directories) that means ~700 distinct
keys for ~4 distinct compiled outputs: `perf.sass hits=22 compiles=682`,
~48 ms per grass compile, 78 % of the serial render (42 s wall).

Research note (on the parent branch, not yet merged):
`claude-notes/research/2026-09-13-connect-docs-render-profile.md` §1.

## Dependency graph

- **discovered-from → bd-fq44dlnm** (in_progress, P2): "Perf: time-profile
  q2 render of the Connect docs". The profile session found this as the
  #1 hotspot. Its branch also carries the `perf.sass` gauge
  (`sass_perf` atomics + `print_sass_stats_if_enabled`, wired into
  `q2 render`) that this strand's description says to verify with.
  Uncommitted there as of this investigation.
- **related ← bd-ddahjqr1** (open, P2): "LRU sass cache leaks orphaned
  entries under parallel Pass 2 (index lost-update)". Separate bug in
  `cache_set_lru`'s read-modify-write of the LRU index. Interaction:
  the orphan leak is *bounded* once this strand shrinks the key space
  to a handful of keys, and orphans currently mask ~30 % of this bug
  (they still serve `cache_get` hits by key). Fixing this one first
  makes that one less urgent; the two fixes don't overlap in code.
- No `blocks` edges in either direction. No epic parent.

## What the code looks like today

All paths in the strand still exist with the described shape.

**The key** — `crates/quarto-core/src/stage/stages/compile_theme_css.rs:218-228`:

```rust
ThemeSpec::Custom(path) => {
    let resolved = theme_context.resolve_path(path);   // document_dir.join(path)
    hasher.update(b"custom:");
    hasher.update(resolved.to_string_lossy().as_bytes()); // ← varies per doc dir
    hasher.update(b"\n");
    let contents = runtime.file_read(&resolved)...;
    hasher.update(&contents);
}
```

`ThemeContext::resolve_path` (`crates/quarto-sass/src/themes.rs:455`) is
a plain `join` — no normalization. `document_dir` is
`ctx.document.input.parent()`, which is project-relative in `q2 render`
(hence strings like `admin/appendix/cli/../../../_extensions/…`).

**Why the spelling varies** — two producers, both by design:

1. Extension-contributed themes: `project/mod.rs::rebase_fragment_paths`
   (line 729) turns `theme.scss` into
   `ConfigValueKind::Path("_extensions/posit-dev/posit-docs/theme.scss")`
   (project-root-relative).
2. Every `Path`-marked value is then rewritten **per document** by the
   metadata merge: `project/format_paths.rs::mark_entry` (line ~250)
   stores `pathdiff::diff_paths(&source, document_dir)`, i.e.
   `../../theme.scss` for a doc two levels deep. This applies to *any*
   custom theme declared in `_quarto.yml` / `_metadata.yml`, not just
   extension ones — so the bug hits every multi-directory project with
   `theme: [my.scss]`, brand-less or not.

Net: same file, one spelling per directory depth/location, one cache
key per spelling. The 10 MB LRU (`SASS_CACHE_BUDGET_BYTES`, ~337 KB per
entry → ~31 entries) then thrashes.

**Where the path legitimately matters.** `process_theme_specs`
(`themes.rs:748-754`) pushes `resolved_path.parent()` onto the sass load
paths, so `@import "_colors"` inside the theme resolves relative to the
theme file's directory. Every rebased spelling has the same parent
directory (modulo `..` segments grass resolves), so **for one file** the
path contributes nothing the contents don't already cover. But the path
*is* doing one job today: it distinguishes two theme files in different
directories whose **text** is identical but whose **imports** differ
(`a/theme.scss` and `b/theme.scss` both `@import "_colors"`, with
different `a/_colors.scss` / `b/_colors.scss`). Dropping the path
outright would alias those. See design question 1.

**Related pre-existing gap (not this strand's bug):** the key hashes
only the top-level theme file. Editing an imported partial
(`_posit-colors.scss` in the Connect docs, `_colors.scss` in the repro)
does **not** invalidate the cache — a stale-CSS bug that exists today
regardless of this fix. Filed as **bd-m3hga05o**.

**Existing test that encodes the current behaviour:**
`test_cache_key_custom_file_reads_content` (compile_theme_css.rs:2289)
asserts that `theme_a.scss` and `theme_b.scss` with identical (empty)
content produce *different* keys. Under "drop the path" that test
inverts; under "normalize the path" it still passes. Either way its
intent needs restating.

### Repro at HEAD

Fixture: `claude-notes/plans/scss-cache-key-path-investigation/fixture/`
— a website project with `theme: [theme.scss]` (which `@import`s
`_colors.scss`) and three documents at depths 0, 1, 2.

Commands (from the fixture directory, `main` @ `35bc11415`):

```bash
rm -rf .quarto/cache/sass _site
cargo run -q --bin q2 -- render .
ls .quarto/cache/sass | grep -v -E '_lru_index|_version' | wc -l      # → 3
for f in .quarto/cache/sass/*; do shasum -a 256 "$f" | cut -c1-16; done | sort | uniq -c
```

Observed (output inspected):

| render                       | sass cache entries | distinct CSS contents |
|------------------------------|-------------------:|----------------------:|
| cold, 3 docs at depth 0/1/2  |                  3 |                     1 |
| warm, same project again     |                  3 (all hits) |          1 |

Three keys for one compiled output — one per document directory,
exactly the strand's mechanism at a scale where it's legible. Expected
after the fix: **1** entry. The three entries are byte-identical
(334,754 B each; same sha256). The rendered site links
`site_libs/quarto/quarto-theme-1dea42e453982763.css`, which contains
the fixture's `.repro{color:#123456}` rule, so the custom theme is
genuinely in play.

Stale-partial check (same session): after `sed` changing `_colors.scss`
to `$repro-fg: #abcdef` and re-rendering, the emitted CSS still reads
`.repro{color:#123456}` and the cache dir still holds the same 3
entries — the partial's edit never reaches the key. Filed as bd-m3hga05o; not in this strand's scope unless option (c) in Q1 is
chosen.

## Proposed phases (draft)

Skeleton only — actual phase contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).**
  - Unit test in `compile_theme_css.rs`: two `ThemeContext`s with
    different `document_dir`s and correspondingly different relative
    spellings of the *same* file (`theme.scss` from `/project`,
    `../theme.scss` from `/project/a`, `../../theme.scss` from
    `/project/a/b`) must yield **one** key. Uses a runtime whose
    `file_read` returns fixed content for the normalized path. Must
    fail at HEAD.
  - Keep-distinct test: two different files in different directories
    with identical contents must still yield distinct keys (pins the
    import-context property; see Q1).
  - Restate `test_cache_key_custom_file_reads_content`'s intent.
  - End-to-end: render the repro fixture through `q2 render` with a
    cleared cache and assert the sass cache dir holds one entry (or,
    once the gauge lands, `compiles=1 hits=2`).
- **Phase 1 — Core change.** Make the hashed identity spelling-independent
  (per Q1): normalize `resolved` before hashing, or replace the path
  component with a canonical identity.
- **Phase 2 — Measure.** Re-run the Connect-docs serial render with a
  cleared cache; expect `compiles` ≈ number of distinct variants (4)
  and wall time to drop from ~42 s toward the ~9 s the research note
  projects for the non-SCSS remainder. Record numbers here.
- **Phase 3 — Docs/notes.** Update the `cache_key` doc comment (it
  currently says "Custom themes contribute their resolved path and
  file contents"); no user-facing docs change.

## Decisions (2026-09-14, with user)

1. **Identity:** (a) — lexically normalize the resolved path (collapse
   `.`/`..` via `components()`), hash that. (c) is bd-m3hga05o, worked
   by a separate agent in parallel.
2. **Where:** normalize inside `ThemeContext::resolve_path` itself, so
   load paths and diagnostics also read `_extensions/…/theme.scss`
   instead of `admin/…/../../../_extensions/…`. Snapshot churn accepted.
3. **Gauge:** the room-5 branch had **no commits** — the gauge was an
   uncommitted 3-file diff. Applied here as its own commit
   (`aab25a7e1`, "Add perf.sass gauge"), code only; the research note
   and profiles stay with bd-fq44dlnm. When that branch later commits
   the same gauge, the merge is textually identical.
4. **Sequencing:** this fix first, then bd-ddahjqr1 (LRU index race) as
   a follow-up commit on the same branch → one PR, individually
   reviewable commits.

## Work items

- [x] Bring the `perf.sass` gauge over (`aab25a7e1`).
- [x] Phase 0 — failing tests: `resolve_path` normalization
      (quarto-sass), one-key-for-many-spellings (`cache_key`),
      keep-distinct restated, end-to-end fixture render → `compiles=1`.
- [x] Phase 1 — normalize in `ThemeContext::resolve_path` (`quarto_util::normalize_lexically`). No snapshot churn materialized: no snapshot pinned a `../` theme path.
- [ ] Phase 2 — measure on the Connect docs (cold serial): expect
      `compiles≈4`, wall ≪ 42 s. Record here.
- [x] Phase 3 — `cache_key` doc comment; plan + strand notes.
- [ ] Follow-up commit: bd-ddahjqr1.

## End-to-end verification (fixture, after the fix)

Invocation, from the fixture dir, output inspected:

```bash
rm -rf .quarto/cache/sass _site
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .
#   perf.sass hits=2 compiles=1 uncached=0        (before: hits=0 compiles=3)
ls .quarto/cache/sass | grep -v -E '_lru_index|_version' | wc -l   # 1  (before: 3)
QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .              # warm: hits=3 compiles=0
```

Every page links the same `site_libs/quarto/quarto-theme-1dea42e453982763.css`,
which still contains the fixture's `.repro{color:#123456}` rule.

**Observation — cold parallel start is a thundering herd.** With the
default job count the cold gauge reads `compiles=3 hits=0` even though
only one entry lands in the cache: all three pages miss before the
first compile finishes. On the Connect docs that bounds the cold cost
at ~`jobs` compiles per variant (16 workers → ≤ 64 compiles, not 704),
and every later page hits. Single-flighting the compile per key would
close that; it is a natural companion to bd-ddahjqr1's in-process
index and is noted there rather than done here.

## Open design questions for the user (answered above; kept for the record)

1. **What identity replaces the raw path string?** Three options, in
   increasing ambition:
   - (a) **Lexically normalize** the resolved path (collapse `.`/`..`
     via `components()`, as `Vfs::normalize_components` already does)
     and hash that. Cheapest; works on every runtime (native, sandbox,
     WASM VFS); keeps the "different directory ⇒ different key"
     property that protects divergent imports. Fails only for symlinks
     and for the same project rendered from different cwds (harmless:
     an extra key, never a wrong hit).
   - (b) **`runtime.canonicalize()`** — resolves symlinks too, but hits
     the filesystem per variant per doc and isn't meaningful on the
     WASM VFS. Not obviously better than (a) for this bug.
   - (c) **Hash the transitive import closure's contents** instead of
     any path — fixes both this bug *and* the stale-partial gap above,
     and makes the key purely content-addressed. Needs an import
     resolver in `cache_key` that mirrors grass's (load paths, partial
     `_` prefix, `.scss`/`.sass`/`.css`, `url()` skipping). Real work,
     and a separate strand's worth of scope.
   My recommendation: **(a) now**, (c) is bd-m3hga05o, which
   also closes the stale-partial gap. Agree?
2. **Should `ThemeContext::resolve_path` itself normalize?** It feeds the
   load path (`theme_dir`) and every diagnostic path (`Q-14-4` "custom
   theme not found", `InvalidScssFile`). Normalizing there would make
   error messages read `_extensions/…/theme.scss` instead of
   `admin/appendix/cli/../../../_extensions/…/theme.scss` — nicer, but
   wider blast radius (snapshot tests on diagnostics). Or keep
   `resolve_path` as is and normalize only inside `cache_key`?
3. **Verification gauge.** The strand says "verify with
   `QUARTO_PERF_STATS=1 → perf.sass`", but that gauge lives on the
   unmerged bd-fq44dlnm branch. Options: (i) wait for that branch to
   land and base this on it; (ii) base on `main` and verify by counting
   `.quarto/cache/sass` entries (the repro does this); (iii) cherry-pick
   the gauge commit onto this branch once it exists. Preference?
4. **Sequencing vs. bd-ddahjqr1 (LRU index race).** Fix this first (it
   shrinks the key space and makes the leak bounded), or both together
   in one PR? They don't touch the same lines.

## Risks / tradeoffs (draft)

- **Aliasing risk if the path is dropped entirely.** Covered above; the
  keep-distinct test in Phase 0 guards it. Normalizing rather than
  dropping avoids the risk.
- **Cross-platform.** Path normalization must go through
  `Path::components()`, not string munging; `to_forward_slashes` is
  already the repo convention for hashing/serializing paths. Windows
  `..` handling and drive prefixes are the usual traps.
- **Existing sass cache contents.** Keys change for every custom theme,
  so the first render after the fix recompiles once per variant and
  the old entries age out of the LRU. No purge needed (the
  `CACHE_VERSION_KEY` generational purge is for resource-hash bumps, not
  key-scheme changes — worth confirming whether to bump it anyway).
- **The measured win depends on `doc_vars`.** The research note found
  two legitimately distinct outputs per variant (navbar/footer layer
  driven); those keys must stay distinct, and the fix doesn't touch
  `doc_vars` hashing. Expect `compiles≈4`, not `2`, on the Connect docs.
