# Byte Provenance Invariant — formal statement and proofs

*The formal core of the incremental-writer contract. The informal
presentation — motivation, the two-contracts framing, the dispatch narrative —
lives in [`incremental-writer-contract.md`](incremental-writer-contract.md);
this file is what that document points at when it says "the proof."*

**Status:** Verified 2026-06-01 by the Plan 7g Phase 6 audit
([`../research/2026-06-01-plan-7g-phase-6-bp-audit.md`](../research/2026-06-01-plan-7g-phase-6-bp-audit.md)).
The statement below is the **strengthened** form: the per-output-byte dichotomy
of the original contract is retained, and a per-source-position **multiplicity**
clause (M) / (C1+) is added. The strengthening is what makes the invariant
entail the *partition* its prose always claimed. The original unstrengthened
statement is true but insufficient — it permits two nodes to copy the same
source byte (the `Space`/`Code` both-`[1,9]` duplication), satisfying its letter
while breaking the partition.

**Depends on three premises** (§2). Two are discharged here as lemmas; the third
(P4 tiling) is the producer precondition established by
[Plan 7g](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md).

---

## 0. Objects

- `Source` — the user's qmd file at file identifier `target`.
- `Source'` — the qmd file the writer produces.
- `AST_new` — the post-edit AST the writer serializes.
- Each AST node `n` carries a `SourceInfo` with four physical shapes
  (`Original`, `Substring`, `Concat`, `Generated`).
- `preimage_in(n, target) : Option<Range<usize>>`
  ([`source_info.rs:435`](../../crates/quarto-source-map/src/source_info.rs)) —
  total, side-effect-free. `Original` → `Some(start..end)` iff in `target`;
  `Substring` → parent's range restricted (composed **additively, unclamped** —
  see P4/§2); `Concat` → `Some(hull)` iff pieces are byte-contiguous else `None`;
  `Generated` → walks the `Invocation` anchor only.
- The writer walks `AST_new` via `plan_user_writes`, producing a tree of
  emission entries that `assemble` folds into `Source'`. The dispatch selects one
  of `R1, R1', R2, R2', R3, R4, R5, R5-special` at each node by
  `(alignment_kind, source_info_shape)`; the full table is in
  [`incremental-writer-contract.md`](incremental-writer-contract.md) §"The
  dispatch" and [Plan 7d](../plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
  Copying rules (emit bytes of `Source`): `R1`, `R1'`, and the **shell/separator
  byte-sources** of `R4` and `OriginalGap`. Recursing rules: `R3`, `R4`.

**Authored content.** For a node `n`, the part of `n`'s qmd serialization
tracing to user authorship: for a leaf, the whole serialization; for a container,
the shell syntax (open, close, per-child separator). Excludes descendants' bytes
(those arrive only when the walk independently visits each child). A node whose
source_info is atomic-`Generated` has *no* authored content (it is pipeline
output) and routes to a non-emitting rule.

## 1. The strengthened invariant

Retained from the original contract, per **output** byte `b ∈ Source'`:

> **(P1) Copied.** `b = Source[i]` for some `i ∈ preimage_in(n, target)` of the
> single node `n` whose visit emitted `b`.
>
> **(P2) Authored.** `b` is a byte of the authored content of the single node
> `n` whose visit emitted `b`.

Each output byte is emitted by exactly one node-visit (the recursion partitions
the output into disjoint regions, one per visit), so the P1/P2 dichotomy is a
genuine partition of `Source'`'s bytes *by emitting visit*. That much the
original statement already gave. The gap was on the **source** side. Added:

> **(M) Multiplicity.** Each source position `i ∈ Source` is emitted via (P1)
> **at most once** across the whole walk. Equivalently: the family of
> preimage ranges actually copied — one per copying-rule firing — is pairwise
> disjoint in `Source`.

(M) is the formal content of "every byte traces to *exactly one* visited node"
read on the source side: no source byte is claimed by two emissions.

