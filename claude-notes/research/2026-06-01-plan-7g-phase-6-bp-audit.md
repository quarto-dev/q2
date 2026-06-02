# Plan 7g Phase 6 — BP + completeness audit (the go/no-go gate)

**Date:** 2026-06-01
**Plan:** [`2026-06-01-q2-preview-plan-7g-source-range-tiling.md`](../plans/2026-06-01-q2-preview-plan-7g-source-range-tiling.md) §"Phase 6"
**Audits:** [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md) §"The Byte Provenance Invariant — formal statement", §"Soundness", §"Completeness".
**Verdict:** **CONDITIONAL GO.** BP (strengthened with multiplicity) and completeness (strengthened to "exactly once") are *provable*, but **only** under an explicit premise set the contract does not currently state. No unfixable obstruction was found. One substantive, structurally-reachable duplication bug surfaced (Hole α) with a concrete fix; the gate's rules classify "a new bug with a concrete fix" as an acceptable gap, not a no-go.

---

## 1. What the gate asked

Plan 7g §"Why this is a prerequisite to 7d" diagnosed a gap: BP's **prose** asserts a *partition* of `Source'` ("every byte traces to exactly one visited node"), but BP's **formula** is a per-output-byte dichotomy (P1 Copied / P2 Authored) that never forbids two nodes claiming the same source byte. The `` a `x = 5` b `` example (`Space` and `Code` both carry `[1,9]`) satisfies BP's letter — both copies are "P1 Copied" — while duplicating the code span in `Source'`. So the formalization had **not** been audited from the tiling/partition perspective. Phase 6 re-audits BP + completeness against the actual code/design, assuming the tiling precondition (P4) 7g would establish, and decides whether the incremental-writer direction is sound at all.

The five specific checks the plan named (numbered 1–5 in Phase 6) are discharged below as lemmas L1–L3 plus the strengthened statement.

## 2. Sources consulted (grounding, not just the prose)

- `preimage_in` — `crates/quarto-source-map/src/source_info.rs:435`. The actual byte-range resolver. Key facts:
  - `Substring` composes additively **without clamping** to the parent range (`:448`). Containment of a child preimage in its parent is *not* guaranteed by `preimage_in`; it is a producer obligation (→ P4 nesting / Hole β).
  - `Concat` returns `Some(hull)` **iff** pieces are byte-contiguous, else `None` (`:459`). Non-contiguous Concat has no preimage and can never drive a copy.
  - `Generated` walks the `Invocation` anchor only (`:467`).
- The writer's actual shell/separator extraction — `crates/pampa/src/writers/incremental.rs`:
  - `assemble_inline_splice` (`:1271`): `prefix = qmd[block_start..first_inline.start]`, `suffix = qmd[last_inline.end..block_end]`. The shells are the **literal byte-complement of the children's outer hull** — copied `Source` bytes (P1).
  - `assemble_inline_content` (`:1325`): each `KeepBefore` child is copied from its **own** span; multi-inline dedupe coalesces only **consecutive** runs whose `Invocation` anchors are `PartialEq`-equal (`:1364`).
- Shortcode multi-block output — `crates/quarto-core/src/transforms/shortcode_resolve.rs`: `ShortcodeResult::Blocks(Vec<Block>)` (`:74`) splices N **independent sibling blocks** into the parent list, each stamped with the same `Invocation` token via `stamp_block` (`:548`). They are **not** wrapped in a single container ⇒ they are independently alignable (→ Hole α).

## 3. The strengthened statement

The original BP/completeness clauses are per-**output**-byte. The partition prose is a per-**source**-position claim. The two differ exactly on multiplicity. The fix is to add a multiplicity clause keyed on source position:

> **(M) Multiplicity.** Each source position `i ∈ Source` is emitted via (P1) **at most once** across the whole walk.

and to upgrade completeness (C1):

> **(C1+) Preserved exactly once.** For every preserved source position `i` (claimed by some `preimage_in(n, target)` for `n` in `AST_new`), `Source[i]` appears in `Source'` **exactly once** — at least once (old C1) and at most once (M).

