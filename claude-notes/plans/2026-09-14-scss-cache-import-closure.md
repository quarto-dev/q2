# SCSS cache key ignores `@import`ed partials (bd-m3hga05o)

**Date:** 2026-09-14
**Braid:** bd-m3hga05o (P2, bug, label `perf`)
**Branch:** `braid/bd-m3hga05o-scss-cache-import-closure` in the room-2 main checkout (based on `main` @ `35bc11415`; topic branch, no worktree, per user request)
**Status:** Investigation — pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The bug is confirmed at HEAD with a two-render repro
(below), every code path involved still has the shape the strand
describes, and the design space is small enough to decide in one
conversation. The interesting finding is that the strand's proposed fix
shape — "mirror grass import resolution in `cache_key`" — is **not the
best option**: both compilers we ship (grass on native, dart-sass in
WASM) already know the exact set of files they loaded, and we can have
them *report* it instead of predicting it. See design question 1.

Sequencing note: this strand was filed from bd-79c4do6g (the per-document
path-spelling key), which is `in_progress` with only a plan skeleton on
its branch (room-1, `braid/bd-79c4do6g-scss-cache-key-path`, one commit
`c7ef9774f`, no fix yet). Depending on Q1's answer, this strand either
subsumes that one or lands after it; the two must not be implemented
independently against the same lines of `cache_key`.

## Issue context

Filed 2026-09-13 by Carlos while investigating bd-79c4do6g. The sass
cache key (`compile_theme_css::cache_key`) hashes, for a
`ThemeSpec::Custom`, the resolved path string plus the **top-level
file's contents only**. Anything the theme reaches through `@import`
(or `@use`) is invisible to the key, so editing a partial serves the
previous compiled CSS from `.quarto/cache/sass` (native) or IndexedDB
(hub-client) until the user clears the cache by hand.

Real-world instance: the posit-docs extension's `theme.scss` imports
`_posit-colors.scss`; every Connect-docs colour tweak is a stale-CSS
trap. Any user following the documented pattern of splitting a custom
theme into partials is affected.

## Dependency graph

- **discovered-from → bd-79c4do6g** (in_progress, P1): the per-directory
  key-spelling bug. Same function, adjacent lines. Its plan
  (`claude-notes/plans/2026-09-13-scss-cache-key-path.md`, on that
  branch only) recommends option (a) — lexically normalize the path —
  and defers the content-addressed key (its option (c)) to *this*
  strand. Its keep-distinct test ("two files with identical text in
  different directories must stay distinct because their imports may
  differ") is exactly the property that hashing the import closure
  makes redundant: once imports are in the key, the path contributes
  nothing.
- **bd-79c4do6g → discovered-from → bd-fq44dlnm** (in_progress, P2): the
  Connect-docs render profile. Carries the unmerged `perf.sass` gauge
  (`hits`/`compiles` counters under `QUARTO_PERF_STATS=1`) that would be
  the natural verification instrument here too.
- **bd-ddahjqr1 (related to bd-79c4do6g, open, P2):** LRU index
  lost-update under parallel Pass 2. Not touched by this strand, but
  any design that stores a *second* cache entry per compile (see Q2
  option B) doubles its exposure to that race.
- No `blocks` edges, no epic parent.

## What the code looks like today

All paths in the strand still exist with the described shape.

**The key** — `crates/quarto-core/src/stage/stages/compile_theme_css.rs:218-228`
hashes `SCSS_RESOURCES_HASH`, each theme's identity (built-in name /
custom path string + top-level contents / brand YAML), the `doc_vars`
defaults, highlight style, minified flag and title-block flag. It runs
*before* compilation, so it can only see what it resolves itself.

