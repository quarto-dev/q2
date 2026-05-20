# Plan 4 — SourceInfo provenance types (Generated + Anchor + AnchorRole)

**Date:** 2026-05-04 (substantially revised 2026-05-20)
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** none directly — foundation for the rest of the provenance
  epic

## Epic context

Plans 3–8 (filter idempotence, this plan, JSON wire format, provenance
audit, incremental writer + soft-drop, runtime filter check, include
round-trip) make up the **provenance epic** — the second wave of work
on the q2-preview branch after Plans 1–2 landed. They share a common
target: a typed, source-mapped notion of "where did this AST node come
from" that lets the incremental writer round-trip edits, lets
attribution credit the right author, and lets future diagnostics surface
resolution chains to users. The file names keep their q2-preview-plan-N
form for continuity with the earlier discussion notes.

## Goal

Extend `SourceInfo` with a single new variant, `Generated`, that
captures every transform-synthesized node in a uniform shape:

```rust
Generated { by: By, anchors: Vec<Anchor> }
```

`by` answers "which transform produced me." `anchors` is a list of
typed, role-labeled source-info pointers that answer "which source
bytes contributed to me." The list is empty for pure synthesis
(sectionize wrappers, filter constructions); has one `Invocation`
entry for shortcode resolutions; can carry additional roles
(`ValueSource`, future `Dispatch`, extension-defined `Other(...)`) as
the provenance picture sharpens.

The pre-existing `FilterProvenance` variant folds into `Generated`
(with `by.kind == "filter"`).

## Scope

### In scope

- Add `Generated { by: By, anchors: Vec<Anchor> }` variant to `SourceInfo`.
- Define `By` struct: `{ kind: String, data: serde_json::Value }`.
- Define `Anchor` struct: `{ role: AnchorRole, source_info: Arc<SourceInfo> }`.
- Define `AnchorRole` enum: `Invocation`, `ValueSource`, `Other(String)`.
  (`Dispatch` is a planned future role; see "Deferred anchor role" below.)
- Implement builder methods on `By` for known kinds: `filter`,
  `sectionize`, `user_edit`, `shortcode`, `include`, `title_block`,
  `footnotes`, `appendix`, `tree_sitter_postprocess`, `raw` (escape hatch).
- Implement helper accessors on `Generated`:
  - `invocation(&self) -> Option<&Arc<SourceInfo>>`
  - `value_source(&self) -> Option<&Arc<SourceInfo>>`
  - `anchors_with_role(&self, role: &AnchorRole) -> impl Iterator<&Arc<SourceInfo>>`
  - `append_anchor(&mut self, role: AnchorRole, source_info: Arc<SourceInfo>)`
- Migrate all `SourceInfo::FilterProvenance` construction sites to
  `SourceInfo::Generated { by: By::filter(...), anchors: vec![] }`,
  carrying `(filter_path, line)` in `by.data`.
- Migrate all `SourceInfo::FilterProvenance` pattern-match sites (~22 files
  flagged earlier) to the new shape.
- Remove the `FilterProvenance` variant.
- Update accessors: `start_offset`, `end_offset`, `length`,
  `resolve_byte_range`, `preimage_in`, `map_offset`,
  `remap_file_ids`, `extract_file_id` (in diagnostic.rs) to handle
  `Generated`. For `Generated`: delegate to `invocation()` for offset
  and byte-range accessors (returns the invocation anchor's range, or
  `None` if there is no invocation anchor).
- Update Lua serde (`pampa/src/lua/diagnostics.rs`) for `Generated`.
  Keep `"FilterProvenance"` recognized as a legacy tag that maps to
  `Generated { by: By::filter(...), anchors: vec![] }` for back-compat
  reads.

### Out of scope

- JSON wire format changes (Plan 5 does that).
- Audit of transforms emitting `SourceInfo::default()` to fix them
  (Plan 6 does that).
- The `is_atomic_custom_node` registry for CustomNode types (Plan 7
  owns it).
