# Transparent wrappers — descending past synthesized block containers

**Status:** Active (introduced 2026-05-25 alongside Plan 7c Phase 8).
**Types:** `pampa::pandoc::Block`, `quarto_source_map::SourceInfo`.
**Reference impl:**
[`crates/pampa/src/writers/incremental.rs`](../../crates/pampa/src/writers/incremental.rs)
(`first_in_user_tree`, `is_transparent_wrapper`,
`derive_target_file_id`, `first_target_anchored_start_in`).
**Plans:**
[Plan 7](../plans/2026-05-04-q2-preview-plan-7-incremental-writer.md)
(writer) ·
[Plan 7c](../plans/2026-05-25-q2-preview-plan-7c-closure-gaps.md)
(Phase 8 — target_file_id descent) ·
[Plan 8](../plans/2026-05-04-q2-preview-plan-8-include-roundtrip.md)
(IncludeExpansion — *not* a transparent wrapper) ·
[Plan 9](../plans/2026-05-22-provenance-plan-9-valuesource-threading.md)
(`title_source_info`) ·
[Plan 10](../plans/2026-05-22-provenance-plan-10-dispatch-anchor.md)
(Lua-emitted wrappers).

## Summary

The post-render AST that q2-preview hands the React iframe is **not
flat.** The render pipeline wraps the user's blocks in synthesized
containers — most notably a single top-level `Div` from
`SectionizeTransform` — that group content by heading level for
sidebar / cross-reference / outline construction. These wrappers
carry `SourceInfo::Generated` with no `Invocation` anchor: they're
structurally part of the AST but have **no source bytes of their own**
in the user's qmd.

A *transparent wrapper* is the name for this shape. Code that asks
"where do the user's source bytes live?" must descend through
transparent wrappers, not read `blocks[0]` directly.

Three writer bugs landed on this rake before the pattern was named
(commits `bdcfdc53`, `b9f64b56`, `2bf92664`): the writer
soft-dropped the wrapper instead of recursing, derived the wrong
file id, and silently deleted the YAML frontmatter. All three were
the same mistake — `blocks[0]` is not necessarily a real user
block.

## Definition

A `Block` is a *transparent wrapper* with respect to a
`target_file_id` when **all three** hold:

1. Its `SourceInfo` is `Generated` with no `Invocation` anchor.
   It has no source token of its own; its bytes are synthesized.
