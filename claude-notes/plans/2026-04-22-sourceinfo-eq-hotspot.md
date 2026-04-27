# Diagnose and fix `SourceInfo::eq` hotspot in hub-client preview

Status: **approach locked in (Option 1) — implementation in progress on `perf/2026-04-22-json-sourcemap`**

Beads: bd-h5l7

## Context

Chrome performance profile of hub-client on a moderately-sized document shows
a large hotspot on

```
<quarto_source_map::source_info::SourceInfo as core::cmp::PartialEq>::eq
```

The flamegraph entry path is `parse_qmd_to_ast` (the `#[wasm_bindgen]` function
at `crates/wasm-quarto-hub-client/src/lib.rs:757`).

This is the first performance-profiling session on Quarto 2. The plan therefore
has two goals of equal weight:

1. **Fix the specific hotspot.**
2. **Establish a repeatable perf workflow** — a reliable native-side proxy for
   hub-client performance, so future sessions don't have to start from
   Chrome-flamegraph guesswork.

## Why Chrome-only measurement is not good enough

The hub-client runs WASM in the browser. Running the profiler gives a
flamegraph, but comparing before/after requires:

- a stable document loaded the same way,
- a warm engine with no stray network / IndexedDB / Automerge work,
- no compositor jitter from other tabs,
- sample counts large enough to see a 10-20% improvement through browser noise.

This is hard to control. We need a native-side proxy that exercises the same
code path deterministically, so we can iterate on fixes with `cargo bench` or
`samply` and only cross-validate in the browser at the end.

## Working hypothesis (to be confirmed with data)

The `SourceInfo::eq` time is almost certainly *caller-dominated*, not *callee-dominated*.
That is: each individual call is cheap, but the call site makes too many of them.

Evidence from reading `crates/pampa/src/writers/json.rs:200-305`:

- `SourceInfoSerializer::intern` is called once per AST node during JSON serialization.
- It first does a pointer lookup into `id_map: HashMap<*const SourceInfo, usize>`
  (fast, O(1)).
- On a miss, it **linearly scans** `content_map: Vec<(SourceInfo, usize)>` and
  calls `existing == source_info` for each entry
  (`crates/pampa/src/writers/json.rs:229-235`).
- `SourceInfo` is stored **by value** inside each `Inline`/`Block` variant in
  `quarto-pandoc-types` (not behind an `Arc`), so every node presents a distinct
  `*const SourceInfo`. The pointer lookup will therefore miss on nearly every call.
- That means intern falls through to the linear scan on every node, giving
  roughly O(n²) behavior in the number of AST nodes. For a moderately-sized
  document (say, a few thousand inlines), this dominates.

`SourceInfo`'s own derived `PartialEq` has a secondary cost: the `Substring`
variant holds `Arc<SourceInfo>`, and `Arc`'s derived `PartialEq` delegates to
the pointee, so each comparison walks the parent chain. YAML frontmatter
builds up nested `Substring` chains. But this only matters per-call — the
dominant factor is still call count.

Two hypotheses to rank once we have data:

- **H1 (strong):** `content_map` grows to O(n) and the scan is O(n²) per document.
  Fix: replace the `Vec` with a `HashMap` keyed on a cheap structural hash, or
  remove the fallback entirely and rely on `Arc`-sharing of `SourceInfo` during
  tree construction.
- **H2 (weaker):** each `SourceInfo::eq` call is expensive because parent
  chains are deep.
  Fix: add an `Arc::ptr_eq` fast path inside a hand-written `PartialEq`, or
  cache a structural hash on `Arc<SourceInfo>`.

Both could be partially true. Measurement decides the split.

## Phase 1 — Reproducible native harness (before any fix)

**Goal:** a single command that reproduces the hotspot on the native binary,
so we can iterate without touching the browser.

