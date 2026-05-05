# Plan 8 — Include round-trip via IncludeExpansion CustomNode

**Date:** 2026-05-04
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M4 (documents with `{{< include >}}` are no longer read-only;
  edits outside includes round-trip cleanly; edits inside are prohibited)

## Goal

Modify `IncludeExpansionStage` to wrap each include's expanded blocks in a
`CustomNode("IncludeExpansion")` whose `source_info` points to the include
shortcode token in the parent file. This gives the incremental writer an
anchor for the include token's source bytes — round-trip preserves
`{{< include foo.qmd >}}` verbatim when the user doesn't touch it.

This plan also adds the qmd-writer arm for `CustomNode("IncludeExpansion")`
and the React component (transparent passthrough that doesn't propagate
`setLocalAst` to slot children, registered with Plan 2). The writer's
atomic-violation logic from Plan 7 enforces the "edits inside an include
are prohibited" contract — `IncludeExpansion` is registered in
`is_atomic_custom_node`.

When this plan lands, M4 is reached: documents with includes are
fully-functional in q2-preview's read+edit mode (with edits outside includes
round-tripping; edits inside surfacing as diagnostics).

## Scope

### In scope

- Modify `IncludeExpansionStage` (`crates/quarto-core/src/stage/stages/include_expansion.rs`)
  to wrap inserted blocks in a `Block::Custom(CustomNode { type_name:
  "IncludeExpansion", … })` instead of splicing them flat. The wrapper's
  `source_info` is the original Paragraph's source_info (the include
  shortcode token's range in the parent file). `plain_data` carries
  `{ "source_path": "<literal arg>", "atomic": true }`.