2. It is recognised by `block_block_children` (i.e. it's a `Div`,
   `BlockQuote`, `Figure`, or `NoteDefinitionFencedBlock` — the
   block-container kinds today's synthesizers emit).
3. At least one descendant has real
   `preimage_in(target_file_id).is_some()` — there's actual user
   content under it.

Condition (3) is what makes the predicate *structural* rather than
opt-in: a Lua filter that wraps existing user paragraphs in a
`Div.callout` produces a Generated Div whose children still carry
their original preimage → it's transparent → the visual editor sees
through it → user edits inside the wrapped content round-trip
cleanly. A filter that constructs a fresh Div from metadata has no
source-bearing children → it's atomic → editor treats it as a unit.
The filter author doesn't have to declare anything; the AST shape
declares it for them.

## Known transparent wrappers today

Produced by `pampa::pandoc::sugar::SectionizeTransform` and friends:

- **sectionize** Div — groups blocks by heading depth (`By::sectionize()`).
- **footnotes-container** Div — collects all footnote definitions.
- **appendix-container** Div — collects appendix-tagged content.

Plus, by structural construction, any Lua-emitted block-container
that meets the three conditions above (Plan 10).

**Not** transparent wrappers:

- `IncludeExpansion` CustomNode (Plan 8) — its `SourceInfo` is
  `Original`, anchored to the include-token bytes in the parent qmd.
  Descent stops at it; that's correct behaviour.
- Atomic CustomNodes like `CrossrefResolvedRef` — `SourceInfo`
  is `Original` pointing at the `@ref` token.
- The synthesized title-block Header (`By::title_block()`) —
  `is_atomic_kind` is `true` for `title-block`. Editor treats the
  resolved title as read-only; the user's source-side knob is the
  YAML `title:` key. (Not block-container shape either.)

## Reference primitive: `first_in_user_tree`

```rust
fn first_in_user_tree<T>(
    blocks: &[Block],
    extract: &impl Fn(&Block) -> Option<T>,
) -> Option<T>
```

Walks `blocks` depth-first. On each block, applies `extract`; if
`Some`, returns it. If `None`, descends through
`block_block_children` and tries again. This is how we see through
transparent wrappers — a wrapper has no source position of its own
(extract returns `None` for it), so the walker looks inside.

The two consumers today are one-liners:

```rust
fn derive_target_file_id(blocks: &[Block]) -> FileId {
    first_in_user_tree(blocks, &|b| b.source_info().root_file_id())
        .unwrap_or(FileId(0))
}

fn first_target_anchored_start_in(blocks: &[Block], target: FileId) -> Option<usize> {
    first_in_user_tree(blocks, &|b| {
        b.source_info().preimage_in(target).map(|r| r.start)
    })
}
```

A `visit_user_blocks(blocks, &mut visit)` sibling (visiting all user
blocks in document order, transparent wrappers skipped) is the
natural extension for callers that need every block, not just the
first; add it the moment a second caller wants it.

## When to use which

| Need | Tool |
|---|---|
| Find the first block where some property holds | `first_in_user_tree` |
| Visit all user blocks in document order | (add `visit_user_blocks` when needed) |
| Ask "is *this specific block* a transparent wrapper?" | `is_transparent_wrapper` |
| Get the document's editing-file id | `derive_target_file_id` |
| Find where the YAML frontmatter region ends | `first_target_anchored_start_in` |

`is_transparent_wrapper` is intentionally a small predicate — used
when a caller needs to make an *explicit* decision (e.g. a future
Q-3-44 diagnostic that hints "your filter walked into a sectionize
wrapper; you probably meant to walk its children"). Routine
source-position lookups should use the walkers, not the predicate.

## Where the code lives, and when to promote it

The primitives live in
`crates/pampa/src/writers/incremental.rs` next to
`block_block_children`. That's the right home today — the writer
is the only consumer.

Promote to `quarto-pandoc-types` (or a new
`quarto-pandoc-types::traversal` module) **the moment a second
crate needs them.** Plan 9's `DocumentProfile` extractor (when it
gains a "first H1" fallback), Plan 10's filter-output classifier,
and the project-replay engine's cell walker are the candidates.
Don't promote pre-emptively — premature generalisation has its own
debt.

## Adding a new synthesizer

If you're writing a new transform that wraps user content in a Div
(or other block container):

1. Emit `SourceInfo::generated(By::<your-kind>())` on the wrapper.
   No `Invocation` anchor (because there's no source token).
2. Preserve the children's existing source_info — don't restamp
   them with the wrapper's `By`. The whole point is that the
   children stay editable.
3. Your wrapper is automatically transparent; nothing else to do.
4. If your `By::<your-kind>()` should *also* be considered
   `is_atomic_kind()` (the resolved children are read-only, like
   shortcode resolutions), add it to the atomic-kind set in
   `crates/quarto-source-map/src/source_info.rs` — separate
   concept, separate decision.

## Anti-patterns

- `ast.blocks[0]` for source-position questions (file id, start
  offset, "the first user block"). Use `first_in_user_tree`.
- `ast.blocks.iter()` flatly for "every user block" enumeration
  when the document might be wrapped. Use a descending visitor.
- Declaring a transparent wrapper via a `By::kind` registry. The
  predicate is structural; don't add an opt-in mechanism that the
  shape already encodes.
- Asking "is this Generated and atomic-kind?" when what you mean
  is "should I descend?" — `is_atomic_kind` and transparency are
  orthogonal. Shortcode resolutions are atomic *and* have
  Invocation anchors (descent is meaningful but the resolved
  content is read-only). Sectionize Divs are *neither* atomic
  *nor* invocation-anchored. Mixing the two predicates produces
  subtle bugs.

## History

| Date | Commit | What |
|---|---|---|
| 2026-05-25 | `bdcfdc53` | `coarsen` recurses Transparent into non-atomic Generated wrappers (the first bug — empty qmd) |
| 2026-05-25 | `b9f64b56` | `derive_target_file_id` descends; Plan 7c Phase 8 closed |
| 2026-05-25 | `2bf92664` | `emit_metadata_prefix` descends; YAML frontmatter preserved |
| 2026-05-25 | (this doc) | Pattern named, primitives centralized |