- [ ] Pick/craft a representative fixture qmd (~same size as the profiled
      document). Candidates: an existing large fixture in `crates/pampa/tests/`,
      the repo's own docs/*.qmd, or a synthetic document of N paragraphs each
      with M inlines. Record the chosen size and a rough token/node count.
- [ ] Add a Criterion bench `crates/pampa/benches/sourceinfo_intern.rs` that:
      - parses the fixture to AST once (outside the timed section),
      - times `writers::json::write(&doc, &context, &mut buf)` with
        `JsonConfig { include_inline_locations: true }` (matches hub-client),
      - reports per-iteration wall time.
- [ ] Add a parameterized variant that scales the fixture 1×, 2×, 4×, 8× to
      confirm the O(n²) shape empirically (if growth is super-linear in
      document size, H1 is confirmed).
- [ ] Capture a native flamegraph with `samply` against the `pampa` binary
      rendering that fixture to JSON. Confirm `SourceInfo::eq` (or
      `SourceInfoSerializer::intern`) shows up with roughly the expected share
      of time. Record baseline %.

Exit criterion: we have one command (`cargo bench --bench sourceinfo_intern`)
and one flamegraph that reproduces the hotspot natively. If the hotspot does
*not* reproduce, we've learned something important and the plan needs revising.

## Phase 2 — Diagnose with instrumentation

**Goal:** know exactly what's happening inside `intern` on a real document,
before writing a fix.

- [ ] Add behind-`#[cfg(feature = "intern-stats")]` counters inside
      `SourceInfoSerializer`:
      - total `intern` calls
      - `id_map` hits
      - `content_map` hits (and avg/max position where the hit landed)
      - misses (terminal path — new pool entry)
      - final `content_map.len()`
- [ ] Run the harness with stats enabled on the fixture. Expected shape if H1
      is right: hit rate is moderate (maybe 20-50%), but the linear scan is
      doing ~N/2 comparisons per call, times ~N calls.
- [ ] Inspect the distribution of `SourceInfo` variants reaching the scan. If
      most are `Original` (shallow compare), the per-call cost is small and
      H1 dominates. If many are deeply nested `Substring`, H2 contributes.
- [ ] Record the numbers in this plan file under "Findings" before proposing a fix.

## Selected approach: Option 1 — drop content-equality dedup entirely

After the historical investigation and the consumer audit (see Findings),
the chosen direction is the simplest of the candidates considered:

1. **Delete `content_map` and its linear scan.** Empirically a 0-hit O(n²)
   path on prose; on YAML-heavy documents, its only productive role is
   collapsing `ConfigValue::{Path, Glob, Expr}` clones with their
   originals into one pool ID. Letting them get separate pool IDs costs
   one extra pool entry per tagged YAML scalar — negligible JSON-size
   impact. The verified-no-consumer audit confirms nothing relies on the
   "ID equality ⇒ structural equality" invariant that `content_map`
   currently provides.

2. **Replace the `*const SourceInfo` cache key on top-level intern calls
   with explicit `Arc::as_ptr` dedup at the one place pointer identity
   actually is stable: the `Substring` parent recursion.** `Arc<SourceInfo>`
   inside `Substring` is owned by AST nodes for the full serialization
   lifetime; its inner address is genuinely stable. By-value `SourceInfo`
   addresses are not, and were the root of the 2026-01-13 memory-reuse
   bug.

3. **Remove the entire `precompute_all_json` machinery, including
   `inlines_keeper`, `precomputed_json`, and the keep-alive-then-drop
   dance.** It exists only to defend against stale `*const SourceInfo`
   hits in `id_map`, which step 2 makes mechanically impossible.
   `write_config_value` for `Path`/`Glob`/`Expr` can synthesize its
   `Inlines` on the fly and call `write_inlines` directly — the same
   shape as the `PandocInlines` arm.

Net: three layered defenses (`content_map`, `id_map` on by-value addresses,
`precompute_all_json` + keepers) collapse to one small `arc_parent_ids:
HashMap<*const SourceInfo, usize>` cache scoped to the `Substring` parent
edge. Intern becomes O(1) amortized with no quadratic scan.

