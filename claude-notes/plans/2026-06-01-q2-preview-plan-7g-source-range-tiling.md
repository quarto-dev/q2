# Plan 7g — Source-range tiling (the producer-side precondition BP assumes)

**Date:** 2026-06-01
**Branch:** feature/provenance (sibling to 7d / 7e / 7f)
**Status:** **Research draft.** The next agent should verify the empirical claims (especially the auditor census) and flesh out the implementation phases. Diagnosis and policy are well-grounded; phase-level implementation detail is deliberately light.
**First goal — do this before anything else:** the BP audit (Phase 6) is a **go/no-go gate** for the whole incremental-writer direction. See *Sequencing*. The tiling implementation (the rest of this plan) is wasted effort if BP/completeness can't be proved.
**Ships:** after 7f (complete), **before 7d** — new prerequisite.

## Epic context

| Sibling | Axis | Status |
|---|---|---|
| Plan 7 | Incremental writer + soft-drop + bridge migration | shipped |
| Plan 7a | Runtime user-filter idempotence | open |
| Plan 7b | Test-coverage consolidation | open |
| Plan 7c | Closure gaps in the denylist cascade | open |
| Plan 7d | Algebraic soundness of coarsen/write | ready; **depends on 7g** |
| Plan 7e | CustomNode qmd serialization | ships after 7d |
| Plan 7f | Prereqs for 7d (framework + wire-format) | **complete** |
| Plan 7g | Source-range tiling (producer-side precondition) | **this plan** |

## Why this is a prerequisite to 7d

`incremental-writer-contract.md` states the Byte Provenance Invariant and proves
it by structural induction. The proof — and the contract's own words — assert a
**partition**:

> "The two clauses partition the bytes of `Source'`. The algebra never overlaps
> them; **every byte traces to exactly one visited node.**"

That partition is **tiling**: sibling node source ranges must be disjoint, and a
parent's range must contain its children's. The proof's R3/R4 step
(`shell_open ++ join(sep, [assemble(c) for c in children]) ++ shell_close`)
relies on children's preimages being disjoint, so each source region is emitted
once.

**This precondition is not currently true, and the producer contract does not
require it.** `provenance-contract.md` requires each node's `SourceInfo` to have
an accurate *shape* (`Original`/`Substring`/`Concat`/`Generated`) and that `s:`
is always populated — but says nothing about ranges being tight, disjoint, or
tiling. So producers can (and do) emit **overlapping** sibling ranges while
satisfying the stated contract.

Concrete failure: for `` a `x = 5` b `` the `Space` and the `Code` node *both*
carry range `[1,9]`. Under 7d's recursive assemble, `assemble(Space)` copies
`Source[1..9]` and `assemble(Code)` copies `Source[1..9]` **again** — the code
span is duplicated in `Source'`. The round-trip BP guarantee collapses.

Note the subtlety: the **formal** BP statement (the P1/P2 dichotomy per output
byte) technically survives — both duplicated copies are "P1 Copied." It's the
**partition** (line 71) and **completeness** that break. So BP's formula is also
*too weak*; it never forbids two nodes claiming the same source byte.

Finding one gap between BP's prose (a partition) and its formula (a per-output-
byte dichotomy that permits duplication) means **the formalization has not been
audited from the tiling perspective**, and we should not assume this is the only
gap. Phase 6 re-audits the BP statement and its proofs against the partition
claim — treating the existing proof as *unverified* under tiling until checked.

7g supplies the missing producer-side precondition. Until it lands, 7d's
soundness proof rests on an assumption the producer violates.

## Root cause (already diagnosed; see bd-1d6io investigation)

q2's scanner intentionally keeps whitespace inside token ranges — a shared
indentation preamble (`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c`
≈ 2371) advances over leading whitespace token-inclusively (`advance(lexer,
false)`; the tree-sitter skip flag `advance(lexer, true)` is used **0** times).
Token boundaries are a *lexing* concern. The Rust AST handlers then normalize
whitespace: they peel a leading `Space` node and `trim` the text — but they
compute `source_info` from `node_source_info_with_context(node)`, the **whole,
whitespace-inclusive node range**. So the peeled `Space` and the trimmed content
both inherit the full range and overlap.

