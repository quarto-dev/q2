# Plan 7d — Algebraic soundness of plan_user_writes / incremental-writer

**Date:** 2026-05-26 (revised 2026-05-29; 2026-06-03; 2026-06-03 source-verification pass — reference fixes #1–4, three-way leaf/container classifier, R4 fallback rule, `LineAware` filter-nesting semantics, R3/R4 table-exclusivity + Property #9 range-equality invariants stated, plus baseline/calibration/CustomNode-inventory/stale-comment checklist items; follow-up: Custom-precedence caveat on the classifier, explicit inline-content dispatch rows + shared `recurse_inline_content_with_helper_shells` primitive)
**Branch:** feature/provenance (ships after 7f and 7g; before 7e and 7c)
**Status:** Implementation-ready. Phase 0 closed 2026-05-29. Design content moved to [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md). This plan is the implementation roadmap.
**Milestone:** none directly. Pre-condition for any future "minimal-edit diffing" work that would consume the user-write plan to derive per-region Monaco edits rather than full-document saves.

## Epic context

Fourth sibling follow-up to Plan 7 in the provenance epic:

| Sibling | Axis | Status |
|---|---|---|
| Plan 7 | Incremental writer + soft-drop + bridge migration | shipped on `feature/provenance` |
| Plan 7a | Runtime user-filter idempotence (input-side validation) | open |
| Plan 7b | Test-coverage consolidation | open |
| Plan 7c | Closure gaps in the existing soft-drop cascade | open (Phases 7 and 7b **obsoleted** by 7d — see below) |
| Plan 7d | Algebraic soundness of the coarsen/write step | **this plan** |
| Plan 7e | CustomNode qmd serialization | sibling, ships after 7d |
| Plan 7f | Prerequisites for 7d (framework + test hygiene + wire-format) | sibling, ships before 7d |
| Plan 7g | Source-range tiling (producer precondition **P4**) | **complete** on `feature/provenance`; prerequisite for 7d's soundness proof |

7d differs from 7c in *disposition*. Plan 7c tightens the existing denylist cascade — each phase adds a branch the cascade should have caught but didn't, or repairs a per-arm predicate that drifts from accuracy. Plan 7d replaces the cascade with an allowlist algebra: every emission is allowed by construction rather than by the absence of a denylist match. **7d ships before 7c**, and the algebra obsoletes 7c's Phases 7 and 7b outright: the inline `UseAfter` dispatch keys on the *new* node's own `source_info` (mirroring the block-level `e584428d` fix), so it never makes the original-side lookup that 7c Phase 7's `displaced_before_idx` was meant to make precise, and R1' already does what 7c Phase 7b's new-side atomicity check does. Those two 7c phases are now marked obsolete in `2026-05-25-q2-preview-plan-7c-closure-gaps.md`; the remaining 7c phases (Q-3-41, TS gate parity, per-kind tests, `Q343Reason`) are orthogonal and unaffected.

The implementation starts from the current HEAD of `feature/provenance` after **7f and 7g** land, so the framework's source_info preservation and user-edit stamping are in place (7f), the wire format renames are done (7f), the `SourceInfo::default()` test-audit has bottomed out (7f), and the **source-range tiling precondition P4** holds (7g — sibling preimages disjoint, parents contain children; the BP/completeness proof depends on it). The `CoarsenedEntry` self-containment property established by commit `e584428d` — every variant produces its emit bytes from its own payload without ambient context — is a precondition for the algebra to compose correctly. Plan 7d is the next step on top.

## Goal

Replace the writer's flat per-arm cascade with a recursive structural dispatch — `plan_user_writes`, the rename of `coarsen` — whose inductive soundness argument discharges BP without per-arm checking. The full statement of BP, the two contracts that make it hold, and the proof of soundness live in [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md). This plan does not restate them; it specifies the implementation.

This is a refactor of one layer of the system. The reconciler, the AST types, `apply_reconciliation`, and the diagnostic catalog are not touched.

## Phase 0 — Validate the algebra (closed 2026-05-29)

All seven validation items resolved through design review with Gordon. Outcomes:

- (1) **R5 trust point: strict.** Single `Generated{by: user_edit, …}` shape. Enforcement is client-side in the React framework (see Plan 7f Phase 3). No empty-preimage tolerance in 7d; 7f ships first to make the framework honest, and 7d trusts the producer contract.
- (2) **CustomNode treatment.** Decomposed shell helpers are correct; current `Block::Custom` arms in the qmd writer are empty placeholders. Split to Plan 7e as a separate scope.
- (3) **List shape.** `ShellOpen` becomes an enum: `Bytes(Bytes)` for fixed-prefix shells (Div, etc.) and `LineAware { marker, continuation_indent }` for per-line markers (lists, BlockQuote when modeled this way). The marker emits once per item; continuation lines get the indent via a `Write` adapter analogous to today's `BulletListContext`.
- (4) **Separator state.** `SeparatorRule` per `Recurse` (variants: `StandardBlock { tight }`, `InlineConcat`, `ListItem { loose }`, `OriginalGap`). One cross-`Recurse` state, `TrailingState` (four variants: `None`, `EndsWithText`, `EndsWithNewline`, `EndsWithBlankLine`), threaded through `assemble` as a function parameter.
- (5) **Plan 7c relationship.** 7d ships **before** 7c and **obsoletes** 7c's Phases 7 (`displaced_before_idx`) and 7b (inline new-side atomicity check): the allowlist dispatch keys on the new node's own `source_info`, so it neither makes the original-side lookup Phase 7 sharpened nor lacks the new-side check Phase 7b added (R1' is that check). Those two phases are marked obsolete in 7c. The other 7c phases are orthogonal. (Revised 2026-06-03 — earlier wording called them "defense-in-depth"; under the allowlist algebra they are simply redundant, not a second line of defense.)
- (6) **Cost.** Phase 4 adds end-to-end benchmarking on a synthetic 500-block fixture with realistic content (sectionize wrappers, shortcodes, callouts). Both old `coarsen` and new `plan_user_writes` are O(n) on subtree size; the bench is a sanity check, not a hopeful negotiation.

Property #9 added: **block-level Invocation coalescing.** Within any `Recurse`, a maximal run of consecutive children whose `preimage_in(target)` returns the same `Some(range)` collapses to a single `Verbatim` of that range. Block-level analogue of today's multi-inline dedupe.

Phases 1–6 below proceed against `feature/provenance` HEAD after 7f **and 7g** have landed (7g is complete).

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

**Separator / coalescing / trailing-state composition.** These three mechanisms are deliberately staged so they never interleave:

- **Property #9 coalescing is a *planner* step** — `plan_user_writes` collapses a maximal run of consecutive children sharing one `preimage_in(target)` into a single `Verbatim` *before* `assemble` runs. So `assemble` never computes a separator *inside* a coalesced run; coalescing and separation are sequential, not entangled.
- **`SeparatorRule::OriginalGap` is an `assemble` per-adjacent-pair decision.** When two adjacent emitted entries both expose target preimages `r_prev`, `r_curr` with `r_prev.end <= r_curr.start` and a same-container-consecutive origin, emit `Source[r_prev.end .. r_curr.start]` (the user's original inter-block whitespace, a P1 copy). Guard with `debug_assert!(gap.start <= gap.end)` plus a graceful fallback to the canonical separator — this is the reversed-slice guard the Phase-6 BP audit flagged (see "Premises surfaced by the Plan 7g Phase 6 BP audit" below). This generalizes today's `compute_separator` "consecutive in original → use original gap" branch (`incremental.rs:1049-1056`).
- **`TrailingState` is derived from the actually-emitted bytes' tail** after each entry (`None` / `EndsWithText` / `EndsWithNewline` / `EndsWithBlankLine`) — *not* tracked symbolically through the rule algebra (which would risk drift). The canonical `SeparatorRule` consults it: `StandardBlock{tight:false}` emits `""` after `EndsWithBlankLine`, `"\n"` after `EndsWithNewline`, `"\n\n"` after `EndsWithText`. This is the principled form of today's lone `prev_block_text.ends_with("\n\n")` special case (`incremental.rs:1059`).
- **Precedence in `assemble`:** try `OriginalGap` first (most faithful — reproduces the user's whitespace as P1 bytes), else fall to the enclosing `Recurse`'s `SeparatorRule` resolved against `TrailingState` (P2 bytes).

### The dispatch table

`plan_user_writes : (Node, target, align_ctx) → UserWrite`. Total recursive function over the AST, dispatched on the pair `(align_ctx.alignment_kind, node.source_info_shape)`. The table:

| Alignment | Source-info / structure | Rule | Operation |
|---|---|---|---|
| `KeepBefore(i)` | preimage in target | R1 | `Verbatim(preimage)` |
| `KeepBefore(i)` | atomic-kind Generated, no preimage | R2 | `Omit` (no warning; content regenerates from baseline) |
| `KeepBefore(i)` | non-atomic, no preimage, **block** container with source-bearing children | R3 (Transparent-form) | `Recurse{ Bytes(""), children-coarsened, "", separator }` |
| `KeepBefore(i)` | non-atomic, no preimage, **inline-content** block (`has_inline_content`) | R3-inline (helper-shell inline recursion — see below) | rare; same primitive as the `UseAfter` inline-content row. |
| `KeepBefore(i)` | non-atomic, no preimage, **true leaf** (no recursable children) | R5 | `Leaf{ serialize_leaf(node) }` (rare; cross-file-rooted leaf, etc.) |
| `UseAfter(j)` | atomic-kind Generated with preimage | R1' (soft-drop) | `Verbatim(preimage)` + Q-3-43 |
| `UseAfter(j)` | atomic-kind Generated, no preimage | R2' (soft-drop) | `Omit` + Q-3-43 |
| `UseAfter(j)` | atomic `Custom` | R5-special (let-user-win) — **deferred to 7e** | *Designed:* `Leaf{ serialize_leaf(node) }` via `plain_data`; no warning. *In 7d:* the qmd `Block::Custom` arm is empty, so `serialize_leaf` emits nothing — treat as **opaque** (R1' verbatim of preimage if present, else R2' Omit). 7e fills the arm and activates R5-special. |
| `UseAfter(j)` | non-atomic, no preimage, **block** container (`is_container_block`) | R3 | `Recurse{ shell_open, children, shell_close, separator }` shells from qmd writer's per-container syntax helpers |
| `UseAfter(j)` | non-atomic, no preimage, **inline-content** block (`has_inline_content`: Para/Plain/Header) | R3-inline (**helper-shell inline recursion** — see below) | `Recurse{ shell_open-from-block-syntax-helper, inlines-each-dispatched-fresh, shell_close, separator }`. No `inline_plan` exists under `UseAfter`; every child inline is dispatched against its own `source_info`. |
| `UseAfter(j)` | non-atomic, no preimage, **true leaf** (none of the three predicates) | R5 | `Leaf{ serialize_leaf(node) }` |
| `UseAfter(j)` | non-atomic with preimage | R1 | `Verbatim(preimage)` (paste-from-elsewhere; trust the producer's source_info) |
| `RecurseIntoContainer{ before, after }` | non-editable inside | R1' / R2' (soft-drop) | `Verbatim(preimage)` + Q-3-43 (if preimage exists), or `Omit` + Q-3-43 (no preimage). Recursion stops here. |
| `RecurseIntoContainer{ before, after }` | editable inside, block container | R3 | `Recurse{ shell_open, children-coarsened-per-`block_container_plans`, shell_close, separator }` |
| `RecurseIntoContainer{ before, after }` | editable inside, inline container | R4 | `Recurse{ shell_open-from-original-prefix, inlines-coarsened-per-`inline_plans`, shell_close-from-original-suffix, separator }` |

The dispatch is total, and the rows are read as an **ordered match** (Phase 2 implements `dispatch()` as an explicit ordered match, not a set of mutually-exclusive predicates the reader must reconcile): for a given alignment kind, the first row whose source-info/structure condition holds wins. Totality (Property #2) and the "exactly one row" claim then read off the order. R3 and R4 are structurally the same operation (recurse with shells); they're listed separately because R3 dispatches on `block_container_plans` while R4 dispatches on `inline_plans`, and the shell sources differ (R4 takes shells from the *original* block's source bytes for the inline-splice case; R3 takes shells from the new container's syntax helpers).

**The "container / leaf" column is a *three-way* structural classifier, not binary, and it reuses the reconciler's own predicates** (`crates/quarto-ast-reconcile/src/compute.rs`) so the writer's recursion boundary stays in lockstep with the reconciler's:

- `is_container_block(n)` (`compute.rs:218` — Div, BlockQuote, Ordered/Bullet/DefinitionList, Figure, Table, Custom) → **block container**, R3 (recurse block children). **Caveat — Custom:** `is_container_block` includes `Block::Custom`, but in 7d a CustomNode is *not* routed to R3. The editability gate marks non-atomic Custom not-editable-inside until 7e, so in the ordered match it matches the earlier "non-editable inside → R1'/R2'" row (opaque soft-drop) *before* reaching the R3 row. Read "is_container_block → R3" as "→ R3 **except Custom**, which the opaque rows claim first" until 7e flips the gate.
- `has_inline_content(n)` (`compute.rs:233` — Paragraph, Plain, Header) → **inline container**, R4 / inline-recurse. **This includes the `UseAfter` Para/Plain/Header case** (brand-new block, no `inline_plan`): the node still routes to inline recursion, where every child inline is itself `UseAfter`/fresh. A Para is *never* serialized whole via R5.
- `is_container_inline(n)` (`compute.rs:759` — Emph, Strong, Link, Span, Cite, Note, …) → **inline container**, inline `Recurse`.
- none of the above → **true leaf**, R5.

These three predicates are pairwise disjoint by construction (verified: `is_container_block` ∩ `has_inline_content` = ∅; the inline predicate operates on a different type), so the classifier is total and unambiguous. Where the dispatch table above says "container" read "matches `is_container_block` or `has_inline_content` (block side) / `is_container_inline` (inline side)"; where it says "leaf" read "matches none of the three."

**Reconciler-table-exclusivity invariant (R3 vs R4 under `RecurseIntoContainer`).** The reconciler's Phase 2 (`compute.rs:131-183`) is a strict type-keyed cascade — `if is_container_block → block_container_plans (or custom_node_plans / table_plans) … else if has_inline_content → inline_plans` — and each `exec_idx` is processed exactly once. Therefore a given `result_idx` populates **at most one** sub-plan table, and R3 (`block_container_plans`) vs R4 (`inline_plans`) is decided by block type with no overlap. Phase 2's `dispatch()` should `debug_assert!` that a `result_idx` present in `block_container_plans` is absent from `inline_plans` (and vice versa), making the exclusivity a checked invariant rather than an assumed one.

**Helper-shell inline recursion (the shared primitive).** Two distinct dispatch situations both need "recurse an inline-content block's inlines, with the block's wrapping bytes coming from the qmd writer's *syntax helper* rather than from original source bytes":

1. A `UseAfter` / no-preimage `KeepBefore` on an `has_inline_content` block (Para/Plain/Header) — the new-row above. There is no `inline_plan` (the reconciler only builds one under `RecurseIntoContainer`), so every child inline is dispatched fresh against its own `source_info`.
2. The **R4 fallback** below, when original-byte shells are unavailable.

Define this once: **`recurse_inline_content_with_helper_shells(block)`** = `Recurse{ shell_open: serialize_block_shell_open(block), children: [plan_user_writes(inline) for inline in block.inlines], shell_close: serialize_block_shell_close(block), separator: InlineConcat }`. For a plain `Para` the shells are empty; for a `Header` the shell-open is `"## "` etc. The shells are R3-style (helper-derived, P2 bytes), *not* R4-style (original-prefix/suffix, P1 bytes). Both situations above resolve to this one function.

**R4 fallback when the original-byte shells are unavailable.** R4 takes its shells from the *original* block's prefix/suffix bytes via `assemble_inline_splice` (`incremental.rs:1279`): `prefix = Source[block.start .. first_inline.start]`, `suffix = Source[last_inline.end .. block.end]`, both resolved through `preimage_in` (so the shells are the exact byte-complement of the children — this is what keeps R4 inside the (M) disjoint family). But `assemble_inline_splice` returns `None` when a boundary is unavailable: a non-contiguous `Concat`, an anchorless `Generated`, or a guard-violating (reversed/non-nested) range (`incremental.rs:1296-1325`). **Today the caller falls back to `Rewrite{write_block_to_string}` — which 7d deletes.** The algebra therefore needs an explicit fallback: when R4's original-byte shells are unavailable, **fall back to `recurse_inline_content_with_helper_shells`** (the shared primitive above) over the same recursed children. *Do not* fall back to a whole-block `serialize_leaf` (that would re-serialize children the algebra is supposed to recurse into, and there is no `Rewrite` to land on). Phase 3 must wire this `None → helper-shell` path; an implementer must not reach for the deleted `Rewrite`.

**CustomNodes are opaque in 7d (until 7e).** Every dispatch row above that would recurse into or serialize a `CustomNode` — R3 on a non-atomic `Custom` container, R5-special on an atomic `Custom` — is gated off until 7e, because (a) the qmd `Block::Custom` shell helper is empty and (b) the writer has **no `custom_node_plans` recursion** today (zero references in `crates/pampa/src/writers/`). So under 7d a `CustomNode` is treated as an **opaque block**: emit `Verbatim` of its original preimage if it has one (the common case — a callout parsed from `:::{.callout-note}…:::` in the source file), else `Omit` + `Q-3-43`. The writer never descends into it. This requires **no custom-specific code** — it reuses the soft-drop rules R1'/R2' — and gives a *better* interim behavior than today's `Rewrite`→empty vanish (the callout is preserved intact; only the edit is refused). 7e opens the box: it fills the `Block::Custom` shell helper **and** wires `custom_node_plans` recursion, after which CustomNodes match R3/R5-special naturally. Mechanism: the writer-side editability check treats non-atomic CustomNodes as not-editable-inside until 7e, so they route through the existing `RecurseIntoContainer → not-editable → R1'/R2'` path.

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
9. **Block-level Invocation coalescing (consecutive runs).** Within any `Recurse`, a maximal run of *consecutive* children whose `preimage_in(target)` returns the same `Some(range)` collapses to a single `Verbatim` of that range — the block-level analogue of today's multi-inline dedupe. This is **consecutive-only**; it does *not* coalesce a same-preimage group whose survivors are non-adjacent. That weaker form is sufficient: the only producer of splittable N-to-1 output (block shortcodes via `ShortcodeResult::Blocks`) renders as a client read-only region, so no edit path can separate the survivors — premise **L3** in the [BP proof](../designs/incremental-writer-bp-proof.md) §6, held by design (2026-06-01), not implemented. 7d need not generalize Property #9; revisit only if a non-client edit path (programmatic, filter block-reorder) is added.

   **The equality predicate is `preimage_in(target)` range-equality, and this is the right choice (precise statement).** For a `Generated` node, `preimage_in` (`source_info.rs:435`) walks the `Invocation` anchor, so every node in an N-to-1 group returns the *identical* range. Under **P4** (7g tiling), sibling preimages are pairwise disjoint *except* the shared-`Invocation` group — so the only way two sibling/consecutive children share a non-empty `preimage_in` range **is** that group, and there the range *equals* the invocation anchor's preimage. Hence range-equality and today's anchor-`PartialEq` (`incremental.rs:1408-1438`) **coincide on exactly the groups Property #9 must coalesce**, *given P4*. Range-equality is moreover **strictly safer for the multiplicity clause (M)**: it also collapses two distinct `SourceInfo` encodings of the same range (e.g. `Original{f,5,10}` vs a `Substring` resolving to `5..10`) that anchor-`PartialEq` would treat as different and thus double-emit bytes `5..10` — an (M) violation. (This supersedes the earlier loose "catches cross-shape collisions" wording: under P4 such collisions cannot arise *between distinct logical tokens* among siblings; range-equality's value is dedup-correctness for the *same* token seen two ways, not catching unrelated collisions.) **Empty-range guard:** P4's disjointness is vacuous for zero-width ranges (`∅ ∩ ∅ = ∅`), so two adjacent children could both report `Some(k..k)`. Coalescing on an empty range is a no-op (a `Verbatim(k..k)` emits nothing, exactly as R1 on each child would), so the predicate should **skip coalescing when `range.start == range.end`** — harmless either way, but skipping keeps the predicate from masking a producer bug that ought to surface elsewhere.

### What the refactor concretely changes

**1. `write_block_to_string` decomposes into shell helpers + `serialize_leaf`.** The unified-pass version (`write_block_to_string` as it exists today) becomes a derived convenience function that the rest of the codebase can still call for native rendering — it just isn't used by the incremental writer's `plan_user_writes` step anymore.

The decomposition covers the container kinds that have qmd writer arms today: BlockQuote, Div, Figure, NoteDefinitionFencedBlock, OrderedList, BulletList, DefinitionList, Table. **CustomNodes (Callout, Theorem, Proof, FloatRefTarget, labelled equations) do not have qmd writer arms today** — their `Block::Custom` arms in `qmd.rs:2354` are empty. CustomNode shell helpers — **and the `custom_node_plans` recursion the writer lacks today** — land in Plan 7e, not in 7d Phase 1. Under 7d alone, custom nodes are **opaque** (verbatim-preserve or omit; see "CustomNodes are opaque in 7d" under the dispatch table): the user's interior edit is refused with `Q-3-43`, but the callout itself is preserved intact. 7e closes the gap by filling the shell helper and wiring the recursion, making interior edits round-trip.

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
  - **`LineAware` semantics = the existing `Write`-adapter pattern** (`BulletListContext` / `OrderedListContext` / `BlockQuoteContext`, `qmd.rs:145-259`): a byte-stream filter that prepends `marker` on the first line and `continuation_indent` after each `\n`, re-arming at every newline. `assemble(Recurse{LineAware{…}, children})` therefore means *wrap the children's assembled byte-stream in this line-prefixing filter* — not "concatenate a marker string then the children." **Nested line-aware containers compose by filter nesting** (a list item inside a blockquote wraps `BulletListContext` around `BlockQuoteContext`, exactly as today), so each newline accumulates both prefixes. This keeps `TrailingState` / `OriginalGap` operating on the *post-filter* emitted byte tail, consistent with "trailing state is derived from actually-emitted bytes."
  - **`LineAware.marker` is computed per item, not per list.** Ordered-list marker spacing is number-dependent (`number < 10` → two spaces, `≥ 10` → one space) and example lists use `(@)` (`qmd.rs:228-243`). The planner fills `marker` for item *i* from item *i*'s number/style at plan-construction time. This stays inside the pre-existing marker-fidelity non-change (markers regenerate canonically; they are not copied from the original source bytes).
- [ ] Do the same for inline containers (Emph, Strong, Link, Image, Span, Cite, Note, …): `serialize_inline_shell_open(inline) → ShellOpen` and `serialize_inline_shell_close(inline) → Bytes`.
- [ ] Define `serialize_leaf(node) → Bytes` as `write_block_to_string` restricted to leaves. **Guard at runtime, not the type level:** `Block` (`block.rs:16`) and `Inline` (`inline.rs:13`) are each a single flat enum with no leaf/container split, so type-enforcement would require carving `LeafBlock`/`ContainerBlock` newtypes across the whole AST — too invasive. Instead, `serialize_leaf` is an exhaustive match over the leaf variants; container variants hit `debug_assert!(false, …)` and return a diagnostic error (a `Q-3-*` code) in release — **do not panic in the writer**. **Define "leaf" by the reconciler's predicates, not a hand-maintained list:** the guard is `debug_assert!(!is_container_block(n) && !has_inline_content(n) && !is_container_inline(n))` (the three predicates from §"The dispatch table"). This keeps `serialize_leaf`'s domain in lockstep with the reconciler's recursion boundary — a variant later added to any of the three predicates is automatically excluded from `serialize_leaf` rather than silently mis-serialized. Property #5 + totality guarantee the guard is never reached in practice; it is belt-and-suspenders that yields a precise error if the dispatch regresses.
- [ ] Preserve `write_block_to_string` as a public convenience function. Its implementation becomes `shell_open + assemble(children-coarsened) + shell_close` — but the incremental writer no longer calls it.
- [ ] **CustomNode arms intentionally not in 7d's scope.** The current empty `Block::Custom(_)` arm (`qmd.rs:2354`) stays empty under 7d; Plan 7e fills it. Under 7d a CustomNode is **opaque** (see "CustomNodes are opaque in 7d" under the dispatch table): emit `Verbatim` of its original preimage if present, else `Omit` + `Q-3-43`; the writer never descends into it (no `custom_node_plans` recursion in 7d). The callout is preserved intact; only the interior edit is refused. 7e fills the shell helper and wires the recursion. The mechanism in 7d is one editability-gate line treating non-atomic CustomNodes as not-editable-inside — *not* any custom-content serialization code.
- [ ] Tests: each shell-helper has a unit test that asserts its output for a known node.

### Phase 2 — Restructure `plan_user_writes` dispatch

- [ ] Define the new `UserWrite` shape with `Verbatim`, `Omit`, `Recurse`, `Leaf` variants. Define `ShellOpen` and `SeparatorRule` enums. Delete `CoarsenedEntry::{Rewrite, Transparent, InlineSplice}` (their roles are absorbed).
- [ ] Rename `coarsen` → `plan_user_writes`, `coarsen_blocks` → `plan_user_writes_blocks` (or absorb into `plan_user_writes`). The function-level renames cascade through `~23` in-file references and `~16` plan-7 references in this file (already done above). `coarsen_keep_before_block` disappears: its logic is absorbed into the dispatch table.
- [ ] Implement the dispatch table from §"The dispatch table" as a single `dispatch(node, align, target) → Rule` function, written as an **explicit ordered match** (first matching row wins) so totality and "exactly one row" are structural, not left to the reader to reconcile predicates. Each rule has a small implementation: R1 packages `Verbatim`; R2 packages `Omit`; R3 / R4 package `Recurse` with shells from Phase 1's helpers and children recursed via `plan_user_writes`; R5 packages `Leaf { serialize_leaf(node) }`. CustomNodes are gated to the opaque (R1'/R2') path until 7e.
- [ ] `plan_user_writes_blocks` becomes a thin wrapper that iterates the `block_alignments`, calls `dispatch` for each, threads separator context.
- [ ] Implement Property #9 (block-level Invocation coalescing) **as a planner step inside `plan_user_writes`, before `assemble` runs**: within each `Recurse`, group *consecutive* children whose `preimage_in(target)` returns the same `Some(range)`; emit a single `Verbatim` for the run. Key on **range-equality** (not anchor `PartialEq`) and **skip the group when `range.start == range.end`** (empty-range guard — see Property #9). Consecutive-only is sufficient (premise L3, held by design — see Property #9 above); correctness of range-equality rests on P4 (7g). Coalescing precedes separation; the two never interleave.
- [ ] **Preserve document-boundary infrastructure.** The existing helpers `emit_metadata_prefix` (`incremental.rs:950`), `find_metadata_trailing_gap` (`:1006`), and `ensure_trailing_newline` (`:1111`) handle the gap between YAML frontmatter and the first block, and the parser's input-padding convention (qmd reader pads input with `\n` when it doesn't end with one). The new `plan_user_writes` + `SeparatorRule` + `TrailingState` design must preserve their behavior. Specifically: `assemble` must still emit the metadata-prefix bytes before the first block's coarsened entry, and must still strip the synthesized trailing `\n` from output when the input qmd didn't have one. Add a regression test for both behaviors on a fixture that exercises them (a doc with YAML frontmatter, no trailing newline).
- [ ] Verify against today's regression tests: every existing test in `crates/pampa/tests/integration/incremental_writer_tests.rs` (moved under `tests/integration/` by bd-xvdop — not the old top-level `tests/incremental_writer_tests.rs`) must still pass byte-for-byte. The refactor doesn't change observable behavior on the inputs the tests cover (modulo CustomNodes, where current behavior is also broken).
- [ ] **Inventory the CustomNode-touching regression tests up front** (before Phase 1 lands). For each, predict its new output under 7d's opaque treatment — **preserved verbatim** (preimage exists, the common `:::{.callout-note}…:::` case) or **`Q-3-43` omit** (no preimage). Record the prediction set in this plan. Then treat any snapshot change *outside* that predicted set as a regression to investigate, not an auto-accept. This converts the "modulo CustomNodes" caveat into a closed checklist rather than a hand-wave.
- [ ] **Delete the stale comment at `incremental.rs:434`** ("the qmd writer's CustomNode arm serializes the fresh plain_data" — the arm is empty) when the let-user-win block is replaced by the dispatch table. The whole block dies in the refactor; this item ensures the false comment is not carried forward into the new dispatch code.

### Phase 3 — Restructure `assemble_inline_content`

- [ ] Define inline `Rule` dispatch analogous to block-level. R1-inline, R2-inline, R3-inline (inline `Recurse` for nested inline containers), R5-inline (leaf inline).
- [ ] `assemble_inline_content` becomes a recursive plan over the inline cascade. Phase 1's two-phase shape (soft-drop substitution + emit-with-dedupe) collapses to a single pass.
- [ ] Multi-inline dedupe (today's `assemble_inline_content` shared-`Invocation`-anchor optimization, `incremental.rs:1408-1438`) collapses into the block-level Property #9 mechanism; the rule keys on **`preimage_in(target)` range-equality** rather than anchor `PartialEq`. This is correct and *dominates* anchor-`PartialEq` for the multiplicity clause (M), and the two coincide on the cases that matter — see the precise statement under Property #9 below.
- [ ] **Inline `UseAfter` dispatches on the *new* node's own `source_info`** — mirroring the block-level `UseAfter` arm (`incremental.rs:392–439`, the `e584428d` fix), which reads `new_si.preimage_in(...)` and never consults the displaced original. This **removes** today's `result_idx` positional proxy (`incremental.rs:1376–1378`) rather than preserving it. Consequence: it **obsoletes 7c Phase 7** (`displaced_before_idx` — there is no original-side lookup left to make precise) and **subsumes 7c Phase 7b** (R1' on atomic-Generated-with-preimage *is* the new-side atomicity check). Do not assume `displaced_before_idx` exists; do not re-introduce the proxy.
- [ ] Verify: every existing inline-cascade test passes byte-for-byte.

### Phase 4 — Property tests for BP + benchmarking

The algebra is sound and complete by construction. Property tests pin both invariants against bugs in the implementation. Phase 4's testing strategy has four coordinated pieces:

**Generator.** `gen_pandoc_with_atomic_descendants` produces random ASTs with atomic-Generated descendants (shortcode, filter, title-block, tree-sitter-postprocess) embedded at varying depths inside non-atomic containers, plus a random user-edit applied on top. The generator extends the existing `crates/quarto-ast-reconcile/src/generators.rs` infrastructure with two new capabilities: injecting atomic-Generated nodes at configurable depths with configurable density, and applying realistic user-edit transformations (paragraph rewrap, list-item insert, etc.). **Set `block_features.custom = false` and `inline_features.custom = false`** in the generator's `GenConfig` (the `custom` flag lives on `BlockFeatures`/`InlineFeatures` — `generators.rs:279`/`167` — reached via `GenConfig.block_features`/`.inline_features`; it is set `true` by the `full()` constructors at `generators.rs:328`/`225`, driving `gen_custom_block`/`gen_custom_inline` for `"Callout"`/`"CustomWidget"`. It is *not* a direct `GenConfig` field). CustomNodes are opaque in 7d (empty `Block::Custom` arm), so a generated CustomNode serializes to empty/soft-drop and would fail `completeness_holds` (`parse(Source') ≢ AST_new`) — soundness `bp_holds` is unaffected. CustomNode property/completeness coverage moves to **Plan 7e**, where the arm is filled. Do not include callout-class-toggle as a generated edit in 7d for the same reason. The generator's coverage of the case space is what determines how thoroughly the dispatch table is exercised — see the coverage instrumentation below.

**Marker-string convention (for soundness).** The soundness property `bp_holds` detects byte leaks via a recognizable marker string embedded in atomic-Generated nodes' resolved content. The generator chooses a fresh marker per iteration (e.g. `__BP_LEAK_e94f__` with a per-iteration UUID suffix), injects it into every atomic node's resolved text, runs the writer, and asserts the marker doesn't appear in `Source'`. The randomness avoids accidental collisions with legitimate document text; the recognizability makes any leak trivially detectable. One line of assertion per iteration; the property scales with proptest's iteration count.

**Structural-equivalence reuse (for completeness).** The completeness property `completeness_holds` asserts that `parse(Source') ≡ AST_new` for non-soft-drop inputs. The equivalence check reuses the reconciler's source-info-blind block hashers — `compute_block_hash_fresh` / `compute_blocks_hash_fresh` (`hash.rs:102`/`115`), or the `HashCache::hash_block(s)` methods — which already exclude per-node `source_info` and other fidelity-irrelevant fields. (Note: there is no symbol named `compute_block_hash`; use the `_fresh` variants. The often-quoted `hash.rs:498` comment about excluding `source_info`/`key_source` documents `compute_meta_hash_fresh`, the `ConfigValue`/meta hash — but its last line confirms the block/inline hashers exclude `source_info` too, which is the property we rely on here.) Reusing this hash means the completeness check absorbs the documented helper-canonicalization gaps (list markers, lazy numbering, block-container shells) at exactly the AST level where the gaps are invisible — no bespoke canonicalization matcher needed; the reconciler's existing source-info-blindness does the work.

**Required dispatch-coverage instrumentation.** Property tests verify every input satisfies the property but say nothing about *which* dispatch rules the generator actually exercises. Feature-gated counters per dispatch row, with minimum-coverage assertions tuned per row, keep the generator honest and document which rules the project considers load-bearing. The full spec is in the work items below.

The four pieces fit together: the generator drives input distribution; the marker-string and structural-equivalence machinery let the two properties assert their respective invariants without bespoke per-rule matchers; the coverage instrumentation pins generator distribution against drift.

- [ ] Write a proptest generator `gen_pandoc_with_atomic_descendants` that produces ASTs with atomic-Generated descendants at varying depths inside non-atomic containers, plus arbitrary user edits applied.
- [ ] Write the property `bp_holds` (soundness): given a generated `(AST_old, AST_new, Source)` and a reconciler plan, run the writer. Assert: the output `Source'` does not contain any of the resolved bytes of atomic-Generated descendants. (Implementation: tag the generator's atomic-resolved content with a recognizable marker string; assert the marker doesn't appear in `Source'`.)
- [ ] Write the property `completeness_holds`: for inputs that don't trigger soft-drop, `parse(Source')` is structurally equivalent to `AST_new`. Implementation: filter the generator to skip cases where the reconciler's plan + atomic-classification would route any node to R1' or R2'; assert structural equivalence via the reconciler's source-info-blind block hashers (`compute_block_hash_fresh` / `compute_blocks_hash_fresh`, `hash.rs:102`/`115` — *not* `compute_block_hash`, which does not exist), which absorb helper-canonicalization gaps (list markers, lazy numbering, block-container shells) by operating at the AST level rather than byte level. The two properties together pin both soundness (no leaks) and completeness (no drops outside soft-drop).
- [ ] Add property tests for individual rule soundness: R1 emits bytes from `Source`; R5 emits authored content with no descendants; R3 / R4 emit bytes that are concatenations of shell + children.

- [ ] **Dispatch-coverage instrumentation.** Property tests verify every input satisfies the property, but say nothing about *which* dispatch rules the generator actually exercises. Add thread-local counters in `plan_user_writes`, gated behind a `dispatch-coverage` build feature (zero cost in production), that tick per dispatch row each time it fires:

  ```rust
  #[cfg(feature = "dispatch-coverage")]
  thread_local! {
      static DISPATCH_COUNTERS: RefCell<DispatchCounters> = Default::default();
  }

  #[derive(Default, Clone)]
  struct DispatchCounters {
      r1: usize, r1_prime: usize, r2: usize, r2_prime: usize,
      r3_helper: usize, r3_transparent: usize, r4: usize,
      r3_inline_helper: usize,  // helper-shell inline recursion: UseAfter/no-preimage
                                // has_inline_content blocks + the R4 None-fallback
      r5: usize, r5_special: usize,
  }
  ```

  Each property test runs proptest first, then checks coverage. **Do not hardcode magic absolute floors** (`r1 >= 100`, …) — they are brittle and arbitrary before the generator has ever run. Instead:

  1. **Primary gate = each reachable row ≥ 1.** This catches a row going *unreachable* (the real regression — a dispatch row that no input can hit, or that a refactor accidentally orphaned). `r5_special` is **excluded** from this gate in 7d (CustomNodes are opaque; it's exercised in 7e).
  2. **Express any magnitude floor as a fraction of the proptest case count `N`** (e.g. `r1 >= N / 10`), so floors scale when someone changes the iteration count instead of silently passing/failing.
  3. **Calibrate the fractions empirically:** run the generator once, record observed per-row counts, set each floor at ~⅓–½ of observed (headroom for run-to-run variance). Don't guess up front.
  4. **Always print the observed per-row distribution** under the `dispatch-coverage` feature (a one-line report per run), so drift is visible even when the test passes.

  ```rust
  // reachability gate (primary)
  for (name, count) in counters.rows_excluding_r5_special() {
      assert!(count >= 1, "dispatch row {name} unreachable by generator");
  }
  // empirical magnitude floors (calibrated; fractions of N)
  assert!(counters.r1 >= n_cases / 10, "R1 under-exercised: {} (< N/10)", counters.r1);
  // … one per common row, floors derived from a calibration run …
  eprintln!("dispatch-coverage: {counters:?}");   // visible distribution report
  ```

  A future contributor adding a dispatch row adds its reachability check; a refactor that orphans a row surfaces as a reachability failure rather than as a silently-passing magic threshold.

- [ ] **Calibration run (explicit step, do not skip).** Run the generator once under `--features dispatch-coverage`, record the observed per-row counts in this plan, and set each magnitude floor at ~⅓–½ of its observed count. The reachability gate (each row ≥ 1) needs no calibration; the fractional floors do. Commit the calibrated numbers so a later contributor sees what "healthy" looks like.
- [ ] Run under `cargo nextest run -p pampa --features dispatch-coverage` with high iteration counts. Save regression seeds if any property fails or any row falls below threshold.

**End-to-end (Playwright) subtask — the R5 trust point.** R5 emits `serialize_leaf(n)` *trusting* the producer's source_info classification (nodes reaching R5 are attested user-authored; atomic-Generated routes to R1'/R2' instead). The strict form trusts the React framework to stamp `Generated{by: user_edit}` (7f Phase 3). The property tests above exercise the writer in isolation with synthetic source_info; they do **not** exercise the real framework→writer stamping path. That path *is* e2e-testable through the existing write harness `hub-client/e2e/q2-preview-render-components-write.spec.ts` (Automerge → hub → browser → WASM → `incremental_write_qmd`). The trust point's *consequences* are observable even though its *universal premise* (no producer ever mis-stamps) is an audit obligation, not a runtime test.

- [ ] **No-leak (soundness).** Fixture containing `{{< lipsum 3 >}}`; in-browser, click into the resolved paragraph, type a word, trigger the write. Assert the written qmd still contains `{{< lipsum 3 >}}` and does **not** contain the resolved lorem-ipsum text or the typed word. (The contract's canonical example, finally tested end-to-end.)
- [ ] **R5 authored-leaf (completeness).** Fixture with a plain paragraph; type a new word; assert the new word appears in the written qmd.
- [ ] **Soft-drop signal.** Assert `Q-3-43` surfaces on the shortcode-paragraph edit.
- [ ] (Honest-status note for the plan log: consequences e2e-tested via Playwright; the "no producer mis-stamps" premise is discharged by 7f's `SourceInfo::default()` audit + the "new kinds default to non-atomic" rule, not by a runtime assertion.)

**Benchmarking subtask.** Synthetic fixture: ~500 top-level blocks with a mix of plain paragraphs, sectionize wrappers, shortcodes, callouts, and one nested list. Measure end-to-end `incremental_write_qmd` time on (a) a single-block edit and (b) a whole-document edit. Both old `coarsen` and new `plan_user_writes` are O(n); the bench is a sanity check.

- [ ] Build the 500-block fixture as a checked-in test fixture under `crates/pampa/tests/fixtures/perf/`.
- [ ] **Capture the baseline as a frozen number *before* Phase 1 lands.** Run the bench against `feature/provenance` HEAD (pre-7d `coarsen`) for cases (a) and (b), and record the absolute timings in this plan. The 2×/1.5× bounds below assert against this committed number — *not* against a baseline re-measured after the refactor (which would be the post-7d code measuring itself, since 7d lands in-place on the branch).
- [ ] Add a benchmark harness that runs the same edit through the new `plan_user_writes`.
- [ ] Assert: new is within 2× of the frozen baseline for case (a); within 1.5× for case (b). If those bounds hold, performance is not a concern.

### Phase 5 — Obsolete the denylist branches the algebra makes redundant

7d ships **before** 7c, so there are no shipped 7c Phase-7/7b branches to remove from the codebase — those phases were never implemented. This phase is therefore mostly bookkeeping (the code-level subsumption happens in Phase 3, where the inline `UseAfter` dispatch drops the positional proxy).

- [x] Mark Plan 7c's Phases 7 (`displaced_before_idx`) and 7b (inline new-side atomicity check) **obsolete** in `2026-05-25-q2-preview-plan-7c-closure-gaps.md` — the allowlist dispatch keys on the new node's own `source_info`, so Phase 7's original-side lookup no longer exists and Phase 7b's check *is* R1'. (Done as part of the 2026-06-03 plan-edit pass.)
- [ ] Confirm no *other* denylist branch survives in the new dispatch (the old cascade's `coarsen_keep_before_block` arms, the inline two-phase soft-drop) — they are absorbed into the ordered match, not retained alongside it. Update/retarget any test that asserted the old cascade's structure to the new dispatch rows.
- [ ] Leave 7c's orthogonal phases (Q-3-41 catalog/gates, TS gate parity, per-kind soft-drop tests, `Q343Reason`) untouched; note in 7c that Phases 4/5's per-kind tests should target 7d's dispatch rows when written.

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
- **CustomNode qmd serialization.** The qmd writer's `Block::Custom` arm is currently empty; 7d does not fix this. The CustomNode shell helpers needed by R3, and the `custom_node_plans` recursion the writer lacks, land in Plan 7e. Under 7d alone, custom nodes are **opaque** (verbatim-preserved or omitted; interior edits refused with `Q-3-43`) — a *better* interim state than today's vanish, but still not round-trippable; 7e closes that gap.

## Relationship to siblings

- **Plan 7** (shipped): provides the existing writer the algebra refactors. 7d's implementation phases (1–6) start from `feature/provenance` HEAD after 7f **and 7g** have landed.
- **Plan 7a** (open): runtime user-filter idempotence detection. Orthogonal.
- **Plan 7b** (open): test-coverage consolidation. The property tests in 7d Phase 4 *complement* Plan 7b's per-shape regression tests.
- **Plan 7c** (open; ships **after** 7d): closure gaps in the denylist cascade. 7d **obsoletes** 7c's Phases 7 and 7b (the inline `UseAfter` dispatch keys on the new node's own `source_info`, so there is no original-side lookup for Phase 7 to sharpen, and R1' already does Phase 7b's job); they are marked obsolete in 7c. 7c's other phases (Q-3-41, TS gate parity, per-kind tests, `Q343Reason`) are orthogonal and stay useful as standalone bug fixes.
- **Plan 7g** (complete): source-range tiling — establishes producer precondition **P4**, on which 7d's BP/completeness proof depends.
- **Plan 7e** (sibling): CustomNode qmd serialization. Ships after 7d; closes the callout-disappears-on-edit bug that 7d does not address.
- **Plan 7f** (sibling, ships before 7d): framework source_info preservation, user-edit stamping, wire-format renames, `SourceInfo::default()` audit. Prerequisite for 7d's strict R5 trust point.

## Risks

- **Refactor scope.** The decomposition of `write_block_to_string` touches every per-container arm in the qmd writer (excluding CustomNodes, which are 7e's scope). Each arm is small but there are many of them. Estimating 500-800 LOC of mechanical refactor work, plus 200-400 LOC of dispatch-table consolidation. Plan 7e adds another ~500-800 LOC for CustomNode shell helpers.
- **Behavioral compatibility.** Every existing test must pass byte-for-byte after the refactor on inputs that don't involve CustomNodes. CustomNode tests are 7e's concern.
- **Cost.** The Phase 4 benchmark is the verification mechanism; if cost regresses significantly, the benchmark catches it.
- **Producer-contract drift.** The algebra leans on producer hygiene at R5's trust point. If a producer introduces a leaf with non-default source_info that doesn't fit the algebra's classifications, R5 may emit bytes the algebra trusts but shouldn't. Plan 7f's `SourceInfo::default()` audit + the producer contract's "new kinds default to non-atomic" rule are the mitigations.

## Premises surfaced by the Plan 7g Phase 6 BP audit (2026-06-01)

The BP/completeness proof was strengthened with a multiplicity clause (M) —
*each source byte is copied at most once* — and re-verified in
[`incremental-writer-bp-proof.md`](../designs/incremental-writer-bp-proof.md)
(audit: [`../research/2026-06-01-plan-7g-phase-6-bp-audit.md`](../research/2026-06-01-plan-7g-phase-6-bp-audit.md)).
It holds under three premises. **One is a live obligation on 7d's dispatch (L2);
L3 is held by design and not an obligation:**

- **L2 — dispatch terminality (LIVE OBLIGATION).** `R1`/`R1'`/`R2`/`R2'` must
  emit and **not** recurse; only `R3`/`R4` recurse. Holds in the design as
  written; the audit flags it as a property to preserve, not change. Property #5
  (leaf-only serialization) and totality already imply it — keep them.
- **L3 — Invocation-coalescing completeness: HELD BY DESIGN, NOT IMPLEMENTED
  (2026-06-01 decision).** Property #9's consecutive-only coalescing does not
  satisfy L3 in general, but the only producer of splittable N-to-1 output (block
  shortcodes via `ShortcodeResult::Blocks`) renders as a **client read-only
  region**, so no front-end edit can leave the survivors non-adjacent. The
  premise's antecedent is unreachable; (M) holds. **7d need not generalize
  Property #9.** Revisit only if a non-client edit path (programmatic, filter
  block-reorder) is added — then coalesce by whole-walk `preimage_in`-equality or
  structurally group N-to-1 output. See proof file §6.
- **R4 shells / `OriginalGap` separators are P1 copies** and must be counted in
  the (M) disjoint family. They are the byte-complement of children within a
  container and are disjoint under P4; no code change beyond ensuring shell/gap
  extraction stays complement-based (it is today: `assemble_inline_splice`).
  **Optional defense-in-depth:** a `debug_assert!(gap.start <= gap.end)` +
  graceful fallback when (re)implementing `SeparatorRule::OriginalGap`, guarding
  the reversed-slice panic that overlapping siblings would cause. The root-cause
  fix for that panic is P4 (7g), not 7d — see 7g Phase 3.

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
