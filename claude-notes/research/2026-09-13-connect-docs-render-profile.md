# Profiling `q2 render` on the Posit Connect docs (docs-quarto-2)

**Date:** 2026-09-13
**Strand:** bd-fq44dlnm
**Plan:** [`claude-notes/plans/2026-09-13-connect-docs-render-profile.md`](../plans/2026-09-13-connect-docs-render-profile.md)
**Fixture:** `~/repos/github/cscheid/q2-connect-docs/docs-quarto-2` (not in this repo)
**Profiles:** `2026-09-13-connect-docs-{serial,parallel}-profile.json.gz` (+ `.syms.json`) in this directory
**Machine:** Apple M5 Max, 18 cores, 128 GB, macOS 26.6.2; q2 at `35bc1141` (main)

> **Status (2026-09-17):** findings 1 and 2 were fixed on `main` before this
> note merged — PR #679 (`4a2d219f` normalizes resolved theme paths so one
> custom theme gets one sass cache key; `fbda74d9` serializes and reconciles
> the LRU index so parallel renders stop leaking entries; `aab25a7e` landed
> the `perf.sass` gauge described below). The follow-up bd-m3hga05o / PR #682
> (`267d24f3`) then made the key validate `@import`ed partials, which the
> content-only key would otherwise have missed. The numbers in this note are
> the pre-fix baseline at `35bc1141`; bd-is4q72tt, bd-j0hmi3rx and
> bd-5yektmwt remain open.

## TL;DR

**78 % of the serial render is compiling Bootstrap SCSS, over and over.**
The runtime `sass` cache exists and is consulted, but its key hashes the
*document-relative* path of the extension's theme file, so every
directory gets its own key and the 10 MB LRU thrashes:

```
perf.sass hits=22 compiles=682 uncached=0     # 704 variant-compiles, 352 docs × {light, dark}
```

Fixing the key (bd-79c4do6g) should take the serial render from ~42 s to
roughly 9 s and the parallel render from ~4 s to well under 2 s on this
machine, with no other change. Three smaller findings fell out on the way:
the LRU index loses updates under parallel Pass 2 and leaks orphan cache
files (bd-ddahjqr1, 101 MB observed); peak RSS is ~1000× the source size
(bd-is4q72tt); and a single-page render inside the site re-renders all 16
listing pages (bd-j0hmi3rx). After SCSS, the profile is the familiar
AST-construction `memmove` + tree-sitter shape (bd-5yektmwt).

## Fixture

| section  | inputs | bytes     | notes                                        |
|----------|-------:|----------:|----------------------------------------------|
| admin    |    173 | 1,634,947 | deep sidebar; generated configuration appendix |
| api      |      2 | 1,144,693 | one 1.1 MB generated `api/index.qmd` (2.0 MB HTML) |
| cookbook |    117 |   382,056 | 16 listing pages live here                    |
| user     |     76 |   536,598 |                                              |
| how-to   |      5 |    35,459 |                                              |
| news     |      3 |     1,199 |                                              |

352 rendered inputs (206 `.qmd` + `.md`) in **349 distinct directories**
(almost every page is `<dir>/index.md`), 2.3 MB of markdown. Project type
`posit-docs` (an extension contributing `theme: {light: [theme.scss],
dark: [theme-dark.scss]}`, `highlight-style: {github, arrow}`), plus the
`quarto-openapi`, `mermaid-zoom`, and `quarto-tiers` extensions;
`llms-txt: true`; pre/post-render scripts commented out locally. Renders
clean (exit 0, no diagnostics) to a 78 MB `_site/`.

## Method

Per `claude-notes/instructions/performance-profiling.md`:

1. `cargo build --release --bin q2` and `--profile=release-perf` (symbols).
2. `/usr/bin/time -l` for wall + RSS, `QUARTO_PERF_STATS=1` for the gauges.
   `_environment` sourced into the shell (`CONNECT_VERSION` etc.).
3. `samply record -s -n --unstable-presymbolicate` at 1 kHz on the
   release-perf binary, serial (`QUARTO_JOBS=1`) and parallel (default,
   16 workers).
4. `crates/perf-harness/scripts/analyze_profile.py` for per-symbol self
   time; the new `bucket_profile.py` (added with this note) for per-crate
   buckets, per-stage inclusive time, and callers of a target symbol.
