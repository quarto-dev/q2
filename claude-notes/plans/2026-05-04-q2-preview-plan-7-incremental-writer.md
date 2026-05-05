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
- `coarsen` rules. Two new entry variants (`Transparent`, `Omit`) plus
  **soft-drop substitution logic** for atomic content:
  - **Verbatim**: KeepBefore + `preimage_in` resolves into target file.
    Today's behavior, generalized via `preimage_in` to work on Derived
    chains too.
  - **Transparent (recurse)**: KeepBefore + Synthetic source_info + block
    has children with recoverable preimages. Recurse on children, produce
    a child-entry list. Wrapper itself emits nothing. Handles Sectionize.
  - **Omit**: KeepBefore + atomic-Synthetic node, OR KeepBefore + Synthetic
    with no recoverable children. The node is dropped from output; the
    next pipeline run regenerates it from baseline content. Used for
    filter-constructed leaves and the rare structurally-stable Synthetic
    leaf.
  - **Rewrite**: UseAfter or non-atomic Recurse-with-changes. Today's
    behavior. Includes the let-user-win case for block-level UseAfter
    on atomic nodes (see §"The coarsen logic" — atomicity does NOT
    block this path; the qmd writer's CustomNode arms know how to write
    fresh atomic CustomNodes from `plain_data`).
  - **InlineSplice**: today's behavior, extended with the multi-inline
    Derived dedupe rule and the **inline-level soft-drop substitution**
    described below.
- **Soft-drop substitutions** for the bad-edit cases. Coarsen detects
  these and **substitutes a safe alignment** rather than aborting the
  whole write:
  - **Inline-level UseAfter on a Derived inline** (user retyped resolved
    shortcode text): substitute KeepBefore for that one inline within
    the surrounding `InlineReconciliationPlan`. The rest of the inline
    plan continues as-is. Emit a `Q-3-42` warning into the warnings
    sink describing what was reverted.
  - **Block-level RecurseIntoContainer on an atomic CustomNode** (user
    edited inside an include): substitute KeepBefore for the wrapper.
    The wrapper's source_info points at the parent-file include token
    (Plan 8); Verbatim copy preserves it. Inner edits never reach the
    qmd writer's CustomNode arm. Emit a `Q-3-43` warning.
  - **Block-level UseAfter on an atomic node** (user replaced or
    deleted an atomic block via React): **let-user-win** — keep as
    Rewrite. The new block goes through the qmd writer's normal arms
    (Plan 8's IncludeExpansion arm reads `plain_data["source_path"]`
    and emits `{{< include … >}}` from a fresh user-edit-tagged
    CustomNode just as cleanly as from a pipeline-emitted one). No
    warning — the user explicitly chose this.
- **No `AtomicViolation` variant**. The previous design had coarsen
  produce an `AtomicViolation` entry that caused `incremental_write` to
  return `Err`. Under soft-drop, every bad-edit case has a safe
  substitution, so `AtomicViolation` is unnecessary. The writer's
  return type stays `Result<(String, Vec<Warning>), Vec<Error>>`-shaped
  (see "Warning channel mechanism" below); `Ok` carries the saved qmd
  plus any soft-drop warnings.
- **Warning channel mechanism**: `coarsen` accepts a
  `&mut Vec<DiagnosticMessage>` warning sink as a parameter. Soft-drop
  substitutions push warnings into the sink. The top-level
  `incremental_write` returns `Ok((String, Vec<DiagnosticMessage>))`
  when no fatal error occurs (warnings can be present), and `Err` only
  for true write failures (UTF-8 errors, qmd writer panics on
  malformed input — same as today). The hub-client's `RenderResponse`
  already carries a `warnings: [...]` field (Plan 1's pipeline
  diagnostics use it); soft-drop warnings flow through the same path.
