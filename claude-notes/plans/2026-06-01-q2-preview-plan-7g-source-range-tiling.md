# Plan 7g — Source-range tiling (the producer-side precondition BP assumes)

**Date:** 2026-06-01 (research) → 2026-06-02 (converted to development plan)
**Branch:** feature/provenance (sibling to 7d / 7e / 7f)
**Status:** **Development plan.** The go/no-go gate (Phase 6) is **PASSED** — BP and completeness are provable under `{P4, L2, L3}` (proof in `incremental-writer-bp-proof.md`). Two scope-adjacent writer/postprocess bugs found while pulling the gate's thread are **fixed and committed** (Phase 8). The tiling work proper — build the auditor, run the real census, fix the handlers, amend the producer contract, CI-enforce — is **not started**; that is the remaining development work, broken into checklists below.
**Ships:** after 7f (complete), **before 7d** — new prerequisite. **Note:** the Phase 6 *gate* passing does **not** by itself unblock 7d. 7d's "BP holds" claim requires 7g's property test (Phase 1/7) green, which is still open work. See *Relationship to siblings*.

## Phase status (development checklist)

- [ ] **Phase 1** — Build the faithful tiling auditor (= the CI property test)
- [ ] **Phase 2** — Census + Concat decision over a broad corpus
- [ ] **Phase 3** — Fix the leading-whitespace family (handler-enforced, TDD)
- [ ] **Phase 4** — Figure `Plain∩Plain` duplication
- [ ] **Phase 4b** — Faithful range for whitespace-gap `None`-Concats (abbreviation-coalesce is the first instance; resolves the Phase 8 open question)
- [ ] **Phase 5** — Amend the producer contract (P1–P4)
- [x] **Phase 6** — Audit BP + completeness (THE GATE) — **PASSED (CONDITIONAL GO), 2026-06-01**
- [ ] **Phase 7** — Wire the property test into CI
- [x] **Phase 8** — Writer crash on Concat/Generated-led inline + `did_coalesce` sibling bug — **fixed & committed 2026-06-02**
- [ ] **Closeout** — full `cargo xtask verify` (workspace + WASM leg) green; the Phase 8 commits touch `quarto-source-map`, so a pampa-only suite is **not** sufficient evidence under the push policy

**Execution order:** Phase 6 was the gate and is done. Remaining order is **1 → 2 → 3 → 4 → 4b → 5 → 7**, with the Closeout verify before any push.

**Pre-implementation review (2026-06-03).** Before building the Phase 1 auditor,
a source-vs-plan review hardened the forward-looking specs (the retrospective
analysis and the gate held up). Incorporated: P4 corrected to carry **both**
audit exceptions (`Concat` hull + atomic N-to-1 same-`Invocation` groups) rather
than the strictly-stronger wording; Phase 1 auditor now resolves **all four**
`SourceInfo` variants (added `Generated`), groups same-`Invocation` siblings into
one unit, treats `None` as "no claim / skip," checks parent⊇child as an
*independent* containment assert (Hole β — `preimage_in` doesn't clamp
`Substring`), defines tightness as a **boundary-byte predicate on source text**
(not a node-text comparison), and guards the `Attr` sidecar walk with the
`kvs.len() == attr_source.attributes.len()` check (bd-3aolj/bd-1e6a5). P2 marked
a producer obligation, not an auditor check. Phase 3 detection strategy made
**auditor-led, not rename-led** (call-site count corrected: 60 in
`treesitter_utils/`, 83 in `pampa/src`).