5. A `perf.sass hits= compiles= uncached=` gauge added to
   `compile_theme_css.rs` (printed from `q2 render` like the other
   `perf.*` gauges) to turn the profile's suspicion into a count.

## Numbers

### Wall time and memory (release, median-ish of 3)

| run                                  | wall    | user   | max RSS |
|--------------------------------------|--------:|-------:|--------:|
| cold (`--clean-cache`), 16 workers   | 5.1–6.3 s | ~56 s | 3.1–3.6 GB |
| warm, 16 workers                     | 3.5–4.3 s | ~46 s | 3.1 GB |
| warm, `QUARTO_JOBS=1`                | 29.8 s  | 28.5 s | 2.0 GB |
| warm, `QUARTO_JOBS=1`, **sass cache cleared first** | **42.0 s** | 40.2 s | 2.5 GB |
| warm, 16 workers, sass cache cleared first | 4.55 s | 60.5 s | 3.6 GB |
| `render api/index.qmd` alone (1.1 MB), serial | 1.6 s | 1.35 s | **1.1 GB** |
| `render user/index.md` alone (small), serial  | 1.85 s | 1.66 s | 158 MB |

Gauges (serial, cleared sass cache):

```
perf.engine-discover jupyter=1 rscript=1
perf.pass1 docs=352 threads_used=1 wall_ms=36
perf.pass2 docs=352 threads_used=1 wall_ms=41577
perf.pass1-engine-resolution lifted=352 fell_through=0
perf.sass hits=22 compiles=682 uncached=0
```

Parallel: `perf.pass2 docs=352 threads_used=16 wall_ms=4149`,
`perf.sass hits=20 compiles=684`. Pass 1 is ~30 ms and irrelevant. The
16-worker speed-up on Pass 2 is 10× (41.6 s → 4.1 s) at 1.5× the CPU
(40 → 60 s user); lock waits (`__ulock_wait*`, `__psynch_cvwait`) are
~1 % of parallel samples — the locale-lock pathology of bd-b7eb7 is
still gone.

The 30 s "warm serial" row is faster than the 42 s "cleared" row only
because ~315 *orphaned* cache files from earlier runs were serving hits
(see finding 2) — the number that reflects the code is 42 s.

### Where the time goes (serial samply, 29,492 samples)

Self time by crate / subsystem (`bucket_profile.py`):

| bucket                 | serial | parallel |
|------------------------|-------:|---------:|
| grass (SCSS compiler)  | 39.3 % | 37.0 % |
| libsystem_malloc       | 25.8 % | 28.1 % |
| memmove / memset / memcmp | 12.9 % | 11.8 % |
| tree-sitter            |  3.2 % |  2.7 % |
| hashing (SipHash, SHA-256) | 2.6 % | 2.3 % |
| pampa                  |  0.9 % |  0.8 % |
| quarto_core            |  0.5 % |  0.5 % |
| scraper / html5ever    |  0.3 % |  0.3 % |

Inclusive time by leaf-most pipeline stage:

| stage                | serial | parallel |
|----------------------|-------:|---------:|
| **compile_theme_css** | **78.4 %** | **75.6 %** |
| parse_document       |  6.0 % |  5.7 % |
| ast_transforms       |  3.7 % |  4.2 % |
| apply_template       |  3.4 % |  3.9 % |
| user_filters         |  2.1 % |  2.4 % |
| include_expansion    |  1.1 % |  1.2 % |
| listing              |  0.8 % |  0.8 % |

76.9 % of all samples have a `grass_compiler` frame on the stack; 70.2 %
sit directly under
`quarto_system_runtime::sass_native::compile_scss_with_embedded`. Most of
the malloc / memmove self time is grass's (`Vec<AstStmt>::clone` 3.6 %,
`AstExpr::clone` 1.8 %, `Value::clone` 1.4 %, `drop_in_place<Value>`
1.5 % are the top Rust symbols after `visit_expr` 4.2 %).

Top self-time symbols, serial:

