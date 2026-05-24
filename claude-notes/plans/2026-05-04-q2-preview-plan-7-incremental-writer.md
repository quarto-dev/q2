# Plan 7 — Incremental writer: preimage walk, Transparent / Omit, atomic soft-drop, multi-inline dedupe

**Date:** 2026-05-04 (revised 2026-05-24)
**Branch:** feature/provenance
**Status:** Implementation plan (API surface settled)
**Milestone:** M3 (edit-back works for non-include, non-pure-synthesis edits)

## Epic context

Part of the **provenance epic** (Plans 3–10). Plan 7 is the keystone:
once the writer understands the typed provenance from Plans 4–6, it
can correctly round-trip user edits, soft-drop bad edits with clear
diagnostics, and surface warnings on both hub-client and the `q2
preview` SPA. The file name keeps its `q2-preview-plan-7-` form for
git-history continuity; new plans in the epic adopt the
`provenance-plan-N-` convention (see Plan 9 / Plan 10).

## Goal

Teach the incremental writer (`pampa::writers::incremental`) to
handle the typed provenance shapes Plans 4–6 introduce so that
q2-preview round-trip edits work correctly. Five new behaviors:

- **`preimage_in(target_file_id)` accessor** on `SourceInfo`: a
  recursive walk through Substring / Concat / Generated chains that
  returns the byte range in the target file if the chain resolves
  there, else `None`. For `Generated`, walks through the
  `Invocation` anchor only — never `ValueSource`, never `Dispatch`,
  never `Other`.
- **`Transparent` coarsen variant**: for `KeepBefore` nodes whose
  `source_info` is `Generated` with empty anchors AND non-atomic
  kind (Sectionize wrappers, footnotes container, appendix
  container), recurse into the children rather than emit a useless
  empty Verbatim. The wrapper itself contributes nothing to output.
- **`Omit` coarsen variant**: for `KeepBefore` nodes that have no
  preimage in target and no source-bearing children (atomic-kind
  Generated with no Invocation anchor — filter constructions,
  title-block synthesis, tree-sitter postprocess space). The node
  is dropped from output; the next pipeline run regenerates it from
  baseline content.
- **Unified editability gate, applied via soft-drop**: a region is
  editable iff it has byte-traceable preimage in the target file
  AND is not an atomic-kind `Generated` AND is not an atomic
  CustomNode. Edits to non-editable regions soft-drop with
  diagnostic warnings rather than aborting the entire write.
- **Multi-inline dedupe rule**: when assembling a run of consecutive
  inlines (in InlineSplice or inline-assembly contexts) whose
  `Invocation` anchors are structurally equal (`PartialEq`), emit
  Verbatim *once* for the group rather than N times. Handles
  multi-inline shortcode resolutions.

Plan 7 also changes the WASM-facing `incremental_write_qmd`
signature: the caller now supplies the baseline AST explicitly
instead of having the writer parse the original qmd internally.
This makes the writer pipeline-agnostic — it diffs the two ASTs
the caller hands it and writes accordingly, regardless of what
pipeline produced them.

When this plan lands, ReactPreview's read-only guard from Plan 1
lifts, and edits in q2-preview round-trip correctly. The q2-preview
SPA gains edit-back via the same writer path — replacing its
current `noopSetAst` with a real handler that routes through
`incrementalWriteQmd` to the sync-client's `updateFileContent`
and through automerge to the ephemeral hub's disk-write.

## API decomposition: parse / transform / reconcile / write

The writer is one node in a four-primitive grammar:

| Primitive | Rust signature (existing) | What it does |
|---|---|---|
| **parse** | `qmd_to_pandoc(bytes) → (Pandoc, ASTContext)` | Lex/parse qmd source to a parse-only AST. No transforms. |
| **transform** | `build_<kind>_transform_pipeline()` + `run_pipeline()` | Apply a pipeline's transform stages to a parse-only AST. Produces a same-shape AST at a different tier. |
| **reconcile** | `compute_reconciliation(&a, &b) → ReconciliationPlan` | Diff two ASTs structurally, producing a plan of KeepBefore / UseAfter / RecurseIntoContainer alignments. |
| **write** | `incremental_write(qmd, original_ast, new_ast, plan)` | Materialize the plan as qmd bytes — Verbatim-copy source bytes for KeepBefore, qmd-writer-serialize for UseAfter / Rewrite. |

The Rust internals already implement this decomposition. The WASM
bridge layer exposes the compositions that callers need.

**Pipeline tier discipline.** "Same pipeline tier" means: the
baseline AST and the new AST were both produced by the same
sequence of transform stages, applied to ASTs that were both
parsed from the same kind of source. The reconciler is tier-agnostic
— it just diffs structures — but the caller must supply ASTs at the
same tier or every Generated wrapper looks like a new insertion.
Two tiers matter today:

- **parse-only**: `parse_qmd_to_ast(content)` output. Used by
  q2-debug, q2-slides, and the WASM demos (kanban, hub-react-todo).
- **q2-preview**: `renderPageInProjectWithAttribution(path, …)`
  output (post-q2-preview-pipeline AST). Used by ReactPreview's
  q2-preview path and the q2-preview SPA.

## Scope

### In scope

#### `preimage_in` accessor (in `quarto-source-map`)

```rust
impl SourceInfo {
    pub fn preimage_in(&self, target: FileId) -> Option<Range<usize>>;
}
```

Walks Substring's `parent`, Concat's `pieces`, Generated's
`Invocation` anchor (via `invocation_anchor()`). Returns
`Some(byte_range)` if the chain resolves to an `Original` in the
target file, else `None`.

**`Invocation` is the only role consulted.** `ValueSource` (Plan 9)
and `Dispatch` (Plan 10) are diagnostic-only. `AnchorRole::Other`
roles are also not walked. This is the binary asymmetry contract:
copying bytes from a `ValueSource` source range would emit raw YAML
metadata into the body — a hard correctness bug. The contract is
documented on `preimage_in` and on `AnchorRole::Other`'s doc-comment.

Future anchor roles default to non-walked unless they're explicitly
added to `preimage_in`'s implementation. Extensions introducing
`AnchorRole::Other("…")` should treat this as a feature: their
attribution data isn't accidentally consulted by the writer.

#### Unified editability predicate

The same predicate gates two surfaces: Plan 2A's React read-only
check (preventing the user from typing into uneditable regions in
the first place) and the writer's soft-drop logic (the contract
guarantor if React has a hole).

```rust
fn is_editable_inside(node: &Node, target_file_id: FileId) -> bool {
    // Atomic CustomNodes (IncludeExpansion, CrossrefResolvedRef):
    // single replaceable units, not editable inside. The user can
    // replace them wholesale via a component menu; they can't type
    // inside them.
    if let Node::Block(Block::Custom(cn)) = node
        && is_atomic_custom_node(&cn.type_name)
    {
        return false;
    }
    // Atomic-kind Generated source_info (shortcode, filter,
    // title-block, tree-sitter-postprocess): pipeline-emitted
    // content whose user-source is the invocation token, not the
    // resolved text.
    if let SourceInfo::Generated { by, .. } = node.source_info()
        && by.is_atomic_kind()
    {
        return false;
    }
    // Catch-all: editable iff the region has byte-traceable preimage
    // in the target file. This covers:
    //   - Original in target: editable. ✓
    //   - Original / Substring rooted outside target: not editable.
    //   - Generated with Invocation anchor pointing into target:
    //     editable IFF non-atomic kind (handled above; this branch
    //     never sees atomic-kind Generated).
    //   - Generated with empty anchors (sectionize, footnotes,
    //     appendix containers): not editable — preimage_in returns
    //     None.
    //   - Generated with only ValueSource / Dispatch anchors
    //     (Plan 9/10 shapes): not editable — preimage_in walks
    //     Invocation only.
    node.source_info().preimage_in(target_file_id).is_some()
}
```

The catch-all clause is the change Plan 7 introduces over earlier
drafts. Non-atomic synthesized containers (sectionize wrappers,
footnotes container, appendix container) are now classified as
non-editable on both surfaces. Edits to them via React go through
the writer's soft-drop path; the React side classifies the region
as read-only and shows the user no edit affordance.

#### `coarsen` rules — two new entry variants plus soft-drop

`CoarsenedEntry` gains two variants alongside today's `Verbatim`,
`Rewrite`, and `InlineSplice`:

- **`Transparent`**: KeepBefore on a `Generated` wrapper with empty
  anchors AND non-atomic kind AND source-bearing children. Recurses
  on the children, producing a child-entry list. The wrapper itself
  emits nothing. Handles Sectionize, footnotes-container,
  appendix-container.
- **`Omit`**: KeepBefore on an atomic-kind `Generated` node with no
  Invocation anchor (filter-constructed leaves, title-block h1,
  tree-sitter postprocess space), OR on a non-atomic `Generated`
  with no children. The node is dropped from output; the next
  pipeline run regenerates it.

Soft-drop substitutions cover the bad-edit cases. Each substitutes
a safe alignment in coarsen and emits a warning rather than
aborting the entire write:

- **Inline-level UseAfter on a region where `is_editable_inside`
  returns false** (typically: user retyped resolved shortcode
  text): substitute KeepBefore for that one inline within the
  surrounding `InlineReconciliationPlan`. The rest of the inline
  plan continues as-is. Emit a `Q-3-42` warning.
- **Block-level RecurseIntoContainer on a region where
  `is_editable_inside` returns false** (user edited inside an
  include, OR inside a synthesized-from-metadata container):
  substitute KeepBefore for the wrapper. For an atomic CustomNode
  (include), the wrapper's `source_info` is Original pointing at
  the include token; Verbatim copy preserves it. For a no-preimage
  `Generated` container, the substitution lands in `Omit` — the
  container regenerates next pipeline run. Either way, inner edits
  never reach the qmd writer's arm. Emit a `Q-3-43` warning.
