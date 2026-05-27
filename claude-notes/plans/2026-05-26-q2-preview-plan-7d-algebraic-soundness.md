# Plan 7d — Algebraic soundness of the coarsen / incremental-writer

**Date:** 2026-05-26
**Branch:** feature/provenance (research; refactor work happens on a child branch once Phase 0 validation closes)
**Status:** Research plan — Phase 0 ("Validate the algebra") gates every subsequent phase. The technical description in this file's introduction is the artifact under review.
**Milestone:** none directly. Pre-condition for any future "minimal-edit diffing" work that would consume the coarsened plan to derive per-region Monaco edits rather than full-document saves.

## Epic context

Fourth sibling follow-up to Plan 7 in the provenance epic:

| Sibling | Axis | Status |
|---|---|---|
| Plan 7 | Incremental writer + soft-drop + bridge migration | shipped on `feature/provenance` |
| Plan 7a | Runtime user-filter idempotence (input-side validation) | open |
| Plan 7b | Test-coverage consolidation | open |
| Plan 7c | Closure gaps in the existing soft-drop cascade | open |
| Plan 7d | Algebraic soundness of the coarsen/write step | this plan |

7d differs from 7c in *disposition*. Plan 7c tightens the existing denylist cascade — each phase adds a branch the cascade should have caught but didn't, or repairs a per-arm predicate that drifts from accuracy. Plan 7d replaces the cascade with an allowlist algebra: every emission is allowed by construction rather than by the absence of a denylist match. The two plans are parallel, not sequenced. If 7d lands first, two of 7c's phases (Phase 7 and the newly-added Phase 7b) become *defense-in-depth* rather than load-bearing — they patch producer-side hygiene failures the algebra would tolerate but does not strictly require. If 7c lands first, the denylist gets more complete in the meantime; 7d's refactor still proceeds against whatever state 7c leaves behind.

