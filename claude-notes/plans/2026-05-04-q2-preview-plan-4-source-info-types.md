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
Generated { by: By, from: SmallVec<[Anchor; 2]> }
```

`by` answers "which transform produced me." `from` is a list of
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

- Add `Generated { by: By, from: SmallVec<[Anchor; 2]> }` variant to `SourceInfo`. Inline capacity 2 covers the steady-state shape after the deferred follow-ups land (Invocation + ValueSource on `meta`/`var` shortcodes; Invocation + Dispatch on Lua-handler shortcodes); see §Risk areas for the trade-off.
- Define `By` struct: `{ kind: String, data: serde_json::Value }`.
- Define `Anchor` struct: `{ role: AnchorRole, source_info: Arc<SourceInfo> }`.
- Define `AnchorRole` enum: `Invocation`, `ValueSource`, `Other(String)`.
  (`Dispatch` is a planned future role; see "Deferred anchor role" below.)
- Implement builder methods on `By` for known kinds: `filter`,
  `sectionize`, `user_edit`, `shortcode`, `include`, `title_block`,
  `footnotes`, `appendix`, `tree_sitter_postprocess`, `raw` (escape hatch).
- Implement helper accessors on `SourceInfo` for the `Generated` shape:
  - `invocation_anchor(&self) -> Option<&Arc<SourceInfo>>`
  - `value_source_anchor(&self) -> Option<&Arc<SourceInfo>>`
  - `anchors_with_role(&self, role: &AnchorRole) -> impl Iterator<Item = &Arc<SourceInfo>>`
  - `append_anchor(&mut self, role: AnchorRole, source_info: Arc<SourceInfo>)`
- Migrate all `SourceInfo::FilterProvenance` construction sites to
  `SourceInfo::Generated { by: By::filter(...), from: smallvec![] }`,
  carrying `(filter_path, line)` in `by.data`.
- Migrate all `SourceInfo::FilterProvenance` pattern-match sites
  (15 files, 27 occurrences — see §Risk areas) to the new shape.
- Remove the `FilterProvenance` variant.
- Update accessors on `SourceInfo` to handle `Generated`:
  - `length`, `start_offset`, `end_offset` — return `0` (same as today's
    `FilterProvenance`; Generated has no characteristic local-text length).
  - `map_offset` — return `None` (offset-within-current-text is undefined
    for Generated; callers wanting source coordinates use
    `resolve_byte_range`).
  - `resolve_byte_range` — delegate to `invocation_anchor()` and recurse
    (returns the invocation anchor's chain-resolved range, or `None` if
    there is no invocation anchor).
  - `remap_file_ids` — walk every `Anchor.source_info` and recurse via
    `Arc::make_mut`. Unlike `FilterProvenance` (no-op), `Generated` CAN
    carry `FileId`s inside its anchors.
  - `extract_file_id` (in `quarto-error-reporting/src/diagnostic.rs`) —
    delegate to `invocation_anchor()` and recurse (parallel to
    `resolve_byte_range`). Empty-`from` Generated returns `None`, which
    matches today's `FilterProvenance` arm; the two call sites in
    `to_ariadne_report` (`diagnostic.rs:674`, `:773`) both tolerate
    `None` gracefully (the main-location path falls through via `?`;
    the detail loop `continue`s), so no caller change is required.
    `extract_file_id` stays a private `fn` on `DiagnosticMessage` — no
    promotion to a `SourceInfo` method, since no duplicate
    file-id-extraction logic exists elsewhere in the workspace.
- Update Lua serde (`pampa/src/lua/diagnostics.rs`) for `Generated`.
  Use `t = "Generated"` as the discriminant; the table carries `by` and
  `from` sub-tables. Keep `"FilterProvenance"` recognized as a legacy
  tag that maps to `Generated { by: By::filter(...), from: smallvec![] }`
  for back-compat reads.

### Out of scope

- JSON wire format changes (Plan 5 does that).
- Audit of transforms emitting `SourceInfo::default()` to fix them
  (Plan 6 does that). `Default for SourceInfo` itself is unchanged
  (stays `Original { FileId(0), 0, 0 }`); Plan 6 fixes incorrect
  emissions at transform sites without modifying the trait impl.
- The `preimage_in` accessor (Plan 7 owns it). Plan 7's `preimage_in`
  consumes `invocation_anchor()` defined here; the contiguity rule
  for `Concat` lives with the implementation in Plan 7.
- The `is_atomic_custom_node` registry for CustomNode types (Plan 7
  owns it).
- The metadata loader changes that would populate `ValueSource`
  anchors on `meta` / `var` shortcode resolutions — that's a separate
  follow-up (see "Deferred anchor role" and Plan 6's "ValueSource
  follow-up" section).
- Registering Lua filter files in `SourceContext` to enable typed
  `Dispatch` anchors. See "Deferred anchor role" below.

## Inherited pre-existing failure (bd-3odjm)

**One test in the workspace is expected to be red throughout Plan 4
and only goes green when Plan 5 ships its first reader change.** Do
not try to fix it inside Plan 4.

- Test: `cargo nextest run -p quarto-core --test idempotence lua_shortcode_lipsum_fixed`
  (orchestrator mode only; `SingleFile` passes).
- Symptom: panic with `MalformedSourceInfoPool` when
  `pampa::readers::json::read` re-parses the orchestrator's AST JSON.
- Root cause (already established): wire-format type-code-3
  collision — writer emits the new `FilterProvenance` payload
  `[filter_path, line]` under code 3, reader still decodes code 3
  as the legacy `Transformed` `[parent_id, ...]`.
- Owner: [Plan 5 — wire format](2026-05-04-q2-preview-plan-5-wire-format.md).

Plan 4's verification gate (Phase 7) and `cargo xtask verify`
therefore expect **exactly one** failing test in
`quarto-core::idempotence` (the test above) until Plan 5's first
reader fix lands. Any other failure is a Plan-4 regression and must
be triaged before continuing.

This is the integration branch's intended long-lived-red state per
Plan 3's §"Long-lived branch policy" — Plan 4 ships on top of that
queue, not in spite of it.

## Work items

Phase-ordered. Each phase compiles cleanly before the next begins.
"Settled" items below (design decisions, semantics rules) are detailed
later in the plan — this list is the actionable extract.

### Phase 1 — Type definitions in `quarto-source-map`

- [ ] Add `smallvec` to the workspace `Cargo.toml` (`[workspace.dependencies]`)
      with the `serde` feature, and depend on it from
      `crates/quarto-source-map/Cargo.toml`. Verified absent in both files
      at the start of Plan 4.
- [ ] Add `By` struct (`kind: String`, `data: serde_json::Value` with
      `#[serde(default, skip_serializing_if = "serde_json::Value::is_null")]`
      — the attribute path needs to be fully qualified, not the short
      `Value::is_null` form).
