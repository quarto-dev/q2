# Plan 7d — Algebraic soundness of plan_user_writes / incremental-writer

**Date:** 2026-05-26 (revised 2026-05-29)
**Branch:** feature/provenance (ships after 7f; before 7e)
**Status:** Implementation-ready. Phase 0 closed 2026-05-29. Design content moved to [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md). This plan is the implementation roadmap.
**Milestone:** none directly. Pre-condition for any future "minimal-edit diffing" work that would consume the user-write plan to derive per-region Monaco edits rather than full-document saves.

## Epic context

Fourth sibling follow-up to Plan 7 in the provenance epic:

| Sibling | Axis | Status |
|---|---|---|
| Plan 7 | Incremental writer + soft-drop + bridge migration | shipped on `feature/provenance` |
| Plan 7a | Runtime user-filter idempotence (input-side validation) | open |
| Plan 7b | Test-coverage consolidation | open |
| Plan 7c | Closure gaps in the existing soft-drop cascade | open (Phases 7 and 7b become defense-in-depth under 7d) |
| Plan 7d | Algebraic soundness of the coarsen/write step | **this plan** |
| Plan 7e | CustomNode qmd serialization | sibling, ships after 7d |
| Plan 7f | Prerequisites for 7d (framework + test hygiene + wire-format) | sibling, ships before 7d |

7d differs from 7c in *disposition*. Plan 7c tightens the existing denylist cascade — each phase adds a branch the cascade should have caught but didn't, or repairs a per-arm predicate that drifts from accuracy. Plan 7d replaces the cascade with an allowlist algebra: every emission is allowed by construction rather than by the absence of a denylist match. If 7c's Phases 7 / 7b haven't shipped by the time 7d lands, they become defense-in-depth — the algebra catches the cases they protect against, *provided* the producer contract is satisfied.

The implementation starts from the current HEAD of `feature/provenance` after 7f lands, so the framework's source_info preservation and user-edit stamping are in place, the wire format renames are done, and the `SourceInfo::default()` test-audit has bottomed out. The `CoarsenedEntry` self-containment property established by commit `e584428d` — every variant produces its emit bytes from its own payload without ambient context — is a precondition for the algebra to compose correctly. Plan 7d is the next step on top.

## Goal

Replace the writer's flat per-arm cascade with a recursive structural dispatch — `plan_user_writes`, the rename of `coarsen` — whose inductive soundness argument discharges BP without per-arm checking. The full statement of BP, the two contracts that make it hold, and the proof of soundness live in [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md). This plan does not restate them; it specifies the implementation.

This is a refactor of one layer of the system. The reconciler, the AST types, `apply_reconciliation`, and the diagnostic catalog are not touched.

## Phase 0 — Validate the algebra (closed 2026-05-29)

All seven validation items resolved through design review with Gordon. Outcomes:

- (1) **R5 trust point: strict.** Single `Generated{by: user_edit, …}` shape. Enforcement is client-side in the React framework (see Plan 7f Phase 3). No empty-preimage tolerance in 7d; 7f ships first to make the framework honest, and 7d trusts the producer contract.
- (2) **CustomNode treatment.** Decomposed shell helpers are correct; current `Block::Custom` arms in the qmd writer are empty placeholders. Split to Plan 7e as a separate scope.
- (3) **List shape.** `ShellOpen` becomes an enum: `Bytes(Bytes)` for fixed-prefix shells (Div, etc.) and `LineAware { marker, continuation_indent }` for per-line markers (lists, BlockQuote when modeled this way). The marker emits once per item; continuation lines get the indent via a `Write` adapter analogous to today's `BulletListContext`.
- (4) **Separator state.** `SeparatorRule` per `Recurse` (variants: `StandardBlock { tight }`, `InlineConcat`, `ListItem { loose }`, `OriginalGap`). One cross-`Recurse` state, `TrailingState` (four variants: `None`, `EndsWithText`, `EndsWithNewline`, `EndsWithBlankLine`), threaded through `assemble` as a function parameter.
- (5) **Plan 7c relationship.** Phases 7 and 7b of 7c become defense-in-depth under the algebra; can be dropped, kept, or shipped before/after 7d.
- (6) **Cost.** Phase 4 adds end-to-end benchmarking on a synthetic 500-block fixture with realistic content (sectionize wrappers, shortcodes, callouts). Both old `coarsen` and new `plan_user_writes` are O(n) on subtree size; the bench is a sanity check, not a hopeful negotiation.

