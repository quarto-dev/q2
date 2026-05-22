# Plan 7 — Incremental writer preimage walk + Transparent + atomic soft-drop + multi-inline dedupe

**Date:** 2026-05-04 (revised 2026-05-20)
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M3 (edit-back works for non-include, non-pure-synthesis edits)

## Epic context

Part of the **provenance epic** (Plans 3–8). Plan 7 is the keystone:
once the writer understands the typed provenance from Plans 4–6, it
can correctly round-trip user edits, soft-drop bad edits with clear
diagnostics, and surface warnings on both hub-client and the `q2
preview` SPA. The file name keeps its q2-preview-plan-N form for
continuity with the earlier discussion notes.

## Goal

Teach the incremental writer (`pampa::writers::incremental`) to handle
the typed provenance shapes Plans 4–6 introduce so that q2-preview
round-trip edits work correctly. Five new behaviors:

- **`preimage_in(target_file_id)` accessor**: a recursive walk through
  Substring/Concat/Generated chains that returns the byte range in the
  target file IF the chain resolves there, else None. For
  `Generated`, walks through the `Invocation` anchor (via
  `invocation_anchor()`).
- **`Transparent` coarsen variant**: for `KeepBefore` nodes whose
  source_info is `Generated` with empty anchors AND non-atomic kind
  (Sectionize's case, footnotes container, appendix wrapper), recurse
  into the children rather than emit a useless empty Verbatim. The
  wrapper itself contributes nothing to the output.
- **Atomic detection via `by.is_atomic_kind()`**: nodes whose
  source_info is `Generated { by, .. }` with `by.is_atomic_kind() == true`
  are atomic. Covers `shortcode`, `filter`, `title-block`,
  `tree-sitter-postprocess`. For shortcode-resolution case: KeepBefore
  → Verbatim copies the `Invocation` anchor's preimage (the shortcode
  token); UseAfter → soft-drop with Q-3-42 warning.
- **Atomic detection via `is_atomic_custom_node`**: `IncludeExpansion`
  CustomNode is atomic via type_name lookup. Same outcome as the
  atomic-kind case (KeepBefore Verbatim; inner edits → soft-drop with
  Q-3-43 warning). Plus `CrossrefResolvedRef` is atomic (already a
  CustomNode in the AST).
- **Multi-inline dedupe rule**: when assembling a run of consecutive
  inlines (in InlineSplice or inline assembly contexts) that all share
  the same `Invocation` anchor source_info, emit Verbatim *once* for
  the group rather than N times. This handles multi-inline shortcode
  resolutions.

This plan also adds a `pipeline_kind: Option<String>` parameter to
`incremental_write_qmd` that runs the q2-preview pipeline on the
baseline AST before reconciling, making the reconcile symmetric. The
parameter threads through both hub-client and the new `q2 preview` SPA
via the shared `@quarto/preview-runtime` package.

When this plan lands, ReactPreview's read-only guard from Plan 1 lifts
(one-block early-return in `handleSetAst`, deletable per Plan 1's
design), and edits in q2-preview round-trip correctly. The **q2
preview SPA also gains edit-back** via the same writer path —
replacing its current `noopSetAst` with a real handler that routes
through `incrementalWriteQmd` to the sync-client's `updateFileContent`
and through automerge to the ephemeral hub's disk-write.

## Scope

### In scope

- `preimage_in` accessor on `SourceInfo` (in `quarto-source-map`).
  Walks Substring's `parent`, Concat's `pieces`, Generated's
  `Invocation` anchor (via `invocation_anchor()`). Returns
  `Some(byte_range)` if the chain resolves to an `Original` in the
  target file, else `None`.
- `coarsen` rules. Two new entry variants (`Transparent`, `Omit`) plus
  **soft-drop substitution logic** for atomic content:
  - **Verbatim**: KeepBefore + `preimage_in` resolves into target file.
    Today's behavior, generalized via `preimage_in` to work on
    Generated chains too (via the `Invocation` anchor).
  - **Transparent (recurse)**: KeepBefore + Generated with empty
    anchors AND non-atomic kind AND block has children with
    recoverable preimages. Recurse on children, produce a child-entry
    list. Wrapper itself emits nothing. Handles Sectionize, footnotes
    container, appendix wrapper.
  - **Omit**: KeepBefore + atomic-kind Generated node with no
    `Invocation` anchor (filter-constructed leaves, title-block h1,
    tree-sitter postprocess), OR Generated with no preimage and no
    source-bearing children. The node is dropped from output; the
    next pipeline run regenerates it from baseline content.
  - **Rewrite**: UseAfter or non-atomic Recurse-with-changes. Today's
    behavior. Includes the let-user-win case for block-level UseAfter
    on atomic nodes (see "The coarsen logic" — atomicity does NOT
    block this path; the qmd writer's CustomNode arms know how to
    write fresh atomic CustomNodes from `plain_data`).
  - **InlineSplice**: today's behavior, extended with the multi-inline
    dedupe rule and the **inline-level soft-drop substitution**
    described below.
