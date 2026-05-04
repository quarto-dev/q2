# Plan 7 — Incremental writer preimage walk + Transparent + atomic-violation + multi-inline dedupe

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M3 (edit-back works for non-include, non-pure-synthesis edits)

## Goal

Teach the incremental writer (`pampa::writers::incremental`) to handle the
new provenance shapes introduced by Plans 4-6 so that q2-preview round-trip
edits work correctly. Five new behaviors:

- **`preimage_in(target_file_id)` accessor**: a recursive walk through
  Substring/Concat/Derived chains that returns the byte range in the target
  file IF the chain resolves there, else None.
- **`Transparent` coarsen variant**: for `KeepBefore` nodes whose source_info
  is `Synthetic` but whose children have recoverable preimages (Sectionize's
  case), recurse into the children rather than emit a useless empty
  Verbatim. The wrapper itself contributes nothing to the output.
- **Atomic detection via `Derived`**: nodes with `Derived` source_info are
  atomic. KeepBefore + Derived → Verbatim copies the preimage (the shortcode
  token, etc.). UseAfter or RecurseIntoContainer touching a Derived node →
  AtomicViolation.
- **Atomic detection via `is_atomic_custom_node`**: `IncludeExpansion`
  CustomNode is atomic via type_name lookup. Same outcome as Derived case
  (KeepBefore Verbatim; anything else → AtomicViolation). Plus
  `CrossrefResolvedRef` is atomic (already a CustomNode in the AST).
- **Multi-inline dedupe rule**: when assembling a run of consecutive inlines
  (in InlineSplice or inline assembly contexts) that all share the same
  Derived source_info `from`, emit Verbatim *once* for the group rather
  than N times. This handles multi-inline shortcode resolutions.

This plan also adds a `pipeline_kind: Option<String>` parameter to
`incremental_write_qmd` (per Decision D — param with default) that runs the
q2-preview pipeline on the baseline AST before reconciling, making the
reconcile symmetric. Existing callers pass `None` and get today's
parse-only baseline behavior; q2-preview's call site passes
`Some("preview")`.

When this plan lands, ReactPreview's read-only guard from Plan 1 lifts, and
edits in q2-preview round-trip correctly.

## Scope

### In scope

- `preimage_in` accessor on `SourceInfo` (in `quarto-source-map`). Walks
  Substring's `parent`, Concat's `pieces`, Derived's `from`. Returns
  `Some(byte_range)` if the chain resolves to an `Original` in the target
  file, else `None`.
- `coarsen` rules:
  - **Verbatim**: KeepBefore + preimage_in resolves into target file.
  - **Transparent (recurse)**: KeepBefore + Synthetic source_info + block
    has children with recoverable preimages. Recurse on children, produce
    a child-entry list. Wrapper itself emits nothing.
  - **Omit**: KeepBefore + Synthetic + no recoverable children. Skip entirely
    (rare; pure-synthesis filter output that survived structurally identical
    between baseline and live).
  - **Rewrite**: UseAfter or Recurse-with-changes for non-atomic blocks.
  - **AtomicViolation**: any non-KeepBefore alignment touching:
    - a `Derived` source_info node (shortcode resolutions etc.), OR
    - a CustomNode whose `type_name` is in `is_atomic_custom_node`'s set.
    Produces a `Q-WRITER-ATOMIC-MODIFIED` diagnostic + aborts the write.
- `is_atomic_custom_node` registry, defined in **`quarto-core`** as
  `pub fn is_atomic_custom_node(type_name: &str) -> bool`. Single source
  of truth, called by:
  - the writer (in `pampa`, via the `quarto-core` dependency) for atomic
    detection in coarsen;
  - Plan 8 (also in `quarto-core`) which adds `IncludeExpansion` to the
    registry's initial set;
  - Plan 2's React layer (which reads the same set, JSON-serialized at
    build time or hard-coded as a TS constant matching the Rust list —
    confirm during implementation; the cleanest is a build-step that
    emits the list as a TS `.ts` module so a single edit propagates).
  Initial set: `["IncludeExpansion", "CrossrefResolvedRef"]`. Looked up
  by `type_name`. Note: `ShortcodeResolution` is NOT in this set —
  shortcode atomicity is handled via the `Derived` source_info path,
  not via a wrapper. Future extensions can register their own atomic
  types via a registration mechanism that's not in scope for v1.
