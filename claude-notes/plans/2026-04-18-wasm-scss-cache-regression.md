# Fix default-theme SCSS recompile regression in hub-client (WASM)

Beads: `bd-i992` (discovered-from `bd-imiw`)

## Symptom

Hub-client editor freezes for a few tenths of a second after every
keystroke when editing any document that has *no* `theme:` set — i.e. the
default, most common case. The freeze is not present in documents that set
an explicit `theme:` (because the themed code path is cache-backed).

## Root cause

`bd-imiw` changed `CompileThemeCssStage` so that when `theme:` is absent,
the stage now compiles the full Bootstrap + Quarto SCSS layer (matching
Quarto 1's implicit default) instead of returning a 244-line static
`DEFAULT_CSS` string. The new code path is at
`crates/quarto-core/src/stage/stages/compile_theme_css.rs:175-195`.

On native, this is fine: `quarto_sass::compile_default_css(...)` has an
in-memory `OnceLock` cache (`DEFAULT_CSS_CACHE` at
`crates/quarto-sass/src/compile.rs:55`). First call compiles, subsequent
calls clone a cached `String`.

On WASM (hub-client), there is no equivalent. The WASM version of
`compile_default_css` at
`crates/quarto-sass/src/compile.rs:357-384` was deliberately designed to
defer caching to the JavaScript layer (`SassCacheManager` / IndexedDB),
with a comment saying so. But the stage's no-theme path — the one we just
added — **never routes through the runtime cache at all**. The themed path
at `compile_theme_css.rs:206-279` uses
`ctx.runtime.cache_get("sass", &key)` → `cache_set`, which on WASM calls
through to IndexedDB via `crates/quarto-system-runtime/src/wasm.rs`. The
no-theme path skips that entirely.

Result: in WASM, every pipeline run performs a fresh Bootstrap SCSS
compilation via the dart-sass JS bridge (`runtime.compile_sass`). Hub-client's
preview debounces at 20 ms
(`hub-client/src/components/render/Preview.tsx:249-251`), so during active
typing this fires many times per second. Each call costs on the order of
100–500 ms, which is exactly the "freeze for a few tenths of a second"
the user reports.

### Why didn't we see it before bd-imiw?

Because before bd-imiw, the no-theme path bypassed the compiler entirely
and returned the static `DEFAULT_CSS` string. Zero cost per render.

### Why doesn't this show up in tests?

`cargo xtask verify` and `cargo nextest run --workspace` run on native
only, where `DEFAULT_CSS_CACHE` saves us. The regression only manifests
on WASM. The existing WASM integration tests don't measure SCSS
compilation latency.

## Design for the fix

Four coordinated changes. Items 3 and 4 are prerequisites that make item
2 safe for the long term, so the bundle lands together.

### 1. In-memory cache on WASM `compile_default_css` (keystroke freeze fix)

Mirror what the native version already does: wrap the compiled CSS in a
module-local `OnceLock` keyed by the minified flag. First render within
the WASM module's lifetime compiles; every subsequent render clones the
cached string in nanoseconds.

Why this is sufficient on its own for the keystroke case:
`compile_default_css` has no document-specific inputs. Every render of
an un-themed document in a given hub-client session wants the same CSS.
One cache slot is enough.

Why the existing comment deferring to JS caching is no longer the right
call:
- IndexedDB is async; reading it per keystroke still costs a frame.
- A `OnceLock<String>` on the WASM heap is cheap (~300 KB compiled
  Bootstrap CSS per session).
- It doesn't conflict with IndexedDB: the JS-layer cache still persists
  across sessions; the `OnceLock` is just a first-level hot cache.

The deliberate "no-cache on WASM" design made sense for the original
intent (compile only when the user opts in to a theme), but bd-imiw put
the default document on this path.

### 2. Route stage no-theme path through the runtime cache (cold-start fix)

Make `CompileThemeCssStage`'s no-theme path consult the same
`runtime.cache_get("sass", key)` that the themed path uses. The cache
key is a fixed string (e.g. `"default"`) combined with the minified flag
and `SCSS_RESOURCES_HASH`.

This saves the ~300 ms cold-start compile on the first render per
session — meaningful, because in-memory cache doesn't survive tab open /
page reload. The primary fix handles *within-session* keystroke cost;
this one handles *between-session* cold-start cost.

Cost: ~15 lines of stage code. Safe to land because items 3 and 4 bound
the IndexedDB growth this would otherwise create over time.

### 3. Generational purge on `SCSS_RESOURCES_HASH` mismatch

When we bump Bootstrap, add/remove a SCSS partial, or otherwise change
the compiled-in SCSS resources, every existing cache entry becomes
orphan data. The generational purge stops them accumulating:

- Store `sass:_resources_version = <SCSS_RESOURCES_HASH>` as a
  metadata entry in the `sass` namespace.
- On first `cache_get` per session (or at WASM module init), read the
  metadata. If it differs from the compiled-in constant, drop the
  entire `sass` namespace and re-write the metadata.
- Idempotent across multi-tab races: each tab arrives at the same new
  hash.

Lives in the cache layer itself (probably `wasm-js-bridge/cache.js` for
WASM and a native equivalent in `quarto-system-runtime`). Benefits the
themed path too — right now themed caches never self-evict across
Bootstrap bumps.

### 4. Per-namespace LRU size cap

Cap the `sass` namespace at a fixed budget (**10 MB**). On
`cache_set`, if the namespace exceeds the budget, evict entries by
least-recently-accessed until back under. Requires `last_accessed`
bookkeeping per entry; added on every `cache_get` / `cache_set`.

Self-healing: eviction of a hot key costs one recompile and a re-cache.
No correctness impact.

Bounding argument (addresses the growth concern):

- Within a single `SCSS_RESOURCES_HASH` generation:
  - Default CSS contributes 1 key (minified flag is always true in
    practice).
  - Built-in Bootswatch themes contribute ≤ ~24 keys.
  - Custom SCSS files contribute one key per (resolved path + content
    hash) — the only unbounded-in-principle source.
- Across generations: item 3 resets to 0 when the hash changes.
- Ceiling: LRU budget. Even a user hammering custom-theme edits cannot
  push the cache past 10 MB.

Lives in the cache layer.

#### Measured entry sizes (2026-04-18)

Seven real compiles via `cargo run --release -p quarto -- render ...`
on minified Bootstrap 5.3.1 + Quarto layer:

| theme              | bytes   | KB  |
|--------------------|---------|-----|
| no-theme (default) | 310 888 | 303 |
| cosmo              | 301 469 | 294 |
| flatly             | 312 238 | 304 |
| darkly             | 311 801 | 304 |
| sketchy            | 318 414 | 310 |
| quartz             | 327 234 | 319 |
| morph              | 332 916 | 325 |

Typical entry ~**300 KB**; range 290–330 KB. Theme CSS sizes don't vary
much because the Bootstrap core dominates; Bootswatch overlays add
single-digit percentage.

#### Cap-sizing math behind the 10 MB choice

- **Headroom**: 10 MB ÷ ~305 KB ≈ **33 entries** before LRU evicts.
- **All Bootswatches fit**: 25 built-in themes × 305 KB ≈ 7.6 MB. A
  user who cycles through every Bootswatch still has ~2.4 MB of slack
  for the default entry and custom-theme churn before any eviction
  fires.
- **Pathological custom-theme case**: a user saving one custom `.scss`
  repeatedly (new content hash each time) hits the cap after ~33 saves.
  Eviction is LRU, so their most recently used entries stay hot.

Reasonable alternatives if we revisit:
- 5 MB (~16 entries) — enough for typical single-project use; tighter
  bound. Breaks the "all Bootswatches fit" invariant.
- 20 MB (~65 entries) — generous, accommodates multi-project power
  users with many custom themes. Overhead is modest.

Landing at 10 MB: big enough that normal use never evicts, small
enough that the pathological case is bounded within a single
Quarto-version generation.

### Design rationale: generational-purge check is not memoized

[Recorded during implementation — future-me, don't re-add the memo
without reading this.]

The helper that runs the generational purge
(`ensure_sass_cache_ready` in the stage) is called on every
`CompileThemeCssStage::run`. The first instinct is to memoize it via a
process-scoped `static OnceLock<()>`: check once per session, short-
circuit afterwards, save one IndexedDB read per render on WASM. I
started there — and backed it out. Why:

**Process-scope vs runtime-scope.** A `static OnceLock` lives for the
whole process. In production that's fine because there's exactly one
runtime per process (hub-client has one `WasmRuntime`; CLI has one
`NativeRuntime`). But `cargo test` runs many tests in the same
process, and each stage test typically constructs its own
`NativeRuntime::with_cache_dir(temp)` with a fresh temp directory.

Failure mode:
1. Test A runs, creates runtime-A (temp dir A). Stage calls helper →
   writes `_version` into temp dir A. `OnceLock` set.
2. Test B runs, creates runtime-B (temp dir B, empty). Stage calls
   helper → `OnceLock` already set → short-circuits without writing
   `_version` into temp dir B.
3. Test B asserts temp dir B contains `_version`. Fails.

The memo's implicit assumption ("one runtime per process") holds in
production but not in tests.

**Alternatives considered.** (a) Key the memo by runtime identity —
`&dyn Trait` is a fat pointer, no stable identity, messy. (b)
`#[cfg(test)]` reset helper — brittle; easy to forget in new tests.
(c) Attach "checked" state to `StageContext` — but that's per-render,
so it wouldn't skip work on subsequent renders.

**Decision.** Drop memoization, call the helper every render. Cost is
a single IndexedDB read for `sass:_version` per render (~1–5 ms on
WASM, sub-millisecond on native). That's dominated by other per-render
work, and it's dwarfed by the 100–500 ms compile it prevents. The
simplicity is worth the negligible cost.

**When to revisit.** If hub-client profiling later shows this read
measurably hurts, options include: moving the "checked" flag into the
runtime itself (e.g. a `WasmRuntime` method that caches its own
`Promise<void>` first check), wrapping the trait in a session-scoped
cache layer, or skipping the check when the caller explicitly opts
into "I just checked a moment ago" semantics. None of these are worth
the complexity preemptively.

### Deferred (separate follow-up, not in this bundle)

- **Bump hub-client's 20 ms render debounce in `Preview.tsx:249-251`.**
  Not part of this fix; the keystroke cost is resolved by item 1
  regardless. Worth considering separately to reduce redundant full
  pipeline runs during fast typing.

## Testing

Latency is awkward to assert directly; test the caching mechanics that
imply the right latency instead. Each item gets its own test(s):

1. **WASM in-memory cache (item 1).** `cfg(target_arch = "wasm32")` test
   that calls `compile_default_css` twice with a mock runtime whose
   `compile_sass` is counted; assert it's invoked exactly once. Lives in
   `crates/quarto-sass/src/compile.rs`.

2. **Stage routes no-theme through runtime cache (item 2).** Mock
   runtime counts `cache_get` / `cache_set` calls; assert the no-theme
   stage path makes one `cache_get` and — on cache miss — one
   `cache_set`, mirroring the themed path. Stage-level test in
   `crates/quarto-core/src/stage/stages/compile_theme_css.rs`.

3. **Generational purge (item 3).** Two sub-tests in the runtime cache
   layer:
   - Happy path: set `sass:_resources_version` to match current hash;
     write a key; subsequent reads hit.
   - Mismatch path: set `sass:_resources_version` to a stale hash; write
     a key under the stale version; initialize the cache and assert the
     key is gone and the version metadata has been rewritten to the
     current hash.

4. **LRU size cap (item 4).** Unit tests in the cache layer:
   - Writing entries totaling > budget triggers eviction of the
     least-recently-accessed entry.
   - `cache_get` updates last-accessed so a recently-read entry
     survives eviction when a newer entry is written.

5. **Native regression guard (existing).** `test_compile_default_css_caching`
   at `crates/quarto-sass/src/compile.rs:539-548` stays; extend to
   verify the stage-level caching too.

## Work plan

Sequence: tests first for each item, then implementation. Items 1 and 3
are independent and can land in either order; item 2 should land after
item 3 so IndexedDB growth is bounded from the moment the stage starts
writing default-theme entries.

- [x] **Item 1: in-memory `OnceLock` on WASM `compile_default_css`.**
  `DEFAULT_CSS_CACHE` is now shared across native and WASM (the cfg gate
  on the `static` was removed). WASM `compile_default_css` now reads
  from the cache on entry and writes on success for the minified case,
  mirroring native. Comment updated to note the cross-session story is
  handled at a higher layer. Native `test_compile_default_css_caching`
  still passes; full workspace 7534 tests pass. Manual WASM validation
  will come at the end-of-session hub-client smoke test.
- [x] **Item 3: generational purge on `SCSS_RESOURCES_HASH` mismatch.**
  Added `cache_versioning::ensure_namespace_version(runtime, namespace,
  version)` in `crates/quarto-system-runtime/src/cache_versioning.rs`.
  Runtime-agnostic: uses the existing `cache_get` / `cache_clear_namespace`
  / `cache_set` trait methods, so native and WASM cache backends get the
  behavior automatically. Reserved key `_version` stores the sentinel.
  5 unit tests cover: fresh namespace stamping, matching-version no-op,
  mismatched-version purge + restamp, other-namespace isolation,
  repeated no-op calls. The memoizing caller-side wrapper (per-session
  check) lands with item 2 in the stage that owns the sass namespace.
- [x] **Item 4: per-namespace LRU size cap.** Added
  `crates/quarto-system-runtime/src/cache_lru.rs` with
  `cache_get_lru` / `cache_set_lru` wrappers. Portable across backends
  (stores an `_lru_index` JSON blob under a reserved key, touches
  `accessed_ms` on read, evicts LRU on write when total tracked size
  exceeds the budget). `SASS_CACHE_BUDGET_BYTES = 10 MB` exported as
  the standard budget. 8 unit tests cover: write-updates-index,
  read-touches-accessed, miss-doesn't-touch, over-budget eviction,
  recently-read entry survives eviction, self-eviction guard
  (just-written entry never chosen as victim in its own call), reserved
  keys rejected by wrapper, reserved keys not tracked in index when
  set via raw API.
- [x] **Item 2: route stage no-theme path through `runtime.cache_get`.**
  `CompileThemeCssStage::run` now calls `ensure_sass_cache_ready`
  (wraps `ensure_namespace_version`) before any cache read/write; the
  no-theme path uses `cache_get_lru` / `cache_set_lru` with a fixed
  `default_minified` / `default_expanded` key; the themed path
  replaced direct `cache_get`/`cache_set` with the LRU variants. Three
  new stage tests assert: writes-to-runtime-cache, uses-cached-value,
  and stale-generation-purges-old-default. Memoization of the
  generational check was considered and dropped — it leaked state
  between tests and between distinct runtimes; the 1-IDB-read cost per
  render is dominated by other per-render work.
- [x] **Verify full workspace and `cargo xtask verify`** — 7550 native
  tests green, hub-client WASM build green, hub-client tests green,
  trace-viewer build/tests green.
- [x] **Manual hub-client smoke test.** Confirmed by @cscheid: no
  keystroke freeze on un-themed docs; performance feels acceptable.
- [ ] **Close bd-i992 with links to the landing commit.**

## Open questions

1. **Where exactly does the generational-purge check fire?** Options:
   (a) lazily on first `cache_get` per namespace per session; (b) on
   WASM module init. (a) is simpler and defers cost to first use; (b)
   is more eager but might duplicate work across idle sessions. Lean
   (a).

2. **LRU budget value.** Proposal: 10 MB for `sass`. Rationale:
   comfortably holds ~25 Bootswatch themes at ~300 KB each (all
   minified). Revisit if users hit the cap.

3. **Eviction granularity.** Evict to N% under the budget on each
   overflow (e.g., to 80 %) vs. evict exactly one entry at a time. The
   "to 80 %" variant avoids thrashing when writes arrive in bursts.
   Probably worth it, but small: confirm during implementation.

4. **Does the bound also apply to non-`sass` namespaces?** Other
   namespaces may want different policies (or none). Keep LRU and
   purge configurable per namespace; default off so unknown namespaces
   opt in explicitly.

5. **Should we expose a "drop cache" button in hub-client UI?** Nice
   escape hatch for debugging; not a requirement for this issue. File
   separately if wanted.

6. **Is `300 KB` of cached CSS per WASM session too much memory for
   item 1?** Almost certainly not — hub-client already holds a larger
   SCSS-resources blob in the same module via `include_dir!`. Worth a
   sanity check if anyone is tracking WASM heap usage.

7. **Should we also cache intermediate assembled SCSS?** No. The output
   (compiled CSS string) is what the stage consumes; caching the output
   is simpler and covers the hot path.

8. **Could we revert to shipping static `DEFAULT_CSS` at no-theme and
   call it a day?** No. That was the old behavior and we deliberately
   removed it as part of `bd-imiw` so navbar/footer features have real
   Bootstrap classes to bind to. The fix here is caching, not reverting.

## Cross-references

- `bd-imiw` — introduced the compile-on-no-theme behavior. That feature
  is correct; only the caching was missed.
- `bd-ulgr` — Bootstrap JS shipping. Not causally related but surfaces
  in the same "Bootstrap-ecosystem resources in WASM" design space.
- `bd-djpt` — Bootstrap Icons shipping. Same design space.
- Plan of origin: `claude-notes/plans/2026-04-18-navbar-footer-design.md`.