This is the core insight: **source info is currently a byproduct of lexing, not
a first-class contract.** The peel-the-`Space` idiom (`citation.rs`,
`note_reference`, `code_span_helpers`, `shortcode`, `quote_helpers`,
`uri_autolink`) was implemented for AST *structure and text* and never for
*ranges*. k-296 (the citation leading-space fix) is direct evidence: it peeled
the Space and trimmed the text but left the ranges whole-node, so citations
still violate the substring invariant today (`Hi @cite` → Space `[2,8]`, Cite
`[2,8]`).

Two histories, one symptom:
- **Code spans are a regression.** At `2b2337be` (2025-10-24) the scanner
  produced a backtick-start delimiter (`code_span_delimiter (0,2)-(0,3)`); the
  2025-10-30/31 inline-parser rewrite routed code spans through the
  whitespace-absorbing path. Was correct, broke.
- **Attribute keys / citations are long-standing** (≥ 2025-08-06): never tight,
  because the substring-invariant contract didn't exist until ~2025-10-26.

The fix is **handler-enforced** (decision confirmed): the AST layer computes
tight ranges. `range_to_source_info_with_context(range, ctx)` already exists to
build `SourceInfo` from an arbitrary range. Decoupling source info from lexer
token boundaries is the structural fix — it makes source info robust to lexer
churn (the very thing that caused the code-span regression).

## The policy (target invariant)

State these in `provenance-contract.md` (Phase 5):

- **P1 Tight ranges.** A node's `source_info` covers exactly the bytes that
  constitute it — its own delimiters included (a code span includes its
  backticks), surrounding whitespace excluded.
- **P2 Whitespace ownership.** Inter-token ASCII whitespace belongs to a `Space`
  node (or block structure) with its own tight range. (Consistent with the
  Unicode-whitespace classification policy.)
- **P3 Symmetry.** Trim both ends — leading *and* trailing. Today's handlers
  only consider leading.
- **P4 Tiling.** Sibling leaf ranges are disjoint; a parent's range contains its
  children's; **no source byte is claimed by two sibling nodes.**

**Scope boundary — do NOT pursue gap-free partition.** BP requires non-overlap +
nesting, not that every byte be owned. Blank lines between blocks, `> ` gutters,
and list indentation are legitimately unowned; BP tolerates them (Deleted/gap
categories). Inventing nodes to own them is a larger, separate, probably-
undesirable goal. **Concat is a documented exception:** a non-contiguous
`Concat` resolves to scattered pieces; `preimage_in` already returns `None` for
it. The *pieces* tile; the Concat node's span is their hull.

## Empirical findings so far (a FLOOR — must be re-measured in Phase 1)

A quick auditor (Original-only, `.c`-tree only) over the 20
`ts-packages/annotated-qmd/examples/*.qmd` found **21 overlaps, 0 nesting
violations**, in two families:
- **Leading-whitespace fold** (most): `Code`, `Cite`, `RawInline`, `Quoted` each
  overlap a preceding `Space`. One root pattern.
- **`Figure` `Plain∩Plain`**: a Figure emits a caption `Plain` and a content
  `Plain` that *both* claim the whole figure range. Distinct synthesis issue.

**This undercounts** — it excluded the common `Substring` (t==1) and all `Concat`
(t==2) pool shapes, never walked `attrS` (so the attribute-key overlap is
invisible — `div-attrs.qmd` reads "clean" despite the known `custom-key`
defect), and didn't check trailing whitespace. The true census is unknown until
Phase 1.

## Sequencing

**Run Phase 6 first.** It is the gate: prove BP *and* completeness hold against
our actual code/design (given the tiling precondition this plan establishes, and
the already-named exceptions). The numbering below is *not* the execution order
— Phase 6 leads. Only if the gate passes does "the plan proper" (Phases 1–5, 7 —
build the auditor, fix the handlers, amend the producer contract, CI-enforce)
become worth doing. The same agent may continue past the gate, or hand off; the
gate's result is the decision point. (No renumbering — Phase 6 is simply first.)