Property #9 added: **block-level Invocation coalescing.** Within any `Recurse`, a maximal run of consecutive children whose `preimage_in(target)` returns the same `Some(range)` collapses to a single `Verbatim` of that range. Block-level analogue of today's multi-inline dedupe.

Phases 1–6 below proceed against `feature/provenance` HEAD after 7f has landed.

---

## Reconciler / writer architecture (brief)

The system has three layers between the user's edit and the bytes that hit disk:

**Layer 1 — Reconciler (`crates/quarto-ast-reconcile`).**
Input: two ASTs (`original`, `executed`). Output: a hierarchical `ReconciliationPlan` of alignment decisions describing how `executed` relates structurally to `original`. The output operations are three per AST level: `KeepBefore(orig_idx)` (this position's content matches the original), `UseAfter(exec_idx)` (this position's content is new), `RecurseIntoContainer { before, after }` (the container at this position was structurally paired; descend into its children for the diff). Same three variants exist for `BlockAlignment` and `InlineAlignment`; lists use a separate three-variant `ListItemAlignment` enum.

The reconciler's algorithm is three-phase (`crates/quarto-ast-reconcile/src/compute.rs:37-215`):
1. Phase 1: exact hash matches anywhere in the original block list → `KeepBefore`.
2. Phase 2: positional type matches at the same index → `RecurseIntoContainer` (or `KeepBefore` for inline-content blocks when the inline plan finds at least one matching inline).
3. Phase 3: fallback for unmatched blocks → `UseAfter`.

A `ReconciliationPlan` is a flat `Vec<BlockAlignment>` plus side-tables of nested plans (one per recursion target): `block_container_plans`, `inline_plans`, `inline_container_plans`, `note_block_plans`, `custom_node_plans`, `table_plans`, `list_item_alignments`, `list_item_plans`. The plan tree mirrors the AST's tree structure.

**Layer 2 — `apply_reconciliation` (`crates/quarto-ast-reconcile/src/apply.rs`).**
Consumes a `ReconciliationPlan` and produces a new `Pandoc` by *moving* original blocks where the plan says `KeepBefore`, *moving* executed blocks where it says `UseAfter`, and recursively reconciling where it says `RecurseIntoContainer`. The output is an AST, not bytes.

The writer does not use `apply_reconciliation`. The two consumers of a `ReconciliationPlan` — `apply_reconciliation` and the writer's `plan_user_writes` — sit side by side, each interpreting the same plan into a different output medium.

**Layer 3 — Writer's `plan_user_writes` (`crates/pampa/src/writers/incremental.rs`).**
Inputs: `original_qmd: &str`, `original_ast: &Pandoc`, `new_ast: &Pandoc`, `plan: &ReconciliationPlan`, `target_file_id`. Output: a tree of `UserWrite` entries that `assemble` walks to produce `Source'`.

The `UserWrite` variants are the writer's internal byte-emission language. The full shape lives below; the semantic decoder is in the design doc.

## Cumulative delta — `main` → `feature/provenance` → 7d

The writer has three states of interest:

**State A — `main` (pre-Plan-7).** Skeletal incremental writer. `CoarsenedEntry` has three variants: `Verbatim`, `Rewrite`, `InlineSplice`. No soft-drop, no editability predicate, no source-info-aware dispatch. Flat per-top-level-block coarsen.

**State B — `feature/provenance` HEAD (Plan 7 shipped + recent fixes).** `CoarsenedEntry` has five variants: `Verbatim`, `Rewrite`, `InlineSplice`, `Transparent`, `Omit`. Soft-drop cascade with six cases. Per-arm source-info-aware dispatch. Known structural weaknesses (`Rewrite`-as-subtree-serializer is the catch-all; per-arm predicate duplication).