Completeness is strengthened symmetrically. Retained: (C2) authored, (R)
refused, (D) deleted (see [contract](incremental-writer-contract.md)
§"Completeness"). (C1) is upgraded:

> **(C1+) Preserved exactly once.** For every source position `i` claimed by
> some `preimage_in(n, target)` for `n ∈ AST_new`, `Source[i]` appears in
> `Source'` **exactly once** — at least once (the original C1) and at most once
> (M).

## 2. Premises

| | Statement | Discharged by |
|---|---|---|
| **P4** | *Tiling.* For visited nodes: sibling preimages are pairwise disjoint, and a parent's preimage contains each child's. **Exceptions:** a contiguous `Concat` node's preimage is the hull of its pieces (the pieces tile; the node is treated as one); a maximal group of nodes sharing one `Invocation` preimage (atomic N-to-1 output) share that preimage by construction and are exempt from sibling-disjointness, with single emission guaranteed by L3. | Producer precondition — [Plan 7g](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md). Note `preimage_in`'s `Substring` arm does *not* clamp to the parent range, so parent⊇child is a real producer obligation the Phase 1 auditor checks, not a consequence of `preimage_in`. |
| **L2** | *Dispatch terminality.* `R1`/`R1'`/`R2`/`R2'` emit and do not recurse; only `R3`/`R4` recurse. Dispatch is total (each node fires exactly one rule). | Lemma §3.1 (proved from the dispatch shape; holds in current code and the 7d design). |
| **L3** | *Invocation-coalescing completeness.* For any maximal set of visited nodes sharing a single `Invocation` preimage `R`, the walk emits `R` **exactly once**. | **Accepted as held by design (2026-06-01 decision); not implemented.** Today's coalescing is consecutive-only and does *not* satisfy L3 in general, but the only producer of splittable N-to-1 output (block shortcodes) renders as a **client read-only region**, so no front-end edit path can leave the survivors non-adjacent — the premise's antecedent is unreachable as the system is designed. Revisit only if a non-client edit path (programmatic edits, filter-driven block reordering) is introduced; the fix would then be whole-walk `preimage_in`-equality coalescing or structural grouping of N-to-1 output. See §6. |

## 3. Lemmas

### 3.1 L2 — preimage-emission is terminal

**Claim.** If a node `n` fires a copying rule, no descendant of `n` is visited.

**Proof.** The dispatch packages `R1`/`R1'` as a terminal `Verbatim` (and
`R2`/`R2'` as a terminal `Omit`); none recurse. Only `R3`/`R4` produce a
`Recurse` that visits children. By totality, `n` fires exactly one rule; if it
fired a copying rule it did not fire `R3`/`R4`, so the walk never descended into
`n`'s children. Hence no descendant of `n` is visited. ∎

**Corollary (C-AD).** The set `V_copy` of nodes that fire a copying rule contains
no ancestor–descendant pair.

### 3.2 L1 — sibling-rooted disjointness

**Claim.** For two incomparable nodes `n, m ∈ V_copy`,
`preimage(n) ∩ preimage(m) = ∅`.

**Proof.** Let `a = lca(n, m)`. Then `n` lies under a child `c_n` of `a` and `m`
under a distinct child `c_m` (`c_n ≠ c_m`, since `n, m` are incomparable). `a` is
a visited ancestor of both `n` and `m`; by C-AD, `a ∉ V_copy`, so `a` fired
`R3`/`R4` and `c_n, c_m` are siblings of one recursion. Applying **P4 nesting**
down each chain `c_n ⤳ n` and `c_m ⤳ m`: `preimage(n) ⊆ preimage(c_n)` and
`preimage(m) ⊆ preimage(c_m)`. By **P4 sibling-disjointness**,
`preimage(c_n) ∩ preimage(c_m) = ∅`. Therefore
`preimage(n) ∩ preimage(m) ⊆ preimage(c_n) ∩ preimage(c_m) = ∅`. ∎

L1 is false without P4: a `Code` node's `Original{1,9}` and a `Space`'s
`Original{1,9}` are independent source_infos, and `preimage_in` returns `[1,9]`
for both. Sibling-disjointness is a producer property, not a structural one.

