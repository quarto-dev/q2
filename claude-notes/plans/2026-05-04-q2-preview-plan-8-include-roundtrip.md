# Plan 8 — Include round-trip (TOMBSTONE — abandoned)

**Status:** **Abandoned 2026-06-05.** No work to do; include round-trip is free
under the node-edit architecture. This file is kept as a tombstone because
~30 references to "Plan 8" remain across the q2-preview plan family
(Plans 1, 2a, 2b, 2c, 4, 6, 7g) and `research/2026-05-05-editable-custom-nodes.md`.
Rather than sweep them all, this note explains what happened.

## What Plan 8 was

A design to make `{{< include child.qmd >}}` round-trip through editing: wrap
each include's expanded blocks in a `CustomNode("IncludeExpansion")` carrying
the parent file's `Original` source_info, register it as atomic, and rely on
the **Plan-7 write-back model** (soft-drop / `Q-3-43` / atomic-violation) to
preserve the include token verbatim and forbid edits inside includes. It also
specified an `IncludeExpansion` React component, an `atomicCustomNodes.ts`
amendment, and an `IncludeExpansionResolveTransform` to unwrap for HTML.

## Why it is abandoned

Two independent reasons, both rooted in the current architecture:

1. **The write-back model it built on was reverted.** Plan 7's soft-drop /
   atomic-violation machinery is gone; the working model is the AST-splice
   approach in [`2026-06-04-target-incremental-writes.md`](2026-06-04-target-incremental-writes.md).

2. **Under that model includes need no include-specific machinery at all.**
   `apply_node_edit` reconciles against the **untransformed** AST
   (`qmd_to_pandoc(content)`), which holds the raw, *unexpanded*
   `{{< include child.qmd >}}` token — `IncludeExpansionStage` runs later in
   the pipeline and never touches that tree. So:
   - **Editing outside an include** leaves the include token as a `KeepBefore`
     block, copied verbatim. No wrapper, no soft-drop.
   - **Editing inside an include** is impossible from the parent: that node's
     `source_info` is rooted in the included file (a different `FileId`), so it
     does not resolve in the parent's AST and `apply_node_edit` returns
     `DestinationNotFound`. Included content is read-only from the parent — the
     correct behavior, for free.

   Both halves are pinned by tests in
   `crates/pampa/tests/integration/node_edit_tests.rs`
   (`apply_node_edit_preserves_include_token_on_outside_edit`,
   `apply_node_edit_rejects_edit_inside_include`).

## Consequences for the references that mention "Plan 8"

- **No `IncludeExpansion` CustomNode wrapper** is produced. `IncludeExpansionStage`
  continues to splice flat blocks (`include_expansion.rs`), and `By::include()`
  stays an unused constructor. HTML rendering already handles flat-spliced
  includes; the `IncludeExpansionResolveTransform` was only needed to unwrap a
  wrapper that no longer exists.
- **No `IncludeExpansion` React component / `atomicCustomNodes.ts` entry** is
  needed. Plans 2a/2b/2c reference these as "shipped by Plan 8"; since no
  `IncludeExpansion` AST node is ever produced, `Fallback.tsx` would never see
  the type and the atomic-registry entry is moot. Those references describe
  contingencies that will not arise.

## If include editing ever needs more

The one thing the new model does *not* give for free is a UI cue: included
content currently renders editable-looking (it has a pool id), and the backend
silently rejects the edit. A small, frontend-only enhancement — treat a node
whose `source_info` file id ≠ the main document as read-only — would close that
gap. That is a q2-preview rendering tweak, not an include-pipeline plan, and
belongs wherever the editability gate lives if/when it is wanted.
