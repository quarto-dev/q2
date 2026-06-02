# Byte provenance for the incremental writer

*A design document specifying the writer-side contract; depends on the producer-side contract in [`provenance-contract.md`](provenance-contract.md).*

## Motivation

The incremental writer takes a structural edit on a rendered AST and produces a new qmd file that, when parsed and run through the pipeline again, yields the AST the user intended. Three inputs go in: the user's qmd source (`Source`), the AST parsed from that source, and a new AST reflecting the edit. One output comes out: a new qmd file (`Source'`). The work between them is byte-level. Every byte in `Source'` must have a defensible origin.

Producing `Source'` is harder than serializing the AST, because the pipeline that turned `Source` into the AST is non-injective in one direction. Sixteen bytes of `{{< lipsum 3 >}}` expand into three Paragraphs of lorem-ipsum text. A `{{< meta title >}}` shortcode resolves into inline content drawn from a YAML value. Synthesizers wrap content in containers that have no source bytes of their own. Filters construct new inlines and blocks whose source identity is the filter, not the user. The reconciler's `KeepBefore` / `UseAfter` / `RecurseIntoContainer` alignments tell the writer *which* AST nodes belong at each position, but they say nothing about *which bytes* to emit.

The risk is a class of bugs, not a single bug. Many editor actions can drive the writer through a path that serializes a subtree as a unit: typing into a paragraph that contains a shortcode-resolved inline, wrapping content around a synthesized container, toggling a class on a Div whose interior holds filter output, adding an item to a list whose first entry came from a metadata-driven shortcode, pasting a block whose descendants carry pipeline source identity. Each such path treats every descendant as if it were user content. Each such subtree may have descendants whose source identity is the pipeline, not the user. The descendants' bytes leak into `Source'`, and the next pipeline run finds literal text where a shortcode used to resolve — or finds synthesized chrome that the synthesizer now re-produces alongside the literal copy. The document drifts away from anything the user intended.

The canonical example is the user editing inside shortcode-resolved content. `Source` contains `{{< lipsum 3 >}}`; the pipeline expands it into three Paragraphs of lorem-ipsum text; the user types `world` into the second paragraph through the React editor; a naive writer emits `Lorem ipsum dolor world sit amet…` into qmd. The shortcode token is gone. The next pipeline run renders the literal text where the shortcode used to resolve, and the document is permanently broken. The wrong byte here is not `world` — the user authored that one. The wrong bytes are `Lorem ipsum dolor` and ` sit amet…`. Those came from the shortcode, not from the user, and they should never have entered source.

The contract exists to forbid this entire class of failure.

## The Byte Provenance Invariant

The writer follows a single rule about every byte it emits. Every byte in `Source'` either was already a byte in `Source` (the writer copied it from a known source position) or is a byte the user is authoring right now through an editor affordance (the writer is serializing an AST node the user just constructed). No third category exists. Pipeline-resolved bytes — shortcode output, filter output, synthesized container chrome, cross-file include content — cannot appear in `Source'`, because they belong to neither category.

This rule is the **Byte Provenance Invariant** (BP), and the rest of this document is concerned with stating it precisely, identifying the two contracts that together make it hold, and proving the construction sound.

## The two contracts

BP holds when two parties — the producer side of the pipeline and the writer side of the incremental update — each uphold their share of the discipline.

The **producer-side contract** governs how AST nodes are stamped with source_info as they are constructed. Pipeline transforms whose output is not user-authored content (shortcode resolution, filter construction, title-block synthesis, tree-sitter postprocessing) stamp `Generated{by: <atomic kind>, …}` on every node they emit. The React framework, on the user's behalf, stamps `Generated{by: user_edit, …}` on every node a user-edit affordance creates. Preserved nodes carry their original source_info forward unchanged. The full set of producer rules lives in [`provenance-contract.md`](provenance-contract.md); this document treats them as a precondition.

The **writer-side contract** — specified by the rest of this document — governs how source_info-stamped nodes turn into bytes. Every byte the writer emits is produced by visiting exactly one AST node, and the visit emits either a `Source`-copy at the node's preimage (satisfying BP clause P1, below) or a serialization of the node's node-local content (satisfying P2). Subtree serialization — emitting bytes derived from a node's descendants without independently visiting those descendants — is forbidden. Atomic-Generated nodes route to non-recursing rules, so their descendants are never visited and their resolved bytes never enter `Source'`.