### 3.3 (M) from L1 ∧ L2 ∧ L3

**Claim.** Under P4, L2, L3, the strengthened multiplicity clause (M) holds.

**Proof.** Let `F` be the family of source ranges copied across the walk — one
range per copying-rule firing. Take two distinct firings with ranges `ρ, σ ∈ F`,
at nodes `n, m`.

- If `n, m` are the same node: a single firing emits one terminal range; the
  shell/separator byte-sources of an `R4`/`OriginalGap` firing (next bullet) are
  accounted there. No self-overlap.
- If `n ≠ m` and both are in `V_copy`: by C-AD they are not ancestor–descendant,
  so they are incomparable; by L1, `ρ ∩ σ = ∅` — *unless* `n, m` belong to a
  shared-`Invocation` group (the P4 exception), in which case L3 guarantees the
  group emits its shared range once, so at most one of `n, m` fires a copy and
  the pair does not arise.
- **Shell and separator copies.** `R4` shells are
  `prefix = Source[block_start .. first_child_start]` and
  `suffix = Source[last_child_end .. block_end]`
  ([`assemble_inline_splice`](../../crates/pampa/src/writers/incremental.rs)`:1287`);
  an `OriginalGap` separator is `Source[child_i_end .. child_{i+1}_start]`. These
  are the byte-complement of the children's hull within the container. Under P4
  (children tile the interior; the container ⊇ children; containers tile the
  document), each such range is disjoint from every child range, from the other
  shell/separator ranges of the same container, and — since containers'
  preimages are themselves P4-disjoint — from every range emitted by any other
  container. Hence they join `F` without overlap.

Every pair in `F` is disjoint; `F` is pairwise disjoint; (M) holds. ∎

(Without P4, `OriginalGap` is not merely non-disjoint: `child_i_end >
child_{i+1}_start` yields a reversed slice — a latent panic. P4 closes this too.)

## 4. Soundness (with multiplicity)

**Claim.** For every input `(Source, AST_old, AST_new, Plan)` from a pipeline run
satisfying the producer contract *and the premises §2*, and for every node `n` in
`AST_new`, the bytes produced by `assemble(plan_user_writes(n, target, α))`
satisfy (P1)/(P2), and the whole-walk emission satisfies (M).

**Proof.** The per-byte (P1)/(P2) part is the original structural induction,
unchanged:

- *R1 / R1'.* Emission is `Source[range]`, `range = preimage_in(n, target)`.
  Every emitted byte is `Source[i]`, `i ∈ range` — (P1).
- *R2 / R2'.* Emission empty — vacuous.
- *R5 (incl. R5-special).* Emission is `serialize_leaf(n)` = `n`'s authored
  content; R5's precondition (no byte-contributing descendants) makes the
  "excludes descendants" clause vacuous — (P2). Trust point: the producer
  contract attests R5 nodes have authored content (atomic-`Generated` nodes route
  to R2'/R1' instead).
- *R3 / R4.* Emission is `shell_open ++ join(sep, [assemble(c)]) ++ shell_close`.
  By IH each `assemble(c)` satisfies (P1)/(P2); concatenation preserves it. Shell
  bytes are (P1) when from the original block prefix/suffix preimage (R4) or (P2)
  when from the qmd writer's syntax helper (R3); separators likewise. Every byte
  satisfies BP.

Termination: induction on finite AST size.

The multiplicity part (M) is **not** per-byte and does not follow from the
induction; it is discharged once, globally, by §3.3 (L1 ∧ L2 ∧ L3). This is the
substantive addition over the original proof: "preserves BP per byte" is true but
silent on whether two nodes copy the same source byte. ∎

## 5. Completeness (exactly once)

**Claim.** Under the producer contract and §2, the emission satisfies (C1+),
(C2), (R), (D).

**Proof.** The "at least once" half of (C1+) and all of (C2)/(R)/(D) are the
original completeness induction, unchanged:

