# SCSS cache key ignores `@import`ed partials (bd-m3hga05o)

**Date:** 2026-09-14
**Braid:** bd-m3hga05o (P2, bug, label `perf`)
**Branch:** `braid/bd-m3hga05o-scss-cache-import-closure` in the room-2 main checkout (based on `main` @ `35bc11415`; topic branch, no worktree, per user request)
**Status:** Implemented on the branch (2026-09-15); Phase 2 measurement partly done. Rebased onto `main` after PR #679 merged (`f0bcb9538`).

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

## Decision (2026-09-14, with user): record the closure — option (R)

The closure of files a compile actually loaded is **recorded as a
by-product of the compile** and **validated on the next lookup**. No
separate "resolve dependencies" entry point exists in grass or
dart-sass, and none is needed: the dependency list is a cached artifact
of the previous compile, the make/ninja depfile pattern.

The lookup becomes two-level, and the two levels must stay separate:

1. **Key** — computed exactly as today from inputs known *before*
   compiling: `SCSS_RESOURCES_HASH`, theme identities, the top-level
   custom file's contents and (normalized, after PR #679) path,
   `doc_vars`, highlight style, minified and title-block flags. Closure
   contents are deliberately **not** in the key; putting them there
   would require knowing the closure before the compile that discovers
   it.
2. **Value** under that key — `(manifest, css)`, where the manifest is
   the list of `(path, sha256(contents))` pairs the compile that
   produced `css` reported loading, embedded `/__quarto_resources__/…`
   reads excluded (those are covered by `SCSS_RESOURCES_HASH` in the
   key).
3. **Lookup** — fetch the value, re-read every manifest path through
   the runtime, hash, compare. All match → hit. Any mismatch, missing
   file, or undecodable value → miss: compile (which yields the fresh
   closure) and overwrite the entry.

Why trusting the *previous* closure is sound: imports are declared
inside the files being hashed, so the closure can only change if some
file already in it changes, and that change fails validation. The gap
is negative dependencies (a new file that would now shadow an existing
import path, with no listed file changing); documented, not solved in
v1 — see Risks.

### What PR #679 (bd-79c4do6g + bd-ddahjqr1) changes for this work

Inspected 2026-09-14: `bugfix/bd-79c4do6g-scss-cache-key-path`, open
against `main`, four commits, full verify green per its description.

- **The path stays in the key** (lexically normalized via the new
  `quarto_util::normalize_lexically`, inside `ThemeContext::resolve_path`).
  Its `test_cache_key_distinct_files_with_same_content_stay_distinct`
  pins that and cites this strand as the reason partials are not
  hashed. With the manifest in place the path is redundant for
  correctness, but it is harmless, it partially covers the
  negative-dependency gap (two directories never alias), and removing
  it would widen this diff for nothing. **Decision: leave the key
  alone; the manifest is purely additive.** Update that test's comment
  to say so.
- **`cache_lru.rs` now serializes every index RMW under a process-wide
  `async_lock::Mutex` and reconciles the index against
  `SystemRuntime::cache_list` on every write.** Consequence for Q2: a
  sibling `<key>.deps` entry (option B) would be adopted by reconcile
  as an independent LRU entry and could be evicted separately from its
  CSS, leaving half a pair; and every extra write now costs a
  `read_dir`. That settles **Q2 = (A), bundle the manifest into the
  value.**
- **The `perf.sass hits/compiles/uncached` gauge exists** in
  `compile_theme_css.rs` (`sass_perf`, printed by
  `print_sass_stats_if_enabled` from `q2 render`). This strand's
  end-to-end checks use it: an edited partial must show `compiles=1`
  on the next render, an unedited one `compiles=0`. A fourth counter,
  `stale` (hit on key, manifest mismatch), is worth adding so the two
  kinds of miss are distinguishable.
- **`SystemRuntime` grew a defaulted `cache_list` method in the same
  PR**, so extending the trait again here (Q4) is in keeping with the
  branch's direction.
- **Base branch.** This work touches the same lines PR #679 touches
  (`cache_key` docs and tests, `variant_css`, `cache_lru.rs`,
  `traits.rs`). Rebase this branch onto
  `origin/bugfix/bd-79c4do6g-scss-cache-key-path` before Phase 0, and
  open the PR against `main` once #679 merges (or stacked, if it does
  not merge first). Do **not** re-implement any of #679 here.

