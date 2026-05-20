# Plan 8 — Include round-trip via IncludeExpansion CustomNode

**Date:** 2026-05-04 (revised 2026-05-20)
**Branch:** feature/q2-preview
**Status:** Implementation plan (open questions named)
**Milestone:** M4 (documents with `{{< include >}}` are no longer read-only;
  edits outside includes round-trip cleanly; edits inside are prohibited)

## Epic context

Part of the **provenance epic** (Plans 3–8). Plan 8 is the last plan in
the epic: it lights up include round-trip via a `CustomNode` wrapper
that consumes the atomic-detection infrastructure Plan 7 ships. The
file name keeps its q2-preview-plan-N form for continuity with the
earlier discussion notes.

## Goal

Modify `IncludeExpansionStage` to wrap each include's expanded blocks in
a `CustomNode("IncludeExpansion")` whose `source_info` is the original
`{{< include foo.qmd >}}` Paragraph's `source_info` (`Original` pointing
at the parent file's include-token bytes). This gives the incremental
writer an anchor for the include token's source bytes — round-trip
preserves `{{< include foo.qmd >}}` verbatim when the user doesn't touch
it.

This plan also adds the qmd-writer arm for `CustomNode("IncludeExpansion")`
and the React component (transparent passthrough that doesn't propagate
`setLocalAst` to slot children — shipped here, not in Plan 2C; see Plan
2C's 2026-05-10 third-pass amendment for the deferral rationale).
The writer's atomic-violation logic from Plan 7 enforces the "edits
inside an include are prohibited" contract — `IncludeExpansion` is
registered in `is_atomic_custom_node`.

When this plan lands, M4 is reached: documents with includes are
fully-functional in q2-preview's read+edit mode (with edits outside
includes round-tripping; edits inside surfacing as Q-3-43 diagnostics).

## Scope

### In scope

- Modify `IncludeExpansionStage`
  (`crates/quarto-core/src/stage/stages/include_expansion.rs`) to wrap
  inserted blocks in a `Block::Custom(CustomNode { type_name:
  "IncludeExpansion", … })` instead of splicing them flat. The wrapper's
  `source_info` is the original Paragraph's `source_info` (the include
  shortcode token's range in the parent file). `plain_data` carries
  `{ "source_path": "<literal arg>", "atomic": true }`.
- **The wrapper's `source_info` stays `Original`, NOT `Generated`** —
  see "Why the wrapper is Original" below.
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
    user-constructed wrappers (`Generated { by: user_edit, anchors: [] }`
    source_info from React). This is the path that fires when the user
    replaces or adds an include via a React UI.
  - **Unreachable path** (RecurseIntoContainer on atomic with inner
    changes): under Plan 7's soft-drop semantics, coarsen substitutes
    KeepBefore for this case before the qmd writer ever sees it. The
    arm includes `unreachable!("coarsen should have substituted KeepBefore
    for atomic CustomNode in RecurseIntoContainer; this branch indicates
    a coarsen bug")` as a debug assertion.
- Add an `IncludeExpansionResolveTransform` to the **Normalization
  Phase** (symmetric with `CalloutResolveTransform`), running in the
  HTML pipeline only (not q2-preview). Unwraps
  `CustomNode("IncludeExpansion")` back into flat blocks for the HTML
  writer to handle generically. See "HTML pipeline resolve transform"
  below.