- **Soft-drop substitutions** for the bad-edit cases. Coarsen detects
  these and **substitutes a safe alignment** rather than aborting the
  whole write:
  - **Inline-level UseAfter on an atomic-Generated inline** (user
    retyped resolved shortcode text): substitute KeepBefore for that
    one inline within the surrounding `InlineReconciliationPlan`. The
    rest of the inline plan continues as-is. Emit a `Q-3-42` warning
    into the warnings sink describing what was reverted.
  - **Block-level RecurseIntoContainer on an atomic CustomNode** (user
    edited inside an include): substitute KeepBefore for the wrapper.
    The wrapper's source_info is Original pointing at the parent-file
    include token (Plan 8); Verbatim copy preserves it. Inner edits
    never reach the qmd writer's CustomNode arm. Emit a `Q-3-43`
    warning.
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
  diagnostics use it; current main collates them in
  `ReactPreview.tsx`); soft-drop warnings flow through the same path.
- **Diagnostic codes** (per the Q-3 conventions; see
  `crates/quarto-error-reporting/src/error_catalog.json`):
  - `Q-3-42` — "Shortcode edit dropped." Emitted when an inline-level
    edit to shortcode-resolved content was substituted by KeepBefore.
    Body: the affected inline's text and the shortcode token's source
    range (from the `Invocation` anchor) so editor UIs can highlight
    it.
  - `Q-3-43` — "Include block edit dropped." Emitted when a
    block-level RecurseIntoContainer on an atomic CustomNode was
    substituted by KeepBefore. Body: the include's `source_path` from
    `plain_data`, plus the wrapper's source range. Actionable
    message: "To edit this content, open `<source_path>` directly."
  Both are `DiagnosticKind::Warning`. Wording references the
  user-facing concepts ("shortcode-resolved content," "include line")
  rather than internal type names like "Derived" or "Generated."
