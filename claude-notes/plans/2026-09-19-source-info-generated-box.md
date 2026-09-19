# Shrink `SourceInfo` from 136 to 32 bytes by boxing the `Generated` payload (bd-1c085k3a)

**Status:** filed for an independent session. Not started.
**Discovered from:** bd-w0x91nmh (PR #698), which measured the traversal
cost and left the enum-size lever on the table. Background and numbers:
`claude-notes/research/2026-09-19-topdown-traverse-alloc-perf.md`.
**Related:** bd-5yektmwt (AST memmove), bd-jn7r22g8 (the previous
`quarto-source-map` change; its plan
`claude-notes/plans/2026-09-18-source-map-map-offset-clone.md` documents
the external-crate workflow this plan reuses).

## Overview

`quarto_source_map::SourceInfo` is 136 bytes. Three of its four variants
are 24 bytes; the fourth is

```rust
Generated { by: By, from: SmallVec<[Anchor; 2]> }
//          By = String + serde_json::Value = 56
//          SmallVec<[Anchor; 2]> = 80  (Anchor = AnchorRole(24, has an Other(String)) + Arc = 32)
```

so every `SourceInfo` in the tree pays 136 bytes for a variant that only
synthesized nodes use. Every AST node carries at least one, and the
attribute-bearing ones carry many more (`AttrSourceInfo` = `Option<SourceInfo>`
+ two `Vec`s of them = 184; `TargetSourceInfo` = two `Option<SourceInfo>` = 272).
That is what makes `Space` 136 bytes, `Str` 160, `Link`/`Image` 768,
`Inline` 776 and `Block` 1552, and it is why `memmove` is 27.5% of a
`q2 render` profile of the Connect docs.

Measured on the current crate (throwaway size test, arm64, release-perf):

| layout                                                    | `SourceInfo` | `Option<SourceInfo>` |
| --------------------------------------------------------- | -----------: | -------------------: |
| today                                                     |          136 |                  136 |
| `Generated(Box<Generated>)`                               |       **32** |               **32** |
| keep struct variant, `data: Option<Box<Value>>`, `from: Vec` |        56 |                   56 |

Boxing is the one that reaches the floor (the `Original` variant is 24
bytes + tag). Estimated downstream effect with **no change to
`quarto-pandoc-types`** (arithmetic; verify with the size test):

| type              | now  | after |
| ----------------- | ---: | ----: |
| `Space`           |  136 |    32 |
| `Str`             |  160 |    56 |
| `AttrSourceInfo`  |  184 |    80 |
| `TargetSourceInfo`|  272 |    64 |
| `Link` / `Image`  |  768 |  ~352 |
| `Inline`          |  776 |  ~360 |
| `Table`           | 1552 |  ~830 |
| `Block`           | 1552 |  ~830 |

The microbenchmark in the research note says walk cost scales roughly
linearly with element size, so this is expected to be worth more than
PR #698's traversal change, and it compounds with it. Boxing
`Link`/`Image` source infos and `Table` afterwards (a separate,
`quarto-pandoc-types`-only change) would take `Inline` to ~250 and
`Block` to ~330.

## Design

In `posit-dev/quarto-source-map` (`main`):

```rust
pub enum SourceInfo {
    Original { .. },            // unchanged
    Substring { .. },           // unchanged
    Concat { .. },              // unchanged
    Generated(Box<Generated>),  // was: Generated { by, from }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generated {
    pub by: By,
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub from: SmallVec<[Anchor; 2]>,
}

impl SourceInfo {
    pub fn generated(by: By) -> Self;                                  // exists today; keep
    pub fn generated_with(by: By, from: impl Into<SmallVec<[Anchor; 2]>>) -> Self; // new
    pub fn as_generated(&self) -> Option<&Generated>;                  // new
    pub fn as_generated_mut(&mut self) -> Option<&mut Generated>;      // new
}
```

**Crate-side plan of record:** the `quarto-source-map` repo has its own
plan at `claude-notes/plans/2026-09-19-source-info-generated-box.md`
(same date, same name), written by the agent working there. It agrees
with this design and is more precise on the crate internals; where the
two differ, **that file wins for the crate and this file wins for the q2
migration**. The one correction it made to an earlier draft of this
file: the existing one-argument `SourceInfo::generated(by)` must keep
its signature — q2 has 228 call sites of it and only one hand-built
`Generated { by, from }` with non-empty anchors — so anchors-at-
construction gets a new `generated_with`, not a second argument.

- **Wire shape is unchanged.** serde's externally tagged enum encoding
  of a newtype variant wrapping a struct is `{"Generated": {"by": …,
  "from": …}}`, byte-identical to the struct variant, and `Box<T>`
  serializes as `T`. Move the `from` attribute onto the struct field.
  Add a round-trip test in the crate that pins the JSON of a `Generated`
  value to a literal string so this cannot regress silently. The TS
  `preview-renderer` reads pampa's writer-internal shape
  (`crates/pampa/src/writers/json.rs`, `d: { by, from }`), not this one,
  so it is unaffected as long as the pampa writer/reader are migrated
  with the rest of the sites.
- **Keep `SmallVec` inside the box.** Once boxed, the inline storage no
  longer affects `SourceInfo`'s size, and keeping the type avoids
  touching the 22 `smallvec!`/`SmallVec` sites in q2. (Switching to
  `Vec<Anchor>` later is a separate, optional cleanup.)
- `By` is untouched. Shrinking `data: serde_json::Value` is unnecessary
  once the payload is boxed.
- **Version.** This changes the public shape of an enum variant, so it
  is `0.2.0`, not `0.1.5`. Two other published crates depend on
  `quarto-source-map`: `quarto-yaml` (0.1.3) and `quarto-error-reporting`
  (0.2.2). Neither names `Generated`, `By`, `Anchor` or `SmallVec` (grep
  of the registry sources, 2026-09-19), so they compile unchanged, but
  cargo will resolve **two copies** of `quarto-source-map` if their
  requirement stays at `0.1.x` while q2 asks for `0.2` — and then
  `quarto_yaml`'s `SourceInfo` is a different type from q2's. So the
  release order is: source-map 0.2.0 → quarto-error-reporting (bump dep,
  0.2.3) → quarto-yaml (bump dep, 0.1.4) → q2 bumps all three together.
  The alternative — shipping this as 0.1.5 because no external consumer
  names the variant — avoids the two follow-on releases but lies about
  semver; only take it with Carlos's explicit sign-off.

In q2: 114 `Generated {` sites in 19 non-test files (52 constructions,
42 patterns), plus test files; the 228 existing `SourceInfo::generated(by)`
calls need no change. Hand-built constructions become
`SourceInfo::generated(by)` / `generated_with(by, from)`; the ~10 sites
that mutate `from` in place use the crate's existing `append_anchor` or
the new `as_generated_mut`; patterns become
`SourceInfo::Generated(g)` with `g.by` / `g.from` (or
`let Generated { by, from } = &**g;`). Heaviest files:
`pampa/src/writers/json.rs` (21), `pampa/src/readers/json.rs` (18),
`quarto-core/src/transforms/shortcode_resolve.rs` (15),
`pampa/src/lua/diagnostics.rs` (10), `quarto-core/src/transforms/appendix.rs`
(8), `title_block.rs` (7). This is mechanical; the compiler finds every
site.

## Checklist

### Phase 0 — tests first (in `quarto-source-map`)

- [ ] Size test: `assert_eq!(size_of::<SourceInfo>(), 32)` and
      `size_of::<Option<SourceInfo>>() == 32`. **Must fail** on today's
      code (136).
- [ ] Wire-shape test: serialize a `Generated` value with a non-empty
      `from` and one with an empty `from` to JSON and compare against
      literal strings captured from the *current* 0.1.4 code, so the
      test is written before the change and proves compatibility after.
- [ ] Existing crate tests + doctests stay green (`cargo test --locked`).

### Phase 1 — crate change

- [ ] Introduce `pub struct Generated`, switch the variant to
      `Generated(Box<Generated>)`, add `SourceInfo::generated(..)` and
      `as_generated()`.
- [ ] Migrate the crate's own uses (`mapping.rs`, `preimage_in`, tests).
- [ ] Bump to 0.2.0, CHANGELOG entry, push a branch, open the PR
      (Carlos reviews/merges/publishes).

### Phase 2 — q2 migration, verified locally before any release

- [ ] Clone the crate to `external-sources/quarto-source-map` (it is not
      present today; `external-sources/` is gitignored) and add a
      temporary, **uncommitted** `[patch.crates-io] quarto-source-map =
      { path = "external-sources/quarto-source-map" }` in the root
      `Cargo.toml`, exactly as bd-jn7r22g8 did. Never commit the patch.
- [ ] Migrate the 114 sites (+ test files). `cargo build --workspace`
      until clean.
- [ ] Add a size regression test in q2: extend
      `crates/pampa/tests/integration/topdown_traverse_alloc_budget.rs`
      (or a sibling module in the same integration binary — do **not**
      add a top-level `tests/*.rs` file) with
      `assert!(size_of::<Inline>() <= 400)` and
      `assert!(size_of::<Block>() <= 900)`, so the next fat field is
      caught in review.
- [ ] `cargo nextest run --workspace`; then full `cargo xtask verify`
      (pampa feeds the WASM leg). The JSON snapshot suites are the
      wire-compatibility check on the pampa side — **no `.snap` file
      should change**; if one does, the serialization shape moved.
- [ ] Measure, same method as bd-w0x91nmh: `release-perf` build,
      `hyperfine -w 2 -r 10 -N` on `docs-quarto-2/api/index.qmd`, then
      `QUARTO_JOBS=1 hyperfine -w 1 -r 3 -N` on the full site, against a
      saved copy of the pre-change binary, back to back. Record both in
      this file.
- [ ] Remove the `[patch.crates-io]` before committing anything.

### Phase 3 — releases and cutover

- [ ] `quarto-source-map` 0.2.0 published.
- [ ] `quarto-error-reporting` dep bump → 0.2.3 published.
- [ ] `quarto-yaml` dep bump → 0.1.4 published.
- [ ] q2: bump all three in `Cargo.toml`, `cargo update -p` each,
      confirm `cargo tree -d` shows a single `quarto-source-map`, commit
      the migration, PR.

## Notes for the implementing agent

- The compiler is the site inventory; don't grep-and-guess. Start with
  `cargo build -p quarto-source-map` under the patch, then
  `cargo build --workspace` and fix errors file by file.
- `writers/json.rs` and `readers/json.rs` in pampa are the only places
  where the *shape* of `Generated` is spelled out by hand for the wire;
  everything else just constructs or matches. Read both before touching
  either.
- `quarto_source_map::SourceInfo::for_test()` and the `empty_source_info`
  helpers in pampa are the usual way tests build values; they may need
  no change at all.
- Do not widen the scope into boxing `Link`/`Image`/`Table` or into
  `By::data`; those are separate strands with separate measurements.