```
 8.71%  _platform_memmove
 4.20%  <grass_compiler::evaluate::visitor::Visitor>::visit_expr
 3.55%  <Vec<grass_compiler::ast::stmt::AstStmt> as Clone>::clone
 2.19%  <grass_compiler::evaluate::env::Environment>::get_var
 2.19%  _platform_memset
 2.02%  0x2a15c [libsystem_malloc.dylib]
 1.75%  <grass_compiler::ast::expr::AstExpr as Clone>::clone
 1.74%  <core::hash::sip::Hasher<Sip13Rounds> as Hasher>::write
 1.53%  drop_in_place::<grass_compiler::value::Value>
 1.51%  <grass_compiler::evaluate::visitor::Visitor>::visit_stmt
```

### What the render looks like *without* the SCSS problem

Dropping every sample with a `compile_theme_css` frame leaves 6,375
samples (21.6 %) — a projection of the post-fix render:

| bucket (of remaining)     |  share | stage (inclusive, of remaining) | share |
|---------------------------|-------:|---------------------------------|------:|
| memmove / memset / memcmp | 28.0 % | parse_document                  | 27.7 % |
| libsystem_malloc          | 17.5 % | ast_transforms                  | 17.0 % |
| tree-sitter               | 14.9 % | apply_template                  | 15.7 % |
| kernel syscalls (`open`, `stat`, `getattrlist`) | 7.0 % | user_filters | 9.9 % |
| pampa                     |  4.4 % | include_expansion               |  5.3 % |
| quarto_core / pandoc_types / source_map | 6.5 % | pass2_renderer     |  5.1 % |
| scraper / html5ever       |  1.4 % | listing                         |  3.5 % |
| lua                       |  1.3 % | code_highlight                  |  2.0 % |

`_platform_memmove` alone is 25.7 % of the remainder — the diffuse
AST-construction copying the 2026-06-01 profile already flagged as the
"largest lever, architectural". Nothing else stands out; the
`whitespace_re` recompile, the per-doc `jupyter` spawn, the eager temp
dir, and the tree-sitter logger from earlier profiles are all absent.

## Findings

### 1. The sass cache key varies per document directory (bd-79c4do6g, P1)

`compile_theme_css::cache_key` hashes, for each `ThemeSpec::Custom(path)`:

```rust
let resolved = theme_context.resolve_path(path);     // document_dir.join(path)
hasher.update(b"custom:");
hasher.update(resolved.to_string_lossy().as_bytes()); // ← the path string
hasher.update(b"\n");
hasher.update(&contents);                             // the file contents
```

For an extension-bundled theme, `project/mod.rs::rebase_fragment_paths`
turns `theme.scss` into a `ConfigValueKind::Path` that is
project-root-relative, and — per its own doc comment — "the per-document
metadata merge keeps adjusting them (project root → document dir) for
documents in subdirectories". So each page's merged metadata carries a
*different* string for the same file (`../../_extensions/…/theme.scss`,
`../../../_extensions/…`, …), `resolve_path` joins it onto a different
`document_dir`, and the hash differs — 349 directories → ~349 × 2 keys
for content that is byte-identical (the compiled CSS has exactly two
distinct outputs per variant across the whole site; see below).

The 10 MB LRU holds 31 entries of ~337 KB, so the miss rate is 97 %:
`hits=22 compiles=682`. At ~48 ms per grass compile (small-doc run:
34 compiles in 1.3 s) that is ~33 s of the 42 s serial render.

Evidence chain: samply → `compile_theme_css` 78 % inclusive → cache dir
inspection (315 files, 2 distinct sizes, all written today) → cleared
cache + gauge (`compiles=682`) → source.

**Fix shape.** The contents are already hashed, so the path is redundant
for cache identity. Either drop `resolved` from the hash, or canonicalize
it (`Path::components()`-normalize or `fs::canonicalize`) so equal files
hash equal. The only thing the path *could* legitimately affect is
`@import` resolution relative to the theme file's own directory, which is
identical for every rebased spelling. Verify with `QUARTO_PERF_STATS=1`:
`perf.sass compiles` should equal the number of distinct compiled
variants (here 4: two light, two dark) and `hits` ≈ 2 × docs.

Two legitimately distinct outputs per variant exist:
`quarto-theme-38879b33bfb3e941.css` (338,637 B) vs
`quarto-theme-b5694da3ba3ddd4c.css` (337,773 B) differ only in the
`header.headroom` / `.nav-footer .toc-actions` rules — some page class
compiles without the navbar/footer layer. That is `doc_vars`/layer-driven
and *should* stay a distinct key; the fix must not collapse it.