- **Block-level UseAfter on a region where `is_editable_inside`
  returns false but the node is an atomic CustomNode** (user
  replaced or deleted an atomic block via React's component menu):
  **let-user-win** — keep as Rewrite. The qmd writer's CustomNode
  arm reads `plain_data` and emits the include syntax from a fresh
  user-edit-tagged CustomNode. No warning — the menu is the
  affordance the user took; the intent is unambiguous.
- **Block-level UseAfter on a region where `is_editable_inside`
  returns false and the node has no preimage** (user replaced a
  synthesized-from-metadata container via React): soft-drop —
  there's no source byte range to anchor a Rewrite at. Substitute
  Omit; the original container regenerates next pipeline run.
  Emit a `Q-3-43` warning.

Earlier drafts had an `AtomicViolation` variant that caused
`incremental_write` to return `Err`. Soft-drop replaces it: every
bad-edit case has a safe substitution, so `AtomicViolation` is
unnecessary. The writer's return type carries warnings alongside
the saved qmd, not as fatal errors.

#### Coarsen pseudo-code

```
fn coarsen(...) -> Vec<CoarsenedEntry>:

For each block alignment from the reconciler:

if alignment is KeepBefore(orig_idx):
    let block = original_ast.blocks[orig_idx];
    if let Some(range) = block.source_info().preimage_in(target_file) {
        // Original / Substring / Concat-contiguous / Generated-via-
        // Invocation-anchor: all resolve here uniformly. Atomic-kind
        // shortcode case lands here too — its Invocation anchor
        // resolves to the token bytes.
        CoarsenedEntry::Verbatim { byte_range: range, orig_idx }
    }
    else if matches!(block.source_info(), SourceInfo::Generated { by, .. })
        && by.is_atomic_kind()
    {
        // Atomic-kind Generated with no Invocation anchor (filter
        // construction, title-block, tree-sitter-postprocess).
        // Drop from output; baseline regenerates next pipeline run.
        //
        // Belt-and-suspenders enforcement of Plan 4's required-anchor
        // invariant for shortcode: a shortcode-Generated without an
        // Invocation anchor would mean silent data loss.
        debug_assert!(
            !by.is_kind("shortcode"),
            "Generated {{ by: shortcode, from: [] }} reached the writer — \
             Plan 6's stamper must always attach an Invocation anchor \
             for shortcode resolutions."
        );
        CoarsenedEntry::Omit
    }
    else if matches!(block.source_info(), SourceInfo::Generated { .. })
        && block has source-bearing children
    {
        // Non-atomic Generated wrapper (Sectionize, footnotes-container,
        // appendix-container) with source-bearing children: Transparent
        // recurse.
        CoarsenedEntry::Transparent { child_entries: <recurse on children> }
    }
    else {
        // Catch-all: KeepBefore with no preimage and no Generated-cascade
        // shape that maps to Omit or Transparent. Examples: cross-file
        // Original (no Plan-8 wrapper yet), Substring chain rooted outside
        // target, gappy Concat. Fall back to Rewrite — re-serialize the
        // unchanged block through the qmd writer. Lossy at the byte level
        // (whitespace, formatting may shuffle) but preserves content. The
        // earlier draft routed these to Omit; that path was data-loss-shaped
        // and should never reach the writer.
        //
        // The reconciler's KeepBefore alignment ties orig_idx to a specific
        // new-side block (they were classified structurally equal). The
        // catch-all serializes that aligned new-side block — equivalently
        // the original-side block, since they compare equal — so the
        // existing `Rewrite { new_idx }` variant fits without modification.
        // Coarsen looks up the aligned new_idx from the plan; no separate
        // variant or field is needed.
        CoarsenedEntry::Rewrite { new_idx: aligned_new_idx }
    }

if alignment is UseAfter(new_idx):
    let new_block = new_ast.blocks[new_idx];
    let was_atomic_custom_node = matches!(&new_block, Block::Custom(cn)
        if is_atomic_custom_node(&cn.type_name));
    let was_no_preimage_generated = matches!(new_block.source_info(),
        SourceInfo::Generated { .. })
        && new_block.source_info().preimage_in(target_file).is_none();

    if !was_atomic_custom_node && was_no_preimage_generated {
        // User replaced a synthesized-from-metadata container wholesale.
        // No source position to anchor at; can't Rewrite. Soft-drop.
        warnings.push(diagnostic_q3_43_widened(new_block));
        CoarsenedEntry::Omit
    } else {
        // Let user win — including for atomic CustomNodes (the user
        // replaced an include via the component menu; the qmd writer's
        // CustomNode arm handles this).
        CoarsenedEntry::Rewrite { new_idx }
    }

if alignment is RecurseIntoContainer { before_idx, after_idx }:
    let block = original_ast.blocks[before_idx];
    if !is_editable_inside(block, target_file) {
        // Inner edits to a non-editable container are reverted.
        warnings.push(diagnostic_q3_43(block));
        if let Some(range) = block.source_info().preimage_in(target_file) {
            // Atomic CustomNode with preimage (include token): Verbatim.
            CoarsenedEntry::Verbatim { byte_range: range, orig_idx: before_idx }
        } else {
            // No-preimage container (synthesized): Omit; regenerates next run.
            CoarsenedEntry::Omit
        }
    } else {
        // Existing recurse logic for inline plans, custom_node_plans, etc.
        // Inline-plan-walking has its own soft-drop substitution
        // (see "Inline-level soft-drop" below).
        ...
    }
```

#### Inline-level soft-drop

Applied during `assemble_inline_content` and when constructing the
inline plan for `InlineSplice`:

```
For each inline alignment in plan.inline_alignments:

if alignment is UseAfter(new_idx) and !is_editable_inside(orig_inlines[before_idx], target):
    // User retyped over a non-editable inline (typically: shortcode
    // resolution). Substitute KeepBefore for the original inline at
    // before_idx — the position the alignment already names. The
    // earlier draft suggested matching the *new* inline's Invocation
    // anchor against original-side anchors, but user-edit inlines
    // don't carry Invocation anchors so there'd be nothing to match.
    warnings.push(diagnostic_q3_42(orig_inlines[before_idx]));
    treat as KeepBefore(before_idx)

if alignment is RecurseIntoContainer and !is_editable_inside(orig_inlines[before_idx], target):
    warnings.push(diagnostic_q3_42(orig_inlines[before_idx]));
    treat as KeepBefore(before_idx)
```

#### `assemble` updates

- **Transparent entries** emit each child's bytes with separators
  computed from the children's original positions. The wrapper
  itself contributes nothing.
- **Omit entries** contribute nothing to output. The original
  `Generated` node is dropped; baseline regenerates next pipeline
  run.
- **Multi-inline dedupe**: within an inline-splice or inline-assembly
  run, detect consecutive `KeepBefore` entries whose inlines'
  `Invocation` anchors are structurally equal (compared via
  `PartialEq` on the anchor's `source_info` — `SourceInfo` derives
  `PartialEq`, so value equality across the full chain). Emit
  Verbatim *once* for the group, using the anchor's preimage byte
  range. Without dedupe, a multi-inline shortcode resolution like
  `**Bold** Title` → `[Strong[Str], Space, Str]` would emit the
  shortcode token N times.
- No `AtomicViolation` handling — soft-drop substitutions happened
  in coarsen; `assemble` sees only safe entries.

#### `incremental_write_qmd` signature change

Today:
```rust
pub fn incremental_write_qmd(original_qmd: &str, new_ast_json: &str) -> String;
```

After Plan 7:
```rust
pub fn incremental_write_qmd(
    original_qmd: &str,
    baseline_ast_json: &str,
    new_ast_json: &str,
) -> String;  // JSON: { success, qmd, warnings, error?, diagnostics? }
```

The third positional argument (`baseline_ast_json`) is the
caller-supplied baseline AST at the same pipeline tier as
`new_ast_json`. The writer no longer parses `original_qmd` to
synthesize a baseline; it uses the caller-supplied one. This makes
the writer pipeline-agnostic: it diffs the two ASTs it's given and
writes accordingly.

The TS wrapper at `ts-packages/preview-runtime/src/wasmRenderer.ts`
mirrors the signature change: `incrementalWriteQmd(originalQmd,
baselineAst, newAst): { qmd, warnings }` (today: `(originalQmd,
newAst): string`).

No `pipeline_kind` parameter. The pipeline tier is implicit in
whichever baseline AST the caller passes.

#### Warning channel mechanism

`coarsen` accepts a `&mut Vec<DiagnosticMessage>` warning sink as
a parameter. Soft-drop substitutions push warnings into the sink.
The WASM bridge serializes the warnings into the response JSON's
existing `warnings` field (already present on `AstResponse`; today
always `None` for `incremental_write_qmd`). The TS wrapper returns
`{ qmd, warnings }`. The hub-client's existing diagnostic collation
(`ReactPreview.tsx::allDiagnostics`, `Editor::diagnosticsToMarkers`)
displays soft-drop warnings the same way it displays pipeline
diagnostics — as Monaco squiggles for located warnings, and as
the `.diagnostics-banner` for unlocated.

#### Diagnostic codes

Two codes, registered in
`crates/quarto-error-reporting/error_catalog.json`:

- **`Q-3-42` — "Shortcode edit dropped".** Emitted when an
  inline-level edit to shortcode-resolved (or other atomic-Generated)
  content was substituted by KeepBefore. Body: the affected inline's
  text and the source range of the invocation token (from the
  `Invocation` anchor) so editor UIs can highlight it.

- **`Q-3-43` — "Generated content edit dropped".** Three emission
  paths, sharing the same code and structural shape:
  - Block-level RecurseIntoContainer on an atomic CustomNode
    (Plan 8's `IncludeExpansion`): body names the include's
    `source_path` from `plain_data`. Message: "To edit this content,
    open `<source_path>` directly."
  - Block-level RecurseIntoContainer on a no-preimage Generated
    container (synthesized appendix / footnotes container after
    Plan 9 stamps ValueSource anchors): body names the metadata
    key when available. Message: "This content is generated from
    metadata; edit `_quarto.yml` / frontmatter to change it."
  - Block-level UseAfter on a no-preimage Generated container:
    same body as the previous case.

Both are `DiagnosticKind::Warning`. Both carry source ranges
(the wrapper's preimage range when available, else the surrounding
block's range), so they squiggle naturally in Monaco.

**Catalog mechanics** (verified). Each Q-* code in
`error_catalog.json` carries one static `message_template` plus
title / subsystem / docs_url. Per-call-site body text uses the
existing `DiagnosticMessageBuilder` API
(`crates/quarto-error-reporting/src/builder.rs`):

```rust
DiagnosticMessageBuilder::warning("Generated content edit dropped")
    .with_code("Q-3-43")
    .problem(format!("To edit this content, open `{}` directly.",
                     source_path))
    .add_hint("...")
    .build()
```

The catalog entry provides one generic `message_template`; the
three emission paths supply their distinct text via the builder.
**No template-able-body infrastructure needed** — the existing
builder API already covers it. Phase 3 ships one catalog entry per
code and three builder helper functions (`diagnostic_q3_42`,
`diagnostic_q3_43_include`, `diagnostic_q3_43_metadata`).

#### `is_atomic_custom_node` registry

Defined in `quarto-core` as:
```rust
pub const ATOMIC_CUSTOM_NODES: &[&str] = &["CrossrefResolvedRef"];
pub fn is_atomic_custom_node(type_name: &str) -> bool;
```

Plan 7 ships the Rust side. The TypeScript hand-mirror at
`ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts`
already exists (Plan 2A shipped it with `CrossrefResolvedRef`).
Plan 8 adds `IncludeExpansion` to both sides.

Extensions that need to contribute atomic types use a future
registration mechanism (see §Open questions); the const set
covers built-ins.

#### Hub-client integration

**Scope clarification: first-demo UX.** Plan 7 lifts the coarse
`pipelineKindForFormat(format) === 'preview'` read-only guard at
`ReactPreview.tsx:429-440` and replaces it with the writer's
soft-drop path. The writer's Q-3-42 / Q-3-43 diagnostics are the
user-facing safety net for the first demo — bad edits don't reach
source, and the user sees a warning. A fine-grained React-side
gate (greying out the affordance per region via the
`is_editable_inside` predicate consulted from JS) is **deferred**
to a future frontend pass. For the first demo, the experience is
"you can type, but it doesn't take, and you see a warning"; that
is the deliverable. Plan 2A's existing atomic-CustomNode gate
continues to prevent the most surprising cases (editing inside
includes) without further work.

- Lift the `handleSetAst` read-only guard in `ReactPreview.tsx:429-440`
  introduced in Plan 1. Wire `setLocalAst` through with the current
  `ast` state as the baseline:
  ```ts
  const handleSetAst = useCallback((newAst) => {
    const { qmd, warnings } = incrementalWriteQmd(content, ast, JSON.stringify(newAst));
    // process warnings (Q-3-42, Q-3-43) into allDiagnostics
    onContentRewrite(qmd);
  }, [content, ast, onContentRewrite]);
  ```
  The `ast` state already holds the previously-rendered post-pipeline
  AST (set by the regular render effect on every successful render).
  No new caching mechanism is required; React's `useState` is the
  cache.

#### q2 preview SPA integration

- Replace `noopSetAst` at `q2-preview-spa/src/PreviewApp.tsx:241`
  with a real handler that calls `incrementalWriteQmd(content,
  baselineAst, newAst)` and then `syncClient.updateFileContent(path,
  newQmd)`. The baseline AST is the SPA's currently-displayed AST
  (mirror of ReactPreview's `ast` state).
- Add **content-match echo-prevention** in the SPA's
  `onFileContent` handler. Just before calling
  `updateFileContent`, hash the qmd being emitted (e.g. SHA-256 or
  a cheaper FNV-1a — exact algorithm settled during implementation)
  and stash `(path, hash)` in a ref. In `onFileContent(path,
  content)`, suppress the re-render if `(path, hash(content))`
  matches the stashed value; otherwise process normally. Robust
  against interleaved unrelated file updates (an unrelated file's
  `onFileContent` doesn't match the stashed `path`, so it processes
  normally).
- Ship `q2-preview-spa/src/components/DiagnosticStrip.tsx` — a
  small SPA-local component (~50 lines TSX + ~20 lines CSS) that
  displays Q-3-42 / Q-3-43 warnings returned by `incrementalWriteQmd`.
  Mirrors hub-client's `.diagnostics-banner` visual style. Applies
  suppress-after-3-by-source-range (see "Autosave-context spam
  mitigation" below).
- Both single-file mode (bd-tnm3k) and project mode work via the
  same code path — the ephemeral hub bridges automerge ↔ disk
  uniformly. No SPA-side branching needed.

#### Move `pipelineKindForFormat` to shared package

`pipelineKindForFormat` lives in `hub-client/src/utils/pipelineKind.ts`
today. The SPA can't import from hub-client. The writer no longer
needs the helper (no `pipeline_kind` parameter), but the SPA's
**display path** does — to choose between `parse_qmd_to_ast` and
`render_page_in_project_with_attribution` when rendering.

Move to `ts-packages/preview-runtime/src/pipelineKind.ts`. Both
hub-client and the SPA import from there. Mechanical move; ~5 LOC
of import-path updates.

#### Diagnostic surfacing in hub-client

Warnings flow through the existing `RenderResponse.warnings` channel
(the same path Plan 1's pipeline diagnostics and attribution-render
diagnostics use). `ReactPreview.tsx::allDiagnostics` already collates
them; `Editor::diagnosticsToMarkers` splits into Monaco markers and
the existing `.diagnostics-banner`. Q-3-42 and Q-3-43 both carry
source ranges, so they squiggle naturally. **No new hub-client UI
needed.**

One known UX gap: the banner is gated on `!isFullscreenPreview`, so
users in fullscreen-preview mode rely on the Monaco squiggles
(visible when they exit fullscreen) rather than the banner. Accepted.

### Out of scope

- **Include round-trip via wrapper-CustomNode** (Plan 8 — uses
  this plan's atomic-detection + soft-drop logic but introduces the
  wrapper itself).
- **Running the transform pipeline inside the writer.** The writer
  is pipeline-agnostic by design; the caller supplies the baseline
  AST at whatever tier they need. Future plans don't change this.
- **Engine output as Generated** (deferred future work).
- **Editable CustomNode slots** (e.g., editing a Callout's title and
  body through React with edits round-tripping back to source). See
  `claude-notes/research/2026-05-05-editable-custom-nodes.md`.
- **Promoting the qmd writer to a fallible `Result` interface
  throughout.** Soft-drop semantics make this unnecessary for
  q2-preview; the remaining panic paths are debug assertions for
  genuine programming errors, not user-facing failure modes. See
  §"The byte-provenance contract" below.
- **Lifting hub-client's diagnostic banner + SPA's `DiagnosticStrip`
  into a shared `@quarto/preview-renderer` component.** Filed as a
  follow-up against the hub-client decomposition epic (bd-hfjj); not
  on Plan 7's critical path.
- **Plan 7 `preimage_in` role-asymmetry unit test and
  appendix-license end-to-end round-trip test.** These exercise the
  `Invocation`-only walking behavior of `preimage_in` and the
  end-to-end correctness of soft-dropping a metadata-derived edit.
  Both depend on Plan 9 having stamped ValueSource anchors on a
  real consumer (the appendix synthesizer); both land in Plan 9's
  Phase 5. Plan 7's test plan retains the structural-only unit
  tests that don't depend on a real ValueSource consumer.

## Design decisions (settled)

- **Decompose into orthogonal primitives.** Parse / transform /
  reconcile / write are independent operations. The writer doesn't
  know about pipelines; the caller composes. The WASM bridge layer
  exposes the compositions callers actually use; future entries
  can land without changing the writer's signature.
- **Caller supplies baseline AST.** Removes the writer's dependency
  on `RenderContext`, `SystemRuntime`, `Format`, and pipeline
  construction machinery. The writer's surface is three strings
  (qmd, baseline, new) in and one JSON envelope out.
- **`Invocation` is the only anchor role the writer consults.**
  `ValueSource`, `Dispatch`, and `Other` are diagnostic-only. The
  asymmetry is load-bearing: copying bytes from a `ValueSource`
  source range would emit raw YAML into the body — a hard
  correctness bug. Documented on `preimage_in` and on
  `AnchorRole::Other`.
- **Soft-drop, not abort.** Bad-edit cases substitute a safe
  alignment in coarsen and emit a warning rather than aborting the
  entire write. The user's other valid edits go through; the bad
  edit is reverted. React (Plan 2A's framework atomic gate) is the
  primary safeguard via read-only enforcement; the writer is the
  contract guarantor; if React has a hole, the writer protects
  without losing the user's session.
- **Unified editability predicate.** Plan 2A's React-side read-only
  check and the writer's coarsen-side soft-drop logic consult the
  same `is_editable_inside(node, target_file_id) -> bool`. Three
  reasons content is uneditable: atomic CustomNode (replaceable
  wholesale via menu, not editable inside); atomic-kind Generated
  (shortcode / filter / title-block — content represents the
  resolved value of an invocation token); no preimage in target
  (synthesized-from-metadata containers — sectionize / footnotes /
  appendix, after Plan 9's stamping).
- **Let-user-win for block-level UseAfter on atomic CustomNode.**
  Replacing an `IncludeExpansion` wholesale (e.g. swapping to a
  different `source_path` via a component menu) goes through the
  qmd writer's CustomNode arm. No warning — the user's intent is
  unambiguous.
- **Soft-drop for block-level UseAfter on no-preimage Generated.**
  Replacing a synthesized-from-metadata container has no source
  position to anchor at; Rewrite would have nowhere to write.
  Substitute Omit + Q-3-43 warning.
- **Multi-inline dedupe via `PartialEq` on anchor source_info.**
  Two consecutive inlines share an `Invocation` anchor iff their
  anchor's `source_info` is `==` (value equality). `SourceInfo`
  derives `PartialEq`, so this is structural — Substring chains,
  Concat pieces, etc. compare element-wise.
- **Inline-level UseAfter substitution targets `before_idx`.** The
  alignment from the reconciler already carries the original-side
  index being replaced; the writer uses that directly. Earlier
  drafts suggested matching the *new* inline's `Invocation` anchor
  against original-side anchors — but user-edit inlines don't
  carry `Invocation` anchors, so there's nothing to match.
- **No `pipeline_kind` parameter on `incremental_write_qmd`.** The
  pipeline tier is implicit in the baseline AST the caller passes.
- **No backward-compat shim for the signature change.** Three
  first-class consumers (ReactPreview, kanban demo, hub-react-todo
  demo) + one type interface (`quarto-sync-client`'s `astOptions`)
  + one TS wrapper (`ts-packages/preview-runtime`'s
  `wasmRenderer.ts`). All in-repo, lockstep-migrable. No npm-exposed
  consumers. No wire-format persistence — the function emits qmd
  text, not a serialized envelope. The codebase has no
  `#[deprecated]` convention; the migration is one PR.
- **Plan 7 keeps its existing filename (`2026-05-04-q2-preview-
  plan-7-incremental-writer.md`)** for git-history continuity. New
  plans in the epic use the `provenance-plan-N-` convention
  (Plan 9, Plan 10).

## Multi-inline shortcode dedupe

When `{{< meta foo >}}` resolves to multiple inlines (e.g. metadata
is markdown like `**Bold** Title` → `[Strong[Str], Space, Str]`),
each resolved inline has the same `Generated { by: shortcode("meta"),
from: [Invocation -> Original{shortcode_range}] }` source_info.

**Block-level:** if both reconciliation inputs produce the same
multi-inline output, the surrounding Para is structurally identical
→ KeepBefore at block level → Verbatim copy of the WHOLE Para's
bytes (including the shortcode token). One copy. ✓

**Inline-level recursion** (when the user edits something else in
the same Para): the reconciler picks `RecurseIntoContainer` with an
inline plan. Each shortcode-derived inline is `KeepBefore`
individually. Without dedupe, each one's Verbatim would emit the
shortcode token → N copies in output.

**Dedupe rule:** when iterating inline alignments in
`assemble_inline_content`, group consecutive `KeepBefore` entries
whose inlines' `Invocation` anchors are `PartialEq`-equal. Emit
Verbatim *once* for the group, using the anchor's preimage byte
range.

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
                // Note: `SourceInfo::concat()` computes each piece's
                // `offset_in_concat` cumulatively (sum of prior lengths), so
                // gaps are structurally impossible in any Concat produced by
                // the in-repo constructors. This branch is defensive against
                // malformed JSON deserialization, not against in-repo callers.
                let ranges: Vec<_> = pieces.iter()
                    .map(|p| p.source_info.preimage_in(target))
                    .collect::<Option<Vec<_>>>()?;
                if ranges.is_empty() { return None; }
                if ranges.windows(2).all(|w| w[0].end == w[1].start) {
                    Some(ranges.first()?.start .. ranges.last()?.end)
                } else {
                    None  // defensive: gaps shouldn't arise from in-repo
                          // constructors; if they do, fall through to the
                          // catch-all Rewrite branch below.
                }
            }
            SourceInfo::Generated { .. } => {
                // Walk through the Invocation anchor's chain.
                // Never walks ValueSource (Plan 9), Dispatch (Plan 10),
                // or Other — these are diagnostic-only.
                self.invocation_anchor()
                    .and_then(|si| si.preimage_in(target))
            }
        }
    }
}
```

The `Generated` case delegates to `invocation_anchor()`, which
returns the first `Invocation` anchor's source_info — typically an
`Original` covering the source token's bytes. So a
shortcode-resolution Generated successfully returns its preimage
range; the writer Verbatim-copies the shortcode token from source.

## Migration plan

### Rust signature

```rust
// Before:
pub fn incremental_write_qmd(original_qmd: &str, new_ast_json: &str) -> String;

// After:
pub fn incremental_write_qmd(
    original_qmd: &str,
    baseline_ast_json: &str,
    new_ast_json: &str,
) -> String;
// JSON: { success, qmd, warnings, error?, diagnostics? }
```

### TypeScript wrapper (`ts-packages/preview-runtime/src/wasmRenderer.ts:712`)

```ts
// Before:
export function incrementalWriteQmd(originalQmd: string, newAst: RustQmdJson): string;

// After:
export function incrementalWriteQmd(
  originalQmd: string,
  baselineAst: RustQmdJson | string,  // accept either parsed or JSON-string for ergonomics
  newAst: RustQmdJson,
): { qmd: string; warnings: Diagnostic[] };
```

### Sync-client interface (`ts-packages/quarto-sync-client/src/types.ts:169`)

```ts
// Before:
incrementalWriteQmd?: (originalQmd: string, newAst: unknown) => string;

// After:
incrementalWriteQmd?: (
  originalQmd: string,
  baselineAst: unknown,
  newAst: unknown,
) => { qmd: string; warnings: Diagnostic[] };
```

### Sync-client call site (`ts-packages/quarto-sync-client/src/client.ts:957`)

```ts
// Before:
qmdText = astOptions.incrementalWriteQmd(cached.source, ast);

// After:
const result = astOptions.incrementalWriteQmd(cached.source, cached.ast, ast);
qmdText = result.qmd;
// Optional: surface result.warnings to a sync-client callback. Default
// to ignore; sync-client is policy-free.
```

The `astCache` already maintains both `source` (qmd) and `ast` (last
parsed AST) per file. `cached.ast` IS the baseline. No demo-side
state changes required.

### Consumer migrations

1. **`hub-client/src/components/render/ReactPreview.tsx:429-440`**
   — `handleSetAst` updated to pass the current `ast` state as the
   baseline. The existing read-only guard for `pipelineKindForFormat
   === 'preview'` deletes. Warnings from the response feed into
   `allDiagnostics` collation alongside pipeline diagnostics.

2. **`q2-demos/kanban/src/{useSyncedAst.ts:93, wasm.ts:79}`** — the
   `astOptions.incrementalWriteQmd` lambda forwards the new third
   argument. `wasm.ts:79`'s wrapper accepts and forwards
   `baselineAst`. The demo's app state is unchanged; sync-client's
   astCache supplies the baseline.

3. **`q2-demos/hub-react-todo/src/{useSyncedAst.ts:93, wasm.ts:79}`**
   — same as kanban.

4. **`q2-preview-spa/src/PreviewApp.tsx`** — new `handleSetAst`
   replaces `noopSetAst` at line 241. Routes through
   `incrementalWriteQmd(content, currentAst, newAst)` with
   content-match echo-prevention (see §Hub-client / SPA
   integration above).

All migrations in one PR; no back-compat shim. The TS-side type
checker catches every call site automatically.

## Open questions for implementation

- **Inline-level Transparent — settled: not needed.** A worktree
  scan of `crates/quarto-core/src/transforms/` and
  `crates/pampa/src/` finds zero inline-level synthesizers that
  produce `Generated { from: [] }` with non-atomic kind and
  source-bearing children. All four Plan-6 synthesizers
  (Sectionize, TitleBlock, Footnotes, Appendix) emit *block*-level
  wrappers; the inlines that synthesizers do touch (e.g. the
  Footnotes `<sup>` inline stack — Span / Superscript / Link / Str)
  carry `Original` source_info cloned from the `Note`'s range, not
  `Generated`. They hit `Verbatim` via `preimage_in`, not
  Transparent. The inline-assembly path's three variants
  (`KeepBefore` / `UseAfter` / `RecurseIntoContainer`) handle every
  shape that reaches it today; the third already preserves
  delimiters and recurses, which is what an inline Transparent
  would amount to. If a future transform begins emitting inline
  Generated-empty-from wrappers, reopen this question — the
  case is structurally absent in Plan-6-stamped output.

- **Concat-with-gaps semantics — settled: structurally
  impossible.** `SourceInfo::concat()` computes each piece's
  `offset_in_concat` as the cumulative sum of prior lengths, so a
  gap would corrupt the Concat invariant. All in-repo
  constructors (`qmd::write_with_source_info`, postprocess
  coalescing, YAML scalar combining, attribute combining, inline
  combining) feed adjacent pieces; the existing
  `concat_piece_lengths_sum_to_buffer_length` and
  `concat_covers_output_with_frontmatter` tests
  (`crates/pampa/tests/qmd_writer_source_info.rs`) lock the
  tile-the-buffer-with-no-gaps property. The `preimage_in`
  gap branch is defensive paranoia against malformed JSON
  deserialization, not against in-repo callers, and the
  catch-all Rewrite fallback is a safe graceful-degradation
  endpoint that should never fire on well-formed input.

- **`is_atomic_custom_node` extension forward-compat — out of
  scope for Plan 7.** The two atomic types today
  (`CrossrefResolvedRef`, `IncludeExpansion`) are both
  Quarto-2-internal; no extension has asked for atomic-type
  registration. Quarto 1 has no public extension-author-facing
  mechanism for custom AST node types either (verified against
  `~/src/quarto-cli` and deepwiki) — its internal registration
  is via `_quarto.ast.add_handler()` (imperative Lua call),
  not declarative YAML, and `_extension.yml` has no
  `custom-nodes:` key. If a future extension genuinely needs to
  contribute an atomic CustomNode type, a separate plan picks
  the registration shape with the right review (mirroring
  Quarto 1's imperative Lua surface, or designing a YAML
  surface, or both). Plan 7 ships the const-set with no
  extension-side coupling; the const-set's lack of an
  extension hook is intentional, not provisional.

- **Runtime user-filter idempotence detection** — split out to
  Plan 7a. See `claude-notes/plans/2026-05-04-q2-preview-plan-7a-
  user-filter-idempotence.md` for the full design — round-trip
  idempotence check, per-filter attribution, `idempotent: false`
  opt-out, Q-3-44 / Q-3-45 diagnostics. Plan 7a is a separable
  follow-up; it doesn't gate M3.

- **Content-match echo-prevention hash choice.** SHA-256 is the
  obvious safe choice (already used in Plan 7a's
  `filter_sources_hash`). FNV-1a or xxHash would be faster but
  cryptographic strength isn't needed — we're just comparing a
  freshly-emitted qmd against an arriving qmd for equality. Confirm
  during SPA implementation.

- **`pampa::pipeline::transform_ast` Rust-internal helper.**
  Extracting the transform step of `render_qmd_to_preview_ast` into
  a standalone `transform_ast(ast: Pandoc, ...) -> Pandoc` would
  let tests exercise the transform tier in isolation. ~30 LOC of
  factoring; not on Plan 7's critical path. Open beads if useful
  during implementation.

## References

### Rust

- `crates/pampa/src/writers/incremental.rs` — the writer.
  Particularly `incremental_write` (line 80), `coarsen` (line 149),
  `assemble` (line 228), `compute_separator` (line 354),
  `block_source_span` (line 448), `assemble_inline_splice` (line
  602), `assemble_inline_content` (line 632),
  `assemble_recursed_container` (line 672), `inline_source_span`
  (line 800).
- `crates/quarto-source-map/src/source_info.rs` — `SourceInfo`,
  `Generated`, `By`, `Anchor`, `AnchorRole`. Plan 7 adds the
  `preimage_in` accessor.
- `crates/quarto-ast-reconcile/src/lib.rs` —
  `compute_reconciliation`, `structural_eq_blocks`,
  `structural_eq_inlines`, `compute_blocks_hash_fresh`,
  `compute_meta_hash_fresh_excluding_rendered`. All used by the
  test plan; the reconciler API itself doesn't change.
- `crates/wasm-quarto-hub-client/src/lib.rs:2947` — WASM entry
  point (signature change).
- `crates/quarto-core/src/lib.rs` (or appropriate module) —
  `ATOMIC_CUSTOM_NODES` const + `is_atomic_custom_node` fn (new).

### TypeScript

- `ts-packages/preview-runtime/src/wasmRenderer.ts:712` — JS
  wrapper (signature change). Imports from this package; both
  hub-client and the SPA consume.
- `ts-packages/preview-runtime/src/pipelineKind.ts` — new home
  for `pipelineKindForFormat` (moved from
  `hub-client/src/utils/pipelineKind.ts`).
- `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts` —
  existing TS hand-mirror of `ATOMIC_CUSTOM_NODES`.
- `ts-packages/quarto-sync-client/src/types.ts:169` —
  `astOptions.incrementalWriteQmd` interface (signature change).
- `ts-packages/quarto-sync-client/src/client.ts:957` — sync-client
  call site (forwards new argument).
- `hub-client/src/components/render/ReactPreview.tsx:429-440` —
  `handleSetAst` guard lift + edit-back wiring.
- `q2-preview-spa/src/PreviewApp.tsx:241` — `noopSetAst` →
  real handler.
- `q2-demos/kanban/src/{useSyncedAst.ts:93, wasm.ts:79}`,
  `q2-demos/hub-react-todo/src/{useSyncedAst.ts:93, wasm.ts:79}`
  — demo wrappers (signature forwarding).

### Plans

- **Plans 4 (Generated + Anchor + By + is_atomic_kind), 5 (wire
  format), 6 (audit)** — provide the AST shape this plan walks.
- **Plan 3** — ships `compute_meta_hash_fresh` /
  `compute_meta_hash_fresh_excluding_rendered` in
  `quarto-ast-reconcile`; the writer-lossless baseline test uses
  both.
- **Plan 7a** (`claude-notes/plans/2026-05-04-q2-preview-plan-7a-
  user-filter-idempotence.md`) — separable follow-up; runtime
  user-filter idempotence check.
- **Plan 8** — uses Plan 7's atomic infrastructure for
  `IncludeExpansion`; not blocking.
- **Plan 9** (`claude-notes/plans/2026-05-22-provenance-plan-9-
  valuesource-threading.md`) — ValueSource consumer wiring;
  appendix synthesizer stamping that makes the Q-3-43-widened
  cases fire on real data. Owns the `preimage_in` role-asymmetry
  unit test and the appendix-license e2e round-trip test (Plan 9
  Phase 5).
- **Plan 10** (`claude-notes/plans/2026-05-22-provenance-plan-10-
  dispatch-anchor.md`) — Dispatch anchor for Lua sources; inherits
  Plan 7's `AnchorRole::Other` policy.

## Test plan

- **Writer-lossless baseline test** (prerequisite for the
  reconciler tests below; lands in Plan 7's first commit alongside
  the foundation test). For each AST shape the writer needs to
  emit (Generated-with-Invocation shortcode resolutions, Plan 8's
  IncludeExpansion CustomNode wrappers, FloatRefTarget / Theorem /
  Proof / Callout CustomNodes, synthesized Sectionize / Footnotes /
  Appendix containers, user-edited variants of each), assert that
  `parse(write(ast))` produces an AST whose
  `compute_blocks_hash_fresh` + `compute_meta_hash_fresh_excluding_rendered`
  equal the input's. This isolates writer bugs from reconciler
  bugs. Fixtures reuse Plan 3's set under
  `crates/quarto-core/tests/fixtures/q2-preview-idempotence/` plus
  any Plan 7-specific shapes.

- **Reconciler source-info-blindness foundation test** (lands in
  Plan 7's first commit): asserts that `structural_eq_blocks` and
  `structural_eq_inlines` return `true` for pairs of nodes that
  differ *only* in source_info. Cover: two Original blocks with
  different file IDs / offsets; two Generated blocks with different
  `By` payloads; two Generated blocks with different anchor lists
  but the same content / attr / plain_data; CustomNode pairs
  differing only in source_info on the wrapper or in any slot child.
  Why it matters: the reconciler drives KeepBefore decisions off
  these functions. If they leak source_info by accident, round-trip
  degenerates to whole-doc Rewrite without obvious symptom.

- **`preimage_in` unit tests** — each variant: Original same / other
  file, Substring chain, Concat contiguous / gappy, Generated with
  no anchors, Generated with Invocation anchor resolving into
  target, Generated with Invocation anchor resolving elsewhere.
  Assert correct byte range or None.

- **`preimage_in` skips non-Invocation roles** — Generated with
  only ValueSource / Dispatch / Other anchors returns None. (The
  full ValueSource end-to-end correctness test lives in Plan 9
  Phase 5 with real appendix-license fixtures; this Plan-7-side
  test pins the unit-level behavior.)

- **Coarsen unit tests** — build mock reconciliation plans + ASTs
  covering:
  - Verbatim (KeepBefore + preimage in target, both Original and
    Generated-with-Invocation cases).
  - Transparent (KeepBefore + non-atomic Generated wrapper with
    source-bearing children — Sectionize / footnotes / appendix).
  - Omit via atomic-kind Generated (KeepBefore + Generated with
    `by.is_atomic_kind() == true` and no anchors — filter
    construction).
  - Omit via non-atomic Generated with no children (rare).
  - Rewrite via catch-all (KeepBefore with no preimage and no
    matching Generated shape — cross-file Original, gappy Concat).
  - Rewrite (UseAfter, non-atomic, editable).
  - **Soft-drop: inline UseAfter on atomic-Generated** — substitute
    KeepBefore for that inline at `before_idx`; surrounding inline
    plan continues; assert `Q-3-42` warning emitted.
  - **Soft-drop: block RecurseIntoContainer on atomic CustomNode**
    (IncludeExpansion) — substitute KeepBefore; assert `Q-3-43`
    warning emitted; assert wrapper's preimage bytes in output.
  - **Soft-drop: block RecurseIntoContainer on no-preimage
    Generated** — substitute Omit; assert `Q-3-43` warning emitted;
    assert nothing emitted for the wrapper.
  - **Soft-drop: block UseAfter on no-preimage Generated** —
    substitute Omit; assert `Q-3-43` warning emitted.
  - **Let-user-win: block UseAfter on atomic CustomNode** — Rewrite
    via qmd writer; no warning. Assert qmd writer's CustomNode arm
    correctly serializes a fresh user-edit-tagged IncludeExpansion
    using `plain_data["source_path"]`.

- **Multi-inline dedupe unit tests** — build a Para with three
  consecutive inlines all sharing the same `Invocation` anchor
  source_info (`PartialEq`-equal). Reconcile against an identical
  Para. Assert the writer emits the shortcode token bytes ONCE,
  not three times. Also: assert dedupe does NOT fire when anchors
  differ structurally.

- **Multi-inline dedupe + ValueSource interaction** (forward-compat
  with Plan 9). Build inlines with shape `Generated { from:
  [Invocation, ValueSource] }`. Two inlines whose `Invocation`
  source_info matches but `ValueSource` source_info differs should
  still dedupe (dedupe consults Invocation only). Add this once
  Plan 9 has stamped ValueSource on a real consumer.

- **Soft-drop interaction tests:**
  - User edits one Derived inline AND a non-atomic inline in the
    same Para → non-atomic edit applied AND shortcode token
    preserved AND `Q-3-42` warning emitted.
  - User edits inside an include AND outside the include in same
    doc → outside edit applied AND include token preserved AND
    `Q-3-43` warning emitted (write succeeds with warnings, not Err).

- **End-to-end round-trip tests** (hub-client):
  - Sectionized doc → edit one paragraph → assert the section
    structure is preserved verbatim except for the edit.
  - Doc with single-inline shortcode (`{{< meta title >}}`) → edit
    a different paragraph → assert the shortcode token is preserved.
  - Doc with multi-inline shortcode (markdown title) → edit a
    different paragraph in same Para → assert the shortcode token
    appears once, not multiple times.
  - Doc with shortcode → attempt to edit the resolved title →
    assert `Q-3-42` warning + the document text is byte-equal to
    a no-op edit (the bad edit was reverted). Save succeeded.
  - Plan 8 covers includes; Plan 9 Phase 5 covers appendix-license;
    this plan establishes the infrastructure.

- **End-to-end round-trip tests (SPA):**
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
  - **Content-match echo-prevention test**: induce a local-edit ↔
    sync-echo cycle; assert the SPA's render effect fires exactly
    once after the edit completes; assert an interleaved unrelated
    file's update is processed normally (not suppressed).

- **Filter-construction soft-drop test** — build an AST with a
  filter-constructed Str (`Generated { by: filter, from: [] }`)
  inside a Para. User retypes it through React → assert `Q-3-42`
  warning + the original Para's source bytes (without the
  decoration) appear in output. Next pipeline run regenerates the
  decoration.

- **Idempotence holds** — re-run Plan 3's idempotence test after
  this plan lands. The AST shape changes shouldn't break it.

## Dependencies

### Hard dependencies

- **Plans 4 / 5 / 6** — provide the typed `Generated { by, from }`
  shape and the synthesizer stamping the writer walks. The writer
  can't test Generated-with-anchor behavior without those types
  existing and being produced by real transforms.
- **Plan 3** — `compute_meta_hash_fresh` /
  `compute_meta_hash_fresh_excluding_rendered` (used by the
  writer-lossless baseline test).

### Soft dependencies / coordination

- **Plan 9** — owns the `preimage_in` role-asymmetry e2e test and
  the appendix-license round-trip test. Plan 7's unit-level
  `preimage_in` test pins behavior; Plan 9's tests pin end-to-end
  correctness once a real ValueSource consumer (the appendix
  synthesizer) exists.
- **Plan 10** — inherits Plan 7's `AnchorRole::Other` policy. No
  ordering constraint.
- **Plan 7a** — separable follow-up; uses Plan 7's writer + warnings
  infrastructure but doesn't gate M3.
- **Plan 8** — uses Plan 7's atomic-CustomNode infrastructure but
  is independent (introduces `IncludeExpansion` to
  `ATOMIC_CUSTOM_NODES`; doesn't change Plan 7's logic).

### What Plan 7 doesn't block

- Plan 9's implementation can start in parallel; the writer-side
  changes don't depend on Plan 9's consumer wiring.
- Plan 10's implementation can start in parallel; Dispatch anchors
  are stamped by Plan 6's post-walk helper, which Plan 10 modifies
  independently.

## Risk areas

- **`incremental.rs` is intricate** (~830 lines, many interlocking
  functions). Adding new coarsen variants and rewiring assemble
  carefully is the meat of this plan. Budget extra time for edge
  cases.

- **Plans 4 / 5 / 6 must land first.** The writer can't test
  Generated-with-anchor walking without those types existing and
  being produced by real transforms. Order matters strictly.

- **InlineSplice + Transparent interaction.** The existing
  InlineSplice logic handles inline-level changes. If Transparent
  at the block level recurses into a block whose inlines need
  splicing, the assembly logic composes both. Test this case —
  it's the trickiest edge.

- **Baseline-AST staleness.** If the caller passes a baseline AST
  that doesn't match the original qmd source (e.g., the qmd source
  changed externally between render and edit), the reconciler
  produces a confused diff and the writer's output is garbage.
  Hub-client's existing `applyingRemoteRef` pattern
  (`hub-client/src/hooks/useAutomergeSync.ts:55`) and the SPA's
  content-match echo-prevention (new in this plan) keep the
  baseline fresh in practice. The contract is: caller MUST pass
  a baseline that's `parse_or_render(originalQmd) at the same tier
  as newAst`. Document this on the WASM entry and TS wrapper.

- **Soft-drop warning visibility.** Warnings flow through the
  existing `RenderResponse.warnings` channel. Hub-client already
  collates them in `ReactPreview.tsx`; Editor's
  `diagnosticsToMarkers` splits into Monaco markers and the
  existing `.diagnostics-banner`. SPA gets the new `DiagnosticStrip`.

- **SPA echo-prevention correctness.** The content-match gate
  must hash the qmd we're emitting exactly as the round-trip
  produces it (no trailing newline differences, no encoding
  variation). Implement with a fixture-based assertion: emit qmd
  X, simulate the echo loop, assert the gate matches.

- **Autosave-context spam mitigation for Q-3-42 / Q-3-43.**
  Hub-client and SPA both use Automerge as the source-of-truth for
  qmd source — there's no discrete "save" action; every keystroke
  triggers a debounced render and incremental write. A user
  persistently typing over an atomic-resolved inline would re-fire
  Q-3-42 on every render, flooding the diagnostic surface.

  **Mitigation:** suppress-after-3 by source range. Monaco squiggles
  (yellow underline at the affected source range) remain as the
  persistent signal in hub-client; the side-panel banner /
  DiagnosticStrip shows the first three occurrences per source
  range and silently drops the rest. Implemented at the
  diagnostic-ingest layer (`ReactPreview.tsx::allDiagnostics`
  collation for hub-client; `DiagnosticStrip` for SPA), not at the
  writer. Plan 7a's Q-3-44 doesn't have this issue — it's cached
  once per document per session.

  Imperative message text matters: Q-3-42 / Q-3-43 read as
  instructions ("To edit this content, open `<source_path>`")
  rather than passive descriptions ("edit was dropped"), since the
  user has no discrete-save affordance to discard the bad edit.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| `preimage_in` accessor (with Generated/Invocation) + tests | ~100 |
| New `CoarsenedEntry` variants (Transparent, Omit) | ~20 |
| `coarsen` logic update (editability gate + soft-drop substitutions) | ~200 |
| `assemble` updates (Transparent walk, Omit handling) | ~80 |
| Multi-inline shortcode dedupe (PartialEq on Invocation anchors) | ~40 |
| Inline-level soft-drop substitution | ~50 |
| `is_atomic_custom_node` registry (Rust side; TS hand-mirror already in place) | ~30 |
| Q-3-42 / Q-3-43 diagnostic codes + catalog entries | ~50 |
| Warning channel plumbing through coarsen → incremental_write return | ~50 |
| `incremental_write_qmd` WASM signature change + JSON envelope | ~40 |
| TS wrapper signature change (`incrementalWriteQmd`) | ~20 |
| Three consumer migrations (ReactPreview + 2 demos) + sync-client type | ~60 |
| ReactPreview guard lift + `ast`-state baseline wiring | ~20 |
| SPA setAst handler + content-match echo-prevention | ~50 |
| `DiagnosticStrip` component for SPA (TSX + CSS) | ~70 |
| `pipelineKindForFormat` move to `ts-packages/preview-runtime` | ~10 |
| Tests (unit + end-to-end round-trip + soft-drop interactions, both surfaces) | ~500 |
| **Total** | **~1390** |

Two focused sessions likely. Flagged as one of the highest-complexity
plans; extend the budget if the InlineSplice + Transparent
composition or the soft-drop catalog expansion surfaces unexpected
interactions.

## Implementation checklist

Work items grouped by phase. Each phase's items are roughly
sequential; phases themselves are mostly sequential, with some
parallelism noted. Plan 6 must land before Phase 1 starts.

**Coordination posture.** This checklist is sized for serial
implementation in a single fresh 1M-context session — the phases
flow linearly, and the entire plan fits comfortably in one
context window. No beads-per-phase split needed. Open a follow-up
beads only for items that surface during implementation and are
genuinely out of scope (e.g. preexisting bugs found in adjacent
code; future-plan-bound features).

### Phase 1 — Foundation primitives (`quarto-source-map`, `quarto-pandoc-types`, `pampa`)

**Implementation note (2026-05-24):** Plan originally placed
`ATOMIC_CUSTOM_NODES` / `is_atomic_custom_node` in `quarto-core`, but
`quarto-core` depends on `pampa` and the writer (in `pampa`) is the
primary consumer — that direction would cycle. Moved the registry
down to `quarto-pandoc-types` (the home of `CustomNode` itself). A
cross-check test in `quarto-core::crossref` pins the literal in
lockstep with `CROSSREF_RESOLVED_REF`.

- [x] `SourceInfo::preimage_in(target: FileId) -> Option<Range<usize>>` accessor with full match (Original, Substring, Concat, Generated)
- [x] Doc-comment on `preimage_in` stating the `Invocation`-only walking policy + asymmetry rationale
- [x] Doc-comment on `AnchorRole::Other` reiterating the policy (future roles default to non-walked)
- [x] `pub const ATOMIC_CUSTOM_NODES: &[&str] = &["CrossrefResolvedRef"]` in `quarto-pandoc-types` (not `quarto-core` — see implementation note)
- [x] `pub fn is_atomic_custom_node(type_name: &str) -> bool` in `quarto-pandoc-types`
- [x] `is_editable_inside_block` / `is_editable_inside_inline` helpers in `pampa::writers::incremental` (two functions sharing a private `is_editable_inside_source_info` core; React side will import an equivalent TS predicate in a future Phase)
- [x] `preimage_in` unit tests: Original same / different file; Substring chain; Concat contiguous / gappy / overlapping / mixed-files; Generated with no anchors; Generated with Invocation anchor resolving in / out of target; Generated with Invocation through Substring chain
- [x] `preimage_in` role-asymmetry unit test: Generated with only ValueSource / Other anchors returns None; mixed Invocation + ValueSource walks Invocation only
- [x] `is_editable_inside` unit tests covering all three uneditable reasons (atomic CustomNode, atomic-kind Generated, no-preimage Generated, value-source-only Generated) plus positive cases
- [x] Reconciler source-info-blindness foundation test in `quarto-ast-reconcile` (Generated-with-different-By, Generated-with-different-anchor-lists, CustomNode wrapper and slot-child blindness)
- [x] `cargo nextest run --workspace` green (9509 tests)
- [x] `cargo xtask verify` green (full 12-step chain including WASM build + hub-client tests)

### Phase 2 — Writer internals (`pampa::writers::incremental`)

**Implementation notes (2026-05-24):**
- The plan's checklist item "Remove `AtomicViolation` variant" was a
  residue of an earlier draft — no such variant existed in the
  pre-Plan-7 code. Marked done by omission.
- The `coarsen` signature change keeps `Result` as the return: the
  warning sink covers soft-drop cases, while the existing `Err` arm
  (reached via `?` from `assemble_inline_splice`) stays for genuine
  structural failures.
- The singleton-KeepBefore inline emit path was updated to use
  `preimage_in(target_file_id)` (with `inline_source_span` fallback).
  Original-SI inlines are byte-identical to the old behavior;
  Generated-SI inlines now emit the Invocation anchor's preimage
  bytes instead of an empty range — fixes a latent zero-length bug
  in the pre-Plan-7 inline-splice path. Multi-inline dedupe sits on
  top: when consecutive KeepBefore entries share an Invocation
  anchor, emit the anchor's preimage *once*.

**Repo facts that bite when constructing test fixtures:**
- `AttrSourceInfo` does **not** implement `Default`. Use
  `quarto_pandoc_types::AttrSourceInfo::empty()` for `Div`/`Header`/
  `Figure`/etc. `attr_source` fields in hand-built fixtures.
- `gen` is a reserved keyword in Rust 2024 edition. Don't name a
  variable `gen` (e.g. for a `SourceInfo::Generated` fixture);
  `gen_info` works.

- [x] Add `CoarsenedEntry::Transparent { child_entries }` variant
- [x] Add `CoarsenedEntry::Omit` variant
- [x] Change `coarsen` signature to accept `&mut Vec<DiagnosticMessage>` warning sink
- [x] Rewrite `coarsen` KeepBefore branch: Verbatim / Omit / Transparent / Rewrite-catch-all cascade per §"Coarsen pseudo-code"
- [x] Rewrite `coarsen` UseAfter branch: atomic-CustomNode-let-user-win, no-preimage-Generated-soft-drop
- [x] Rewrite `coarsen` RecurseIntoContainer branch: `is_editable_inside` gate + soft-drop substitution + Verbatim-or-Omit fallback
- [x] Inline-level soft-drop in `assemble_inline_content`: substitute KeepBefore via `before_idx` when `!is_editable_inside`
- [x] Multi-inline dedupe in `assemble_inline_content`: PartialEq grouping on Invocation anchor source_info
- [x] `assemble` handles Transparent (recursive child emission via `emit_entries` helper, shared `prev_entry` state across the wrapper boundary)
- [x] `assemble` handles Omit (no-op, doesn't update `prev_entry`)
- [x] ~~Remove `AtomicViolation` variant~~ — variant never existed in the codebase; checklist item was stale (see implementation note above)
- [x] Change `incremental_write` return type: `Result<(String, Vec<DiagnosticMessage>), Vec<DiagnosticMessage>>` (same for `compute_incremental_edits`); WASM bridge + all test callers migrated
- [x] `debug_assert!` for the shortcode-Generated-with-empty-from regression case (Plan 6 stamper invariant) — in `coarsen_keep_before_block`
- [ ] Writer-lossless baseline test (Plan 7 first-commit prerequisite): for each Generated / CustomNode shape, assert `parse(write(ast))` hash equals input via `compute_blocks_hash_fresh` + `compute_meta_hash_fresh_excluding_rendered` — **deferred to Plan 7b Phase 1** (`claude-notes/plans/2026-05-24-q2-preview-plan-7b-test-orama.md`)
- [x] Coarsen unit tests: Verbatim, Transparent (sectionize wrapper with source-bearing children), Omit (atomic-kind filter construction), Rewrite-catch-all (cross-file Original), Rewrite (UseAfter editable)
- [x] Coarsen soft-drop unit tests: inline UseAfter on atomic-Generated (Q-3-42); block RecurseIntoContainer on atomic CustomNode (Q-3-43, Verbatim path); block RecurseIntoContainer on no-preimage Generated (Q-3-43, Omit path); block UseAfter on no-preimage Generated (Q-3-43, Omit path)
- [x] Let-user-win unit test: block UseAfter on atomic CustomNode → Rewrite; no warning
- [x] Multi-inline dedupe unit tests: positive (anchors PartialEq-equal → one Verbatim); negative (anchors differ → individual emits); ValueSource cross-talk (Plan 9 forward-compat — anchors match on Invocation but differ on ValueSource → still dedupes)
- [ ] Soft-drop interaction test: shortcode edit + non-atomic edit in same Para — **deferred to Plan 7b Phase 1**
- [ ] Filter-construction soft-drop test (UseAfter into a filter-constructed inline) — **deferred to Plan 7b Phase 1**

### Phase 3 — Diagnostic catalog (`quarto-error-reporting`)

- [x] `Q-3-42` entry in `error_catalog.json`: title "Shortcode edit dropped"; problem text; hint text; severity Warning
- [x] `Q-3-43` entry in `error_catalog.json`: title "Generated content edit dropped"; severity Warning. (Single generic `message_template`; the three emission paths supply distinct body text via the builder — per Plan 7 §"Catalog mechanics".)
- [x] Diagnostic builder helpers `diagnostic_q3_42_inline(inline)` and `diagnostic_q3_43_block(block)` used by `coarsen`'s soft-drop sites; live in `pampa::writers::incremental` (not `quarto-error-reporting`, which doesn't depend on `quarto-pandoc-types`)
- [x] Unit tests: each soft-drop unit test asserts the correct Q-3-42 / Q-3-43 code is emitted

### Phase 4 — WASM bridge signature change (`wasm-quarto-hub-client`)

**Repo facts the implementer needs:**

- **The `wasm-quarto-hub-client` crate is NOT in the cargo workspace.**
  `cargo build -p wasm-quarto-hub-client` fails with "did not match
  any packages". Build via `cd hub-client && npm run build:wasm` or
  implicitly via `cargo xtask verify` step 6.
- **`AstResponse.warnings` is `Option<Vec<JsonDiagnostic>>`, not raw
  serde Value.** Convert via `diagnostics_to_json(&warnings, ctx)`,
  where `ctx: &SourceContext`. In the post-Phase-4 body, the
  baseline AST's `ASTContext` carries this — access via
  `baseline_context.source_context` (the field that `ASTContext`
  exposes). Phase 2 wired this via the old `original_context`
  variable; the equivalent post-Phase-4 binding is the baseline AST's.

- [x] Change `incremental_write_qmd` Rust signature: add `baseline_ast_json: &str` as second positional argument
- [x] WASM body: deserialize `baseline_ast_json` via `pampa::readers::json::read` (parallel to existing `new_ast_json` deserialization); drop the qmd-parse step
- [x] Populate `AstResponse.warnings` field from `incremental_write`'s warning vec via `diagnostics_to_json(&warnings, &baseline_context.source_context)`
- [x] Doc-comment specifies the baseline-tier contract (caller responsibility to match tier of `new_ast_json`)

### Phase 5 — TypeScript wrapper + sync-client interface

- [x] `ts-packages/preview-runtime/src/wasmRenderer.ts:712` — `incrementalWriteQmd(originalQmd, baselineAst, newAst): { qmd, warnings }`
- [x] Accept `baselineAst` as `RustQmdJson | string` for ergonomics; stringify internally
- [x] `ts-packages/preview-runtime/src/wasm-quarto-hub-client.d.ts:78` — new signature in WASM type declaration
- [x] `hub-client/src/types/wasm-quarto-hub-client.d.ts:69` — new signature in hub-client's WASM type declaration
- [x] `ts-packages/quarto-sync-client/src/types.ts:169` — `astOptions.incrementalWriteQmd` interface signature change
- [x] `ts-packages/quarto-sync-client/src/client.ts:957` — pass `cached.ast` as baseline; surface `result.qmd` to `updateFileContent`; warnings ignored at sync-client level (policy-free; demos consume them via wrapper)
- [x] Move `hub-client/src/utils/pipelineKind.ts` → `ts-packages/preview-runtime/src/pipelineKind.ts`; update imports in hub-client and SPA (SPA had no import yet — Phase 7)

### Phase 6 — Consumer migrations

- [x] `hub-client/src/components/render/ReactPreview.tsx:429-440` — `handleSetAst` updated: delete read-only guard, pass `ast` state as baseline, ingest warnings into next diagnostics push via `pendingWriteWarningsRef`
- [x] `hub-client/src/types/wasm-quarto-hub-client.d.ts:69` — type declaration updated
- [x] `q2-demos/kanban/src/wasm.ts:79` — wrapper accepts baselineAst, forwards to WASM
- [x] `q2-demos/kanban/src/useSyncedAst.ts:93` — astOptions lambda accepts third positional argument
- [x] `q2-demos/kanban/src/types/wasm-quarto-hub-client.d.ts:8` — type declaration updated
- [x] `q2-demos/hub-react-todo/src/wasm.ts:79` — wrapper signature update
- [x] `q2-demos/hub-react-todo/src/useSyncedAst.ts:93` — astOptions lambda update
- [x] `q2-demos/hub-react-todo/src/types/wasm-quarto-hub-client.d.ts:8` — type declaration update
- [x] Workspace `cargo build --workspace` + `cargo nextest run --workspace` green
- [x] `cd hub-client && npm run build:all` green (WASM type alignment)
- [x] `cd hub-client && npm run test:ci` green

### Phase 7 — q2-preview SPA integration

- [x] `q2-preview-spa/src/PreviewApp.tsx`: baseline read via `astJsonRef` mirroring `state.astJson` (avoided new state — the ref keeps `handleSetAst`'s identity stable across re-renders, which the iframe's effect-deps care about)
- [x] Replace `noopSetAst` with real `handleSetAst` that calls `incrementalWriteQmd(content, baselineJson, newAst)`
- [x] Content-match echo-prevention: hash emitted qmd via FNV-1a, stash `(path, hash)` in `lastEmittedRef`; matching incoming `onFileContent` consumes the ref and returns early
- [x] Hash algorithm decision recorded in `fnv1aHex` docstring (FNV-1a: in-process equality, 32 bits sufficient, zero-dependency, matches existing actor-color hash pattern)
- [x] `q2-preview-spa/src/components/DiagnosticStrip.tsx` component (inline styles per existing SPA convention; ~120 LOC TSX, no separate CSS file)
- [x] DiagnosticStrip ingest from `incrementalWriteQmd` result's warnings field via `writeWarnings` state
- [x] Suppress-after-3-by-source-range mitigation in DiagnosticStrip (`suppressAfterThree` helper)
- [x] Imperative message text for Q-3-42 / Q-3-43 — catalog entries already imperative from Phase 3 (`"edit the invocation token in source instead"`); DiagnosticStrip surfaces title + problem verbatim

### Phase 8 — End-to-end tests

- [x] Hub-client: WASM-level wrapper contract test (`hub-client/src/services/incrementalWrite.wasm.test.ts`) — pins the 3-arg API, identity round-trip, paragraph-edit preservation, structured error on malformed baseline JSON. Run via `npm run test:wasm`; 3/3 passing.
- [x] Plan 3's idempotence test re-run — passes within `cargo xtask verify` (9535/9535 Rust tests, includes `crates/quarto-core/tests/idempotence.rs`).
- [ ] **Deferred to Plan 7b Phases 2 + 3** (`claude-notes/plans/2026-05-24-q2-preview-plan-7b-test-orama.md`; consolidates `bd-3izo3`) — the broader Playwright scenario matrix (sectionized round-trip in a real hub session, single/multi-inline shortcode preservation, Q-3-42 byte-equal-no-op, Q-3-43 footnotes regeneration, SPA edit-paragraph round-trip in both project and single-file modes, SPA Q-3-42 DiagnosticStrip, mixed atomic + non-atomic, echo-prevention fixture). Each spec needs ~60 LOC of fixture/server setup and runs only under `cargo xtask verify --e2e`. The Rust-side soft-drop matrix is already exhaustively covered in `crates/pampa/src/writers/incremental.rs`; the deferred work is end-to-end *delivery* coverage, not new correctness coverage.

### Phase 9 — Verification + cleanup

- [x] `cargo xtask verify` green (full chain: Rust workspace + hub-build + hub-tests) — see `/tmp/plan7-phase4-6-verify.log`
- [x] **Refresh `q2 preview` WASM chain before smoke testing** (per `CLAUDE.md` §"Verifying Rust changes in `q2 preview`"; addresses the 2026-05-20 stale-WASM incident):
    - [x] `cd hub-client && npm run build:wasm` — rebuild WASM from Plan 7's Rust changes
    - [x] `cargo xtask build-q2-preview-spa` — bundle WASM into `q2-preview-spa/dist/`
    - [x] `cargo build --bin q2` — re-embed `dist/` via `include_dir!`
- [x] q2 preview boot smoke: `cargo run --bin q2 -- preview /tmp/plan7-smoke` rendered correctly; user confirmed the preview in their browser (2026-05-24 session). The full edit round-trip (drag-to-trigger-handleSetAst → observe DiagnosticStrip on atomic edit) is part of the deferred Playwright matrix above.
- [ ] **Deferred to the user** — hub-client manual smoke (edit sectionized doc, observe section structure in saved qmd) and SPA manual smoke with echo-prevention assertion. The user is doing these by hand; the e2e equivalents land via Plan 7b Phases 2 + 3.
- [x] Plan 7 marked complete (Phases 1-7 + 9 done; Phase 8 partially landed, remainder tracked separately).
- [x] Bump `hub-client/changelog.md` with a one-line entry per the two-commit workflow (commit `b5d6d08a`).
- [x] Plan 9's `preimage_in` role-asymmetry e2e test reference is in Plan 9 Phase 5 (added a "Plan 7 shipped 2026-05-24" status note so the deferral state is unambiguous when Plan 9 lands).

## Notes

This is the most intricate plan in the set. It's the keystone for
M3 — once this lands, q2-preview is truly editable for the common
case in BOTH hub-client and the q2 preview SPA. Take care with
test coverage; round-trip bugs in the writer can corrupt source
silently if not caught.

### Soft-drop replaces hard-abort

Plan 7 substitutes safe alignments in coarsen and emits warnings
rather than aborting the entire write. The user-facing contract
"this edit must be prohibited" is honored (the bad edit doesn't
reach source); the user-facing failure mode "the entire save was
rejected" is not. React (Plan 2A's framework atomic gate) is the
primary safeguard via read-only enforcement; the writer is the
contract guarantor; if React has a hole the writer protects
without losing the user's session.

The let-user-win exception for block-level UseAfter on atomic
**CustomNode** (user-replaced or -deleted via React's component
menu) is a deliberate asymmetry: when the user explicitly destroys
an atomic CustomNode through an explicit affordance, we trust
them. The qmd writer's CustomNode arms know how to write fresh
atomic types from `plain_data`. The corresponding case for
no-preimage Generated containers stays soft-drop — there's no
source position to anchor a Rewrite at.

### Filter mutations are not flagged as atomic — accepted corner

Plan 4 distinguishes filter constructions (`pandoc.Str("decoration")`
→ `Generated { by: filter, from: [] }`, atomic) from filter
mutations (`Str.text = upper(Str.text)` → keeps Original source_info,
NOT atomic).

A user editing a filter-mutated Str through React produces an
unusual round-trip: the user types "world" over the filter-output
"HELLO"; the writer Rewrites "world" to source; the next pipeline
run filters "world" → "WORLD". For idempotent filters (uppercase)
this is fine — the typed text round-trips through filter to itself.
For non-idempotent filters (`x => upper(x) + "!"`) the typed text
gets a `!` appended on every save, which is confusing.

We accept this corner rather than flagging filter mutations as
atomic because:
- (a) it would require revising Plan 4 to track filter mutations
  distinctly from plain Original source_info (a notable type-system
  change);
- (b) Plan 7a's runtime user-filter idempotence detection catches
  the AST-level non-idempotence that would actually corrupt
  round-trip;
- (c) Plan 3's idempotence test enforces the contract for built-in
  filters at CI time.

Users who write non-idempotent filters get a warning at runtime
and can decide whether the trade-off is acceptable.

### The byte-provenance contract

The contract isn't "no materialization" — that phrasing is too
blunt. **The writer materializes constantly** in the neutral
sense: every Rewrite path materializes new bytes through the qmd
writer; even Verbatim copies are a kind of materialization. The
contract is more precise: the writer only emits bytes whose origin
can be honestly traced to either **existing source bytes in the
target file** (Verbatim copies, slot preimages via `preimage_in`)
or **fresh AST the user constructed** (Rewrite paths fed by
user-supplied AST nodes via the qmd writer's normal arms).

What soft-drop forbids — by structural construction — is the case
where the writer would emit bytes synthesized from a wrapper's
slot children as flat content in the parent file. Plan 8's
qmd-writer arm for `IncludeExpansion` in a non-Verbatim path
would (under an old defensive-fallback design) walk the wrapper's
content slot and emit those blocks as flat parent-file bytes —
but those blocks come from `foo.qmd`, not from `parent.qmd` source
nor from user input. Writing them into `parent.qmd` would put bytes
there whose provenance is the included file — dishonest at the
parent-file boundary.

Under soft-drop, coarsen substitutes KeepBefore (Verbatim of the
wrapper's parent-file include-token bytes) before the qmd writer
ever sees that case. The arm becomes `unreachable!()` — a debug
assertion for coarsen bugs, not a user-facing failure mode.
Promoting the qmd writer to a fallible `Result` interface to make
the unreachable case recoverable would be over-engineering, since
correct coarsen makes the case structurally absent.

The let-user-win Rewrite path for atomic CustomNodes is
provenance-honest: when the user constructs a fresh
`IncludeExpansion` through React (with `plain_data = { source_path:
"bar.qmd" }`) and the writer materializes `{{< include bar.qmd >}}`
into source, the bytes' origin is the user's edit. Plan 8's
qmd-writer arm reads `plain_data`, doesn't read `source_info`,
and emits the include syntax — same arm whether the wrapper came
from `IncludeExpansionStage` (pipeline) or from React (user). That
symmetry is what makes the let-user-win case clean.

The corresponding case for no-preimage Generated containers
soft-drops instead of let-user-win because those containers have
no parent-file source position — Rewrite would have nowhere to
write. The user's edit is rejected with Q-3-43; the original
content regenerates from baseline metadata on the next pipeline
run.

### Decomposition of operations

Plan 7's surface change — `incremental_write_qmd` takes a baseline
AST instead of parsing internally — is a small step in a larger
decomposition. The four primitives (parse / transform / reconcile /
write) are already implemented as separate Rust functions. Plan 7
makes the WASM boundary reflect that decomposition: the writer's
WASM entry doesn't conflate the parse step with the write step
anymore. The caller composes parse + transform separately (or
re-uses an already-rendered AST from a prior call), then hands two
ASTs and the source bytes to the writer.

This decomposition makes future pipeline kinds free: the writer
doesn't need a new parameter for each new kind, because it doesn't
know what a pipeline is. The caller picks which render function to
call; the writer just diffs.

## Follow-ups closed

- **`CoarsenedEntry::Rewrite` carried `new_idx` instead of
  pre-computed text** (Phase 2 design vestige).
  Closed 2026-05-25 by
  [`coarsened-entry-self-contained`](./2026-05-25-coarsened-entry-self-contained.md).
  The `result_idx is unused for child Rewrites (...not exercised by
  today's synthesizers)` comment introduced in commit `9a473fe9` was
  accurate at the time, but became reachable once Plan 7c Phase 8
  (`bdcfdc53`) added a Transparent-recursion path in `coarsen_blocks`
  for changed wrappers. The fix lifts `Rewrite` to carry
  `block_text: String` (matching `InlineSplice`'s precedent), making
  every `CoarsenedEntry` variant self-contained. The contract is
  documented in
  [`incremental-writer-internals.md`](../designs/incremental-writer-internals.md).
