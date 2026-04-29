# 07-incremental — Incremental rebuilds

Demonstrates Quarto 2's two render modes (full and subset), the
profile cache, and `--clean-cache`.

## What this demonstrates

- **Mode A: full project render.** Re-renders Pass 2 (the body
  pipeline) for every page. Pass 1 (static profile extraction)
  consults a content-hash-keyed cache for unchanged inputs.
- **Mode B: subset render.** Naming specific files renders only
  those files, leaving other `_site/*.html` outputs (and their
  mtimes) unchanged. Pass 1 still walks the whole project so
  cross-page indices stay consistent — but the profile cache
  makes that walk cheap on warm input.
- **Profile cache.** Lives at `<project>/.quarto/cache/profiles/`,
  one JSON file per page keyed by SHA-256 of source plus all
  participating `_metadata.yml` / `_quarto.yml` / include content.
- **`--clean-cache`.** Wipes `<project>/.quarto/cache/` (so the
  next render is fully cold). Does not touch `_site/`.
- **Sitemap incremental merge.** Mode B preserves sitemap entries
  for non-targets; only the rendered pages' `<lastmod>` updates.

## Why no Pass-2 cache

Pass 2 runs user-supplied Lua filters and language engines, which
can have side effects (network calls, time, randomness, file I/O).
Caching the bytes they produce would silently break reproducibility
for any non-pure filter. The narrower `freeze` feature (separate
epic, opt-in per chunk) caches engine output explicitly when the
user knows it's safe.

The win from Mode B isn't "Pass 2 was cached" — it's "Pass 2
didn't need to run for unaffected pages."

## How to run

### Step 1 — Mode A, cold cache

```bash
cargo run --bin q2 -- render examples/websites/07-incremental
```

All five pages render. The directory
`.quarto/cache/profiles/` now contains five files, one per page.

```bash
ls examples/websites/07-incremental/.quarto/cache/profiles/   # → 5 files
```

### Step 2 — Mode A, warm cache

Re-run the same command. All five pages still render their Pass 2
(by design — see "Why no Pass-2 cache" above), but Pass 1 hits the
cache for each, so the cold profile work is skipped.

### Step 3 — Mode B, single file

```bash
cargo run --bin q2 -- render examples/websites/07-incremental/posts/first.qmd
```

Only `_site/posts/first.html` is rewritten. Verify by capturing
mtimes before and after:

```bash
stat -f '%Sm %N' examples/websites/07-incremental/_site/**/*.html
# Run Mode B
stat -f '%Sm %N' examples/websites/07-incremental/_site/**/*.html
# Only posts/first.html should have a newer mtime
```

### Step 4 — Sitemap merge

`_site/sitemap.xml` is updated in place. Only the entry for
`posts/first.html` gets a new `<lastmod>`; entries for the other
four pages are preserved verbatim.

```bash
grep -E '<loc>|<lastmod>' examples/websites/07-incremental/_site/sitemap.xml
```

### Step 5 — Clean cache

```bash
cargo run --bin q2 -- render examples/websites/07-incremental --clean-cache
```

Wipes `.quarto/cache/` before rendering. The next render rebuilds
the whole profile cache from scratch.

## Try it

- Edit a page's body and run Mode B on that file alone. Confirm
  only that page changes on disk.
- Edit a page's frontmatter `title:`. Mode B re-renders the named
  file but won't refresh sibling sidebars yet — see "known
  limitation" below.
- `touch _quarto.yml` (without changing it) and run Mode A.
  Profile cache should still hit (the content hash is unchanged
  even if mtime moved).
- Edit `_quarto.yml` to add a new sidebar entry. Mode A re-renders
  every page (sibling sidebars update). Mode B targeting one page
  would not.

## Known limitation (bd-par3)

Mode B currently does not detect changes to nav config (e.g.
`_quarto.yml`'s sidebar block) and refresh sibling pages whose
sidebars are now stale. The follow-up `bd-par3` adds a
nav-config-hash sentinel: when the hash changes, Mode B augments
its render set with sidebar members of the targets so their nav
HTML stays fresh.

For now: if you change nav config, run Mode A.

## Notes

- Pass-2 cache resumption from a serialized `AtProfile` (Phase 0
  was designed to enable it) is tracked as `bd-ee4z`. Today Pass-2
  re-runs the head pipeline from source on every render.
- Parallel per-file rendering is tracked as `bd-pdwr`.