- **Diagnostic codes** (per the Q-3 conventions; see
  `crates/quarto-error-reporting/src/error_catalog.json`):
  - `Q-3-42` — "Shortcode edit dropped". Emitted when an inline-level
    edit to Derived content was substituted by KeepBefore. Body:
    affected inline's Derived `by.kind` and resolved-to text, plus the
    shortcode token's source range so editor UIs can highlight it.
  - `Q-3-43` — "Include block edit dropped". Emitted when a
    block-level RecurseIntoContainer on an atomic CustomNode was
    substituted by KeepBefore. Body: the include's `source_path` from
    `plain_data`, plus the wrapper's source range. Actionable message:
    "to edit this content, open `<source_path>` directly."
  Both are `DiagnosticKind::Warning`. No new structural fields on
  `DiagnosticMessage` — discriminants are in the code+notes.
- `is_atomic_custom_node` registry, defined in **`quarto-core`** as
  `pub const ATOMIC_CUSTOM_NODES: &[&str]` plus
  `pub fn is_atomic_custom_node(type_name: &str) -> bool`. Single
  source of truth on the Rust side, consumed by the writer (`pampa`),
  Plan 8 (extends the const), and **hand-mirrored to TypeScript** at
  `hub-client/src/utils/atomicCustomNodes.ts` with a sync comment.
  This matches the codebase's existing pattern for cross-language
  type pairs (e.g., `hub-client/src/types/intelligence.ts` mirrors
  `quarto-lsp-core` types this way; `hub-client/src/types/diagnostic.ts`
  mirrors `DiagnosticMessage`). The codebase does not use codegen for
  this; doc comments + code review keep the lists aligned. Initial
  set: `["IncludeExpansion", "CrossrefResolvedRef"]`. Note:
  `ShortcodeResolution` is NOT in this set — shortcode atomicity is
  handled via the `Derived` source_info path, not via a wrapper.

  **Migration path for extension-contributed atomic types**: the
  hand-mirror is the right shape for built-ins. Extension-contributed
  atomic types (a future plan; see §Open questions
  "is_atomic_custom_node lookup — extension forward-compat") will
  replace the JS const with a `wasm_bindgen` runtime lookup populated
  per-render from loaded extensions. The migration changes the JS
  data source but not the React-side dispatch logic — components
  continue to call `isAtomicCustomNode(typeName)`; the function's
  implementation switches from a const lookup to a context lookup.
- `assemble`:
  - Walks Transparent entries by emitting each child's bytes with
    separators computed from the children's original positions.
  - Omit entries contribute nothing to the output (the original
    Synthetic node is dropped; baseline regenerates next pipeline run).
  - Inline-level dedupe: within an inline-splice or inline-assembly run,
    detect consecutive inlines sharing the same Derived `from` and emit
    one Verbatim (the from's preimage range) instead of N.
  - No AtomicViolation handling — soft-drop substitutions happened in
    coarsen; assemble sees only safe entries.
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
  atomic-detection + soft-drop logic but introduces the wrapper itself).
- Engine output as Derived (deferred future work).
- Editable CustomNode slots (e.g., editing a Callout's title and body
  through React with edits round-tripping back to source). See
  `claude-notes/research/2026-05-05-editable-custom-nodes.md`.
- Promoting the qmd writer to a fallible `Result` interface throughout.
  Soft-drop semantics make this unnecessary for q2-preview; the
  remaining panic paths are debug assertions for genuine programming
  errors (e.g., `unreachable!()` in Plan 8's qmd-writer arm for atomic
  CustomNodes in non-Verbatim paths), not user-facing failure modes.

## Design decisions (settled in conversation)