**The lookup** — `compile_theme_css.rs:743-800`: `cache_key` →
`cache_get_lru(runtime, "sass", key)` → on miss
`compile_with_doc_vars_via_runtime` → `cache_set_lru`. Entries are the
raw CSS bytes; there is no metadata beside them. The namespace has a
generational purge (`ensure_namespace_version` keyed on `CSS_BUILD_ID`)
and a 10 MB LRU (`SASS_CACHE_BUDGET_BYTES`, `cache_lru.rs`); the LRU
reserves `_lru_index` and `_version` as key names.

**How imports actually resolve — native.** `sass_native::compile_scss`
(`crates/quarto-system-runtime/src/sass_native.rs`) hands grass a
`RuntimeFs` adapter implementing `grass::Fs { is_dir, is_file, read }`
over the `SystemRuntime` (embedded resources first, then the runtime).
grass's `find_import` (`grass_compiler-0.13.4/src/evaluate/visitor.rs:801`)
tries, relative to the importing file's directory and then each load
path: `_`-partial and plain, with `.import.{sass,scss,css}`, `.sass`,
`.scss`, `.css`, and `index.*` in a directory. The theme file's
directory is on the load paths (`themes.rs:748-754`), and built-in
resources live under a virtual `/__quarto_resources__/…` prefix that the
embedded provider serves. **Every file grass loads goes through
`RuntimeFs::read` — a `RefCell<Vec<PathBuf>>` there captures the exact
import closure with zero import-resolution logic of our own.**

**How imports actually resolve — WASM.** `ts-packages/wasm-js-bridge/src/sass.js`
calls dart-sass `compileString` with a custom VFS importer
(`createVfsImporter`: relative, absolute, then load paths; `_` partial
and extension probing). It returns `result.css` and drops
`result.loadedUrls`, which dart-sass populates with every canonical URL
it loaded (`vfs:/project/…` for user files). Same closure, already
computed, currently discarded.

**The trait.** `SystemRuntime::compile_sass(scss, load_paths, minified)
-> RuntimeResult<String>` (`traits.rs:606`); two implementations
(`native.rs:410`, `wasm.rs:552`) plus the `NotSupported` default. Adding
"which files did you load" to the result touches exactly those three
sites and the JS bridge's `d.ts`.

**Prior art in Q1.** `external-sources/quarto-cli/src/core/sass.ts:402-413`
knows about this hole and punts: if the SCSS input text matches
`/@import/`, the compile goes to a **session** cache (temp dir, wiped at
exit) instead of the persistent one. Correct, but it means Q1 never
persistently caches any theme that uses imports — every fresh `quarto
render` recompiles them. Worth knowing as the floor, not the target.

**Existing tests** (`compile_theme_css.rs:2253-2301`, `:2535`):
`test_cache_key_deterministic`, `_differs_for_minified`,
`_differs_for_different_themes`, `_custom_file_reads_content`,
`_custom_file_different_content`, `_builtin_no_file_reads`. None
involves an import. `_builtin_no_file_reads` pins that built-in-only
configs never touch the runtime filesystem from `cache_key` — any
design that pre-resolves imports in `cache_key` must keep that true.

### Repro at HEAD

Fixture: `claude-notes/plans/scss-cache-import-closure-investigation/fixture/`
(copied from the bd-79c4do6g branch). `theme.scss` does
`@import "_colors"`; `_colors.scss` sets `$repro-fg: #123456`.

Commands (from the fixture directory, `main` @ `35bc11415`, after a green
`cargo xtask verify --skip-hub-build`):

```bash
rm -rf .quarto/cache/sass _site
cargo run -q --bin q2 -- render .
grep -o '\.repro{[^}]*}' _site/site_libs/quarto/quarto-theme-*.css
sed -i '' 's/#123456/#abcdef/' _colors.scss          # edit the PARTIAL
cargo run -q --bin q2 -- render .
grep -o '\.repro{[^}]*}' _site/site_libs/quarto/quarto-theme-*.css
printf '\n/* touch */\n' >> theme.scss                # control: edit the TOP-LEVEL file
cargo run -q --bin q2 -- render .
grep -o '\.repro{[^}]*}' _site/site_libs/quarto/quarto-theme-*.css
```