- *R1 / R1'.* Every position in `range` appears — (C1) for `n`'s preimage. R1'
  additionally satisfies (R) (authored content refused, `Q-3-43` pushed); (C2)
  does not apply (soft-drop site).
- *R2 / R2'.* Empty emission; `n` atomic-`Generated`, no preimage — (C1)/(C2)
  vacuous; R2' satisfies (R).
- *R5.* `serialize_leaf(n)` emits all of `n`'s authored content — (C2).
  R5-special falls here (let-user-win via `plain_data` is Authored, not Refused).
- *R3 / R4.* By IH each child emits its (C1)/(C2) bytes; shells/separators appear
  by construction (R4 from original prefix/suffix preimage → (C1) at shell
  positions; R3 from syntax helper → (C2)). (D): a source position is either
  claimed by some `preimage_in` (Preserved, emitted by R1/R1') or not (Deleted,
  no rule emits it) — exact set complement.

The "at most once" half of (C1+) is precisely (M), proved in §4/§3.3. Together:
each preserved source position appears exactly once.

**Boundary note (non-gap-free P4).** P4 is deliberately not gap-free: blank lines,
`> ` gutters, list indentation are legitimately unowned. Such structural gaps are
*not* dropped by (D) — they are reproduced as **(P2)-authored** separator/shell
bytes (`SeparatorRule::StandardBlock`, R3 helpers), so the round-trip is faithful.
Only genuinely content-free bytes are (D)-dropped. This is the (C2)↔(D) boundary
and is where "non-overlap, not gap-free" (Plan 7g §"The policy") is cashed out.
Helper-emitted bytes carry the documented byte-level-fidelity caveat (bullet
marker, lazy numbering, fence-width normalization) — soundness holds (the bytes
are honest authored content), only the original syntactic choice may not
round-trip. ∎

## 6. Known gap held open (acceptable; tracked)

**Hole α — atomic N-to-1 with non-adjacent survivors.** A block-level shortcode
emits N independent sibling blocks (`ShortcodeResult::Blocks`) sharing one
`Invocation`, all with the same preimage `R`. They violate P4 sibling-disjointness
by construction and are covered only by the P4 exception + L3. **L3 is the load-
bearing premise here, and today's writer does not satisfy it:** the dedupe in
[`assemble_inline_content`](../../crates/pampa/src/writers/incremental.rs)`:1364`
and Plan 7d's Property #9 coalesce only *consecutive* runs. If a transform leaves
the survivors non-adjacent (insert a block between two resolved paragraphs), `R`
is emitted twice and the shortcode token is duplicated in `Source'`, expanding on
the next render. Structurally reachable; currently masked (not prevented) by the
React read-only-region gate, which the backend invariant must not rely on.

This is an acceptable gap per the gate's rules (a new bug with a concrete fix),
not a partition failure.

**Decision (2026-06-01): held by design, not fixed.** The antecedent — a
non-adjacent split of the N survivors — is unreachable through the front end:
block-shortcode output renders as a client read-only region, so the user cannot
place a cursor between the resolved blocks to insert or reorder. L3 therefore
holds for every edit the system can actually produce, and (M) holds with it. The
gap reopens only if a non-client edit path is added (programmatic edits, a filter
that reorders blocks); at that point the fix is whole-walk `preimage_in`-equality
coalescing or structural grouping of atomic N-to-1 output. No work is tracked
against it until such a path exists.

## 7. References

- Informal presentation: [`incremental-writer-contract.md`](incremental-writer-contract.md).
- Producer precondition (P4): [Plan 7g](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md).
- Audit that produced this file: [`../research/2026-06-01-plan-7g-phase-6-bp-audit.md`](../research/2026-06-01-plan-7g-phase-6-bp-audit.md).
- Dispatch table / writer obligations (L2, L3): [Plan 7d](../plans/2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
- Code: `preimage_in` ([`source_info.rs:435`](../../crates/quarto-source-map/src/source_info.rs)); writer ([`incremental.rs`](../../crates/pampa/src/writers/incremental.rs)).