- **Sectionize's transparent recurse pattern**: `Synthetic` wrappers with
  source-bearing children get the Transparent treatment. Children's bytes
  are contiguous in source (Sectionize doesn't reorder), so emitting them
  in order produces the right output. The wrapper emits nothing.
- **Atomic detection has three paths** (all converging through the same
  `is_atomic` helper):
  1. **Derived source_info** (shortcode resolutions). Any node whose
     `source_info` is `Derived` is atomic.
  2. **Atomic Synthetic source_info** (filter constructions, title-block
     synthesis, tree-sitter postprocess space, etc.). Detected via
     `By::is_atomic_synthesizer()` (Plan 4 method on the `By` struct,
     keyed off `by.kind`).
  3. **Atomic CustomNode types** (IncludeExpansion, CrossrefResolvedRef).
     Looked up via `is_atomic_custom_node(&type_name) -> bool`.
- **Why three paths**: shortcode resolutions and filter constructions
  don't get wrappers (wrappers are too heavy for non-cross-file cases);
  they propagate atomicity via source_info shape. Includes use a
  wrapper because of the cross-file FileId issue (the included blocks
  live in another file; we need an anchor in the parent file).
- **Soft-drop, not abort**: bad-edit cases substitute a safe alignment
  in coarsen and emit a warning rather than aborting the entire write.
  The user's other (valid) edits go through; the bad edit is reverted
  to KeepBefore (or KeepBefore-equivalent for inline-level cases).
  Reasoning: the React side (Plan 2) is the primary safeguard via
  read-only enforcement; the writer is the contract guarantor; if both
  are correct the warning channel rarely fires; if React has a hole the
  writer protects without losing the user's session. "Edit cannot apply"
  is honored (the bad edit doesn't reach source); "edit cannot apply
  silently" is not (a Q-3-42/Q-3-43 warning surfaces in the diagnostic
  panel).
- **Let-user-win for block-level UseAfter on atomic** (user replaced
  or deleted an atomic block via React). Coarsen does NOT substitute
  here; the new block goes through Rewrite via the qmd writer. The
  qmd writer's CustomNode arms know how to write fresh atomic types
  from `plain_data` (Plan 8's IncludeExpansion arm reads
  `plain_data["source_path"]`). This composes naturally — a fresh
  user-edit-tagged IncludeExpansion serializes the same way as a
  pipeline-emitted one. No warning; the user's intent is clear.
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
fn is_atomic(node) -> bool {
    match node.source_info() {
        SourceInfo::Derived { .. } => true,
        SourceInfo::Synthetic { by } if by.is_atomic_synthesizer() => true,
        _ => {}
    }
    match node {
        Block::Custom(cn) if is_atomic_custom_node(&cn.type_name) => true,
        _ => false,
    }
}

For each block alignment from the reconciler:

if alignment is KeepBefore(orig_idx):
    let original_block = original_ast.blocks[orig_idx];
    if let Some(range) = original_block.source_info().preimage_in(target_file) {
        // Includes the atomic case (Derived + KeepBefore): Verbatim copy
        // of the preimage. preimage_in walks Derived chains to the from.
        CoarsenedEntry::Verbatim { byte_range: range, orig_idx }
    }
    else if matches!(original_block.source_info(), SourceInfo::Synthetic { by })
        && by.is_atomic_synthesizer()
    {
        // Atomic Synthetic with no preimage (filter construction etc.).
        // Drop from output; baseline regenerates next pipeline run.
        CoarsenedEntry::Omit
    }
    else if matches!(original_block.source_info(), SourceInfo::Synthetic { .. })
        && original_block has children
    {
        // Non-atomic Synthetic wrapper (Sectionize etc.) — Transparent recurse.
        CoarsenedEntry::Transparent { child_entries: <recurse on children> }
    }
    else {
        // Synthetic with no children, or some other shape with no preimage.
        CoarsenedEntry::Omit
    }

if alignment is UseAfter(new_idx):
    // Let user win — including for atomic types. The qmd writer's
    // CustomNode arms know how to write fresh atomic CustomNodes from
    // plain_data (Plan 8's IncludeExpansion arm reads source_path).
    // No atomic check here; trust the alignment.
    CoarsenedEntry::Rewrite { new_idx }

if alignment is RecurseIntoContainer { before_idx, after_idx }:
    let original_block = original_ast.blocks[before_idx];
    if is_atomic(original_block) {
        // SOFT-DROP: inner edits to an atomic block are reverted.
        // Substitute KeepBefore — Verbatim copy of the wrapper's preimage.
        warnings.push(diagnostic_q3_43(original_block));
        if let Some(range) = original_block.source_info().preimage_in(target_file) {
            CoarsenedEntry::Verbatim { byte_range: range, orig_idx: before_idx }
        } else {
            // Atomic node lacks a preimage in target — extremely unusual.
            // Substitute Omit; warning already pushed.
            CoarsenedEntry::Omit
        }
    } else {
        // Existing recurse logic for inline plans, custom_node_plans, etc.
        // The inline-plan-walking step has its own soft-drop substitution
        // (see "Inline-level soft-drop" below).
        ...
    }
```

**Inline-level soft-drop** (applied during `assemble_inline_content` and
when constructing the inline plan for InlineSplice):

```
For each inline alignment in plan.inline_alignments:

if alignment is UseAfter(new_idx) and is_atomic(new_inlines[new_idx]):
    // User retyped over a Derived inline (shortcode resolution).
    // Substitute KeepBefore for the corresponding original inline.
    warnings.push(diagnostic_q3_42(new_inlines[new_idx]));
    treat as KeepBefore(<the corresponding original index>)

if alignment is RecurseIntoContainer and the original inline is_atomic:
    // Same shape as the block-level recurse-on-atomic case.
    warnings.push(diagnostic_q3_42(orig_inlines[before_idx]));
    treat as KeepBefore(before_idx)
```

The "corresponding original index" for inline-level UseAfter substitution
is the index in `orig_inlines` whose Derived `from` matches the new inline's
`from`. In the multi-inline shortcode case, multiple original inlines share
the same `from`; any of them produces the right Verbatim result (they all
preimage to the same shortcode token bytes, which the dedupe rule emits
once anyway).

The `assemble` step iterates coarsened entries:

- Verbatim → copy byte range from `original_qmd`.
- Rewrite → use the qmd writer to serialize the new block.
- InlineSplice → existing splice logic, extended with (a) the
  multi-inline Derived dedupe rule and (b) inline-level soft-drop
  substitutions before assembly.
- Transparent → emit children's bytes recursively.
- Omit → skip (contribute nothing to output).

The function returns `Ok((String, Vec<DiagnosticMessage>))` carrying the
saved qmd plus any soft-drop warnings that fired during coarsen. It only
returns `Err` for genuine write failures (UTF-8 errors, qmd writer failures
on malformed input — same as today's writer).

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
- **The `is_atomic_custom_node` lookup — extension forward-compat**:
  today's hardcoded `pub const ATOMIC_CUSTOM_NODES: &[&str]` works for
  built-in atomic types. Future extensions (including the eventual
  TSX-extension story) will need to register their own atomic types
  without modifying `quarto-core`.

  The forward-compat design (deferred to a follow-up plan; commits
  the *shape* now without writing implementation code):

  - **YAML schema** in `_extension.yml`:
    ```yaml
    contributes:
      custom-nodes:
        - { type: MyCustomBlock, atomic: true }
        - { type: AnotherWidget }              # atomic defaults to false
    ```
  - **Rust runtime aggregation** mirrors `resolve_filters()`'s pattern:
    `pub fn collect_atomic_custom_node_types(extensions: &[Extension]) -> HashSet<String>`
    starts from the built-in set and adds extension-contributed entries
    where `atomic == true`.
  - **Function signature evolution**:
    `is_atomic_custom_node(name)` →
    `is_atomic_custom_node(name, &registry: &HashSet<String>)`. The
    writer (in `pampa`) gets the registry from `StageContext` at coarsen
    time. ~30 callers cascading; mechanical.
  - **Rust→JS sync** for extension types (the genuinely-new piece —
    the hand-mirror approach in Plan 7 doesn't work for extension
    types because they aren't known at hub-client build time):
    a `wasm_bindgen` export `get_atomic_custom_node_types()` is called
    once per render after extensions are loaded; populates a React
    context. The hand-mirrored TS const remains the fallback for the
    no-extensions / WASM-initializing case and stays correct for
    built-ins.
  - **Plan 8's `IncludeExpansion`**: lands in the built-in set today
    via `pub const ATOMIC_CUSTOM_NODES`. After the follow-up plan, the
    set is built from a built-in's `_extension.yml` rather than
    hardcoded — same effect via the same code path that user
    extensions use, no privileged route.

  This sketch commits the schema choice (`contributes.custom-nodes` with
  `atomic: bool`) and the function-signature migration path. Plan 7
  ships the const-based registry; the runtime aggregation, schema
  parsing, and `wasm_bindgen` lookup all land in a follow-up when an
  extension actually needs to register an atomic type.
- **Sibling vs param**: Decision D was "param with default" but Plan 4 / 7
  could implement it either way. Confirm during implementation. Param is
  cleaner (one fewer entry point). Sibling is more isolated. Either works.
- **Runtime user-filter idempotence detection**: split out to Plan 7a.
  See `claude-notes/plans/2026-05-04-q2-preview-plan-7a-filter-idempotence.md`
  for the full design — round-trip idempotence check, per-filter
  attribution, `idempotent: false` opt-out, Q-3-44 / Q-3-45
  diagnostics. Plan 7a is a separable follow-up that builds on Plan 7's
  `pipeline_kind: Some("preview")` machinery; it doesn't gate M3.

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

- **Reconciler source-info-blindness foundation test** (new, lands in
  Plan 7's first commit): asserts that `structural_eq_blocks` and
  `structural_eq_inlines` (in `quarto-ast-reconcile`) return `true` for
  pairs of nodes that differ *only* in source_info. Cover all the new
  shapes: two Original blocks with different file IDs / offsets; two
  Synthetic blocks with different `By` payloads; two Derived blocks with
  different `from` chains but the same content/attr/plain_data;
  CustomNode pairs differing only in source_info on the wrapper or in
  any slot child. The hash function already excludes source_info
  (verified by Plan 3 and existing
  `compute_blocks_hash_fresh::test_same_content_same_hash`); this test
  covers the *equality* path too. Why it matters: the reconciler drives
  KeepBefore decisions off these functions. If they leak source_info
  by accident, q2-preview round-trip would degenerate to whole-doc
  Rewrite without any obvious symptom — every test that doesn't inspect
  the alignment plan would still pass. Catch the leak structurally
  rather than discover it via correctness regressions.
- **`preimage_in` unit tests**: each variant (Original same/different file,
  Substring chain, Concat contiguous/gappy, Synthetic, Derived). Assert
  correct byte range or None.
- **Coarsen unit tests**: build mock reconciliation plans + ASTs covering:
  - Verbatim (KeepBefore + preimage in target, both Original and Derived).
  - Transparent (KeepBefore + non-atomic Synthetic wrapper with
    source-bearing children — Sectionize case).
  - Omit via atomic Synthetic (KeepBefore + Synthetic with
    `by.is_atomic_synthesizer() == true` and no preimage — filter
    construction case).
  - Omit via Synthetic with no children (rare).
  - Rewrite (UseAfter, non-atomic).
  - **Soft-drop: inline UseAfter on Derived** — substitute KeepBefore
    for that inline, surrounding inline plan continues; assert
    `Q-3-42` warning emitted.
  - **Soft-drop: block RecurseIntoContainer on atomic CustomNode**
    (IncludeExpansion) — substitute KeepBefore for the wrapper;
    assert `Q-3-43` warning emitted; assert wrapper's preimage bytes
    in output.
  - **Let-user-win: block UseAfter on atomic node** — Rewrite via qmd
    writer; no warning. Assert qmd writer's CustomNode arm correctly
    serializes a fresh user-edit-tagged IncludeExpansion (uses
    `plain_data["source_path"]`).
- **Multi-inline dedupe unit tests**: build a Para with three consecutive
  inlines all sharing the same Derived `from`. Reconcile against an
  identical Para. Assert the writer emits the shortcode token bytes
  ONCE, not three times, in the inline-assembly output.
- **Soft-drop interaction tests**:
  - User edits one Derived inline AND a non-atomic inline in the same
    Para → assert non-atomic edit is applied AND shortcode token is
    preserved AND `Q-3-42` warning emitted.
  - User edits inside an include AND outside the include in same doc →
    assert outside edit is applied AND include token is preserved AND
    `Q-3-43` warning emitted (write succeeds with warnings, not Err).
- **End-to-end round-trip tests**:
  - Sectionized doc → edit one paragraph → assert the section structure
    is preserved verbatim except for the edit.
  - Doc with single-inline shortcode (`{{< meta title >}}`) → edit a
    different paragraph → assert the shortcode token is preserved.
  - Doc with multi-inline shortcode (markdown title) → edit a different
    paragraph in same Para → assert the shortcode token appears once,
    not multiple times.
  - Doc with shortcode → attempt to edit the resolved title → assert
    `Q-3-42` warning + the document text is byte-equal to a no-op edit
    (i.e., the bad edit was reverted). Save succeeded.
  - (Plan 8 covers includes; this plan establishes the infrastructure.)
- **Filter-construction soft-drop test**: build an AST with a
  filter-constructed Str (Synthetic { by: filter }) inside a Para. User
  retypes it through React → assert `Q-3-42` warning + the original
  Para's source bytes (without the decoration) appear in output. Next
  pipeline run regenerates the decoration.
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
- **Soft-drop warning visibility**: warnings flow through the existing
  `RenderResponse.warnings` channel (the same path Plan 1's pipeline
  diagnostics use). ReactPreview already displays diagnostics in the
  editor. Confirm Q-3-42 / Q-3-43 warnings reach the diagnostic panel
  and are visually distinguishable from pipeline warnings (or are
  acceptably co-mingled — TBD by hub-client UX).
- **Autosave-context spam mitigation for Q-3-42 / Q-3-43**: hub-client
  uses Automerge as the source-of-truth for qmd source — there's no
  discrete "save" action; every keystroke triggers a debounced render
  and incremental write. So a user persistently typing over a Derived
  inline (resolved shortcode) would re-fire Q-3-42 on every render,
  flooding the diagnostic panel with copies of the same warning.
  Same for Q-3-43 if the user keeps editing inside an include.
  Mitigation: **suppress-after-3** in the diagnostic banner. The
  Monaco squiggle (yellow underline at the affected source range)
  remains as the persistent signal; the side-panel banner shows the
  first three occurrences per source range and silently drops the
  rest. Implemented at the diagnostic-ingest layer in `Preview.tsx`
  (or wherever warnings are processed for display), not at the
  writer. Plan 7a's Q-3-44 doesn't have this issue — it's cached
  once per document per session, so it fires at most once.
  Imperative message text matters here too: Q-3-42 / Q-3-43 should
  read as instructions ("To edit this content, open `<source_path>`")
  rather than passive descriptions ("edit was dropped"), since the
  user has no discrete-save affordance to discard the bad edit.
  Plan 7's soft-drop is what guarantees the qmd source-of-truth
  doesn't accept the bad edit even though the in-React AST briefly
  held it.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `preimage_in` accessor (with Derived) + tests | ~100 |
| New `CoarsenedEntry` variants (Transparent, Omit) | ~20 |
| `coarsen` logic update (atomic detection + soft-drop substitutions) | ~180 |
| `assemble` updates (Transparent walk, Omit handling) | ~80 |
| Multi-inline shortcode dedupe rule in inline assembly | ~40 |
| Inline-level soft-drop substitution in inline plan | ~50 |
| `is_atomic_custom_node` registry + TS hand-mirror | ~40 |
| Q-3-42 / Q-3-43 diagnostic codes + catalog entries | ~40 |
| Warning channel plumbing through coarsen → incremental_write return | ~50 |
| `pipeline_kind` parameter + WASM bridge + TS wrapper | ~80 |
| ReactPreview guard lift + edit-back wiring | ~20 |
| Tests (unit + end-to-end round-trip + soft-drop interactions) | ~400 |
| **Total** | **~1100** |

Two focused sessions likely. Flagged as one of the highest-complexity plans;
extend the budget if the InlineSplice + Transparent composition surfaces
unexpected interactions.

## Notes

This is the most intricate plan in the set. It's the keystone for M3 —
once this lands, q2-preview is truly editable for the common case. Take
care with the test coverage; round-trip bugs in the writer can corrupt
source silently if not caught.

### Soft-drop replaces hard-abort (revised from earlier draft)

An earlier draft of this plan had AtomicViolation as a hard error — any
edit to atomic content aborted the entire write. We revised to soft-drop:
each bad-edit case substitutes a safe alignment in coarsen and emits a
warning, but the user's other edits go through. The user-facing contract
"this edit must be prohibited" is honored (the bad edit doesn't apply);
the user-facing failure mode "the entire save was rejected" is not.
React (Plan 2) is the primary safeguard via read-only enforcement; the
writer is the contract guarantor; if React has a hole the writer
protects without losing the user's session.

The let-user-win exception for block-level UseAfter on atomic
(user-replaced or -deleted atomic block via React) is a deliberate
asymmetry: when the user explicitly destroys an atomic block, we trust
them. The qmd writer's CustomNode arms know how to write fresh atomic
types from `plain_data` (Plan 8's IncludeExpansion arm reads
`plain_data["source_path"]`), so this composes through the normal
Rewrite path with no special handling.

### Filter mutations are not flagged as atomic — accepted corner

Plan 4 distinguishes filter constructions (`pandoc.Str("decoration")` →
`Synthetic { by: filter }`, atomic) from filter mutations
(`Str.text = upper(Str.text)` → keeps Original source_info, NOT atomic).

A user editing a filter-mutated Str through React produces an unusual
round-trip: the user types "world" over the filter-output "HELLO";
the writer Rewrites "world" to source; the next pipeline run filters
"world" → "WORLD". For idempotent filters (uppercase) this is fine —
the typed text round-trips through filter to itself. For non-idempotent
filters (`x => upper(x) + "!"`) the typed text gets a `!` appended on
every save, which is confusing.

We accept this corner rather than flagging filter mutations as atomic
because (a) it would require revising Plan 4 to track filter mutations
distinctly from plain Original source_info (a notable type-system
change), (b) the runtime user-filter idempotence detection (above)
catches the AST-level non-idempotence that would actually corrupt
round-trip, and (c) Plan 3's idempotence test enforces the
contract for built-in filters at CI time. Users who write
non-idempotent filters get a warning at runtime and can decide whether
the trade-off is acceptable for their workflow.

### The byte-provenance contract (and why the writer stays infallible)

The contract isn't "no materialization" — that phrasing is too blunt
and conflates two cases. **The writer materializes constantly** in the
neutral sense: every Rewrite path materializes new bytes through the
qmd writer; even Verbatim copies are a kind of materialization (bytes
appearing in the saved file). The contract is more precise: the writer
only emits bytes whose origin can be honestly traced to either
**existing source bytes in the target file** (Verbatim copies, slot
preimages via `preimage_in`) or **fresh AST the user constructed**
(Rewrite paths fed by user-supplied AST nodes via the qmd writer's
normal arms).

What soft-drop forbids — by structural construction — is the case
where the writer would emit bytes synthesized from a wrapper's slot
children as flat content in the parent file. Concretely: if Plan 8's
qmd-writer arm for `IncludeExpansion` were reached in a non-Verbatim
path, it would (under the old defensive-fallback design) walk the
wrapper's content slot and emit those blocks as flat parent-file bytes
— but those blocks come from foo.qmd, not from parent.qmd source nor
from user input. Writing them into parent.qmd would put bytes there
whose provenance is the included file, which is dishonest at the
parent-file boundary.

Under soft-drop, coarsen substitutes KeepBefore (Verbatim of the
wrapper's parent-file include-token bytes) before the qmd writer ever
sees that case. The arm becomes `unreachable!()` — a debug assertion
for coarsen bugs, not a user-facing failure mode. Promoting the qmd
writer to a fallible `Result` interface to make the unreachable case
recoverable would be over-engineering, since correct coarsen makes the
case structurally absent. WASM panic-abort still kills the session if
the assertion fires, but that's the same risk profile as any other
writer bug; it's not specific to atomic enforcement, and it's
reachable only via a programming error in coarsen.

The let-user-win Rewrite path is provenance-honest: when the user
constructs a fresh `IncludeExpansion` through React (with `plain_data
= { source_path: "bar.qmd" }`) and the writer materializes
`{{< include bar.qmd >}}` into source, the bytes' origin is the user's
edit. Plan 8's qmd-writer arm reads `plain_data`, doesn't read
`source_info`, and emits the include syntax — same arm whether the
wrapper came from `IncludeExpansionStage` (pipeline) or from React
(user). That symmetry is what makes the let-user-win case clean.