**Pre-implementation review, round 2 (2026-06-03).** A second source-vs-plan pass
resolved the remaining ambiguities and one latent design question. Decided:
(#1 / #6-gap) tightness and hull-gap "owned whitespace" are **space/tab only — a
newline at a boundary or in a gap is *not* owned** (so a newline boundary is not a
tightness violation, and a newline-spanning `Concat` gap makes the node
genuinely-scattered → blessed `None`); (#2) the auditor groups same-`Invocation`
siblings with the **same `PartialEq`-on-anchor predicate the writer uses**
(`incremental.rs` ~1357), sharing the helper; (#3) checks (a) non-overlap and (b)
containment run at **all** AST levels, tightness (c) at **inline-leaf only**, and
`⊆` is **non-strict** (a same-`Invocation` child range may equal its parent's);
(#6) the **semantic-ownership rule** sorts `None`-Concats — whitespace-only
inter-piece gap → producer bug to fix with a contiguous hull; gap containing other
content → genuinely scattered → blessed `None`. Two repercussions fold back into
the phases: the auditor now **flags** whitespace-gap `None`-Concats (R1, Phase 1)
instead of silently tallying them, and hull-emission **re-verifies the gap is
whitespace-only before acting** (R2, Phase 3/4b) so it can never manufacture an
overlap. P4's two qualifiers are also re-framed as *intra-node* (`Concat` hull —
not a sibling exception) vs *inter-node* (atomic N-to-1 — the only genuine
overlap), so Phase 5 writes them as distinct categories.

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
  Unicode-whitespace classification policy.) **P2 is a *producer obligation*,
  not an auditor check.** It is discharged by the Phase 3 shared helper (which
  carves peeled whitespace *into* an adjacent `Space`), and the auditor enforces
  its observable consequence — P3 tightness. A direct P2 coverage check ("every
  inter-token whitespace byte is owned by some `Space`") is **deliberately not
  built**: it would require distinguishing must-own inter-token whitespace from
  legitimately-unowned structural whitespace (blank lines, `> ` gutters,
  indentation), i.e. the gap-free classification this plan declines (scope
  boundary below). P3 + P4 already pin down "no content node bleeds into
  whitespace," which is the load-bearing half.
- **P3 Symmetry.** Trim both ends — leading *and* trailing. Today's handlers
  only consider leading.
- **P4 Tiling.** Sibling leaf ranges are disjoint; a parent's range contains its
  children's; **no source byte is claimed by two sibling nodes**, qualified by the
  two refinements below.

**P4's two refinements are *different kinds* of thing — the contract (Phase 5)
must keep them separate** (carried from the Phase 6 audit's premise table — the
plan's earlier "no source byte is claimed by two sibling nodes" was strictly
stronger than the P4 the audit actually proved, and is corrected here). The
earlier draft lumped both as "blessed same-preimage groups," which conflated an
*intra-node* definition with an *inter-node* exception:

1. **(Intra-node — NOT a sibling-disjointness exception) the `Concat` hull.** A
   `Concat`'s *pieces* tile internally and the node presents as **one unit** to
   its siblings — exactly one claim, never two. A *contiguous* `Concat` presents
   its hull; a *non-contiguous* one makes no contiguous claim at all
   (`preimage_in` → `None`; see the semantic-ownership rule below for which of
   those are bugs vs. genuinely scattered). This item only *defines what one
   sibling's claim is* when that sibling is a `Concat`; it does not let two
   siblings share a byte.
2. **(Inter-node — the only genuine overlap exception) atomic N-to-1
   same-`Invocation` groups.** One source construct (e.g. a
   block shortcode `{{< lipsum 3 >}}` occupying range `R`) expands to N sibling
   nodes, each stamped `Generated { from: [Invocation -> token@R] }`
   (`crates/quarto-core/src/transforms/shortcode_resolve.rs` `stamp_block` :624,
   anchor ~:781). All N resolve via
   `preimage_in`'s `Generated` arm to the **same** range `R`, so by the literal
   sibling-disjointness rule they maximally overlap. The Phase 6 audit
   (§5 Hole α / L3, premise table P4) established this sharing is **acceptable
   by design** — the writer coalesces same-`Invocation` runs and emits `R` once,
   and the front end cannot split the group (read-only region). The **auditor is
   a different consumer of P4 than the writer**: it is a static structural pass
   with no notion of edits or emission order, so it would flag this group as
   N−1 overlaps unless the exception is encoded. The auditor must therefore
   **partition siblings by `Invocation`-anchor identity, collapse each
   same-`Invocation` group to its shared range, and check disjointness *between
   units*** — exactly as `Concat` pieces are one unit to siblings. This keeps
   the auditor, the writer, and the contract agreeing on what "one claim" means.

**Scope boundary — do NOT pursue gap-free partition.** BP requires non-overlap +
nesting, not that every byte be owned. Blank lines between blocks, `> ` gutters,
and list indentation are legitimately unowned; BP tolerates them (Deleted/gap
categories). Inventing nodes to own them is a larger, separate, probably-
undesirable goal.

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

**(Resolved — Phase 6 is done; verdict CONDITIONAL GO. The current execution
order lives in *Phase status* at the top. This section is retained as the
historical rationale for why the gate led.)**

**Run Phase 6 first.** It is the gate: prove BP *and* completeness hold against
our actual code/design (given the tiling precondition this plan establishes, and
the already-named exceptions). The numbering below is *not* the execution order
— Phase 6 leads. Only if the gate passes does "the plan proper" (Phases 1–5, 7 —
build the auditor, fix the handlers, amend the producer contract, CI-enforce)
become worth doing. The same agent may continue past the gate, or hand off; the
gate's result is the decision point. (No renumbering — Phase 6 is simply first.)

## Phases

### Phase 1 — Build the faithful tiling auditor (= the CI enforcement test)

**Which representation.** Build the auditor **Rust-side, over the in-memory
`Inline`/`Block` AST**, resolving each node's range with
`SourceInfo::preimage_in(target)` (`crates/quarto-source-map/src/source_info.rs:435`)
— one resolver that already handles all three shapes (`Original`, `Substring`,
`Concat`), so you do **not** transcribe the pool-index arithmetic yourself. The
`d`=parent-pool-index + relative-`r` / `Concat`-piece-union description is how the
**TS wire-format reader** (`ts-packages/annotated-qmd/src/source-map.ts`) does the
same resolution from the *serialized* pool; that file is the cross-check
reference (auditor-fidelity risk below), **not** what we build here. CI runs the
Rust side.

**Home.** Land it alongside the existing corpus test in
`crates/pampa/tests/integration/incremental_writer_tests.rs` (the
`incremental_write_never_panics_on_pampa_corpus` neighbor from Phase 8), so the
Phase 7 property test can call the same function. (Mild discoverability quibble:
this is a *producer*-range auditor living in a file named for the *writer*. That
is deliberate — it shares the corpus harness and Phase 7 calls the same function —
but give the function a producer-oriented name, e.g. `audit_source_range_tiling`,
so a reader grepping for tiling finds it.)

Walk the full AST **including `Attr` kvs key/value ranges**, and assert: (a)
sibling non-overlap, (b) parent ⊇ child, (c) leading- and trailing-whitespace
tightness. This artifact is *both* the measurement tool and the permanent CI
property test (the thing whose absence let this hide).

- [ ] Write the auditor as a Rust function over a parsed AST that, for each node,
      resolves its `SourceInfo` to a concrete source range via the **same** path
      as `preimage_in` (do not reimplement — call/share it) for **all four
      variants**: `Original`, `Substring`, `Concat` (union of pieces), **and
      `Generated`** (walks the `Invocation` anchor). `Generated` was omitted from
      the earlier draft's "three shapes"; it must be resolved because (a) skipping
      it makes the auditor *unsound* — two unrelated `Generated` nodes mapping to
      overlapping ranges would be a real tiling bug the auditor would never see —
      and (b) it carries the same-`Invocation` groups that P4's inter-node
      exception (refinement 2) turns on (next item).
- [ ] **Same-`Invocation` unit grouping (encodes P4 refinement 2).** Before the
      sibling-disjointness check, partition siblings by `Invocation`-anchor
      identity; collapse each maximal same-`Invocation` group to its single shared
      range and check disjointness *between units*, not between raw nodes. **Use
      the *same* grouping predicate the writer uses — `PartialEq`-equality on the
      `Invocation` anchor `SourceInfo`** (the writer's multi-inline dedupe at
      `crates/pampa/src/writers/incremental.rs` ~1357 compares `Invocation`
      anchors with `PartialEq`). Do **not** invent a separate notion of "identity"
      (Arc-pointer equality, or resolved-range equality): the whole point of this
      grouping is that the auditor, the writer, and the contract agree on what
      "one claim" means, so the auditor must use the writer's predicate — ideally
      factor a shared helper so the two cannot drift. This is the static-checker
      analogue of the writer's same-`Invocation` coalescing, and it mirrors how
      `Concat` pieces are treated as one unit to siblings. Without it the auditor
      false-positives on every block-shortcode expansion and would fail CI
      (Phase 7) on any corpus file containing one.
- [ ] **`None`-preimage rule (with the semantic-ownership split).** A node whose
      `SourceInfo` resolves to `None` makes *no contiguous source claim*, so it
      cannot overlap, cannot violate containment, and has no boundary to test —
      **it is excluded from the three tiling checks (a)/(b)/(c).** But `None` is
      not monolithic; the auditor must sort it:
      - **`Generated` with no resolvable `Invocation`** → skip + low-severity
        census tally. No contiguous claim is recoverable.
      - **Non-contiguous `Concat`** → **descend into `pieces`**, resolve each piece
        via `preimage_in` (pieces are `Original`/`Substring`, so they *do*
        resolve), sort the piece-ranges, and inspect the inter-piece gap bytes in
        the source text (the auditor already holds the source bytes for the
        tightness predicate):
        - **gap is whitespace-only** (space/tab; a newline in the gap counts as
          *not* owned → falls to the next bullet) → **flag as a distinct
          `whitespace-gap-concat` finding** (a producer bug whose fix is a
          contiguous hull — Phase 4b's template). This is the class that was
          previously invisible to *both* the auditor and the violation census;
          surfacing it here is the R1 repercussion of the semantic-ownership rule.
        - **gap contains non-whitespace (another node's content) or a newline** →
          genuinely scattered → blessed `None`-skip + census tally.
      Keep the `whitespace-gap-concat` findings **out of the initial gate** (count
      unknown until Phase 2; treat like the leading-whitespace family — report
      first, gate after the fixes drive it to zero). (A node that *ought* to carry
      a tight `Original`/`Substring` but resolves `None` is a missing-source-info
      defect — Plan 7f's domain, not 7g's tiling gate.)
- [ ] Walk the full AST including `Attr` kvs key **and** value ranges (the floor
      audit never walked attributes, which is why `div-attrs.qmd` read "clean"
      despite the known `custom-key` defect). The per-kv ranges live in
      `AttrSourceInfo.attributes: Vec<(Option<SourceInfo>, Option<SourceInfo>)>`
      (`quarto-pandoc-types/src/attr.rs:55`), a **positionally-keyed sidecar** to
      the `kvs` map. **Apply the alignment guard the type's own doc prescribes
      (`attr.rs:44-48`):** before zipping, assert
      `kvs.len() == attr_source.attributes.len()` (and the analogue for classes);
      on mismatch, **skip attr-range auditing for that node and emit a census row
      `attr-alignment-skipped (bd-3aolj/bd-1e6a5)`** rather than passing silently
      or fabricating an overlap. Treat a per-kv `None` `SourceInfo` as "no claim"
      (skip). Do **not** try to re-derive the kv→range mapping — that is
      bd-3aolj/bd-1e6a5's job, not the auditor's.
**Per-level check matrix (which check runs at which AST level):** (a) and (b) run
at **every** level (block siblings *and* inline siblings; block→inline and
inline→inline containment) — the Figure `Plain∩Plain` defect (Phase 4) is a
block-level sibling overlap, so block-level (a) is load-bearing. (c) tightness
runs at the **inline-leaf level only** (block ranges legitimately abut
newlines/blank lines). State this matrix explicitly in the auditor's doc comment.

- [ ] Assert (a) sibling non-overlap (between *units*, per the grouping rule), at
      **all** levels (block and inline siblings).
- [ ] Assert (b) parent ⊇ child as an **independent containment check**, at all
      levels — compute parent and child ranges separately and assert
      `child ⊆ parent`. **`⊆` is non-strict** (equality is allowed): a
      same-`Invocation` child's resolved range *equals* its parent block's range,
      and a single-child container's range may equal its child's — both satisfy
      containment. Do **not** assume `preimage_in` enforces it. (`preimage_in`'s
      `Substring` arm composes offsets *without clamping* to the parent — Phase 6
      Hole β, `source_info.rs:448-449` — so a runaway `Substring` yields a
      silently-too-large range, not `None`; only a real containment assert catches
      it.)
- [ ] (c) **Tightness as a boundary-byte predicate on the source text** (P1/P3),
      **inline-leaf level only**. For an inline-leaf node with non-empty resolved
      range `[s, e)` in file `F`: **violation iff `source[s]` is `' '`/`'\t'` OR
      `source[e-1]` is `' '`/`'\t'`.** **Whitespace for this check is space/tab
      only — a newline at a boundary is *not* a violation** (decided 2026-06-03,
      round-2 review; matches the hull-gap "owned whitespace" rule in Phase 4b so
      the two cannot drift). Do **not** compare the range's bytes against the
      node's *text* — normalization (smart quotes, entity/escape decoding) makes
      source bytes legitimately differ from node text, so a text-equality check
      would false-fail on every normalized node. Caveats to honor: empty ranges
      (`s == e`) are vacuously tight; skip the predicate where the range is `None`
      and for same-`Invocation` groups.
- [ ] Emit, on violation, a structured report: file, node type, both ranges, the
      overlapping bytes — usable both as a census row and as a test-failure message.
- [ ] Cross-check the auditor's `Substring`/`Concat`/`Generated` resolution against
      `preimage_in` on a handful of known cases before trusting the census
      (auditor-fidelity risk).

### Phase 2 — Census + Concat decision
Run Phase 1's auditor over a broad corpus (annotated-qmd examples,
`pandoc-match-corpus`, `docs/`). Produce a violation census by node type /
handler. Confirm the leading-whitespace family is the bulk; surface any
`Substring`/`Concat`/attr violations the floor missed. Decide and document the
`Concat` exception precisely.

- [ ] Run the auditor over the corpora: `ts-packages/annotated-qmd/examples/*.qmd`
      (20 files), `crates/pampa/tests/pandoc-match-corpus/`, and `docs/`.
- [ ] Produce a violation census grouped by node type / originating handler; record
      it in this plan (replaces the FLOOR numbers in *Empirical findings so far*).
- [ ] Confirm the leading-whitespace family is the bulk and that the `Figure
      Plain∩Plain` family (Phase 4) is distinct.
- [ ] Surface any `Substring` (t==1) or `attrS` violations the floor audit could not
      see — decide whether they need a new policy class (open risk: *Substring unknowns*).
- [ ] **Attr coverage is conditional on bd-3aolj / bd-1e6a5.** Tally the
      `attr-alignment-skipped` census rows (Phase 1 guard). If a non-trivial
      fraction of corpus attrs hit the misalignment, that is the signal to
      prioritize bd-3aolj/bd-1e6a5 *before* trusting the attr census numbers —
      surface it as a finding; do **not** absorb those fixes into 7g.
- [ ] Decide and document the `Concat` exception precisely (pieces tile; the node's
      span is their hull; `preimage_in` returns `None` for non-contiguous). Confirm
      against `preimage_in`'s actual contiguous/None behavior.
- [ ] **Freeze the authoritative handler-fix list for Phase 3 from the census** (the
      list in Phase 3 is a verified-by-inspection starting point, not the census).

### Phase 3 — Fix the leading-whitespace family (handler-enforced, TDD)
A shared helper computes a node's tight range from its trimmed content and
carves the leading/trailing whitespace into the `Space`.

**Detection lever — two patterns, not one.** The overlap is produced by two
distinct idioms, so a single grep for `node_source_info_with_context(node)` is
**insufficient** (verified 2026-06-02):

1. **Whole-node-helper sites** — peel a `Space`, trim the text, but build
   `SourceInfo` from `node_source_info_with_context(node)` (the whole,
   whitespace-inclusive range).
2. **Manual-offset sites** — compute ranges directly from `node.start_byte()`
   (e.g. `uri_autolink.rs`, `shortcode.rs`); these never call the helper, so a
   rename/flag lever does **not** surface them. They must be found from the
   Phase 2 census, not from a helper grep.

**The Phase 1 auditor — not a rename — is the authoritative detector for *both*
patterns.** It works on *resolved ranges*, not on which helper a handler called,
so it catches violations regardless of idiom; it is precise (flags actual
overlaps, not mere uses of a function); and it is already wired into Phase 2's
census and Phase 7's CI gate.

**Do not rename `node_source_info_with_context`.** The earlier draft called it a
"20+ call site" lever; the real count is **60 call sites inside
`treesitter_utils/` alone, 83 across `pampa/src`** — and the overwhelming
majority are *legitimate* (nodes like headings, fenced code, and thematic breaks
whose whole-node range *is* the correct tight range, no surrounding whitespace to
peel). A blanket rename would force an atomic 83-site change and flood the diff
with churn on call sites that were never wrong, *burying* the handful of real
pattern-(1) fixes. If a compile-time smell is still wanted, introduce a
**narrowly-named helper** (e.g. `tight_source_info_for_trimmed_node`) that the
Phase 3 fix routes the whitespace-peeling handlers through *as they are fixed*,
leaving the legitimate callers of `node_source_info_with_context` untouched — so
the "smell" is the *remaining* use of the old helper on a whitespace-carrying
node, introduced incrementally rather than as one big rename.

**Handlers to fix — verified-by-inspection starting point (2026-06-02); the
authoritative list is the Phase 2 census.** The earlier draft named three files
that do not exist (`key_value_specifier.rs`, `quoted_span.rs`, `raw_specifier.rs`);
corrected to the real tree:

- `code_span_helpers.rs` — code spans **and** raw-inline (`raw_specifier`/
  `raw_attribute` handling lives here, not in a separate file). Uses the helper. *(pattern 1)*
- `citation.rs` — uses the helper. *(pattern 1)*
- `commonmark_attribute.rs` (+ `span_link_helpers.rs`) — attribute keys/values
  (this is where `key_value_specifier` work actually lives). *(verify pattern in Phase 2)*
- `quote_helpers.rs` — quoted spans (not `quoted_span.rs`). *(pattern 1)*
- `shortcode.rs` — computes `Space` ranges manually from `node.start_byte()`. *(pattern 2)*
- `uri_autolink.rs` — manual offset computation, does **not** call the helper. *(pattern 2)*
- the `node_source_info_with_context(node)` sites in `treesitter.rs`. *(pattern 1)*

Write byte-offset regression tests *first* (inline-code-in-prose, multi-kv attr,
citation, doubled separator whitespace, trailing whitespace).

- [ ] Add a shared helper that derives a node's tight range from trimmed content and
      carves leading **and** trailing whitespace into the adjacent `Space` (P3 symmetry).
- [ ] (Optional smell aid — *not* a rename.) If wanted, add a narrowly-named
      helper (`tight_source_info_for_trimmed_node`) and route whitespace-peeling
      handlers through it as they are fixed; leave the 60+ legitimate
      `node_source_info_with_context` callers untouched. The Phase 1 auditor is
      the authoritative detector for both patterns regardless.
- [ ] Write failing byte-offset regression tests first, then fix each handler in the
      Phase-2-frozen list one at a time (TDD), re-running the Phase 1 auditor after each.
- [ ] Re-run the Phase 1 auditor over the corpus to confirm the leading-whitespace
      family is driven to zero (catches both pattern-1 and pattern-2 sites).

**On the reversed-slice writer panics (resolved 2026-06-02):** the
Phase 6 audit's `compute_separator` block-gap concern (slicing
`qmd[prev_block.end .. curr_block.start]`) turned out to be **unreachable** from
real input — an 802-file identity sweep through `incremental_write` fired that
branch on every adjacent top-level block pair with **0 panics** (top-level blocks
tile positionally). P4 closes it by construction; no dedicated test needed beyond
the corpus sweep. The *reachable* reversed-slice bug was a different site —
`assemble_inline_splice`'s prefix on a `Concat`-led inline — now fixed under
**Phase 8** below. **Decided (2026-06-03): handler-only.** Code spans are a
*scanner* regression, but the handler re-derives the correct tight range
regardless of the loose token (the whole "decouple source-info from lexing"
thesis), so handler-trim alone suffices. Restoring the scanner's backtick-start
boundary as defense-in-depth is a **non-blocking follow-up that reopens only if
Phase 2 shows the loose token causes harm *other than* ranges.** See the Resolved
list under *Risks / open questions*.

### Phase 4 — Figure `Plain∩Plain` duplication
Separate, contained: a Figure's caption `Plain` and content `Plain` share the
whole figure range. Investigate the Figure synthesis path; give each its own
range. Likely small.

- [ ] Locate the Figure synthesis path that emits caption `Plain` + content `Plain`
      (Figure handling lives in `crates/pampa/src/pandoc/treesitter_utils/postprocess.rs`).
- [ ] Write a failing byte-offset test asserting the two `Plain` ranges are disjoint.
- [ ] Give each `Plain` its own tight range; re-run the Phase 1 auditor to confirm.

### Phase 4b — Faithful range for whitespace-gap `None`-Concats (abbreviation-coalesce is the first instance)

**Generalized (round-2 review, 2026-06-03).** This phase was originally scoped to
abbreviation-coalesce alone. Under the semantic-ownership rule, abbreviation-
coalesce is just the *first* member of a class: any producer that emits a
`None`-resolving `Concat` whose inter-piece gap is **whitespace-only** (the
`whitespace-gap-concat` finding the Phase 1 auditor now flags, R1). The fix
technique below is the **template for the whole class** — factor it as a reusable
helper, and apply it to every site Phase 2's census surfaces, abbreviation-
coalesce being the worked example. Sites whose gap contains *other content* (or a
newline) are genuinely scattered and stay blessed `None` — do **not** hull them
(see the safety guard below).

Separate, contained, same faithfulness thesis as the rest of 7g. Pandoc keeps an
abbreviation glued to the following word with a **non-breaking space** (U+00A0);
`coalesce_abbreviations` (`crates/pampa/src/pandoc/treesitter_utils/postprocess.rs:596`)
implements this, merging `Str "Dr."` + `Space` + `Str "Smith"` into a single
`Str "Dr.\u{00A0}Smith"`. **The text behavior is correct (Pandoc compat) and must
not change — only the `source_info` is wrong.**

Today the merged node's `source_info` is `start_info.combine(&end_info)`
(`:656-661`) = a 2-piece `Concat[Original[0,3), Original[4,9)]` for `Dr. Smith`.
The original space `[3,4)` is swallowed by the coalesce and covered by neither
piece, so the `Concat` is **non-contiguous → `preimage_in` returns `None`**
(`source_info.rs:451-466`). Consequences:

- **Writer damage.** `None` preimage ⇒ the merged token can't be Verbatim-copied
  ⇒ editing any paragraph with "Dr. Smith" forces the lossy `Rewrite` path for
  the whole block. This is exactly the writer-quality loss 7g exists to remove.
- **The auditor *does* catch it — but only after the round-2 R1 refinement.** The
  earlier draft's blanket `None`-skip rule would have missed it: a node with no
  preimage was excluded from all checks, so a corpus full of abbreviations would
  report auditor-green while silently degrading the writer. The semantic-ownership
  rule fixes this — the Phase 1 auditor descends into the `Concat` pieces, sees a
  whitespace-only gap, and emits a `whitespace-gap-concat` finding. This is *why*
  the refinement matters and why this class belongs in the plan checklist (it is
  now a first-class, mechanically-detected finding) rather than a follow-up bead
  that would never resurface.
- The current `combine` is **already unfaithful**: it joins only the *first* and
  *last* coalesced tokens, so a 3-token chain (`Dr. Smith Jr.`) drops the middle
  word's provenance entirely. There is no precise per-word map to preserve.

**Fix.** Keep the nbsp text; replace the merged `source_info` with a **contiguous
hull over the whole coalesced run**: resolve `start_info`/`end_info` via
`preimage_in`; if both land in the same file (they do — the run is a consecutive
slice of the original inline sequence, so it maps to a contiguous source span),
emit a single tight range `[start.start .. end.end)`. Fall back to today's
`combine` only when the endpoints don't resolve into one file.

**Safety guard — hull only when the gap is whitespace-only (R2, NON-NEGOTIABLE).**
Emitting `[start.start .. end.end)` swallows every byte between the pieces. That
is correct *only* when those bytes are inter-token whitespace the node legitimately
owns. If the gap ever contained another node's content (a genuinely-scattered
`Concat`), the hull would swallow that node's source bytes and **manufacture a
containment/overlap violation** — the exact failure 7g exists to prevent. So the
hull helper must **independently re-verify the inter-piece gap bytes are
whitespace-only (space/tab; a newline disqualifies)** before emitting, and fall
back to `combine` (`None`) otherwise. The Phase 1 auditor *flags* candidates; the
fix helper *re-checks* before acting — neither trusts the other. (For the
abbreviation case the gap is always a single consumed `Space`, never a newline —
`coalesce_abbreviations` matches `Str, Space, Str`, not `SoftBreak` — so the guard
always passes there; it earns its keep on the general class.)

The merged token then round-trips losslessly to the *original* source, is copyable
by the writer, and is auditor-visible-and-clean (tight boundaries, no sibling
claims the hull). **The hull copies *source bytes*, not node text** (R5): for
`Dr.   Smith` (three spaces) the hull `[start.start .. end.end)` covers all three
original spaces and Verbatim-restores them, even though the node's text holds a
single nbsp — a concrete case proving why tightness must be a source-boundary
check, never a node-text comparison. The swallowed whitespace is now honestly
owned by the merged token (the `Space` node was consumed) — P2-consistent, and
disjoint from the structural whitespace the scope boundary leaves unowned (a
coalesce can only consume a `Space` *inline*, never a blank line / `> ` gutter /
list indentation).

- [ ] Write a failing byte-offset test first: real parse of `"Dr. Smith wrote…"`,
      assert the merged `Str`'s `preimage_in(file)` returns `Some(start..end)`
      (the full `Dr. Smith` hull), **not** `None`. Confirm red before the fix.
- [ ] Factor the hull computation as a **reusable helper** (resolve endpoints →
      verify same-file → **verify inter-piece gap is whitespace-only** → emit
      `[start.start .. end.end)`, else `combine`/`None`). `coalesce_abbreviations`
      (`:656-661`) is its first caller.
- [ ] Add a failing test for the safety guard: a *synthetic* `None`-Concat whose
      gap contains non-whitespace must **not** be hulled (helper returns the
      `combine`/`None` fallback). Confirms the hull can never swallow other content.
- [ ] Add a round-trip test: editing a paragraph containing "Dr. Smith" no longer
      forces the `Rewrite` fallback (the token is Verbatim-copyable), and the
      emitted source contains the **original regular space**, not the nbsp.
- [ ] Apply the helper to every other `whitespace-gap-concat` site the Phase 2
      census surfaces (abbreviation-coalesce is the worked example; the census is
      the authoritative list, exactly as for the Phase 3 leading-whitespace family).
- [ ] Re-run the Phase 1 auditor over the corpus to confirm these tokens now pass
      tightness/tiling (rather than being flagged as `whitespace-gap-concat`).
      Expect snapshot churn; review as corrections.

### Phase 5 — Producer contract
- Add P1–P4 to `provenance-contract.md` as a **stated BP precondition**, with
  the Concat exception and the explicit "non-overlap, not gap-free" boundary.
- Cross-link from `incremental-writer-contract.md` ("the two are designed in
  pairs" — this is the third party that was never written down).

- [ ] Add P1 (tight ranges), P2 (whitespace ownership), P3 (symmetry), P4 (tiling)
      to `provenance-contract.md` as a stated BP precondition. State P4's two
      qualifiers as **distinct categories, not one lumped "exception" list**: the
      `Concat` hull is **intra-node** (it *defines* what one sibling's claim is
      when that sibling is a `Concat`; it is **not** a sibling-disjointness
      exception), while the atomic N-to-1 same-`Invocation` group is the **only
      genuine inter-node** overlap exception. Do *not* restate the earlier,
      strictly-stronger "no source byte is claimed by two sibling nodes," and do
      not conflate the two qualifiers. Note P2 as a **producer obligation
      discharged by the Phase 3 helper, not an auditor check** (the auditor
      enforces its consequence, P3).
- [ ] Document the **semantic-ownership rule** for `None`-resolving `Concat`s:
      whitespace-only inter-piece gap → producer bug, fix with a contiguous hull
      (Phase 4b); gap containing other content or a newline → genuinely scattered →
      blessed `None`. State the whitespace-only **safety guard** on hull emission.
- [ ] Document the "non-overlap, not gap-free" scope boundary (blank lines,
      `> ` gutters, list indentation are legitimately unowned) and note it is
      **disjoint from** the hull-owned-whitespace population (a coalesce/merge can
      only consume a `Space` *inline*, never structural whitespace).
- [ ] Cross-link `provenance-contract.md` ↔ `incremental-writer-contract.md` so the
      producer/consumer pairing is explicit (it currently is not written down).

### Phase 6 — Audit BP + completeness (THE GATE — do this first)

**STATUS (2026-06-01): COMPLETE — verdict CONDITIONAL GO.** Full audit in
[`claude-notes/research/2026-06-01-plan-7g-phase-6-bp-audit.md`](../research/2026-06-01-plan-7g-phase-6-bp-audit.md).
BP (strengthened with multiplicity (M)) and completeness (strengthened to
"exactly once" (C1+)) are *provable* under an explicit premise set:
**{P4 tiling, L2 dispatch-terminality, L3 whole-walk Invocation-coalescing}**.
(M) reduces cleanly to L1 (sibling-rooted disjointness ⇐ P4) ∧ L2 (no
ancestor/descendant double-count ⇐ terminal R1/R1'). No unfixable obstruction.
**One substantive reachable bug surfaced (Hole α):** atomic N-to-1 shortcode
output (`ShortcodeResult::Blocks` → N independent sibling blocks sharing one
`Invocation`) duplicates the token range when survivors are left non-adjacent,
because today's coalescing (and Plan 7d Property #9) is *consecutive-only*.
Fixable → acceptable gap, not a no-go. **L3 decision (2026-06-01): held by
design, not implemented** — the only splittable N-to-1 producer (block
shortcodes) renders read-only at the client, so no front-end edit reaches the
non-adjacent split; (M) holds. Revisit only if a non-client edit path appears.
Also surfaced: R4 shells / OriginalGap separators must be counted in the (M)
disjoint family (P4-dependent); the latent reversed-`OriginalGap` panic under
¬P4 is **fixed by P4 itself** — tracked as a Phase 3 writer regression test
(above), not a separate fix.

On-pass outputs (all done): (a) verified statement + proofs moved into
[`incremental-writer-bp-proof.md`](../designs/incremental-writer-bp-proof.md);
(b) `incremental-writer-contract.md` points at it; (c) premises recorded on 7d's
tail (L2 live; L3 held-by-design; shell/separator multiplicity).

---

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

- [ ] Land the Phase 1 auditor as a `cargo nextest` property test over a corpus
      (extend the existing `incremental_write_never_panics_on_pampa_corpus` pattern
      from Phase 8 — that one only catches panics, not tiling violations).
- [x] **Decided (2026-06-03):** land as a `cargo nextest` property test only — no
      separate repo-wide `cargo xtask verify` lane, unless the corpus sweep proves
      too slow for default nextest (one enforcement path, not two that drift).
- [ ] Confirm the test fails on a reverted Phase 3 handler fix (proves it would have
      caught the original drift) before declaring CI enforcement done.

**Unblocks 7d:** this is the "7g's property test is green" precondition 7d's
BP-holds claim depends on. The Phase 6 gate passing is necessary but not
sufficient — 7d stays blocked until this lands.

### Phase 8 — Writer crash on `Concat`/`Generated`-led inline (degenerate-offset boundary)

**Found during the Phase 6 audit (2026-06-02); confirmed live, reachable from
real corpus files.** A **distinct, writer-side bug** — *not* a tiling/producer
defect — discovered while pulling the Phase 6 thread on the reversed-slice family.
Adding it here because it belongs to the same "source ranges meet the writer"
story, even though the fix is purely in the incremental writer.

**Symptom.** `incremental_write` (the WASM/hub entry) panics with
`byte range starts at N but ends at 0` when a top-level inline-content block
(`Paragraph`/`Plain`/`Header`) whose **first or last inline carries a `Concat`
or `Generated` source_info** is reconciled through the **InlineSplice** path
(`RecurseIntoContainer` alignment).

**Mechanism (verified).** `assemble_inline_splice`
(`crates/pampa/src/writers/incremental.rs` ~1287) computes
`prefix = original_qmd[block_span.start .. inline_start]` where
`inline_start = inline_source_span(orig_inlines[0]).start`, and
`inline_source_span` (~1599) reads `SourceInfo::start_offset()`. The relevant
accessors return **non-source offsets** for `Concat`/`Generated`
(`crates/quarto-source-map/src/source_info.rs` ~350-371): `start_offset()` is the
**sentinel `0`** for both `Concat` and `Generated`; `end_offset()` is `0` for
`Generated` but the **`Concat`'s own `length()`** (a small positive — *not* `0`,
correcting an earlier overstatement here). Either way the value bears no relation
to the byte's position in the source file. So the prefix slice becomes
`qmd[block.start .. 0]` — reversed — and panics. The suffix slice has the
analogous hazard on the last inline: for a `Concat`-led last inline it slices
`qmd[length .. block.end]` (wrong bytes, panics only if `length > block.end`); for
a `Generated`-led one, `qmd[0 .. block.end]` (wrong bytes). The `preimage_in` fix
makes all four boundaries correct regardless.

**Root cause is NOT tiling.** The triggering `Concat` is *well-formed and
contiguous*: e.g. `Str "Table:"` parses as
`Concat[ Original[35..40] "Table" ++ Original[40..41] ":" ]` — the inline parser
tokenizes the `:` separately and re-joins via `SourceInfo::combine`
(`source_info.rs:317`, which builds a 2-piece `Concat`). The pieces tile, so
`preimage_in(target)` returns the correct `Some(35..41)`; only the
`start_offset()` *accessor* returns the degenerate `0`. 7g's P4 tiling work would
not change this Concat (it is already P4-correct). **The writer simply reads the
wrong accessor.**

**Reachability.** A corpus scan on this branch found **10** top-level paragraphs
whose first inline's `start_offset()` (0) is less than the block's start — all
contiguous `Concat`s — spanning several constructs: literal punctuation text
(`Table:`), links/images, anchor shorthands, math-with-attr, smart-punctuation /
escaped text. Files: `04_links_images`, `04_simple_links`,
`anchor_shorthand_variants` (×2), `smoke/018`, `smoke/table`, `math-with-attr`,
`table-no-caption-table-prefix`, `ansi/colors-with-formatting`,
`ansi/ordered-lists`. Each panics when its block is reconciled as an InlineSplice.
Confirmed via a Rust-level repro. **Live-UI reachability is not yet confirmed** —
it depends on whether a real hub edit drives that block to `RecurseIntoContainer`
(InlineSplice) vs. whole-block replace/`KeepBefore`; a manual caption edit in
quarto-hub did *not* reproduce. Worth a follow-up to drive the WASM bridge into an
InlineSplice on a Concat-led block.

**Fix (TDD) — DONE 2026-06-02, committed `b43fadef`.** (Closeout caveat: the
full-workspace `cargo xtask verify` — not just the pampa suite — is still owed,
because this touches `quarto-source-map`/the WASM leg. Tracked in *Phase status*.)
- [x] Failing regression test `inline_splice_concat_led_paragraph_does_not_panic`
  (`crates/pampa/tests/integration/incremental_writer_tests.rs`): real parse of
  `tests/smoke/table.qmd` (Concat-led `Str "Table:"`), mutate a `Str` to force
  InlineSplice. Confirmed red (`panicked … incremental.rs:1287 … starts at 35 but
  ends at 0`) before the fix; green after.
- [x] Splice boundaries now derived from `preimage_in(target_file_id)` (block,
  first inline, last inline) instead of `start_offset()`/`end_offset()`. For the
  `Table:` case this yields prefix `qmd[35..35]` = `""`. Localized to
  `assemble_inline_splice` (the only caller of that helper); `inline_source_span`
  semantics left unchanged. `assemble_inline_content` already used `preimage_in`
  for the content bytes, so only the boundary computation was broken.
- [x] When an edge inline (or the block) has **no preimage** (`None`), or the
  ranges would be out of order, `assemble_inline_splice` returns `Ok(None)` and
  the caller falls back to `Rewrite { write_block_to_string(new_block) }`. Slices
  use `.get()` as belt-and-suspenders. (BP proof's None-preimage routing, Phase 6
  check #4.)
- [x] Corpus property test `incremental_write_never_panics_on_pampa_corpus`:
  scans `git ls-files '*.qmd'` (pampa crate — covers every confirmed trigger:
  captions, links/images, anchors, math-with-attr), drives identity + a
  first-`Str` mutation through `catch_unwind`, asserts no panic. (Repo-wide xtask
  lane is a possible Phase 7 extension; the existing proptest generators in
  `inline_splice_property_tests.rs` never emit Concat-led inlines, so they miss
  this.)
**Sibling bug — `did_coalesce` function-scope provenance corruption — FIXED (TDD) 2026-06-02, committed `018fb934`.**
A second, independent bug found while pulling this thread (a background agent
weaponized it; verified directly on this branch). `coalesce_abbreviations`
(`postprocess.rs`) declared `did_coalesce` *outside* the `while` loop and never
reset it, so every `Str` **after** the first abbreviation-coalesce took the
`start_info.combine(&end_info)` branch — i.e. `combine(self, self)` on a Str that
didn't coalesce — producing a **doubled-self `Concat`** (`start_offset()==0`,
`end_offset()==2*len`, `preimage_in()==None`). Harm: provenance corruption on
~13 corpus files → annotated-qmd substring-invariant violation, attribution
drops provenance, and the incremental writer forced into lossy `Rewrite`
fallback. On `origin/main` (no Phase 8 fix) it is additionally a **live crash +
silent wrong-output** on edit; on this branch the Phase 8 `preimage_in` guards
mitigate the writer crash but the provenance corruption remained until this fix.
- [x] Failing unit test
  `postprocess::did_coalesce_tests::non_coalesced_str_after_abbreviation_keeps_its_original_source_info`:
  `["Dr.", Space, "Smith", Space, "wrote"]` → `"wrote"` must keep `Original[10..15]`.
  Confirmed red (got `Concat` `(0,10)`, `preimage None`) before the fix; green after.
- [x] Fix: declare `did_coalesce` **inside** the loop (reset per `Str`); accumulate
  a separate `any_coalesce` for the function's return value (the caller's fixpoint
  loop). The intended `Dr.+Smith` coalesce is preserved.
- [x] Full pampa suite green (3914), **no snapshot churn**. (Former open question —
  now **scheduled as Phase 4b**, no longer left for later: the *legitimate*
  coalesce case's `combine` produces a non-contiguous `Concat`/`preimage None`.
  The **nbsp text** is the intended Pandoc behavior and stays; the **`source_info`**
  is the defect — Phase 4b replaces it with a contiguous hull so the merged token
  is copyable and auditor-visible. Under the round-2 semantic-ownership rule the
  auditor **flags** it as a `whitespace-gap-concat` finding (it descends into the
  pieces and sees the gap is the swallowed space), so it is a first-class,
  mechanically-detected item in the plan checklist rather than a bead that would
  get lost.)

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

- **Auditor fidelity.** If the auditor's `Substring`/`Concat`/`Generated`
  resolution diverges from `preimage_in`, the census is wrong. Cross-check
  against the Rust impl. (`Generated` resolution and the same-`Invocation`
  unit-grouping are the newest and least-exercised paths — cross-check a
  block-shortcode case explicitly.)
- **Blast radius of handler changes.** Many existing tests assert byte ranges
  indirectly (snapshots). Expect snapshot churn; each change must be reviewed as
  a *correction*, not a regression. The property test is the backstop. Note the
  detection strategy is **auditor-led, not rename-led** — `node_source_info_with_context`
  has 60 callers in `treesitter_utils/` (83 across `pampa/src`), almost all
  legitimate, so a blanket rename is the wrong lever (see Phase 3).
### Resolved (decided 2026-06-03, during the pre-implementation review)

- **Concat semantics — RESOLVED via the semantic-ownership rule (2026-06-03,
  round-2 review).** `preimage_in`'s contiguous→hull behavior is fine (Phase 6
  Hole γ: not a soundness issue). The disposition of a *non-contiguous* `Concat`
  (`preimage_in` → `None`) is **not** decided by piece-adjacency — the earlier
  draft's "non-contiguous → blessed `None`; adjacent-but-mis-joined → bug" was
  **self-contradictory**: Phase 4b's `Dr. Smith` `Concat[Original[0,3),
  Original[4,9)]` has a one-byte *gap* (the swallowed space), so its pieces are
  **not** adjacent, yet it is plainly a bug to fix, not a blessing. The correct
  criterion is **semantic ownership**, applied by inspecting the inter-piece gap
  bytes in the source:
  - **gap is whitespace-only** (space/tab; a newline disqualifies → next bullet) →
    the node legitimately owns that inter-token whitespace (e.g. a consumed
    `Space`), so the `None` is an artifact of a lossy join → **producer bug**, fix
    with a contiguous hull (Phase 4b's template, with the whitespace-only safety
    guard so a hull never swallows other content).
  - **gap contains other nodes' content, or a newline** → the node is **genuinely
    scattered** across source → **blessed `None`-skip.**
  This is now mechanically detectable, so it is *not* left to human triage: the
  Phase 1 auditor descends into the pieces and emits a `whitespace-gap-concat`
  finding (R1). Which concrete `Concat`s are bugs is still a census output
  (Phase 2); the **rule that sorts them is now settled and self-consistent.**
- **BP premise feeding back to 7d — RESOLVED.** Phase 6 already ran and surfaced
  the premise set {P4, L2, L3}; L3 (whole-walk same-`Invocation` coalescing) is
  recorded on 7d's tail as held-by-design, and the shell/OriginalGap multiplicity
  accounting is in the proof file. Nothing further to discover here — see the
  Phase 6 audit and `incremental-writer-bp-proof.md`. (Retained only as a pointer;
  no longer an open item.)
- **Code spans, scanner vs handler — DECIDED handler-only.** The handler
  re-derives the correct tight range regardless of the loose lexer token (the
  whole "decouple source-info from lexing" thesis), so **handler-only is the
  firm decision.** Restoring the scanner's backtick-start boundary as
  defense-in-depth is **downgraded to a non-blocking follow-up** that reopens
  *only* on the Phase 2 evidence test below.
- **Phase 7 placement — DECIDED.** The auditor lands as a `cargo nextest`
  property test (gates every PR via the normal suite); **no separate repo-wide
  `cargo xtask verify` lane** unless the corpus sweep proves too slow for default
  nextest. One enforcement path, not two that can drift. (Reflected in Phase 7.)

### Open — but the *protocol* is now fixed (answers are data-dependent)

- **Substring unknowns.** The floor audit skipped `Substring`; Phase 2 may surface
  a class the policy hasn't considered. **Protocol:** if a `Substring` violation
  appears that is *not* a leading/trailing-whitespace case, **stop and classify
  before fixing** — it is one of (i) a new policy class to state in the contract,
  (ii) a producer bug to fix, or (iii) an exception to bless *with written
  justification*. Do not improvise a fix that quietly weakens the invariant.
- **Scanner code-span harm (the W1 residual).** Reopens the scanner-hardening
  follow-up *only if* Phase 2 shows the loose backtick-start token causes harm
  **other than** ranges (which the handler already corrects). Absent such
  evidence, handler-only stands.
- **Live-UI reachability of the Phase 8 InlineSplice bug.** Empirical — needs a
  real WASM-bridge edit that drives a `Concat`-led block into an `InlineSplice`
  (a manual caption edit did not reproduce). The bug is **already fixed
  defensively**, so this is a *confirm-or-close* verification task, not a gate on
  7g. Run it during Closeout/e2e if a browser is available; otherwise note it
  unconfirmed.

## References

- Investigation (annotated-qmd instances A/B + citation): beads `bd-1d6io`.
- Consumer contract that needs this: [`incremental-writer-contract.md`](../designs/incremental-writer-contract.md) (the partition claim).
- Producer contract to amend: [`provenance-contract.md`](../designs/provenance-contract.md).
- Helper: `range_to_source_info_with_context` in `crates/pampa/src/pandoc/location.rs`.
- Affected handlers (corrected to the real tree, 2026-06-02; authoritative list = Phase 2 census): `crates/pampa/src/pandoc/treesitter_utils/{code_span_helpers,citation,commonmark_attribute,span_link_helpers,quote_helpers,uri_autolink,shortcode}.rs`, `crates/pampa/src/pandoc/treesitter.rs`. Note `raw_specifier`/`raw_attribute` handling lives **inside** `code_span_helpers.rs`; there is no `key_value_specifier.rs`/`quoted_span.rs`/`raw_specifier.rs`.
- Prior art (peel-Space-but-not-range): k-296 (citation), `inline_note_reference` (in `postprocess.rs`).
- Consumer: [`2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`](2026-05-26-q2-preview-plan-7d-algebraic-soundness.md).
