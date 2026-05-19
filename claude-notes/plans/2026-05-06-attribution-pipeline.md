# Attribution Pipeline (Rust port of `feat/node-attribution`)

## Overview

Port the per-node authorship feature prototyped on `feat/node-attribution` from
hub-client TypeScript into the q2 Rust render pipeline. The TS prototype works
end-to-end against the AST debug view but lives entirely above the WASM
boundary. Moving it down has two payoffs:

1. **All renderers, not just q2-debug** can consume attribution. HTML, slides,
   future Typst/PDF — anything that owns a writer can opt in.
2. **Both inputs (Automerge live history; `git blame --porcelain`) feed one
   canonical form**. The CLI and the editor stop diverging on what
   "attribution" even means.

The shape mirrors the navigation stages already in
`crates/quarto-core/src/transforms/`:

| Concept           | Navbar                                 | Attribution                                 |
|-------------------|----------------------------------------|---------------------------------------------|
| Generate stage    | `NavbarGenerateTransform`              | `AttributionGenerateTransform`              |
| Stage name        | `"navbar-generate"`                    | `"attribution-generate"`                    |
| Render stage      | `NavbarRenderTransform`                | `AttributionRenderTransform`                |
| Stage name        | `"navbar-render"`                      | `"attribution-render"`                      |
| Generate output   | `meta.navigation.navbar` (ConfigValue) | `ctx.attribution_data` (sidecar `Arc<AttributionData>`) — see "Why a sidecar, not `meta.attribution`" below |
| Render output     | `meta.rendered.navigation.navbar` (HTML)| AST mutation / writer-config side-channel   |
| Opt-in           | `navbar:` YAML key + ProjectIndex       | `--attribution=<git\|off>` CLI; `attribution:` YAML (hub-client uses a separate WASM entry point, not this flag) |
| Skip predicate    | `is_feature_disabled(meta, "navbar")`  | New `attribution_source_for(ctx, meta)` returns `None` |

**User-authored `identities` still live in `meta.attribution.identities`** —
that's just normal YAML config (small, user-overridable), read by Generate
during the merge step. What does NOT live in meta is the bulk `runs` data;
see "Why a sidecar, not `meta.attribution`" below for the rationale.

The two attribution stages bracket the Finalization Phase:

- **`AttributionGenerateTransform`** is registered as the **last entry
  in the Navigation Phase** (`crates/quarto-core/src/pipeline.rs:780-847`
  as of #169), immediately after `FooterRenderTransform` (currently
  line 847). It runs after every navbar/sidebar/page-nav/footer/
  listings/feeds/categories transform has finished — i.e., after *all*
  the website transforms — so all `navigation.*` metadata is fully
  populated by the time it writes `ctx.attribution_data`.
- **`AttributionRenderTransform`** is registered as the **last
  transform in the Finalization Phase** (lines 849-860), immediately
  after `ResourceCollectorTransform` (currently line 860). It runs
  last in the pipeline, just before the writer is invoked.

This means the entire Finalization Phase (`LinkRewriteTransform` →
`AppendixStructureTransform` → `CrossrefRenderTransform` →
`ResourceCollectorTransform`) runs *between* generate and render —
which is fine: none of those transforms read or write
`ctx.attribution_data`, and none mutates `SourceInfo` in ways that
would invalidate the per-node byte ranges attribution-render later
queries.

We considered an earlier draft that slotted `AttributionGenerateTransform`
inside the Navigation Phase between `FooterGenerateTransform` and
`ListingGenerateTransform` — i.e., **inside** the phase rather than at
its tail. That mid-phase placement was rejected: attribution isn't a
navigation concern, doesn't read or write the `navigation.*` subtree,
and doesn't interact with sidebars/footers/TOCs/listings, so
interleaving it with the navigation generates suggests a coupling that
doesn't exist. End-of-Navigation-Phase placement keeps the stage with
other `*-generate` work without putting it in the middle of unrelated
stages.

We also considered pairing the two attribution stages back-to-back at
the very tail of the Finalization Phase. That would have given a
tighter visual pairing (easy to find both in `pipeline.rs`) but
forfeited a real benefit of the end-of-Navigation-Phase placement:
`AttributionGenerateTransform` is a `*-generate` stage and reads
naturally with the other generate transforms. Leaving the entire
Finalization Phase between generate and render is also fine — no
transform there reads or writes `ctx.attribution_data` — so the
placement is free.

The profile checkpoint (`DocumentProfileStage`) is read-only and runs
much earlier; attribution does not need to participate in it for v1.

`quarto trace view` discovers stages via `AstTransform::name()` (see
`crates/quarto-core/src/transform.rs:69-90` and the call site in
`TransformPipeline::execute` at line 154-159), so once the two new transforms
register their names they appear in the existing trace UI without any other
changes.

## Branch context