The two contracts compose. Producer hygiene makes the writer's classification meaningful; the writer's structural discipline makes producer hygiene consequential. Either contract violated alone breaks BP: a producer that lies about source_info defeats the writer's classification, and a writer that subtree-serializes defeats the producer's stamping.

## From flat coarsen to recursive plan_user_writes

`coarsen` on `main` is flat: it produces one entry per top-level block. Anything inside a top-level block collapses into a single `Rewrite` that serializes the entire new block in one pass, without consulting any individual descendant's source_info. BP cannot be enforced from this shape.

The redesign makes `coarsen` recursive and renames it `plan_user_writes`. The recursive version produces one entry per AST node, descending through every container the reconciler descended through and consuming the tree of sub-plans (`block_container_plans`, `inline_plans`, `custom_node_plans`, `table_plans`, `list_item_plans`, and the inline analogues) that the flat coarsen ignored. Each visit classifies its node on the pair `(alignment_kind, source_info_shape)`, selects a dispatch rule, and emits a `UserWrite` whose shape encodes how the node contributes bytes to `Source'`. Containers emit a `Recurse` entry carrying the recursed children; leaves emit `Verbatim`, `Omit`, or `Leaf`. The result is a tree of entries mirroring the AST.

Recursion is what makes BP enforceable. BP is a property each byte must satisfy individually, and individuality requires that each AST node be classified individually. The flat coarsen could not separate the user's `world` from the shortcode's `Lorem ipsum dolor` — both lived inside a Para that the flat coarsen handled as a single `Rewrite`. The recursive `plan_user_writes` visits the Para first, recognizes its atomic-Generated source_info, copies the shortcode token's preimage as a `Verbatim` entry, and never descends into the `Str` children underneath. The user's edit is soft-dropped with a `Q-3-43` warning; the shortcode token survives unchanged; the byte `L` of `Lorem` never enters `Source'`, because the algebra never visited a node from which `L` could come.

## The Byte Provenance Invariant — formal statement

> **The verified formal statement and proofs live in
> [`incremental-writer-bp-proof.md`](incremental-writer-bp-proof.md).** This
> section is the *informal presentation*. The proof file carries the
> **strengthened** statement: the per-output-byte (P1)/(P2) dichotomy below is
> retained, plus a per-source-position **multiplicity** clause (M) — *each
> source byte is copied at most once* — without which the partition claimed in
> the next paragraph does not actually follow (two nodes may copy the same
> source byte; see the `Space`/`Code` `[1,9]` case in
> [Plan 7g](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md)). The
> strengthened invariant holds under three premises — **P4** tiling (producer,
> Plan 7g), **L2** dispatch terminality, **L3** Invocation-coalescing — verified
> in the [Phase 6 audit](../research/2026-06-01-plan-7g-phase-6-bp-audit.md).

Every AST node carries a `SourceInfo` value recording its byte-level origin. Four physical shapes encode that origin:

- `Original{file, start, end}` — the node's bytes are `file[start..end]`.
- `Substring{parent, start, end}` — a contiguous restriction of `parent`'s bytes.
- `Concat[pieces]` — the concatenation of `pieces`, each itself a `SourceInfo`.
- `Generated{by, from}` — bytes synthesized by an operation tagged `by`, with `from` a list of `Anchor` values recording diagnostically useful source positions.

The function `preimage_in : (SourceInfo, FileId) → Option<Range<usize>>` lifts these shapes into a contiguous byte range in a target file, or returns `None` when no such range exists. The walk is recursive:

- `Original{f, s, e}` returns `Some(s..e)` if `f == target` and `None` otherwise.
- `Substring{parent, s, e}` walks `parent`, then restricts the returned range to `s..e`.
- `Concat[pieces]` resolves every piece and returns the union of their ranges when the pieces are contiguous in `target`; it returns `None` otherwise.
- `Generated{by, from}` walks the `Invocation` anchor only. `ValueSource`, `Dispatch`, and `Other` anchors are diagnostic-only and produce no bytes — a role-asymmetry the producer contract enforces.

`preimage_in` is total and side-effect-free.

