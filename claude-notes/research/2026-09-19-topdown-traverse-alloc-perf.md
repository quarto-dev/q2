# Why `topdown_traverse_blocks::walk_vec` bottoms out in the allocator (and memmove)

**Status:** fact-finding, no solution committed. Branch
`perf/topdown-traverse-alloc` (off `main` @ `6b7be8f0`). The experimental
code changes described below are left **uncommitted in the working tree**
of that branch so they can be diffed and discarded or picked up
individually; only this note is committed.

Related: bd-5yektmwt ("AST construction memmove is ~28% of non-SCSS
render CPU"), `claude-notes/research/2026-09-13-connect-docs-render-profile.md`,
`claude-notes/research/2026-09-17-connect-docs-render-profile.md`. This
note is the "attribute memmove to callers before designing" step those
asked for, restricted to the `pampa::filters` traversal.

## Fixture and method

- Document: `docs-quarto-2/api/index.qmd` in `cscheid/q2-connect-docs`
  (1.1 MB, generated OpenAPI reference). Rendered as a single file inside
  the project: `q2 render api/index.qmd`, warm SCSS cache.
- AST size (from `pampa -t json`, counting `t` tags): ~200k inlines
  (Str 99,850; Space 82,494; Code 11,621; SoftBreak 3,009; Link 1,146;
  Strong 931; Span 617), ~25k blocks (Para 14,722; Plain 6,690; Header
  1,474; Div 1,178; Table 1,012; ~18.5k table cells).
- Profile: Carlos's samply capture `warm.json.gz` + `warm.json.syms.json`
  (release-perf build, 1 ms interval, 1,021 samples on the main thread,
  so the render is ~1 s). The JSON is unsymbolicated in place; the
  `.syms.json` carries per-library address → inline-frame chains. I wrote
  a small symbolicator + aggregator (scratch scripts, not kept) to bucket
  samples by inclusive function, leaf, and pipeline stage.
- Counters: a counting `#[global_allocator]` wrapper in the `q2` binary
  plus per-pass counters in `pampa::filters::topdown_traverse` (both
  gated on `QUARTO_PERF_STATS`, printed as `perf.alloc` / `perf.topdown` /
  `perf.topdown.pass`, following the `perf.<gauge>` convention). These
  give exact allocation counts and bytes per traversal pass, which the
  sampled profile cannot.
- Timing: `hyperfine -w 2 -r 10 -N` on saved binaries.

## What the profile says

### Where `walk_vec` sits

| Bucket (inclusive, main thread)                        | samples | share |
| ------------------------------------------------------ | ------: | ----: |
| `ParseDocumentStage`                                   |     413 | 40.5% |
| `AstTransformsStage` (66 hand-rolled transforms)       |     138 | 13.5% |
| `UserFiltersStage` (Lua walk)                          |     118 | 11.6% |
| `IncludeExpansionStage` (two `dedup_scoped_heading_ids`) |      58 |  5.7% |
| `pampa::filters::topdown_traverse` (all passes)        |     189 | 18.5% |

Every one of the 189 `walk_vec` samples has one of four callers, all in
pampa's parse-time postprocessing, none in quarto-core's transforms
(quarto-core has **zero** callers of `topdown_traverse`; its 66
transforms each carry their own walker):

| caller of the outermost `topdown_traverse`              | samples |
| ------------------------------------------------------- | ------: |
| `postprocess::merge_strs` (`with_inlines`)              |      62 |
| `autoid::dedup_scoped_heading_ids` (2 passes, `with_header`) |      56 |
| `postprocess::postprocess` (the big combined filter)    |      43 |
| `readers::qmd::read` (`with_raw_block`, metadata lift)  |      28 |

### The leaf is memmove, not malloc

Leaf distribution of the 189 `walk_vec` samples: **98 `_platform_memmove`**,
51 inside `libsystem_malloc`, 10 self time in `topdown_traverse_inline`,
the rest noise. Across the whole profile memmove is the single largest
leaf (281 samples, **27.5%**) and `libsystem_malloc` is second (205,
20%). The frames between `walk_vec` and memmove are `ptr::read` /
`Vec::append_elements` / `spec_extend` — i.e. moving AST values by
value, not copying strings.

### Why: the enums are enormous

`std::mem::size_of` (release-perf, arm64):

| type          | bytes | why                                                                                  |
| ------------- | ----: | ------------------------------------------------------------------------------------ |
| `Inline`      |  **776** | largest variants `Link`/`Image` = 768                                              |
| `Link`/`Image`|   768 | `TargetSourceInfo` 272 + `AttrSourceInfo` 184 + `SourceInfo` 136 + `Attr` 104 + target 48 + content 24 |
| `Span`, `Code`|   448 | attr + attr_source + source_info                                                     |
| `Str`         |   160 | `String` 24 + `SourceInfo` 136                                                       |
| `Space`       |   136 | just a `SourceInfo`                                                                  |
| `SourceInfo`  |   136 | `Generated { by: By, from: SmallVec<[Anchor; 2]> }` is the fat variant               |
| `Block`       | **1552** | largest variant `Table` = 1552 (`TableHead` 448 + `TableFoot` 448 + …)            |
| `Figure`      |   632 | next-largest block after Table                                                       |
| `Header`/`Div`|   456/448 |                                                                                  |
| `Cell`        |   472 |                                                                                      |

So a `Vec<Inline>` holding a paragraph of 20 words is 15.5 KB, and 82k
`Space` nodes each occupy 776 bytes of which 136 are meaningful. Every
by-value move of an `Inline` is a 776-byte memcpy; every `Block` move is
1.5 KB.

### How the traversal multiplies the moves

`topdown_traverse_inline(inline) -> Inlines` (pre-experiment):

1. `walk_vec` starts `result = vec![]` (no capacity) and for each element
   calls `topdown_traverse_inline`, which for *every* node — including
   the ~185k terminal `Str`/`Space`/`Code`/`SoftBreak` — returns
   `vec![inline]`: one heap alloc of 776 B, one 776 B copy in.
2. `result.extend(...)`: another 776 B copy out, then the singleton Vec is
   freed. `result` grows by doubling, so each element is copied again
   ~once more on average during reallocs.
3. Every container is rebuilt by value (`Inline::Emph(Emph { content:
   topdown_traverse_inlines(e.content), ..e })`): the old `Vec` is
   consumed and a new one allocated, per container per pass, even when
   the filter touched nothing.
4. The filter API takes ownership (`FnMut(T, &mut ctx) -> FilterReturn<T, Vec<T>>`),
   so even `Unchanged` nodes make the round trip through the closure by
   value.

Measured per full pass over this document (counting allocator, baseline
code):

| pass                           |  ms | allocs | reallocs | bytes requested |
| ------------------------------ | --: | -----: | -------: | --------------: |
| `postprocess` (combined filter) |  49 |   294k |      28k |          727 MB |
| `merge_strs`                    |  68 |   554k |      71k |         1.55 GB |
| `qmd::read` `with_raw_block`    |  29 |   268k |      25k |          675 MB |
| `dedup_scoped_heading_ids` #1   |  29 |   269k |      25k |          675 MB |
| `dedup_scoped_heading_ids` #2   |  29 |   268k |      25k |          675 MB |
| **all 110 passes**              | **206** | **1.66M** | **175k** | **4.3 GB** |

Whole render: 6.33M allocations, 9.17 GB requested, 734k reallocs, in
~0.92 s of pass-2 wall time. The traversal alone is 26% of the
allocation count and 47% of the bytes. A "pure" pass (one block-type
filter that changes nothing) costs 29 ms and 268k allocations: ~200k
singleton Vecs for the inlines, ~25k for the blocks, ~25k+16k container
Vecs, plus 25k growth reallocs. That is about 145 ns and 3.4 KB of
allocator traffic per inline node for doing nothing.

Note the five passes above are the only ones that matter; the other 105
`topdown_traverse` calls are over tiny documents (project config
markdown, include stubs, etc.).

### The closures add their own waste

- `postprocess`'s `with_inlines` step 0 did `break_cleaned.push(inlines[i].clone())`
  for every element — a **deep clone of every inline in the document**,
  at every nesting level — and step 1 cloned each element again into
  `math_processed`. Since containers are visited recursively, nested
  content was cloned once per ancestor level.
- `merge_strs` cloned every `Str`'s text (`s.text.clone()`) before
  deciding whether to merge it, and built `result` without capacity; then
  `coalesce_abbreviations` builds a third vector.
- `dedup_scoped_heading_ids` runs two full tree-rebuilding passes to
  touch 1,474 headers; pass 1 only reads.

### The same pattern outside `pampa::filters`

This is not specific to `topdown_traverse`; it is the AST's cost model.
memmove leaf samples by phase: `pampa postprocess` 56, `TransformPipeline`
55, `treesitter_to_pandoc` 51, Lua `apply_filters` 50, `dedup` 28. Inside
`AstTransformsStage`, `LlmsCaptureTransform` alone is 83 of 138 samples
(deep `Block::clone` of the document plus `clean_inlines`), and
`shortcode_resolve::resolve_inlines` shows the same rebuild-by-value
shape. The Lua walker (`pampa::lua::walk`) moves `Vec<Inline>` by value
through async fns. Anything that shrinks `Inline`/`Block` or removes
by-value moves pays off in all of these, not just in the five passes
above.

## Experiments

All three change nothing observable: `cargo nextest run -p pampa -p
quarto-core` → 9,261 passed, 0 failed, on the B+D tree.

### B — push-into traversal + capacity reservation (`crates/pampa/src/filters.rs`)

`topdown_traverse_inline_into(inline, filter, ctx, out: &mut Inlines)`
pushes results into the caller's vector; the `handle_*_filter!` /
`*_apply_and_maybe_recurse!` macros take an `$out` and `push`/`extend`
instead of building `vec![..]`. `walk_vec` allocates
`Vec::with_capacity(vec.len())`. The public `topdown_traverse_inline` /
`_block` keep their signatures as thin wrappers.

| pass                          | before | after B | allocs before → after |
| ----------------------------- | -----: | ------: | --------------------: |
| pure walk (each of 3)         |  29 ms |   15 ms |        268k → 43k     |
| `merge_strs`                  |  68 ms |   55 ms |        554k → 329k    |
| `postprocess`                 |  49 ms |   46 ms |        294k → 235k    |
| all passes, bytes requested   | 4.3 GB |  2.2 GB |  reallocs 175k → 66k  |

End-to-end (hyperfine, 10 runs): **1.062 s → 0.981–0.991 s**, i.e. −7%.
The remaining 15 ms per pure pass is the two unavoidable 776 B moves per
node (read out of the old Vec, write into the new one) plus one
alloc/free per container.

### D — stop cloning in the postprocess closures (`postprocess.rs`)

`with_inlines` step 0 moves elements instead of cloning (a one-flag
state machine for the LineBreak+SoftBreak rule); step 1 has a fast path
that moves the whole vector through when it contains no `Attr`;
`merge_strs` moves `s.text` instead of cloning it and reserves capacity.

| pass           | after B | after B+D | allocs |
| -------------- | ------: | --------: | -----: |
| `postprocess`  |   46 ms |     38 ms | 235k → 171k |
| `merge_strs`   |   55 ms |     48 ms | 329k → 227k |

End-to-end: **0.963–0.968 s** (−10% vs baseline).

### E — mimalloc as the `q2` binary's global allocator

Two lines in `crates/quarto/{Cargo.toml,main.rs}` (the counting wrapper
now wraps `mimalloc::MiMalloc` instead of `System`). Native binary only;
the WASM crate is untouched.

End-to-end: **0.865 s ± 0.005** (−10% vs B+D, **−23% vs baseline**; user
0.73 s vs 0.87 s, sys 0.27 s vs 0.33 s). This is consistent with
`libsystem_malloc` being 20% of self time in the profile: mimalloc's
fast path is much cheaper than macOS's malloc for the 6M small
allocations this render does, and the win would likely be *larger* on
the Linux release binaries, where musl's mallocng is slower still.

#### History of the mimalloc question

Searched `claude-notes/`, braid, and git log. mimalloc was **never
rejected on musl grounds**. The record is decision **D4** in
`claude-notes/plans/2026-07-28-linux-release-static-musl.md`
(bd-dofxhzaj): the plan identified musl's allocator performance as "the
one thing genuinely not de-risked", and Carlos's call was *"We don't
care about perf right now, not without a demonstrated pathological case
from a real scenario"* — so "no benchmark phase and no `mimalloc`
change", to be revisited as its own strand once a real repro exists.
This note is arguably that repro. Two caveats from the same notes:

- `claude-notes/instructions/performance-profiling.md` records an
  earlier mimalloc trial that "changed nothing" — but that was chasing a
  tree-sitter lock that turned out to be the locale lock in `snprintf`,
  not an allocator problem. Negative result there, positive here.
- `#[global_allocator]` only covers Rust allocations. tree-sitter's C
  side (~13% of this profile in `ts_parser_parse`) still calls libc
  `malloc` unless `tree_sitter::set_allocator` is also pointed at
  mimalloc. Worth a follow-up measurement.
- The release legs are static `*-unknown-linux-musl` built with
  `musl-gcc` (see the release runbook); `libmimalloc-sys` 0.1.49 is
  plain C built via `cc`, its `build.rs` has no libc-specific branches
  (it keys on `target_os`, not `target_env`), upstream mimalloc supports
  musl and Alpine packages it. So it *should* build on both musl legs,
  but it has not been tried in this repo's CI, and there is no musl
  cross toolchain or Docker on this machine to try locally. That is the
  one thing to verify before adopting: push the branch and run the
  release pipeline's dry-run (its `Assert the binary needs no glibc
  (Alpine)` step is exactly the check), or `docker run alpine` a
  `cargo build --target x86_64-unknown-linux-musl` on a Linux box.

Carlos, on seeing the −10%: *"if a 10% overall win is in play, maybe we
should reconsider and try harder to use mimalloc"* — so suggestion 2
below is live, not hypothetical.

### Microbenchmark: cost vs. element size

`crates/pampa/tests/integration/zz_sizes_experiment.rs` (experiment file,
not to be kept): 200k elements in 16-element vectors, 3 passes.

| element             | naive (`vec![x]` + extend, no capacity) | push-into + capacity | in place (`&mut`, no moves) |
| ------------------- | --------------------------------------: | -------------------: | --------------------------: |
| `Inline` (776 B)    |                                 69.2 ms |              45.4 ms |                     13.6 ms |
| 160 B struct        |                                 11.0 ms |               6.5 ms |                      0.9 ms |
| 40 B struct         |                                  3.9 ms |               2.5 ms |                      0.5 ms |

Two independent multipliers: element size (776 → 160 B is ~6× on the
rebuild cost) and rebuild-vs-in-place (another ~3–7×). Experiment B moved
us from column 1 to column 2 only.

## Assessment

The mechanism behind "stacks through `walk_vec` bottom out at
allocators" is three stacked costs, in decreasing order of leverage:

1. **`Inline` and `Block` are 776 and 1552 bytes** because the enum is
   sized by its fattest variant (`Link`/`Image`, `Table`), and every node
   carries a 136-byte `SourceInfo` whose size is set by the `Generated`
   variant's inline `SmallVec<[Anchor; 2]>`. Every move, clone, drop, and
   `Vec` growth anywhere in the pipeline pays for this, which is why
   memmove is 27.5% of the whole profile and not just of the traversal.
2. **The traversal rebuilds the tree by value on every pass** and (before
   B) allocated a singleton `Vec` per node. Five full passes over this
   document happen before the first quarto-core transform even runs.
3. **A few closures deep-clone the document** for no reason (postprocess
   step 0/1, merge_strs text clone). Cheap to fix, and D fixed the ones
   on the hot passes.

The allocator itself is the fourth term, orthogonal to the other three,
and the cheapest 10% available.

## Suggestions, ranked

1. **Land B + D as-is** (after a review pass; they are small, local, and
   test-clean). ~10% end-to-end on this document, less alloc pressure
   everywhere `topdown_traverse` is used. Keep the `perf.topdown`
   counters (they follow the `QUARTO_PERF_STATS` convention) but drop the
   backtrace print, the counting allocator, and the size/benchmark test.
2. **Open the mimalloc strand** that D4 in the musl plan said should
   exist once there was a repro. Verify the two musl legs build it (a
   nightly run would tell), then measure again with
   `tree_sitter::set_allocator` also redirected. −10% for two lines is
   hard to beat; the only real risk is the build matrix.
3. **Shrink `Inline`/`Block`.** Options, from least to most invasive:
   - Box the source-info side of `Link`/`Image` (`attr_source`,
     `target_source`: 456 of the 768 bytes) — `Inline` drops to ~456
     (Span/Code), then boxing `AttrSourceInfo` in `Span`/`Code`/`InlineAttr`
     too brings it near 250. Serialization is unaffected (serde sees
     through `Box`); pattern matches on `attr_source` need `*` in a
     handful of places.
   - Box `Table` (and `Figure`) inside `Block` → `Block` ≈ 480.
   - Shrink `SourceInfo` itself by boxing the `Generated` payload (or
     making `from` a `Vec`): 136 → ~40 bytes, which halves `Str` and
     `Space` and cuts every `AttrSourceInfo` in proportion. This is in the
     externalized `quarto-source-map` crate, so it is a cross-repo change,
     but it is the single biggest lever since every node has ≥1
     `SourceInfo`. Measure with the counting allocator + the size test
     before choosing; the microbenchmark says the walk cost scales
     roughly linearly with element size.
4. **An in-place (`&mut`) visitor for the read-mostly passes.** The
   ownership-taking `Filter` API forces the rebuild. The five hot passes
   don't need ownership: `dedup` pass 1 only reads headers, pass 2
   mutates `header.attr.0` in place, the raw-block lift replaces a block
   in place, `merge_strs` replaces a vector's contents. A
   `fn visit_mut(&mut Block/Inline, ctx) -> Action { Keep, Replace(Vec<_>) }`
   walker with a rare splice path would run these at the "in place"
   column of the microbenchmark (~5× cheaper than B). This is a new API
   surface next to `Filter`, not a rewrite of it; `Filter` can stay for
   the callers that genuinely restructure. Worth doing after (3) — the
   relative win is larger once nodes are small, since the container
   alloc/free then dominates.
5. **Fewer passes over the parse output.** `dedup` pass 1 could be a
   header iterator over `&Pandoc` (zero allocs) instead of a rebuild;
   `merge_strs` could run inside the postprocess `with_inlines` closure
   (it already rebuilds every inline vector) instead of a second full
   pass. Each removed pass is 15–30 ms here.
6. **Out of scope but visible in the same profile:** `LlmsCaptureTransform`
   deep-clones the whole document (83 samples, 8% — the largest single
   transform), and the project-level file walks (`discovery::walk_rec`,
   `glob::expand::walk_rec`, `dependency_graph`, ~90 samples of `stat`/
   `open`/`getdirentries`) are ~9% of a single-file render. Neither is a
   traversal problem, both are bigger than any one pass above.

## Reproduce

```bash
# in q2, on perf/topdown-traverse-alloc with the working-tree experiments applied
cargo build --profile release-perf --bin q2
cd ~/repos/github/cscheid/q2-connect-docs/docs-quarto-2
QUARTO_PERF_STATS=2 ~/rooms/room-5/q2/target/release-perf/q2 render api/index.qmd 2>&1 | grep '^perf\.topdown'
hyperfine -w 2 -r 10 -N '<baseline q2> render api/index.qmd' '<experiment q2> render api/index.qmd'
# size table + move-cost microbenchmark
cargo nextest run -p pampa --test integration zz_sizes_experiment --no-capture --cargo-profile release-perf
```