- `is_atomic_custom_node` registry, defined in **`quarto-core`** as
  `pub const ATOMIC_CUSTOM_NODES: &[&str]` plus
  `pub fn is_atomic_custom_node(type_name: &str) -> bool`. Plan 7
  ships the **Rust side** (writer in `pampa` consumes it; Plan 8
  extends the const to add `IncludeExpansion`). The **TypeScript
  hand-mirror** at
  `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts` (path
  moved from hub-client during the Plans 1–2 / Phase D refactor) is
  the JS-side equivalent. Both sides ship with `CrossrefResolvedRef`;
  Plan 8 adds `IncludeExpansion`. The TS file's header comment
  documents the sync convention. Initial set:
  `["CrossrefResolvedRef"]` (Plan 8 adds `IncludeExpansion` to both
  sides). Note: shortcodes (`meta`, `kbd`, etc.) are NOT in this set
  — shortcode atomicity is handled via the `by.is_atomic_kind()` path
  on Generated source_info, not via a wrapper.

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
    Generated node is dropped; baseline regenerates next pipeline run).
  - Inline-level dedupe: within an inline-splice or inline-assembly
    run, detect consecutive inlines whose `Invocation` anchor
    references structurally-equal source_info and emit one Verbatim
    (the anchor's preimage range) instead of N.
  - No AtomicViolation handling — soft-drop substitutions happened in
    coarsen; assemble sees only safe entries.
- `pipeline_kind` parameter added to `incremental_write_qmd`. When
  `Some("preview")`:
  - Re-parses `original_qmd` (today's behavior).
  - **Runs the q2-preview transform pipeline on the baseline** (this
    is the NEW step). Produces a baseline AST at the same pipeline
    tier as the live AST.
  - Reconciles new vs baseline.
  - Writes via the updated coarsen/assemble logic.
- **Hub-client integration**: Lift the `handleSetAst` read-only guard
  in `ReactPreview.tsx` introduced in Plan 1. Wire `setLocalAst`
  through with `pipeline_kind: "preview"`.
- **q2 preview SPA integration** (new in this revision):
  - Replace `noopSetAst` in `q2-preview-spa/src/PreviewApp.tsx` with a
    real handler that calls `incrementalWriteQmd(content, newAst,
    "preview")` and then `syncClient.updateFileContent(path, newQmd)`.
  - Add an `applyingRemoteRef`-equivalent to the SPA (mirroring
    hub-client's `useAutomergeSync.ts` pattern) to suppress the loop
    where SPA-initiated edits round-trip through automerge → disk →
    file watcher → re-sync → re-render and could otherwise re-fire
    setAst.
  - Ship `q2-preview-spa/src/components/DiagnosticStrip.tsx` — a small
    SPA-local component (~50 lines TSX + ~20 lines CSS) that displays
    Q-3-42 / Q-3-43 warnings returned by `incrementalWriteQmd`.
    Mirrors hub-client's `.diagnostics-banner` visual style. Applies
    suppress-after-3-by-source-range (see "Autosave-context spam
    mitigation" below).
  - Both single-file mode (bd-tnm3k; project_root = parent dir, watcher
    constrained to one file) and project mode work via the same code
    path — the ephemeral hub already bridges automerge ↔ disk
    uniformly. No SPA-side branching needed.
- **Diagnostic surfacing in hub-client**: warnings already flow
  through `ReactPreview.tsx`'s `allDiagnostics` collation (line
  163-166) and reach `Editor`'s `diagnosticsToMarkers` split into
  Monaco squiggles (located warnings) and the existing
  `.diagnostics-banner` (unlocated). Q-3-42 and Q-3-43 both carry
  source ranges, so they squiggle naturally. **No new hub-client UI
  needed.** The existing infrastructure handles them.

  One known UX gap: the banner is gated on `!isFullscreenPreview`, so
  users in fullscreen-preview mode rely on the Monaco squiggles
  (visible when they exit fullscreen) rather than the banner. Accepted.

### Out of scope

- Include round-trip via wrapper-CustomNode (Plan 8 — uses this plan's
  atomic-detection + soft-drop logic but introduces the wrapper itself).
- Engine output as Generated (deferred future work).
- Editable CustomNode slots (e.g., editing a Callout's title and body
  through React with edits round-tripping back to source). See
  `claude-notes/research/2026-05-05-editable-custom-nodes.md`.
- Promoting the qmd writer to a fallible `Result` interface throughout.
  Soft-drop semantics make this unnecessary for q2-preview; the
  remaining panic paths are debug assertions for genuine programming
  errors (e.g., `unreachable!()` in Plan 8's qmd-writer arm for atomic
  CustomNodes in non-Verbatim paths), not user-facing failure modes.
- Lifting hub-client's diagnostic banner + SPA's `DiagnosticStrip` into
  a shared `@quarto/preview-renderer` component. Filed as a follow-up
  against the hub-client decomposition epic (bd-hfjj); not on Plan 7's
  critical path.

## Design decisions (settled in conversation)

- **Sectionize's transparent recurse pattern**: `Generated` wrappers
  with empty anchors AND non-atomic kind AND source-bearing children
  get the Transparent treatment. Children's bytes are contiguous in
  source (Sectionize doesn't reorder), so emitting them in order
  produces the right output. The wrapper emits nothing.
- **`FootnotesTransform` and `AppendixStructureTransform` containers
  also fit the Transparent pattern.** Plan 2B's audit added both
  transforms to the q2-preview pipeline. Their synthesized container
  Divs (`<div class="footnotes">`, `<div id="quarto-appendix">`) have
  no source preimage and non-atomic kinds (`footnotes`, `appendix`),
  but their children carry source_info from the user-typed footnote
  content / user-defined `:::{.appendix}` blocks. Same Transparent
  treatment as Sectionize.

  Worth noting: `FootnotesTransform`'s synthesized `<sup>` markers are
  NOT pure synthesis — `create_footnote_ref` at
  `crates/quarto-core/src/transforms/footnotes.rs:440-460` clones
  source_info from the original `Note` inline, so the markers carry
  the same byte range as the user's `^[footnote text]` syntax.
  Round-trip-friendly as `Original` without extra writer work; only
  the bare `<div class="footnotes">` wrapper is the Transparent case.

  **Plan 6's `make_error_inline` and `shortcode_to_literal` follow
  the same pattern** (added 2026-05-22). For unknown shortcodes
  (`{{< bogus >}}`) and escaped shortcodes (`{{</ meta foo >}}`),
  Plan 6 threads the *original shortcode token's* source_info through
  to the visible Str (and the wrapping Strong for the error case).
  Both layers carry `Original` source_info covering the same token
  bytes — structurally identical to the footnote `<sup>` overlap.
  `is_atomic_kind()` does NOT fire (source_info is Original, not
  Generated); these regions go through the writer's normal
  Verbatim-copy path. The user can edit `?bogus` or `{{</...>}}` in
  React just like any other text region. No special writer handling
  needed; Plan 6's test plan adds a round-trip regression for both.
- **Atomic detection has two convergent paths** (collapsed from three
  in earlier drafts; the unified `Generated` variant replaces the
  separate "Derived" path):
  1. **Atomic `Generated` source_info** (shortcode resolutions, filter
     constructions, title-block synthesis, tree-sitter postprocess
     space). Detected via `by.is_atomic_kind()` (Plan 4 method on the
     `By` struct, keyed off `by.kind`).
  2. **Atomic CustomNode types** (`IncludeExpansion`,
     `CrossrefResolvedRef`). Looked up via
     `is_atomic_custom_node(&type_name) -> bool`.
- **Why two paths**: filter constructions, shortcode resolutions, and
  title-block synthesis don't get wrappers (wrappers are too heavy
  for non-cross-file cases); they propagate atomicity via source_info
  shape. Includes use a wrapper because of the cross-file FileId
  issue (the included blocks live in another file; we need an anchor
  in the parent file). Plan 8's wrapper is a `CustomNode("IncludeExpansion")`
  whose source_info is Original — atomicity comes via `type_name`,
  not via source_info shape. See Plan 4's "Original vs Generated on
  synthesized nodes" for the full rationale.
- **Soft-drop, not abort**: bad-edit cases substitute a safe
  alignment in coarsen and emit a warning rather than aborting the
  entire write. The user's other (valid) edits go through; the bad
  edit is reverted to KeepBefore (or KeepBefore-equivalent for
  inline-level cases). Reasoning: the React side (Plan 2A's framework
  atomic gate) is the primary safeguard via read-only enforcement;
  the writer is the contract guarantor; if both are correct the
  warning channel rarely fires; if React has a hole the writer
  protects without losing the user's session. "Edit cannot apply"
  is honored (the bad edit doesn't reach source); "edit cannot apply
  silently" is not (a Q-3-42/Q-3-43 warning surfaces in the
  diagnostic panel).
- **Let-user-win for block-level UseAfter on atomic** (user replaced
  or deleted an atomic block via React). Coarsen does NOT substitute
  here; the new block goes through Rewrite via the qmd writer. The
  qmd writer's CustomNode arms know how to write fresh atomic types
  from `plain_data` (Plan 8's IncludeExpansion arm reads
  `plain_data["source_path"]`). This composes naturally — a fresh
  user-edit-tagged IncludeExpansion serializes the same way as a
  pipeline-emitted one. No warning; the user's intent is clear.
- **Multi-inline shortcode dedupe**: a multi-inline shortcode
  resolution produces several inlines all sharing the same
  `Invocation` anchor source_info. The writer's inline-assembly path
  needs to detect this and emit Verbatim *once* for the group.
  Without this, the assembly emits the shortcode token N times.
- **Param-with-default for `incremental_write_qmd`**: add a
  `pipeline_kind: Option<String>` parameter. `None` = current behavior
  (parse-only baseline). `Some("preview")` = run q2-preview pipeline
  on baseline. Existing callers (q2-debug demos, sync client,
  ReactPreview's q2-debug path) continue to work unchanged.

## The coarsen logic

```
fn is_atomic(node) -> bool {
    match node.source_info() {
        SourceInfo::Generated { by, .. } if by.is_atomic_kind() => true,
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
        // Generalized: handles Original, Substring, Concat-contiguous,
        // and Generated-via-Invocation-anchor uniformly. The atomic
        // shortcode case lands here too: Generated's `preimage_in`
        // walks `invocation_anchor()` and resolves to the token bytes.
        CoarsenedEntry::Verbatim { byte_range: range, orig_idx }
    }
    else if matches!(original_block.source_info(), SourceInfo::Generated { by, .. })
        && by.is_atomic_kind()
    {
        // Atomic-kind Generated with no Invocation anchor (filter
        // construction, title-block, tree-sitter-postprocess).
        // Drop from output; baseline regenerates next pipeline run.
        //
        // Belt-and-suspenders enforcement of Plan 4's required-anchor
        // invariant for shortcode: routing a shortcode-Generated to Omit
        // would mean silent data loss (the source bytes disappear from
        // the round-tripped document). The Plan 6 stamper is responsible
        // for never producing this shape, but a regression there would
        // surface here as a benign-looking Omit. The debug_assert catches
        // it in dev / test builds.
        debug_assert!(
            !by.is_kind("shortcode"),
            "Generated {{ by: shortcode, from: [] }} reached the writer — \
             Plan 6's stamper must always attach an Invocation anchor for \
             shortcode resolutions (Plan 4 §Required-anchor invariant)."
        );
        CoarsenedEntry::Omit
    }
    else if matches!(original_block.source_info(), SourceInfo::Generated { .. })
        && original_block has children
    {
        // Non-atomic Generated wrapper (Sectionize, footnotes,
        // appendix) — Transparent recurse.
        CoarsenedEntry::Transparent { child_entries: <recurse on children> }
    }
    else {
        // Generated with no children, or some other shape with no preimage.
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

**Inline-level soft-drop** (applied during `assemble_inline_content`
and when constructing the inline plan for InlineSplice):

```
For each inline alignment in plan.inline_alignments:

if alignment is UseAfter(new_idx) and is_atomic(new_inlines[new_idx]):
    // User retyped over an atomic-Generated inline (shortcode resolution).
    // Substitute KeepBefore for the corresponding original inline.
    warnings.push(diagnostic_q3_42(new_inlines[new_idx]));
    treat as KeepBefore(<the corresponding original index>)

if alignment is RecurseIntoContainer and the original inline is_atomic:
    // Same shape as the block-level recurse-on-atomic case.
    warnings.push(diagnostic_q3_42(orig_inlines[before_idx]));
    treat as KeepBefore(before_idx)
```

The "corresponding original index" for inline-level UseAfter
substitution is the index in `orig_inlines` whose `Invocation` anchor
source_info matches the new inline's. In the multi-inline shortcode
case, multiple original inlines share the same anchor source_info;
any of them produces the right Verbatim result (they all preimage to
the same shortcode token bytes, which the dedupe rule emits once
anyway).

The `assemble` step iterates coarsened entries:

- Verbatim → copy byte range from `original_qmd`.
- Rewrite → use the qmd writer to serialize the new block.
- InlineSplice → existing splice logic, extended with (a) the
  multi-inline dedupe rule and (b) inline-level soft-drop
  substitutions before assembly.
- Transparent → emit children's bytes recursively.
- Omit → skip (contribute nothing to output).

The function returns `Ok((String, Vec<DiagnosticMessage>))` carrying
the saved qmd plus any soft-drop warnings that fired during coarsen.
It only returns `Err` for genuine write failures (UTF-8 errors, qmd
writer failures on malformed input — same as today's writer).

## Multi-inline shortcode dedupe

When `{{< meta foo >}}` resolves to multiple inlines (e.g., metadata is
markdown like `**Bold** Title` → `[Strong[Str], Space, Str]`), each
resolved inline has the same `Generated { by: shortcode("meta"),
from: [Invocation -> Original{shortcode_range}] }` source_info.

Block-level: if both pipeline runs produce the same multi-inline
output, the surrounding Para is structurally identical → KeepBefore at
block level → Verbatim copy of the WHOLE Para's bytes (including the
shortcode token). One copy. ✓

Inline-level recursion (when the user edits something else in the same
Para): the reconciler picks `RecurseIntoContainer` with an inline
plan. Each shortcode-derived inline is `KeepBefore` individually.
Without dedupe, each one's Verbatim emits the shortcode token → N
copies in output.

Dedupe rule: when iterating inline alignments in
`assemble_inline_content`, group consecutive `KeepBefore` entries
whose inlines' `Invocation` anchors share structurally-equal
`source_info`. Emit Verbatim *once* for the group, using the anchor's
preimage byte range.

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
                if ranges.windows(2).all(|w| w[0].end == w[1].start) {
                    Some(ranges.first()?.start .. ranges.last()?.end)
                } else {
                    None  // gappy concat — can't Verbatim-copy
                }
            }
            SourceInfo::Generated { .. } => {
                // Walk through the Invocation anchor's chain.
                self.invocation_anchor()
                    .and_then(|si| si.preimage_in(target))
            }
        }
    }
}
```

The `Generated` case delegates to `invocation_anchor()`, which returns
the first `Invocation` anchor's source_info — typically an `Original`
covering the source token's bytes. So a shortcode-resolution Generated
successfully returns its preimage range; the writer Verbatim-copies the
shortcode token from source.

## Open questions for implementation

- **Inline-level Transparent**: today the writer has `InlineSplice` for
  inline-level changes within a block. Does Transparent apply to inlines
  too (e.g., a `Span` with Generated source_info containing
  source-bearing inlines)? Probably yes — extend the same pattern.
  Confirm during implementation.
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
    types because they aren't known at hub-client / preview-renderer
    build time): a `wasm_bindgen` export
    `get_atomic_custom_node_types()` is called once per render after
    extensions are loaded; populates a React context. The
    hand-mirrored TS const remains the fallback for the
    no-extensions / WASM-initializing case and stays correct for
    built-ins.
  - **Plan 8's `IncludeExpansion`**: lands in the built-in set today
    via `pub const ATOMIC_CUSTOM_NODES`. After the follow-up plan, the
    set is built from a built-in's `_extension.yml` rather than
    hardcoded — same effect via the same code path that user
    extensions use, no privileged route.

  This sketch commits the schema choice (`contributes.custom-nodes`
  with `atomic: bool`) and the function-signature migration path.
  Plan 7 ships the const-based registry; the runtime aggregation,
  schema parsing, and `wasm_bindgen` lookup all land in a follow-up
  when an extension actually needs to register an atomic type.
- **Runtime user-filter idempotence detection**: split out to Plan 7a.
  See `claude-notes/plans/2026-05-04-q2-preview-plan-7a-user-filter-idempotence.md`
  for the full design — round-trip idempotence check, per-filter
  attribution, `idempotent: false` opt-out, Q-3-44 / Q-3-45
  diagnostics. Plan 7a is a separable follow-up that builds on Plan
  7's `pipeline_kind: Some("preview")` machinery; it doesn't gate M3.

## References

- `crates/pampa/src/writers/incremental.rs` — the writer to modify.
  Particularly `coarsen` (line 149), `assemble` (line 228),
  `compute_separator` (line 354), `block_source_span` (line 447), the
  helper for inline byte ranges (line 800).
- `crates/quarto-source-map/src/source_info.rs:185-237` — accessor
  patterns to extend.
- `crates/wasm-quarto-hub-client/src/lib.rs:2947` —
  `incremental_write_qmd` entry point to extend.
- `ts-packages/preview-runtime/src/wasmRenderer.ts:712` — the JS
  wrapper (path moved from hub-client during Phase D — both hub-client
  and the q2 preview SPA import from this shared package).
- `hub-client/src/components/render/ReactPreview.tsx:429-440` —
  `handleSetAst` guard to lift. Plan 1 implemented the `doRender`
  format switch via `pipelineKindForFormat(format)` already; Plan 7
  wires the same helper into the edit-back path so the guard can be
  replaced with a call to `incrementalWriteQmd` that passes the
  `pipeline_kind`.
- `q2-preview-spa/src/PreviewApp.tsx:240-243` — `noopSetAst` to
  replace with a real handler that routes through automerge.
- `hub-client/src/hooks/useAutomergeSync.ts:225-240` —
  `applyingRemoteRef` + `handleContentRewrite` pattern the SPA needs to
  mirror.
- `hub-client/src/components/Editor.tsx:923-933` — existing
  `.diagnostics-banner` for unlocated diagnostics (hub-client side; the
  SPA needs its own equivalent component).
- `hub-client/src/components/Editor.css:16-55` — banner CSS the SPA
  can mirror in its own DiagnosticStrip styling.
- `hub-client/src/utils/diagnosticToMonaco.ts:90` —
  `diagnosticsToMarkers` splits diagnostics into Monaco markers and
  unlocated; hub-client-only because the SPA has no Monaco.
- `hub-client/src/utils/pipelineKind.ts` — Plan 1's TS helper
  (`pipelineKindForFormat`); Plan 7's JS-side call site reads it.
- `crates/quarto-core/src/stage/stages/ast_transforms.rs` —
  `AstTransformsStage::run()` JIT branch already dispatches on
  `ctx.format.pipeline_kind` (Plan 1); no edit needed for Plan 7
  itself.
- `crates/quarto-core/src/format.rs` — `Format::pipeline_kind` (Plan
  1); Plan 7 reads it in the `incremental_write_qmd` body to drive
  the baseline-pipeline selection.
- Plans 4 (Generated + Anchor + By + is_atomic_kind), 5 (wire format),
  6 (audit) — provide the AST shape this plan walks.
- Plan 3 — ships `compute_meta_hash_fresh` /
  `compute_meta_hash_fresh_excluding_rendered` in `quarto-ast-reconcile`;
  the writer-lossless baseline test below uses both. Plan 3's fixture
  set under `crates/quarto-core/tests/fixtures/q2-preview-idempotence/`
  is the starting point for the baseline test's fixture inputs.

## Test plan

- **Writer-lossless baseline test** (prerequisite for the reconciler
  tests below; lands in Plan 7's first commit alongside the foundation
  test). For each AST shape the writer needs to emit
  (Generated-with-Invocation shortcode resolutions, IncludeExpansion
  CustomNode wrappers, FloatRefTarget / Theorem / Proof / Callout
  CustomNodes, synthesized Sectionize / Footnotes / Appendix
  containers, user-edited variants of each), assert that
  `parse(write(ast))` produces an AST whose
  `compute_blocks_hash_fresh` + `compute_meta_hash_fresh_excluding_rendered`
  (from `quarto-ast-reconcile`, landed by Plan 3) equal the input's.
  This isolates writer bugs from reconciler bugs: a reconciler test
  failing on one of these shapes can be diagnosed as a writer-lossless
  baseline regression vs. a reconciliation regression.
  Fixtures reuse Plan 3's set (in
  `crates/quarto-core/tests/fixtures/q2-preview-idempotence/`) plus
  any Plan 7-specific shapes (e.g., a doc with an
  IncludeExpansion CustomNode that has been user-edited).
- **Reconciler source-info-blindness foundation test** (new, lands in
  Plan 7's first commit): asserts that `structural_eq_blocks` and
  `structural_eq_inlines` (in `quarto-ast-reconcile`) return `true` for
  pairs of nodes that differ *only* in source_info. Cover the new
  shapes: two Original blocks with different file IDs / offsets; two
  Generated blocks with different `By` payloads; two Generated blocks
  with different anchor lists but the same content/attr/plain_data;
  CustomNode pairs differing only in source_info on the wrapper or in
  any slot child. The hash function already excludes source_info
  (verified by Plan 3 and existing
  `compute_blocks_hash_fresh::test_same_content_same_hash`); this test
  covers the *equality* path too.
- **`preimage_in` unit tests**: each variant (Original same/different
  file, Substring chain, Concat contiguous/gappy, Generated with no
  anchors, Generated with Invocation anchor resolving into target,
  Generated with Invocation anchor resolving elsewhere). Assert correct
  byte range or None.
- **Coarsen unit tests**: build mock reconciliation plans + ASTs covering:
  - Verbatim (KeepBefore + preimage in target, both Original-source
    and Generated-with-Invocation cases).
  - Transparent (KeepBefore + non-atomic Generated wrapper with
    source-bearing children — Sectionize / footnotes / appendix cases).
  - Omit via atomic-kind Generated (KeepBefore + Generated with
    `by.is_atomic_kind() == true` and no anchors — filter
    construction case).
  - Omit via non-atomic Generated with no children (rare).
  - Rewrite (UseAfter, non-atomic).
  - **Soft-drop: inline UseAfter on atomic-Generated** — substitute
    KeepBefore for that inline, surrounding inline plan continues;
    assert `Q-3-42` warning emitted.
  - **Soft-drop: block RecurseIntoContainer on atomic CustomNode**
    (IncludeExpansion) — substitute KeepBefore for the wrapper;
    assert `Q-3-43` warning emitted; assert wrapper's preimage bytes
    in output.
  - **Let-user-win: block UseAfter on atomic node** — Rewrite via qmd
    writer; no warning. Assert qmd writer's CustomNode arm correctly
    serializes a fresh user-edit-tagged IncludeExpansion (uses
    `plain_data["source_path"]`).
- **Multi-inline dedupe unit tests**: build a Para with three
  consecutive inlines all sharing the same `Invocation` anchor
  source_info. Reconcile against an identical Para. Assert the writer
  emits the shortcode token bytes ONCE, not three times, in the
  inline-assembly output.
- **Soft-drop interaction tests**:
  - User edits one shortcode-resolved inline AND a non-atomic inline
    in the same Para → assert non-atomic edit is applied AND shortcode
    token is preserved AND `Q-3-42` warning emitted.
  - User edits inside an include AND outside the include in same doc →
    assert outside edit is applied AND include token is preserved AND
    `Q-3-43` warning emitted (write succeeds with warnings, not Err).
- **End-to-end round-trip tests** (hub-client):
  - Sectionized doc → edit one paragraph → assert the section
    structure is preserved verbatim except for the edit.
  - Doc with single-inline shortcode (`{{< meta title >}}`) → edit a
    different paragraph → assert the shortcode token is preserved.
  - Doc with multi-inline shortcode (markdown title) → edit a
    different paragraph in same Para → assert the shortcode token
    appears once, not multiple times.
  - Doc with shortcode → attempt to edit the resolved title → assert
    `Q-3-42` warning + the document text is byte-equal to a no-op
    edit (i.e., the bad edit was reverted). Save succeeded.
  - (Plan 8 covers includes; this plan establishes the infrastructure.)
- **End-to-end round-trip tests (SPA)**:
  - SPA boots against a project with a single doc; edit a paragraph
    via setLocalAst; assert the qmd on disk reflects the edit and
    automerge content matches.
  - Single-file mode (bd-tnm3k): same test with a `.qmd` outside any
    `_quarto.yml` project root; assert the original file path is
    written.
  - Edit a shortcode in the SPA → assert Q-3-42 warning appears in
    DiagnosticStrip; assert qmd on disk is unchanged.
  - Edit a non-atomic block and a shortcode-resolved inline together
    → assert non-atomic edit applies, shortcode preserved, Q-3-42
    warning shows.
- **Filter-construction soft-drop test**: build an AST with a
  filter-constructed Str (Generated { by: filter, from: [] }) inside
  a Para. User retypes it through React → assert `Q-3-42` warning + the
  original Para's source bytes (without the decoration) appear in
  output. Next pipeline run regenerates the decoration.
- **Idempotence holds**: re-run Plan 3's idempotence test after this
  plan lands. The AST shape changes from this plan's transforms
  shouldn't break it.

## Dependencies

- Depends on: Plans 4 (Generated + Anchor + By + is_atomic_kind), 5
  (wire format), 6 (audit + Invocation anchors on shortcode
  resolutions).
- Soft-depends on Plan 3 for `compute_meta_hash_fresh` (used by the
  writer-lossless baseline test). If Plan 3 hasn't landed when this
  plan starts, the helper can be inlined into Plan 7's test crate and
  promoted to `quarto-ast-reconcile` when Plan 3 catches up — but
  landing Plan 3 first avoids the duplication.
- Blocks: nothing structurally; Plan 8 builds on the atomic
  infrastructure but is independent (uses `is_atomic_custom_node` for
  IncludeExpansion).
- Lifts the read-only mode that Plan 1 introduced for q2-preview.
- Lights up the q2 preview SPA's edit-back path (which currently uses
  `noopSetAst`).

## Risk areas

- **`incremental.rs` is intricate**: ~1000 lines, many interlocking
  functions. Adding new coarsen variants and rewiring assemble
  carefully is the meat of this plan. Budget extra time for edge
  cases.
- **Plans 4 / 5 / 6 must land first**. The writer can't test
  Generated-with-anchor walking without those types existing. Order
  matters strictly.
- **InlineSplice + Transparent interaction**: the existing InlineSplice
  logic handles inline-level changes. If Transparent at the block
  level recurses into a block whose inlines need splicing, the
  assembly logic composes both. Test this case — it's the trickiest
  edge.
- **Soft-drop warning visibility**: warnings flow through the existing
  `RenderResponse.warnings` channel (the same path Plan 1's pipeline
  diagnostics and attribution-render diagnostics use). Hub-client
  already collates them in `ReactPreview.tsx`; Editor's
  `diagnosticsToMarkers` splits into Monaco markers and the existing
  `.diagnostics-banner`. SPA needs the new `DiagnosticStrip`.
- **SPA write-loop suppression**: the `applyingRemoteRef` pattern
  hub-client uses must be mirrored in the SPA. Without it, the SPA's
  setAst → updateFileContent → automerge → re-sync → re-render cycle
  could re-fire setAst on the round-trip-equivalent AST. Mitigation:
  set a flag before initiating the write; clear it on the next
  re-render that's structurally equal to what we just wrote. Mirrors
  the hub-client pattern.
- **Autosave-context spam mitigation for Q-3-42 / Q-3-43**: hub-client
  and SPA both use Automerge as the source-of-truth for qmd source —
  there's no discrete "save" action; every keystroke triggers a
  debounced render and incremental write. So a user persistently
  typing over an atomic-resolved inline would re-fire Q-3-42 on every
  render, flooding the diagnostic surface with copies of the same
  warning. Same for Q-3-43 if the user keeps editing inside an
  include.

  **Mitigation**: suppress-after-3 by source range. The Monaco
  squiggle (yellow underline at the affected source range) remains as
  the persistent signal in hub-client; the side-panel banner /
  DiagnosticStrip shows the first three occurrences per source range
  and silently drops the rest. Implemented at the diagnostic-ingest
  layer (`ReactPreview.tsx`'s allDiagnostics collation for hub-client;
  `DiagnosticStrip` for SPA), not at the writer. Plan 7a's Q-3-44
  doesn't have this issue — it's cached once per document per
  session, so it fires at most once.

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
| `preimage_in` accessor (with Generated/Invocation) + tests | ~100 |
| New `CoarsenedEntry` variants (Transparent, Omit) | ~20 |
| `coarsen` logic update (atomic detection + soft-drop substitutions) | ~180 |
| `assemble` updates (Transparent walk, Omit handling) | ~80 |
| Multi-inline shortcode dedupe rule in inline assembly | ~40 |
| Inline-level soft-drop substitution in inline plan | ~50 |
| `is_atomic_custom_node` registry + TS hand-mirror | ~40 |
| Q-3-42 / Q-3-43 diagnostic codes + catalog entries | ~40 |
| Warning channel plumbing through coarsen → incremental_write return | ~50 |
| `pipeline_kind` parameter + WASM bridge + TS wrapper | ~80 |
| ReactPreview guard lift + edit-back wiring (hub-client) | ~20 |
| SPA setAst handler + applyingRemoteRef pattern | ~40 |
| `DiagnosticStrip` component for SPA (TSX + CSS) | ~70 |
| Tests (unit + end-to-end round-trip + soft-drop interactions, both surfaces) | ~500 |
| **Total** | **~1310** |

Two focused sessions likely. Flagged as one of the highest-complexity
plans; extend the budget if the InlineSplice + Transparent composition
surfaces unexpected interactions.

## Notes

This is the most intricate plan in the set. It's the keystone for M3 —
once this lands, q2-preview is truly editable for the common case in
BOTH hub-client and the q2 preview SPA. Take care with the test
coverage; round-trip bugs in the writer can corrupt source silently if
not caught.

### Soft-drop replaces hard-abort (retained from earlier draft)

An earlier draft of this plan had AtomicViolation as a hard error —
any edit to atomic content aborted the entire write. We revised to
soft-drop: each bad-edit case substitutes a safe alignment in coarsen
and emits a warning, but the user's other edits go through. The
user-facing contract "this edit must be prohibited" is honored (the
bad edit doesn't apply); the user-facing failure mode "the entire
save was rejected" is not. React (Plan 2A's framework atomic gate) is
the primary safeguard via read-only enforcement; the writer is the
contract guarantor; if React has a hole the writer protects without
losing the user's session.

The let-user-win exception for block-level UseAfter on atomic
(user-replaced or -deleted atomic block via React) is a deliberate
asymmetry: when the user explicitly destroys an atomic block, we trust
them. The qmd writer's CustomNode arms know how to write fresh atomic
types from `plain_data` (Plan 8's IncludeExpansion arm reads
`plain_data["source_path"]`), so this composes through the normal
Rewrite path with no special handling.

### Filter mutations are not flagged as atomic — accepted corner

Plan 4 distinguishes filter constructions (`pandoc.Str("decoration")`
→ `Generated { by: filter, from: [] }`, atomic) from filter
mutations (`Str.text = upper(Str.text)` → keeps Original source_info,
NOT atomic).

A user editing a filter-mutated Str through React produces an unusual
round-trip: the user types "world" over the filter-output "HELLO"; the
writer Rewrites "world" to source; the next pipeline run filters
"world" → "WORLD". For idempotent filters (uppercase) this is fine —
the typed text round-trips through filter to itself. For
non-idempotent filters (`x => upper(x) + "!"`) the typed text gets a
`!` appended on every save, which is confusing.

We accept this corner rather than flagging filter mutations as
atomic because (a) it would require revising Plan 4 to track filter
mutations distinctly from plain Original source_info (a notable
type-system change), (b) Plan 7a's runtime user-filter idempotence
detection catches the AST-level non-idempotence that would actually
corrupt round-trip, and (c) Plan 3's idempotence test enforces the
contract for built-in filters at CI time. Users who write
non-idempotent filters get a warning at runtime and can decide
whether the trade-off is acceptable for their workflow.

### The byte-provenance contract (and why the writer stays infallible)

The contract isn't "no materialization" — that phrasing is too blunt
and conflates two cases. **The writer materializes constantly** in the
neutral sense: every Rewrite path materializes new bytes through the
qmd writer; even Verbatim copies are a kind of materialization (bytes
appearing in the saved file). The contract is more precise: the
writer only emits bytes whose origin can be honestly traced to either
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
recoverable would be over-engineering, since correct coarsen makes
the case structurally absent. WASM panic-abort still kills the
session if the assertion fires, but that's the same risk profile as
any other writer bug; it's not specific to atomic enforcement, and
it's reachable only via a programming error in coarsen.

The let-user-win Rewrite path is provenance-honest: when the user
constructs a fresh `IncludeExpansion` through React (with
`plain_data = { source_path: "bar.qmd" }`) and the writer
materializes `{{< include bar.qmd >}}` into source, the bytes' origin
is the user's edit. Plan 8's qmd-writer arm reads `plain_data`,
doesn't read `source_info`, and emits the include syntax — same arm
whether the wrapper came from `IncludeExpansionStage` (pipeline) or
from React (user). That symmetry is what makes the let-user-win case
clean.