- [ ] Add `AnchorRole` enum (`Invocation`, `ValueSource`, `Other(String)`).
- [ ] Add `Anchor` struct (`role: AnchorRole`, `source_info: Arc<SourceInfo>`).
- [ ] Add `Generated { by: By, from: SmallVec<[Anchor; 2]> }` variant
      to `SourceInfo`. Keep `FilterProvenance` for now — it's removed
      at the end of Phase 5.
- [ ] Verify the new enum still implements `Debug`, `Clone`,
      `PartialEq`, `Serialize`, `Deserialize` (including with the
      `SmallVec` field — needs `serde` feature on `smallvec`).

### Phase 2 — Constructors and accessors

- [ ] `By::filter`, `By::sectionize`, `By::user_edit`, `By::shortcode`,
      `By::include`, `By::title_block`, `By::footnotes`, `By::appendix`,
      `By::tree_sitter_postprocess`, `By::raw`.
- [ ] `By::shortcode` doc-comment states the required-Invocation-anchor
      invariant (see §"Required-anchor invariant for `shortcode`" for
      the exact wording).
- [ ] `By::is_atomic_kind` (returns true for `filter | shortcode |
      title-block | tree-sitter-postprocess`).
- [ ] `By::is_kind`, `By::as_filter`.
- [ ] `Anchor::invocation`, `Anchor::value_source` constructors.
- [ ] `SourceInfo::generated(by)` constructor (empty `from`).
- [ ] `SourceInfo::invocation_anchor`, `SourceInfo::value_source_anchor`.
- [ ] `SourceInfo::anchors_with_role`, `SourceInfo::append_anchor`.