## Phases (research-level; next agent to flesh out)

### Phase 1 — Build the faithful tiling auditor (= the CI enforcement test)
Resolve all three pool shapes (`Original` direct; `Substring` follows
`d`=parent-pool-index + relative `r`; `Concat` unions its piece list), walk the
full AST **including `attrS.kvs` key/value ranges**, and assert: (a) sibling
non-overlap, (b) parent ⊇ child, (c) trailing-whitespace tightness. Reference
implementations exist: Rust `preimage_in` and the TS wire-format reader — this
is transcription, not invention. This artifact is *both* the measurement tool
and the permanent CI property test (the thing whose absence let this hide).

### Phase 2 — Census + Concat decision
Run Phase 1's auditor over a broad corpus (annotated-qmd examples,
`pandoc-match-corpus`, `docs/`). Produce a violation census by node type /
handler. Confirm the leading-whitespace family is the bulk; surface any
`Substring`/`Concat`/attr violations the floor missed. Decide and document the
`Concat` exception precisely.

### Phase 3 — Fix the leading-whitespace family (handler-enforced, TDD)
A shared helper computes a node's tight range from its trimmed content and
carves the leading/trailing whitespace into the `Space`. Rename or flag
`node_source_info_with_context(node)` so its use for a whitespace-carrying node
is a visible smell. Handlers to fix (verify list against Phase 2):
`code_span_helpers`, `citation`, raw-inline (`raw_specifier`/`raw_attribute`),
`quoted_span`, `key_value_specifier` (attr keys/values), `shortcode`,
`uri_autolink`, plus the `node_source_info_with_context(node)` sites in
`treesitter.rs`. Write byte-offset regression tests *first* (inline-code-in-
prose, multi-kv attr, citation, doubled separator whitespace, trailing
whitespace). **Open question:** code spans are a *scanner* regression — decide
whether handler-trim alone suffices (the policy says yes; the handler re-derives
the right range regardless of the loose token) or whether to also restore the
scanner's backtick-start boundary as defense-in-depth. Default to handler-only
unless Phase 2 shows the loose token causes other harm.

### Phase 4 — Figure `Plain∩Plain` duplication
Separate, contained: a Figure's caption `Plain` and content `Plain` share the
whole figure range. Investigate the Figure synthesis path; give each its own
range. Likely small.

### Phase 5 — Producer contract
- Add P1–P4 to `provenance-contract.md` as a **stated BP precondition**, with
  the Concat exception and the explicit "non-overlap, not gap-free" boundary.