**State C — after Plan 7d.** `UserWrite` (renamed) has four variants: `Verbatim`, `Omit`, `Recurse`, `Leaf`. The dispatch is a single table over `(alignment_kind, source_info_shape)`. `Rewrite` is gone — decomposed into `Recurse` over containers + `Leaf` over actual leaves. `Transparent` is gone (subsumed by `Recurse` with empty shells). `InlineSplice` is gone (subsumed by `Recurse` where shells come from the original block's prefix/suffix). BP is provable by induction on the dispatch table (proof in design doc).

The net effect from State A to State C is significant but not radical: the *concept* of an incremental writer with a coarsened intermediate language is preserved from `main`; what changes is the variant set, the dispatch shape, the function name, and the soundness property.

## The proposed algebra

### The user-write language

```rust
enum UserWrite {
    Verbatim {
        byte_range: Range<usize>,
        orig_idx: Option<usize>,  // separator hint, never byte-production context
    },
    Omit {
        warning: Option<Diagnostic>,
    },
    Recurse {
        shell_open:  ShellOpen,
        children:    Vec<UserWrite>,
        shell_close: Bytes,
        separator:   SeparatorRule,
    },
    Leaf {
        block_text: Bytes,
    },
}

enum ShellOpen {
    Bytes(Bytes),                                       // fixed prefix; most containers
    LineAware { marker: Bytes, continuation_indent: Bytes },  // list items, BlockQuote
}

enum SeparatorRule {
    StandardBlock { tight: bool },     // \n\n for loose, \n for tight; the default for non-list blocks
    InlineConcat,                       // empty; inlines concatenate directly
    ListItem { loose: bool },           // pairs with LineAware shell
    OriginalGap,                        // when adjacent children are preimage-derived & consecutive, use original bytes
}
```

This unifies today's five variants into four. Renames and consolidations:

- Today's `Transparent { child_entries }` becomes `Recurse { shell_open: Bytes(""), shell_close: "", children, separator }`. The wrapper contributes no bytes; only the children's compositions do. Sectionize Div, footnotes container, appendix container all use this shape today and continue to under the algebra.
- Today's `InlineSplice { block_text, orig_idx }` becomes `Recurse` where `shell_open` and `shell_close` are the *original-qmd prefix and suffix bytes* of the block being spliced, and `children` are the inline children. Today's `InlineSplice` is essentially "the container's wrapper is the same as the original; only the inside changed" — exactly what `Recurse` models when shells come from `Source`.
- Today's `Rewrite { block_text }` does *not* survive in its current form. Its substance — "serialize this subtree as text" — is decomposed. For container blocks, that work becomes a `Recurse` whose shells come from the qmd writer's container-shell helpers and whose `children` come from the algebra's recursion. For leaf blocks, that work becomes a `Leaf` whose `block_text` is `serialize_leaf(node)` — `serialize_leaf` being `write_block_to_string` restricted to nodes with no recursable descendants.

`assemble : (UserWrite, TrailingState) → (Bytes, TrailingState)` is the fold. `Verbatim` returns `Source[byte_range]`; `Omit` returns empty; `Recurse` returns `shell_open ++ join(separator, [assemble(c) for c in children]) ++ shell_close`, with the separator's emission consulting the trailing-state hint to suppress redundant blank lines; `Leaf` returns `block_text`.

### The dispatch table

`plan_user_writes : (Node, target, align_ctx) → UserWrite`. Total recursive function over the AST, dispatched on the pair `(align_ctx.alignment_kind, node.source_info_shape)`. The table:

| Alignment | Source-info / structure | Rule | Operation |
|---|---|---|---|
| `KeepBefore(i)` | preimage in target | R1 | `Verbatim(preimage)` |
| `KeepBefore(i)` | atomic-kind Generated, no preimage | R2 | `Omit` (no warning; content regenerates from baseline) |
| `KeepBefore(i)` | non-atomic, no preimage, container with source-bearing children | R3 (Transparent-form) | `Recurse{ Bytes(""), children-coarsened, "", separator }` |
| `KeepBefore(i)` | non-atomic, no preimage, no recursable children | R5 | `Leaf{ serialize_leaf(node) }` (rare; cross-file-rooted leaf, etc.) |
| `UseAfter(j)` | atomic-kind Generated with preimage | R1' (soft-drop) | `Verbatim(preimage)` + Q-3-43 |
| `UseAfter(j)` | atomic-kind Generated, no preimage | R2' (soft-drop) | `Omit` + Q-3-43 |
| `UseAfter(j)` | atomic `Custom` | R5-special (let-user-win) | `Leaf{ serialize_leaf(node) }` via `plain_data`; no warning |
| `UseAfter(j)` | non-atomic, no preimage, container | R3 | `Recurse{ shell_open, children, shell_close, separator }` shells from qmd writer's per-container syntax helpers |
| `UseAfter(j)` | non-atomic, no preimage, leaf | R5 | `Leaf{ serialize_leaf(node) }` |
| `UseAfter(j)` | non-atomic with preimage | R1 | `Verbatim(preimage)` (paste-from-elsewhere; trust the producer's source_info) |
| `RecurseIntoContainer{ before, after }` | non-editable inside | R1' / R2' (soft-drop) | `Verbatim(preimage)` + Q-3-43 (if preimage exists), or `Omit` + Q-3-43 (no preimage). Recursion stops here. |
| `RecurseIntoContainer{ before, after }` | editable inside, block container | R3 | `Recurse{ shell_open, children-coarsened-per-`block_container_plans`, shell_close, separator }` |
| `RecurseIntoContainer{ before, after }` | editable inside, inline container | R4 | `Recurse{ shell_open-from-original-prefix, inlines-coarsened-per-`inline_plans`, shell_close-from-original-suffix, separator }` |

The dispatch is total. R3 and R4 are structurally the same operation (recurse with shells); they're listed separately because R3 dispatches on `block_container_plans` while R4 dispatches on `inline_plans`, and the shell sources differ (R4 takes shells from the *original* block's source bytes for the inline-splice case; R3 takes shells from the new container's syntax helpers).

Soundness proof: see [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md) §"Soundness."

### Properties enforced

The algebra implies the following properties as theorems:

1. **(BP) Byte-provenance soundness.** For every byte of `Source'`, (P1) or (P2) holds. Proven in the design doc.
2. **Totality of dispatch.** Every `(node, align)` pair matches exactly one row in the table.
3. **Compositionality.** `plan_user_writes(container) = Recurse(shells, [plan_user_writes(child)])`. The writer's behavior on a subtree is a function of its behavior on the components.
4. **Source-info-driven dispatch within alignment kind.** Given a fixed alignment kind, the rule that fires depends only on `node.source_info` and structural shape. No ambient context. No per-arm duplication of predicates.
5. **Leaf-only serialization.** `serialize_leaf` is invoked only on nodes that the algebra classifies as R5 leaves. No path through the algebra calls a serialization function on a non-leaf AST node without first recursing into its children via R3 / R4.
6. **Termination.** Recursion only on strictly smaller substructures (children). The AST is finite. Termination is by structural induction on AST size.
7. **Diagnostic determinism.** The set of warnings produced is a function of the input ASTs alone — the warning a `(alignment, source_info)` cell emits is fixed by the table; no order-dependence, no cascade-arm-dependence.
8. **Reconciler-independence of rule choice.** The reconciler's `Plan` informs *which* node is coarsened at each position and *what alignment context* applies, but rule selection within a row is determined by source_info and structural shape alone.
9. **Block-level Invocation coalescing.** Within any `Recurse`, no byte range in the target file is emitted more than once at adjacent child positions. AST nodes whose `preimage_in(target)` returns the same range — multi-inline shortcode resolution, multi-block shortcode resolution, any future N-to-1 producer — collapse to a single `Verbatim` of the shared range.

### What the refactor concretely changes

**1. `write_block_to_string` decomposes into shell helpers + `serialize_leaf`.** The unified-pass version (`write_block_to_string` as it exists today) becomes a derived convenience function that the rest of the codebase can still call for native rendering — it just isn't used by the incremental writer's `plan_user_writes` step anymore.

The decomposition covers the container kinds that have qmd writer arms today: BlockQuote, Div, Figure, NoteDefinitionFencedBlock, OrderedList, BulletList, DefinitionList, Table. **CustomNodes (Callout, Theorem, Proof, FloatRefTarget, labelled equations) do not have qmd writer arms today** — their `Block::Custom` arms in `qmd.rs:2354` are empty. CustomNode shell helpers land in Plan 7e, not in 7d Phase 1. Under 7d alone, custom-node editing remains broken (visible as a callout-disappears-on-edit bug); 7e closes that gap.

**2. `UserWrite::Rewrite` ceases to exist.** `Leaf { block_text }` replaces it for genuine leaves. `Recurse` replaces it for containers. No catch-all subtree serializer remains.

**3. `plan_user_writes` dispatch becomes source-info-aware uniformly.** Today's per-arm cascade with predicate duplication disappears. For each block we intend to emit, compute `(alignment_kind, source_info_shape)`, look up the row, apply the rule.

The inline cascade in `assemble_inline_content` undergoes the analogous restructuring. The multi-inline dedupe optimization is preserved as a `SeparatorRule::InlineConcat`-flavored coalescing within `Recurse`. Property #9 above generalizes that dedupe to the block level.

### Where user edits land

A practical clarification, because the discussion that led to this plan kept conflating "where bytes get serialized" with "where user edits land in the output." They are different questions.

The algebra has three base cases that produce bytes:

- **R1 (Verbatim).** Emits bytes from `Source`. These bytes were authored by the user *at the position they came from*. R1 fires for unchanged content (KeepBefore on a node with preimage) AND for atomic-content soft-drop (UseAfter or RecurseIntoContainer on a non-editable node with preimage — substitute the preimage as the safe alternative). Same emission operation, different alignment contexts.
- **R3 / R4 shell bytes.** Emitted by `Recurse`'s shell-emission step. These are the *syntax* of a container — the `:::` of a Div, the `> ` of a BlockQuote, the `- ` of a list item, the `:::{.callout-note}` of a callout. The bytes are user-authorable because the user could have typed them directly in qmd. R3's shells come from the qmd writer's syntax helpers when the container is newly constructed; R4's shells come from `Source` when the container is being inline-spliced (preserving the original block's wrapping bytes).
- **R5 (Leaf serialization).** Emits bytes from `serialize_leaf(node)` — the leaf node's own content rendered as text. `Str("hello")` becomes `hello`. `Code` block emits its code-fence syntax plus content (treated as a leaf because its content is bytes, not children needing recursion). Atomic `Custom` (via let-user-win) emits its qmd syntax derived from `plain_data`.

User edits land at all three:

| Kind of edit | Rule(s) that produce bytes for the edit | Example |
|---|---|---|
| User reorders / wraps / moves existing content | R1 (copies preserved bytes from `Source` at original positions) + R3/R4 shells (for new containers wrapping the moved content) | Wrap three paragraphs in a blockquote: R1 copies the paragraph bytes, R3 emits `> ` prefixes. |
| User constructs a new structural parent | R3 / R4 (shells of the new container) + recursion through children | Add a new list item: R3 emits the list's per-item iteration, the new item's R3 emits `- `, the item's children fire their own rules. |
| User types new leaf content | R5 (serialize the new leaf) | Type a word in a Para: R5 emits the new `Str`'s text. |
| User replaces atomic Custom via component picker | R5-special (let-user-win on atomic Custom) | Pick a different include source: R5 emits the new `{{< include … >}}` syntax derived from `plain_data`. |
| User attempts to edit atomic-Generated content | R1' soft-drop or R2' soft-drop (emit preimage + warning, OR omit + warning) | Type into a lipsum-resolved paragraph: R1' emits the `{{< lipsum 3 >}}` token, Q-3-43 warns. |

A single user edit typically produces bytes from *multiple* rules in combination. The algebra's recursion walks down the new AST shape, choosing the right rule at each level based on `(alignment, source_info)`.

---

## Phases

### Phase 1 — Decompose `write_block_to_string`

- [ ] Identify every per-container arm in `crates/pampa/src/writers/qmd.rs` that produces output for a container block (Div, BlockQuote, Figure, NoteDefinitionFencedBlock, OrderedList, BulletList, DefinitionList, Table).
- [ ] For each, extract `serialize_block_shell_open(block) → ShellOpen` and `serialize_block_shell_close(block) → Bytes`. For lists and BlockQuote (which need per-line marker semantics), the shell-open returns `ShellOpen::LineAware`; for everything else, `ShellOpen::Bytes`.
- [ ] Do the same for inline containers (Emph, Strong, Link, Image, Span, Cite, Note, …): `serialize_inline_shell_open(inline) → ShellOpen` and `serialize_inline_shell_close(inline) → Bytes`.
- [ ] Define `serialize_leaf(node) → Bytes` as `write_block_to_string` restricted to leaves. Type-enforce or runtime-assert that the function panics on non-leaf input.
- [ ] Preserve `write_block_to_string` as a public convenience function. Its implementation becomes `shell_open + assemble(children-coarsened) + shell_close` — but the incremental writer no longer calls it.
- [ ] **CustomNode arms intentionally not in 7d's scope.** The current empty `Block::Custom(_)` arm stays empty under 7d; Plan 7e fills it. Under 7d, R3 on a non-atomic CustomNode (e.g. Callout) falls through to soft-drop semantics, not bytes-emission — custom-node editing remains visibly broken until 7e.
- [ ] Tests: each shell-helper has a unit test that asserts its output for a known node.

### Phase 2 — Restructure `plan_user_writes` dispatch

- [ ] Define the new `UserWrite` shape with `Verbatim`, `Omit`, `Recurse`, `Leaf` variants. Define `ShellOpen` and `SeparatorRule` enums. Delete `CoarsenedEntry::{Rewrite, Transparent, InlineSplice}` (their roles are absorbed).
- [ ] Rename `coarsen` → `plan_user_writes`, `coarsen_blocks` → `plan_user_writes_blocks` (or absorb into `plan_user_writes`). The function-level renames cascade through `~23` in-file references and `~16` plan-7 references in this file (already done above). `coarsen_keep_before_block` disappears: its logic is absorbed into the dispatch table.
- [ ] Implement the dispatch table from §"The dispatch table" as a single `dispatch(node, align, target) → Rule` function. Each rule has a small implementation: R1 packages `Verbatim`; R2 packages `Omit`; R3 / R4 package `Recurse` with shells from Phase 1's helpers and children recursed via `plan_user_writes`; R5 packages `Leaf { serialize_leaf(node) }`.
- [ ] `plan_user_writes_blocks` becomes a thin wrapper that iterates the `block_alignments`, calls `dispatch` for each, threads separator context.
- [ ] Implement Property #9 (block-level Invocation coalescing): within each `Recurse`, group consecutive children whose `preimage_in(target)` returns the same range; emit a single `Verbatim` for the run.
- [ ] **Preserve document-boundary infrastructure.** The existing helpers `emit_metadata_prefix` (`incremental.rs:942`), `find_metadata_trailing_gap` (`:998`), and `ensure_trailing_newline` (`:1103`) handle the gap between YAML frontmatter and the first block, and the parser's input-padding convention (qmd reader pads input with `\n` when it doesn't end with one). The new `plan_user_writes` + `SeparatorRule` + `TrailingState` design must preserve their behavior. Specifically: `assemble` must still emit the metadata-prefix bytes before the first block's coarsened entry, and must still strip the synthesized trailing `\n` from output when the input qmd didn't have one. Add a regression test for both behaviors on a fixture that exercises them (a doc with YAML frontmatter, no trailing newline).
- [ ] Verify against today's regression tests: every existing test in `crates/pampa/tests/incremental_writer_tests.rs` must still pass byte-for-byte. The refactor doesn't change observable behavior on the inputs the tests cover (modulo CustomNodes, where current behavior is also broken).

### Phase 3 — Restructure `assemble_inline_content`

- [ ] Define inline `Rule` dispatch analogous to block-level. R1-inline, R2-inline, R3-inline (inline `Recurse` for nested inline containers), R5-inline (leaf inline).
- [ ] `assemble_inline_content` becomes a recursive plan over the inline cascade. Phase 1's two-phase shape (soft-drop substitution + emit-with-dedupe) collapses to a single pass.
- [ ] Multi-inline dedupe (today's `compute_separator` shared-`Invocation`-anchor optimization) collapses into the block-level Property #9 mechanism; the rule keys on `preimage_in` equality rather than anchor `PartialEq` (slightly more general; catches cross-shape collisions).
- [ ] Verify: every existing inline-cascade test passes byte-for-byte.

### Phase 4 — Property tests for BP + benchmarking

The algebra is sound by construction, but a property test pins the invariant against bugs in the implementation.

- [ ] Write a proptest generator `gen_pandoc_with_atomic_descendants` that produces ASTs with atomic-Generated descendants at varying depths inside non-atomic containers, plus arbitrary user edits applied.
- [ ] Write the property `bp_holds` (soundness): given a generated `(AST_old, AST_new, Source)` and a reconciler plan, run the writer. Assert: the output `Source'` does not contain any of the resolved bytes of atomic-Generated descendants. (Implementation: tag the generator's atomic-resolved content with a recognizable marker string; assert the marker doesn't appear in `Source'`.)
- [ ] Write the property `completeness_holds`: for inputs that don't trigger soft-drop, `parse(Source')` is structurally equivalent to `AST_new`. Implementation: filter the generator to skip cases where the reconciler's plan + atomic-classification would route any node to R1' or R2'; assert byte-level equivalence (or AST-level for cases where helper canonicalization legitimately differs from original — list markers, lazy numbering, block-container shells). The two properties together pin both soundness (no leaks) and completeness (no drops outside soft-drop).
- [ ] Add property tests for individual rule soundness: R1 emits bytes from `Source`; R5 emits authored content with no descendants; R3 / R4 emit bytes that are concatenations of shell + children.
- [ ] Run under `cargo nextest run -p pampa` with high iteration counts. Save regression seeds if any fail.

**Benchmarking subtask.** Synthetic fixture: ~500 top-level blocks with a mix of plain paragraphs, sectionize wrappers, shortcodes, callouts, and one nested list. Measure end-to-end `incremental_write_qmd` time on (a) a single-block edit and (b) a whole-document edit. Both old `coarsen` and new `plan_user_writes` are O(n); the bench is a sanity check.

- [ ] Build the 500-block fixture as a checked-in test fixture under `crates/pampa/tests/fixtures/perf/`.
- [ ] Add a benchmark harness that runs the same edit against both branches' implementations (the comparison is against the `feature/provenance` HEAD baseline before 7d).
- [ ] Assert: new is within 2× of baseline for case (a); within 1.5× for case (b). If those bounds hold, performance is not a concern.

### Phase 5 — Retire denylist branches obviated by the algebra

- [ ] Audit Plan 7c's open phases. For each phase that becomes defense-in-depth under the algebra (Phase 7's `displaced_before_idx`, Phase 7b's inline atomic-Generated check), decide whether to retain or drop.
- [ ] Remove obsolete branches from the codebase. Update tests to match.

### Phase 6 — Update design docs

- [ ] [`claude-notes/designs/incremental-writer-contract.md`](../designs/incremental-writer-contract.md) has already been rewritten to specify BP, the dispatch table reference, and the soundness proof. Cross-link from this plan.
- [ ] Cross-link from `provenance-contract.md` §7 (atomic-kind set and consumer impact).
- [ ] Add a "Follow-ups closed" entry to Plan 7 pointing here, retiring the algebraic-soundness item from its open tail.

## What 7d does not change

Explicit non-changes, for clarity:

- **The reconciler's algorithm.** `compute_reconciliation` and its helpers stay as they are. Three-phase pass; same hash-match / positional / fallback logic.
- **`BlockAlignment` / `InlineAlignment` / `ListItemAlignment` types.** Same variants. No payload changes.
- **`apply_reconciliation` (AST-level reconciliation).** Independent of the writer; not touched.
- **`ReconciliationPlan` shape.** All sub-plan tables (`block_container_plans`, `inline_plans`, etc.) stay.
- **The wire format.** The plan is computed inside WASM and never crosses the boundary as JSON; nothing in `ts-packages/quarto-sync-client/src/types.ts` changes. (The wire-format renames of `attrS` → `a` and `sourceInfoPool` → `p` ship in Plan 7f, not 7d.)
- **The diagnostic catalog.** Q-3-41, Q-3-42, Q-3-43 stay. The algebra reorganizes which dispatch row emits which code; the codes themselves don't change.
- **The producer-side contract (`provenance-contract.md`).** The role-asymmetry rule, the `By::` catalog, the atomic-kind set — all stay. The algebra inherits these as preconditions on its input.
- **Pre-existing list/blockquote marker fidelity gaps.** Bullet markers (`*` / `-` / `+`) collapse to `*`; ordered-list lazy numbering (e.g. `1. / 1. / 1.`) regenerates as `1. / 2. / 3.`. Blockquote `>` prefix variations normalize. 7d preserves this pre-existing behavior; fidelity requires a typed-AST extension (per-item source_info on list items) that's out of scope. Tracked as a separate follow-up.
- **CustomNode qmd serialization.** The qmd writer's `Block::Custom` arm is currently empty; 7d does not fix this. The CustomNode shell helpers needed by R3 land in Plan 7e. Under 7d alone, custom-node edits remain broken; 7e closes that gap.

## Relationship to siblings

- **Plan 7** (shipped): provides the existing writer the algebra refactors. 7d's implementation phases (1–6) start from `feature/provenance` HEAD after 7f has landed.
- **Plan 7a** (open): runtime user-filter idempotence detection. Orthogonal.
- **Plan 7b** (open): test-coverage consolidation. The property tests in 7d Phase 4 *complement* Plan 7b's per-shape regression tests.
- **Plan 7c** (open): closure gaps in the denylist cascade. Phases 7 / 7b become defense-in-depth under 7d; Phases 1–6 stay useful as standalone bug fixes.
- **Plan 7e** (sibling): CustomNode qmd serialization. Ships after 7d; closes the callout-disappears-on-edit bug that 7d does not address.
- **Plan 7f** (sibling, ships before 7d): framework source_info preservation, user-edit stamping, wire-format renames, `SourceInfo::default()` audit. Prerequisite for 7d's strict R5 trust point.

## Risks

- **Refactor scope.** The decomposition of `write_block_to_string` touches every per-container arm in the qmd writer (excluding CustomNodes, which are 7e's scope). Each arm is small but there are many of them. Estimating 500-800 LOC of mechanical refactor work, plus 200-400 LOC of dispatch-table consolidation. Plan 7e adds another ~500-800 LOC for CustomNode shell helpers.
- **Behavioral compatibility.** Every existing test must pass byte-for-byte after the refactor on inputs that don't involve CustomNodes. CustomNode tests are 7e's concern.
- **Cost.** The Phase 4 benchmark is the verification mechanism; if cost regresses significantly, the benchmark catches it.
- **Producer-contract drift.** The algebra leans on producer hygiene at R5's trust point. If a producer introduces a leaf with non-default source_info that doesn't fit the algebra's classifications, R5 may emit bytes the algebra trusts but shouldn't. Plan 7f's `SourceInfo::default()` audit + the producer contract's "new kinds default to non-atomic" rule are the mitigations.

## References

- Design doc / contract: [`claude-notes/designs/incremental-writer-contract.md`](../designs/incremental-writer-contract.md).
- Producer-side contract: [`claude-notes/designs/provenance-contract.md`](../designs/provenance-contract.md).
- Prerequisites plan: [`2026-05-29-q2-preview-plan-7f-prereqs.md`](2026-05-29-q2-preview-plan-7f-prereqs.md).
- CustomNode plan (follow-on): [`2026-05-29-q2-preview-plan-7e-customnode-qmd.md`](2026-05-29-q2-preview-plan-7e-customnode-qmd.md).
- Today's writer: `crates/pampa/src/writers/incremental.rs`.
- Reconciler: `crates/quarto-ast-reconcile/src/compute.rs` (algorithm), `src/types.rs` (alignment types), `src/apply.rs` (AST-level apply, not used by the writer).
- Sibling primitive: [`claude-notes/designs/transparent-wrappers.md`](../designs/transparent-wrappers.md) — the traversal-side analogue (`first_in_user_tree`) of the writer's emission-side recursion.
- Plan 7 (shipped): [`2026-05-04-q2-preview-plan-7-incremental-writer.md`](2026-05-04-q2-preview-plan-7-incremental-writer.md) — note prepended to that plan: "Plan 7d renames `CoarsenedEntry` to `UserWrite` and replaces the cascade with an algebraic dispatch. The code samples below show the Plan-7-era shape; see `incremental-writer-contract.md` for the current shape."
- Plan 7c (open): [`2026-05-25-q2-preview-plan-7c-closure-gaps.md`](2026-05-25-q2-preview-plan-7c-closure-gaps.md) — the denylist-tightening sibling.