### Phase 3 — Update existing accessors for the `Generated` arm

- [ ] `length`, `start_offset`, `end_offset` → return `0` (in `source_info.rs`).
- [ ] `map_offset` → return `None` (in `mapping.rs`).
- [ ] `resolve_byte_range` → delegate to `invocation_anchor()` and recurse.
- [ ] `remap_file_ids` → walk `from`, recurse via `Arc::make_mut`.
- [ ] `extract_file_id` in `quarto-error-reporting/src/diagnostic.rs` →
      delegate to `invocation_anchor()` and recurse.

### Phase 4 — Lua serde

- [ ] Add `Generated` arm to `source_info_to_lua_table` in
      `pampa/src/lua/diagnostics.rs` (`t = "Generated"`, `by` and `from`
      sub-tables).
- [ ] Add `Generated` arm to `source_info_from_lua_table`.
- [ ] Keep `"FilterProvenance"` legacy reader: maps to
      `Generated { by: By::filter(path, line), from: smallvec![] }`.
      Indefinitely accepted; writes never emit it. The Concat arm
      already recurses through `source_info_from_lua_table`, so a
      legacy `"FilterProvenance"` table nested inside a `Concat` piece
      is handled automatically — no Concat-specific code path needed.
      (Verified: no `.snap` or `.json` file in `crates/` or `tests/`
      contains the `"FilterProvenance"` string today, so no fixture
      migration is required either.)

### Phase 5 — Migration

- [ ] Add deprecated alias `SourceInfo::filter_provenance(path, line)`
      that constructs the new `Generated` shape, so the migration can
      land in waves without breaking call sites.
- [ ] Sweep all `SourceInfo::FilterProvenance` references (15 files,
      27 occurrences across both literal constructions and pattern-match
      arms — verify with `git grep "SourceInfo::FilterProvenance"` at
      start). Construction sites → `SourceInfo::Generated { by:
      By::filter(...), from: smallvec![] }`. Pattern-match arms →
      `Generated { by, .. }` checking `by.as_filter()` where path/line
      is needed.
- [ ] Sweep `SourceInfo::filter_provenance(...)` constructor-function
      callers (5 files per `git grep "SourceInfo::filter_provenance("`)
      → either the new shape or the deprecated alias added above; once
      all callers are migrated, the alias can come out.
- [ ] Remove the `FilterProvenance` variant from `SourceInfo`.
- [ ] Remove the deprecated `SourceInfo::filter_provenance` alias.

### Phase 6 — Tests (see §Test plan for full descriptions)

Type / builder:
- [ ] Unit tests for every `By` builder (all 10 kinds incl. `raw`).
- [ ] `By::is_atomic_kind` coverage (atomic set + extension kinds).
- [ ] `By::is_kind` + `By::as_filter` coverage.
- [ ] Unit tests for `Anchor::invocation` / `Anchor::value_source`.
- [ ] JSON round-trip: `By`, `Anchor`, `Generated` (no anchors / with
      Invocation / multi-anchor).

Accessor tests on `Generated`:
- [ ] `length` / `start_offset` / `end_offset` for `Generated` → `0`.
- [ ] `map_offset` for `Generated` → `None`.
- [ ] `resolve_byte_range` recursion through `Invocation -> Substring`
      → resolves correctly; empty `from` and ValueSource-only `from`
      → `None`.
- [ ] `remap_file_ids` for `Generated` walks every anchor's source_info
      via `Arc::make_mut` (regression guard — must NOT be no-op).
- [ ] `extract_file_id` for `Generated` (in `quarto-error-reporting`)
      delegates to `invocation_anchor` and recurses.
- [ ] `invocation_anchor` coverage (present / absent / ValueSource-only).
- [ ] `value_source_anchor` coverage (parallel).
- [ ] `anchors_with_role` coverage (each known role + unknown role).
- [ ] `append_anchor` mutator coverage.

Structural:
- [ ] Rename `test_filter_provenance_tracking`
      (`filter_tests.rs:740-813`) and update assertions to the
      `Generated` shape with `by.as_filter()` recovery.