- Add a React component for `IncludeExpansion` at
  `ts-packages/preview-renderer/src/q2-preview/custom/IncludeExpansion.tsx`
  (q2-preview's built-in custom-node registry, post-2pre / 2B / 2C).
  Plan 2C deferred the placeholder per its third-pass amendment
  (2026-05-10): until Plan 8 lands the AST node, `Fallback.tsx` covers
  the unknown `type_name` gracefully, and Plan 8 ships the real
  component together with the AST node and the `atomicCustomNodes.ts`
  addition:
  - Transparent passthrough: render the content slot's blocks normally.
  - Read-only: do not pass `setLocalAst` to slot children (enforced via
    the framework's atomic-aware dispatcher in `framework/dispatch.tsx`
    reading
    `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts`).
  - Visual indicator (optional): subtle background tint or hover badge
    "from foo.qmd".
- Register `"IncludeExpansion"` in **both** sides of the atomic registry:
  Rust `ATOMIC_CUSTOM_NODES` const (Plan 7 introduces the const +
  `is_atomic_custom_node()` function) and TypeScript hand-mirror
  `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts` (Plan
  2A introduces the file with the initial `["CrossrefResolvedRef"]`
  set). Plan 8 amends both to add `"IncludeExpansion"`.
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
- Attributing the include line in HTML rendering. The
  `IncludeExpansionResolveTransform` unwraps the wrapper before
  `AttributionRenderTransform` runs, so HTML output has no DOM anchor
  for the include-line author. See "HTML attribution" below — this is
  intentional v1 behavior.

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
- **Source_info on the wrapper points to the original Paragraph**, not
  to the inner Shortcode. The Paragraph's range covers the whole
  `{{< include >}}` line (including any whitespace/newline padding);
  the Shortcode's covers just the token. Paragraph gives a cleaner
  verbatim copy.
- **Nested includes produce nested wrappers naturally**. When the
  include expansion processes a child file that itself has includes,
  recursion produces nested CustomNode wrappers. Each wrapper anchors
  at its own parent-file include token. Round-trip semantics compose:
  untouched at any level → preserved; touched at any level →
  atomic-violation at the deepest affected wrapper.
- **React component is read-only** (Plan 8 ships the per-type
  IncludeExpansion component, deferred from Plan 2C per its third-pass
  amendment; Plan 2A's framework atomic gate enforces read-only
  behavior). The IncludeExpansion component does not pass
  `setLocalAst` to children. This is the primary enforcement; the
  writer's atomic-violation is the contract guarantor.
- **Render-side resolve, not writer arm.** The HTML writer stays
  generic — it doesn't grow knowledge of `IncludeExpansion`. The
  `IncludeExpansionResolveTransform` unwraps in the Normalization Phase
  (symmetric with `CalloutResolveTransform`), and the unwrapped blocks
  flow through the rest of the HTML pipeline normally. This preserves
  the Pandoc / Quarto convention of "resolve to standard AST before
  writers" — see "Considered alternatives" below.

## Why the wrapper is Original

The wrapper's `source_info` is `Original`, inherited from the original
Paragraph it substitutes for. This may look inconsistent with Plan 6's
audit (which puts other transform-synthesized wrappers like Sectionize
into `Generated`), but it follows a principled rule:

**Two pieces of provenance information** need to land somewhere when a
transform synthesizes a node:

1. **Generator identity** — "which transform produced me."
2. **Source anchor** — "which source bytes are this node's canonical preimage."

For non-CustomNode synthesized nodes (Sectionize's Section Div,
filter-constructed Str, footnotes container Div), there's no other slot
for (1), so `source_info` carries both via `Generated { by, anchors }`.

For CustomNode synthesized nodes, (1) is **already encoded** in
`CustomNode.type_name`. The wrapper *is* an `IncludeExpansion` by
virtue of `type_name`; there's no need for `source_info` to also say
"I was made by IncludeExpansionStage." So `source_info` only has to do
(2) — and the natural shape for (2) when the wrapper substitutes 1:1
for a source-mapped parser node is the inherited `Original`.

This isn't a Plan 8 invention — it's the existing pattern for every
source-mapped CustomNode in the codebase:

| CustomNode `type_name` | Source-mapped from | `source_info` shape |
|---|---|---|
| `Callout` | `:::{.callout-warning} … :::` Div | Original (inherited) |
| `Theorem` / `Proof` / etc. | `:::{.theorem #thm-foo} … :::` Div | Original (inherited) |
| `CrossrefResolvedRef` | `@thm-foo` Cite | Original (inherited) |
| `FloatRefTarget` | Figure / table / listing Div | Original (inherited) |
| `IncludeExpansion` (Plan 8) | `{{< include foo.qmd >}}` Paragraph | Original (inherited) |

In contrast, Sectionize's Section Div is NOT a CustomNode (it's a
plain Div) AND it doesn't 1:1-substitute for a source-mapped parser
node (it's a structural grouping over a Header + its body). So its
`source_info` has to carry generator identity via `Generated { by:
sectionize, anchors: [] }`.

**The rule, in one sentence**: a synthesized node uses **Original**
`source_info` if and only if it is a CustomNode whose 1:1 source
preimage is a parser-emitted node. Everything else uses **Generated**.

See Plan 4's "Original vs Generated on synthesized nodes" section for
the full taxonomy.

## HTML pipeline resolve transform

The wrapper change applies to the `IncludeExpansionStage`, which runs
in BOTH the HTML and q2-preview pipelines. For HTML output, the
wrapper would otherwise reach the HTML writer with no native rendering
arm for `IncludeExpansion`. The cleanest fix:

Add `IncludeExpansionResolveTransform` that runs ONLY in the HTML
pipeline (not q2-preview, where the React `IncludeExpansion` component
handles rendering directly). Unwraps `CustomNode("IncludeExpansion")`
back into flat blocks — the slot's `content` Blocks become siblings of
the surrounding content. The HTML writer then processes the flat
blocks generically.

**Placement**: Normalization Phase, symmetric with
`CalloutResolveTransform` (`crates/quarto-core/src/pipeline.rs:988`).
Like Callout, the resolve fires early so the rest of the pipeline sees
standard AST. `Q2_PREVIEW_TRANSFORM_EXCLUDED` lists
`"callout-resolve"`; add `"include-expansion-resolve"` to that list so
q2-preview keeps the wrappers for React rendering.

## HTML attribution

When the resolve transform unwraps the wrapper, the wrapper's
`source_info` (Original pointing at the parent's include token) is
gone before `AttributionRenderTransform` runs at the tail of the
Finalization Phase. Consequences:

- The unwrapped included blocks have `file_id != 0` (foo.qmd's
  FileId). `query_attribution` skips them per the v1 single-doc
  invariant. **No attribution on included content in HTML.**
- The include-line author has no node to be attributed against. The
  Paragraph that previously held `{{< include foo.qmd >}}` was deleted
  by `IncludeExpansionStage`. **No attribution on the include line in
  HTML.**

This matches what current main produces (without Plan 8, the include
line and its content are also un-attributed in HTML output), so it's
not a regression. It's *intentional* v1 behavior: in the rendered HTML,
there's no DOM element that represents "the include line" — those
source bytes don't appear in the rendered output. Attributing them
would require synthesizing a wrapping HTML element, which is
inconsistent with the "resolve to standard AST" convention.

**q2-preview attributes the include line correctly.** q2-preview
excludes the resolve transform, so the wrapper survives all the way to
JSON serialization and React. `AttributionRenderTransform` visits the
wrapper, resolves its `Original` source_info to a byte range, and
records the include-line author via the existing `query_byte_range`
max-time logic. The React `IncludeExpansion` component receives the
attribution record and surfaces it as the authorship pill on the
wrapper region.

When v2 multi-file blame lands (`crates/quarto-core/src/attribution/types.rs:58`
flags this as v1-only), the unwrapped HTML children gain attribution
from foo.qmd's blame. The HTML include-line itself remains
un-attributed because there's still no DOM anchor — that's a structural
property of HTML output, not a v1 limitation we plan to remove.

## Considered alternatives

**Option C — render `IncludeExpansion` natively in the HTML writer.**
Investigated during the 2026-05-20 design discussion. Cleaner for v2
attribution (the wrapper would survive to the HTML writer, which could
emit a `<div class="quarto-include">` with the include-line author's
`data-attr-*`). Rejected because it breaks the Pandoc / Quarto
convention of resolving CustomNodes to standard AST before writers see
them. The convention is load-bearing: it lets each new output format
(future Typst, future PDF) work generically without growing
CustomNode-specific arms.

The decision is recoverable if needed — the type definitions and
wrapper shape don't change. Switching to native rendering later means
dropping the resolve transform and adding writer arms; it doesn't
require revising Plan 8's type design.

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
    source_info: paragraph.source_info.clone(),  // Original{parent, include_token_range}
})
```

The included blocks inside the slot keep their own FileId (set by the
existing remap_file_ids logic in `IncludeExpansionStage`). They render
correctly in q2-preview's React layer because Plan 8's IncludeExpansion
component renders the slot's content using the same dispatch (until
Plan 8 ships, q2-preview's `Fallback.tsx` does the same generic slot
walk — same visual outcome, just no per-type styling).

## Round-trip walkthrough

**Case 1 — untouched include**:

- Both pipeline runs (live and baseline) produce identical wrappers.
- Reconciler picks `KeepBefore` for the wrapper.
- Plan 7's coarsen sees `is_atomic_custom_node("IncludeExpansion") == true`
  → goes the Verbatim path because `preimage_in(parent_file)` returns
  the include token's byte range (the wrapper's source_info is
  `Original{parent, start, end}`).
- `assemble` copies `original_qmd[start..end]` — the literal `{{< include
  foo.qmd >}}` text. ✓ Source preserved.

**Case 2 — edit outside include, untouched include in same doc**:

- Reconciler's plan has `KeepBefore` for the include wrapper, mixed
  alignments for other blocks.
- The include wrapper goes through the Verbatim path (case 1 above).
  Other blocks are handled per their own alignment. The include token
  in source is preserved verbatim. Edit outside is rewritten.

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
  warning surfaces in the diagnostic panel (hub-client) or
  DiagnosticStrip (SPA).

**Case 3b — user replaces or deletes the include via React**:

- Reconciler's plan has `UseAfter` for a different block (or no
  alignment for the original include — implicit deletion).
- Plan 7's coarsen sees block-level UseAfter on a non-atomic block, OR
  no alignment for the original wrapper (implicit deletion handled by
  the reconciler's plan structure). **Let-user-win** — Rewrite via
  qmd writer; the include is gone from output.
- If the user replaced the include with a fresh IncludeExpansion (e.g.,
  changed `foo.qmd` to `bar.qmd` via a hypothetical UI), the new
  wrapper has `Generated { by: user_edit, anchors: [] }` source_info
  and `plain_data["source_path"] = "bar.qmd"`. The qmd writer's arm
  reads `plain_data` and emits `{{< include bar.qmd >}}`. No warning
  — the user's intent is clear.

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
- The outer wrapper's source_info is Original pointing at parent.qmd's
  bytes. The inner wrapper's source_info is Original pointing at
  foo.qmd's bytes (via the FileId remap).
- Round-trip in parent.qmd: outer's `preimage_in(parent_file)` returns
  the parent's include token range. Verbatim copy preserves
  `{{< include foo.qmd >}}` in parent.qmd. The inner wrapper's bytes
  never get serialized because the outer's Verbatim wins.

## Open questions for implementation

- **`source_path` accuracy**: the literal arg from the shortcode
  (`"foo.qmd"`) is what we re-emit on save. Plan 7's Verbatim copy path
  doesn't use it (we copy bytes), but the Rewrite path
  (let-user-win for fresh user-constructed IncludeExpansion) does.
  Make sure the IncludeExpansionStage stores the literal arg verbatim
  — including any whitespace or quoting the user typed — so a
  round-trip through React preserves the user's syntactic choices when
  possible.
- **Recorded includes side-channel**: today's `IncludeExpansionStage`
  writes to `doc.recorded_includes` for cache invalidation. The wrapper
  change shouldn't affect this — confirm.
- **`extract_include_path` recognition**: today the function recognizes
  a Paragraph containing exactly one include Shortcode inline. After
  the wrapper change, the structure is unchanged at that recognition
  point (the wrapper is built from the recognized Paragraph). The
  recognition logic continues to work.

## References

- `crates/quarto-core/src/stage/stages/include_expansion.rs:80-278` —
  the stage implementation. The splicing logic at lines 215-220 is
  what gets replaced with wrapper construction.
- `crates/quarto-pandoc-types/src/custom.rs` — CustomNode struct.
- `crates/pampa/src/writers/qmd.rs` — qmd writer's CustomNode arm
  (existing for Callout etc. — extend with IncludeExpansion).
- `crates/quarto-core/src/transforms/callout_resolve.rs` — pattern to
  mirror for `IncludeExpansionResolveTransform`. Note: Callout's
  resolve runs in the Normalization Phase
  (`crates/quarto-core/src/pipeline.rs:988`), not the Finalization
  Phase. Plan 8's resolve runs at the same point in the HTML
  pipeline.
- `crates/quarto-core/src/pipeline.rs:1181` —
  `Q2_PREVIEW_TRANSFORM_EXCLUDED` const; add
  `"include-expansion-resolve"` to skip the unwrap in the q2-preview
  pipeline.
- Plan 6 — provenance audit. Sets the precedent for "preserve source
  info on transform output." Plan 6 uses `Generated` with
  `Invocation` anchors for shortcodes; Plan 8 uses the
  wrapper-CustomNode pattern for includes, since cross-file FileId
  prevents shortcode-style anchoring from working.
- Plan 7 — coarsen logic (Verbatim, Transparent, Omit, soft-drop
  substitutions, is_atomic_custom_node registry).
- Plan 2A — `ts-packages/preview-renderer/src/utils/atomicCustomNodes.ts`
  (the JS-side atomic registry that Plan 8 amends to add
  `"IncludeExpansion"`).
- Plan 2B — framework recursion + atomic gate that the
  IncludeExpansion component runs through; CustomNode unwrap/rewrap
  walks that produce the JS-native shape Plan 2C's component
  consumes.
- Plan 2C — React component infrastructure (registers IncludeExpansion
  component as a transparent read-only passthrough; Plan 2C already
  ships the placeholder component as dormant wiring before Plan 8
  produces these CustomNodes).

## Test plan

- **Untouched-include round-trip**: parse a parent.qmd with an include,
  run pipeline, write back without modification, assert byte-equal.
- **Edit-outside round-trip**: edit a paragraph outside the include in
  the AST, write back, assert the include token is byte-equal-preserved
  and the edited paragraph is rewritten.
- **Edit-inside soft-drop**: programmatically modify a Str inside the
  IncludeExpansion's content slot (bypass the React layer), call
  `incremental_write_qmd_for_preview`, assert the result is `Ok` with
  the include token byte-equal-preserved AND a `Q-3-43` warning in the
  warnings vec referencing the include's `source_path`.
- **Replace-include let-user-win**: programmatically replace an
  IncludeExpansion with a fresh user-constructed IncludeExpansion (new
  source_path), call the writer, assert the output contains
  `{{< include <new_source_path> >}}` with no warning. The qmd writer's
  CustomNode arm hit the Rewrite path with `Generated { by: user_edit,
  anchors: [] }` source_info and read `plain_data["source_path"]`.
- **Delete-include let-user-win**: replace an IncludeExpansion with a
  Para in the new AST, call the writer, assert the include token is
  gone from output and the Para's text appears in its place. No
  warning.
- **Nested includes round-trip**: parent → foo → bar. Untouched: all
  three preserved. Edit inside bar: `Q-3-43` warning with bar's
  wrapper source range; the inner edit is reverted via Plan 7's
  soft-drop, parent.qmd byte-equal to no-op edit.
- **HTML pipeline resolve test**: render a doc with an include through
  the HTML pipeline; assert the resulting HTML contains the included
  content flat (not wrapped in a `<div data-custom-type="IncludeExpansion">`
  or similar) — the resolve transform unwrapped it before the HTML
  writer ran.
- **q2-preview pipeline preservation test**: render the same doc
  through the q2-preview pipeline; assert the resulting AST contains
  the IncludeExpansion CustomNode wrapper (not unwrapped). The JSON
  writer emits it; the React component renders it.
- **q2-preview attribution test**: with a `PreBuiltAttributionProvider`
  installed, render a doc with an include through q2-preview; assert
  the wrapper's `astContext.attribution` record references the
  include-line author (the latest author of the parent's include line
  bytes). HTML output of the same doc has no attribution on the
  include line (intentional v1 behavior).
- **Plan 2C component test**: render an IncludeExpansion wrapper;
  assert setLocalAst is not propagated to children (no edit
  affordance).
- **Idempotence**: re-run Plan 3's idempotence test with includes. The
  wrapper should be deterministic across runs.

## Dependencies

- Depends on: Plans 4, 6, 7 (Generated types not strictly needed for
  the wrapper's source_info since it stays Original; the audit pattern
  for what kinds of nodes get Generated vs Original; the writer's
  atomic logic).
- Plan 2C also depends on this for the IncludeExpansion component
  (which Plan 8 confirms is needed; Plan 2C ships the placeholder
  dormant).
- Final plan in the sequence; nothing depends on it.

## Risk areas

- **The include's wrapper source_info uses the *parent* file's
  FileId**. The included blocks inside the slot have a *different*
  FileId. Plan 7's `preimage_in(parent_file)` correctly returns None
  for the children (because their FileId differs). This is the intended
  behavior — children contribute nothing to the verbatim-copy path;
  only the wrapper does. Confirm by walking through the test cases.
- **Existing tests for `IncludeExpansionStage`**: the existing tests
  assert the spliced-flat behavior (e.g.,
  `assert_eq!(doc.ast.blocks.len(), 2)` after expanding one include).
  Update these tests for the wrapper behavior
  (`assert_eq!(doc.ast.blocks.len(), 1)` — the Paragraph is replaced
  by one wrapper).
- **The `recorded_includes` side-channel**: existing pipeline-cache
  logic reads this. The wrapper change shouldn't affect it because we
  still call `record_include` at the same point. Confirm during
  implementation.
- **Existing HTML pipeline tests**: the wrapper change applies to the
  HTML pipeline too (we're modifying `IncludeExpansionStage`, which
  runs in both HTML and q2-preview pipelines). The
  `IncludeExpansionResolveTransform` in the Normalization Phase
  unwraps before the HTML writer sees it, so HTML output is
  byte-equivalent to current main. Verify with a regression test.
- **Extension-registration forward-compat**: Plan 8 adds
  `IncludeExpansion` to the hardcoded `pub const ATOMIC_CUSTOM_NODES`
  set in `quarto-core`. After the future extension-registration
  follow-up plan (see Plan 7 §Open questions
  "is_atomic_custom_node lookup — extension forward-compat"),
  `IncludeExpansion` will be declared in a built-in's `_extension.yml`
  via `contributes.custom-nodes: [{type: IncludeExpansion, atomic: true}]`
  rather than hardcoded in `quarto-core`. The const-based registry
  Plan 8 ships is forward-compatible — the migration is a data-source
  change, not a code change.

## Estimated scope

| Component | Lines (rough) |
|---|---|
| IncludeExpansionStage modification (wrap inserted blocks) | ~40 |
| qmd writer arm for IncludeExpansion (atomic) | ~30 |
| `IncludeExpansionResolveTransform` (Normalization Phase, HTML only) | ~50 |
| Adding `"include-expansion-resolve"` to `Q2_PREVIEW_TRANSFORM_EXCLUDED` | ~5 |
| `is_atomic_custom_node` registration (Rust + TS hand-mirror) | ~10 |
| React component (transparent passthrough, read-only) | ~30 |
| Test updates for existing IncludeExpansionStage tests | ~50 |
| New round-trip tests (untouched, edit-outside, soft-drop, replace, nested, HTML, attribution) | ~250 |
| **Total** | **~465** |

Two focused sessions likely.

## Notes

The wrapper-CustomNode pattern is the right shape for includes because
the included content lives in a *different file* than the parent.
Their source_info points into foo.qmd, not parent.qmd. There's no
`Generated`-with-`Invocation`-anchor chain that can connect those
blocks back to the parent file's include token bytes (the anchor's
chain would need to resolve into the target file, and foo.qmd is a
different FileId). So we need a wrapper at the parent-file level whose
`source_info` is `Original{parent_file, include_token_range}` to serve
as the writer's anchor. That's what `CustomNode("IncludeExpansion")`
provides.

Shortcodes (Plan 6) don't have this issue (they resolve in the same
file) which is why they use `Generated { by: shortcode, anchors: [Invocation -> ...] }`
instead of a wrapper. The genuine cross-file case is the only one that
warrants the wrapper.

The HTML-pipeline-resolve-transform finding is the kind of thing the
design discussion exists to surface. The wrapper change has
implications for HTML output that aren't immediately visible from the
q2-preview-only lens. Plan 8's implementation kickoff should land the
resolve transform alongside the wrapper change to keep HTML
byte-equivalent across the transition.