**Authored content.** For an AST node `n`, *authored content* refers to the part of `n`'s qmd serialization that traces to user authorship — for a leaf, the entire serialization; for a container, the shell syntax (open, close, per-child separator). Authored content excludes descendants' bytes, which propagate through the recursion.

The producer contract scopes the term: a node whose source_info is atomic-Generated represents pipeline output, not user authorship, and has *no authored content* — its serializable text is the pipeline's, not the user's. The dispatch routes such nodes to non-emitting rules (R1' / R2'), so the writer never tries to serialize the pipeline's bytes as if they were the user's. When the proofs below speak of authored content, they refer to bytes the producer contract attests as user-authored — pipeline-generated nodes have none, by definition.

Let `Source` be the user's qmd file at file identifier `target`, and let `Source'` be the qmd file the writer produces. The invariant binds every byte of `Source'`:

> **(BP)** For every byte `b` in `Source'`, exactly one of the following holds.
>
> **(P1) Copied.** `b = Source[i]` for some position `i ∈ preimage_in(n, target)` for some AST node `n` in the new AST.
>
> **(P2) Authored.** `b` was produced by serializing the *authored content* of a single AST node `n`. Children's bytes do not arrive through their parent's serialization; they arrive only when the algebra independently visits each child.

The two clauses partition the bytes of `Source'`. The algebra never overlaps them; every byte traces to exactly one visited node.

## The dispatch

The recursive `plan_user_writes` selects one of five rules at each node, by the pair `(alignment_kind, source_info_shape)`. R1 emits a `Verbatim` of the node's preimage when the node has one. R2 emits an `Omit` when the node is atomic-Generated with no preimage. R3 and R4 emit a `Recurse` over a container's children, with shells from the qmd writer's syntax helpers (R3) or from the original block's prefix/suffix bytes (R4). R5 emits a `Leaf` carrying `serialize_leaf(n)` when `n` has no descendants that contribute bytes. The full dispatch table — every `(alignment_kind, source_info_shape)` pair and its rule — lives in the implementation plan ([Plan 7d](../plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md)).

The dispatch is total: every well-formed input lands at exactly one rule. No catch-all `Rewrite` arm exists; no path leaves a node uncategorized.

## Soundness

> The verified proof (with the multiplicity strengthening and lemmas L1/L2/L3)
> is in [`incremental-writer-bp-proof.md`](incremental-writer-bp-proof.md) §4.
> The per-byte argument below is correct as far as it goes; it is silent on
> multiplicity, which the proof file discharges separately.

**Claim.** For every input `(Source, AST_old, AST_new, Plan)` produced by a q2 pipeline run that satisfies the producer contract, and for every node `n` in `AST_new` at alignment context `α`, the bytes produced by `assemble(plan_user_writes(n, target, α))` satisfy BP.

**Proof.** By structural induction on `n`.

*Base case R1.* The emission is `Source[range]`, where `range = preimage_in(n, target)`. For any position `i ∈ range`, the emitted byte is `Source[i]` — exactly the form (P1) requires. (P1) holds for every emitted byte.

*Base case R2.* The emission is empty. BP holds vacuously.

*Base case R5.* The emission is `serialize_leaf(n)`, which produces `n`'s authored content. R5's precondition — `n` has no descendants that contribute bytes — makes the "excludes descendants' bytes" part of authored content vacuous for `n`, so `serialize_leaf(n)` is the entire emission. (P2) holds. The trust point sits at the producer contract's classification: nodes routed to R5 are those whose source_info attests user authorship; nodes whose source_info is atomic-Generated route to R1' or R2' instead and never reach R5, so the writer never tries to serialize pipeline content as authored content.

*Inductive case R3 / R4.* The emission is `shell_open ++ join(separator, [assemble(c) for c in children]) ++ shell_close`. By the inductive hypothesis, each `assemble(c)` produces bytes satisfying BP, and concatenation preserves BP per byte — every byte of every `assemble(c)` continues to satisfy whichever clause it satisfied before. The remaining bytes are the shells and the separators. Shell bytes satisfy either (P1) — when they come from the original block's prefix/suffix preimage (R4) — or (P2) — when they come from the qmd writer's syntax helper, which emits `n`'s authored content determined entirely by the container kind, with no reference to any descendant (R3). Separator bytes come from either the preserved gap's preimage (P1) or the qmd writer's separator helper (P2), by the same reasoning. Every byte of the full emission therefore satisfies BP.