The original per-output-byte clauses (P1/P2, C2/R/D) are retained unchanged; (M)/(C1+) are added. "Preserves BP per byte" was true but insufficient once the partition is part of the claim — exactly the plan's check #5.

## 4. Reduction of (M) to two structural lemmas

(M) holds iff the family of preimage ranges actually copied — one per `R1`/`R1'` firing, plus the R4 shell ranges and OriginalGap separator ranges, all of which are P1 copies — is **pairwise disjoint** in `Source`. Let `V_copy` be the set of visited nodes that fire a copying rule. Any two distinct members of `V_copy` are either **ancestor–descendant** or **incomparable** in the AST. Two lemmas cover the two cases.

### L2 — preimage-emission is terminal (no ancestor–descendant double-count) [check #2]

**Claim.** If `n ∈ V_copy`, no descendant of `n` is visited at all.

**Proof.** `R1`/`R1'` package a `Verbatim`/`Omit` and **do not recurse**; only `R3`/`R4` recurse. Dispatch totality (each node fires exactly one rule) ⇒ if `n` fired a copying rule it did not fire `R3`/`R4`, so the walk never descended into `n`'s children. ∎

So `V_copy` contains no ancestor–descendant pair. **Grounded:** `CoarsenedEntry::Verbatim` carries only a `byte_range`, no child entries (`incremental.rs:46`); the 7d design maps `R1 → Verbatim` and `R3/R4 → Recurse` as disjoint arms. L2 is a property the 7d dispatch must preserve; it holds today and in the design.

### L1 — sibling-rooted disjointness (no incomparable double-count) [check #1]

**Claim.** For incomparable `n, m ∈ V_copy`, `preimage(n) ∩ preimage(m) = ∅`.

**Proof.** Let `a = lca(n, m)`; `n` descends into child `c_n` of `a`, `m` into `c_m`, with `c_n ≠ c_m`. By L2, `a ∉ V_copy` (it is a visited ancestor of both), so `a` fired `R3`/`R4` and `c_n, c_m` are siblings. Using **P4 nesting** (`parent ⊇ child`) inductively down each chain, `preimage(n) ⊆ preimage(c_n)` and `preimage(m) ⊆ preimage(c_m)`. Using **P4 sibling-disjointness**, `preimage(c_n) ∩ preimage(c_m) = ∅`. Hence `preimage(n) ∩ preimage(m) = ∅`. ∎

**L1 is exactly where P4 is load-bearing.** Sibling-disjointness is *not* a consequence of `preimage_in`'s structure (a `Code` node's `Original{1,9}` and a `Space`'s `Original{1,9}` are independent — `preimage_in` happily returns `[1,9]` for both). It is the producer precondition 7g establishes. **Without P4, L1 is false and the Space/Code counterexample stands** — confirming the plan's diagnosis that the unstrengthened statement does not entail its own partition prose.

### Crux

**(M) ⟺ L1 ∧ L2 ⟺ P4-tiling ∧ dispatch-terminality.** Both are achievable: P4 by 7g's producer fixes; terminality by 7d's dispatch shape (and it already holds). Neither is an unfixable obstruction. This is the core of the **GO**.

## 5. Adversarial sweep — is {P4, terminality} *sufficient*? (the plan's "do not assume one hole")

P4 + terminality discharge the two clean structural cases. I then hunted for residual holes. Findings, by severity:

### Hole α (SUBSTANTIVE, reachable) — atomic N-to-1 with non-adjacent survivors

A block-level shortcode resolves to **N sibling blocks** (`ShortcodeResult::Blocks`), each carrying `Generated{Invocation: token}`, all with `preimage_in = R` (the *same* token range). These siblings **violate P4 sibling-disjointness by construction** (identical, non-empty preimages). The writer avoids duplication via coalescing — but `assemble_inline_content`'s dedupe (and Plan 7d's Property #9) coalesce only **consecutive** same-`Invocation` runs. If a transform leaves the N survivors **non-adjacent** — e.g. `AST_new = [lip₁, new_para, lip₂, lip₃]` after inserting a block — then `lip₁ → Verbatim(R)` and `lip₂(+lip₃ coalesced) → Verbatim(R)` both fire, emitting the token range **twice**. `Source'` then contains `{{< lipsum 3 >}}` twice; the next pipeline run expands it to 6 paragraphs. **(M) violated, and real document drift.**