Observed (output inspected, 2026-09-14):

| step                                  | emitted `.repro` rule       | theme CSS fingerprint | sass cache entries |
|---------------------------------------|-----------------------------|-----------------------|-------------------:|
| cold render                           | `color:#123456`             | `1dea42e453982763`    |                  3 |
| edit `_colors.scss` → `#abcdef`, render | **`color:#123456`** (stale) | `1dea42e453982763` (same) |              3 (same) |
| append a comment to `theme.scss`, render | `color:#abcdef`          | `6b75e5a4dec17318`    |                  6 |

The second row is the bug: the partial's edit changes nothing — same
key, same cached bytes, same fingerprint, no new entries. The third row
is the control: touching the top-level file (whose contents *are*
hashed) misses the cache, recompiles, and only then picks up the
partial's new colour. (The "3" and "6" are one entry per document
directory — bd-79c4do6g's bug, visible in the same run.) After the
control render `_site/site_libs/quarto/` holds both
`quarto-theme-1dea42e453982763.css` and `-6b75e5a4dec17318.css`, so the
stale file is never even cleaned up — a second, smaller symptom for the
theme-stage output cleanup, not this strand.

The "users must `rm -rf .quarto/cache/sass`" workaround from the strand
holds: with the cache dir removed the first render emits `#abcdef`.

## Design options (for Q1)

The closure can only be known two ways: **predict** it before compiling,
or **record** it while compiling.