The induction proceeds on AST size, which is finite, so termination is guaranteed.

The argument depends on the producer contract: a producer that stamps non-atomic source_info on a node whose content is pipeline output will route that node to R5 or R1, and the writer will emit pipeline bytes as if user-authored — satisfying BP's letter but not its spirit. Producer hygiene is the substrate; the writer's dispatch and recursion are the structure built on it. With both contracts in force, BP holds throughout `Source'`. ∎

## Completeness

> The verified proof (with (C1+) "exactly once") is in
> [`incremental-writer-bp-proof.md`](incremental-writer-bp-proof.md) §5.

Soundness rules out *leaks* — pipeline bytes appearing in `Source'`. Completeness rules out *drops* — user-authored bytes failing to appear when they should. Both are necessary: a writer that emits nothing is trivially sound but useless; a writer that emits everything is trivially complete but unsafe. The recursive `plan_user_writes` proves the dual of BP by the same structural induction.

Every byte falls into exactly one of four categories. Two appear in `Source'`; two do not:

> **(C1) Preserved.** For every AST node `n` in `AST_new` with `preimage_in(n, target) = Some(range)`, every byte at every position `i ∈ range` in `Source` appears in `Source'`.
>
> **(C2) Authored.** For every AST node `n` in `AST_new`, if `n` is not at a soft-drop site, every byte of `n`'s authored content appears in `Source'`.
>
> **(R) Refused.** A node `n` is at a *soft-drop site* when the reconciler aligned `n` via `UseAfter` or `RecurseIntoContainer` *and* the editability gate returns "not editable" — `n`'s source_info is atomic-Generated, or `n` is an atomic CustomNode with `RecurseIntoContainer` (interior edit, not picker-replacement). At a soft-drop site, the writer refuses to emit `n`'s authored content; instead, it emits `n`'s preimage (R1') or nothing (R2'), and pushes a `Q-3-42` or `Q-3-43` warning into the diagnostic surface.
>
> **(D) Deleted.** Bytes at positions in `Source` that no AST node in `AST_new` claims via `preimage_in` do *not* appear in `Source'`.

(C1) is the consumer-side dual of (P1): where (P1) says every emitted copy has a known source position, (C1) says every known source position the new AST still references is emitted. (C2) is the consumer-side dual of (P2): where (P2) says every serialized byte traces to a single visited node's authored content, (C2) says every visited node's authored content is serialized. (R) and (D) are the *negative* completeness clauses — bytes the writer correctly does *not* emit. (R) is the soft-drop feature: the writer's principled refusal to emit pipeline-resolved content as if user-authored, communicated to the user via a warning. (D) is the user's intentional removal: bytes deleted from the AST do not appear in `Source'`.

The four categories partition every byte. Every byte in `Source` is either Preserved (still claimed by a surviving node) or Deleted (no longer claimed). Every byte from a user edit is either Authored (emitted at a non-soft-drop site) or Refused (the writer pushed back at a soft-drop site). A soft-drop site simultaneously triggers (C1) for the node's preimage emission and (R) for the refused authored content; the two clauses speak about different byte sets at the same node.

R5-special (let-user-win on atomic CustomNode wholesale replacement) is *not* a soft-drop site. The user replaced the entire node via a component picker — an unambiguous intent — and the writer emits the new node's qmd via `plain_data`. No warning. R5-special falls under (C2) Authored.

**Claim.** For every input `(Source, AST_old, AST_new, Plan)` produced by a q2 pipeline run that satisfies the producer contract, the bytes produced by `assemble(plan_user_writes(root, target, α))` satisfy (C1), (C2), (R), and (D).

**Proof.** By structural induction on `n` in `AST_new`.

*Base case R1.* The emission is `Source[range]` where `range = preimage_in(n, target)`. Every byte at every position `i ∈ range` appears in the emission. (C1) holds for `n`.

*Base case R1' (soft-drop with preimage).* The emission is identical to R1: `Source[range]`. (C1) holds for `n`'s preimage. (C2) does not apply — `n` is at a soft-drop site. (R) is satisfied: the writer refused `n`'s authored content and pushed a `Q-3-43` warning.

*Base case R2.* The emission is empty. By the producer contract, `n` is atomic-Generated with no preimage — pipeline-synthesized content with no authored content. (C1) and (C2) hold vacuously.