- The metadata loader changes that would populate `ValueSource`
  anchors on `meta` / `var` shortcode resolutions — that's a separate
  follow-up (see "Deferred anchor role" and Plan 6's "ValueSource
  follow-up" section).
- Registering Lua filter files in `SourceContext` to enable typed
  `Dispatch` anchors. See "Deferred anchor role" below.

## Design decisions (settled in conversation)

- **Single `Generated` variant, not two.** Earlier drafts proposed
  `Synthetic` + `Derived` to separate "no preimage" from "has preimage
  but is atomic." The unified `Generated { by, anchors: Vec<Anchor> }`
  expresses both with one variant: anchor-list empty for pure
  synthesis, anchor-list with `Invocation` for shortcode-style
  resolutions. The "has preimage" property is `gen.invocation().is_some()`,
  not a separate enum arm.
- **`by` records generator identity; `anchors` records source contributions.**
  These are orthogonal axes. Atomicity is determined by `by.kind`
  (per the `is_atomic_kind()` predicate); anchor-presence is orthogonal
  to atomicity.
- **Anchors are typed `Arc<SourceInfo>`, not dynamic JSON.** Path C in
  the 2026-05-20 discussion: rather than stuff source-info chain
  metadata into `by.data` (dynamic typing), use a typed list of
  role-labeled anchors. `by.data` shrinks to per-kind *non-source-info*
  configuration.
- **Filter mutations stay Original**. A Lua filter that does
  `Str.text = upper(Str.text)` doesn't change source_info. The mutated
  Str retains its Original chain. This is unchanged from the existing
  Lua machinery contract.
- **Filter constructions become `Generated { by: filter, anchors: [] }`**.
  `pandoc.Str("decoration")` in a Lua filter produces this shape (the
  Lua machinery's auto-attach replaces the existing FilterProvenance
  emission). Lua-file path and line live in `by.data` until
  Lua-file-registration lands; then they migrate to a `Dispatch` anchor.
- **Shortcode resolutions become `Generated { by: shortcode(name), anchors: [Invocation -> token_si] }`.**
  Plan 6 owns the resolver-side stamping; the resolver appends an
  `Invocation` anchor pointing at the shortcode token's source range.
- **Sugar transforms stay Original**. CalloutTransform et al. inherit
  source_info from their input Div. The Div's bytes are the canonical
  preimage of the resulting CustomNode wrapper; the wrapper's
  `type_name` carries the generator identity, so `source_info` doesn't
  need to also encode it. The same reasoning applies to Plan 8's
  `CustomNode("IncludeExpansion")` wrapper. See "Original vs Generated
  on synthesized nodes" below.
- **`By` is an open struct, not a closed enum**. Forward-compatibility
  for TS-Quarto-Lua-port and extension-defined kinds. Mirrors the
  existing precedent in `CustomNode.plain_data` and `Artifact.metadata`
  — open `serde_json::Value` at extension/dispatch seams; static typing
  everywhere else.
- **`AnchorRole` is a closed enum with an `Other(String)` escape hatch**.
  The known roles (`Invocation`, `ValueSource`) are the load-bearing
  ones the core consults. `Other(String)` lets extensions or future
  plans add roles without modifying the type.
- **Kind-string convention**: kebab-case, namespaced for third-party
  (`ext/<extension>/foo`). Same for `AnchorRole::Other` values.
- **Builder methods for known kinds, plus `raw` escape hatch**.

## The proposed shape

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Arc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    Generated { by: By, anchors: Vec<Anchor> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct By {
    /// Short kind tag, kebab-case. Examples: "filter", "shortcode",
    /// "sectionize", "user-edit", "title-block".
    /// Third-party kinds should namespace: "ext/my-extension/foo".
    pub kind: String,

    /// Per-kind configuration that is NOT a source-info pointer.
    /// Anchors live in `Generated.anchors`, not here.
    /// `Null` for kinds that don't carry per-instance data.
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Anchor {
    pub role: AnchorRole,
    pub source_info: Arc<SourceInfo>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AnchorRole {
    /// The user-written construct that triggered this node's creation
    /// (e.g. the `{{< meta foo >}}` token in the active document).
    /// Load-bearing: the writer's `preimage_in` and attribution's
    /// `resolve_byte_range` consult the first anchor with this role.
    /// At most one per node by convention.
    Invocation,

    /// Where the VALUE this node carries was defined, when distinct
    /// from the invocation site (e.g. `footer:` in `_metadata.yml` for
    /// a `{{< meta footer >}}` resolution). Diagnostic-only — does not
    /// affect the writer or attribution decisions in v1.
    ValueSource,

    /// Extension-defined or future role we haven't enumerated.
    /// String is kebab-case, namespaced (`ext/<name>/<role>`).
    Other(String),
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

    /// True if a `Generated { by: <self>, .. }` node should be treated
    /// as atomic by the incremental writer. Atomic nodes are produced
    /// by the pipeline and represent content the user shouldn't edit
    /// through React (filter constructions, shortcode resolutions,
    /// synthesized title h1, tree-sitter-inserted spaces).
    ///
    /// Atomicity is determined by `kind` alone — orthogonal to
    /// anchor-presence. A `Generated { by: shortcode, anchors: [...] }`
    /// is atomic; so is a `Generated { by: filter, anchors: [] }`.
    pub fn is_atomic_kind(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "filter" | "shortcode" | "title-block" | "tree-sitter-postprocess"
        )
    }

    pub fn is_kind(&self, kind: &str) -> bool { self.kind == kind }

    /// If this is a `filter` kind, return its `(filter_path, line)` payload.
    pub fn as_filter(&self) -> Option<(&str, usize)> {
        if self.kind != "filter" { return None; }
        let path = self.data.get("filter_path")?.as_str()?;
        let line = self.data.get("line")?.as_u64()? as usize;
        Some((path, line))
    }
}

impl Anchor {
    pub fn invocation(source_info: Arc<SourceInfo>) -> Self {
        Self { role: AnchorRole::Invocation, source_info }
    }
    pub fn value_source(source_info: Arc<SourceInfo>) -> Self {
        Self { role: AnchorRole::ValueSource, source_info }
    }
}

impl SourceInfo {
    pub fn generated(by: By) -> Self {
        SourceInfo::Generated { by, anchors: Vec::new() }
    }
}

// Helper methods on Generated-shape access — typically called via
// matching `SourceInfo::Generated { by, anchors } => ...`. We provide
// the helpers as free functions on the variant pattern; example:

impl SourceInfo {
    /// If this is `Generated`, return the first anchor whose role is
    /// `Invocation`. Returns `None` otherwise (including for
    /// non-`Generated` variants).
    pub fn invocation_anchor(&self) -> Option<&Arc<SourceInfo>> {
        match self {
            SourceInfo::Generated { anchors, .. } => anchors
                .iter()
                .find(|a| matches!(a.role, AnchorRole::Invocation))
                .map(|a| &a.source_info),
            _ => None,
        }
    }

    /// If this is `Generated`, return the first anchor whose role is
    /// `ValueSource`. Returns `None` otherwise.
    pub fn value_source_anchor(&self) -> Option<&Arc<SourceInfo>> {
        match self {
            SourceInfo::Generated { anchors, .. } => anchors
                .iter()
                .find(|a| matches!(a.role, AnchorRole::ValueSource))
                .map(|a| &a.source_info),
            _ => None,
        }
    }
}
```

## Variant semantics summary

- **Original**: literal source bytes. The default. Most parser output.
- **Substring**: a textual slice of another SourceInfo. Existing pattern.
- **Concat**: concatenation of SourceInfos (e.g., from AttrSourceInfo's
  combine_all). Existing pattern. **Contiguity expectation**: writer
  paths that need to Verbatim-copy a Concat (Plan 7's `preimage_in`)
  return `Some(range)` only when all pieces resolve into the target
  file AND are byte-contiguous in source order. Non-contiguous Concats
  return `None`, and Plan 7's coarsen falls through to Rewrite.
- **Generated**: produced by a pipeline transform. `by` records the
  producer; `anchors` records any source-side contributions. The
  variant subsumes the previous `Synthetic`/`Derived` distinction:
  - Empty anchors → pure synthesis (sectionize wrappers, filter
    constructions, title-block h1, tree-sitter postprocess, footnotes
    container, appendix wrapper, user-edit).
  - `Invocation` anchor present → has a source-side preimage (every
    shortcode resolution; future filter-with-trigger-anchor cases).
  - `ValueSource` anchor present → records where the value came from
    (future, gated on metadata-loader changes).
  - `Other(...)` anchor present → extension-defined.

  Writer behavior (Plan 7) consults `by.is_atomic_kind()` for
  atomicity and `gen.invocation_anchor()` for the preimage byte range.

## Original vs Generated on synthesized nodes

Two pieces of provenance information need to land somewhere when a
transform produces a node:

1. **Generator identity** — "which transform produced me."
2. **Source anchor** — "which source bytes are this node's canonical preimage."

For non-CustomNode synthesized nodes (sectionize Div, filter Str,
footnotes Div), there's no other slot for (1), so `source_info` carries
both via `Generated { by, anchors }`.

For CustomNode synthesized nodes, (1) is already encoded in
`CustomNode.type_name`. The wrapper *is* a `Callout` / `IncludeExpansion`
/ `CrossrefResolvedRef` by virtue of `type_name`; `source_info` only
needs to do (2). And the natural shape for (2) — when the CustomNode
1:1-substitutes for a parser-emitted source-mapped node — is the
inherited `Original` (or whatever `SourceInfo` shape the substituted
node carried).

| Synthesized node kind | Has CustomNode `type_name`? | Substitutes 1:1 for source-mapped node? | `source_info` shape |
|---|---|---|---|
| `IncludeExpansion` wrapper (Plan 8) | Yes | Yes (the include-line Paragraph) | Original (inherited) |
| `Callout` / `Theorem` / `Proof` / etc. | Yes | Yes (the source Div) | Original (inherited) |
| `CrossrefResolvedRef` | Yes | Yes (the source Cite) | Original (inherited) |
| `FloatRefTarget` | Yes | Yes (the source Div) | Original (inherited) |
| Sectionize Section Div | No | No (structural grouping) | `Generated { by: sectionize, anchors: [] }` |
| Footnotes container Div | No | No (structural grouping) | `Generated { by: footnotes, anchors: [] }` |
| Appendix wrapper Div | No | No (structural grouping) | `Generated { by: appendix, anchors: [] }` |
| Title-block synthesized h1 | No | No (synthesized from `title:` YAML) | `Generated { by: title_block, anchors: [] }` |
| Tree-sitter postprocess Space | No | No (inserted between nodes) | `Generated { by: tree_sitter_postprocess, anchors: [] }` |
| Shortcode resolution output | No | No (resolved from value, distinct from token bytes) | `Generated { by: shortcode("…"), anchors: [Invocation, …] }` |
| Filter-constructed node | No | No (filter computed it) | `Generated { by: filter, anchors: [] }` (Dispatch anchor in the future) |

The rule:

> A synthesized node uses **Original** `source_info` if and only if it
> is a CustomNode whose 1:1 source preimage is a parser-emitted node.
> Everything else uses **Generated**.

## `by.data` shape per kind

`by.data` is open `serde_json::Value` (matching the `CustomNode.plain_data`
and `Artifact.metadata` precedents). The known shapes per kind are:

| `by.kind` | `by.data` contents |
|---|---|
| `shortcode` (Rust handler) | `{ "name": "<shortcode-name>" }` |
| `shortcode` (Lua handler) | `{ "name": "<shortcode-name>", "lua_path": "<path>", "lua_line": <n> }` until Lua-file-registration; then just `{ "name": "<shortcode-name>" }` |
| `filter` | `{ "filter_path": "<path>", "line": <n> }` until Lua-file-registration; then `{}` |
| `sectionize` / `footnotes` / `appendix` / `title-block` / `tree-sitter-postprocess` / `user-edit` | `{}` (empty) |
| `ext/<name>/<kind>` (third-party) | extension-defined, opaque to core |

Convention: `data` is a JSON object with kind-specific known fields.
Consumers must treat unknown fields as opaque metadata. Producers may
add fields without breaking readers that don't look for them. Adding a
new field to a known kind's `data` is a non-breaking change.

This same convention applies to `CustomNode.plain_data`; Plan 4 codifies
it once for both seams. The pattern is "open Value at extension/dispatch
seams; static typing everywhere else" — `Anchor.source_info` stays
typed `Arc<SourceInfo>`; only the truly per-kind, heterogeneous data
sits in `by.data`.

## Atomic-kind set

`By::is_atomic_kind()` returns true for kinds whose nodes are "atomic"
from the incremental writer's perspective — nodes the user can't edit
honestly through React, because the pipeline regenerated them from
source-side input.

| `by.kind` | Atomic? | Role |
|---|---|---|
| `filter` | Yes | filter-constructed leaves; user edits the filter, not the output |
| `shortcode` | Yes | shortcode resolutions; user edits the token, not the resolved content |
| `title-block` | Yes | synthesized title h1; user edits `title:` metadata |
| `tree-sitter-postprocess` | Yes | parser-side synthetic spaces |
| `sectionize` | No (Transparent) | structural wrapper; children are editable |
| `footnotes` | No (Transparent) | container; children are editable |
| `appendix` | No (Transparent) | container; children are editable |
| `user-edit` | No | React-constructed; user-typed by definition |

Atomicity is per-kind, orthogonal to `anchors`. A `Generated { by: shortcode,
anchors: [Invocation -> token_si] }` is atomic; so is a
`Generated { by: filter, anchors: [] }`. The writer's coarsen
(Plan 7) consults `by.is_atomic_kind()` and `gen.invocation_anchor()`
independently.

Extensions that contribute new `by.kind` values are not atomic by
default. If an extension wants its kind to be atomic, the
`is_atomic_kind()` predicate (or a follow-up extension-registration
mechanism — see Plan 7 §Open questions) needs to recognize it. v1
hardcodes the built-in set.

## Migrations

The pre-existing `FilterProvenance` is renamed/folded:

- **Construction**: `SourceInfo::filter_provenance("path", 42)` →
  `SourceInfo::Generated { by: By::filter("path", 42), anchors: vec![] }`.
  The `(filter_path, line)` pair lives in `by.data` until
  Lua-file-registration lands.
  Add a deprecated alias `SourceInfo::filter_provenance` that
  constructs the new shape; remove after migration completes.
- **Pattern-match**: every `SourceInfo::FilterProvenance { filter_path, line }`
  arm becomes `SourceInfo::Generated { by, .. }` and inspects via
  `by.as_filter()` to recover the path/line.
- **Lua serde**: read `"FilterProvenance"` tag (legacy) and reconstruct
  as `Generated { by: By::filter(...), anchors: vec![] }`. New
  constructions emit `"Generated"` tag (or whatever Plan 4 picks for
  the new variant's Lua-table discriminant — convention TBD during
  implementation).

## Deferred anchor role

**`Dispatch` anchor (future).** When a Lua-implemented shortcode
handler or user filter constructs a node, the natural shape for
"where in Lua source was this constructed" is:

```rust
Anchor {
    role: AnchorRole::Dispatch,  // not in v1
    source_info: Arc::new(Original { file_id: kbd_lua_id, start, end }),
}
```

This requires Lua filter files to be registered in `SourceContext` so
they have `FileId`s. That's its own infrastructure work touching the
Lua engine, the source context, the diagnostic machinery, and the
cache-key surface. We defer it.

In the interim, the Lua machinery continues to carry `(filter_path,
line)` in `by.data` (see the `by.data` table above for `filter` and
Lua-dispatched `shortcode` kinds). When the Lua-file-registration
follow-up lands, the data migrates out of `by.data` and into a
`Dispatch` anchor; `AnchorRole::Dispatch` joins the enum (a
forward-compatible enum extension); and `by.data` for those kinds
shrinks to per-kind config only.

Filed as a follow-up beads issue at provenance-epic implementation time.

**`ValueSource` anchor (defined, deferred firing).**
`AnchorRole::ValueSource` is defined in Plan 4's type. The shortcode
resolver doesn't attach it yet, because the metadata loader doesn't
record per-key source-info today (every metadata key's `source_info`
points at where the value was parsed from, but the merged metadata
that the resolver consults doesn't expose this). A separate follow-up
issue covers extending the metadata loader to thread per-key source
through to the merged value. When that lands, Plan 6's stamper
appends `ValueSource` anchors for `meta` and `var` shortcode
resolutions whose values came from outside the active document.

Both follow-ups are pure additions when they land — neither requires
reopening Plan 4's type design. The shape is forward-compatible by
construction.

## Resolve-byte-range and preimage-in semantics

```rust
impl SourceInfo {
    pub fn resolve_byte_range(&self) -> Option<(usize, usize, usize)> {
        match self {
            SourceInfo::Original { file_id, start_offset, end_offset } =>
                Some((file_id.0, *start_offset, *end_offset)),
            SourceInfo::Substring { parent, start_offset, end_offset } => {
                let (fid, parent_start, _) = parent.resolve_byte_range()?;
                Some((fid, parent_start + start_offset, parent_start + end_offset))
            }
            SourceInfo::Concat { .. } => None,
            SourceInfo::Generated { .. } => self
                .invocation_anchor()
                .and_then(|si| si.resolve_byte_range()),
        }
    }

    pub fn preimage_in(&self, target: FileId) -> Option<Range<usize>> {
        match self {
            SourceInfo::Original { file_id, start_offset, end_offset }
                if *file_id == target => Some(*start_offset..*end_offset),
            SourceInfo::Original { .. } => None,
            SourceInfo::Substring { parent, start_offset, end_offset } => {
                let parent_range = parent.preimage_in(target)?;
                Some(parent_range.start + start_offset .. parent_range.start + end_offset)
            }
            SourceInfo::Concat { pieces } => /* existing contiguity logic */,
            SourceInfo::Generated { .. } => self
                .invocation_anchor()
                .and_then(|si| si.preimage_in(target)),
        }
    }
}
```

Both accessors collapse `Generated`'s handling into "look up the
invocation anchor; recurse into its source_info." Pure synthesis
(empty anchors) returns `None`. Multi-anchor Generateds (when
`ValueSource` lands) still only consult `Invocation` — `ValueSource`
is diagnostic-only.

## Open questions for implementation

- **Lua serde back-compat**: read `"FilterProvenance"` tag (legacy) and
  reconstruct as `Generated { by: By::filter(...), anchors: vec![] }`.
  New constructions emit `"Generated"` tag. Read both indefinitely;
  writes migrate to new immediately.
- **Tests update**: `pampa/src/lua/filter_tests.rs::test_filter_provenance_tracking`
  asserts on `SourceInfo::FilterProvenance`. Update to assert on
  `Generated { by, .. }` with `by.is_kind("filter")` and check
  `by.as_filter()` returns the right path/line.
- **`Generated` variant Lua-table discriminant**: convention TBD —
  candidate is `t = "Generated"` with `by` and `anchors` sub-tables.

## References

- `crates/quarto-source-map/src/source_info.rs:22` — current SourceInfo enum.
- `crates/quarto-source-map/src/source_info.rs:48-54` — current
  FilterProvenance variant.
- `crates/quarto-source-map/src/source_info.rs:185-237` — accessors that
  need updating (start_offset, end_offset, length, resolve_byte_range,
  remap_file_ids).
- `crates/quarto-source-map/src/mapping.rs:17-74` — `map_offset` recursion;
  needs new arm.
- `crates/pampa/src/lua/diagnostics.rs:60-145` — Lua serde to extend.
- `crates/pampa/src/lua/filter_tests.rs:663-728` — test to update.
- `crates/quarto-pandoc-types/src/custom.rs:75` — `CustomNode.plain_data`
  (the prior-art for `serde_json::Value` at extension seams; same
  convention now applies to `By.data`).
- `crates/quarto-core/src/artifact.rs:71` — `Artifact.metadata`
  (second precedent for the same pattern).

## Test plan

- Unit tests for each `By` builder method (constructs the right kind and data).
- Unit tests for `Anchor::invocation()` / `Anchor::value_source()`
  constructors.
- Round-trip test: `By` → JSON → `By` (serde derive).
- Round-trip test: `Anchor` → JSON → `Anchor` (serde derive).
- Integration test: filter-provenance test (renamed from
  `test_filter_provenance_tracking`) confirms a filter-created Str gets
  `Generated { by: filter, anchors: [] }` with `(filter_path, line)`
  recoverable via `by.as_filter()`.
- `invocation_anchor()` accessor test: a Generated with `[Invocation -> X]`
  returns `Some(X)`; with `[]` returns `None`; with
  `[ValueSource -> Y]` (no Invocation) returns `None`.
- `value_source_anchor()` accessor test: parallel coverage.
- Accessor recursion test: a `Generated { anchors: [Invocation -> Substring{parent: Original{42, 100, 200}, 10, 20}] }`
  resolves to `(42, 110, 120)` via `resolve_byte_range`; same value via
  `preimage_in(FileId(42))`.
- `is_atomic_kind()` test: confirms the set named in §"Atomic-kind set".
- Lua-serde round-trip: typed → Lua table → typed, including legacy
  `"FilterProvenance"` tag back-compat.

## Dependencies

- Depends on: nothing (pure type change in the foundation crate).
- Blocks: Plan 5 (wire format extension), Plan 6 (provenance audit),
  Plan 7 (writer's preimage walk uses Generated and the
  `invocation_anchor` helper).

## Risk areas

- **Migration scope**: ~22 files pattern-match `SourceInfo` variants.
  Each needs migration arms for `Generated`. Most are mechanical:
  Generated arm returns what FilterProvenance did (usually `0`, `0`,
  or `None`) when there are no anchors, or delegates to the invocation
  anchor for offset accessors.
- **`Vec<Anchor>` allocation**: every Generated value carries an
  allocation. In practice the vec is empty most of the time
  (sectionize/footnotes/etc.). `SmallVec<[Anchor; 1]>` or similar
  could avoid the allocation for the empty / single-entry cases — TBD
  during implementation if profiling shows it matters. Default to
  plain `Vec` initially.
- **`serde_json::Value` in PartialEq derives**: `Value` implements
  `PartialEq` but with potentially weird semantics for floats. For our
  use, kinds carry string + small structured data; should be fine.
  Test the cases.
- **Removing `FilterProvenance` is a breaking change for downstream
  consumers**. Within the q2 workspace this is bounded; if any external
  code imports the variant by name, they'd break. Search for
  non-workspace usages before removing (probably none).

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `Generated` variant + `Anchor` + `AnchorRole` types | ~80 |
| Accessors (invocation_anchor, value_source_anchor, etc.) | ~60 |
| `By` struct + builders + `is_atomic_kind` | ~120 |
| `resolve_byte_range` / `preimage_in` / map_offset updates | ~50 |
| Pattern-match migrations (~22 files) | ~250 |
| FilterProvenance construction site migrations | ~30 |
| Lua serde extension + back-compat | ~80 |
| Test updates and new tests | ~250 |
| **Total** | **~920** |

One to two focused sessions. The unified-variant design reduces the
total cost vs. the previous Synthetic-plus-Derived dual-variant draft
(every accessor and migration site collapses one arm).

## Notes

The conceptual surface is "one new variant, `Generated`, with a typed
anchor list." The pattern-match migration touches many files but most
arms are mechanical.

Per the open-struct decision, `By` is `{ kind, data }` rather than a
closed enum. Builder methods give ergonomic, self-documenting
construction at known call sites; `By::raw` lets extensions add kinds
without modifying the type. The `Anchor` list is typed throughout —
each entry's `source_info` is an `Arc<SourceInfo>`, not dynamic JSON.

The earlier `Synthetic`/`Derived` split was a useful intermediate during
design discussion (it crystallized the atomic-vs-not distinction), but
the unified `Generated` shape captures the same information with fewer
moving parts. The "has preimage" property becomes
`gen.invocation_anchor().is_some()` rather than a separate enum arm;
atomicity stays per-`by.kind`, orthogonal to anchor-presence.

| Kind | Variant | Anchors | When used |
|---|---|---|---|
| `filter` | Generated | `[]` (Dispatch later) | Lua filter constructions (`pandoc.Str(...)`) |
| `sectionize` | Generated | `[]` | SectionizeTransform's section Divs |
| `title-block` | Generated | `[]` | TitleBlockTransform's synthesized h1 |
| `footnotes` | Generated | `[]` | FootnotesTransform's container Div |
| `appendix` | Generated | `[]` | AppendixStructureTransform's wrapper Div |
| `tree-sitter-postprocess` | Generated | `[]` | parser-side synthetic Spaces |
| `user-edit` | Generated | `[]` | React-constructed nodes |
| `shortcode` | Generated | `[Invocation]` (`+ValueSource` later, `+Dispatch` later for Lua) | shortcode resolutions (Plan 6) |
| `include` | (wrapped CustomNode, source_info Original) | — | wrapper CustomNode in Plan 8 |
| `crossref-resolve` | (wrapped CustomNode, source_info Original) | — | already a CustomNode today |