- Cross-link from `incremental-writer-contract.md` ("the two are designed in
  pairs" — this is the third party that was never written down).

### Phase 6 — Audit BP + completeness (THE GATE — do this first)
The duplication gap is a symptom: the formal statement does not entail the
partition its prose asserts. **Before any implementation**, rigorously verify
that BP **and** completeness hold against our actual code/design — assuming the
tiling precondition (P4) this plan would establish, and the already-named
exceptions. Do **not** assume the one hole found is the only one.

**Go / no-go.** This audit decides whether the incremental-writer direction is
sound at all:
- **Pass** → expand the now-verified formal statement + proofs into their **own
  file** (e.g. `claude-notes/designs/incremental-writer-bp-proof.md`), and leave
  the **informal presentation** in `incremental-writer-contract.md` (pointing at
  the proof file). Then proceed with the plan proper (Phases 1–5, 7).
- **No-go** → if BP/completeness genuinely break down — *not* reducible to an
  already-named exception (e.g. list punctuation / ordered-list numbering) and
  *not* a new bug we can fix — **stop and report.** A negative result here is a
  sad but **acceptable** outcome: it saves the effort of building tiling
  machinery for a guarantee that could never hold.

**Acceptable gaps only:** (a) exceptions already named in the contract (list
punctuation, numbering, synthetic/Generated nodes, the Concat hull), or (b) new
bugs with a concrete fix. Any other failure is a no-go.

Specific checks to verify or repair:

1. **Sibling disjointness as a stated lemma.** The R3/R4 step "concatenation
   preserves BP per byte" silently assumes children's emitted source regions are
   disjoint; otherwise `join` double-emits. Make this a lemma discharged by the
   producer tiling precondition (P4), not an implicit assumption.
2. **Preimage emission is terminal (nesting non-double-count).** A `Substring`
   node's preimage is a *sub-range of its parent's* — ancestor and descendant
   overlap *by construction*. The writer must never emit an ancestor's
   `Verbatim` preimage *and* recurse into descendants (that emits the nested
   region twice). Confirm R1 is terminal (copies, does not recurse) and state
   this as the nesting-side disjointness property.
3. **Multiplicity in the statement.** Strengthen (BP) so each source byte is
   emitted *at most once* via P1 across the whole walk — not merely "each output
   byte is P1 or P2." Likewise strengthen completeness (C1) from "appears" to
   "appears exactly once."
4. **None-preimage nodes.** Confirm `Concat`(non-contiguous) / `Generated`
   (preimage `None`) route only to non-copying rules, and that a Concat's pieces
   don't double-count with anything that copies. Tie to the producer Concat
   exception.
5. **Re-prove** soundness and completeness under the strengthened statement and
   the now-explicit lemmas. "Preserves BP per byte" is true but insufficient
   once the partition is part of the claim.

Output on pass: the verified proof moved to its own file (informal presentation
stays in `incremental-writer-contract.md`), the new lemmas/premises stated, and
any premise 7d's dispatch must satisfy surfaced back to 7d. Output on no-go: a
short write-up of exactly where the partition fails and why it is not a named
exception or a fixable bug.

### Phase 7 — Wire the property test into CI
Land Phase 1's auditor as a `cargo nextest` test (and/or `cargo xtask verify`
lane) so future range drift fails at the introducing PR. This is what would have
caught bd-1d6io, A, B, and the citation case at introduction.

## Relationship to siblings

- **Plan 7d** depends on this: 7d's BP soundness proof assumes the tiling
  precondition 7g establishes. 7d should not land claiming BP holds until 7g is
  in (or 7g's property test is green).
- **Plan 7f** (complete): provided framework source_info preservation and
  user-edit stamping. 7g is the *producer-range* analogue — 7f made sure nodes
  *carry* source_info; 7g makes sure the ranges *tile*.
- **Plan 7e**: CustomNode serialization. CustomNode interior ranges should be
  audited by Phase 1's tool too.

## Risks / open questions for the next agent

- **Auditor fidelity.** If the auditor's `Substring`/`Concat` resolution diverges
  from `preimage_in`, the census is wrong. Cross-check against the Rust impl.
- **Blast radius of handler changes.** Many existing tests assert byte ranges
  indirectly (snapshots). Expect snapshot churn; each change must be reviewed as
  a *correction*, not a regression. The property test is the backstop.
- **Substring unknowns.** The floor audit skipped Substring entirely; Phase 2
  may surface a class the policy hasn't considered.
- **Scanner vs handler for code spans** (Phase 3 open question above).
- **Concat semantics** — confirm `preimage_in`'s contiguous/None behavior is the
  exception we want to bless, or whether some Concats should be made contiguous.
- **BP proof may need a new premise (feeds back to 7d).** If Phase 6 finds the
  soundness proof depends on a sibling-disjointness or preimage-terminality
  property the dispatch doesn't yet guarantee, 7d's dispatch table — not just
  the contract prose — may need a row-level invariant. Surface findings to 7d.

## References

- Investigation (annotated-qmd instances A/B + citation): beads `bd-1d6io`.
- Consumer contract that needs this: [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md) (the partition claim).
- Producer contract to amend: [`provenance-contract.md`](../designs/provenance-contract.md).
- Helper: `range_to_source_info_with_context` in `crates/pampa/src/pandoc/location.rs`.
- Affected handlers: `crates/pampa/src/pandoc/treesitter_utils/{code_span_helpers,citation,key_value_specifier,quoted_span,uri_autolink,shortcode,raw_specifier}.rs`, `crates/pampa/src/pandoc/treesitter.rs`.
- Prior art (peel-Space-but-not-range): k-296 (citation), `inline_note_reference`.
- Consumer: [`2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