- [ ] `combine()` × `Generated` structural test (zero-length Concat
      piece, `map_offset` skips over it).
- [ ] Lua-serde round-trip including legacy `"FilterProvenance"` tag
      back-compat read.

### Phase 7 — Verification gate

- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace`: **exactly one** failure —
      `quarto-core::idempotence::lua_shortcode_lipsum_fixed`
      (bd-3odjm, owned by Plan 5). Any other failure is a Plan-4
      regression and must be triaged. See §"Inherited pre-existing
      failure (bd-3odjm)" above.
- [ ] `cargo xtask verify`: Step 5 trips on the same single
      bd-3odjm failure; every other step green (full — `quarto-source-map`
      is consumed by the WASM client, so the hub-build leg matters).
- [ ] `git grep "SourceInfo::FilterProvenance"` returns zero hits
      across `crates/` (variant gone).
- [ ] `git grep "SourceInfo::filter_provenance"` returns zero hits
      across `crates/` (deprecated alias gone).
- [ ] `git grep '"FilterProvenance"'` in Rust code returns only the
      legacy-Lua-reader arm and the legacy code-3 JSON-reader arm —
      no writer emissions, no other readers.

## Design decisions (settled in conversation)

- **Single `Generated` variant, not two.** Earlier drafts proposed
  `Synthetic` + `Derived` to separate "no preimage" from "has preimage
  but is atomic." The unified `Generated { by, from: SmallVec<[Anchor; 2]> }`
  expresses both with one variant: anchor-list empty for pure
  synthesis, anchor-list with `Invocation` for shortcode-style
  resolutions. The "has preimage" property is `gen.invocation_anchor().is_some()`,
  not a separate enum arm.
- **`by` records generator identity; `from` records source contributions.**
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
- **Filter constructions become `Generated { by: filter, from: [] }`**.
  `pandoc.Str("decoration")` in a Lua filter produces this shape (the
  Lua machinery's auto-attach replaces the existing FilterProvenance
  emission). Lua-file path and line live in `by.data` until
  Lua-file-registration lands; then they migrate to a `Dispatch` anchor.
- **Shortcode resolutions become `Generated { by: shortcode(name), from: [Invocation -> token_si] }`.**
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
- **Anchor list ordering is append order**. `from` is a `SmallVec`;
  iteration is insertion order. `append_anchor` pushes to the end.
  Accessors that find by role (`invocation_anchor`, `value_source_anchor`)
  return the first match — at most one anchor per known role by
  convention. Serde round-trips preserve order. No producer sorts;
  no consumer reorders.
- **Builder methods for known kinds, plus `raw` escape hatch**.
  `By::raw(kind, data)` accepts any `kind` string — including built-in
  names like `"shortcode"` or `"filter"`. Forgery (an extension calling
  `By::raw("shortcode", …)` without the required Invocation anchor)
  is caught downstream by Plan 6's audit-completion test and Plan 7's
  `debug_assert!`, so no constructor-level rejection is needed. The
  convention is still `ext/<extension>/<kind>` for third-party kinds —
  collisions with built-ins are a misuse caught at audit time, not a
  type error.

## The proposed shape

**Naming.** Read the new variant as: this node was generated **by** some
transform, **from** some anchors. `by` records the producer; `from` is
the list of `Anchor`s that record the source-side contributions. The
items in the list are `Anchor` values; methods that operate on individual
items keep "anchor" in their name (`invocation_anchor`,
`value_source_anchor`, `append_anchor`, `anchors_with_role`), while the
field name and any Lua-table key use `from`. `by` / `from` reads cleanly
in both Rust and Lua serializations — preserve that pairing throughout.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SourceInfo {
    Original { file_id: FileId, start_offset: usize, end_offset: usize },
    Substring { parent: Arc<SourceInfo>, start_offset: usize, end_offset: usize },
    Concat { pieces: Vec<SourcePiece> },
    Generated { by: By, from: SmallVec<[Anchor; 2]> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct By {
    /// Short kind tag, kebab-case. Examples: "filter", "shortcode",
    /// "sectionize", "user-edit", "title-block".
    /// Third-party kinds should namespace: "ext/my-extension/foo".
    pub kind: String,

    /// Per-kind configuration that is NOT a source-info pointer.
    /// Anchors live in `Generated.from`, not here.
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
    /// anchor-presence. A `Generated { by: shortcode, from: [...] }`
    /// is atomic; so is a `Generated { by: filter, from: [] }`.
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
        SourceInfo::Generated { by, from: SmallVec::new() }
    }
}

// Helper methods on Generated-shape access — typically called via
// matching `SourceInfo::Generated { by, from } => ...`. We provide
// the helpers as free functions on the variant pattern; example:

impl SourceInfo {
    /// If this is `Generated`, return the first anchor whose role is
    /// `Invocation`. Returns `None` otherwise (including for
    /// non-`Generated` variants).
    pub fn invocation_anchor(&self) -> Option<&Arc<SourceInfo>> {
        match self {
            SourceInfo::Generated { from, .. } => from
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
            SourceInfo::Generated { from, .. } => from
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
  producer; `from` records any source-side contributions. The
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
both via `Generated { by, from }`.

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
| Sectionize Section Div | No | No (structural grouping) | `Generated { by: sectionize, from: [] }` |
| Footnotes container Div | No | No (structural grouping) | `Generated { by: footnotes, from: [] }` |
| Appendix wrapper Div | No | No (structural grouping) | `Generated { by: appendix, from: [] }` |
| Title-block synthesized h1 | No | No (synthesized from `title:` YAML) | `Generated { by: title_block, from: [] }` |
| Tree-sitter postprocess Space | No | No (inserted between nodes) | `Generated { by: tree_sitter_postprocess, from: [] }` |
| Shortcode resolution output | No | No (resolved from value, distinct from token bytes) | `Generated { by: shortcode("…"), from: [Invocation, …] }` |
| Filter-constructed node | No | No (filter computed it) | `Generated { by: filter, from: [] }` (Dispatch anchor in the future) |

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

Atomicity is per-kind, orthogonal to `from`. A `Generated { by: shortcode,
from: [Invocation -> token_si] }` is atomic; so is a
`Generated { by: filter, from: [] }`. The writer's coarsen
(Plan 7) consults `by.is_atomic_kind()` and `gen.invocation_anchor()`
independently.

Extensions that contribute new `by.kind` values are not atomic by
default. If an extension wants its kind to be atomic, the
`is_atomic_kind()` predicate (or a follow-up extension-registration
mechanism — see Plan 7 §Open questions) needs to recognize it. v1
hardcodes the built-in set.

### Required-anchor invariant for `shortcode`

A `Generated { by: shortcode(...), from: [] }` is **not a valid state**.
Every shortcode-resolution node must carry at least one `Invocation`
anchor pointing at the source token's byte range. The resolver
(Plan 6) is responsible for maintaining this invariant; downstream
consumers (Plan 7's writer, error-reporting) may assume it.

Plan 4 documents the invariant; enforcement is split across the two
producers/consumers of the shape:

- **Plan 6 (producer)** owns the audit-completion test that walks the
  post-stamping AST and asserts no `Generated { by: shortcode, from: [] }`
  remains. The stamper is the only construction site for `by: shortcode`
  in v1; the test verifies it always attaches the `Invocation` anchor.
- **Plan 7 (consumer)** adds a `debug_assert!` in `coarsen`'s
  atomic-no-anchor branch. The writer routes "atomic + no invocation"
  to `Omit` (drop the node, pipeline regenerates next run); for filter
  that's correct, for shortcode it's silent data loss — the assertion
  catches the bad shape before that branch fires, in dev / test builds.

No constructor-level enforcement in v1. The `By::shortcode(name)`
builder stays symmetric with the other `By::xxx()` builders; the
required-anchor invariant is a *resolver* invariant, not a *type*
invariant. If a second required-anchor rule appears later, promote
the audit assertion into a shared validator pass.

The `By::shortcode` doc-comment must state the invariant explicitly,
so anyone reaching for the builder from a new call site reads:

```rust
/// Construct a `By` for a shortcode resolution.
///
/// **Invariant.** Every `Generated { by: shortcode(...), .. }` must
/// carry at least one `Invocation` anchor in `from` pointing at the
/// source token's byte range. Use only inside a `Generated` whose
/// anchor list is populated; constructing the bare shape with empty
/// `from` is rejected by Plan 6's audit-completion test and trips
/// Plan 7's writer `debug_assert!`.
pub fn shortcode(name: impl Into<String>) -> Self { ... }
```

## Migrations

The pre-existing `FilterProvenance` is renamed/folded:

- **Construction**: `SourceInfo::filter_provenance("path", 42)` →
  `SourceInfo::Generated { by: By::filter("path", 42), from: smallvec![] }`.
  The `(filter_path, line)` pair lives in `by.data` until
  Lua-file-registration lands.
  Add a deprecated alias `SourceInfo::filter_provenance` that
  constructs the new shape; remove after migration completes.
- **Pattern-match**: every `SourceInfo::FilterProvenance { filter_path, line }`
  arm becomes `SourceInfo::Generated { by, .. }` and inspects via
  `by.as_filter()` to recover the path/line.
- **Lua serde**: read `"FilterProvenance"` tag (legacy) and reconstruct
  as `Generated { by: By::filter(...), from: smallvec![] }`. New
  constructions emit `"Generated"` tag with `by` and `from` sub-tables
  (per §In scope).

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

The migration applies to **both** affected kinds, symmetrically:

| kind | shape today | shape after Lua-file-registration |
|---|---|---|
| `filter` | `Generated { by: filter{path, line}, from: [] }` | `Generated { by: filter{}, from: [Dispatch -> lua_si] }` |
| `shortcode` (Lua handler) | `Generated { by: shortcode{name, lua_path, lua_line}, from: [Invocation -> token_si] }` | `Generated { by: shortcode{name}, from: [Invocation -> token_si, Dispatch -> lua_si] }` |
| `shortcode` (Rust handler) | `Generated { by: shortcode{name}, from: [Invocation -> token_si] }` | unchanged (no Lua source to point at) |

A Lua-handler shortcode after registration carries **two** anchors —
`Invocation` for the user-written token, `Dispatch` for the Lua
handler that resolved it. The anchor list is what makes this clean:
adding `Dispatch` doesn't disturb `Invocation`, and the writer's
preimage walk (Plan 7) still looks at `invocation_anchor()` only.

Tracked as **bd-36fr9** ("Provenance follow-up: Dispatch anchor for
Lua-handler filter & shortcode").

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

Tracked as **bd-129m3** ("Provenance follow-up: ValueSource anchor
stamping for meta/var shortcodes").

Both follow-ups are pure additions when they land — neither requires
reopening Plan 4's type design. The shape is forward-compatible by
construction.

## Resolve-byte-range semantics

`resolve_byte_range` is Plan 4's responsibility (existing accessor on
`SourceInfo`, gains a `Generated` arm). `preimage_in` is Plan 7's —
Plan 4 only ships the building block it depends on, `invocation_anchor()`.

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
}
```