### Remaining decisions (recommendations; confirm or override)

- **Q2 — manifest location: (A).** One value per key: a small header
  (format tag, JSON manifest, separator) followed by the CSS bytes.
  Apply the envelope uniformly to *every* entry in the `sass`
  namespace, including the default no-theme entries (their manifest is
  empty), so there is exactly one encode/decode pair. Old-format
  entries fail to decode and read as misses; additionally bump the
  namespace version stamp (append a format tag to what
  `ensure_sass_cache_ready` stores) so the generational purge clears
  them in one go instead of one by one.
- **Q4 — trait change: honest signature.** `compile_sass` returns a
  `SassOutput { css: String, loaded_files: Vec<PathBuf> }`. Three
  runtime sites (`traits.rs` default, `native.rs`, `wasm.rs`), the JS
  bridge (`sass.js` returns `{css, loadedUrls}` instead of a string;
  `sass.d.ts` follows), and the `quarto-sass` callers in `compile.rs`
  that must propagate the list (`compile_with_doc_vars_via_runtime`
  chain) versus discard it (`compile_default_css`, tests). An additive
  `compile_sass_tracked` with an "unknown closure" default would force
  every caller to reason about "unknown ≠ empty"; the honest change is
  smaller to get right.
- **Q5 — validate by content hash**, not mtime: the runtime has no
  mtime API on the WASM VFS, partials are small, and the top-level file
  is already hashed by content. An in-process memo `(path → hash)` for
  the duration of one render bounds the cost on large closures; add it
  only if Phase 3 shows it matters.
- **Native recording point: `RuntimeFs::read`'s runtime-fallback
  branch only** (`sass_native.rs`), via a `RefCell<Vec<PathBuf>>` on
  the adapter. Embedded hits are not recorded. Paths are whatever grass
  asked for — `theme_dir.join(import)` — which after #679 is the
  normalized project-relative form `file_read` accepts.
- **WASM recording point: `result.loadedUrls`** in `sass.js`, filtered
  to the `vfs:` scheme and mapped back to the `/project/…` path the
  VFS importer resolved; the embedded-resource prefix filtered out on
  the Rust side with the same predicate native uses.

## Work items

### Phase 0 — tests first (each must fail before its fix)