- Update the qmd writer (`pampa/src/writers/qmd.rs` CustomNode arm) to
  handle `"IncludeExpansion"`. Two paths:
  - **Verbatim path** (KeepBefore in Plan 7's coarsen): nothing to do —
    coarsen produces `Verbatim` and assemble copies bytes from
    `original_qmd` directly. The CustomNode arm isn't involved.
  - **Rewrite path** (block-level UseAfter on a fresh user-constructed
    IncludeExpansion — let-user-win per Plan 7): the arm reads
    `plain_data["source_path"]` and emits `{{< include <source_path> >}}`.
    The arm reads `plain_data` only — it does NOT inspect `source_info`,
    so it works identically for pipeline-emitted wrappers (Original
    source_info pointing at the parent file's include token) and
    user-constructed wrappers (Synthetic { by: user_edit } source_info
    from React). This is the path that fires when the user replaces or
    adds an include via a React UI.
  - **Unreachable path** (RecurseIntoContainer on atomic with inner
    changes): under Plan 7's soft-drop semantics, coarsen substitutes
    KeepBefore for this case before the qmd writer ever sees it. The
    arm includes `unreachable!("coarsen should have substituted KeepBefore
    for atomic CustomNode in RecurseIntoContainer; this branch indicates
    a coarsen bug")` as a debug assertion.
- Add a React component for `IncludeExpansion` in custom.tsx (Plan 2's
  registry):
  - Transparent passthrough: render the content slot's blocks normally.
  - Read-only: do not pass `setLocalAst` to slot children.
  - Visual indicator (optional): subtle background tint or hover badge
    "from foo.qmd".
- Register `"IncludeExpansion"` in `is_atomic_custom_node` (the function
  Plan 7 introduces).
- Tests covering:
  - Untouched include: round-trip preserves `{{< include foo.qmd >}}`.
  - Edit outside include: that paragraph rewrites; include token preserved.
  - Edit inside include (forced via test): soft-dropped per Plan 7 —
    `Q-3-43` warning; include token preserved verbatim; save succeeds.
  - Nested includes (foo includes bar): nested CustomNode wrappers, both
    preserved on untouched; soft-dropped with `Q-3-43` on touched-inside.
  - Cross-file FileId handling: the included blocks have their own FileId
    (set by the existing remap logic); `preimage_in(parent_file)` returns
    None for them; Plan 7's Transparent recurse handles them.

### Out of scope

- Editing the contents inside an include (soft-dropped per Plan 7 —
  the user must open foo.qmd directly to edit included content).
- Emitting bytes synthesized from the wrapper's slot children as flat
  parent-file content (provenance-dishonest; see Plan 7
  §"The byte-provenance contract"). The qmd-writer arm's non-Verbatim
  branch for atomic CustomNodes becomes `unreachable!()` because
  Plan 7's coarsen substitutes KeepBefore upstream.
- Resolving include shortcodes outside the standard
  `Paragraph[Shortcode("include")]` form (current behavior preserved —
  only top-level paragraph-form includes are handled).

## Design decisions (settled in conversation)

- **Wrapper-CustomNode approach**, not a side-table anchor mechanism.
  Reuses CustomNode infrastructure (JSON wrapper encoding, qmd writer's
  CustomNode arm, reconciler's CustomNode handling). Smaller and cleaner
  than a side-table.
- **Atomic for round-trip**: includes are atomic. Edits inside are
  soft-dropped per Plan 7 — the include token is preserved verbatim and
  a `Q-3-43` warning surfaces. The user must open `foo.qmd` directly
  to edit included content. The byte-provenance reasoning is in Plan 7
  §"The byte-provenance contract": the writer never emits bytes
  synthesized from the wrapper's slot children into the parent file,
  because those bytes' origin is the included file, not parent.qmd
  source nor user input.
- **Source_info on the wrapper points to the original Paragraph**, not to
  the inner Shortcode. The Paragraph's range covers the whole `{{< include >}}`
  line (including any whitespace/newline padding); the Shortcode's covers
  just the token. Paragraph gives a cleaner verbatim copy.
- **Nested includes produce nested wrappers naturally**. When the include
  expansion processes a child file that itself has includes, recursion
  produces nested CustomNode wrappers. Each wrapper anchors at its own
  parent-file include token. Round-trip semantics compose: untouched at any
  level → preserved; touched at any level → atomic-violation at the deepest
  affected wrapper.
- **React component is read-only** (Plan 2's responsibility). The IncludeExpansion
  component does not pass `setLocalAst` to children. This is the primary
  enforcement; the writer's atomic-violation is the contract guarantor.

## The wrapper structure

```rust
Block::Custom(CustomNode {
    type_name: "IncludeExpansion".to_string(),
    slots: hashlink::LinkedHashMap::from([(
        "content".to_string(),
        Slot::Blocks(included_blocks),  // FileId of foo.qmd
    )]),
    plain_data: serde_json::json!({
        "source_path": include_path_arg,  // the literal arg, e.g. "foo.qmd"
        "atomic": true,
    }),
    attr: ("".to_string(), vec![], LinkedHashMap::new()),
    source_info: paragraph.source_info.clone(),  // include token's parent-file bytes
})
```

The included blocks inside the slot keep their own FileId (set by the
existing remap_file_ids logic in `IncludeExpansionStage`). They render
correctly in q2-preview's React layer because Plan 2's IncludeExpansion
component renders the slot's content using the same dispatch.

## Round-trip walkthrough

**Case 1 — untouched include**:

- Both pipeline runs (live and baseline) produce identical wrappers.
- Reconciler picks `KeepBefore` for the wrapper.
- Plan 7's coarsen sees `is_atomic_custom_node("IncludeExpansion") == true`
  → goes the Verbatim path because `preimage_in(parent_file)` returns the
  include token's byte range (the wrapper's source_info is `Original{parent,
  start, end}`).
- `assemble` copies `original_qmd[start..end]` — the literal `{{< include
  foo.qmd >}}` text. ✓ Source preserved.

**Case 2 — edit outside include, untouched include in same doc**:

- Reconciler's plan has `KeepBefore` for the include wrapper, mixed alignments
  for other blocks.
- The include wrapper goes through the Verbatim path (case 1 above). Other
  blocks are handled per their own alignment. The include token in source
  is preserved verbatim. Edit outside is rewritten.

**Case 3 — edit inside the include (somehow)**:

- Reconciler's plan has `RecurseIntoContainer` for the wrapper because
  something in its content slot differs.
- Plan 7's coarsen sees `is_atomic_custom_node("IncludeExpansion") == true`
  AND alignment is RecurseIntoContainer → **soft-drop substitution**:
  coarsen produces a `Verbatim` entry for the wrapper instead of
  recursing, and pushes a `Q-3-43` warning into the warning sink.
- `assemble` copies `original_qmd[wrapper.source_range]` — preserves
  `{{< include foo.qmd >}}` verbatim. The user's other edits in the same
  doc go through normally.
- The user gets a `Q-3-43` warning with the wrapper's source range and
  the include's `source_path` from `plain_data`: "Edit inside
  `{{< include foo.qmd >}}` was not saved. To edit this content, open
  foo.qmd directly." Save **succeeded** (other edits applied);
  warning surfaces in the diagnostic panel.

**Case 3b — user replaces or deletes the include via React**:

- Reconciler's plan has `UseAfter` for a different block (or no
  alignment for the original include — implicit deletion).
- Plan 7's coarsen sees block-level UseAfter on a non-atomic block, OR
  no alignment for the original wrapper (implicit deletion handled by
  the reconciler's plan structure). **Let-user-win** — Rewrite via
  qmd writer; the include is gone from output.
- If the user replaced the include with a fresh IncludeExpansion (e.g.,
  changed `foo.qmd` to `bar.qmd` via a hypothetical UI), the new
  wrapper has Synthetic { by: user_edit } source_info and
  `plain_data["source_path"] = "bar.qmd"`. The qmd writer's arm reads
  `plain_data` and emits `{{< include bar.qmd >}}`. No warning — the
  user's intent is clear.

**Case 4 — nested includes**:

- `parent.qmd` contains `{{< include foo.qmd >}}`. `foo.qmd` contains
  `{{< include bar.qmd >}}`.
- After IncludeExpansionStage:
  ```
  parent.qmd ast.blocks = [
      ...,
      CustomNode("IncludeExpansion", source: parent's include token, content: [
          ... foo.qmd blocks ...,
          CustomNode("IncludeExpansion", source: foo's include token, content: [
              ... bar.qmd blocks ...
          ]),
          ...
      ]),
      ...
  ]
  ```
- The outer wrapper's source_info points to parent.qmd's bytes. The inner
  wrapper's source_info points to foo.qmd's bytes (via the FileId remap).
- Round-trip in parent.qmd: outer's `preimage_in(parent_file)` returns the
  parent's include token range. Verbatim copy preserves
  `{{< include foo.qmd >}}` in parent.qmd. The inner wrapper's bytes never
  get serialized because the outer's Verbatim wins.

## Open questions for implementation

- **`source_path` accuracy**: the literal arg from the shortcode (`"foo.qmd"`)
  is what we re-emit on save. Plan 7's Verbatim copy path doesn't use it
  (we copy bytes), but the Rewrite path (let-user-win for fresh
  user-constructed IncludeExpansion) does. Make sure the
  IncludeExpansionStage stores the literal arg verbatim — including
  any whitespace or quoting the user typed — so a round-trip through
  React preserves the user's syntactic choices when possible.
- **Recorded includes side-channel**: today's `IncludeExpansionStage` writes
  to `doc.recorded_includes` for cache invalidation. The wrapper change
  shouldn't affect this — confirm.
- **`extract_include_path` recognition**: today the function recognizes a
  Paragraph containing exactly one include Shortcode inline. After the
  wrapper change, the structure is unchanged at that recognition point
  (the wrapper is built from the recognized Paragraph). The recognition
  logic continues to work.

## References

- `crates/quarto-core/src/stage/stages/include_expansion.rs:80-278` — the
  stage implementation. The splicing logic at lines 215-220 is what gets
  replaced.
- `crates/quarto-pandoc-types/src/custom.rs` — CustomNode struct.
- `crates/pampa/src/writers/qmd.rs` — qmd writer's CustomNode arm
  (existing for Callout etc. — extend with IncludeExpansion).
- Plan 6 — provenance audit. Sets the precedent for "preserve source info
  on transform output." (Plan 6 uses Derived for shortcodes; Plan 8 uses
  the wrapper-CustomNode pattern for includes, since cross-file FileId
  prevents Derived from working.)
- Plan 7 — coarsen logic (Verbatim, Transparent, Omit, soft-drop
  substitutions, is_atomic_custom_node registry).
- Plan 2 — React component infrastructure (registers IncludeExpansion
  component as a transparent read-only passthrough).

## Test plan

- **Untouched-include round-trip**: parse a parent.qmd with an include,
  run pipeline, write back without modification, assert byte-equal.
- **Edit-outside round-trip**: edit a paragraph outside the include in the
  AST, write back, assert the include token is byte-equal-preserved and
  the edited paragraph is rewritten.
- **Edit-inside soft-drop**: programmatically modify a Str inside the
  IncludeExpansion's content slot (bypass the React layer), call
  `incremental_write_qmd_for_preview`, assert the result is `Ok` with
  the include token byte-equal-preserved AND a `Q-3-43` warning in the
  warnings vec referencing the include's `source_path`.
- **Replace-include let-user-win**: programmatically replace an
  IncludeExpansion with a fresh user-constructed IncludeExpansion (new
  source_path), call the writer, assert the output contains
  `{{< include <new_source_path> >}}` with no warning. The qmd writer's
  CustomNode arm hit the Rewrite path with Synthetic { by: user_edit }
  source_info and read `plain_data["source_path"]`.
- **Delete-include let-user-win**: replace an IncludeExpansion with a
  Para in the new AST, call the writer, assert the include token is
  gone from output and the Para's text appears in its place. No
  warning.
- **Nested includes round-trip**: parent → foo → bar. Untouched: all three
  preserved. Edit inside bar: `Q-3-43` warning with bar's wrapper source
  range; the inner edit is reverted via Plan 7's soft-drop, parent.qmd
  byte-equal to no-op edit.
- **Plan 2 component test**: render an IncludeExpansion wrapper; assert
  setLocalAst is not propagated to children (no edit affordance).
- **Idempotence**: re-run Plan 3's idempotence test with includes. The
  wrapper should be deterministic across runs.

## Dependencies

- Depends on: Plans 4, 6, 7 (Synthetic types not strictly needed since the
  wrapper uses Original; the audit pattern; the writer's atomic logic).
- Plan 2 also depends on this for the IncludeExpansion component (which
  Plan 8 confirms is needed).
- Final plan in the sequence; nothing depends on it.

## Risk areas

- **The include's wrapper source_info uses the *parent* file's FileId**.
  The included blocks inside the slot have a *different* FileId. Plan 7's
  `preimage_in(parent_file)` correctly returns None for the children
  (because their FileId differs). This is the intended behavior — children
  contribute nothing to the verbatim-copy path; only the wrapper does.
  Confirm by walking through the test cases.
- **Existing tests for `IncludeExpansionStage`**: the existing tests assert
  the spliced-flat behavior (e.g., `assert_eq!(doc.ast.blocks.len(), 2)`
  after expanding one include). Update these tests for the wrapper
  behavior (`assert_eq!(doc.ast.blocks.len(), 1)` — the Paragraph is
  replaced by one wrapper).
- **The `recorded_includes` side-channel**: existing pipeline-cache logic
  reads this. The wrapper change shouldn't affect it because we still call
  `record_include` at the same point. Confirm during implementation.
- **Existing HTML pipeline tests**: the wrapper change applies to the HTML
  pipeline too (we're modifying `IncludeExpansionStage`, which runs in
  both HTML and q2-preview pipelines). For HTML output, the wrapper
  passes through subsequent transforms unchanged and gets serialized to
  HTML by `RenderHtmlBodyStage`. Confirm the HTML writer's CustomNode
  arm handles `"IncludeExpansion"` (or that we don't need it because the
  HTML pipeline runs `CrossrefRenderTransform` etc. that don't touch this
  type — but ensure HTML output looks right).

  Actually: the HTML pipeline doesn't have a transform that materializes
  IncludeExpansion. So the HTML writer SEES the wrapper at render time.
  The simplest fix: make the HTML writer's CustomNode arm transparently
  render the slot content (effectively materializing into HTML, which is
  the right thing for HTML output). Or: add a render-side resolve transform
  for IncludeExpansion that runs only in the HTML pipeline.

  This is the one significant complication. Worth investigating during
  implementation. The cleanest answer is probably: a small render-side
  transform `IncludeExpansionResolveTransform` that runs ONLY in the HTML
  pipeline (not q2-preview), unwraps `CustomNode("IncludeExpansion")`
  back into flat blocks for the HTML writer to handle normally.

  Symmetric with `CalloutResolveTransform`. Same shape.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| IncludeExpansionStage modification (wrap inserted blocks) | ~40 |
| qmd writer arm for IncludeExpansion (atomic) | ~30 |
| HTML pipeline resolve transform (unwrap before HTML writer) | ~50 |
| `is_atomic_custom_node` registration | ~5 |
| React component (transparent passthrough, read-only) | ~30 |
| Test updates for existing IncludeExpansionStage tests | ~50 |
| New round-trip tests | ~200 |
| **Total** | **~405** |

Two focused sessions likely. The HTML pipeline resolve transform is the
piece I didn't fully account for in my earlier estimates — confirm scope
during implementation kickoff.

## Notes

The HTML-pipeline-resolve-transform finding is the kind of thing the
research plan exists to surface. The wrapper change has implications for
HTML output that aren't immediately visible from the q2-preview-only lens.
Plan 8's research plan should make this explicit so that the
implementation session doesn't get blindsided.

Why a wrapper for includes (different from shortcodes): includes pull in
content from a *different file*. The included blocks have a different
FileId than the parent file. Their source_info points into foo.qmd, not
parent.qmd. There's no `Derived` chain that can connect those blocks
back to the parent file's include token bytes — Derived requires a `from`
that resolves into the target file. So we need a wrapper at the parent-file
level whose source_info is `Original{parent_file, include_token_range}` to
serve as the writer's anchor. That's what `CustomNode("IncludeExpansion")`
provides. Shortcodes don't have this issue (they resolve in the same file)
which is why they use Derived (Plan 6) instead of a wrapper.