The `Generated` arm collapses to "look up the invocation anchor;
recurse into its source_info." Pure synthesis (empty `from`) returns
`None`. Multi-anchor Generateds (when `ValueSource` lands) still only
consult `Invocation` — `ValueSource` is diagnostic-only.

Plan 7's `preimage_in` follows the same `Generated` pattern (it
delegates to `invocation_anchor()`); see Plan 7 §"`preimage_in`
semantics" for the full implementation including Concat contiguity.

## References

- `crates/quarto-source-map/src/source_info.rs:21-55` — current
  `SourceInfo` enum (incl. `FilterProvenance` variant at lines 49-54).
- `crates/quarto-source-map/src/source_info.rs:162-233` — accessors that
  need updating (`length`, `start_offset`, `end_offset`,
  `resolve_byte_range`, `remap_file_ids`).
- `crates/quarto-source-map/src/mapping.rs:17-74` — `map_offset`
  recursion; needs `Generated` arm (returns `None`, like
  `FilterProvenance` does today).
- `crates/quarto-error-reporting/src/diagnostic.rs:556-575` —
  `extract_file_id` traversal; needs a `Generated` arm that delegates
  to `invocation_anchor()` and recurses (parallel to
  `resolve_byte_range`).
- `crates/pampa/src/lua/diagnostics.rs:50-145` — Lua serde to extend.
- `crates/pampa/src/lua/filter_tests.rs:740-813` — `test_filter_provenance_tracking`; rename and update assertions to the `Generated` shape.
- `crates/quarto-pandoc-types/src/custom.rs:75` — `CustomNode.plain_data`
  (the prior-art for `serde_json::Value` at extension seams; same
  convention now applies to `By.data`).