- [x] Rebase — onto `origin/main` instead (PR #679 had merged);
      `cargo xtask verify --skip-hub-build` green at the new base.
- [x] `sass_native.rs`: `compile_scss` reports the partial it loaded
      through the runtime and does **not** report embedded Bootstrap
      files (fixture: theme importing `_colors` from a temp dir, with
      `@import "bootstrap/…"` alongside).
- [x] `sass.test.ts` (`ts-packages/wasm-js-bridge`): the bridge result
      carries the VFS files dart-sass loaded, mapped to `/project/…`
      paths; embedded resource URLs excluded.
- [x] `compile_theme_css.rs` unit: envelope encode/decode round-trip;
      an undecodable (legacy) value reads as a miss.
- [x] `compile_theme_css.rs` unit, the bug: custom theme importing a
      partial through a mock runtime; second lookup with the partial's
      contents changed must miss; unchanged must hit; a *removed*
      partial must miss.
- [x] `tests/integration/sass_cache_key.rs` (extend #679's file):
      three-depth site whose theme imports a partial — render, edit the
      partial, render again; the emitted theme CSS carries the new
      colour and `perf.sass` shows `compiles=1 stale=1`; a third render
      with nothing edited shows `compiles=0`.
- [x] Update the comment on
      `test_cache_key_distinct_files_with_same_content_stay_distinct`.

### Phase 1 — core change

- [x] `SassOutput` type + `compile_sass` signature change across
      `traits.rs`, `native.rs`, `wasm.rs`; `RuntimeFs` records reads.
- [x] `sass.js` / `sass.d.ts`: return `{css, loadedUrls}`; `wasm.rs`
      unpacks and maps URLs to VFS paths.
- [x] `quarto-sass/compile.rs`: propagate `loaded_files` through the
      `compile_with_doc_vars` chain; other callers take `.css`.
- [x] `compile_theme_css.rs`: envelope encode on `cache_set_lru`,
      decode + manifest validation on `cache_get_lru`; `stale` counter;
      namespace version stamp gains a format tag; `cache_key` doc
      comment describes the two levels.

### Phase 2 — measure

- [x] Fixture: the three-render sequence above via `q2 render`, output
      inspected, recorded here.
- [ ] Connect docs (posit-docs theme imports one partial): cold and
      warm serial with `QUARTO_PERF_STATS=1`; warm wall must not
      regress measurably against #679's 6.22 s (one extra small read +
      hash per document per variant).
- [x] hub-client (WASM path): verified at the Node level, not in a
      browser. `hub-client/src/services/sassCachePartials.wasm.test.ts`
      drives the real WASM module + dart-sass bridge + the cache
      bridge's ephemeral in-memory mode, spies on `jsCompileSass`, and
      asserts: unchanged inputs → no new compile; edited `_colors.scss`
      → exactly one recompile and the new colour in the linked theme
      CSS. With the bridge's `loadedUrls` blanked (the old behaviour)
      the test fails at the edited-partial step. **A browser check
      through `q2 preview` was attempted and could not run:** the
      preview's project sync never puts `.scss` files into the VFS, so
      the SPA fails with Q-14-4 before any cache code runs (`q2 render`
      of the same project works). Filed as **bd-cmefgkq8**
      (discovered-from this one); cause not yet located.

### Phase 3 — notes

- [x] `cache_lru.rs` module docs: values in the `sass` namespace are
      enveloped; `cache_versioning` note on the format tag.
- [ ] Strand comment + close; note the negative-dependency gap as a
      follow-up strand if the user wants it tracked.

## End-to-end verification (fixture, after the fix, 2026-09-15)

From `claude-notes/plans/scss-cache-import-closure-investigation/fixture/`,
through the real binary, output inspected:

```bash
rm -rf .quarto/cache/sass _site
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .
sed -i '' 's/#123456/#abcdef/' _colors.scss            # edit the PARTIAL only
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .
QUARTO_JOBS=1 QUARTO_PERF_STATS=1 cargo run -q --bin q2 -- render .
```

| render                    | `perf.sass`                          | `index.html` links        | `.repro` in linked CSS | cache entries |
|---------------------------|--------------------------------------|---------------------------|------------------------|--------------:|
| cold                      | `hits=2 compiles=1 uncached=0 stale=0` | `…-1dea42e453982763.css` | `color:#123456`        |             1 |
| `_colors.scss` edited     | `hits=2 compiles=1 uncached=0 stale=1` | `…-6b75e5a4dec17318.css` | `color:#abcdef`        |             1 |
| warm, nothing edited      | `hits=3 compiles=0 uncached=0 stale=0` | same                     | same                   |             1 |

Before the fix the second row read `hits=3 compiles=0` and still linked
the first fingerprint (see "Repro at HEAD"). The one cache entry after
the edit is the same key overwritten; its first two lines are

```
q2-sass-cache-v1
[{"path":"_colors.scss","sha256":"b95decd7…"}]
```

Tests written first and seen failing at HEAD: the integration test
(`editing_an_imported_partial_recompiles_the_theme`, stale CSS
assertion), the JS bridge test (result was a string), and the
new-API unit tests (compile errors). All pass after the change.

## Risks / tradeoffs

- **Negative dependencies.** A newly created file that would shadow an
  existing import path is not detected until some listed file changes
  or the top-level key changes. grass probes with `is_file` before
  `read`; recording probed-but-missing paths would close this at the
  cost of a longer manifest. Documented for v1; the path-in-key from
  #679 already prevents the cross-directory variant.
- **WASM path mapping.** `loadedUrls` come back as `vfs:` URLs carrying
  the `/project/` prefix; the manifest must store the exact path form
  the runtime's `file_read` accepts, or every hub-client hit misses
  (a silent perf regression, not a correctness one — make the
  `sass.test.ts` case assert the mapped form).
- **Envelope format change** invalidates every existing entry once;
  the version-stamp bump makes that a single purge.
- **Cost on hit.** One `file_read` + hash per manifest entry per
  document per variant. One partial on posit-docs; a theme importing a
  vendored library could list dozens. Phase 2 measures; the per-render
  memo is the fallback.
- **Thundering herd on cold start** (noted in #679) is unchanged by
  this work and remains a separate follow-up.
- **Cross-platform.** Manifest paths are stored with forward slashes
  (`to_forward_slashes` convention) and compared as `Path`s. Windows
  CRLF does not affect content hashing since files are read as bytes.
