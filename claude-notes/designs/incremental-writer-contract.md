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

Let `Source` be the user's qmd file at file identifier `target`, and let `Source'` be the qmd file the writer produces. The invariant binds every byte of `Source'`:

> **(BP)** For every byte `b` in `Source'`, exactly one of the following holds.
>
> **(P1) Copied.** `b = Source[i]` for some position `i ∈ preimage_in(n, target)` for some AST node `n` in the new AST.
>
> **(P2) Authored.** `b` was produced by serializing the *node-local content* of a single AST node `n` — the part of `n`'s qmd serialization that does not include any of `n`'s descendants' bytes. For a leaf node, node-local content is the entire serialization. For a container node, it is the shell syntax: `> ` for a BlockQuote, `:::{.foo}\n` and `:::\n` for a Div, `- ` for a bullet item, the per-child separator. Children's bytes do not arrive through their parent's serialization; they arrive only when the algebra independently visits each child.

The two clauses partition the bytes of `Source'`. The algebra never overlaps them; every byte traces to exactly one visited node.

## The dispatch

The recursive `plan_user_writes` selects one of five rules at each node, by the pair `(alignment_kind, source_info_shape)`. R1 emits a `Verbatim` of the node's preimage when the node has one. R2 emits an `Omit` when the node is atomic-Generated with no preimage. R3 and R4 emit a `Recurse` over a container's children, with shells from the qmd writer's syntax helpers (R3) or from the original block's prefix/suffix bytes (R4). R5 emits a `Leaf` carrying `serialize_leaf(n)` when `n` has no descendants that contribute bytes. The full dispatch table — every `(alignment_kind, source_info_shape)` pair and its rule — lives in the implementation plan ([Plan 7d](../plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md)).

The dispatch is total: every well-formed input lands at exactly one rule. No catch-all `Rewrite` arm exists; no path leaves a node uncategorized.

## Soundness

**Claim.** For every input `(Source, AST_old, AST_new, Plan)` produced by a q2 pipeline run that satisfies the producer contract, and for every node `n` in `AST_new` at alignment context `α`, the bytes produced by `assemble(plan_user_writes(n, target, α))` satisfy BP.

**Proof.** By structural induction on `n`.

*Base case R1.* The emission is `Source[range]`, where `range = preimage_in(n, target)`. For any position `i ∈ range`, the emitted byte is `Source[i]` — exactly the form (P1) requires. (P1) holds for every emitted byte.

*Base case R2.* The emission is empty. BP holds vacuously.

*Base case R5.* The emission is `serialize_leaf(n)`, which produces bytes from `n`'s own immediate content. R5's precondition — `n` has no descendants that contribute bytes — makes the "no descendants' bytes" clause of (P2) vacuously satisfied for every emitted byte. (P2) holds. The trust point sits here: `n`'s content is assumed to represent user-authored bytes. The producer contract is what guarantees this; it stamps atomic kinds on non-user-authored leaves, which route to R1' or R2' rather than R5, so any node that reaches R5 carries a source_info shape compatible with user authorship by construction.

*Inductive case R3 / R4.* The emission is `shell_open ++ join(separator, [assemble(c) for c in children]) ++ shell_close`. By the inductive hypothesis, each `assemble(c)` produces bytes satisfying BP, and concatenation preserves BP per byte — every byte of every `assemble(c)` continues to satisfy whichever clause it satisfied before. The remaining bytes are the shells and the separators. Shell bytes satisfy either (P1) — when they come from the original block's prefix/suffix preimage (R4) — or (P2) — when they come from the qmd writer's syntax helper, which emits node-local syntax determined entirely by the container kind, with no reference to any descendant (R3). Separator bytes come from either the preserved gap's preimage (P1) or the qmd writer's separator helper (P2), by the same reasoning. Every byte of the full emission therefore satisfies BP.

The induction proceeds on AST size, which is finite, so termination is guaranteed.

The argument depends on the producer contract: a producer that stamps non-atomic source_info on a node whose content is pipeline output will route that node to R5 or R1, and the writer will emit pipeline bytes as if user-authored — satisfying BP's letter but not its spirit. Producer hygiene is the substrate; the writer's dispatch and recursion are the structure built on it. With both contracts in force, BP holds throughout `Source'`. ∎

## What BP does not promise

- **Position correctness.** BP says each byte has a defensible origin. It does not say each byte landed at the right place in `Source'`. That's the responsibility of `assemble`: separators between entries, shell composition in `Recurse`, gap preservation.

- **Diagnostic fidelity.** Whether the right warnings (`Q-3-42`, `Q-3-43`, future codes) accompany each soft-drop is the diagnostic layer's job. BP is silent on warnings.

- **Marker-character fidelity on lists and blockquotes.** q2's AST normalizes list-item markers (every bullet marker — `*`, `-`, `+` — collapses to `*`; ordered-list numbers regenerate sequentially from the first item's start, so `1. / 1. / 1.` becomes `1. / 2. / 3.` on round-trip) and consumes blockquote `>` prefixes during parsing. Round-tripping content that exercises these surface-level choices canonicalizes on every write. The fix requires a typed-AST extension carrying per-item source_info on list items, which is tracked as a separate work item and is out of scope for the writer-side contract.

- **Engine-output boundary enforcement.** When the pipeline's engine-execution stage runs an engine (Knitr, Jupyter) on the AST, the engine returns a new AST that the reconciler treats as ground truth. q2 currently has no post-execute check that the engine modified only the code blocks of languages it claimed. Tracked separately; blocked on the `claims_language` extension to the engine trait.

- **Producer-side hygiene.** BP inherits the producer contract as a precondition. If a producer attaches misleading source_info — a synthesized leaf with source_info pointing at someone else's bytes — BP will faithfully emit those bytes per (P1) and the result will be wrong. The trust point is narrow (only at R5, where the algebra trusts that a leaf reaching it represents user-authored content), but it is real, and it is the producer contract's job to honor it.

## References

- Producer-side contract: [`provenance-contract.md`](provenance-contract.md).
- Implementation plan: [`plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](../plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
- Prerequisite framework + test-hygiene work: [`plans/2026-05-29-q2-preview-plan-7f-prereqs.md`](../plans/2026-05-29-q2-preview-plan-7f-prereqs.md).
- CustomNode qmd serialization (post-7d): [`plans/2026-05-29-q2-preview-plan-7e-customnode-qmd.md`](../plans/2026-05-29-q2-preview-plan-7e-customnode-qmd.md).
- Sibling primitive: [`transparent-wrappers.md`](transparent-wrappers.md) — the traversal-side analogue of the writer's emission-side recursion.