- `crates/quarto-core/src/artifact.rs:71` — `Artifact.metadata`
  (second precedent for the same pattern).

## Test plan

### Type / builder tests

- Unit tests for each `By` builder method (constructs the right kind
  and data). Cover all ten: `filter`, `sectionize`, `user_edit`,
  `shortcode`, `include`, `title_block`, `footnotes`, `appendix`,
  `tree_sitter_postprocess`, `raw`.
- `By::is_atomic_kind()` test: confirms the set named in §"Atomic-kind
  set" returns `true` exactly for `filter | shortcode | title-block |
  tree-sitter-postprocess` and `false` for everything else (including
  extension `ext/…/…` kinds).
- `By::is_kind()` / `By::as_filter()` coverage.
- Unit tests for `Anchor::invocation()` / `Anchor::value_source()`
  constructors.
- Round-trip test: `By` → JSON → `By` (serde derive).
- Round-trip test: `Anchor` → JSON → `Anchor` (serde derive).

### Accessor tests on `Generated`

- `length()` / `start_offset()` / `end_offset()` for `Generated`
  return `0` regardless of `from` contents.
- `map_offset()` for `Generated` returns `None` regardless of offset
  argument.
- `resolve_byte_range()` recursion: a
  `Generated { from: [Invocation -> Substring{parent: Original{42, 100, 200}, 10, 20}] }`
  resolves to `(42, 110, 120)`. A `Generated` with empty `from` returns
  `None`. A `Generated` with only a `ValueSource` anchor (no
  `Invocation`) returns `None`. (Plan 7 owns the matching `preimage_in`
  tests.)
- `remap_file_ids()` for `Generated`: build a
  `Generated { from: [Invocation -> Original{FileId(0), …}, ValueSource -> Original{FileId(3), …}] }`,
  apply `|id| FileId(id.0 + 10)`, assert both anchors' source_info
  carry remapped FileIds. This catches the "no-op like FilterProvenance"
  regression — `Generated` must NOT be a no-op since it can hold FileIds.
- `extract_file_id()` (in `quarto-error-reporting`) for `Generated`
  delegates to `invocation_anchor()` and recurses. Mirrors
  `resolve_byte_range`'s test surface.
- `invocation_anchor()` accessor: a Generated with `[Invocation -> X]`
  returns `Some(X)`; with `[]` returns `None`; with `[ValueSource -> Y]`
  (no Invocation) returns `None`.
- `value_source_anchor()` accessor: parallel coverage.
- `anchors_with_role()` accessor: a Generated with
  `[Invocation -> X, ValueSource -> Y, Other("foo") -> Z]` returns the
  right anchors for each role, and an empty iterator for an unknown role.
- `append_anchor()` mutator: starting from `Generated { from: [] }`,
  append an Invocation then a ValueSource; assert both are present in
  order.

### Structural tests

- Integration test: filter-provenance test renamed from
  `test_filter_provenance_tracking` (at `filter_tests.rs:740-813`)
  confirms a filter-created Str gets `Generated { by: filter, from: [] }`
  with `(filter_path, line)` recoverable via `by.as_filter()`.
- `combine()` × `Generated` structural test: combining an `Original`
  with a `Generated` produces a `Concat` whose Generated piece has
  length `0` (matches `Generated::length()`). `map_offset` over the
  combined Concat skips the Generated piece. This pins behavior even
  though no production code path combines Generated source_info today.
- Lua-serde round-trip: typed → Lua table → typed, including legacy
  `"FilterProvenance"` tag back-compat (reads as `Generated { by:
  filter, from: [] }`; never round-trips back to `FilterProvenance`).

## Dependencies

- Depends on: nothing (pure type change in the foundation crate).
- Blocks: Plan 5 (wire format extension), Plan 6 (provenance audit),
  Plan 7 (writer's preimage walk uses Generated and the
  `invocation_anchor` helper).

## Risk areas

- **Migration scope**: 15 files pattern-match `SourceInfo::FilterProvenance`
  (27 occurrences total — verified by grep against the worktree). Each
  needs migration arms for `Generated`. Most are mechanical: the
  `Generated` arm returns what `FilterProvenance` did today (usually
  `0`, `0`, or `None`) for offset/length accessors, or delegates to
  `invocation_anchor()` for `resolve_byte_range` / `extract_file_id`.
- **Anchor-list allocation**: `from` is typed `SmallVec<[Anchor; 2]>`
  from day 1 (with the `serde` feature enabled). Inline capacity of 2
  covers all expected shapes through the deferred follow-ups with zero
  heap allocation:
    - empty (sectionize / footnotes / appendix / title-block /
      tree-sitter-postprocess / filter constructions today) — the bulk
      of synthesized nodes;
    - one Invocation (Rust-handler shortcode resolutions, today);
    - two anchors (Invocation + ValueSource for `meta`/`var` once
      bd-129m3 lands; Invocation + Dispatch for Lua-handler shortcodes
      once bd-36fr9 lands).
  Cap=2 costs +16 bytes per `Generated` over cap=1 even when `from` is
  empty, but eliminates the heap spill cap=1 would incur on every
  multi-anchor shortcode in the steady state. Three-or-more-anchor
  Generateds (Invocation + ValueSource + Dispatch on a Lua-handler
  `meta` shortcode) still spill — same cost as `Vec<Anchor>` would have
  been. Adds a `smallvec` workspace dependency (verified absent today).
- **`serde_json::Value` in PartialEq derives**: `Value` implements
  `PartialEq` but with potentially weird semantics for floats. For our
  use, kinds carry string + small structured data; should be fine.
  Test the cases. (Verified: no production call site relies on
  `SourceInfo == SourceInfo` today — the `PartialEq` derive is required
  by the wider `Block`/`Inline` derives but isn't itself load-bearing.
  Plan 7's coarsen may compare structurally once it lands; the
  `Value::PartialEq` semantics on small kebab-case objects are
  well-behaved.)
- **Removing `FilterProvenance` is a breaking change for downstream
  consumers**. Within the q2 workspace this is bounded; if any external
  code imports the variant by name, they'd break. Search for
  non-workspace usages before removing (probably none).
- **`Default` on containers of `SourceInfo`**: verified no struct in
  `quarto-pandoc-types/src/{block,inline}.rs` derives `Default` (each
  `SourceInfo`-bearing struct is constructed explicitly), so changing
  `SourceInfo`'s arm set can't cascade into a broken
  `#[derive(Default)]`. The hand-written `Default for SourceInfo` impl
  (the `Original { FileId(0), 0, 0 }` zero-value) stays unchanged.