### 2. LRU index loses updates under parallel Pass 2; orphans leak (bd-ddahjqr1, P2)

`cache_set_lru` is write-value → load index → upsert → evict → store
index, with no lock. `cache_lru.rs` documents the race as benign ("a
wrongly-evicted hot entry is recompiled"), but the other direction is a
leak: an entry whose index upsert loses is on disk forever, untracked and
never evicted. Measured from `rm -rf .quarto/cache/sass`:

| run                | files | index entries | index bytes | disk  |
|--------------------|------:|--------------:|------------:|------:|
| serial             |    31 |            31 | 10,478,387  | 10.3 MB |
| serial again       |    31 |            31 | 10,478,387  | 10.3 MB |
| + one parallel     |    70 |            31 | 10,473,547  | 23 MB |
| + second parallel  |    93 |            31 | 10,473,547  | 31 MB |

The project's cache dir held **315 files / 101 MB** before this session.
Because `cache_get_lru` looks up by key on the backend (not via the
index), orphans still serve hits — which is why the 30 s serial number
looked better than the 42 s cleared-cache truth. Finding 1 makes this
visible; without finding 1 the key space is ~4 and the leak is bounded.
Still worth fixing (per-process in-memory index flushed once, or a mutex
around the RMW, or reconcile index against directory on load).

### 3. Memory: ~1000× the source (bd-is4q72tt, P2)

`api/index.qmd` (1.1 MB) alone peaks at 1.1 GB RSS; a small page at
158 MB (that run also compiled SCSS 34 times, see 4); the full serial
render at 2.0–2.5 GB and the parallel render at 3.0–3.6 GB
(`peak memory footprint` 2.9 GB). This profile was CPU-only, so the
attribution is open: Pandoc AST + `SourceInfo` per node, JSON
intermediates, grass AST clones, project artifacts retained until end of
render (bd-w5qyuzeg). Re-measure after bd-79c4do6g, then heap-profile
the single `api/index.qmd` render.

### 4. Single-page render re-renders every listing page (bd-j0hmi3rx, question)

`q2 render user/index.md` inside the site: `perf.sass hits=4
compiles=30` → 17 documents through the pipeline for one requested page
(16 listing pages + the page), 1.85 s wall — slower than rendering the
1.1 MB API page alone. `listing` (12 %) and `dependency_graph` (2 %) are
visible in that profile. Whether all 16 must re-render (vs. only the
listings that include the page, or none when its profile is unchanged) is
a design question, not a bug per se.

### 5. After SCSS: AST construction (bd-5yektmwt, P3)

See the projection table above. Same shape as 2026-06-01, now the
dominant term once finding 1 lands. Attribute `memmove` to callers
(`bucket_profile.py <profile> _platform_memmove`) before designing.

## Comparison with the 2026-06-01 qmd-plans profile

| bucket               | 2026-06-01 (qmd-plans, cosmo) | 2026-09-13 (Connect, excl. SCSS) |
|----------------------|------------------------------:|---------------------------------:|
| tree-sitter parse    | ~29 % | ~15 % |
| memmove / AST build  | ~13 % | ~28 % (+17.5 % malloc) |
| filesystem syscalls  | ~14 % | ~7 % |
| regex compilation    | fixed | absent |
| SHA-256 cache keys   | ~1.7 % | <1 % |

The June profile used a built-in Bootswatch theme, for which the cache
key is `builtin:<name>` and never varies — finding 1 is specific to
**custom / extension theme files**, which is why three months of
profiling on Quarto-team fixtures never saw it. Real-world sites
(posit-docs, brand themes, any `theme: [my.scss]`) hit it on every page.

## Reproduce

```bash
# in q2
cargo build --release --bin q2 && cargo build --profile=release-perf --bin q2
# in docs-quarto-2 (source _environment first)
set -a; . ./_environment; set +a
rm -rf .quarto/cache/sass
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 /usr/bin/time -l q2 render        # perf.sass hits=22 compiles=682
QUARTO_JOBS=1 samply record -s -n --unstable-presymbolicate -o serial.json.gz -- \
  <q2>/target/release-perf/q2 render
<q2>/crates/perf-harness/scripts/analyze_profile.py serial.json.gz --top 45
<q2>/crates/perf-harness/scripts/bucket_profile.py  serial.json.gz grass_compiler
```