- **(P) Predict in `cache_key`.** Re-implement grass's `find_import`
  walk (and dart-sass's, which differs in details — `.import.*`
  variants, `index` files, `@use` namespaces, `url()` and `http:`
  skipping, `meta.load-css`) over the runtime, recursively, before
  every lookup. This is what the strand text proposes. Cost: a second
  SCSS import resolver that must track two upstream compilers; every
  lookup re-reads the whole closure even on a hit (cheap for a few
  partials, but it's per document per variant); still cannot see
  files a *built-in* layer imports (fine — those are covered by
  `SCSS_RESOURCES_HASH`). Benefit: the key stays a pure function of
  inputs, so the cache stays a flat `key → css` map and nothing else
  changes.
- **(R) Record during compile, validate on lookup.** Extend
  `compile_sass` to return `{ css, loaded_files }` (native: recorded
  by `RuntimeFs::read`; WASM: `loadedUrls` mapped back to VFS paths;
  embedded `/__quarto_resources__/…` reads filtered out on both). Store
  beside the CSS a manifest of `(path, sha256(contents))` for the
  closure. On lookup: compute the top-level key as today (minus the
  path spelling, per bd-79c4do6g), fetch the manifest, re-hash the
  listed files through the runtime, hit only if all match. This is the
  make/ninja depfile pattern and what sass-loader / dart-sass `--watch`
  do. Cost: a cache-entry format change (see Q2), a trait-signature
  change, and the first render after an edit still pays one compile
  (unavoidable). Benefit: exactly correct for both compilers by
  construction, no resolver to maintain, and hits cost one small read
  per closure file.

Both fix the strand's bug. (R) is the one I'd recommend; (P) is what
the strand literally asked for, hence the question.

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion.

- **Phase 0 — Test plan (TDD).**
  - Unit test in `compile_theme_css.rs`: a custom theme whose file
    imports a partial; change the partial's contents through the test
    runtime; the second lookup must **miss**. Must fail at HEAD.
  - Under (R): a `sass_native` test that `compile_scss` reports the
    partial it loaded and *not* the embedded Bootstrap files; a
    `sass.test.ts` case asserting the bridge surfaces `loadedUrls`.
  - Under (P): resolver tests for `_` prefix, `.scss`/`.sass`/`.css`,
    nested import, load-path fallback, `url()`/`http:` skipped, cycle
    termination; keep `test_cache_key_builtin_no_file_reads` green.
  - End-to-end: the fixture's two-render sequence via `q2 render`,
    asserting the emitted CSS carries the edited colour.
- **Phase 1 — Core change** (per Q1/Q2).
- **Phase 2 — Fold in bd-79c4do6g** if the user chooses (R): drop the
  path string from the key (the closure now discriminates divergent
  imports), invert `test_cache_key_custom_file_reads_content`'s
  intent, and re-run its 3-document fixture expecting one entry.
  Otherwise leave that strand's normalization fix as is.
- **Phase 3 — Measure.** Connect docs, cleared cache, `perf.sass` once
  it lands (or entry counting): closure validation must not
  measurably cost more than the hit it protects.
- **Phase 4 — Notes.** Update the `cache_key` doc comment and, if the
  entry format changes, the LRU/versioning notes in `cache_lru.rs`.

## Open design questions for the user

1. **Predict or record?** (P) re-implements import resolution in
   `cache_key`; (R) has the compilers report the files they loaded and
   validates a manifest on lookup. My recommendation is **(R)** — it is
   correct for grass *and* dart-sass without us owning a resolver, and
   the instrumentation points (`RuntimeFs::read`, `result.loadedUrls`)
   already exist. Agree, or do you want the key to stay a pure
   pre-compile function?
2. **If (R), where does the manifest live?** (A) Bundle it into the
   cache value — a small header (JSON manifest + separator) before the
   CSS bytes, one entry per key, one LRU slot, one write; needs a
   format tag and a `CSS_BUILD_ID`-style purge of old entries. (B) A
   sibling entry `<key>.deps` — leaves CSS bytes untouched but doubles
   writes through the LRU index (bd-ddahjqr1's race) and can orphan
   half a pair on eviction. I lean **(A)**.
3. **Fold bd-79c4do6g into this branch?** With (R) the path string in
   the key is pure noise, so dropping it is a one-line side effect
   here, and its 3-doc fixture becomes a second end-to-end test. That
   makes bd-79c4do6g a duplicate to close when this lands. With (P)
   the two stay separate and its normalization fix should land first.
   Your call on sequencing, given that one is P1 and this is P2.
4. **Is the trait change acceptable?** `compile_sass` returning a
   struct instead of `String` touches `traits.rs`, `native.rs`,
   `wasm.rs`, `sass.js`/`sass.d.ts`, and every caller in `quarto-sass`
   (`compile.rs` has ~6). Alternative: a new `compile_sass_tracked`
   method with a default that calls the old one and reports an empty
   closure (which must then be treated as "unknown → never cache",
   not "no deps"). Prefer the honest signature change or the additive
   method?
5. **Should a hit re-hash the closure files, or compare mtimes?** The
   runtime has no mtime API that works on the WASM VFS, and partials
   are small; I'd hash contents (what the top-level file already
   does). Flagging in case you want an mtime fast path on native.

## Risks / tradeoffs (draft)

- **Negative dependencies.** Neither (P) nor (R) as sketched notices a
  *new* file that would now shadow a lookup (e.g. adding
  `theme-dir/_colors.scss` when the import previously resolved via a
  later load path). grass probes with `is_file` before `read`;
  recording the probed-but-missing paths would close this, at the cost
  of a longer manifest. Rare enough to document rather than solve in v1.
- **WASM path mapping.** `loadedUrls` come back as `vfs:` URLs with the
  `/project/` prefix; the manifest must store the same path form the
  runtime's `file_read` accepts, or every hub-client hit would miss.
- **Cache format change** (Q2-A) invalidates every existing entry once;
  the generational purge already exists for exactly this.
- **Cost on hit.** One `file_read` + hash per closure file per document
  per variant. For posit-docs that is one partial; for a theme that
  imports a vendored library it could be dozens. Phase 3 measures it;
  an in-process memo of `(path → hash)` per render would bound it.
- **Cross-platform.** Manifest paths must be stored with forward
  slashes (`to_forward_slashes` convention) and compared as `Path`s,
  never as strings.