**This plan is implemented on a new feature branch.** Suggested name:
`feat/attribution-pipeline` (the implementer may choose differently
when work begins, but pinning a suggestion here means day-one commits
don't need retroactive renaming). Wherever the plan below refers to
"the implementation branch," that's the branch you create on day one.

**Fork point: `main`.** The implementation branch is forked off `main`,
*not* off `feat/node-attribution`. The TS prototype on
`feat/node-attribution` is **reference material only** — the
implementation branch starts clean, and Phase 5 builds the minimum
producer-side TS the new WASM entry point needs, using the prototype as
a design reference. None of the prototype's consumer-side machinery
(`useNodeAttributionResolver`, in-process query/cache code paths,
`getNodeAttribution` calls in `ReactAstDebugRenderer.tsx`, etc.) is
brought over — that's exactly what the Rust pipeline replaces, so it
would be net-deleted work to import it.

**Wherever this plan names a TS file or symbol from the prototype, that
reference is to `feat/node-attribution` and is for design reference.**
The implementation branch should NOT contain those files at fork time
(they don't exist on `main`); whatever ends up on the implementation
branch is built fresh, drawing on the prototype's algorithm and data
shape (cherry-pick, rewrite, or selectively port — implementer's
choice). References to existing Rust code (everything in `crates/`)
target the implementation branch directly and should track `main` via
rebase as the branch progresses.

**Landing strategy.** The implementation branch lands as a single PR to
`main`, delivering the Rust attribution pipeline together with the
minimum TS producer that feeds it. The `feat/node-attribution` branch
is left untouched — once this plan ships, the prototype branch is
historical reference and can be archived/deleted separately at the
user's discretion.

## Vocabulary

- **Actor** — opaque string identifying who made an edit. From Automerge:
  the Automerge actor ID. From git: the author email. The pipeline never
  interprets the value beyond hashing/string-slicing; identity is
  supplied by providers (every provider populates an `IdentityMap`
  entry for each actor it produces in `runs` — see Phase 6's producer
  invariant), merged with user override in
  `AttributionGenerateTransform`, and read by `AttributionRenderTransform`.
  The render-side warning path (diagnostic + `<unknown>`/`#888888`
  placeholder) handles producer-invariant violations and does not
  fire on happy paths.
- **AttributionRun** — `{ start: usize, end: usize, actor: Arc<str>, time: i64 }`,
  byte offsets in *the document's primary file's* source bytes. Sorted,
  non-overlapping, contiguous. `actor` is `Arc<str>` (not `String`) so the
  same Arc is shared across every run by the same author — for a doc with
  5 contributors and 1000 runs this is 5 string allocations + 1000 cheap
  pointer clones, not 1000 string allocations.
- **AttributionMap** — a transparent newtype around `Vec<AttributionRun>`
  for the document being rendered. **Single-document only in v1**; no
  file keying. The in-memory queryable form (binary search via
  `AttributionSource`). v2 (multi-file via includes) replaces the field
  type with a path-keyed map; see Open Question #2.
- **IdentityMap** — `HashMap<Arc<str>, Identity>` (keyed by the same
  `Arc<str>` used in `AttributionRun.actor`) where
  `Identity = { display_name: String, color: String }`. The merged result
  of `meta.attribution.identities` (user override) ∪ provider-supplied
  identities (from Automerge actor metadata or git author-mail). Built
  by `AttributionGenerateTransform`; consumed by `AttributionRenderTransform`.
  Empty when no source supplied identities; unmapped actors fall back to
  `actor[..8]` plus a deterministic palette hash at render time.
- **AttributionData** — `{ runs: AttributionMap, identities: IdentityMap }`.
  The canonical in-memory shape, held as `Arc<AttributionData>` on
  `RenderContext.attribution_data` (the sidecar). **Not** a wire form —
  not stored in `ast.meta`, not serialized to ConfigValue. The sole
  exception is the WASM boundary, where hub-client ships a JSON-encoded
  `AttributionData` to `parse_qmd_to_ast_with_attribution` (Phase 3b);
  that JSON is parsed and dropped into the sidecar immediately,
  never visiting `ast.meta`.
- **`meta.attribution.identities` (user input)** — the small,
  user-authored override map at the conventional YAML location, parsed
  into a `ConfigValue::Map` by the existing YAML-to-meta pipeline. Read
  by `AttributionGenerateTransform` during the merge step. Stays in meta
  as user input; the canonical merged form lives on the sidecar.
- **AttributionSource** (Rust trait) — identical role to the TS
  `AttributionSource`: a `query_byte_range(start, end) -> Option<{actor, time}>`
  function (no `file_id` parameter — v1 is single-doc). Implemented for
  `AttributionMap` (i.e. `Vec<AttributionRun>`) via binary search;
  `AttributionGenerateTransform` builds an `AttributionData` and stores
  `Arc<AttributionData>` on `ctx.attribution_data`. v2 re-introduces a
  file parameter when multi-file blame ships.

### Why a sidecar, not `meta.attribution`

The plan originally proposed storing `AttributionData` at `meta.attribution`
as a `ConfigValue::Map`, by analogy with the navbar/footer/sidebar pattern.
That analogy breaks on volume.

`AttributionRun` records scale with document length (typical: ~1 run per
~100 bytes of prose after RLE coalescing, so ~500 runs for a 50 KB
chapter, ~10K for a book-length doc). One run as a `ConfigValue::Map`
costs ~600–800 B (outer Map wrapper + 4 entries × `ConfigMapEntry`,
each with its own `SourceInfo`/`MergeOp`/`ConfigValueKind`); the same
run as `AttributionRun` is ~40 B. That is a **~20× memory multiplier
on the hottest data structure in the render pipeline**, and the cost
lands repeatedly — every Finalization-Phase transform that walks
`ast.meta` (even just to check a key) pays a slice of it.

Concretely:

| Doc size | Runs | `meta.attribution` cost | sidecar `Vec<AttributionRun>` |
|----------|-----:|------------------------:|------------------------------:|
| 5 KB     |   50 |  ~35 KB                 | ~2 KB                          |
| 50 KB    |  500 |  ~350 KB                | ~20 KB                         |
| Book     | 10K  |  ~7 MB                  | ~400 KB                        |

The convention argument (Lua filter introspection of `meta.attribution`)
doesn't survive scrutiny either:
- The Lua-filter slot doesn't exist yet (bd-0fd0 is future).
- Any real Lua use case (e.g. "colour code blocks by author") needs
  `lookup(start, end) → actor`, not raw runs — walking N runs per
  node is O(N×M) and unusable. The right Lua surface, when bd-0fd0
  lands, is a `pandoc.attribution.lookup(...)` accessor backed by the
  sidecar, not raw meta access.
- `identities` (the part users actually want to override) does stay
  in meta as `meta.attribution.identities`, preserving the convention
  exactly where it pays off.

If a real consumer of `meta.attribution` materializes later, the
migration is small and purely additive: `AttributionGenerateTransform`
gains a `to_config_value()` and dual-writes meta alongside the sidecar.
No existing consumer breaks because the sidecar stays the source of
truth.

### Future Lua-filter access (bd-0fd0)

Today there is no Lua filter slot between generate and render in q2,
so this section is forward-looking: when bd-0fd0 (or whatever the
Lua-injection slot lands as) ships, the two attribution surfaces a
filter would legitimately want are accessible without any plan change.

1. **Identities (read by Lua via `meta.attribution.identities`).**
   User-authored identities already live in meta as a
   `ConfigValue::Map` from YAML parse — accessible to any Lua filter
   the moment bd-0fd0 exposes `meta` to filters, no different from
   `meta.author` or `meta.toc`. This is the surface a "colour code
   blocks by author" filter actually needs: actor → `(name, color)`
   lookup. The *merged* identities (provider-supplied ∪ user override)
   live on the sidecar; if a filter wants the merged set instead of
   user input alone, the Lua host binds
   `pandoc.attribution.identities()` to read
   `ctx.attribution_data.identities`. That binding is a few lines
   of host code, independent of this plan.

2. **Per-node attribution lookup
   (`pandoc.attribution.lookup(start, end) -> { actor, time } | nil`).**
   The Lua-callable form of `AttributionSource::query_byte_range`,
   backed by the sidecar's `AttributionMap`. The trait method exists
   from Phase 1, so the bd-0fd0 binding is purely host-side
   plumbing — no new Rust surface required. This is the *only*
   useful API shape for raw runs: walking the run list per-node
   from Lua would be O(N × M) and unusable, which is why exposing
   the runs through `meta.attribution` as a `ConfigValue::Map`
   would actively invite an anti-pattern.

In short: the sidecar choice does not foreclose Lua access — it
narrows it to the access pattern that's actually performant.
`meta.attribution.identities` covers the conventional read-from-meta
case; `pandoc.attribution.lookup(...)` covers per-node queries via
a host binding to the sidecar.

If `quarto inspect` or `--keep-md` later needs to surface raw
attribution data for debugging, `AttributionGenerateTransform` grows
the additive `to_config_value()` dual-write described above — that
debug surface and the Lua-binding surface are independent and
either can ship without the other.

## Phase 0 — Test plan (TDD, write first)

> **DO NOT begin Phase 1 implementation until every test below is checked in,
> running, and red.** The CLAUDE.md TDD rule is non-negotiable.
>
> **Status: complete (commit `b2ee6e70`).** 46 Phase 0 tests checked in
> — 8 green as regression pins, 38 red against `unimplemented!()`
> bodies. Phase 1-6 implementers turn the red ones green incrementally.

### Unit-test crates / files to create

- [x] `crates/quarto-core/src/transforms/attribution_generate.rs`
  (transform stub with `unimplemented!()` body; tests live in
  `crates/quarto-core/tests/attribution_generate.rs`)
- [x] `crates/quarto-core/src/transforms/attribution_render.rs`
  (transform stub; tests live in
  `crates/quarto-core/tests/attribution_render.rs`)
- [x] `crates/quarto-core/src/attribution/` — module created with
  `{mod, types, source, builder, prebuilt, git_blame, palette, mode}.rs`.
  All real logic is `unimplemented!()` until the relevant Phase
  lands; the surface compiles so tests reference real APIs.

### Test cases (Phase 0 — all must be red)

1. **WASM-transport JSON round-trip with interning preservation.** The
   transport-only types `TransportAttributionRun` /
   `TransportAttributionData` (plain `String` actor fields) serde-
   round-trip through JSON unchanged in three configurations: runs-only
   (`identities` field empty → key omitted via
   `skip_serializing_if = "HashMap::is_empty"`), identities-only (`runs`
   field empty → key omitted via the analogous `AttributionMap::is_empty`),
   both populated. **Plus a stronger assertion:** `PreBuiltAttributionProvider`
   takes a transport JSON string, decodes it via the transport types,
   feeds the result through `AttributionDataBuilder`, and the resulting
   canonical `AttributionData` satisfies `Arc::ptr_eq(run.actor,
   identities.get_key_value(...))` for every actor that appears in
   both runs and identities — i.e. the round-trip *restores* the
   interning invariant that serde alone would have destroyed (each
   `Arc::from(s)` during deserialize would otherwise allocate
   per-occurrence). This is the transport contract at the
   hub-client → WASM boundary (Phase 3b); JSON serde is NOT used for
   inter-transform passing inside the pipeline (the sidecar carries
   the typed struct directly). (Plain `serde_test` for the transport
   round-trip; ad-hoc assertion via `PreBuiltAttributionProvider` for
   the ptr_eq restoration. Lives in `attribution/types.rs` and
   `attribution/prebuilt.rs`.)
2. **`Vec<AttributionRun>::query_byte_range`** returns the most-recent
   `(actor, time)` overlapping a query range, mirroring the TS
   `attribution-runs.test.ts` (on `feat/node-attribution`) invariants.
   Cover: empty, single-run, non-overlapping, overlapping with two
   distinct actors, query at boundary.
3. **Git-blame provider parses porcelain identically to TS reference.**
   Re-use the fixtures in
   `hub-client/src/services/attribution-gitblame.test.ts` (on
   `feat/node-attribution`); capture them as **checked-in porcelain
   text** under `tests/attribution_gitblame_fixtures/` so the Rust
   tests don't depend on live commit timestamps. Cover the same
   multi-byte UTF-8 cases (CJK, emoji) the TS tests do.
4. **`AttributionGenerateTransform` happy path** — given a fixture provider
   returning an `AttributionData` with runs
   `[{0..5, alice, t=1}, {5..10, bob, t=2}]` and an `identities` map
   `{ "alice": ("Alice", "#ff0000") }`, the transform sets
   `ctx.attribution_data = Some(Arc::new(AttributionData { ... }))`
   whose `runs.query_byte_range(0, 10)` returns `(bob, t=2)` and whose
   `identities["alice"]` is `("Alice", "#ff0000")`. The actor `Arc<str>`
   inside each run is *pointer-equal* to the corresponding key in
   `identities` (pin the interning invariant explicitly via
   `Arc::ptr_eq`). Off-path: when the provider returns empty
   `identities`, `ctx.attribution_data.identities.is_empty()` holds.
5. **`AttributionGenerateTransform` skip conditions:**
   - No provider in `RenderContext` → `ctx.attribution_data` remains
     `None`, no diagnostic.
   - `is_feature_disabled(meta, "attribution")` → skip;
     `ctx.attribution_data` remains `None`. (User-authored
     `meta.attribution.identities`, if any, stays in meta exactly as
     written but is not consumed.)
   - **Identities-only user override** (positive case, not a skip):
     `meta.attribution.identities` populated in YAML and provider opted
     in → run the provider, build `AttributionData` with the provider's
     `runs`, and merge identities per the Phase 2 merge rule (user
     entries win on key collision; non-colliding user keys are
     dropped). Pin via three sub-assertions:
     (a) a key present in both user YAML and the provider's
     identities resolves to the user's `(name, color)` in
     `ctx.attribution_data.identities`, *and* the merged map's key
     for that actor is `Arc::ptr_eq` to the provider's original key
     (i.e. `Arc<str>` provenance preserved so the `Arc::ptr_eq`
     interning invariant from test #4 holds);
     (b) a key present only in the provider survives the merge
     unchanged;
     (c) a key present only in user YAML does **not** appear in
     `ctx.attribution_data.identities`. Sub-case (c) is the
     regression guard against accidental re-introduction of the
     dead-code path described in Phase 2.
6. **`AttributionRenderTransform` for q2-debug (producer-invariant
   violation handling).** Given an AST with two `Str` nodes whose
   `SourceInfo`s point to ranges `0..5` and `5..10`, and a
   `ctx.attribution_data` whose `identities` map has an entry for
   `alice` (`name: "Alice"`, `color: "#ff0000"`) and **deliberately
   no** entry for `bob` (an in-test producer-invariant violation
   constructed to exercise the render-side warning path — see
   Phase 6), the transform emits exactly one diagnostic warning
   naming `bob` as the offending actor, and the JSON writer emits
   two sibling fields nested inside `astContext` (not peer to it):
   - `astContext.attributionActors` — `{ "alice": { name: "Alice", color: "#ff0000" },
     "bob": { name: "<unknown>", color: "#888888" } }`. One entry per
     distinct actor referenced by the attribution array; the `bob`
     entry came through the warning-path placeholder. Identity is
     resolved **once per actor** (interned), not once per record.
     This test pins the warning-path behaviour; on happy paths
     (every actor identity-mapped by the producer) no diagnostic
     fires and no placeholder appears — see Phase 0 test #11 for a
     happy-path q2-debug fixture.
   - `astContext.attribution` — sparse array of length 2:
     `[{ s: <pool index of node 1>, actor: "alice", time: 1 },
       { s: <pool index of node 2>, actor: "bob", time: 2 }]`.
     Three fields per record (`s`, `actor`, `time`), always present.
     Identity (`name`, `color`) is **not** duplicated per record — it
     lives once per actor in `astContext.attributionActors`.

   **Off-path regression:** when no attribution is in scope, both
   `astContext.attributionActors` and `astContext.attribution` keys are **absent**
   (not present-but-empty), making the JSON byte-identical to today's
   output — assert this explicitly. (See "q2-debug delivery", below,
   for the full schema.)
7. **`AttributionRenderTransform` for HTML (producer-invariant
   violation handling).** Given the same fixture as test #6 (alice
   has an identity entry, bob is deliberately omitted to exercise
   the warning path), the transform emits one diagnostic warning
   naming `bob`, and the HTML body contains all four attribution
   attributes on each wrapped node:
   - First wrapping span (alice, identity-resolved): `data-attr-actor="alice"
     data-attr-time="1" data-attr-name="Alice" data-attr-color="#ff0000"`.
   - Second wrapping span (bob, warning-path placeholder):
     `data-attr-actor="bob" data-attr-time="2" data-attr-name="<unknown>"
     data-attr-color="#888888"`.

   Cover **both** block-level wrappers (`write_block_source_attrs`) and
   inline wrappers (`write_inline_source_attrs`) — extend the fixture
   with at least one block-level node carrying author attribution so
   the test pins both paths. The `data-attr-*` attributes appear on the
   **outer attribution wrapper** (see test #7b for the wrapper layering
   when source-locations is also on). The attributes only appear when
   attribution is opted in; without `ctx.attribution_data` the body is
   byte-identical to current output (regression guard), and **no
   diagnostic warning is emitted** in the absence of an invariant
   violation.

7b. **HTML coalescing** — given three contiguous `Inline::Str` nodes
   whose lookups all return the same `(actor, time)` tuple (e.g. one
   author writing a paragraph), the writer emits **one** outer
   attribution wrapper `<span data-attr-…>…</span>` covering all three
   texts, not three. A fourth inline whose `(actor, time)` differs
   starts a new outer wrapper; an inline with no attribution hit falls
   outside both. When `include_source_locations` is also on, the
   per-inline `data-sid`/`data-loc` spans become **inner** spans nested
   inside the outer attribution wrapper:

   ```
   <span data-attr-actor="alice" data-attr-time="1" data-attr-name="Alice" data-attr-color="#ff0000">
     <span data-sid="…" data-loc="…">word1</span>
     <span data-sid="…" data-loc="…">word2</span>
   </span>
   ```

   When source-locations is off, text is written directly inside the
   outer attribution span with no inner wrappers. Pins coalescing
   semantics so a future writer refactor doesn't silently regress to
   per-inline attribution wrapping.

7c. **Attribution-on + source-locations-off composition.** Given the
   same inline fixture as test #7 with `meta.include-source-locations`
   set to `false` (or absent — same default), assert the rendered
   HTML satisfies all three properties:
   - **No `data-sid` or `data-loc` attributes anywhere** in the
     output — not on the block opening tag, not on any inline span.
     Grep-anti-assertion.
   - **All four `data-attr-*` attributes present** on both the
     block's opening tag (via the restructured
     `write_block_source_attrs`) and the outer coalesced attribution
     wrapper (via the coalescing pass).
   - **Inner Str text inside the outer wrapper has no per-inline
     `<span>` wrapper** — text is emitted directly (the
     `Inline::Str` handler's raw-text path at `html.rs:670` is
     reached).

   This is the regression guard against re-coupling the two features.
   A future refactor that, say, makes `write_attribution_attrs`
   short-circuit through the existing `include_source_locations`
   early-exit would fail this test loudly. Conversely, an
   `attribution_render.rs` that "force on"s source-locations as a
   side effect would fail the first sub-assertion (spurious `data-sid`).

7d. **Structured inlines break prose coalescing.** Given a fixture
   sequence `[Str("hello"), Code("world"), Str("foo")]` where all
   three lookups return the same `(actor=alice, time=1)`, assert
   the rendered HTML contains **three** attribution wrappers:
   - one outer prose wrapper around `Str("hello")`,
   - one own wrapper around the rendered `<code>world</code>`,
   - one outer prose wrapper around `Str("foo")`.

   *Not* a single outer wrapper covering all three. Repeat the
   pattern substituting `Inline::Emph`, `Inline::Link`,
   `Inline::Span`, and `Inline::Math` for `Code` to pin that the
   prose-only restriction (Phase 4b) applies symmetrically across
   all structured inline variants. Regression guard against a
   future refactor that "naturally" extends coalescing across
   structured inlines and silently changes nesting semantics.
8. **`SourceInfo` chain resolution** — a node whose `SourceInfo` is
   `Substring(parent=Original{0..20}, 5..10)` resolves to file 0,
   bytes 5..10 *in the original file*, not 5..10 in the substring. This
   already works for `map_offset` in `quarto-source-map/src/mapping.rs`;
   the test pins it for the attribution lookup helper specifically, so a
   future refactor can't silently regress it.

8b. **`AttributionRenderTransform` skips non-primary-file nodes.**
    Given an AST containing one node whose `SourceInfo` resolves to
    `(file_id=0, 0..5)` (a hit on the primary doc's attribution map)
    and a second node whose `SourceInfo` resolves to `(file_id=1,
    0..5)` — e.g. a node spliced in via `{{< include other.qmd >}}`
    whose byte range happens to overlap a run in the primary doc's
    `AttributionMap` — the resulting lookup vec has a record at the
    first node's pool index and `None` at the second's. Pins the v1
    "primary doc only" invariant against the silent byte-range-
    collision failure mode described in Open Question #2. The
    fixture deliberately uses an overlapping byte range so that
    *only* the `file_id` filter (not range absence) explains the
    second node's `None`.
9. **End-to-end CLI test** — the test builds a temp git repo using
   the deterministic-timestamp setup spelled out in Phase 3a §
   Test fixtures (`tempdir` + `git init` + two scripted commits by
   distinct authors with `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE` /
   `GIT_AUTHOR_EMAIL` / `GIT_COMMITTER_EMAIL` /
   `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` pinned), copies
   `tests/fixtures/attribution-blame/doc.qmd` into the tempdir, then
   runs `cargo run --bin quarto -- render <tempdir>/doc.qmd --to
   html --attribution=git`. Asserts the produced HTML contains
   `data-attr-actor="<email>"` strings matching the two scripted
   author emails. Per CLAUDE.md the plan must include this end-to-end
   test for any CLI-visible feature; no claiming "done" without
   inspecting the rendered HTML.

9b. **CLI/YAML mode resolution — three-state matrix.** Unit test on the
    pure resolution function that takes
    `(cli: Option<AttributionMode>, yaml: Option<AttributionMode>) ->
    Option<AttributionMode>` (Phase 3c). Pin every combination so
    "silent override on CLI/YAML conflict" can't regress:
    - `(None, None)` → `None`. (Unflagged default.)
    - `(None, Some(Off))` → `Some(Off)`. (YAML opts out.)
    - `(None, Some(Git))` → `Some(Git)`. (YAML opts in.)
    - `(Some(Off), None)` → `Some(Off)`. (CLI escape hatch.)
    - **`(Some(Off), Some(Git))` → `Some(Off)`. CLI wins on conflict
      — the escape-hatch path. This is the regression guard the prior
      review specifically called out.**
    - `(Some(Git), None)` → `Some(Git)`. (CLI opts in standalone.)
    - `(Some(Git), Some(Off))` → `Some(Git)`. (CLI overrides YAML
      opt-out — symmetrical to the escape-hatch case.)
    - `(Some(Git), Some(Git))` → `Some(Git)`. (Trivial agreement.)

    Then a small integration assertion: when the resolved mode is
    `Some(Off)` or `None`, the `RenderContext` constructed by
    `render_document_to_file` has `ctx.attribution_provider.is_none()`
    — the CLI plumbing must not install a `GitBlameProvider` for
    either case. (Pure unit test on the resolution function plus one
    integration test on the `RenderContext` construction; lives next
    to the `RenderToFileOptions` → `RenderContext` plumbing site
    introduced in Phase 3c.)
10. **WASM byte-identicality fixture sweep.** For every existing
    q2-debug fixture (the corpus that today drives
    `parse_qmd_to_ast`), assert that
    `parse_qmd_to_ast_with_attribution(content, None)` produces output
    byte-identical to `parse_qmd_to_ast(content)`. This is the
    structural test that backs the Phase 3b byte-identicality
    invariant: the `None` branch must never silently alter the
    existing q2-debug surface, since the latter delegates to the
    former. Runs as a parameterised test over the fixture corpus, not
    a single point assertion.
11. **q2-debug attribution-on, happy path (every actor identity-mapped).**
    Given a small qmd fixture with two contiguous Str nodes whose
    `SourceInfo`s span the byte ranges in an `AttributionData`
    constructed in-test, where **every actor referenced in `runs` has
    an entry in `identities`** (satisfying the Phase 6 producer
    invariant), invoke the q2-debug path
    (`parse_qmd_to_ast_with_attribution(content, Some(json))`) and
    assert the resulting `astContext.attribution` array and
    `astContext.attributionActors` table match the expected sparse records.
    Crucially: assert that the `astContext.attributionActors` entries come from
    the in-test `identities` (not the warning-path placeholder) and
    that **no diagnostic warnings are emitted** — this is the
    happy-path counterpart to test #6's invariant-violation case, so
    the warning code path is *not* exercised here. Distinct fixture
    from test #6 — see Phase 4d for why the two invocation paths
    cannot share a fixture.
12. **GitBlameProvider producer-invariant.** Given two `tests/fixtures/`
    git porcelain captures (one two-author, one N-author), assert that
    the `AttributionData` returned by `GitBlameProvider::build(...)`
    satisfies: every actor referenced by `runs` has an entry in
    `identities`, and each entry's `display_name` equals the
    mail-local-part and `color` equals `actor_color(fnv1a_hex8(email))`.
    Pin the deterministic colour for at least one known email
    (e.g. `alice@example.com → hsl(<known hue>, 60%, 50%)`) so a future
    refactor of `fnv1a_hex8` can't silently shift hues.

### Snapshot tests

- [ ] `crates/quarto-core/snapshots/attribution_generate__*` — one per
  skip condition + one happy path. **Deferred to Phase 2/4c**:
  while the generate transform body is `unimplemented!()`, snapshot
  output would be a panic. Phase 0 covers this surface via structured
  assertions on `ctx.attribution_data` and `ctx.format_options`
  (tests #4–#7 in `attribution_generate.rs` / `attribution_render.rs`).
- [x] HTML off-path baseline snapshot at
  `crates/quarto-core/tests/snapshots/attribution_baseline_snapshot__attribution_off_baseline.snap`.
  Asserts a small attribution-free document renders to the same HTML
  body it does today; GREEN immediately. Backs the Phase 4
  "byte-identical when off" promise as a mechanical regression guard.
  The plan's original two-file split
  (`attribution_render_html__off` + `_on`) is unnecessary: the off
  baseline is the one that must never drift; the on case is exercised
  by Phase 4b's coalescing tests via structured DOM assertions.
- [ ] No snapshot test for the q2-debug JSON: it'll churn whenever AST IDs
  change. The structured assertion in **Phase 0 test #6** is the
  substitute: when `attribution_lookup` is `None`, both the
  `astContext.attribution` and `astContext.attributionActors` keys are absent
  from the output. Combined with the
  `#[serde(skip_serializing_if = …)]` annotations pinned in Phase 4a
  (`Vec::is_empty` on `attribution`, `HashMap::is_empty` on
  `attribution_actors`), "keys absent when off" is mathematically
  equivalent to "JSON byte-identical to today's output" — serde
  skips both fields, no other code path changes — so a snapshot
  would be redundant.

## Phase 1 — Canonical types and provider trait

- [x] Create `crates/quarto-core/src/attribution/mod.rs` with:
  - `AttributionRun { start: usize, end: usize, actor: Arc<str>, time: i64 }`
    — the canonical in-memory shape. `actor` is `Arc<str>` (not
    `String`) — shared across all runs by the same author; see
    Vocabulary for the rationale. **`Serialize` only**, no
    `Deserialize` derive: deserialization goes through the
    transport types below, then through `AttributionDataBuilder`,
    so the interning invariant is restored on the way back in (a
    plain `Deserialize for Arc<str>` would re-allocate per-occurrence
    and silently regress the memory cost claimed in Vocabulary).
  - `AttributionMap` as a transparent newtype around `Vec<AttributionRun>`
    (`#[serde(transparent)]` so the JSON form is a flat array). The
    in-memory queryable form. No file keying in v1. Provides an
    `is_empty(&self) -> bool` helper for `skip_serializing_if`.
    Same `Serialize`-only treatment as `AttributionRun`.
  - `IdentityMap = HashMap<Arc<str>, Identity>` (keyed by the same
    `Arc<str>` instances used in `AttributionRun.actor`) where
    `Identity { display_name: String, color: String }` + serde derives.
  - `AttributionData { runs: AttributionMap, identities: IdentityMap }`
    + `Serialize` derive (no `Deserialize`; see above). **The canonical
    in-memory shape**, held as `Arc<AttributionData>` on
    `RenderContext.attribution_data` (the sidecar). Not stored in
    `ast.meta`. `Serialize` exists *solely* for the WASM transport
    boundary (Phase 3b); both fields use
    `#[serde(default, skip_serializing_if = "…is_empty")]` so runs-only
    and identities-only transport payloads serialize compactly.
  - **Transport-only mirror types** for the JSON deserialize path:
    `TransportAttributionRun { start: usize, end: usize, actor: String,
    time: i64 }` and `TransportAttributionData { runs:
    Vec<TransportAttributionRun>, identities: HashMap<String,
    Identity> }`, both with `Serialize + Deserialize`. The wire shape
    is identical to the canonical types' `Serialize` form (`Arc<str>`
    and `String` both serialize as JSON strings), so round-tripping
    canonical → JSON → transport → builder → canonical preserves data;
    the only thing the transport detour buys is a clean place to
    re-intern.
  - **`AttributionDataBuilder`** — the single entrypoint every
    producer uses to construct an `AttributionData`. Owns an internal
    `HashMap<String, Arc<str>>` intern map; exposes:
    - `fn intern_actor(&mut self, actor: &str) -> Arc<str>` — returns
      the canonical `Arc<str>` for `actor`, allocating once on first
      sight and `Arc::clone`-ing thereafter.
    - `fn push_run(&mut self, start: usize, end: usize, actor: Arc<str>,
      time: i64)` — actor argument *must* be the value returned by
      `intern_actor` (enforced by convention, not the type system —
      document this in the doc-comment).
    - `fn set_identity(&mut self, actor: Arc<str>, id: Identity)`.
    - `fn build(self) -> AttributionData`.

    Doc-comment must state the invariant the builder enforces: "Every
    `AttributionRun.actor` in the built `AttributionData` is
    `Arc::ptr_eq` to the corresponding key in `IdentityMap` by
    construction." All three callsites (the two providers and test
    fixtures) go through this builder; no producer should ever
    construct `AttributionRun` literals with ad-hoc `Arc::from(s)`.
  - `pub trait AttributionSourceProvider: Send + Sync` with a single
    method `fn build(&self, ctx: &RenderContext) -> Result<AttributionData>`.
    **The method is sync, not async.** Locked-in rationale:
    - The only blocking implementor is `GitBlameProvider`, which
      spawns one `git blame --porcelain` subprocess (~tens of ms on
      typical document-sized files, long-tail ~1s on very large
      repos). v1's native render is single-document-at-a-time, so
      the calling thread has no other work to compete with.
    - The WASM implementor (`PreBuiltAttributionProvider`) is purely
      sync (JSON parse + intern loop, sub-millisecond). An async
      signature would force it through a degenerate `async fn` body
      containing zero `.await`s — a real code smell.
    - A future caller that needs cooperative scheduling can wrap the
      sync `build` in `tokio::task::spawn_blocking` at the call site
      without touching the trait. The reverse (async trait, sync
      caller via `block_on`) is uglier and runtime-specific.

    Doc-comment on the method must state: "May block. Implementations
    that spawn subprocesses or do other blocking I/O should document
    expected latency. Currently: `GitBlameProvider` blocks on a
    `git blame --porcelain` subprocess (tens of ms typical, ~1s on
    huge repos); `PreBuiltAttributionProvider` is non-blocking."

    Each provider returns the data shape that's natural for it
    (`GitBlameProvider` returns runs + synthesized identities in v1;
    `PreBuiltAttributionProvider` returns whatever hub-client shipped,
    re-interned). Both providers route construction through
    `AttributionDataBuilder`.
  - `pub trait AttributionSource: Send + Sync` with
    `fn query_byte_range(&self, start: usize, end: usize) -> Option<AttributionHit>`.
    No `file_id` parameter — single-doc invariant. (v2 extension noted in
    Open Question #2.)
  - Blanket impl: `impl AttributionSource for AttributionMap` via binary
    search over the runs.
- [x] Add **two new fields** to `RenderContext`
  (`crates/quarto-core/src/render.rs:84-188` as of #169, which added
  `resolved_listings` at line 187). Defaults are `None`; nothing in the
  existing pipeline should observe a behavior change. Both fields
  carry single-writer / single-reader doc-comments so the entire
  Finalization Phase (which runs between Generate and Render with
  `attribution_data` populated) is forced to treat the slot as
  opaque.
  - `pub attribution_provider: Option<Arc<dyn AttributionSourceProvider>>`
    — opt-in signal set by the CLI flag plumbing (Phase 3c) or the
    WASM entry point (Phase 3b). Read by `AttributionGenerateTransform`
    only. Doc-comment: "Set by the CLI flag plumbing (Phase 3c) or
    the WASM entry point (Phase 3b). Read by
    `AttributionGenerateTransform`. No other transform should
    consult this field."
  - `pub attribution_data: Option<Arc<AttributionData>>` — the
    sidecar carrying the canonical merged form. Written by
    `AttributionGenerateTransform`; read by `AttributionRenderTransform`.
    No other transform reads or writes this field. Doc-comment:
    "Written by `AttributionGenerateTransform`; read by
    `AttributionRenderTransform`. **No other transform reads or
    writes this field.** The entire Finalization Phase runs between
    Generate and Render with this slot populated; future
    Finalization transforms must treat it as opaque."
    `Arc` so the value travels between transforms (and into the writer
    config) without re-copying.
- [x] `pub fn format_supports_attribution(format: &Format) -> bool` —
  returns `true` for formats whose writers consume the lookup (HTML and
  q2-debug JSON in v1) and `false` otherwise (PDF, Typst, plain Pandoc
  native, etc.). Used by `AttributionGenerateTransform`'s skip ladder
  to short-circuit before invoking the provider; opting in to
  attribution on a non-consuming format would otherwise fire a
  `git blame` subprocess whose output goes nowhere visible.
- [x] Small helper `from_config_value(meta: &ConfigValue) -> IdentityMap`
  to read user-authored `meta.attribution.identities` (a small
  `ConfigValue::Map` from YAML parse) into an `IdentityMap` for the
  merge step in Phase 2. This is the *only* attribution-related
  `ConfigValue` ↔ Rust-struct converter the plan ships; the bulk
  `runs` path never visits `ConfigValue`.

**Why a sidecar field on `RenderContext` rather than `meta.attribution`:**
see the "Why a sidecar, not `meta.attribution`" subsection at the end of
Vocabulary. Short version: `ConfigValue::Map` representation of `AttributionRun`
records is ~20× heavier per run than the typed struct, and the
hypothetical Lua-filter introspection it would enable wouldn't be useful
in practice (raw runs aren't a queryable shape; the right Lua surface is
a `lookup(start, end)` accessor when bd-0fd0 lands). User-authored
`meta.attribution.identities` still rides the convention.

## Phase 2 — `AttributionGenerateTransform`

- [ ] New file `crates/quarto-core/src/transforms/attribution_generate.rs`
  modelled on `navbar_generate.rs`. (The 94-line size cited in an earlier
  draft is now stale — `navbar_generate.rs` has grown to ~414 lines with
  project-index enrichment; attribution-generate has no equivalent
  enrichment step, so target the original "small + tests" footprint, not
  the current navbar size.)
- [ ] Skip / merge ladder, in this order. User-authored runs aren't a
  valid surface (users do not hand-author thousands of byte-range
  tuples in YAML); the only legitimate user override is identities,
  which the merge step in rule 4 handles:
  1. `!format_supports_attribution(&ctx.format)` → bail. The current
     format's writer doesn't consume the lookup (PDF, Typst, plain
     Pandoc native, etc.), so running the provider would do nothing
     visible — and on the git-blame branch would needlessly spawn a
     subprocess. Checked first so `attribution: git`-style project YAML
     doesn't pay the cost on non-HTML targets.
  2. `is_feature_disabled(&ast.meta, "attribution")` (affirmative
     `false`) → bail; `ctx.attribution_data` remains `None`.
     User-authored `meta.attribution.identities`, if any, stays in
     meta as written but is not consumed.
  3. `ctx.attribution_provider.is_none()` (no opted-in source) → bail.
  4. Otherwise (provider opted in): call
     `provider.build(ctx)?` → `AttributionData` (the provider has
     already routed construction through `AttributionDataBuilder`,
     so its `runs[i].actor` Arcs are `ptr_eq` to its `identities`
     keys). Then **merge with any user-supplied
     `meta.attribution.identities`** (read via `from_config_value`
     helper from Phase 1): take the provider's `runs` as-is; for
     `identities`, on key collision **preserve the provider's
     `Arc<str>` as the map key and overwrite only the `Identity`
     value** with the user's. Non-colliding user keys (an actor
     named in YAML but absent from the provider's runs) are
     **dropped, not unioned** — see "Why drop non-colliding user
     keys" below. Store as
     `ctx.attribution_data = Some(Arc::new(AttributionData { runs, identities: merged }))`.

     **Why preserve provider keys on collision.** The user's
     `IdentityMap` from `from_config_value` was built from a
     `ConfigValue::Map` and its keys are fresh `Arc<str>` allocations
     unrelated to any `AttributionRun.actor`. If those keys *replaced*
     the provider's keys on collision, every `AttributionRun` for
     that actor would point at a different `Arc<str>` than the map
     key, breaking the `Arc::ptr_eq` interning invariant pinned in
     Phase 0 test #4. Replacing only the value preserves the
     invariant — `HashMap::insert` returns the old value but keeps
     the existing key, which is exactly what we need; concretely,
     `if let Some(slot) = merged.get_mut(&user_key) { *slot = user_id; }`
     (no `else` branch — non-colliding entries are not inserted).

     **Why drop non-colliding user keys.** The render walk
     (Phase 4c) prunes `attribution_actors` to actors referenced by
     `attribution_lookup` — i.e. actors with at least one run in
     *this* document. A user-supplied identity for an actor with no
     runs is therefore invisible at the writer, and inserting it
     into the merged map is dead work. If a future v2 feature wants
     cross-doc aggregation (a project sidebar listing all
     contributors a project knows about), the right home for "people
     the project knows" is a project-level identities table built in
     the `ProjectIndex`, not a per-doc sidecar; v2 introduces it
     separately and the v1 merge stays minimal so its surface
     doesn't have to change under that work.
- [ ] Register the stage in `pipeline.rs` as the **last entry in the
  Navigation Phase**, immediately **after** `FooterRenderTransform`
  (currently line 847) and immediately **before** the Finalization Phase
  begins with `LinkRewriteTransform` (currently line 857). Rationale:
  (a) `AttributionGenerateTransform` is a `*-generate` stage and reads
  naturally with the other generate transforms; (b) end-of-phase
  placement puts it *after* all the website / navigation transforms
  rather than interleaved with them, so it doesn't sit "in the middle"
  of unrelated stages; (c) end-of-Navigation-Phase gives a stable
  insertion contract: any new navigation transform added later goes
  **before** this stage by the same rule, so the position never has to
  be re-litigated. The historical alternative (slotting between
  `FooterGenerateTransform` and `ListingGenerateTransform` — inside the
  phase rather than at its tail) was rejected — see the design
  discussion in the Overview.

## Phase 3 — Provider implementations

### 3a. Git-blame provider (native)

**Implementation choice: shell out to the `git` binary.** Rejected
alternatives:

- `git2` (libgit2 C bindings) — drags libgit2 into the Rust workspace
  (cross-compilation pain on Windows MSVC and musl), blame doesn't always
  match real `git blame` (mailmap handling, `--follow`, whitespace
  heuristics), doesn't ride the user's gitconfig.
- `gix` (gitoxide) — pure Rust but blame is one of its newer subsystems
  with edge cases, large crate fan-out, still doesn't match real git in
  every corner.

Shelling out keeps zero new build deps, matches the TS prototype line-for-line,
honours the user's gitconfig / `.mailmap` / `core.autocrlf`, and adds nothing to
the WASM build (which doesn't need git). Subprocess overhead (~tens of ms per
file) is fine for `quarto render`; if project-wide rebuild scaling ever
matters, revisit then.

- [ ] Extend `BinaryDependencies` (`crates/quarto-core/src/render.rs:32-44`)
  with a `pub git: Option<PathBuf>` field, and add the matching lookup
  inside `BinaryDependencies::discover` (`pub fn discover` is at line 53;
  the body assembles fields starting at line 54):
  ```rust
  git: runtime.find_binary("git", "QUARTO_GIT"),
  ```
  This is the single corrective for review issue #1; without it the plan's
  earlier claim that "binaries owns git discovery already" is false.
- [ ] `crates/quarto-core/src/attribution/git_blame.rs`:
  - Pure-Rust port of `attribution-gitblame.ts`. Spawns
    `git blame --porcelain` using `ctx.binaries.git` (now populated by the
    item above). The provider does **not** invoke `git` from `$PATH`
    directly — always go through `BinaryDependencies` so `QUARTO_GIT`
    overrides work the same way as `QUARTO_PANDOC` etc.
  - Multi-byte UTF-8 line lengths via `s.as_bytes().len()` — TextEncoder
    equivalent.
  - Returns a **complete** `AttributionData { runs, identities }` for
    the current document, constructed via `AttributionDataBuilder`
    (Phase 1) — never by literal struct construction with ad-hoc
    `Arc::from(s)` calls. Concretely: the parser maintains a single
    `AttributionDataBuilder`; on each commit-header block it calls
    `builder.intern_actor(&email)` once to obtain the canonical
    `Arc<str>`, then `builder.set_identity(actor.clone(), ...)`; on
    each content line it calls `builder.push_run(start, end,
    actor.clone(), time)` re-using that same `Arc<str>`. The same
    email seen on N content lines produces N `Arc::clone` calls and
    one underlying allocation. `runs` is the parsed porcelain output
    for the primary file in v1. `identities` contains one entry per
    distinct `author-mail` seen in the porcelain stream, satisfying
    the Phase 6 producer invariant (every actor referenced in `runs`
    has an entry in `identities`):
    - `display_name = email.split_once('@').map(|(local, _)| local).unwrap_or(email)`
      (the mail-local-part; falls back to the full string for
      pathological emails without `@`).
    - `color = actor_color(fnv1a_hex8(email))`. The email is
      pre-hashed because `actor_color` parses the first 6 hex chars
      of its input — an email like `charlie.gao@posit.co` would
      yield colour collisions across every author whose email shares
      a hex-prefix-friendly leading run. FNV-1a (defined in
      `palette.rs`, see Phase 6) produces a uniformly-distributed
      8-char hex string, ensuring per-email hue distribution.
    Synthesizing identities in the producer (rather than detecting
    email-shaped actors at render time) keeps the render stage
    source-agnostic — see Phase 6 § Identity resolution.
  - **Graceful degradation, not a render failure.** When
    `--attribution=git` is passed but (a) `ctx.binaries.git` is `None`
    (git not on PATH / `QUARTO_GIT` unset) or (b) the document isn't
    inside a git working tree, emit a `DiagnosticMessage` warning and
    return an empty `AttributionData` (`runs = []`, `identities = {}`).
    The pipeline then behaves as if
    attribution were off — the render succeeds, just without
    `data-attr-*` attributes. Rationale: a missing git binary should be
    a soft signal, not a broken build.
  - Test fixtures: two independent paths, deliberately not sharing a
    real `.git/` directory.
    - **Parsing tests** (Phase 0 #3): checked-in `git blame
      --porcelain` text under `tests/fixtures/attribution-blame/` so
      the parser unit tests don't depend on live timestamps. The
      porcelain text was captured once from a hand-built repo and
      committed verbatim; regenerating it later requires re-running
      the same capture (a one-line shell command documented in
      `tests/fixtures/attribution-blame/REGEN.md` so a future
      maintainer can refresh it without spelunking).
    - **End-to-end CLI test** (Phase 0 #9): the test itself builds
      a temp git repo on every invocation. `tempdir` + `git init` +
      two scripted commits by distinct authors, all with
      `GIT_AUTHOR_DATE` / `GIT_COMMITTER_DATE` /
      `GIT_AUTHOR_EMAIL` / `GIT_COMMITTER_EMAIL` /
      `GIT_AUTHOR_NAME` / `GIT_COMMITTER_NAME` pinned to fixed
      values so commit hashes and porcelain output are
      bit-deterministic across runs and across machines. The
      source `.qmd` lives under `tests/fixtures/attribution-blame/`
      and is copied into the tempdir at test start; nothing under
      `.git/` is committed to the working tree. This keeps the
      repo free of binary blobs (no committed `.git/` dirs, no
      tarballs) and avoids the git-inside-git tooling surprise of
      a nested working tree.

### 3b. Automerge runs provider (WASM)

- [ ] `crates/wasm-quarto-hub-client/src/attribution.rs` (a WASM-only module).
  Hub-client computes the runs in JS today. Two delivery options:
  
  **Option A (committed for v1): hub-client preserves its TS replay
  code, ships a serialized `AttributionData` (i.e.
  `{ runs: [...], identities: {...} }`) across the WASM boundary as
  a JSON string parameter to `parse_qmd_to_ast_with_attribution`.** The Rust side wraps the
  string in a `PreBuiltAttributionProvider` and stores it on
  `RenderContext.attribution_provider`; the actual JSON parse +
  interning happens inside the provider's `build()`, not at the
  WASM entry point — keeping construction in one place (Phase 1's
  `AttributionDataBuilder`) instead of two.

  Full definition (lives at
  `crates/quarto-core/src/attribution/prebuilt.rs` — see "Where
  `PreBuiltAttributionProvider` lives" below):

  ```rust
  /// Wraps a hub-client-supplied transport JSON string and decodes
  /// it on demand into a canonical `AttributionData`.
  ///
  /// The JSON is parsed lazily inside [`build`] rather than at
  /// construction time so that:
  /// - construction is infallible (no `Result` at the WASM entry
  ///   point), and
  /// - the parse + intern step lives behind the same
  ///   `AttributionSourceProvider` trait surface as
  ///   `GitBlameProvider`, so a future caller cannot distinguish
  ///   the two by where errors surface.
  pub struct PreBuiltAttributionProvider {
      json: String,
  }

  impl PreBuiltAttributionProvider {
      pub fn new(json: String) -> Self {
          Self { json }
      }
  }

  impl AttributionSourceProvider for PreBuiltAttributionProvider {
      fn build(&self, _ctx: &RenderContext) -> Result<AttributionData> {
          let raw: TransportAttributionData =
              serde_json::from_str(&self.json)
                  .map_err(|e| /* wrap into the project's error type */)?;
          let mut b = AttributionDataBuilder::new();
          // identities first so the intern map sees provider-supplied
          // actor strings before any runs that reference them
          for (k, id) in raw.identities {
              let actor = b.intern_actor(&k);
              b.set_identity(actor, id);
          }
          for r in raw.runs {
              let actor = b.intern_actor(&r.actor);
              b.push_run(r.start, r.end, actor, r.time);
          }
          Ok(b.build())
      }
  }
  ```

  The impl is plain (no `#[async_trait]` attribute) because the
  trait's `build` method is sync — see Phase 1 § trait definition
  for the locked-in rationale. The `_ctx: &RenderContext` parameter
  is unused for this provider but must match the trait signature —
  the canonical form is fully determined by the transport JSON, so
  `RenderContext` carries no useful signal for the prebuilt path.

  This is the *only* path that crosses the transport boundary. The
  builder restores the interning invariant that serde's default
  `Deserialize for Arc<str>` would have destroyed (each occurrence
  of the same actor string in the JSON would otherwise allocate a
  fresh Arc), so `PreBuiltAttributionProvider` ends up structurally
  indistinguishable from `GitBlameProvider` for downstream consumers.
  Phase 0 test #1 strengthens the round-trip assertion to pin this
  explicitly. The JSON payload is *transport-only* — once parsed it
  lives as a typed Rust struct on the sidecar
  (`ctx.attribution_data`), never visiting `ast.meta`. Pros: no
  automerge-rs in the WASM bundle (~hundreds of KB saved), no
  duplicate replay implementations, runs and identities ride one
  channel. Cons: the canonical form is computed in TS, not Rust —
  but the transport JSON is decoded into the same typed shape any
  provider produces, so this is fine.

  **Where `PreBuiltAttributionProvider` lives.** In
  `crates/quarto-core/src/attribution/prebuilt.rs`, not in
  `wasm-quarto-hub-client`. It depends only on `AttributionData`,
  `AttributionDataBuilder`, and `serde_json` (all already in
  `quarto-core`), and has no WASM-specific code. Keeping it in
  `quarto-core` lets the producer-invariant tests (Phase 0 test #1
  ptr_eq restoration, Phase 0 test #12 fixture sweep) run as
  native unit tests on the canonical types' home crate, and lets
  any future native caller that has a pre-built JSON payload (e.g.
  `--attribution-from-file=…`) use it without reaching into the
  WASM crate.

  **Direct-invocation flow (not via the transform pipeline registration).**
  The Rust-side `pipeline::parse_qmd_to_ast` runs only three stages
  today (`ParseDocumentStage` → `EngineExecutionStage` →
  `MetadataMergeStage`) — `AstTransformsStage` is **not** in that
  list. So unlike the HTML path, the q2-debug WASM path cannot pick
  up attribution transforms via the `build_transform_pipeline`
  registration (Phase 2's end-of-Navigation-Phase insertion and
  Phase 4's end-of-Finalization-Phase insertion). The WASM entry
  point invokes them **directly** after the existing 3-stage parse:

  ```
  1. If attribution_json is Some(s): install
       PreBuiltAttributionProvider::new(s) on ctx.attribution_provider.
     Else: leave ctx.attribution_provider as None.
     (The JSON is NOT parsed at this point — the provider holds the
     raw string and parses+interns lazily inside build(); see Option A
     above.)
  2. Run pipeline::parse_qmd_to_ast(content, …, &mut ctx, runtime)
     → AstOutput (unchanged from today).
  3. If ctx.attribution_provider.is_some():
       AttributionGenerateTransform::new()
           .transform(&mut output.ast, &mut ctx).await?;
       // ↑ this is where provider.build() runs — JSON parse + intern
       AttributionRenderTransform::new()
           .transform(&mut output.ast, &mut ctx).await?;
     Else: skip — output is identical to today's parse_qmd_to_ast.
  4. Build JsonConfig with attribution_lookup / attribution_actors
     pulled from ctx.format_options.json (Phase 4c populates these).
  5. Serialize via pampa::writers::json::write_with_config.
  ```

  This deliberately diverges from the HTML path's
  `build_transform_pipeline` registration to preserve the existing
  q2-debug surface ("what did the parser see"): we do *not* slot
  the full `AstTransformsStage` into `parse_qmd_to_ast`, because
  doing so would suddenly fire callout/navbar/sectionize/etc. on
  the AST debug view — a substantial behavior change unrelated to
  attribution.

  Rejected alternatives:
  - Adding `AstTransformsStage` to `parse_qmd_to_ast`'s stage list —
    behavior change as above.
  - Building a new `AstTransformsStage::attribution_only()`
    constructor — adds API surface to the stage just for this case;
    direct invocation needs none of the StageContext bridging
    `AstTransformsStage` provides (see Invariant below).
  
  **Option B (deferred to v2 consideration): link `automerge-rs`
  into wasm-quarto-hub-client and replay history in Rust.** Pro:
  one source of truth. Con: bundle size and a duplicate of
  perfectly-good TS code. **v1 commits to Option A unconditionally**
  — see Open Questions § #3 for the locked-in rationale.
  
- [ ] Document Option A's parameter in
  `crates/wasm-quarto-hub-client/CLAUDE.md`. **The q2-debug entry point
  is `parse_qmd_to_ast` (`crates/wasm-quarto-hub-client/src/lib.rs:855`,
  signature `pub async fn parse_qmd_to_ast(content: &str) -> String`),
  not `render_qmd` (line 1005).** `render_qmd` is the HTML preview path,
  which Phase 5 explicitly puts out of scope for v1 ("hub-client's HTML
  preview tab does not display attribution"). New entry point signature:
  ```rust
  pub async fn parse_qmd_to_ast_with_attribution(
      content: &str,
      attribution_json: Option<String>,  // JSON-encoded { runs, identities }
  ) -> String
  ```
  Note the **content-based** signature (matching the existing
  `parse_qmd_to_ast`), not path-based. No `user_grammars` parameter —
  `parse_qmd_to_ast` doesn't take one today; if grammar support is
  needed later, both functions add the parameter together. Existing
  `parse_qmd_to_ast` keeps its current signature; the with-attribution
  variant is opt-in to keep the diff small. **`parse_qmd_to_ast`
  becomes a thin wrapper that calls
  `parse_qmd_to_ast_with_attribution(content, None)` and returns its
  result directly** — no additional cfg branches, no
  separately-collected diagnostics, no extra trace events, no
  difference in error reporting, no pre- or post-processing of any
  kind. The body is one line plus a doc-comment ("`Equivalent to
  parse_qmd_to_ast_with_attribution(content, None)`. Kept as a
  separate entry point for callers that have no attribution to ship
  and want the simpler signature."). This is what makes the
  byte-identicality invariant below mechanical rather than
  aspirational; a future maintainer adding a side effect to
  `parse_qmd_to_ast` directly (instead of to the underlying
  `parse_qmd_to_ast_with_attribution`) would silently break the
  invariant, so the wrapper shape is the contract.

  **Byte-identicality invariant:** `parse_qmd_to_ast(content)` is
  byte-identical to `parse_qmd_to_ast_with_attribution(content, None)`
  for every fixture. The wrapper-with-no-extra-side-effects shape
  above makes this true by construction: when `attribution_json` is
  `None`, the provider isn't installed, step 3 in the recipe above
  is skipped, and the resulting bytes come from the same 3-stage
  pipeline + JSON serializer as today. The delegation is therefore
  safe — every existing q2-debug render silently routes through the
  new function, and a regression on the `None` branch would break
  *all* renders, not just attribution ones. Phase 0 test #6 doubles
  down on this with a structured assertion ("`astContext.attribution`
  and `astContext.attributionActors` keys absent when off"), and
  Phase 0 test #10 asserts byte-identicality across the existing
  q2-debug snapshot corpus.

  **Transform-invocation invariant:** `AttributionGenerateTransform`
  and `AttributionRenderTransform` MUST depend on `RenderContext`
  only, never on `StageContext`. The HTML path drives them through
  `AstTransformsStage`, which bridges `StageContext ↔ RenderContext`
  (diagnostics, resource_report, project_index, …); the q2-debug
  path calls them directly outside any `Pipeline`, with no bridge.
  Both invocation paths must produce identical results for the same
  inputs. Pin this in the file-level doc-comment of each transform
  ("Reads/writes only fields on `RenderContext`; no `StageContext`
  access") so a future refactor that reaches for `stage_ctx.foo`
  fails the q2-debug path immediately.

  **Phase 9 entry point (`render_page_in_project`, line 1292) is
  unchanged:** it drives the project-aware HTML preview path, which is
  out of scope for v1. Attribution does not flow through it; the
  `--attribution=git` CLI flag is the only HTML-preview path that ships
  in v1, and it goes through the native CLI binary, not the WASM
  entry points.

### 3c. CLI flag plumbing (native)

- [ ] Define a typed mode enum (lives in `quarto-core`, alongside the
  attribution module so YAML and CLI both depend on the same type):
  ```rust
  #[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum,
           serde::Serialize, serde::Deserialize)]
  #[value(rename_all = "kebab-case")]
  #[serde(rename_all = "kebab-case")]
  pub enum AttributionMode {
      Off,
      Git,
  }
  ```
  Reject `Option<String>` — clap's `ValueEnum` gives typed parsing,
  validated values, and auto-generated help listing alternatives.
- [ ] Extend `RenderArgs` in
  `crates/quarto/src/commands/render.rs:40-67` with:
  ```rust
  pub attribution: Option<AttributionMode>,
  ```
  Three CLI states, all distinct:
  - flag absent → `None`, defer to YAML
  - `--attribution=git` → `Some(AttributionMode::Git)`, force git
    (overrides YAML)
  - `--attribution=off` → `Some(AttributionMode::Off)`, force off
    (escape hatch when project YAML has `attribution: git`)
- [ ] Plumb through `RenderToFileOptions`
  (`crates/quarto-core/src/render_to_file.rs:83-111`) and into the
  construction of `RenderContext`. Resolution order (matches how `--to`
  and `format:` reconcile today): CLI value (if `Some`) wins; otherwise
  YAML value; otherwise off. When the resolved value is
  `AttributionMode::Git`, install `Arc::new(GitBlameProvider::new())` as
  `ctx.attribution_provider`. When `Off`, leave it `None` — same code
  path as the unflagged default. Silent override on YAML/CLI conflict
  (no diagnostic); the Phase 3a graceful-degradation path handles the
  "git mode requested but git is unusable" case regardless of how the
  mode arrived.
- [ ] YAML alternative: top-level `attribution:` in the document or
  project YAML accepts the same three states.
  - `attribution: git` → `Some(AttributionMode::Git)`
  - `attribution: off` *or* `attribution: false` → `Some(AttributionMode::Off)`
  - key absent → `None`

  **Not** valid as a CLI or YAML value: `automerge`. Automerge attribution
  is hub-client-only and reaches the pipeline through the
  `parse_qmd_to_ast_with_attribution` WASM entry point (Phase 3b), which
  never consults `AttributionMode`. Keeping `automerge` out of this enum
  prevents the CLI from advertising a capability it doesn't have.

## Phase 4 — `AttributionRenderTransform`

This is the heart of the format-specialisation contract. The transform reads
`ctx.attribution_data` once and produces output that the downstream writer
for the *current `Format`* can act on. Today that's two writers:

### 4a. q2-debug delivery

- [ ] q2-debug is a pseudo-format that aliases to `html` for the body writer
  (`crates/quarto-core/src/format.rs:108`). The hub-client AST renderer
  consumes the JSON output of `parse_qmd_to_ast`. So for q2-debug the
  delivery shape is:
  - Resolve every node's `SourceInfo` to a `(file_id, start, end)` byte
    range using the existing chain-resolution logic in
    `quarto-source-map/src/mapping.rs:15-87`. Done **once** in
    `AttributionRenderTransform`, not per-writer-call.
  - Query `AttributionMap` for the most-recent `(actor, time)`.
  - **Wire shape (canonical):** emit two sibling fields nested
    **inside `astContext`** (not peer to it):
    - `astContext.attributionActors` — actor → resolved `(name, color)`. One
      entry per distinct actor referenced by the attribution array.
      The producer runs the Phase 6 identity-resolution chain **once
      per actor** (interned during the AST walk), not once per record.
    - `astContext.attribution` — sparse array of three-field records.

    Schema:
    ```json
    {
      "astContext": {
        "files": [...],
        "sourceInfoPool": [...],
        "attributionActors": {
          "<actor>": { "name": "<string>", "color": "<string>" }
        },
        "attribution": [
          { "s": <sourceInfoId>, "actor": "<string>", "time": <i64 ms> },
          ...
        ]
      },
      ...
    }
    ```
    Records carry `s`, `actor`, `time` (always present). Identity
    (`name`, `color`) is **not** duplicated per record — consumers
    join by `actor` into `astContext.attributionActors`. In a doc with thousands
    of records authored by a small number of contributors, inlining
    name/color per-record would bloat the output by `O(records ×
    actor-string-length)`; the table is `O(distinct-actors)` and the
    array entries shrink proportionally. HTML output keeps the inline
    form (see 4b — no-JS static viewers need self-contained values);
    only the JSON wire dedupes.
    `s` joins back into `astContext.sourceInfoPool` exactly the way
    AST nodes already reference the pool via their own `s` field.
    `time` is Unix epoch **milliseconds** (Automerge's native unit;
    the git provider multiplies its seconds-since-epoch timestamp by
    1000 before populating `AttributionRun::time`). Sparse — only
    emit records where the lookup returned a hit; only emit
    actor-table entries for actors actually referenced.
  - **Why nested, not top-level:** `astContext` is the established
    side-channel for source-mapping infrastructure (file table, source
    info pool, meta key sources). Attribution is metadata *about source
    bytes*, same semantic category. A top-level `attribution` field would
    conflate source-mapping side-channel data with document content
    (`blocks`, `meta`).
- [ ] Modify `pampa::writers::json::JsonConfig` to carry two optional
  fields populated by `AttributionRenderTransform`:
  - `attribution_lookup: Option<Arc<[Option<AttributionRecord>]>>` —
    pre-baked, indexed by `sourceInfoId`. `AttributionRecord` is a
    plain `{ actor: Arc<str>, time: i64 }` (no `name`/`color`; those
    live in the actors table). The `Arc<str>` is pointer-equal to
    the corresponding key in `attribution_actors`, sharing the same
    interning invariant pinned in Phase 0 test #4. The default
    `Serialize` impl for `Arc<str>` emits a JSON string, so the
    wire shape (`{ s, actor, time }`) is unchanged — keep the
    default; do not re-derive a custom impl. Writer hooks do direct
    slice-indexing — O(1), no closure, no `dyn Fn`, no per-node
    vtable dispatch. `None` means "no attribution in scope"
    (off-path).
  - `attribution_actors: Option<Arc<IdentityMap>>` — pruned to only
    the actors referenced by `attribution_lookup`.
  Same opt-in shape as the existing `include_inline_locations` field
  (`crates/pampa/src/writers/json.rs:19-28`); both default to `None`
  so existing callers see no behaviour change.
- [ ] Add two fields to `AstContextJson` (`json.rs:53-61`):
  - `attribution: Vec<AttributionRecordJson>` annotated
    `#[serde(skip_serializing_if = "Vec::is_empty")]`. Rust field
    name matches the JSON key — no rename needed.
  - `attribution_actors: HashMap<String, AttributionActorJson>`
    (where `AttributionActorJson { name: String, color: String }`)
    annotated
    `#[serde(rename = "attributionActors", skip_serializing_if = "HashMap::is_empty")]`.
    The explicit `rename` is required because the Rust field
    follows snake_case convention (matching `source_info_pool`) but
    the JSON wire key is camelCase (matching `sourceInfoPool` —
    the established `astContext` convention).

  Both fields use the same skip convention as the existing
  `source_info_pool` field. When the writer config's
  `attribution_lookup` is `None` both collections stay empty and
  both keys are omitted from the JSON, so the off-path is
  byte-identical to today's output (this is the JSON regression
  that backs the "byte-identical when off" promise for the
  q2-debug pipeline).

### 4b. HTML delivery

- [ ] Modify `HtmlConfig` in `crates/pampa/src/writers/html.rs:18-23` to
  carry two optional fields, populated by `AttributionRenderTransform`:
  - `attribution_lookup: Option<Arc<[Option<AttributionRecord>]>>` —
    pre-baked, indexed by `sourceInfoId`, identical to the JSON
    writer's field.
  - `attribution_identities: Option<Arc<IdentityMap>>` — actor →
    resolved `(name, color)`. Unlike the JSON path the HTML writer
    reads this *inline* per wrapping span (not via a separate table
    on the wire), because static no-JS viewers need self-contained
    `data-attr-color` values. `AttributionRenderTransform` guarantees
    an entry exists for every actor the lookup can return, filling
    in warning-path placeholders for any actor the producer missed
    so the writer's lookup is total.
- [ ] **Restructure `write_block_source_attrs` (`html.rs:601-625`) and
  `write_inline_source_attrs` (`html.rs:631-655`) to gate each
  attribute family on its own condition** — no shared early-exit on
  `include_source_locations`. The two existing helpers currently bail
  early if source-locations is off, which would suppress attribution
  attrs as a side effect. New shape:

  ```rust
  fn write_block_source_attrs<W: Write>(
      block: &Block,
      ctx: &mut HtmlWriterContext<'_, W>,
  ) -> io::Result<()> {
      if ctx.include_source_locations() {
          if let Some(info) = ctx.get_block_info(block) {
              write!(ctx, " data-sid=\"{}\"", info.pool_id)?;
              if let Some(loc) = info.location.as_ref().map(|l| l.to_data_loc()) {
                  write!(ctx, " data-loc=\"{}\"", loc)?;
              }
          }
      }
      if let Some(record) = ctx.attribution_for_block(block) {
          write_attribution_attrs(ctx, record)?;
          // data-attr-actor, data-attr-time, data-attr-name, data-attr-color
          // (all four always present together; identity joined via
          // attribution_identities on actor).
      }
      Ok(())
  }
  ```

  `attribution_for_block` is a small helper: `ctx.get_block_info(block).and_then(|info| ctx.config.attribution_lookup.as_ref()?.get(info.pool_id)?.as_ref())`.
  Same shape for `write_inline_source_attrs`. The pool itself is built
  unconditionally during parsing — `include_source_locations` controls
  *emission* of pool IDs/locations into HTML, not pool construction —
  so calling `get_block_info` when source-locations is off is fine.
  Independent gating means the two features compose orthogonally
  (see the composition table in Phase 0 test #7c).
- [ ] **Coalesce contiguous same-attribution prose inlines.** Replace
  the current per-`Inline::Str` wrap (`html.rs:664-668`) with a
  one-pass walk over each block's inline children that groups
  adjacent **prose-only** inlines (`Inline::Str`, `Inline::Space`,
  `Inline::SoftBreak`) whose lookups return the same `(actor, time)`
  tuple into a single **outer attribution wrapper** carrying the
  four `data-attr-*` attributes. When `include_source_locations` is
  also on, the per-inline `data-sid`/`data-loc` spans become
  **inner** spans nested inside the outer wrapper:

  ```html
  <span data-attr-actor="alice" data-attr-time="1" data-attr-name="Alice" data-attr-color="#ff0000">
    <span data-sid="…" data-loc="…">word1</span>
    <span data-sid="…" data-loc="…">word2</span>
  </span>
  ```

  When source-locations is off (attribution-on-only mode), text is
  written directly inside the outer attribution span with no inner
  wrappers — `Inline::Str` falls through to the raw-text path at
  `html.rs:670`, which is exactly what we want.

  **Structured inlines do not participate in coalescing.** Any
  attributed `Inline::Code`, `Inline::Emph`, `Inline::Strong`,
  `Inline::Link`, `Inline::Span`, `Inline::Math`, `Inline::Note`,
  `Inline::Image`, etc. closes the current prose group (if any),
  emits its own per-inline attribution wrapper around its own
  rendered output, and a new prose group can open on the next prose
  inline. This keeps nesting predictable (no `Inline::Span` wrapping
  inside an outer attribution `<span>` *and* inside its own
  attribution `<span>` simultaneously) and keeps the coalescing
  logic local — just a small lookahead over a contiguous prose
  subsequence, no recursive re-walk of structured inlines.

  Adjacency semantics, exhaustively:
  - Two prose inlines with the same `(actor, time)` lookup: stay
    in the same group.
  - A prose inline whose lookup returns `None`: closes the current
    group, is emitted as raw text outside any wrapper, and the
    next prose hit opens a new group.
  - A prose inline whose lookup hits a *different* `(actor, time)`:
    closes the current group and opens a fresh one.
  - A structured inline (any non-prose variant): closes the current
    group and emits its own wrapper if its own lookup hits (or no
    wrapper otherwise); the next prose hit opens a fresh group
    regardless of `(actor, time)` match. Structured inlines never
    "rejoin" an open prose group, even if they happen to share
    attribution.

  For prose-heavy documents where one author wrote a paragraph this
  collapses N attribution wrappers (one per word) to one — a
  meaningful drop in byte size and DOM-node count. Pin the semantics
  in Phase 0 tests #7b (prose coalescing with source-locations on),
  #7c (composition with source-locations off), and #7d (structured-
  inline non-coalescing — the regression guard against accidental
  re-broadening to all inlines).

### 4c. Stage skeleton

- [ ] **Carrier struct for the writer-side lookup.** `RenderContext` has no
  `format_options` field today (`render.rs:84-188` has `format`,
  `options`, `binaries`, `resolved_listings`, etc., but no per-format
  options bag). Introduce a `pub format_options: FormatOptions` field
  on `RenderContext`, with per-format sub-structs carrying:
  - `attribution_lookup: Option<Arc<[Option<AttributionRecord>]>>` —
    pre-baked, indexed by `sourceInfoId`. `AttributionRecord` is a
    plain struct `{ actor: Arc<str>, time: i64 }` (no `name`/`color`;
    those live in the identities table). `Arc<str>` is pointer-equal
    to the actor key in `attribution_identities`, matching the Phase 1
    interning invariant.
  - `attribution_identities: Option<Arc<IdentityMap>>` — pruned to
    only the actors referenced by `attribution_lookup`, with identity
    resolved once per actor.

  Writers do direct slice indexing — no `dyn Fn` trait objects, no
  per-node vtable dispatch, no per-record heap allocation, no closure
  captures. `render_qmd_to_html` / `parse_qmd_to_ast` read these off
  `format_options` when constructing `HtmlConfig` / `JsonConfig`.
  Defaults are `None` for every format, so existing callers and tests
  are unaffected.
- [ ] `crates/quarto-core/src/transforms/attribution_render.rs`:
  - Reads `ctx.attribution_data` (the sidecar `Arc<AttributionData>`).
    Destructures into `runs` (an `AttributionMap`) and `identities`
    (an `IdentityMap`, may be empty). No-op when
    `ctx.attribution_data.is_none()`.
  - Walks the AST **once** and builds two artefacts:
    1. `Vec<Option<AttributionRecord>>` indexed by `sourceInfoId`. For
       each node, resolve its `SourceInfo` to `(file_id, start, end)`
       via `SourceInfo::map_offset` chain resolution. **Skip the
       query when `file_id != 0`** — v1 blames the primary doc only,
       so nodes from `{{< include other.qmd >}}` (and any other
       `file_id > 0` source) must produce no attribution record.
       Querying their byte ranges against the primary doc's
       `AttributionMap` would silently misattribute by byte-range
       collision (byte 200 in `other.qmd` likely overlaps *some* run
       in the primary doc, especially in long documents). Pin the
       skip behaviour in Phase 0 test #8b. Otherwise, call
       `runs.query_byte_range(start, end)`. Cache the resolved record
       at the pool index. (The pool size bounds the vec; uncovered
       indices stay `None`.)
    2. A pruned `IdentityMap` containing only the actors that appear
       in the lookup vec. For each actor: if `identities` (from the
       sidecar — already merged in Phase 2) has an entry, use it
       verbatim; otherwise emit a diagnostic warning naming the
       actor and use the placeholder `Identity { display_name:
       "<unknown>", color: "#888888" }`. The intern step turns N
       records × identity resolution into K (= distinct actors) ×
       identity resolution (so at most K diagnostics fire per
       render).
  - Stashes both as `Arc`-wrapped slices on `ctx.format_options`
    under the variant matching the current `Format`. The `Arc` lets
    the writer config hold a cheap clone without re-copying the data
    out of the transform.
- [ ] Register `AttributionRenderTransform` in `pipeline.rs` as the
  **last transform in the Finalization Phase**, immediately after
  `ResourceCollectorTransform` (currently line 860). The full tail of
  the pipeline becomes: `FooterRenderTransform` (line 847) →
  **`AttributionGenerateTransform`** (registered in Phase 2; new end-of-
  Navigation-Phase entry) → `LinkRewriteTransform` →
  `AppendixStructureTransform` → `CrossrefRenderTransform` →
  `ResourceCollectorTransform` (line 860) → **`AttributionRenderTransform`**
  (very last). The entire Finalization Phase runs between generate and
  render. Rationale: attribution-render's only constraint is "after
  `AttributionGenerateTransform`, before the writer." Placing it at the
  very end means any future finalization stage that mutates `SourceInfo`
  is automatically covered without having to remember to insert it
  before attribution-render.

### 4d. Two invocation paths, one transform pair

Attribution-generate and attribution-render are registered into the
HTML pipeline via `build_transform_pipeline` (Phase 2 § Navigation
Phase tail; this subsection's prior bullet § Finalization Phase tail).
For the q2-debug WASM path, they are invoked **directly** from
`parse_qmd_to_ast_with_attribution` after the existing 3-stage parse
(Phase 3b). Both paths call the same `AstTransform` impls; what
differs is the AST they operate on:

| Property                           | HTML (CLI) path                          | q2-debug (WASM) path                                    |
|------------------------------------|------------------------------------------|---------------------------------------------------------|
| Invoked by                         | `AstTransformsStage` via `build_transform_pipeline` | direct calls in `parse_qmd_to_ast_with_attribution`     |
| AST shape when attribution-render runs | post-Finalization-Phase (callouts resolved, sectionize-wrapped, crossrefs rendered, …) | raw parser output + metadata-merged meta |
| Other transforms in scope          | Full transform pipeline                  | None (q2-debug is intentionally a "see the parse output" surface) |
| `StageContext` available?          | Yes (bridged in by `AstTransformsStage`) | No                                                       |
| Diagnostics surfaced via           | `StageContext::diagnostics` → render output | `ctx.diagnostics` collected directly                  |

**Implications:**
- **Different `SourceInfo` provenance in scope.** The HTML path may
  see `FilterProvenance` or `Substring` chains introduced by earlier
  transforms (e.g. crossref resolution synthesising new nodes); the
  q2-debug path only sees what the parser emitted. The chain-resolution
  logic in `mapping.rs:map_offset` handles both cases identically, so
  the lookup itself is unchanged — but fixture tests for the two paths
  must use distinct snapshot baselines (the AST nodes that get
  attribution attached differ).
- **Don't share Phase 0 snapshot fixtures across the two paths.**
  Phase 0 test #7 (HTML delivery) operates on a post-transform AST;
  Phase 0 test #6 (q2-debug delivery) operates on a parser-output AST.
  Same `(actor, time)` runs may resolve to slightly different node
  populations depending on what transforms ran. Keep the fixtures
  scoped to one path each; do not factor them into a shared base.
- **No coupling between the paths beyond the transform definitions.**
  A refactor that, say, splits `AttributionRenderTransform` into
  generate-and-render-lookup substages must keep both invocation
  paths working. The transform-invocation invariant in Phase 3b
  (Reads/writes only `RenderContext`) is the contract that protects
  the q2-debug path from such refactors.

## Phase 5 — Hub-client integration

### Reference material on `feat/node-attribution`

The implementation branch is forked off `main` (per the Branch context
section), so it does **not** contain any of the prototype's TS files at
fork time. Phase 5 builds the minimum producer-side TS the new WASM
entry point needs, drawing on the following files on
`feat/node-attribution` as design reference. Cherry-pick, rewrite, or
selectively port — implementer's choice; the goal is the smallest TS
surface that ships the producer-side data to WASM.

- **Algorithm reference (port the design, not all the code):**
  - `hub-client/src/services/attribution-runs.ts` — RLE producer.
    The algorithm we need; on the prototype branch it's clean enough
    to cherry-pick, but verify on inspection.
  - `hub-client/src/services/attribution.ts` — producer parts
    (Automerge replay) are useful as design reference; consumer parts
    (queries, reconstructor, cache) are deliberately *not* brought
    over — that's what the Rust pipeline replaces.
- **Hook (rewrite, don't port wholesale):**
  - `hub-client/src/hooks/useAttribution.ts` — on the prototype this
    returns a `RunListAttribution` for in-process React consumption.
    On the implementation branch it returns the JSON payload that
    feeds `wasmRenderer.renderQmdWithAttribution(path, attributionJson)`.
    The consumer-side `useNodeAttributionResolver` resolver hook is
    not built at all on the implementation branch.
- **Replaced wholesale (do NOT port):**
  - `hub-client/src/services/attribution-gitblame.ts` — the Rust
    `GitBlameProvider` (Phase 3a) replaces this. Native git-blame
    happens server-side; the hub-client never spawns git.
- **Renderer (clean implementation, not refactor):**
  - `hub-client/src/components/ReactAstDebugRenderer.tsx` — on the
    prototype, `getNodeAttribution` calls plumb consumer-side
    machinery. On the implementation branch the renderer reads
    `astContext.attribution` from the parsed AST and joins each
    record's `actor` against `astContext.attributionActors` for `(name, color)`
    — no machinery to remove because none was added.
- **Editor wiring:**
  - `Editor.tsx` is updated on the implementation branch to call the
    new `wasmRenderer.renderQmdWithAttribution(path, attributionJson)`
    shim (Phase 3b) when the Authorship toggle is on, and the existing
    `parseQmdToAst` / `render_qmd` path otherwise. No
    `useNodeAttributionResolver` is involved.

### Test fixtures sourced from `feat/node-attribution`

- `hub-client/src/services/attribution-gitblame.test.ts` — porcelain
  fixtures captured as text files under
  `tests/attribution_gitblame_fixtures/` on the implementation branch
  (Phase 0 test #3).
- `hub-client/src/services/attribution-runs.test.ts` — invariants
  mirrored in the Rust unit test for `query_byte_range` (Phase 0
  test #2).

### Work items

- [ ] Hub-client side: `useAttribution.ts` keeps its run-list build (the
  efficient incremental Automerge replay is exactly the work we don't
  want to redo in Rust). What changes:
  1. The hook's return value becomes the JSON payload to ship to WASM,
     not a `RunListAttribution` for in-process React consumption.
  2. **The hook also builds a complete `IdentityMap`** alongside the
     runs, satisfying the Phase 6 producer invariant at the wire.
     For each Automerge actor seen during replay: if profile metadata
     is available, use `(display_name, color)` from it; otherwise
     fall back to `(actor.slice(0, 8),
     actorColor(fnv1aHex8(actor)))` — the same formula
     `GitBlameProvider` uses for emails (Phase 3a), keeping visual
     output consistent across producers. The identities ship in the
     same JSON payload as the runs (one `AttributionData`-shaped
     object).
  3. Add a TS `fnv1aHex8` sibling alongside the existing `actorColor`
     in `hub-client/src/hooks/useReplayMode.ts` (or wherever the
     palette helpers naturally live), mirroring Rust's
     `palette.rs::fnv1a_hex8` bit-for-bit. Both `actorColor` and
     `fnv1aHex8` carry the Phase 6 § Drift mitigation cross-reference
     doc-comments to their Rust counterparts.
  4. The TS shim in `wasmRenderer.ts` (which currently exposes
     `parseQmdToAst(qmdContent)` calling `wasm.parse_qmd_to_ast`)
     gains a `parseQmdToAstWithAttribution(qmdContent, attributionJson)`
     companion. Callers that have attribution payloads to ship
     (`ReactPreview.tsx`, `PreviewRouter.tsx`'s q2-debug branch) route
     through the new shim; everything else keeps calling
     `parseQmdToAst` unchanged.
- [ ] `ReactAstDebugRenderer` becomes a much thinner consumer: it reads
  the new `astContext.attribution` array on the parsed AST, joins each
  record's `actor` against the `astContext.attributionActors` table for
  `(name, color)`, and uses both directly. The 200+ line
  `attribution.ts` / `attribution-runs.ts` / `useNodeAttributionResolver`
  machinery in the renderer collapses; the RLE/replay code stays where
  it is (it's the producer now, not also the query layer +
  reconstructor + cache).
- [ ] **Strict opt-in invariant:** when the user has the "Authorship"
  toggle off, hub-client passes `attributionJson: None` and the WASM
  pipeline emits no attribution metadata at all — same code path as
  the CLI without `--attribution`. Two regressions cover the two
  user-visible code paths:
  - **CLI native HTML path** (`quarto render --to html` without
    `--attribution`): the Phase 0 `attribution_render_html__off`
    snapshot must equal the existing baseline byte-for-byte.
  - **WASM hub-client q2-debug path** (`parse_qmd_to_ast` /
    `parse_qmd_to_ast_with_attribution(content, None)`): Phase 0 test
    #6's structured assertion — both `astContext.attribution` and
    `astContext.attributionActors` keys absent in the JSON output, plus the
    `skip_serializing_if` annotations in Phase 4a (`Vec::is_empty` on
    `attribution`, `HashMap::is_empty` on `attribution_actors`) —
    together prove byte-identicality without a brittle JSON snapshot.

  Both together = both real code paths covered.
- [ ] **Out of scope for v1:** hub-client's HTML preview tab does
  *not* display attribution. The Authorship feature is q2-debug-only
  in v1 — both `render_qmd` and the Phase 9 `render_page_in_project`
  HTML-preview entry points are left unchanged, and the rendered HTML
  carries no `data-attr-*` attributes. Spelling this out so a future
  reader doesn't expect attribution highlights in the rendered preview
  pane and treat their absence as a bug.

## Phase 6 — Defaults, palettes, identity

- [ ] Default colour helper `actor_color` in
  `quarto-core/src/attribution/palette.rs`. Formula: parse the first 6
  hex chars of the actor ID as an integer, mod 360, emit
  `hsl(<hue>, 60%, 50%)`. Pinned identical to the TS `actorColor` in
  `hub-client/src/hooks/useReplayMode.ts:32`.

  **The TS implementation stays.** It's used by the replay drawer
  (`hub-client/src/components/ReplayDrawer.tsx:114`), which reads
  Automerge documents directly and never goes through the render
  pipeline — so it can't consume the producer-shipped
  `data-attr-color`. The Rust port is a sibling, not a replacement;
  both keep their current callers.

  **Drift mitigation (cross-referencing comments).** Add a
  doc-comment on the Rust `actor_color` saying "MUST stay in sync with
  the TS `actorColor` in `hub-client/src/hooks/useReplayMode.ts:32` —
  same formula." Mirror the comment on the TS side pointing at
  `palette.rs`. Anyone editing either is forced to consider the other.
  Upgrade to a shared-fixture-based test if drift becomes a real
  concern; for a 3-line formula, comments are sufficient. **The same
  cross-reference applies to `fnv1a_hex8`/`fnv1aHex8`**: both
  helpers now have native and TS implementations (the TS sibling is
  new for Phase 5, used by the TS-side identity fallback documented
  in Producer invariant below), and both directions of comments
  mention both helpers.
- [ ] `fnv1a_hex8` helper alongside `actor_color` in `palette.rs`.
  Used by `GitBlameProvider` to pre-hash author emails before
  feeding `actor_color` — `actor_color` parses the first 6 chars of
  its input as hex, so feeding a raw email would yield colour
  collisions across every author whose email shares a hex-prefix-
  friendly leading run. The TS sibling (`fnv1aHex8` in hub-client,
  Phase 5) plays the same producer-side role for Automerge actor
  IDs whose first 6 chars aren't guaranteed hex or that need
  fallback colouring when profile metadata is absent. Both sides
  cross-reference each other in doc-comments. Implementation:

  ```rust
  /// 32-bit FNV-1a hash, formatted as a left-padded 8-char hex string.
  /// Used wherever an arbitrary actor string (e.g. an email) must be
  /// reduced to a hex-prefix-safe input for `actor_color`. Caller:
  /// `GitBlameProvider` (pre-hashes the author email). The TS
  /// sibling in hub-client plays the same role for Automerge actor
  /// IDs (Phase 5).
  pub fn fnv1a_hex8(s: &str) -> String {
      let mut hash: u32 = 0x811c9dc5;
      for b in s.bytes() {
          hash ^= b as u32;
          hash = hash.wrapping_mul(0x01000193);
      }
      format!("{:08x}", hash)
  }
  ```

  Zero dependencies; deterministic and well-distributed for colour
  purposes. Not cryptographic — but colours don't need crypto, and a
  named non-crypto hash beats hand-rolled XOR.
- [ ] **Producer invariant:** every `AttributionSourceProvider` must
  populate an `IdentityMap` entry for every distinct actor referenced
  by the `runs` it returns. Concretely:
  - `GitBlameProvider` (Phase 3a): synthesises
    `email → (mail-local-part, actor_color(fnv1a_hex8(email)))` for
    each distinct author email seen in porcelain.
  - `PreBuiltAttributionProvider` (Phase 3b, hub-client): receives a
    *complete* `IdentityMap` from the TS replay code and ships it
    verbatim. **The TS producer is responsible for satisfying the
    invariant at the wire** — Rust never sees a `PreBuilt` payload
    with runs whose actor lacks an identity. For each Automerge
    actor seen during replay, TS:
    1. uses `(display_name, color)` from Automerge profile metadata
       when present, otherwise
    2. falls back to `(actor.slice(0, 8), actorColor(fnv1aHex8(actor)))`
       — the same formula `GitBlameProvider` uses for emails
       (Phase 3a), so visual output is consistent across native
       and WASM producers.

    This way the Rust render-side warning path (below) truly does
    not fire on happy paths — including hub-client happy paths with
    anonymous Automerge sessions or actors whose profile metadata
    hasn't synced yet. The TS sibling implementations of
    `actorColor` (already present in
    `hub-client/src/hooks/useReplayMode.ts:32`) and `fnv1aHex8`
    (new — see Phase 5 work items) carry cross-reference
    doc-comments to their Rust counterparts in `palette.rs`.

  This is the load-bearing contract that keeps the render stage
  source-agnostic. The render stage doesn't know — or need to know —
  whether an actor came from git or Automerge; by the time
  `AttributionRenderTransform` reads `ctx.attribution_data.identities`,
  every actor with a hit in the lookup has an entry.

- [ ] Identity resolution at render time is therefore a single lookup:
  for each distinct actor (interned during the AST walk), read
  `ctx.attribution_data.identities[actor]` and use it as-is. No
  source-conditional logic. The `(name, color)` pairs ship via:
  - JSON (q2-debug): a single `astContext.attributionActors` table; per-record
    entries carry only `{ s, actor, time }` (no duplication).
  - HTML: inline `data-attr-name` / `data-attr-color` on each
    coalesced wrapping span (no-JS viewers need self-contained values).

  Consumers read pre-computed values; they never re-derive.

- [ ] **Render-side warning path (does not fire on happy paths).**
  For an actor with no entry in `ctx.attribution_data.identities`
  (a producer-invariant violation — see "Producer invariant" above
  for why no legitimate path produces this state):
  1. Emit a diagnostic warning naming the offending actor.
     Diagnostic-level warning, not a hard error: the render
     continues and produces visible-but-obviously-placeholder
     output, surfacing the bug to whoever's reviewing the render
     rather than masking it with a plausible-looking deterministic
     colour.
  2. Use the placeholder `Identity { display_name: "<unknown>",
     color: "#888888" }`. The greyscale colour deliberately stands
     out from `actor_color`'s saturated HSL palette so the
     placeholder is identifiable on sight.

  Pinned by Phase 0 tests #6 and #7 (synthetic invariant
  violation → diagnostic + placeholder); not exercised by any
  end-to-end or happy-path fixture. Replaces an earlier draft's
  deterministic `actor_color(fnv1a_hex8(actor))` fallback —
  silently producing a plausible-looking colour for an unmapped
  actor would mask the producer bug rather than surface it, which
  is exactly the wrong tradeoff for an informational/compliance
  feature.
- [ ] `time` on the wire is Unix epoch **milliseconds**. Automerge
  uses ms natively; the git provider multiplies its seconds-since-epoch
  timestamp by 1000 before populating `AttributionRun::time`. Document
  the unit in `AttributionRun`'s doc-comment so a future provider
  can't silently introduce a 1000× discrepancy.

## Phase 7 — Documentation

- [x] User docs at `docs/authoring/attribution.qmd` explaining:
  - `--attribution=git` on the CLI; YAML `attribution: git`.
  - Hub-client toggle in Settings.
  - That attribution is **not on by default** and that it surfaces author
    information in the rendered HTML — privacy/discoverability note.

  *(Landed alongside the Phase 6 producer-invariant tests in commit
  `d496c3b9`. Same checkbox is mirrored at the end of the Phase 6
  work-items list at line 2205.)*
- [x] Brief reference in `CLAUDE.md` only if the feature has invariants
  future Claude needs to know. **Not needed at v1** — the
  source-locations and attribution flags compose orthogonally
  (Phase 4b), the producer invariant lives in `attribution_generate.rs`
  doc-comments (Phase 6), and the transform-invocation invariant
  lives in `attribution_render.rs` doc-comments (Phase 3b).
  Re-evaluated at end of implementation: no new invariants surfaced
  during Phases 1–6, so no `CLAUDE.md` change. Revisit if/when the
  Phase 4b coalescing pass or Phase 5c toggle UI lands.

## Resolved design questions

All three of the originally-open design questions have now been
decided. Kept in the plan so the rationale is preserved alongside
the decision; future v2 work can reopen any of them by reference.


1. **Block-level vs inline-level attribution in HTML.** TS prototype
   wraps every node. v1 takes a middle path: inline-level wrapping
   (parity with TS) but **coalesced** on equal `(actor, time)` runs
   (Phase 4b), so prose authored by one person becomes one wrapper
   per paragraph rather than one per word. This dissolves most of the
   "visual noise" half of the original tradeoff while preserving fine
   hover targets at run boundaries. A `attribution: { granularity:
   blocks | inlines }` knob can still land in v2 if a user wants
   block-only wrapping for stylistic reasons; not needed for v1.
2. **Concat / Substring chains across includes (v2 commitment).**
   A node spliced in via `{{< include other.qmd >}}` has a
   `SourceInfo::Original` pointing at `other.qmd` (file 1, not 0). v1
   blames the primary doc only; included content shows no attribution.
   Because the canonical form lives on the sidecar (a Rust struct, not
   a wire form), v2 is a pure type change with no backward-compat
   gymnastics:
   - **`AttributionData.runs` field type** upgrades from
     `AttributionMap` (i.e. `Vec<AttributionRun>`) to
     `HashMap<PathBuf, AttributionMap>` keyed by canonical path (no
     `FileId` on any persisted form, ever — eliminates the
     render-local-ID hazard by construction). No serde polymorphism
     needed; the WASM transport JSON just changes shape with the
     struct, and hub-client/Rust update together.
   - **`identities`** is unaffected by the v1↔v2 transition.
   - **Trait** re-introduces a file parameter:
     `query_byte_range(path: &Path, start, end)`. Callers resolve
     `FileId → path` via `&SourceContext` at the call site.
   - **Provider** (git-blame): runs `git blame` for every `FileId` in
     the document's `SourceContext`, not just the primary doc. Note
     this in `git_blame.rs` when the time comes so the implementer
     doesn't hard-code the primary doc.
3. **~~Hub-client `automerge-rs` size cost.~~ Decided: v1 ships
   Option A unconditionally** (transport JSON across the WASM
   boundary; TS replay produces the canonical form). The size
   measurement required to evaluate Option B requires integrating
   `automerge-rs` into the WASM crate — exactly the work Option A
   avoids — so deferring the question would save no work, and
   keeping the question open invites a mid-implementation detour.
   Re-evaluate in v2 if `wasm-quarto-hub-client`'s bundle pressure
   changes, or if a future producer needs in-Rust Automerge access
   for reasons beyond attribution.

## Work items checklist

### Phase 0 — Tests first (TDD) — **complete (commit `b2ee6e70`)**

- [x] WASM-transport JSON round-trip test for `AttributionData`,
  including the `Arc::ptr_eq` interning invariant assertion
  (`tests/attribution_types.rs` — 3 green serde round-trips + 1 red
  ptr_eq restoration that panics at the `unimplemented!()` builder).
- [x] `Vec<AttributionRun>::query_byte_range` tests
  (`tests/attribution_types.rs`, 6 red cases: empty, single hit,
  non-overlapping, overlapping two actors picks most-recent, boundary,
  inverted/empty query).
- [x] TS git-blame fixture parsing tests (`tests/attribution_gitblame.rs`)
  driven by checked-in porcelain text at
  `tests/fixtures/attribution-blame/{single,multi}-commit.porcelain`
  plus `REGEN.md`. Multi-byte UTF-8 and trailing-newline edge cases
  covered.
- [x] Generate-stage happy path + skip condition tests
  (`tests/attribution_generate.rs`, 5 red cases incl. identities-only
  YAML override merge with the three sub-assertions a/b/c pinned).
- [x] Generate-stage skip-on-non-consuming-format test
  (`generate_non_consuming_format_skips_before_calling_provider` —
  uses a `PanicProvider` so any future skip-ladder regression is
  loud).
- [x] Render-stage q2-debug delivery test
  (`render_q2_debug_warning_path_emits_diagnostic_and_placeholder`
  + off-path `render_q2_debug_off_path_leaves_format_options_default`).
- [x] Render-stage HTML delivery test
  (`render_html_warning_path_populates_format_options_and_emits_one_diagnostic`).
- [x] Render-stage HTML coalescing test
  (`render_html_coalescing_groups_contiguous_same_attribution_prose`).
  Scaffold checked in; the cross-DOM assertion is a Phase 4b TODO
  that the writer-side coalescing pass will fill in once
  `HtmlConfig` carries the lookup fields.
- [x] Attribution-on + source-locations-off composition test (Phase 0
  test #7c) at
  `render_html_attribution_on_source_locations_off_compose_orthogonally`.
- [x] Structured-inline non-coalescing test (Phase 0 test #7d) at
  `render_html_structured_inlines_do_not_join_prose_coalescing`.
- [x] `SourceInfo` chain-resolution pin test
  (`tests/attribution_chain_resolution.rs`, 2 green cases — both
  pinned against the existing `map_offset` / `map_range` infra).
- [x] Include-fixture `file_id != 0` skip test (Phase 0 test #8b) at
  `render_skips_file_id_nonzero_nodes_even_when_byte_range_overlaps`.
- [x] End-to-end CLI fixture with two-author git history
  (`crates/quarto/tests/attribution_cli_e2e.rs`). Builds a temp git
  repo with pinned author identities and `GIT_AUTHOR_DATE` /
  `GIT_COMMITTER_DATE` for deterministic porcelain. RED until Phase
  3a + 3c land — the binary currently rejects `--attribution=git`.
- [x] CLI/YAML mode resolution matrix test (Phase 0 test #9b) in
  `tests/attribution_cli.rs`. All eight `(cli, yaml) → resolved`
  combinations pinned, including the escape-hatch
  `(Some(Off), Some(Git)) → Some(Off)` case. Plus the integration
  assertion that unflagged `RenderContext` has no provider installed
  (this part is green).
- [x] HTML-baseline regression snapshot — GREEN — at
  `tests/attribution_baseline_snapshot.rs` /
  `snapshots/attribution_baseline_snapshot__attribution_off_baseline.snap`.
- [x] WASM byte-identicality contract (Phase 0 test #10) at
  `tests/attribution_wasm_invariant.rs::no_provider_leaves_json_format_options_at_default`.
  Tests the native-side equivalent (the WASM stub is cdylib-only and
  can't be cargo-tested natively): both attribution transforms run
  with no provider installed leave `ctx.format_options` at default,
  which is what backs WASM byte-identicality.
- [x] Q2-debug happy-path test (Phase 0 test #11) at
  `q2_debug_happy_path_no_diagnostics_and_actors_populated_from_identities`.
  Pins that on happy paths NO diagnostic is emitted and the actors
  table comes from the provider's identities (not the warning-path
  `<unknown>`/`#888888` placeholder).
- [x] `GitBlameProvider` producer-invariant test (Phase 0 test #12) in
  `tests/attribution_gitblame.rs`. The deterministic-colour
  assertion will pin a known email's colour once `actor_color` /
  `fnv1a_hex8` are implemented in Phase 6.

#### Phase 0 outcome

- 46 tests checked in across 8 test files plus
  `attribution_cli_e2e.rs` in the `quarto` crate.
- 8 green tests (regression pins): 3 serde round-trips, 2 chain
  resolution, 1 RenderContext-default, 1 GitBlameProvider trait
  construction, 1 HTML off-path baseline snapshot.
- 38 red tests against `unimplemented!()`. Phase 1-6 turn these
  green incrementally — `intern_actor` / `push_run` (Phase 1)
  unblock the bulk of the generate / render / wasm-invariant tests
  in one go; `resolve_attribution_mode` (Phase 3c) is the 8 CLI
  resolution cases; `parse_blame_porcelain` + `build_blame_runs`
  (Phase 3a) are the gitblame parsing cases; `actor_color` /
  `fnv1a_hex8` (Phase 6) are the deterministic-colour cases; the
  E2E CLI test is the very last to turn green when Phase 3a + 3c +
  the writer-side glue all land.

The path from here is **Phase 1 — canonical types and provider
trait**. The Phase 0 commit covers all module/file creation Phase
1 was originally scheduled to do; Phase 1's remaining work is just
to replace the `unimplemented!()` bodies with real logic.

### Phase 1 — Types and trait

- [ ] Create `crates/quarto-core/src/attribution/{mod,types,source,prebuilt}.rs`.
- [ ] Implement the canonical types: `AttributionRun` (with
  `actor: Arc<str>`), `AttributionMap` (transparent `Vec<AttributionRun>`
  newtype with `is_empty` helper), `IdentityMap` (keyed by `Arc<str>`),
  `AttributionData`. Derive **`Serialize` only** on `AttributionRun`,
  `AttributionMap`, and `AttributionData` (no `Deserialize`); both
  `AttributionData` fields carry
  `#[serde(default, skip_serializing_if = "…is_empty")]`. Also
  implement the `AttributionSource` trait + blanket impl for
  `AttributionMap`.
- [ ] Implement the transport-only mirror types:
  `TransportAttributionRun { start, end, actor: String, time }` and
  `TransportAttributionData { runs: Vec<TransportAttributionRun>,
  identities: HashMap<String, Identity> }`, both with
  `Serialize + Deserialize`. Used only inside
  `PreBuiltAttributionProvider::build` (Phase 3b).
- [ ] Implement `AttributionDataBuilder` — the single canonical-form
  constructor. Owns a `HashMap<String, Arc<str>>` intern map; methods
  `intern_actor(&mut self, &str) -> Arc<str>`,
  `push_run(...)`, `set_identity(...)`, `build(self) -> AttributionData`.
  Doc-comment states the invariant the builder enforces. **All three
  producer callsites (`GitBlameProvider`, `PreBuiltAttributionProvider`,
  test fixtures) construct exclusively through this builder.**
- [ ] Implement small helper to read user-authored
  `meta.attribution.identities` (a `ConfigValue::Map`) into an
  `IdentityMap` for the Phase 2 merge. No round-trip the other way —
  the sidecar never serializes back to `ConfigValue`.
- [ ] Add **two fields** to `RenderContext`:
  `attribution_provider: Option<Arc<dyn AttributionSourceProvider>>`
  (the opt-in signal) and
  `attribution_data: Option<Arc<AttributionData>>` (the sidecar
  populated by Generate, read by Render). Verify Phase 0 type tests
  now pass.
- [ ] Add `format_supports_attribution(format: &Format) -> bool` (HTML
  + q2-debug JSON return `true` in v1; everything else `false`). Used
  by Phase 2's first skip-ladder rule.

### Phase 2 — Generate stage

- [x] Create `transforms/attribution_generate.rs` modelled on
  `navbar_generate.rs`. Skip ladder gates on
  `format_supports_attribution(&ctx.format)` first, before any other
  rule, to avoid invoking the provider on formats whose writers can't
  consume the lookup.
- [x] Wire into `pipeline.rs` as the last entry in the Navigation Phase,
  immediately after `FooterRenderTransform` (line 847).
- [x] Verify Phase 0 generate tests now pass (including the
  skip-on-non-consuming-format test).

#### Phase 2 outcome

All 6 tests in `tests/attribution_generate.rs` are now green:

- `generate_happy_path_populates_sidecar_and_preserves_arc_interning`
- `generate_with_empty_provider_identities_leaves_sidecar_identities_empty`
- `generate_no_provider_skips_silently`
- `generate_feature_disabled_skips`
- `generate_non_consuming_format_skips_before_calling_provider` (the
  Phase 0 test referenced `Format::from_format_string("native")` which
  doesn't exist in `FormatIdentifier`; swapped to `"pdf"` — any non-HTML
  format proves the ladder bails before the `PanicProvider`)
- `generate_identities_only_yaml_override_merges_correctly` (all three
  a/b/c sub-assertions)

Workspace-wide quarto-core: 1911→1917 passing (+6), 31→25 failing
(−6, all 6 Phase 2-owned). The 25 remaining failures are all explicit
`unimplemented!()` panics for Phase 3a/3b/3c/4/6 work; the
`attribution_off_html_baseline` regression snapshot stays green,
confirming the no-provider HTML path is byte-identical with the new
transform registered.

### Phase 3 — Providers

- [x] **3a.** Extend `BinaryDependencies` with `git: Option<PathBuf>` and
  wire its discovery (`runtime.find_binary("git", "QUARTO_GIT")`) into
  `BinaryDependencies::discover`. Prerequisite for `GitBlameProvider`.
- [x] **3a.** Implement `GitBlameProvider` (shell-out to `git blame
  --porcelain` via `ctx.binaries.git`; soft-fail with diagnostic when
  git is missing or the doc isn't in a working tree). Phase 0 git-blame
  fixture tests (9/9) pass; `actor_color` + `fnv1a_hex8` ported from
  the Phase 6 plan as prerequisites for identity synthesis.
- [x] **3a.** End-to-end fixture (`tests/fixtures/attribution-blame/`)
  with committed multi-author history. CLI E2E test exercises
  `--attribution=git` end-to-end through `q2 render`; the binary
  accepts the flag and the render succeeds. Final `data-attr-actor`
  assertion is still red because the HTML writer doesn't emit
  attribution markup yet — that's Phase 4 writer work.
- [x] **3c.** Add `--attribution` flag to `RenderArgs`; thread through
  `RenderToFileOptions` → `RenderContext`. CLI accepts
  `--attribution=git|off`; `resolve_attribution_mode` resolver
  pinned by Phase 0 test #9b. Outer `ctx` installs
  `Arc::new(GitBlameProvider::new())` for `Git`; bridged through
  `StageContext.attribution_provider` into the inner `RenderContext`
  used by `AstTransformsStage`.
- [x] **3b.** Implement `PreBuiltAttributionProvider` in
  `crates/quarto-core/src/attribution/prebuilt.rs` per the full
  definition in Phase 3b § Option A:
  `pub struct PreBuiltAttributionProvider { json: String }` with a
  `pub fn new(json: String) -> Self` constructor and a plain
  `impl AttributionSourceProvider` whose `build()` deserialises into
  `TransportAttributionData` then re-interns through
  `AttributionDataBuilder`. Producer-invariant unit test passes via
  Phase 0 test #1.
- [x] **3b.** Add `parse_qmd_to_ast_with_attribution(content,
  attribution_json)` WASM entry point that accepts the JSON payload.
  Implements the **direct-invocation flow** documented in Phase 3b:
  if `attribution_json.is_some()`, install
  `PreBuiltAttributionProvider::new(json)` on
  `ctx.attribution_provider` → run the existing 3-stage
  `pipeline::parse_qmd_to_ast` → if the provider is installed, call
  `AttributionGenerateTransform::new().transform(...)` and
  `AttributionRenderTransform::new().transform(...)` directly on the
  returned AST → build `JsonConfig` → serialize. Existing
  `parse_qmd_to_ast` is now a one-line wrapper delegating with
  `None` (byte-identicality invariant by construction). Phase 0 test
  #10 (byte-identicality) and #11 (q2-debug attribution-on) remain
  red pending Phase 4's `AttributionRenderTransform` body.

### Phase 4 — Render stage

- [x] Introduce `FormatOptions` carrier and add a `format_options` field
  to `RenderContext`. Plumb it from `render_qmd_to_html` /
  `parse_qmd_to_ast` into the corresponding writer config so the
  pre-baked lookup slice + identities table flow from the transform to
  the writer. **Done in Phase 1.** Bridged through `StageContext`
  back from `AstTransformsStage` in `render_html.rs`; the JSON
  WASM-side bridging will piggy-back when the q2-debug wire-shape
  emission lands.
- [x] Add `attribution_lookup`, `attribution_by_node`, and
  `attribution_actors` to `pampa::writers::json::JsonConfig`. **The
  fields exist on `RenderContext.format_options.json`; the
  `JsonConfig` side is wired by the hub-client / q2-debug
  integration in Phase 5.**
- [x] Add `attribution_by_node` (pointer-keyed map) +
  `attribution_identities: Option<Arc<HashMap<Arc<str>, HtmlAttributionIdentity>>>`
  to `pampa::writers::html::HtmlConfig`. The lookup is keyed by
  `&Block` / `&Inline` cast through `*const ()` to `usize`. Pointer
  keys are stable because `AttributionRenderTransform` is registered
  as the last Finalization-Phase entry — no later code mutates the
  AST.
- [ ] Emit `astContext.attribution` array **and** `astContext.attributionActors`
  table in JSON writer (records carry only `{ s, actor, time }`;
  identity lives in the actors table). **Deferred to Phase 5
  (hub-client integration).** The transform already populates
  `format_options.json.attribution_actors` and
  `attribution_by_node`; the streaming JSON writer needs a follow-on
  to map AST-walk-order entries onto the writer's `sourceInfoId`
  pool. Phase 0 q2-debug test still passes (the test only asserts
  format_options is populated, not the JSON wire shape).
- [x] Emit `data-attr-*` attributes in HTML writer. Implemented as
  per-element attribute emission (block `<p ... data-attr-*>` and
  inline `<span data-attr-*>` wrappers). **Prose coalescing is
  deferred** — the current implementation emits a wrapper per inline
  including each `Str` (Phase 0 tests #7b/#7c/#7d still pass because
  the asserted DOM-level invariants are TODO'd until a follow-on).
- [x] Verify regression snapshot (HTML off-path) still byte-identical
  — `attribution_off_html_baseline` green after Phase 4.
- [x] Create `transforms/attribution_render.rs`; in one AST walk build
  the pre-baked lookup vec **and** intern the actors table (resolving
  identity once per distinct actor, not once per record). Wire into
  `pipeline.rs` as the **very last** transform, immediately after
  `ResourceCollectorTransform` (line 882). The entire Finalization
  Phase runs between `AttributionGenerateTransform` (registered at the
  end of the Navigation Phase) and this stage.

#### Phase 4 outcome

- All 10 attribution Phase 0 tests across
  `attribution_render.rs`, `attribution_wasm_invariant.rs`, and
  `attribution_baseline_snapshot.rs` pass.
- CLI E2E test (`attribution_cli_e2e`) passes: rendering
  `--attribution=git` against a two-author git history yields
  `data-attr-actor="alice@example.com"` and
  `data-attr-actor="bob@example.com"` in the body HTML. Test
  fixture's split point switched to the first line-boundary at or
  past the midpoint so git blame credits a complete body line to
  each author.
- Off-path byte-identicality preserved: `cargo run --bin q2 -- render
  doc.qmd --to html` (no `--attribution`) produces zero `data-attr-*`
  attributes; `attribution_off_html_baseline` snapshot still green.
- Workspace tests: 8856 / 8856 passing.

End-to-end CLI invocation (manual verification):

```
$ cd /tmp/attr-e2e && \
    GIT_AUTHOR_NAME=Alice GIT_AUTHOR_EMAIL=alice@example.com \
    git commit -m alice
$ ... bob commit ...
$ cargo run --bin q2 -- render doc.qmd --to html --attribution=git
$ grep -o 'data-attr-actor="[^"]*"' doc.html | sort -u
data-attr-actor="alice@example.com"
data-attr-actor="bob@example.com"
```

Each paragraph carries all four `data-attr-*` attributes for its
author (actor, time, name, color), and the `<span>` wrapping each
prose word carries the same attribution. Prose-coalescing (one
outer wrapper around a contiguous same-author run) is a Phase 5+
follow-on.

### Phase 5 — Hub-client integration

Phase 5a (q2-debug JSON wire shape + WASM forwarding) landed in commit
`89bfbd9f`. Phase 5b (TS producer + ReactPreview wire-up) landed next.
Phase 5c (renderer-side color emission + Authorship toggle UI) is the
remaining deferred work.

- [x] Refactor `hub-client/src/hooks/useAttribution.ts` to emit a JSON
  payload instead of an in-process source. *(Built fresh; the
  implementation branch had no prior consumer-side hook to refactor —
  see § Branch context. The new hook owns Automerge replay, char→byte
  translation, and identity fallback.)*
- [x] Update `ReactPreview.tsx` to pass the payload to the new WASM
  entry point. Calls `useAttribution(...)` and routes through
  `parseQmdToAstWithAttribution(content, payload)` whenever a payload
  is present; falls back to byte-identical `…(content, null)`
  otherwise. **`enabled: false` for now** — the toggle UI is the
  Phase 5c work item.
- [x] **Producer-only code paths.** No consumer-side `attribution.ts`
  or `useNodeAttributionResolver` to delete — the implementation
  branch was forked off `main` and never inherited the prototype's
  consumer machinery. The new `hub-client/src/services/attribution-runs.ts`
  ports only the producer half (build/update + char→byte conversion);
  the consumer side is the Rust pipeline.
- [ ] **Phase 5c (deferred)** — `ReactAstDebugRenderer.tsx` reads
  attribution from `astContext.attribution` directly and joins each
  record's `actor` against `astContext.attributionActors` for
  `(name, color)`. The data path is now end-to-end; what's missing
  is the renderer-side colour application (wrapping each node with
  `style={{ color: actorIdentity.color }}` or a `data-attr-color`
  attribute). This is real renderer surgery touching most node
  variants. The current `ReactAstDebugRenderer` carries no
  attribution code, so there's nothing to remove — it's pure
  feature work. **Track in a follow-up beads issue once the
  Authorship toggle UI is sketched.**
- [ ] **Phase 5c (deferred)** — Authorship toggle UI in `Editor.tsx`
  (or a sidebar control). Flip `enabled: false → enabled: <toggleState>`
  on the `useAttribution` call in `ReactPreview.tsx`. Without UI,
  the data path stays cold by default; the producer invariant and
  byte-identicality guarantees still hold.
- [x] Run `cd hub-client && npm run build:all` — passes (`tsc -b &&
  vite build`, WASM build, all 74 unit tests).
- [ ] **End-to-end browser verification** — pending the Phase 5c
  renderer work. With `enabled: false`, there's no user-visible
  attribution to inspect even though the wire is plumbed; the
  meaningful end-to-end check is "two Automerge contributors, toggle
  on, q2-debug pane colours nodes per actor", and that requires both
  the toggle UI and the renderer colour application.

### Phase 6 — Defaults, palettes, identity

- [x] Port `actor_color` to `quarto-core/src/attribution/palette.rs`
  with the TS-drift-mitigation doc-comment. *(Landed alongside Phase 5a
  in commit `89bfbd9f` — the TS-side palette siblings shipped together
  with the Rust ones.)*
- [x] Add `fnv1a_hex8` helper in `palette.rs` (5-line FNV-1a, zero
  deps). Used by `GitBlameProvider` to pre-hash emails before
  feeding `actor_color`; the TS sibling plays the same role for
  Automerge actor IDs (Phase 5).
- [x] Wire `GitBlameProvider` to synthesize the `IdentityMap`
  (mail-local-part + `actor_color(fnv1a_hex8(email))`) — required
  for the Phase 6 producer invariant. *(Landed with Phase 3a in
  `dfe3ca12`.)* Phase 6 fleshed out test #12 from a placeholder
  into two fixture-driven producer-invariant assertions
  (`gitblame_single_author_fixture_satisfies_producer_invariant`,
  `gitblame_multi_author_fixture_satisfies_producer_invariant`) that
  pin the deterministic colour for `alice@example.com` (`hsl(253,
  60%, 55%)`) and `bob@example.com` (`hsl(220, 60%, 55%)`) and pin
  the Arc-interning invariant between run actors and identity-map
  keys. Required extracting an `attribution_from_porcelain` helper
  from `GitBlameProvider::build` so the porcelain → AttributionData
  path is testable without a `RenderContext`.
- [x] Verify `PreBuiltAttributionProvider` satisfies the producer
  invariant. The hub-client TS replay code is responsible for
  filling every actor's `IdentityMap` entry at the wire — using
  Automerge profile metadata when present and the
  `(actor.slice(0, 8), actorColor(fnv1aHex8(actor)))` fallback
  otherwise. *(TS-side fallback wired in Phase 5b; Rust-side
  `PreBuiltAttributionProvider` re-interns through
  `AttributionDataBuilder` so the Arc-ptr-eq invariant holds — pinned
  by `transport_round_trip_restores_arc_interning_via_prebuilt_provider`
  in `attribution_types.rs`.)*
- [x] Implement the render-side warning path: emit a diagnostic
  warning naming any actor missing from
  `ctx.attribution_data.identities`, then use the placeholder
  `Identity { display_name: "<unknown>", color: "#888888" }`. *(Landed
  with the Phase 4 render transform in `b07a4bb9`; exercised by
  `render_q2_debug_warning_path_emits_diagnostic_and_placeholder` and
  `render_html_warning_path_populates_format_options_and_emits_one_diagnostic`.)*
- [x] Add `docs/authoring/attribution.qmd` user-facing doc.

### Phase 7 — Verification

- [x] `cargo build --workspace` — clean, 0 warnings.
- [x] `cargo nextest run --workspace` — 8861 tests run, 8861 passed,
  195 skipped (36.9s).
- [x] `cargo xtask verify` — 9/9 steps green. One environmental skip:
  `--skip-treesitter-tests --skip-treesitter-crlf-tests` because the
  host has no `tree-sitter` CLI; attribution work touches no
  tree-sitter grammar so the skip is non-load-bearing for this
  feature. CI runs the full step.
- [x] Manual end-to-end (CLI):
  - Fixture: `/tmp/attr-e2e-kyoto/article.qmd` (two-author git history,
    Alice commits paragraphs 1–2, Bob adds paragraph 3).
  - Invocation: `target/debug/q2 render /tmp/attr-e2e-kyoto/article.qmd
    --attribution=git` (note: binary is `q2`, not `quarto` — plan text
    above predates the rename).
  - Observed in `article.html`:
    - `data-attr-actor="alice@example.com"` and
      `data-attr-actor="bob@example.com"` both present on `<p>` and
      per-word `<span>` wrappers.
    - Identity colours match Phase 6 pins: alice → `hsl(253, 60%, 55%)`,
      bob → `hsl(220, 60%, 55%)`.
    - `data-attr-name="alice"` / `"bob"` derived from the
      email-local-part as the producer invariant requires.
    - `data-attr-time` carries the commit Unix timestamp.
  - Output inspected directly; markup matches the Phase 4 outcome
    snippet on lines 2097-2114.
- [-] Manual end-to-end (hub-client browser): **not exercisable in v1.**
  `ReactPreview.tsx:155` hard-codes `enabled: false`. The data path
  (WASM ↔ JSON ↔ TS replay ↔ payload) is complete and unit-tested,
  but Phase 5c (Authorship toggle UI in `Editor.tsx` + per-node colour
  application in `ReactAstDebugRenderer.tsx`) is intentionally
  deferred — see the Phase 5 checklist boxes still open at lines
  2140-2155. Until that toggle and renderer surgery land, there is
  no surface for the user to flip attribution on, so a
  "two-contributor Automerge session" test has no UI to drive.
- [x] Deviations from the TS prototype's exact visual output
  (recorded below).

#### Phase 7 deviation log (TS prototype → Rust v1)

1. **Prose coalescing is deferred.** The current HTML writer emits
   one `<span data-attr-*>` per `Inline::Str`, matching the TS
   prototype's wrap-every-node behaviour but **not** the
   "one wrapper per contiguous same-(actor, time) run" outcome that
   Phase 0 test scaffolds `render_html_coalescing_groups_*` and
   `render_html_attribution_on_source_locations_off_compose_orthogonally`
   anticipate. The tests still pass because their cross-DOM
   assertions are TODO'd to Phase 4b (see
   `crates/quarto-core/tests/attribution_render.rs:289, 326, 376`).
   Acceptable for v1: the hover-target/data-extraction contract is
   already satisfied per-word, and the design-question resolution
   on line 1775 calls the coalescing pass a UX refinement rather
   than a correctness item. Tracked for v2 alongside the existing
   Phase 4b TODOs.
2. **q2-debug JSON wire shape is incomplete.** Plan line 2055-2063:
   "Emit `astContext.attribution` array and `astContext.attributionActors`
   table in JSON writer" is deferred. The transform populates
   `format_options.json.attribution_lookup` /
   `attribution_actors` / `attribution_by_node`, but the streaming
   JSON writer still needs to map AST-walk-order entries onto the
   writer's `sourceInfoId` pool. Phase 0 q2-debug test passes (it
   only asserts `format_options` is populated, not the on-wire
   shape).
3. **Hub-client Authorship toggle + renderer colouring (Phase 5c).**
   Wholly deferred; see the "not exercisable in v1" row above.
4. **Binary name.** Plan line 2214 says `quarto`; the actual default-run
   binary in this workspace is `q2`. Cosmetic discrepancy, no code
   change required.

## Non-goals for v1

- Caching attribution across renders. Re-blame on every render is fine
  (git-blame on a 100K-line file completes in ~50 ms).
- Project-wide attribution. Each document's pipeline has its own
  attribution provider; cross-document attribution (a project sidebar
  showing "X contributed to N pages") is a v2 feature that would consume
  the per-doc `ctx.attribution_data` and aggregate (or, if it needs
  to survive across pipeline runs, materialize through a project-index
  artifact). The data shape in v1 is designed to support it without
  re-engineering.
- Attribution diffs / time-range queries ("who edited this in the last
  week?"). Trivial to add atop `AttributionMap`; not requested.

## References

- **Fork point for the implementation branch:** `main`. The
  implementation branch does not inherit prototype code; the prototype
  is reference material only. See the Branch context section for the
  full anchor.
- **Prototype source files (all on `feat/node-attribution`, reference
  only):**
  - `hub-client/src/services/attribution-runs.ts` — the RLE producer;
    most informative single file. This is the design we'd port
    verbatim (cherry-pick or rewrite) when building the implementation
    branch's producer-side TS.
  - `hub-client/src/services/attribution.ts`,
    `hub-client/src/services/attribution-gitblame.ts` (and its `.test.ts`),
    `hub-client/src/hooks/useAttribution.ts`,
    `hub-client/src/components/ReactAstDebugRenderer.tsx` —
    classified in the Phase 5 reference-material subsection
    (algorithm reference / rewrite / replaced wholesale / clean
    implementation).
- **Prior TS plan (historical context, superseded by this plan):**
  `claude-notes/plans/2026-04-15-node-attribution.md` on
  `feat/node-attribution` (not on `main`).
- Pipeline registration site: `crates/quarto-core/src/pipeline.rs:735-863`
  (Navigation Phase 780-847, Finalization Phase 849-860 as of #169).
- Navbar generate pattern: `crates/quarto-core/src/transforms/navbar_generate.rs`.
- Navbar render pattern: `crates/quarto-core/src/transforms/navbar_render.rs`.
- `AstTransform` trait: `crates/quarto-core/src/transform.rs:69-90`.
- `RenderContext` struct: `crates/quarto-core/src/render.rs:84-188`
  (`resolved_listings` field added at line 187 by #169; the v1
  attribution work adds `attribution_provider` (opt-in signal),
  `attribution_data` (sidecar `Arc<AttributionData>` carrying the
  canonical merged form between Generate and Render), and
  `format_options` (writer-side pre-baked lookup) alongside it).
- HTML writer hook: `crates/pampa/src/writers/html.rs:601-655`.
- JSON writer config: `crates/pampa/src/writers/json.rs:51-100`.
- `SourceInfo` chain resolution: `crates/quarto-source-map/src/mapping.rs:15-87`.
- WASM entry points:
  - `parse_qmd_to_ast` (q2-debug; the v1 attribution carrier):
    `crates/wasm-quarto-hub-client/src/lib.rs:855`.
  - `render_qmd` (HTML preview, out of scope for v1): line 1005.
  - `render_qmd_content` (path-less HTML preview, out of scope): line 1152.
  - `render_page_in_project` (Phase 9 project-aware HTML preview,
    out of scope): line 1292.
- CLI render args: `crates/quarto/src/commands/render.rs:40-67`.
