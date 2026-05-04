# Plan 4 — SourceInfo provenance types (Synthetic + Derived + By struct)

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — foundation for Plans 5/6/7/8

## Goal

Extend `SourceInfo` with two new variants:

- `Synthetic { by: By }` — for nodes that have no source preimage at all
  (Sectionize's section Divs, filter constructions, synthesized title h1s,
  the footnotes container, etc.). Replaces the existing `FilterProvenance
  { filter_path, line }` variant — FilterProvenance becomes the special
  case `Synthetic { by: By::filter(...) }`.
- `Derived { from: Arc<SourceInfo>, by: By }` — for nodes that have a
  source preimage AND distinct atomic semantics. Used for shortcode
  resolutions: the resolved Str's `from` chain points at the shortcode
  token's bytes, and the `by` records that this is shortcode-derived
  content (so the writer can prohibit edits via Plan 7's atomic detection).
  *Not* used for filter mutations (those stay `Original` — non-atomic) or
  sugar transforms (their CustomNodes inherit Original from their input
  Div — also non-atomic).

`By` is an open `{ kind: String, data: serde_json::Value }` struct that
appears as the payload of both Synthetic and Derived. The `Original`,
`Substring`, `Concat` variants are unchanged.

## Scope

### In scope

- Add `Synthetic { by: By }` variant to `SourceInfo` enum.
- Add `Derived { from: Arc<SourceInfo>, by: By }` variant.
- Define `By` struct: `{ kind: String, data: serde_json::Value }`.
- Implement builder methods on `By` for known kinds: `filter`, `sectionize`,
  `user_edit`, `shortcode`, `include`, `title_block`, `footnotes`,
  `appendix`, `tree_sitter_postprocess`, `raw` (escape hatch).
- Migrate all `SourceInfo::FilterProvenance` construction sites to
  `SourceInfo::Synthetic { by: By::filter(...) }`.
- Migrate all `SourceInfo::FilterProvenance` pattern-match sites (~22 files
  flagged earlier).
- Remove the `FilterProvenance` variant.
- Update accessors: `start_offset`, `end_offset`, `length`, `map_offset`,
  `remap_file_ids`, `extract_file_id` (in diagnostic.rs) to handle both
  new variants. For `Derived`: recurse into `from` for offset accessors
  (returns the `from`'s offsets if the chain leads to Original).
- Update Lua serde (`pampa/src/lua/diagnostics.rs`) for both new variants.
  Keep `"FilterProvenance"` recognized as a legacy tag that maps to
  `Synthetic { by: By::filter(...) }` for back-compat reads.

### Out of scope

- JSON wire format changes (Plan 5 does that).
- Audit of transforms emitting `SourceInfo::default()` to fix them
  (Plan 6 does that).
- The `preimage_in` accessor (Plan 7 does that).
- Helper accessors like `as_filter()` — minimal interface in Plan 4;
  helpers added as call sites need them (Plans 6/7).

## Design decisions (settled in conversation)

- **`Derived` is reintroduced** (we'd dropped it earlier and walked it
  back). It came back because pure provenance preservation can't
  distinguish "shortcode resolution" (atomic; user edits prohibited at
  the writer level) from "filter mutation" (non-atomic; user edits
  flow to source). Both have a preimage in the same file; both could
  use Original; only Derived gives the writer a type-level way to know
  which is which.
- **Filter mutations stay Original**. A Lua filter that does
  `Str.text = upper(Str.text)` doesn't change source_info. The mutated
  Str retains its Original chain.
- **Filter constructions become Synthetic**. `pandoc.Str("decoration")`
  in a Lua filter produces `Synthetic { by: By::filter(filter_path, line) }`
  (replaces the existing FilterProvenance auto-attachment).
- **Shortcode resolutions become Derived**. The shortcode resolver
  emits `Derived { from: Original{shortcode_token_range}, by:
  By::shortcode(name) }` on resolved nodes. Plan 6 owns this.
- **Sugar transforms stay Original**. CalloutTransform et al. inherit
  source_info from their input Div. They're not atomic — the user
  editing a callout's body content is fine.
- **`By` is an open struct, not a closed enum**. Forward-compatibility
  for TS-Quarto-Lua-port and extension-defined kinds. Mirrors the
  `CustomNode.plain_data` pattern (also `serde_json::Value`-typed).
- **Kind-string convention**: kebab-case, namespaced for third-party
  (`ext/<extension>/foo`).
- **Builder methods for known kinds, plus `raw` escape hatch**.

## The proposed shape

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Arc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    Synthetic { by: By },
    Derived { from: Arc<SourceInfo>, by: By },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct By {
    /// Short kind tag, kebab-case. Examples: "filter", "sectionize",
    /// "user-edit", "shortcode", "include", "title-block".
    /// Third-party kinds should namespace: "ext/my-extension/foo".
    pub kind: String,

    /// Free-form structured data specific to this kind.
    /// `Null` for kinds that don't carry per-instance data.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

impl By {
    pub fn filter(filter_path: impl Into<String>, line: usize) -> Self { ... }
    pub fn sectionize() -> Self { ... }
    pub fn user_edit() -> Self { ... }
    pub fn shortcode(name: impl Into<String>) -> Self { ... }
    pub fn include() -> Self { ... }
    pub fn title_block() -> Self { ... }
    pub fn footnotes() -> Self { ... }
    pub fn appendix() -> Self { ... }
    pub fn tree_sitter_postprocess() -> Self { ... }
    pub fn raw(kind: impl Into<String>, data: serde_json::Value) -> Self { ... }
}
```

## Variant semantics summary

- **Original**: literal source bytes. The default. Most parser output.
- **Substring**: a textual slice of another SourceInfo. Existing pattern.
- **Concat**: concatenation of SourceInfos (e.g., from AttrSourceInfo's
  combine_all). Existing pattern.
- **Synthetic**: NO source preimage. The node was created from nothing.
  Sectionize wrappers, filter constructions, synthesized title h1s.
  Writer omits or recurses (Plan 7).
- **Derived**: HAS a source preimage but is a distinct transform output.
  The `from` chain points at the source bytes; `by` describes the
  transform. Writer treats as atomic (Plan 7) — KeepBefore Verbatim
  copies preimage; UseAfter triggers AtomicViolation. Used for shortcode
  resolutions; later for crossref cite resolutions if/when needed.

## Migrations

The pre-existing `FilterProvenance` is renamed/folded:

- **Construction**: `SourceInfo::filter_provenance("path", 42)` →
  `SourceInfo::Synthetic { by: By::filter("path", 42) }`.
  Add a deprecated alias `SourceInfo::filter_provenance` that constructs
  the new shape, eased migration; remove after migration completes.
- **Pattern-match**: every `SourceInfo::FilterProvenance { filter_path, line }`
  arm becomes `SourceInfo::Synthetic { by }` and inspects `by.kind ==
  "filter"` and `by.data["filter_path"]` / `by.data["line"]`. Or a small
  helper `By::as_filter() -> Option<(&str, usize)>` for the common case.

## `By` helper accessors

Plan 4 ships these helpers up front, so call sites in Plans 6 and 7 read
provenance consistently rather than each writing ad-hoc string-equality
checks against `by.kind`:

```rust
impl By {
    /// True if this kind matches the given string (sugar for `self.kind == kind`).
    pub fn is_kind(&self, kind: &str) -> bool { self.kind == kind }

    /// If this is a `filter` kind, return its `(filter_path, line)` payload.
    /// Returns None for any other kind.
    pub fn as_filter(&self) -> Option<(&str, usize)> {
        if self.kind != "filter" { return None; }
        let path = self.data.get("filter_path")?.as_str()?;
        let line = self.data.get("line")?.as_u64()? as usize;
        Some((path, line))
    }
}
```

Add more accessors as Plans 6/7 surface concrete repeated patterns. The
above two cover the immediate needs (filter-provenance recovery in tests,
generic kind matching in writer dispatch). Don't proliferate accessors
preemptively — `as_shortcode()`, `as_sectionize()`, etc. can be added if
their call sites prove repetitive.

## Builder list is extensible

The `By` builder list above (`filter`, `sectionize`, `user_edit`, etc.) is
the v1 known set. **Plan 6's audit may discover sites Plan 4 didn't
anticipate** — if so, Plan 6 adds new `By::<kind>()` builders to extend
the set. Builders are inert from Plan 4's perspective (a builder is just
a constructor that produces `By { kind: "...", data: ... }`); adding one
doesn't require reasoning about Plan 4's invariants.

Convention: each new builder gets a doc-comment explaining what kind of
node uses it and why. Keeps the `By` type's purpose discoverable.

## Open questions for implementation

- **Lua serde back-compat**: read `"FilterProvenance"` tag (legacy) and
  reconstruct as `Synthetic { by: By::filter(...) }`. New constructions
  emit `"Synthetic"` tag. Read both indefinitely; writes migrate to new
  immediately.
- **Tests update**: `pampa/src/lua/filter_tests.rs::test_filter_provenance_tracking`
  asserts on `SourceInfo::FilterProvenance`. Update to assert on
  `Synthetic { by }` with `by.is_kind("filter")` and check
  `by.as_filter()` returns the right path/line.

## References

- `crates/quarto-source-map/src/source_info.rs:22` — current SourceInfo enum.
- `crates/quarto-source-map/src/source_info.rs:48-54` — current
  FilterProvenance variant.
- `crates/quarto-source-map/src/source_info.rs:185-237` — accessors that
  need updating (start_offset, end_offset, length, remap_file_ids).
- `crates/quarto-source-map/src/mapping.rs:17-74` — `map_offset` recursion;
  needs new arm.
- `crates/pampa/src/lua/diagnostics.rs:60-145` — Lua serde to extend.
- `crates/pampa/src/lua/filter_tests.rs:663-728` — test to update.
- `crates/quarto-pandoc-types/src/custom.rs:75` — `CustomNode.plain_data`
  (the prior-art shape we're mirroring).

## Test plan

- Unit tests for each `By` builder method (constructs the right kind and data).
- Round-trip test: `By` → JSON → `By` (serde derive).
- Integration test: filter-provenance test (renamed from
  `test_filter_provenance_tracking`) confirms a filter-created Str gets
  `Synthetic { by: By::filter(...) }` source_info.
- Derived round-trip: build a `Derived { from: Original, by: By::shortcode("...") }`
  value; round-trip through JSON (Plan 5) and Lua serde; assert structural
  equality.
- Accessor recursion test: a `Derived` value's `start_offset()` / `end_offset()`
  / `length()` walk through `from` and return the from's offsets.
- Lua-serde round-trip: typed → Lua table → typed, including legacy
  `"FilterProvenance"` tag back-compat.

## Dependencies

- Depends on: nothing (pure type change in the foundation crate).
- Blocks: Plan 5 (wire format extension), Plan 6 (provenance audit), Plan 7
  (writer's preimage walk uses Synthetic and Derived).

## Risk areas

- **Migration scope**: ~22 files pattern-match `SourceInfo` variants. Each
  needs migration arms for *both* `Synthetic` and `Derived`. Most are
  mechanical: Synthetic arm returns what FilterProvenance did (usually
  `0`, `0`, or `None`); Derived arm recurses into `from` for offset
  accessors and returns the same as Synthetic for FileId-extracting helpers.
- **`Derived` accessor recursion**: `start_offset()`, `end_offset()`,
  `length()` need to recurse into `from`. A long Derived chain could
  in principle stack overflow, but in practice chains are 1-2 deep.
  Same risk profile as Substring.
- **`serde_json::Value` in PartialEq derives**: `Value` implements `PartialEq`
  but with potentially weird semantics for floats. For our use, kinds are
  string + small structured data; should be fine. Test the cases.
- **Removing `FilterProvenance` is a breaking change for downstream
  consumers**. Within the q2 workspace this is bounded; if any external code
  imports the variant by name, they'd break. Search for non-workspace usages
  before removing (probably none).

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `Synthetic` variant + accessors | ~50 |
| `Derived` variant + recursive accessors | ~50 |
| `By` struct + builders | ~100 |
| Pattern-match migrations (~22 files, both new variants) | ~250 |
| FilterProvenance construction site migrations | ~30 |
| Lua serde extension + back-compat (both variants) | ~80 |
| Test updates and new tests | ~200 |
| **Total** | **~760** |

One focused session, possibly stretching into a second given the slightly
larger scope from carrying Derived alongside Synthetic.

## Notes

The conceptual surface is "two new variants, one of which (`Synthetic`)
generalizes `FilterProvenance`." The pattern-match migration touches many
files but most arms are mechanical — Synthetic behaves like FilterProvenance
for offset accessors (returns 0, 0); Derived recurses into `from`.

Per the open-struct decision, `By` is `{ kind, data }` rather than a closed
enum. Builder methods give ergonomic, self-documenting construction at known
call sites; `By::raw` lets extensions add kinds without modifying the type.
The same `By` value appears as the payload of both Synthetic and Derived —
many kinds can be either depending on context, though in practice they
correspond cleanly:

| Kind | Variant | When used |
|---|---|---|
| `filter` | Synthetic | Lua filter constructions (`pandoc.Str(...)`) |
| `sectionize` | Synthetic | SectionizeTransform's section Divs |
| `title-block` | Synthetic | TitleBlockTransform's synthesized h1 |
| `footnotes` | Synthetic | FootnotesTransform's container Div |
| `appendix` | Synthetic | AppendixStructureTransform's wrapper Div |
| `tree-sitter-postprocess` | Synthetic | parser-side synthetic Spaces |
| `user-edit` | Synthetic | React-constructed nodes |
| `shortcode` | Derived | shortcode resolutions (Plan 6) |
| `include` | (wrapped, not Derived) | wrapper CustomNode in Plan 8 |
| `crossref-resolve` | (wrapped, not Derived) | already a CustomNode today |

Reintroducing Derived was a reversal of an earlier "drop it" decision.
The reversal happened when we recognized that Original chains alone can't
distinguish "shortcode resolution" (atomic) from "filter mutation"
(non-atomic). Derived gives Plan 7 the type-level distinction it needs to
trigger AtomicViolation correctly.