- **Reachability:** structurally reachable in the *backend* (independent sibling blocks, not a single grouped unit), but **unreachable through the front end**: block-shortcode output renders as a client read-only region, so no edit affordance can place a cursor between the resolved blocks to insert or reorder. The non-adjacent split that triggers the duplication cannot be produced by any front-end edit as the system is designed.
- **Classification:** acceptable gap (new bug, concrete fix), **not** a no-go.
- **DECISION (2026-06-01, Gordon): held by design, not fixed.** L3 holds for every edit the front end can produce, so (M) holds with it. No work is tracked against L3. The gap reopens only if a non-client edit path is introduced (programmatic edits, a filter that reorders blocks); the fix would then be whole-walk `preimage_in`-equality coalescing (subsumes today's consecutive dedupe) or structural grouping of atomic N-to-1 output.

### L3 — Invocation-coalescing completeness (NEW premise)

Generalizing Hole α: P4 sibling-disjointness must be stated to **exempt** same-`Invocation` atomic N-to-1 groups (they share a preimage by design, like `Concat` pieces share a hull), and the writer must guarantee **single emission** of that shared preimage across the whole walk. This is a third premise alongside P4 and terminality:

> **L3.** For any maximal set `S` of visited nodes sharing a single `Invocation` preimage `R`, the walk emits `R` exactly once.

Today's mechanism (consecutive-only) does **not** satisfy L3. **This is the one place the existing design (Property #9, "consecutive children") is provably insufficient** and must be strengthened in 7d.

### Hole ε (LATENT CRASH under ¬P4) — reversed OriginalGap

`SeparatorRule::OriginalGap` uses `qmd[child_i.end .. child_{i+1}.start]`. Under overlapping siblings (¬P4), `child_i.end > child_{i+1}.start` ⇒ a **reversed slice** ⇒ panic or corruption. So ¬P4 doesn't merely weaken (M) — it can crash the writer. Another reason P4 is load-bearing; also a hint that the *current* (pre-7g) writer has a latent panic on the very overlaps 7g fixes. **Update 2026-06-02:** an 802-file identity sweep proved this specific `compute_separator` block-gap path **unreachable** from real input (top-level blocks tile positionally). The reachable reversed-slice bug was the *inline-splice* analogue — see §8 and 7g Phase 8.

### Hole β (producer obligation) — unclamped Substring

`preimage_in`'s `Substring` arm does not check `end ≤ parent_len`. P4 nesting (child ⊆ parent) is therefore a real producer obligation the Phase 1 auditor must check directly (Substring offsets within parent), not something `preimage_in` enforces. Acceptable — it is precisely what P4 / the Phase 1 auditor exist for.

### Hole γ (documented exception) — non-contiguous Concat

Non-contiguous `Concat` → `preimage = None` → routes to `R3`/`R5`/`R2`, never `R1` ⇒ never copies ⇒ no (M) issue. If serialized via `R5`, its bytes are (P2) authored, not copied — falls under the contract's existing "byte-level fidelity of helper-emitted bytes" non-promise, not a soundness hole. The contiguous `Concat` node copies its hull once (terminal) and is P4-disjoint from siblings as a single node. Both consistent with the named Concat exception.

### Hole δ / shells (resolved under P4) — R4 shells join the disjoint family

R4 shells (`prefix`/`suffix`) and OriginalGap separators are **P1 copies** and must be counted in `V_copy`'s range family for (M). They are the byte-complement of children within a block; under P4 (children tile the interior, blocks tile the document) they are disjoint from children, from each other, and from other blocks' emissions. The proof must state this explicitly — the contract's prose currently classifies shells as "P1 or P2" without accounting for their **multiplicity**.

### Hole ζ (completeness boundary) — non-gap-free P4 is compatible with round-trip

P4 is deliberately **not** gap-free: blank lines between blocks, `> ` gutters, list indentation are legitimately unowned. (D) says unclaimed bytes are dropped — yet the round-trip must still reproduce structural separators. Resolution: structural gaps are reproduced as **(P2)-authored** separator/shell bytes (`SeparatorRule::StandardBlock`, `R3` shell helpers), **not** as (P1) copies; only genuinely content-free bytes are (D)-dropped. This is the (C2)↔(D) boundary and is where "non-overlap, not gap-free" actually gets cashed out — make it explicit in the proof.

## 6. Premise set the proof depends on (and where each lives)

| Premise | Statement | Owner / status |
|---|---|---|
| **P4** | Sibling preimages disjoint; parent ⊇ child. Exceptions: `Concat` hull, atomic N-to-1 same-`Invocation` groups. | **7g producer fix** (this plan). Phase 1 auditor enforces; Phase 5 states it in `provenance-contract.md`. |
| **L2** | `R1`/`R1'` are terminal (emit, don't recurse); only `R3`/`R4` recurse. | **7d dispatch.** Holds today and in the design. State as a lemma. |
| **L3** | Same-`Invocation` shared preimage emitted exactly once across the whole walk (not just consecutive). | **7d dispatch — NEEDS STRENGTHENING.** Property #9 "consecutive" is insufficient. Feeds back to 7d. |

## 7. Verdict and required outputs

**CONDITIONAL GO.** Proceed with 7g (build the auditor, fix handlers, amend the producer contract) and with 7d. The incremental-writer direction is sound *given* {P4, L2, L3}. None is unfixable; P4 is 7g's whole point, L2 already holds, L3 is a bounded strengthening of an existing optimization.

Per Phase 6's "on pass" instructions:

1. **Move the verified statement + proofs into their own file:** [`incremental-writer-bp-proof.md`](../designs/incremental-writer-bp-proof.md) — strengthened (M)/(C1+) clauses, lemmas L1/L2/L3, P4 as a stated premise, re-proved soundness + completeness. *(next step this session)*
2. **Leave the informal presentation** in `incremental-writer-contract.md`, pointing at the proof file. *(next step)*
3. **Surface premises back to 7d:** L3 (generalize Property #9 coalescing to whole-walk preimage-equality) and the explicit shell/OriginalGap multiplicity accounting. Record on Plan 7d's risk/premise tail.

**Outputs NOT triggered** (no no-go writeup): no partition failure was found that is irreducible to a named exception or a fixable bug.

## 8. Disposition of follow-ups (2026-06-01 decisions)

- **L3 / Hole α (atomic N-to-1 duplication):** *held by design, not tracked.*
  Unreachable through the front end (read-only region). See §5 and the proof
  file §6. No bead.
- **Reversed-slice writer panics — RESOLVED 2026-06-02.** There are *two* sites
  in the writer that slice `original_qmd[a..b]` from source-range arithmetic:
  - `compute_separator`'s block-gap (`qmd[prev_block.end .. curr_block.start]`,
    Hole ε): an 802-file **identity sweep** through `incremental_write` fired
    this on every adjacent top-level block pair — **0 panics, 0 real top-level
    overlaps**. **Unreachable** from real input (top-level blocks tile
    positionally); P4 closes it by construction. No dedicated fix/test beyond the
    corpus sweep. The hand-built repro I'd written for it was deleted as it
    guarded a dead path.
  - `assemble_inline_splice`'s prefix/suffix (`qmd[block.start .. inline.start]`):
    **the live bug.** An **edit sweep** + latent-surface scan found ~10 corpus
    files whose first inline is a contiguous `Concat` (`start_offset()`→sentinel
    0), e.g. `Str "Table:"` = `Concat[Original "Table" ++ Original ":"]`. Editing
    such a paragraph reverses the prefix slice → panic
    (`byte range starts at 35 but ends at 0`). **NOT a tiling bug** — the Concat
    is P4-correct; `preimage_in` returns the right hull; the writer just read the
    wrong accessor (`start_offset()` vs `preimage_in`). Fixed under **7g Phase 8**:
    boundaries now from `preimage_in`, with a `Rewrite` fallback when None.
    Regression tests: `inline_splice_concat_led_paragraph_does_not_panic` +
    `incremental_write_never_panics_on_pampa_corpus`.