### Why not Option 2 (hash-keyed `content_map`)

Option 2 — `HashMap<StructuralKey, usize>` keyed on a cheap hash of
`SourceInfo` content — would also fix the perf and additionally preserve
the canonical-ID invariant. It was demoted because:

- The audit (see Findings) confirmed no consumer needs that invariant.
- Encoding "ID equality ⇒ structural equality" into the format is a
  fragile contract. Consumers that come to depend on it implicitly would
  silently break the day someone uses a different writer or alters dedup
  semantics. Better not to expose that contract.
- Option 1 deletes more code than Option 2 adds, and removes the
  precompute machinery entirely. Smaller surface area, fewer
  invariants to maintain.

If a future requirement does need canonical pool entries, layering Option 2
on top of Option 1's cleaner skeleton is straightforward — `impl Hash for
SourceInfo` is one derive on `quarto-source-map`. Worth keeping in mind,
not worth doing prophylactically.

## Implementation work breakdown

Done methodically; can split across sessions. Each phase ends with a
verifiable state and a checkpoint commit.

### Phase A — Switch the recursion cache to `Arc::as_ptr` ✅ done

Goal: make the `Substring` parent dedup explicit and use `Arc::as_ptr`,
so the existing pointer-cache contract narrows to the only key that's
actually stable. After this phase the writer still uses `arc_parent_ids`
and `content_map` — we only change *what the cache holds*. Tests should
still pass; no snapshot churn expected.

- [x] Read `crates/pampa/src/writers/json.rs` `intern` end-to-end with
      this restructuring in mind.
- [x] Rename the field from `id_map: HashMap<*const SourceInfo, usize>`
      to `arc_parent_ids: HashMap<*const SourceInfo, usize>`. New
      semantics: only Substring parent Arcs go in, keyed by
      `Arc::as_ptr`.
- [x] Remove the top-of-`intern` by-value pointer lookup (empirically a
      0-hit branch anyway).
- [x] In the `SourceInfo::Substring { parent, .. }` arm, before recursing,
      look up `Arc::as_ptr(parent)` in `arc_parent_ids`; on hit, reuse
      the cached id; on miss, recurse into `intern(parent)` and insert.
- [x] Remove the by-value pointer insertion at the bottom of `intern`.
- [x] Run `cargo nextest run -p pampa` — **3722/3722 pass**, 0 snapshot
      diffs. Behavior preserved.
- [x] Run instrumented binary on `1x.qmd` and `8x.qmd`:
       - 1×: intern_calls=8,850, eq_comparisons=39,156,825,
         content_map_hits=0, arc_parent_hits=0, pool_size=8,850
         (identical to baseline; wall time not meaningful — nextest was
         consuming CPU in parallel).
       - 8×: intern_calls=70,786, eq_comparisons=2,505,293,505,
         content_map_hits=0, arc_parent_hits=0, pool_size=70,786
         (identical to baseline).
       - `arc_parent_hits=0` is expected — this document has no YAML
         frontmatter so no `Substring` variants exist. Will be non-zero
         on YAML-heavy docs once we test those.

### Phase B — Delete `content_map` and its scan ✅ done

Goal: remove the O(n²) hot loop.

- [x] Remove the `content_map` field from `SourceInfoSerializer`.
- [x] Remove the `for (existing, id) in &self.content_map { ... }` loop
      from `intern`.
- [x] Remove the `self.content_map.push((source_info.clone(), id))` at
      the bottom of `intern`.
- [x] Update the `QUARTO_PERF_STATS` Drop impl. New output is
      `perf.intern intern_calls=N arc_parent_hits=N pool_size=N`.
      (Env var was renamed from `QUARTO_INTERN_STATS` to
      `QUARTO_PERF_STATS` during Phase F cleanup so one toggle can
      drive every perf gauge.)
- [x] Update unit test `test_source_info_pool_original` — it explicitly
      encoded the old by-value canonical-ID invariant (`intern(&s)`
      twice ⇒ same id). Rewrote to assert the new contract: two calls
      with the same SourceInfo value produce two pool entries that
      resolve to structurally-equal content. The inline docstring
      points future readers at this plan.
- [x] Run `cargo nextest run -p pampa`. 13 JSON snapshots flagged; all
      verified pool-resolution-equivalent (identical canonicalized AST
      when pool IDs are dereferenced) via
      `/tmp/check_snap_diffs2.py`. None were structural changes.
      Accepted via `cargo insta accept`.
- [x] Full pampa suite: **3722/3722 pass** after accept.
- [x] Run instrumented binary on fixtures — perf result below.

**Perf result (before → after, user CPU):**

| Size |  Before |  After | Speedup |
|------|--------:|-------:|--------:|
| 1×   |  0.20 s | 0.12 s |   1.7×  |
| 2×   |  0.58 s | 0.23 s |   2.5×  |
| 4×   |  1.93 s | 0.53 s |   3.6×  |
| 8×   |  6.80 s | 1.36 s |   5.0×  |
| 16×  | 25.61 s | 3.90 s |   6.6×  |

Growth per doubling collapsed from ~4× to ~2× — the O(n²) tail is gone.
`eq_comparisons` = 0 trivially (the loop is removed).

**Snapshots affected (13):** `002.snap`, `003.snap`,
`anchor-shorthand-01-simple`, `anchor-shorthand-02-in-paragraph`,
`anchor-shorthand-03-hyphenated`, `anchor-shorthand-04-underscored`,
`anchor-shorthand-05-numeric`, `horizontal-rules-vs-metadata`,
`html-comment-30-three-dashes`, `math-with-attr`, `table-alignment`,
`table-caption-attr`, `yaml-tags`. All showed pool growth + index
shifts; none showed structural AST change. Pool growth per document
corresponds to the number of sites that previously deduped via
`content_map` (ConfigValue Path/Glob/Expr clones, `SourceInfo::default()`
wrappers on synthesized Spans, and a few other coincidences).

### Phase C — Remove `precompute_all_json` ✅ done

Goal: collapse the precomputation pass and `inlines_keeper` machinery
that exists solely to defend against stale `*const SourceInfo` hits.
With Phase A's narrower cache, that bug is mechanically impossible.

- [x] Deleted `precomputed_json: HashMap<*const ConfigValue, Value>` from
      `JsonWriterContext`.
- [x] Deleted `precompute_all_json`, `precompute_config_value_json`,
      `precompute_block_json`, and the `inlines_keeper` threading.
- [x] Kept `build_path_inlines`, `build_glob_inlines`,
      `build_expr_inlines` — they're now called directly from
      `write_config_value` and read cleaner than inlining.
- [x] In `write_config_value`, replaced the `Path | Glob | Expr` arm:
      each variant now synthesizes its `Inlines` on the fly and
      delegates to `write_inlines`, exactly like the `PandocInlines`
      arm.
- [x] Removed the call to `precompute_all_json(pandoc, &mut ctx)` from
      `write_pandoc`.
- [x] One snapshot changed: `yaml-tags.snap` — the intern order for
      tagged YAML values shifted because the walk order is now
      outer-first (from `write_config_value`) rather than inner-first
      (from the precomputation pass). Pool-resolution-equivalent per
      `/tmp/check_snap_diffs2.py`. Accepted.
- [x] Full pampa suite: **3722/3722 pass**.
- [x] Perf stable vs Phase B (1× 0.12s, 16× 4.25s — within noise;
      deletion of the precompute pass doesn't affect the steady-state
      hotspot, which was already eliminated in Phase B).

### Phase D — Workspace verification ✅ done

- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — **7624/7624 pass**, 195 skipped.
- [x] `cargo xtask verify` — all verification steps passed. Covers
      Rust build + tests, hub-client WASM build, hub-client vitest,
      trace-viewer tests.

Overall diff at Phase D:
```
crates/pampa/src/writers/json.rs   | 368 ++++----- (+85 -283)
13 snapshot files                  | pool-id churn only (pool-resolution-equivalent)
```

`json.rs` down by 198 lines net. Three cache/defense layers collapsed
to one `arc_parent_ids` field scoped to the only place pointer identity
is stable.

### Phase E — Cross-validate in hub-client ✅ done (2026-04-22)

- [x] User verified on the canonical 50-paragraph lorem-ipsum document
      (`/Users/cscheid/Desktop/daily-log/2026/04/22/test.qmd`) that
      `SourceInfo::eq` is no longer a hotspot in Chrome profiling.
- [x] A new, different hotspot was identified during that verification.
      Out of scope for this session — will be addressed in a follow-up
      that exercises the same workflow this plan establishes.

### Phase F — Codify the workflow ✅ done

- [x] Wrote `claude-notes/instructions/performance-profiling.md` — a
      playbook version of the workflow we just used. Structured as
      8 steps with concrete commands and templates, plus a "what not
      to do" section and a case-study pointer back to this plan.
- [x] Documented `QUARTO_PERF_STATS=1` (renamed from
      `QUARTO_INTERN_STATS` during Phase F for generality) and where
      the counters live. Output prefix convention: `perf.<gauge-name>`
      so multiple gauges can coexist under one env var.
- [x] Added a "Performance profiling" section to `CLAUDE.md` between
      "Debugging Approach" and "Claude Code hooks", with a CRITICAL
      pointer to the instructions doc and a note about the env var.
- [x] Decided: **keep the counters in place**. They cost nothing at
      runtime (a few `usize` increments per intern call) and the next
      perf session on a different hotspot will almost certainly benefit
      from the pattern already being present. They're also mentioned in
      `CLAUDE.md` so they won't be mysterious to a future reader.

## Verification gate before pushing

CLAUDE.md "GIT PUSH POLICY" applies. Before requesting push approval:
- [ ] `cargo build --workspace` clean
- [ ] `cargo nextest run --workspace` clean
- [ ] `cargo xtask verify` clean (covers hub-client too)
- [ ] Snapshot diffs explicitly enumerated in commit messages
- [ ] Browser cross-validation completed (Phase E)

## Open questions — resolved

1. **Profiled document?** Resolved 2026-04-22 — user provided
   `/Users/cscheid/Desktop/daily-log/2026/04/22/test.qmd` (50 lorem
   paragraphs, 30 KB, 8850 intern calls). Scaled variants
   `/tmp/q2-intern-bench/{1,2,4,8,16}x.qmd` confirmed O(n²).
2. **Need `include_inline_locations: true`?** Out of scope for this
   session — the intern path runs regardless of that flag, so it's
   adjacent. Worth a follow-up audit of what the hub-client actually
   reads from the `l` field, but doesn't block this fix.
3. **Arc-sharing `SourceInfo` in AST (old F2)?** Deferred. Not needed
   for this hotspot. Would simplify other things but is a separate
   refactor that touches `quarto-pandoc-types` and every constructor.

## Findings

### 2026-04-22 — O(n²) confirmed empirically, and both caches are dead weight

Instrumented `SourceInfoSerializer::intern` with four counters
(`stat_intern_calls`, `stat_id_map_hits`, `stat_content_map_hits`,
`stat_eq_comparisons`) plus a `Drop` that prints to stderr when
`QUARTO_PERF_STATS=1` (originally `QUARTO_INTERN_STATS=1`, renamed
during Phase F). Ran against the canonical fixture (`test.qmd` from
2026-04-22 daily log — 50 lorem-ipsum paragraphs, no YAML, no structure)
and scaled variants at 2×, 4×, 8×, 16×.

Command:
`QUARTO_PERF_STATS=1 target/release/pampa <fixture>.qmd -t json --json-source-location full > /dev/null`

| Size | intern_calls (n) | eq_comparisons | n(n−1)/2 | id_map_hits | content_map_hits | wall time |
|------|-----------------:|---------------:|---------:|------------:|-----------------:|----------:|
| 1×   |            8,850 |     39,156,825 |     39,156,825 |           0 |                0 |    0.22 s |
| 2×   |           17,698 |    156,600,753 |    156,600,753 |           0 |                0 |    0.63 s |
| 4×   |           35,394 |    626,349,921 |    626,349,921 |           0 |                0 |    2.03 s |
| 8×   |           70,786 |  2,505,293,505 |  2,505,293,505 |           0 |                0 |    7.01 s |
| 16×  |          141,570 | 10,020,961,665 | 10,020,961,665 |           0 |                0 |   26.29 s |

Two conclusions, both stronger than the draft hypothesis:

1. **`eq_comparisons` matches `n(n−1)/2` exactly** at every size. The linear
   scan over `content_map` never short-circuits; every intern call walks the
   entire map. Pure O(n²) in AST-node count.
2. **Both cache tiers have a 0% hit rate.** `id_map` (pointer-keyed) never
   hits because `SourceInfo` is held by value in each `Inline`/`Block` and
   every call site presents a fresh borrow. `content_map` (content-keyed
   fallback) never hits because every `SourceInfo` in this document is
   genuinely distinct (different offsets per node). **The entire interning
   machinery pays full O(n²) cost on this document and dedups nothing.**

This is a stronger statement than "the fallback is slow." For documents
shaped like `test.qmd` (prose, no frontmatter, no shared parent chains),
the content-equality fallback cannot possibly earn back its cost — there
is no structural duplication to find. The fallback exists for a narrow
case (cloned `Path` values inside `ConfigValue` during metadata
serialization) and charges every other document for it.

Wall-time scaling is approaching 4× per doubling (the expected shape for
a quadratic hot loop sitting on top of a linear pipeline). At 16× the
document, 26 seconds are spent interning.

### Implications for Phase 3 fix candidates

The data sharpens the earlier ranking:

- **F1 (hash-keyed dedup)** now clearly wins on ergonomics: replacing
  `Vec<(SourceInfo, usize)>` with `HashMap<StructuralKey, usize>` makes
  the fallback amortized O(1). On `test.qmd`-shaped documents it still
  dedups nothing, but the wasted work becomes O(n) hashes instead of O(n²)
  comparisons.
- An even smaller variant of F1 worth considering: since the fallback
  never hits on this document, **we could also just delete the fallback**
  and handle the narrow `Path`-clone case at its source (either by
  constructing those `SourceInfo`s through an `Arc` that the `id_map`
  can catch, or by exposing a dedicated `intern_cloned` entry point that
  the metadata writer calls with an explicit key). Worth investigating
  how many call sites actually rely on the content-equality fallback
  before committing to hashing everything.
- **F2 (Arc-sharing `SourceInfo` in AST)** remains a bigger refactor
  than F1 and is no longer needed to fix the reported hotspot. Keep it
  as future work, not this session.
- **F3 (`Arc::ptr_eq` fast path in `SourceInfo::eq`)** is not relevant
  to the dominant cost on this document — the `SourceInfo`s being
  compared here are `Original` variants, which already compare in O(1).
  F3 only helps documents with deep `Substring` chains from YAML; worth
  keeping in mind but not the lead fix.
- **F4 (`include_inline_locations=false`)** would not help: the intern
  counters run regardless of that flag. `resolve_location` is a separate
  per-node cost that F4 *would* reduce, but it's not the hotspot on the
  flamegraph.

### Instrumentation in the tree

The counters and `Drop` print are currently on `main` (uncommitted) at
`crates/pampa/src/writers/json.rs`. Keep them through the fix so we can
re-run the same experiment after the fix and show the scan work
collapsing. Remove (or gate behind a `debug-intern-stats` feature) once
the fix is merged.

### 2026-04-22 — history of the current design (mined from claude-notes)

Following the empirical finding that the content-equality fallback never
hits on prose, I went back through the plans to understand why the
design looks the way it does. Three documents tell the full story.

**Origin (2025-10-19, k-44)** —
`claude-notes/sourceinfo-serialization-optimization-design.md` and
`claude-notes/plans/2025-10-19-sourceinfo-pool-serialization.md`. The
pool was introduced to fix a **25–55× JSON blowup** from each `Substring`
mapping embedding its full parent chain inline. For a YAML frontmatter
with ~100 sibling nodes sharing one parent chain, the old format wrote
the chain 100 times. Expected savings: ~93% for metadata-heavy docs.

The **original design was pointer-only** — just
`id_map: HashMap<*const SourceInfo, usize>`. "Risk 1: Pointer
Instability" is acknowledged in the design doc (lines 498–505) and
dismissed: *"SourceInfo is never moved during serialization (only
borrowed) ... if this becomes an issue, use ID generation instead of
pointer addresses."* No `content_map`. No precomputation.

**The caveat analysis (2025-10-19)** —
`claude-notes/rc-serialization-caveat-analysis.md` concluded serde
doesn't deduplicate Rc/Arc content automatically (it's depth-first and
stateless). This motivated the pool design above.

**content_map added (2025-12-29, commit e4056b9f "meta value is now
config value")** — This commit landed as part of the Phase 5
`MetaValueWithSourceInfo → ConfigValue` migration. The commit's own
plan (`2025-12-29-snapshot-change-review.md`) does not mention
`content_map` at all — the fallback was added as a silent support
change, probably because the refactor started synthesizing `Inlines` at
serialization time for `ConfigValue::{Path, Glob, Expr}` variants, and
those synthesized `Inlines` contain *cloned* `SourceInfo` values whose
addresses don't match the ConfigValue's original. The pointer cache
missed, IDs diverged across equivalent content, and the
content-equality fallback was introduced to catch it.

**The real bug (2026-01-13)** —
`claude-notes/plans/2026-01-13-precomputation-memory-reuse-bug.md`.
This is the document the user half-remembered. The `content_map` alone
didn't fix the problem; a snapshot test kept failing *intermittently*.
Root cause was subtler than content mismatch:

> "If the allocator reuses Clone A's address for Clone B...
> `id_map.get(&A)` returns `Some(2)` ← **WRONG ID!**"

When `write_config_value` for the `!expr` entry synthesized an `Inlines`,
interned its cloned `SourceInfo` (address A, id=2), then dropped the
`Inlines`, the heap slot at A was freed. The *next* ConfigValue's
`build_path_inlines` could be given address A back by the allocator —
whereupon `id_map[A]` returned id=2 from the previous, unrelated
interning. Non-deterministic because allocator reuse depends on ASLR,
stack layout, and prior alloc history. That plan **explicitly
considered and rejected** content-based hashing ("Option 1"):

> "Requires changes to `quarto-source-map` crate; hash computation for
> deep Substring chains could be expensive."

Instead it chose "Option 3": a `precompute_all_json` pass with an
`inlines_keeper: Vec<Inlines>` that holds every synthesized temporary
alive for the duration of precomputation so addresses can't be reused.
See `crates/pampa/src/writers/json.rs:484-502` for the `inlines_keeper`
comment.

### What the current architecture actually defends against

Three layered defenses have accreted around pointer instability:

1. `id_map` — same-pointer hits (the only case that was ever intended).
2. `content_map` — fallback for clones with *different* addresses but
   identical content. Empirically a 0-hit, full-O(n²) path on prose.
3. `precompute_all_json` + `inlines_keeper` — prevents the allocator
   from reusing a freed temporary's address for a new temporary,
   avoiding stale `id_map` hits. This is the fix for the 2026-01-13
   bug.

All three exist because the cache key is `*const SourceInfo` on a value
that is held by-value in the AST and cloned freely during `ConfigValue`
serialization. The key was never stable for cloned values, and the
defenses fight that fact.

### Redesign proposal (revised F1)

Per the 2026-01-13 plan the "right" fix was rejected on two grounds:
(a) it needs a change in `quarto-source-map`; (b) deep hash cost.
Neither is a real blocker:

- (a) Deriving `Hash` on `SourceInfo` is one line in
  `quarto-source-map/src/source_info.rs` and is purely additive.
- (b) Deep `Substring` parent chains are already walked by the existing
  derived `PartialEq` on every comparison — a recursive `Hash` is
  the same cost *at most once per unique parent chain* (because every
  parent `Arc<SourceInfo>` visited by `intern` is visited exactly once
  if we dedup on Arc identity).

The simpler, tighter redesign our data now justifies:

1. **Delete `content_map` outright.** Its only productive role was
   catching clones created by `ConfigValue::{Path, Glob, Expr}`
   synthesis. The cost of not catching those clones is a handful of
   extra pool entries per document (one per clone; pool grows by at
   most a few tens of entries even on heavy metadata), which is
   negligible vs. the 93% YAML savings the pool already delivers.
2. **Replace the `*const SourceInfo` key in `id_map` with
   `Arc::as_ptr(parent)` for the `Substring` recursion only.** This is
   the only place where pointer identity is actually stable, because
   `Arc<SourceInfo>` is a value held inside the AST for the full
   lifetime of serialization — no clone-drop-alloc cycle applies.
3. **Stop caching `*const SourceInfo` for by-value `SourceInfo` on AST
   nodes.** Our measurement shows those hits are 0% on prose anyway;
   on YAML they'd be rare too, since distinct AST nodes rarely share a
   by-value `SourceInfo`.
4. **Remove `precompute_all_json` + `inlines_keeper` +
   `precomputed_json`.** They exist only to defend against stale
   `*const SourceInfo` hits. With step 3 eliminating that cache, the
   2026-01-13 bug can't recur regardless of allocator behavior.
   `write_config_value` can synthesize its `Inlines` on the fly again.

Net effect on the hotspot: intern becomes O(1) amortized, with no
linear scan and no quadratic work. Net effect on the code: three
layered defenses collapse to one small cache on the one thing that
actually has a stable identity (Arcs inside the AST). Net effect on
JSON output: the pool may gain a handful of duplicate entries for
`ConfigValue::{Path, Glob, Expr}` clones; structurally identical but
addressed differently. Pool size grows by O(number-of-tagged-YAML-
values), not per-AST-node.

### Correctness check before implementing

Before coding, verify:

- [ ] Any downstream reader (the annotated-parse converter at
      `claude-notes/plans/2025-10-23-json-to-annotated-parse-conversion.md`)
      relies on pool IDs only as stable references within a single
      document, not on "same content ⇒ same ID." Grep usages; I believe
      this is true but haven't confirmed.
- [ ] No snapshot test implicitly encodes the current dedup behavior by
      asserting on specific pool sizes for `Path/Glob/Expr` cases.
      Snapshot updates are expected; flag any that show a **change in
      structure** rather than just pool indices shifting.
- [ ] `SourceInfo::FilterProvenance` stays correct — it's not Arc-held
      and may be cloned; the redesign assigns it a fresh pool ID per
      occurrence, which is fine (same as Original).

Still a draft — before starting the redesign, we should answer the
open question about the JSON consumer, and decide whether we want a
one-line `impl Hash for SourceInfo` as a belt-and-suspenders for a
future change that does want content-based dedup (pure addition; no
current caller needs it).