*Base case R2' (soft-drop without preimage).* The emission is empty + `Q-3-43` warning. (C1) holds vacuously (no preimage). (C2) does not apply (soft-drop). (R) is satisfied.

*Base case R5.* The emission is `serialize_leaf(n)`. By R5's classification and the producer contract, `n` has authored content; `serialize_leaf` emits it. Every byte of `n`'s authored content appears in the emission. (C2) holds. R5-special falls here (let-user-win via `plain_data` is still Authored, not Refused).

*Inductive case R3 / R4.* The emission is `shell_open ++ join(separator, [assemble(c) for c in children]) ++ shell_close`. By the inductive hypothesis, each `assemble(c)` emits every (C1) Preserved and (C2) Authored byte in `c`'s subtree. Shell and separator bytes appear by construction: R4 takes them from the original block's prefix/suffix preimage (satisfying (C1) at the shell positions); R3 takes them from the qmd writer's syntax helper (satisfying (C2) for the container's authored content). Every byte at every level of `n`'s subtree appears in the emission.

(D) follows from the per-node coverage: every byte in `Source` is either covered by some `n.preimage_in(target) = Some(...)` for `n` in `AST_new` (Preserved, handled by R1 or R1') or it isn't (Deleted, no rule emits it). The set complement is exact.

∎

## What BP and Completeness do not promise

- **Position correctness.** BP says each byte has a defensible origin and completeness says each authored byte appears. Neither says bytes land at the right place in `Source'`. That's `assemble`'s responsibility: separators between entries, shell composition in `Recurse`, gap preservation.

- **Diagnostic fidelity.** Whether the right warnings (`Q-3-42`, `Q-3-43`, future codes) accompany each soft-drop is the diagnostic layer's job. BP and completeness are silent on warning content.

- **Byte-level fidelity of helper-emitted bytes.** Whenever the writer emits bytes via a syntax helper rather than copying from `Source` — R3 shells for block containers being recursed into, list-item markers, ordered-list numbers — the helper produces canonical bytes determined by the container kind, not by the user's original byte choice. If the user's original differs from the canonical (e.g., `-` instead of `*` for bullets, `1. / 1. / 1.` instead of `1. / 2. / 3.` for lazy numbering, `::::` instead of `:::` for Div fences), the user's original bytes don't round-trip — they're replaced by helper output. This is a **completeness gap**: strict (C1) is not preserved at these positions for nodes whose dispatch fires a helper-emitting rule. Soundness still holds (helper output is honest authored content traceable to (P2)); only the byte-level fidelity of the original syntactic choice is lost. The fix requires per-position fidelity tracking in the AST that's out of scope for the writer-side contract. Inline containers handled by R4 do preserve original shells via prefix/suffix preimage.

- **Engine-output boundary enforcement.** When the pipeline's engine-execution stage runs an engine (Knitr, Jupyter) on the AST, the engine returns a new AST that the reconciler treats as ground truth. q2 currently has no post-execute check that the engine modified only the code blocks of languages it claimed. Tracked separately; blocked on the `claims_language` extension to the engine trait.

- **Producer-side hygiene.** Both BP and completeness inherit the producer contract as a precondition. A producer that misclassifies source_info breaks both invariants — non-atomic stamping on pipeline output breaks soundness (the writer emits pipeline bytes as if user-authored); atomic-Generated stamping on user content breaks completeness (the writer refuses to emit content that was actually user-authored). The trust point is narrow (only at R5, where the algebra trusts that a node reaching it has authored content), but it is real, and it is the producer contract's job to honor it.

## References

- Producer-side contract: [`provenance-contract.md`](provenance-contract.md).
- Implementation plan: [`plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](../plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
- Prerequisite framework + test-hygiene work: [`plans/2026-05-29-q2-preview-plan-7f-prereqs.md`](../plans/2026-05-29-q2-preview-plan-7f-prereqs.md).
- CustomNode qmd serialization (post-7d): [`plans/2026-05-29-q2-preview-plan-7e-customnode-qmd.md`](../plans/2026-05-29-q2-preview-plan-7e-customnode-qmd.md).
- Sibling primitive: [`transparent-wrappers.md`](transparent-wrappers.md) — the traversal-side analogue of the writer's emission-side recursion.