- `assemble`:
  - Walks Transparent entries by emitting each child's bytes with separators
    computed from the children's original positions.
  - AtomicViolation entries are collected as diagnostics; the assembly
    produces an Err.
  - Inline-level dedupe: within an inline-splice or inline-assembly run,
    detect consecutive inlines sharing the same Derived `from` and emit
    one Verbatim (the from's preimage range) instead of N.
- `pipeline_kind` parameter added to `incremental_write_qmd`. When
  `Some("preview")`:
  - Re-parses `original_qmd` (today's behavior).
  - **Runs the q2-preview transform pipeline on the baseline** (this is the
    NEW step). Produces a baseline AST at the same pipeline tier as the
    live AST.
  - Reconciles new vs baseline.
  - Writes via the updated coarsen/assemble logic.
- Lift the `handleSetAst` read-only guard in `ReactPreview.tsx` introduced
  in Plan 1. Wire `setLocalAst` through with `pipeline_kind: "preview"`.

### Out of scope

- Include round-trip via wrapper-CustomNode (Plan 8 — uses this plan's
  AtomicViolation logic but introduces the wrapper itself).
- Engine output as Derived (deferred future work).

## Design decisions (settled in conversation)

- **Sectionize's transparent recurse pattern**: `Synthetic` wrappers with
  source-bearing children get the Transparent treatment. Children's bytes
  are contiguous in source (Sectionize doesn't reorder), so emitting them
  in order produces the right output. The wrapper emits nothing.
- **Atomic detection has two paths**:
  1. **Derived source_info** (used for shortcode resolutions). Any node
     whose `source_info` is `Derived` is atomic. KeepBefore Verbatims the
     preimage; anything else is an AtomicViolation.
  2. **Atomic CustomNode types** (used for IncludeExpansion, CrossrefResolvedRef).
     Looked up via `is_atomic_custom_node(&type_name) -> bool`. Same
     KeepBefore-Verbatim-only contract.
- **Why two paths**: shortcode resolutions don't get a wrapper (we
  decided wrappers were too heavy for shortcodes); they propagate
  atomicity via `Derived` source_info on the resolved nodes. Includes
  use a wrapper because of the cross-file FileId issue (the included
  blocks live in another file; we need an anchor in the parent file).
- **The error is loud**: a `Q-WRITER-ATOMIC-MODIFIED` diagnostic with the
  affected node's source_info, a clear actionable message, and an aborted
  write. Better than silently materializing. React (Plan 2) is the
  primary safeguard (no editing affordance); the writer is the contract
  guarantor.
- **Multi-inline shortcode dedupe**: a multi-inline shortcode resolution
  produces several inlines all sharing the same Derived `from`. The
  writer's inline-assembly path needs to detect this and emit Verbatim
  *once* for the group. Without this, the assembly emits the shortcode
  token N times.
- **Param-with-default for `incremental_write_qmd`** (Decision D): add a
  `pipeline_kind: Option<String>` parameter. `None` = current behavior
  (parse-only baseline). `Some("preview")` = run q2-preview pipeline on
  baseline. Existing callers (q2-debug demos, sync client, ReactPreview's
  q2-debug path) continue to work unchanged.

## The coarsen logic

```
fn is_atomic(block: &Block) -> bool {
    match block.source_info() {
        SourceInfo::Derived { .. } => true,
        _ => {}
    }
    match block {
        Block::Custom(cn) if is_atomic_custom_node(&cn.type_name) => true,
        _ => false,
    }
}

For each block alignment from the reconciler:

if alignment is KeepBefore(orig_idx):
    let original_block = original_ast.blocks[orig_idx];
    if is_atomic(original_block) {
        // Atomic types must be KeepBefore.
        // Today's reconciler reports KeepBefore only when structurally
        // identical including children. So this branch is the success path.
        if let Some(range) = original_block.source_info().preimage_in(target_file) {
            CoarsenedEntry::Verbatim { byte_range: range, orig_idx }
        } else {
            // Should not happen for atomic types — they're built by transforms
            // that set source_info to a real preimage-bearing range
            // (Original or Derived from Original).
            CoarsenedEntry::AtomicViolation { node_si: original_block.source_info() }
        }
    }
    else if let Some(range) = original_block.source_info().preimage_in(target_file) {
        CoarsenedEntry::Verbatim { byte_range: range, orig_idx }
    }
    else if matches!(original_block.source_info(), SourceInfo::Synthetic { .. })
        && original_block has children
    {
        // Sectionize/etc. — Transparent recurse.
        CoarsenedEntry::Transparent { child_entries: <recurse on children> }
    }
    else {
        // Synthetic with no children, or some other shape with no preimage.
        CoarsenedEntry::Omit
    }

if alignment is UseAfter(new_idx):
    let new_block = new_ast.blocks[new_idx];
    if is_atomic(new_block) {
        // Atomic types added by the user are unusual; treat as error for now.
        // (Could relax later if a use case appears.)
        CoarsenedEntry::AtomicViolation { node_si: new_block.source_info() }
    } else {
        CoarsenedEntry::Rewrite { new_idx }
    }

if alignment is RecurseIntoContainer { before_idx, after_idx }:
    let original_block = original_ast.blocks[before_idx];
    if is_atomic(original_block) {
        // Atomic node must not have inner changes.
        CoarsenedEntry::AtomicViolation { node_si: original_block.source_info() }
    } else {
        // Existing recurse logic for inline plans, custom_node_plans, etc.
        ...
    }
```

The same `is_atomic` check applies at the inline level for Derived inlines
(shortcode-resolved Strs, etc.). A shortcode-resolved Str inside a Para
that the user edited elsewhere: KeepBefore for the Str → Verbatim; UseAfter
for an edited shortcode-resolved Str → AtomicViolation.

The `assemble` step iterates coarsened entries:

- Verbatim → copy byte range from `original_qmd`.
- Rewrite → use the qmd writer to serialize the new block.
- InlineSplice → existing splice logic, extended with the dedupe rule
  for consecutive inlines sharing the same Derived `from`.
- Transparent → emit children's bytes recursively.
- Omit → skip.
- AtomicViolation → collect as diagnostic; abort the write at the end.

If any AtomicViolation entries were collected, the function returns
`Err(Vec<DiagnosticMessage>)` instead of `Ok(String)`.

## Multi-inline shortcode dedupe

When `{{< meta foo >}}` resolves to multiple inlines (e.g., metadata is
markdown like `**Bold** Title` → `[Strong[Str], Space, Str]`), each
resolved inline has the same `Derived { from: Original{shortcode_range},
by: By::shortcode("meta") }` source_info.

Block-level: if both pipeline runs produce the same multi-inline output,
the surrounding Para is structurally identical → KeepBefore at block
level → Verbatim copy of the WHOLE Para's bytes (including the shortcode
token). One copy. ✓

Inline-level recursion (when the user edits something else in the same
Para): the reconciler picks `RecurseIntoContainer` with an inline plan.
Each shortcode-derived inline is `KeepBefore` individually. Without
dedupe, each one's Verbatim emits the shortcode token → N copies in
output.

Dedupe rule: when iterating inline alignments in
`assemble_inline_content`, group consecutive `KeepBefore` entries whose
inlines share the same `Derived` source (compare the `Arc<SourceInfo>`
identity of `from`, or by structural equality of the `from` value). Emit
Verbatim *once* for the group, using the `from`'s preimage byte range.

This applies only at the inline level (where multi-inline shortcode
resolutions occur). Block-level rarely sees this case.

## `preimage_in` semantics

```rust
impl SourceInfo {
    pub fn preimage_in(&self, target: FileId) -> Option<Range<usize>> {
        match self {
            SourceInfo::Original { file_id, start_offset, end_offset }
                if *file_id == target =>
                Some(*start_offset..*end_offset),
            SourceInfo::Original { .. } =>
                None,
            SourceInfo::Substring { parent, start_offset, end_offset } => {
                let parent_range = parent.preimage_in(target)?;
                Some(parent_range.start + start_offset .. parent_range.start + end_offset)
            }
            SourceInfo::Concat { pieces } => {
                // All pieces must resolve into target file AND be contiguous.
                let ranges: Vec<_> = pieces.iter()
                    .map(|p| p.source_info.preimage_in(target))
                    .collect::<Option<Vec<_>>>()?;
                if ranges.is_empty() { return None; }
                // Confirm contiguous: ranges[i].end == ranges[i+1].start
                if ranges.windows(2).all(|w| w[0].end == w[1].start) {
                    Some(ranges.first()?.start .. ranges.last()?.end)
                } else {
                    None  // gappy concat — can't Verbatim-copy
                }
            }
            SourceInfo::Synthetic { .. } => None,
            SourceInfo::Derived { from, .. } => {
                // Walk through the `from` chain to find a preimage in the target.
                from.preimage_in(target)
            }
        }
    }
}
```

The `Derived` case delegates to `from`, which usually resolves to an
`Original` covering the source token bytes. So a `Derived` shortcode
resolution successfully returns its preimage range; the writer Verbatim
copies the shortcode token from source.

## Open questions for implementation

- **Inline-level Transparent**: today the writer has `InlineSplice` for
  inline-level changes within a block. Does Transparent apply to inlines
  too (e.g., a `Span` with Synthetic source_info containing source-bearing
  inlines)? Probably yes — extend the same pattern. Confirm during
  implementation.
- **Concat-with-gaps**: if a Concat's pieces resolve to non-contiguous
  ranges, `preimage_in` returns None per the algorithm above. Coarsen
  falls through to Rewrite. Confirm this is the right semantics.
- **The `is_atomic_custom_node` lookup**: today's hardcoded function works.
  Future extensions might want to register their own atomic types. Defer
  the registration mechanism — add when needed.
- **Diagnostic content for AtomicViolation**: should include the wrapper's
  source range, the include/shortcode's name, and a clear actionable
  message ("To edit this region, modify [foo.qmd / the YAML title key]
  directly").
- **Sibling vs param**: Decision D was "param with default" but Plan 4 / 7
  could implement it either way. Confirm during implementation. Param is
  cleaner (one fewer entry point). Sibling is more isolated. Either works.

## References

- `crates/pampa/src/writers/incremental.rs` — the writer to modify.
  Particularly `coarsen` (line 149), `assemble` (line 228), `compute_separator`
  (line 354), `block_source_span` (line 447), the helper for inline byte
  ranges (line 800).
- `crates/quarto-source-map/src/source_info.rs:185-237` — accessor patterns
  to extend.
- `crates/wasm-quarto-hub-client/src/lib.rs:2166` — `incremental_write_qmd`
  entry point to extend.
- `hub-client/src/services/wasmRenderer.ts:531` — the JS wrapper.
- `hub-client/src/components/render/ReactPreview.tsx` — `handleSetAst`
  guard to lift.
- Plans 4 (Synthetic + By), 5 (wire format), 6 (audit) — provide the
  AST shape this plan walks.

## Test plan

- **`preimage_in` unit tests**: each variant (Original same/different file,
  Substring chain, Concat contiguous/gappy, Synthetic, Derived). Assert
  correct byte range or None.
- **Coarsen unit tests**: build mock reconciliation plans + ASTs covering:
  - Verbatim (KeepBefore + preimage in target).
  - Transparent (KeepBefore + Synthetic wrapper with source-bearing children
    — Sectionize case).
  - Omit (Synthetic with no recoverable children).
  - Rewrite (UseAfter, non-atomic).
  - AtomicViolation via Derived (KeepBefore on Derived works; UseAfter on
    Derived → AtomicViolation).
  - AtomicViolation via is_atomic_custom_node (KeepBefore on
    IncludeExpansion works; RecurseIntoContainer or UseAfter →
    AtomicViolation).
- **Multi-inline dedupe unit tests**: build a Para with three consecutive
  inlines all sharing the same Derived `from`. Reconcile against an
  identical Para. Assert the writer emits the shortcode token bytes
  ONCE, not three times, in the inline-assembly output.
- **End-to-end round-trip tests**:
  - Sectionized doc → edit one paragraph → assert the section structure is
    preserved verbatim except for the edit.
  - Doc with single-inline shortcode (`{{< meta title >}}`) → edit a
    different paragraph → assert the shortcode token is preserved.
  - Doc with multi-inline shortcode (markdown title) → edit a different
    paragraph in same Para → assert the shortcode token appears once,
    not multiple times.
  - Doc with shortcode → attempt to edit the resolved title → assert
    `Q-WRITER-ATOMIC-MODIFIED` diagnostic.
  - (Plan 8 covers includes; this plan establishes the infrastructure.)
- **Idempotence holds**: re-run Plan 3's idempotence test after this plan
  lands. The AST shape changes from this plan's transforms shouldn't break
  it.

## Dependencies

- Depends on: Plans 4 (Synthetic + Derived + By), 5 (wire format), 6
  (audit + Derived provenance on shortcode resolutions).
- Blocks: nothing structurally; Plan 8 builds on the atomic infrastructure
  but is independent (uses `is_atomic_custom_node` for IncludeExpansion).
- Lifts the read-only mode that Plan 1 introduced for q2-preview.

## Risk areas

- **`incremental.rs` is intricate**: ~1000 lines, many interlocking
  functions. Adding new coarsen variants and rewiring assemble carefully
  is the meat of this plan. Budget extra time for edge cases.
- **Plan 4 / 5 / 6 must land first**. The writer can't test Synthetic
  walking without those types existing. Order matters strictly.
- **InlineSplice + Transparent interaction**: the existing InlineSplice
  logic handles inline-level changes. If Transparent at the block level
  recurses into a block whose inlines need splicing, the assembly logic
  composes both. Test this case — it's the trickiest edge.
- **AtomicViolation surfacing**: the diagnostic needs to reach the user via
  the existing diagnostics channel. ReactPreview displays diagnostics in
  the editor; confirm AtomicViolation diagnostics flow through.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `preimage_in` accessor (with Derived) + tests | ~100 |
| New `CoarsenedEntry` variants (Transparent, Omit, AtomicViolation) | ~30 |
| `coarsen` logic update (atomic detection: Derived OR is_atomic_custom_node) | ~150 |
| `assemble` updates (Transparent walk, AtomicViolation collection) | ~100 |
| Multi-inline shortcode dedupe rule in inline assembly | ~40 |
| `is_atomic_custom_node` registry | ~30 |
| `pipeline_kind` parameter + WASM bridge + TS wrapper | ~80 |
| ReactPreview guard lift + edit-back wiring | ~20 |
| Tests (unit + end-to-end round-trip) | ~350 |
| **Total** | **~900** |

Two focused sessions likely. Flagged as one of the highest-complexity plans;
extend the budget if the InlineSplice + Transparent composition surfaces
unexpected interactions.

## Notes

This is the most intricate plan in the set. It's the keystone for M3 — once
this lands, q2-preview is truly editable for the common case. Take care
with the test coverage; round-trip bugs in the writer can corrupt source
silently if not caught.

Per the user's decision: AtomicViolation is a hard error, not a
materialization. The user's exact words: "this edit has to be prohibited."
The diagnostic message should reflect that prohibition is intentional —
not a TODO, not a future improvement, but the contract.