The implementation in this plan starts from the current HEAD of `feature/provenance`, after the rebase that landed `incremental-writer-contract.md` (today's Task 1) and after the soft-drop fix that prompted this whole line of investigation (commit `e584428d`). The CoarsenedEntry self-containment property that fix established — every variant produces its emit bytes from its own payload without ambient context — is a precondition for the algebra to compose correctly. Plan 7d is the next step on top.

## Goal

Bring the writer's coarsen step under a soundness proof. The property to prove is the **byte-provenance invariant (BP)**: every byte the writer emits is either (i) copied verbatim from `Source` at a position some AST node identifies as its source-side knob, or (ii) produced by serializing a single AST leaf whose own immediate content is the user's authored content. The invariant rules out, by structural induction over the AST: resolved shortcode bytes leaking back into source; filter-output bytes leaking back into source; synthesized container chrome leaking back into source; in general, any byte derived from pipeline output that the user could not have authored at the position it lands.

Today's writer satisfies BP only by enumeration: a list of per-alignment-arm predicates that have grown branch-by-branch as bugs surfaced. The list is incomplete by construction (the lipsum fix on `e584428d` was one example; the new Plan 7c Phase 7b is the inline-level analogue). The goal is to replace the enumeration with a *total* dispatch on `(alignment_kind, source_info_shape)` whose inductive soundness argument discharges BP without per-arm checking.

This is a refactor of one layer of the system. The reconciler, the AST types, `apply_reconciliation`, and the diagnostic catalog are not touched.

## Status

Phase 0 — Validate the algebra — is the gating phase. The text under "The proposed algebra" below is the artifact under review. No code changes happen in Phase 0; the user reads, asks questions, and either approves the algebra (allowing Phases 1-6 to proceed) or sends it back for revision.

Phases 1-6 are *sketched* below, not specified in fine detail. Their concrete shape depends on decisions made during Phase 0 (which trust points to tighten, which `CoarsenedEntry` variants survive the refactor, how the qmd writer's per-container arms decompose). The sketches exist so Phase 0 can judge the *scope* of work the algebra implies, not so a fresh agent can implement them cold.

---

## Reconciler / coarsen architecture (brief)

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

The writer does not use `apply_reconciliation`. The two consumers of a `ReconciliationPlan` — `apply_reconciliation` and the writer's `coarsen` — sit side by side, each interpreting the same plan into a different output medium.

**Layer 3 — Writer's `coarsen` (`crates/pampa/src/writers/incremental.rs`).**
Inputs: `original_qmd: &str`, `original_ast: &Pandoc`, `new_ast: &Pandoc`, `plan: &ReconciliationPlan`, `target_file_id`. Output: `Vec<CoarsenedEntry>` — a flat list of byte-emission instructions that `assemble` walks to produce `Source'`.

The `CoarsenedEntry` variants are the writer's internal byte-emission language:
- `Verbatim { byte_range, orig_idx }` — copy `original_qmd[byte_range]`.
- `InlineSplice { block_text, orig_idx }` — emit pre-computed text mixing original-source prefix/suffix with newly-serialized inline content.
- `Rewrite { block_text }` — emit pre-computed text from re-serializing an AST subtree via the qmd writer.
- `Transparent { child_entries }` — inline children's bytes (wrapper contributes nothing).
- `Omit` — emit nothing.

`Rewrite` is a Layer-3 concept, not a reconciler output. The reconciler never says "rewrite anything"; the writer's coarsen translates each alignment, in context with the node's source_info, into a `CoarsenedEntry`. The current mapping (alignment + source_info → CoarsenedEntry) is the table the algebra reorganizes.

## Cumulative delta — `main` → `feature/provenance` → 7d

The coarsen step has three states of interest:

**State A — `main` (pre-Plan-7).** The incremental writer exists in skeletal form. `CoarsenedEntry` has three variants: `Verbatim`, `Rewrite`, `InlineSplice`. There is no soft-drop infrastructure; non-editable content isn't recognized as such; the writer's dispatch is essentially "try Verbatim, otherwise Rewrite the whole block." No `Transparent` (synthesized wrappers don't get descended-through), no `Omit` (no soft-drop), no editability predicate, no source-info-aware dispatch.

**State B — `feature/provenance` HEAD (Plan 7 shipped + recent fixes).** `CoarsenedEntry` has five variants: `Verbatim`, `Rewrite`, `InlineSplice`, `Transparent`, `Omit`. The soft-drop cascade exists with six cases (per `incremental-writer-contract.md` §"Soft-drop semantics"). `is_editable_inside` predicate exists. Source-info-aware dispatch exists within each alignment arm. The cascade has known structural weaknesses (`Rewrite`-as-subtree-serializer is the catch-all; per-arm predicate duplication; the lipsum-paragraph and inline-UseAfter-with-atomic-source_info gaps Plan 7c and our recent commit `e584428d` patched).

**State C — after Plan 7d.** `CoarsenedEntry` has four variants: `Verbatim`, `Omit`, `Recurse`, `Leaf`. The dispatch is a single table over `(alignment_kind, source_info_shape)`. `Rewrite` is gone (its work decomposes into `Recurse` over containers + `Leaf` over actual leaves). `Transparent` is gone (subsumed by `Recurse` with empty shells). `InlineSplice` is gone (subsumed by `Recurse` where shells come from the original block's prefix/suffix). The byte-provenance invariant is provable by induction on the dispatch table.

The net effect from State A to State C is significant but not radical: the *concept* of an incremental writer with a coarsened intermediate language is preserved from `main`; what changes is the variant set, the dispatch shape, and the soundness property. The reconciler is unchanged across all three states; only Layer 3 of the system moves.

## The proposed algebra — technical description

### Setup

The writer is a function

```
write : (Source, AST_old, AST_new, Plan) → (Source', Warnings)
```

where `Source` is the user's qmd bytes; `AST_old` is the AST produced by parsing `Source` through some pipeline tier; `AST_new` is the same AST after a structural edit applied by some upstream layer (React framework, programmatic edit, etc.); `Plan` is the reconciler's diff between `AST_old` and `AST_new`; `Source'` is the qmd bytes the writer produces, intended to round-trip through the pipeline back to `AST_new` (modulo non-meaningful whitespace).

The job is non-trivial because the pipeline `Source → AST` is non-injective. Some bytes of `Source` produce multiple AST nodes (shortcode resolution: a token of seventeen bytes produces three paragraphs). Some AST nodes have no `Source` bytes (sectionize wrapper; title-block synthesis). Writing back a mutated AST means deciding, for every byte in `Source'`, what source-side identity it has.

### Provenance

Every AST node carries a `SourceInfo` value with four physical shapes:

- `Original{file, start, end}` — bytes come from `file[start..end]`.
- `Substring{parent, start, end}` — bytes are a contiguous restriction of `parent`'s bytes.
- `Concat[pieces]` — bytes are the concatenation of `pieces`, each itself a `SourceInfo`.
- `Generated{by, from}` — bytes were synthesized by an operation tagged `by`; `from` is a list of `Anchor` values that record diagnostically-useful source positions without claiming byte-equivalence.

The derived operation `preimage_in(node, target_file_id) → Option<Range>` answers: *given a node, what contiguous byte range in `target_file_id` corresponds to its source-side identity?* The walk rules:

- `Original{f, s, e}` returns `Some(s..e)` if `f == target`, else `None`.
- `Substring{parent, s, e}` walks `parent`, restricts the returned range.
- `Concat[pieces]` returns `Some(union)` iff every piece resolves contiguously in `target`; otherwise `None`.
- `Generated{by, from}` walks `from` looking for an `Anchor` whose `role` is `Invocation`. Other roles (`ValueSource`, `Dispatch`, `Other`) are diagnostic-only and *do not* contribute to byte-traceability. This is the role-asymmetry contract (see `incremental-writer-contract.md` §"The role-asymmetry contract on `Generated.from`").

`preimage_in` is total (every node returns either `Some` or `None`) and side-effect-free.

### The user-authorable predicate

Define `editable_inside(node, target) : Bool` as `true` iff all three hold:

1. `node` is not an atomic `Custom` block or inline (its type name is not in `ATOMIC_CUSTOM_NODES`).
2. `node.source_info` is not `Generated{by, _}` with `by.is_atomic_kind()`.
3. `preimage_in(node, target).is_some()`.

A node where `editable_inside(node, target) = true` represents content the user can directly edit at the position `preimage_in` identifies. The negations matter:

- (1) Atomic `Custom` nodes (`IncludeExpansion`, `CrossrefResolvedRef`) are replaceable wholesale via UI but not editable byte-by-byte.
- (2) Atomic-kind `Generated` (shortcode, filter, title-block, tree-sitter-postprocess) are pipeline outputs whose user-side knob is the invocation token, not the resolved content.
- (3) No preimage means there are no bytes in the target file to map back to.

### The byte-provenance invariant (BP)

> **(BP)** For every byte `b` in `Source'`, exactly one of:
>
> **(P1)** `b = Source[i]` for some position `i`, where `i ∈ preimage_in(n, target)` for some AST node `n` in `AST_old`. *Read:* `b` was lifted verbatim from the user's source file, at a position the writer identified as the source-side knob for some AST node.
>
> **(P2)** `b` was produced by `serialize_leaf(n)` for some AST node `n`, where `n` has no children to recurse into and `serialize_leaf` emits only bytes derived from `n`'s own immediate content. *Read:* `b` was generated by re-serializing a single AST node that has no descendants whose serialization could also contribute bytes.

Two notes on (P1):

- (P1) does not condition on `editable_inside(n, target)`. It says only that the bytes came from some node's source-side identity. Whether the user is *currently* allowed to edit that position is a separate matter; it determines whether a warning rides along with the emission, but it does not affect whether the emission satisfies BP. Atomic-Generated soft-drop emits Verbatim of the token bytes — the bytes satisfy (P1) (they're at the Invocation anchor's range in target), and a Q-3-43 warning is emitted alongside.

- `preimage_in` only returns Some when the source bytes are recoverable as a contiguous range in target. For Generated nodes, that range is the *Invocation* token, not the resolved content. So Verbatim of a Generated node's preimage emits the token bytes — exactly what the writer wants when the user attempts to edit resolved content.

Two notes on (P2):

- The "no descendants to recurse into" clause is what makes the algebra's recursion structural. Today's writer has paths that call `write_block_to_string` on a non-leaf, and that function walks the entire subtree, emitting bytes from every node it traverses. Under BP, that path is forbidden. The only way bytes can be generated (rather than copied) is through `serialize_leaf` on a node with no children.

- The recursion that produces a container's bytes happens *outside* the serialization, in the algebra's dispatch step, which independently classifies each child via the rules below. The container's *shell* bytes (the `:::` of a Div, the `> ` of a BlockQuote, the `- ` of a list item) are also user-authorable bytes — the user could have typed them — but they are emitted by the recursion's compositional step, not by leaf serialization. We will revisit this distinction in the "Where user edits land" subsection below.

What BP rules out: every byte in `Source'` must come from either the user's existing source (P1) or a single-node leaf serialization (P2). There is no way for the writer to emit bytes derived from walking a subtree that includes atomic-Generated descendants the user can't author. The unsoundness today's writer has — that the catch-all `Rewrite` path can serialize a subtree whose descendants haven't been individually classified — is structurally absent under BP.

What BP does not promise: position correctness (whether the bytes land at the right place in `Source'`; that's `assemble`'s job), warning fidelity (whether the right diagnostics are emitted; that's the diagnostic layer's job, on top of BP), and producer-side hygiene (whether AST leaves with no provenance are actually user-authored; that's the producer contract, on which BP relies as a narrow trust point — see "Open design judgment" below).

### The coarsened-tree algebra

The algebra reorganizes the writer's output language. The proposed `CoarsenedEntry` shape:

```rust
enum CoarsenedEntry {
    Verbatim {
        byte_range: Range<usize>,
        orig_idx: Option<usize>,  // separator hint, never byte-production context
    },
    Omit {
        warning: Option<Diagnostic>,
    },
    Recurse {
        shell_open: Bytes,
        children: Vec<CoarsenedEntry>,
        shell_close: Bytes,
        separator: SeparatorRule,
    },
    Leaf {
        block_text: Bytes,
    },
}
```

This unifies today's five variants (`Verbatim`, `Omit`, `Transparent`, `InlineSplice`, `Rewrite`) into four. The renames and consolidations:

- Today's `Transparent { child_entries }` becomes the special case `Recurse { shell_open: "", shell_close: "", children, separator }`. The wrapper contributes no bytes; only the children's compositions do. Sectionize Div, footnotes container, appendix container all use this shape today and continue to under the algebra.

- Today's `InlineSplice { block_text, orig_idx }` becomes a more general case: `Recurse { shell_open, children, shell_close, separator }` where `shell_open` and `shell_close` are the *original-qmd prefix and suffix bytes* of the block being spliced, and `children` are the inline children. The pre-computed `block_text` field is derived: `block_text = shell_open + assemble(children) + shell_close`. Today's `InlineSplice` is essentially "the container's wrapper is the same as the original; only the inside changed" — exactly what `Recurse` models when `shell_open` and `shell_close` come from `Source`.

- Today's `Rewrite { block_text }` does *not* survive in its current form. Its substance — "serialize this subtree as text" — is decomposed. For container blocks, that work becomes a `Recurse` whose `shell_open` and `shell_close` come from the qmd writer's container-shell helpers and whose `children` come from the algebra's recursion. For leaf blocks, that work becomes a `Leaf` whose `block_text` is `serialize_leaf(node)` — `serialize_leaf` being `write_block_to_string` restricted to nodes with no recursable descendants.

`assemble : CoarsenedEntry → Bytes` is the fold: `Verbatim` returns `Source[byte_range]`; `Omit` returns `""`; `Recurse` returns `shell_open ++ join(separator, [assemble(c) for c in children]) ++ shell_close`; `Leaf` returns `block_text`.

### The dispatch table

`coarsen : (Node, target, align_ctx) → CoarsenedEntry`. Total recursive function over the AST, dispatched on the pair `(align_ctx.alignment_kind, node.source_info_shape)`. The table:

| Alignment | Source-info / structure | Rule | Operation |
|---|---|---|---|
| `KeepBefore(i)` | preimage in target | R1 | `Verbatim(preimage)` |
| `KeepBefore(i)` | atomic-kind Generated, no preimage | R2 | `Omit` (no warning; content regenerates from baseline) |
| `KeepBefore(i)` | non-atomic, no preimage, container with source-bearing children | R3 (Transparent-form) | `Recurse{ "", children-coarsened, "", separator }` |
| `KeepBefore(i)` | non-atomic, no preimage, no recursable children | R5 | `Leaf{ serialize_leaf(node) }` (rare; cross-file-rooted leaf, etc.) |
| `UseAfter(j)` | atomic-kind Generated with preimage | R1' (soft-drop) | `Verbatim(preimage)` + Q-3-43 |
| `UseAfter(j)` | atomic-kind Generated, no preimage | R2' (soft-drop) | `Omit` + Q-3-43 |
| `UseAfter(j)` | atomic `Custom` | R5-special (let-user-win) | `Leaf{ serialize_leaf(node) }` via `plain_data`; no warning |
| `UseAfter(j)` | non-atomic, no preimage, container | R3 | `Recurse{ shell_open, children, shell_close, separator }` where shells come from the qmd writer's per-container syntax helpers |
| `UseAfter(j)` | non-atomic, no preimage, leaf | R5 | `Leaf{ serialize_leaf(node) }` |
| `UseAfter(j)` | non-atomic with preimage | R1 | `Verbatim(preimage)` (paste-from-elsewhere case; trust the source_info producer marked) |
| `RecurseIntoContainer{ before, after }` | non-editable inside | R1' / R2' (soft-drop) | `Verbatim(preimage)` + Q-3-43 (if preimage exists), or `Omit` + Q-3-43 (no preimage). Recursion stops here. |
| `RecurseIntoContainer{ before, after }` | editable inside, block container | R3 | `Recurse{ shell_open, children-coarsened-per-`block_container_plans`, shell_close, separator }` |
| `RecurseIntoContainer{ before, after }` | editable inside, inline container | R4 | `Recurse{ shell_open-from-original-prefix, inlines-coarsened-per-`inline_plans`, shell_close-from-original-suffix, separator }` — the inline-splice case generalized |

The dispatch is total: every `(align, source_info)` pair matches exactly one row. R3 and R4 are structurally the same operation (recurse with shells); they're listed separately because R3 dispatches on `block_container_plans` while R4 dispatches on `inline_plans`, and the shell sources differ (R4 takes shells from the *original* block's source bytes for the inline-splice case; R3 takes shells from the new container's syntax helpers).

The rule R1' / R2' on `RecurseIntoContainer` for non-editable nodes is the soft-drop substitution: even though the reconciler said "recurse into this container," the writer overrides the recursion because the container itself isn't editable. The recursion would emit user-side bytes the user can't actually author; the substitution emits the wrapper's preimage instead. This is what the existing soft-drop cascade does today; the algebra preserves the behavior under R1' / R2'.

### Soundness sketch

**Claim.** For any well-formed input `(Source, AST_old, AST_new, Plan)` and any node `n` in `AST_new` with alignment context `align`, `assemble(coarsen(n, target, align))` produces bytes that satisfy BP.

**Proof.** By structural induction on `n`.

- **R1 base case.** Emits `Source[range]` where `range = preimage_in(node, target)`. `Source[range]` is by definition bytes in `Source` at a position `range` is the preimage of an AST node. (P1) holds for every byte. The atomicity of the node doesn't enter; (P1) doesn't require editability.

- **R2 base case.** Emits no bytes. Vacuously satisfies BP.

- **R5 base case.** Emits `serialize_leaf(node)` where `node` is a leaf (no children to recurse into). `serialize_leaf` emits bytes derived from `node`'s own immediate content; by R5's precondition (no recursable descendants), the "no descendants whose serialization would contribute" clause of (P2) is vacuously true. (P2) holds. The trust point: we trust `node`'s immediate content represents user-authored bytes; if a producer creates such a leaf without it being user content, the trust is misplaced. This is the producer-side contract's job, narrowed by the algebra to leaves only.

- **R3 / R4 inductive case.** Emits `shell_open ++ join(separator, [assemble(c) for c in children]) ++ shell_close`. By inductive hypothesis, each `assemble(c)` satisfies BP. Concatenation preserves BP per-byte (each byte still satisfies (P1) or (P2)). `shell_open` and `shell_close` are user-authorable syntax for the container kind — for R3 with new containers, they come from the qmd writer's syntax helpers (e.g., `:::{.foo}\n` for a Div, `> ` per line for a BlockQuote); for R4, they come from `Source[original_prefix_range]` and `Source[original_suffix_range]` of the original block. In the R3 case, the shell bytes satisfy (P2) (they're emitted by a structural leaf operation — the syntax helper — that has no descendants of its own to walk). In the R4 case, the shell bytes satisfy (P1) (they're copied from `Source` at the original block's range). Separator bytes are user-authorable whitespace; treated like shell bytes.

QED, informally.

### Properties enforced

The algebra implies the following properties as theorems (some require Phase 0 design judgment to fully nail down; flagged below):

1. **(BP) Byte-provenance soundness.** For every byte of `Source'`, (P1) or (P2) holds. Proven by the structural induction above.

2. **Totality of dispatch.** Every `(node, align)` pair matches exactly one row in the table; the writer is a total function on well-formed inputs.

3. **Compositionality.** `coarsen(container) = Recurse(shells, [coarsen(child)])`. The writer's behavior on a subtree is a function of its behavior on the components. This is what makes inductive reasoning over the AST possible.

4. **Source-info-driven dispatch within alignment kind.** Given a fixed alignment kind, the rule that fires depends only on `node.source_info` and structural shape. No ambient context. No per-arm duplication of predicates.

5. **Leaf-only serialization.** `serialize_leaf` is invoked only on nodes that the algebra classifies as R5 leaves. Subtree serialization is structurally absent — there is no path through the algebra that calls a serialization function on a non-leaf AST node without first recursing into its children via R3 / R4.

6. **Termination.** `coarsen` recurses only on strictly smaller substructures (children). The AST is finite. Termination is by structural induction on AST size.

7. **Diagnostic determinism.** The set of warnings produced is a function of the input ASTs alone — the warning a `(alignment, source_info)` cell emits is fixed by the table; no order-dependence, no cascade-arm-dependence.

8. **Reconciler-independence of rule choice.** The reconciler's `Plan` informs *which* node is coarsened at each position and *what alignment context* applies, but the rule selection within a row is determined by source_info and structural shape alone. New reconciler outputs (a hypothetical fourth alignment kind, a new sub-plan type) would require a new row block in the table but would not require changes to existing rows.

### What the refactor concretely changes (starting from `feature/provenance` HEAD)

The architectural delta from the implementation starting point is concentrated in three places:

**1. `write_block_to_string` decomposes into two functions.** Today, `write_block_to_string(block) → text` walks the block's entire subtree and emits text via the qmd writer's per-container arms. Under the algebra, it splits:

- `serialize_block_shell_open(block) → Bytes` and `serialize_block_shell_close(block) → Bytes` emit the wrapper bytes of a container — the open and close syntax — without consulting the container's children. For Div: `:::{.foo}\n` and `:::\n`. For BlockQuote: per-line `> ` prefix (modelled as a SeparatorRule that prefixes each child's lines). For NoteDefinitionFencedBlock: the fenced-block syntax. For list shapes (`BulletList`, `OrderedList`): the per-item marker (a per-item shell within a list-Recurse).

- `serialize_leaf(node) → Bytes` is `write_block_to_string` restricted to leaves. It is structurally guaranteed not to recurse: the function panics (or is enforced via the type system if we want to be strict) if invoked on a non-leaf node.

The qmd writer's per-container arms are refactored to expose this decomposition. The unified-pass version (`write_block_to_string` as it exists today) becomes a derived convenience function that the rest of the codebase can still call for native rendering — it just isn't used by the incremental writer's coarsen step anymore.

**2. `CoarsenedEntry::Rewrite` ceases to be a subtree operation.** The variant `Leaf { block_text }` replaces it for genuine leaves. The cases today's `Rewrite` covered:

- `coarsen_keep_before_block` catch-all (cross-file Original, gappy Concat, no-preimage Generated without source-bearing children) → becomes either R3 (for containers — the catch-all recurses into children) or R5 (for leaves — emit the leaf's serialization).
- `coarsen_blocks::UseAfter` let-user-win on non-atomic, no-preimage → R3 for containers, R5 for leaves.
- `coarsen_blocks::RecurseIntoContainer` with inline_plan but not splice-safe → R4 with shells from the *new* block's syntax (since the splice-safety failure means we can't preserve the original's shell verbatim across the rewrite).
- `coarsen_blocks::RecurseIntoContainer` no inline_plan, block-children case → R3 with shells from the new container's syntax.

**3. `coarsen_blocks` dispatch becomes source-info-aware uniformly.** Today's `coarsen_blocks` matches on `BlockAlignment` first, then dispatches on source_info within each arm. The same predicates (atomic-Generated check, preimage check, editability check) appear in two or three arms with slightly different surrounding logic in each. Under the algebra, the dispatch flips: for each block we intend to emit, compute `(alignment_kind, source_info_shape)`, look up the row, apply the rule. The per-arm duplication of the soft-drop cascade disappears.

The inline cascade in `assemble_inline_content` undergoes the analogous restructuring. Today's two-phase shape (Phase 1: substitute safe alignments; Phase 2: emit with multi-inline dedupe) becomes a single recursive coarsen over the inline-level R1–R5 table. The multi-inline dedupe optimization (`compute_separator` checking that consecutive `KeepBefore` entries share an `Invocation` anchor and emitting their shared token once) is preserved as a separator rule.

`compute_separator` itself becomes a method on the new `SeparatorRule` value carried by `Recurse`. The "consecutive-in-original" optimization (today's `orig_idx: Option<usize>` on `Verbatim` and `InlineSplice`) survives as a separator-rule variant; the indices remain `Option`-typed because children inside a `Recurse` don't have top-level positional identity.

### Where user edits land

A practical clarification, because the discussion that led to this plan kept conflating "where bytes get serialized" with "where user edits land in the output." They are different questions.

The algebra has three base cases that produce bytes:

- **R1 (Verbatim).** Emits bytes from `Source`. These bytes were authored by the user *at the position they came from*. R1 fires for unchanged content (KeepBefore on a node with preimage) AND for atomic-content soft-drop (UseAfter or RecurseIntoContainer on a non-editable node with preimage — substitute the preimage as the safe alternative). Same emission operation, different alignment contexts.

- **R3 / R4 shell bytes.** Emitted by `Recurse`'s shell-emission step. These bytes are the *syntax* of a container — the `:::` of a Div, the `> ` of a BlockQuote, the `- ` of a list item, the `:::{.callout-note}` of a callout. The bytes are user-authorable because the user could have typed them directly in qmd. R3's shells come from the qmd writer's syntax helpers when the container is newly constructed; R4's shells come from `Source` when the container is being inline-spliced (preserving the original block's wrapping bytes).

- **R5 (Leaf serialization).** Emits bytes from `serialize_leaf(node)` — the leaf node's own content rendered as text. `Str("hello")` becomes `hello`. `Code` block emits its code-fence syntax plus content (treated as a leaf because its content is bytes, not children needing recursion). Atomic `Custom` (via let-user-win) emits its qmd syntax derived from `plain_data`.

User edits land at all three:

| Kind of edit | Rule(s) that produce bytes for the edit | Example |
|---|---|---|
| User reorders / wraps / moves existing content | R1 (copies preserved bytes from `Source` at original positions) + R3/R4 shells (for new containers wrapping the moved content) | Wrap three paragraphs in a blockquote: R1 copies the paragraph bytes, R3 emits `> ` prefixes. |
| User constructs a new structural parent | R3 / R4 (shells of the new container) + recursion through children | Add a new list item: R3 emits the list's per-item iteration, the new item's R3 emits `- `, the item's children fire their own rules. |
| User types new leaf content | R5 (serialize the new leaf) | Type a word in a Para: R5 emits the new `Str`'s text. |
| User replaces atomic Custom via component picker | R5-special (let-user-win on atomic Custom) | Pick a different include source: R5 emits the new `{{< include … >}}` syntax derived from `plain_data`. |
| User attempts to edit atomic-Generated content | R1' soft-drop or R2' soft-drop (emit preimage + warning, OR omit + warning) | Type into a lipsum-resolved paragraph: R1' emits the `{{< lipsum 3 >}}` token, Q-3-43 warns. |

A single user edit typically produces bytes from *multiple* rules in combination. The algebra's recursion walks down the new AST shape, choosing the right rule at each level based on `(alignment, source_info)`. The structural property is: every byte produced satisfies BP, regardless of which rule produced it.

The reason R5 *appears* central in the soundness story: R5 is the only rule that emits bytes by *serializing AST content*, so it's where the algebra's "trust point" sits (the residual assumption that a non-atomic leaf with no preimage represents user-authored content). R3/R4's shell bytes are emitted by syntax helpers that don't carry trust (they emit fixed syntax based on the container kind). R1's bytes are copied from `Source` (trust derives from `Source` being the user's file). So when proving BP, R5 is the place to worry. When asking "where does user-typed content land in `Source'`," the answer is "R3 + R4 + R5 in combination, distributed across the recursion."

### Open design judgment

Each item below is a decision Phase 0 resolves before Phases 1-6 proceed. They are not implementation-prescriptive; they're design questions whose answer constrains the refactor's shape.

**1. R5's trust point.** Today's writer trusts `Original`-source_info content's preimage. The algebra inherits that trust. It also adds a trust point at R5: nodes that reach R5 are assumed user-authored even if their source_info doesn't strictly say so (e.g., `SourceInfo::default()` on a freshly-React-typed leaf). The producer contract (`provenance-contract.md`) is the safeguard. Two possible tightenings:

   (a) **Permissive** (what the table above proposes): any leaf with `source_info` that isn't atomic-Generated reaches R5. Trust the producer to mark synthesized leaves correctly.

   (b) **Strict**: R5 fires only on leaves whose source_info is `Original`-rooted-in-target OR `Generated{by: user_edit, _}` OR equivalent explicit user-content markers. Anything else (including `SourceInfo::default()`) becomes R2' (Omit + warning) or R5'-with-warning. Tightens the trust surface but requires the React framework to attach explicit user-content source_info on edits.

   Phase 0 picks (a) or (b). The tradeoff is producer hygiene burden vs. residual writer trust.

**2. Custom node treatment.** Today, `CoarsenedEntry::Rewrite` on a non-atomic CustomNode (like Callout) serializes the whole node via the qmd writer's CustomNode arm, which reads `plain_data` and walks the slot contents. Under the algebra, this needs to decompose:

   (a) The CustomNode's "shell" is its `plain_data`-derived open and close syntax (e.g., `:::{.callout-note}\n` and `:::\n`).

   (b) The slot contents are coarsened independently via the `custom_node_plans` side-table.

   This requires the qmd writer's CustomNode arm to expose `serialize_custom_shell_open(plain_data)` and `serialize_custom_shell_close(plain_data)`. Mechanically straightforward; needs a per-Custom-type sweep.

**3. List shapes.** `BulletList` / `OrderedList` carry `Vec<Vec<Block>>` — a list of items, each item itself a Vec<Block>. The per-item marker (`- `, `1. `, etc.) is per-item-syntactic, not per-list. Two modelling choices:

   (a) `Recurse` carries a `SeparatorRule::ListItem { marker_fn }` that emits the marker before each child's bytes. The list itself is `Recurse { shell_open: "", children, shell_close: "", separator: ListItem }`. Each item is itself a `Recurse` over its blocks.

   (b) List items become a separate `CoarsenedEntry` variant (`ListItem { marker, content }`) the recursion handles specially.

   (a) is more uniform; (b) is more explicit. Phase 0 picks.

**4. Separator state threading.** Today's `compute_separator` looks at adjacent `Verbatim`/`InlineSplice` entries' `orig_idx` values to decide blank-line vs. newline-only spacing. Under the algebra, separators are per-`Recurse`-level. The right model is probably: `SeparatorRule` carries enough information to reproduce today's decisions *given the local children*, without consulting global state. The exact rule needs to be written out in Phase 0's design pass.

**5. Inline-cascade alignment with Plan 7c.** Plan 7c Phase 7's `displaced_before_idx` enrichment of `InlineAlignment::UseAfter` becomes *defense-in-depth* under the algebra rather than load-bearing: the algebra dispatches on the *new* inline's source_info, and a UseAfter on atomic-Generated-with-preimage fires R1' regardless of whether the displaced original is tracked. But if React strips source_info during edits (replaces an atomic inline with a fresh inline carrying `SourceInfo::default()`), R5 fires on the fresh leaf and the atomic content is overwritten. Phase 7's reconciler-side tracking is the second line of defense against that producer-side failure mode. Phase 0 decides whether to retain Phase 7 (defense-in-depth) or drop it (trust the producer contract).

**6. Cost.** Today's `write_block_to_string` is a single function call that walks a subtree once. The algebra's R3/R4 recursion through every container layer is potentially O(layers × per-layer-work) more invocations. In practice R1 (Verbatim) short-circuits most unchanged subtrees, so the actual cost likely matches today's. Worth measuring before committing; Phase 4's property tests provide a natural benchmarking harness.

---

## Phases

### Phase 0 — Validate the algebra

The reader (user) confirms:

- [ ] The byte-provenance invariant (P1 + P2) is the right invariant for the writer.
- [ ] The dispatch table covers all `(alignment, source_info)` pairs the system today produces.
- [ ] The decomposition of `Rewrite` into `Recurse` + `Leaf` matches the desired structure.
- [ ] R5's trust point is acceptable as stated (or is tightened per "Open design judgment" #1).
- [ ] Custom-node treatment, list-shape treatment, and separator-state threading land at the decisions made in "Open design judgment" #2–#4.
- [ ] The relationship to Plan 7c Phases 7 / 7b (defense-in-depth vs drop) is settled per "Open design judgment" #5.
- [ ] The scope of changes (refactor `write_block_to_string`; restructure `coarsen_blocks` and `assemble_inline_content`; retire `Rewrite`) is acceptable.

Open questions raised here. No code changes happen until Phase 0 closes.

### Phase 1 — Decompose `write_block_to_string`

- [ ] Identify every per-container arm in `crates/pampa/src/writers/qmd.rs` that produces output for a container block (Div, BlockQuote, Figure, NoteDefinitionFencedBlock, OrderedList, BulletList, DefinitionList, Table, Custom block).
- [ ] For each, extract `serialize_block_shell_open(block) → Bytes` and `serialize_block_shell_close(block) → Bytes`. Move the children-emitting code out of the arm; the arm emits only the wrapper.
- [ ] Do the same for inline containers (Emph, Strong, Link, Image, Span, Cite, Note, …): `serialize_inline_shell_open(inline) → Bytes` and `serialize_inline_shell_close(inline) → Bytes`.
- [ ] Define `serialize_leaf(node) → Bytes` as `write_block_to_string` restricted to leaves. Type-enforce or runtime-assert that the function panics on non-leaf input.
- [ ] Preserve `write_block_to_string` as a public convenience function that the rest of the codebase (native rendering, snapshot tests, etc.) can call. Its implementation becomes `shell_open + assemble(children-coarsened) + shell_close` — but the incremental writer no longer calls it.
- [ ] Tests: each shell-helper has a unit test that asserts its output for a known node.

### Phase 2 — Restructure `coarsen_blocks` dispatch

- [ ] Define the new `CoarsenedEntry` shape with `Verbatim`, `Omit`, `Recurse`, `Leaf` variants. Delete `Transparent`, `InlineSplice`, `Rewrite` (their roles are absorbed).
- [ ] Implement the dispatch table from "The proposed algebra" as a single `dispatch(node, align, target) → Rule` function. Each rule has a small implementation: R1 packages `Verbatim`; R2 packages `Omit`; R3 / R4 package `Recurse` with shells from Phase 1's helpers and children recursed via `coarsen`; R5 packages `Leaf { serialize_leaf(node) }`.
- [ ] `coarsen_blocks` becomes a thin wrapper that iterates the `block_alignments`, calls `dispatch` for each, threads separator context.
- [ ] Delete `coarsen_keep_before_block` (its logic is absorbed into the dispatch table).
- [ ] Verify against today's regression tests: every existing test in `crates/pampa/tests/incremental_writer_tests.rs` must still pass byte-for-byte. The refactor doesn't change observable behavior on the inputs the tests cover.

### Phase 3 — Restructure `assemble_inline_content`

- [ ] Define inline `Rule` dispatch analogous to block-level. R1-inline, R2-inline, R3-inline (inline `Recurse` for nested inline containers), R5-inline (leaf inline).
- [ ] `assemble_inline_content` becomes a recursive coarsen over the inline cascade. Phase 1's two-phase shape (soft-drop substitution + emit-with-dedupe) collapses to a single pass.
- [ ] Multi-inline dedupe (today's `compute_separator` shared-`Invocation`-anchor optimization) becomes a `SeparatorRule::InlineDedupe` carried by the parent `Recurse`.
- [ ] Verify: every existing inline-cascade test passes byte-for-byte.

### Phase 4 — Property tests for BP

The algebra is sound by construction, but a property test pins the invariant against bugs in the implementation.

- [ ] Write a proptest generator `gen_pandoc_with_atomic_descendants` that produces ASTs with atomic-Generated descendants at varying depths inside non-atomic containers, plus arbitrary user edits applied.
- [ ] Write the property `bp_holds`: given a generated `(AST_old, AST_new, Source)` and a reconciler plan, run the writer. Assert: the output `Source'` does not contain any of the resolved bytes of atomic-Generated descendants. (Implementation: tag the generator's atomic-resolved content with a recognizable marker string; assert the marker doesn't appear in `Source'`.)
- [ ] Add property tests for individual rule soundness: R1 emits bytes from `Source`; R5 emits bytes derived only from the leaf's own content; R3 / R4 emit bytes that are concatenations of shell + children.
- [ ] Run under `cargo nextest run -p pampa` with high iteration counts. Save regression seeds if any fail.

### Phase 5 — Retire denylist branches obviated by the algebra

- [ ] Audit Plan 7c's open phases. For each phase that becomes defense-in-depth under the algebra (Phase 7's `displaced_before_idx`, Phase 7b's inline atomic-Generated check), decide per Phase 0's "Open design judgment" #5 whether to retain or drop.
- [ ] Remove obsolete branches from the codebase. Update tests to match.

### Phase 6 — Update design docs

- [ ] `claude-notes/designs/incremental-writer-contract.md`: the six-case soft-drop enumeration in §"Soft-drop semantics" is replaced by a pointer to this plan's dispatch table. The §"Non-soft-drop branches in the same cascade" sub-section is rewritten to reflect the algebra's uniform handling rather than the present-day cascade asymmetry. The §"`CoarsenedEntry` self-containment" sub-section is updated to reflect the new variant set (`Verbatim`, `Omit`, `Recurse`, `Leaf`).
- [ ] Add a §"Algebraic soundness" section to the contract doc that states BP, the dispatch table, and the soundness sketch.
- [ ] Cross-link from `provenance-contract.md` §7 (atomic-kind set and consumer impact).
- [ ] Add a "Follow-ups closed" entry to Plan 7 pointing here, retiring the algebraic-soundness item from its open tail.

## What 7d does not change

Explicit non-changes, for clarity:

- **The reconciler's algorithm.** `compute_reconciliation` and its helpers stay as they are. Three-phase pass; same hash-match / positional / fallback logic.
- **`BlockAlignment` / `InlineAlignment` / `ListItemAlignment` types.** Same variants. No payload changes.
- **`apply_reconciliation` (AST-level reconciliation).** Independent of the writer; not touched.
- **`ReconciliationPlan` shape.** All sub-plan tables (`block_container_plans`, `inline_plans`, etc.) stay.
- **The wire format.** The plan is computed inside WASM and never crosses the boundary as JSON; nothing in `ts-packages/quarto-sync-client/src/types.ts` changes.
- **The diagnostic catalog.** Q-3-41, Q-3-42, Q-3-43 stay. The algebra reorganizes which dispatch row emits which code; the codes themselves don't change.
- **The producer-side contract (`provenance-contract.md`).** The role-asymmetry rule, the `By::` catalog, the atomic-kind set — all stay. The algebra inherits these as preconditions on its input.

## Relationship to siblings

- **Plan 7** (shipped): provides the existing writer the algebra refactors. 7d's implementation phases (1-6) start from `feature/provenance` HEAD.

- **Plan 7a** (open): runtime user-filter idempotence detection. Orthogonal — concerns the validity of *inputs* to the writer (whether filters break round-trip), not the writer itself. 7a's work is independent of 7d.

- **Plan 7b** (open): test-coverage consolidation. The property tests in 7d Phase 4 *complement* Plan 7b's per-shape regression tests. 7d's properties are coarser (input-distribution-driven; assert structural properties of output); 7b's are finer (specific shapes, specific assertions). Both are useful; neither obviates the other.

- **Plan 7c** (open): closure gaps in the denylist cascade.
  - Phases 1-6 of 7c remain useful regardless of 7d. They address concrete present-day bugs (Q-3-41 catalog gap, TS-side gate, per-kind soft-drop coverage, `Q343Reason` typing, `target_file_id` descent).
  - Phases 7 and 7b of 7c become defense-in-depth under 7d (the algebra catches the cases they protect against, *provided* the producer contract is satisfied). Phase 0's design judgment #5 decides whether to retain them.
  - 7c can ship before 7d, after 7d, or in parallel. The two plans are independent in scope; only the defense-in-depth question links them.

## Risks

- **Refactor scope.** The decomposition of `write_block_to_string` touches every per-container arm in the qmd writer. Each arm is small but there are many of them (Div, BlockQuote, OrderedList, BulletList, DefinitionList, Figure, NoteDefinitionFencedBlock, Table, Custom block, Header, Paragraph, Plain, …). Estimating 500-1000 LOC of mechanical refactor work, plus 200-400 LOC of dispatch-table consolidation.

- **Behavioral compatibility.** Every existing test must pass byte-for-byte after the refactor. The algebra is designed to preserve behavior on today's inputs; any deviation is a refactor bug. Phase 4's property tests guard against regressions on inputs today's tests don't cover.

- **Cost.** Recursive emission may be slower than today's single-pass `write_block_to_string`. Phase 4 includes benchmarking; if cost regresses significantly, Phase 0 may need to revisit (e.g., add memoization, or keep `write_block_to_string` as an optimized path for trees the algebra has already verified safe).

- **Producer-contract drift.** The algebra leans on producer hygiene at R5's trust point. If a producer (a new transform, a Lua filter, a future synthesizer) introduces a leaf with non-default source_info that doesn't fit the algebra's classifications, R5 may emit bytes the algebra trusts but shouldn't. The mitigation is the producer contract's pre-existing rule ("new kinds default to non-atomic; promote deliberately") combined with the property tests catching obvious violations.

- **The CustomNode decomposition.** Phase 1's split of CustomNode arms into shell helpers may surface CustomNode types that resist the decomposition (e.g., where the open syntax depends on the slot content, or where serialization isn't naturally separable into shell + children). These are spot-fixable but may add work.

## References

- This plan's algebraic content was developed across the 2026-05-25 / 2026-05-26 sessions on `feature/provenance` after the lipsum-paragraph regression (commit `e584428d`) prompted reconsideration of the writer's structural soundness.
- Today's writer: `crates/pampa/src/writers/incremental.rs` (~2700 LOC; `coarsen_blocks`, `coarsen_keep_before_block`, `assemble`, `assemble_inline_content`, `write_block_to_string`).
- Reconciler: `crates/quarto-ast-reconcile/src/compute.rs` (algorithm), `src/types.rs` (alignment types), `src/apply.rs` (AST-level apply, not used by the writer).
- Contract doc: [`claude-notes/designs/incremental-writer-contract.md`](../designs/incremental-writer-contract.md) — the byte-provenance contract this plan makes provable.
- Producer-side contract: [`claude-notes/designs/provenance-contract.md`](../designs/provenance-contract.md) — the rules producers must satisfy for the algebra's trust points to hold.
- Sibling primitive: [`claude-notes/designs/transparent-wrappers.md`](../designs/transparent-wrappers.md) — the traversal-side analogue (`first_in_user_tree`) of the writer's emission-side recursion.
- Plan 7 (shipped): [`claude-notes/plans/2026-05-04-q2-preview-plan-7-incremental-writer.md`](./2026-05-04-q2-preview-plan-7-incremental-writer.md) — the writer the algebra refactors.
- Plan 7c (open): [`claude-notes/plans/2026-05-25-q2-preview-plan-7c-closure-gaps.md`](./2026-05-25-q2-preview-plan-7c-closure-gaps.md) — the denylist-tightening sibling plan.