- **`combine()` with a `Generated` operand**: structurally valid (it
  produces a `Concat` with a zero-length `Generated` piece, since
  `Generated::length()` returns `0`), but semantically dead — the
  Generated side carries no preimage bytes for adjacent-text coalescing,
  and `map_offset` will skip over the zero-length piece. Verified: all
  17 `.combine(` call sites in the workspace (`attr.rs`,
  `postprocess.rs`, `location.rs`, `yaml/parser.rs`, etc.) combine
  Original/Substring shapes; nothing combines FilterProvenance today, so
  Generated won't be combined either unless a future transform reaches
  for it. The Phase 6 `combine() × Generated` test documents the
  intended fall-through behavior for any future caller, not a current
  regression. No type-level prevention in v1.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `Generated` variant + `Anchor` + `AnchorRole` types | ~80 |
| Accessors (invocation_anchor, value_source_anchor, etc.) | ~60 |
| `By` struct + builders + `is_atomic_kind` | ~120 |
| `resolve_byte_range` / `map_offset` / `remap_file_ids` / `extract_file_id` updates | ~50 |
| Pattern-match migrations (15 files, 27 occurrences) | ~180 |
| FilterProvenance construction site migrations | ~30 |
| Lua serde extension + back-compat | ~80 |
| Test updates and new tests | ~250 |
| **Total** | **~850** |

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
