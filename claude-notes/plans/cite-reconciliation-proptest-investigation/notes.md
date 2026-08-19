# Investigation notes — bd-205v6 (flaky proptest: reconciliation_preserves_structure_full_ast)

**Date:** 2026-08-19. Investigated at `main` @ `4b4a63ce`.

## Reproduction

The seed from the 2026-08-19 strand comment reproduces deterministically at HEAD:

```bash
echo "cc c8f784938848bdc2b251dd3da92a1a70c7135144181b7792bef0b40b47e30234" \
  >> crates/quarto-ast-reconcile/proptest-regressions/lib.txt
cargo nextest run -p quarto-ast-reconcile -E 'test(reconciliation_preserves_structure_full_ast)'
# FAIL — "Result should be structurally equal to 'after'"
```

(Seed deliberately not committed — it would hard-red the workspace until the bug is fixed.
The original 2026-05-26 seed from the strand description is
`cc 8f798bbfabf9a12269dc90aa13188f685afe8cbf6f4acb2da01f9bff1af93158`.)

## First divergence in the shrunk case

Extracted the `Result:` / `After:` debug dumps from the failure and found the first
differing character. The divergence is inside a `Cite`'s `citations` field:

- **Result** (wrong): `Citation { id: "", prefix: [Str "A"], suffix: [Strikeout[Emph[Str "gu"]]], mode: NormalCitation, ... }`
  — this is the **before** document's citation.
- **After** (expected): `Citation { id: "te", prefix: [Strikeout[Str "d"]], suffix: [EditComment[Shortcode ...]], ... }`

So reconciliation paired a before-Cite with an after-Cite and kept the before side's
`citations` payload wholesale.

## Root cause

This is the bd-3zp3z4jx bug class (container identity inherited across a type-only
match), recurring for `Cite`:

1. **compute.rs — inline alignment Step 2** (type-based container matching,
   `compute_inline_alignments`, ~line 654–689): the identity guard added for
   bd-3zp3z4jx checks `Custom.type_name`, `Link.target+attr`, `Image.target+attr`,
   `Span.attr` — and **falls through to `_ => true` for `Cite`**. Two Cites with
   completely different `citations` (id, mode, prefix, suffix) are paired as "the
   same container" (`RecurseIntoContainer`).
2. **apply.rs — `apply_inline_container_reconciliation`** (~line 346): the `Cite`
   arm reconciles only `o.content` and keeps `o.citations` from the original.
   Unlike `Link`/`Image`/`Span`/…, it copies **no** structural fields from the
   exec side. Result: the reconciled AST carries the before-citations.

The hash (`hash.rs:302`) and `structural_eq_inline` (`hash.rs:920`) both *do* cover
citation id/mode/prefix/suffix — so Step 1 (exact match) correctly rejects the pair;
it's Step 2 that wrongly accepts it.

Note the design constraint from the bd-3zp3z4jx comment: a container's non-child
identity is spliced **verbatim from original source** by the incremental writer
(`assemble_recursed_container`; `inline_children()` in
`crates/pampa/src/writers/incremental.rs:924` treats only `c.content` as a Cite's
children — citation prefix/suffix live in the verbatim-copied delimiter region).
So "fix it in apply by copying `e.citations`" would make the AST disagree with the
spliced source text; the source-consistent fix is the Step-2 `UseAfter` fallthrough.

## Minimal repro (unit test)

`minimal-repro-test.rs` in this directory — a 1-paragraph, 2-inline case:
`[Str "anchor", Cite]` on both sides, Str identical (so block phase 2 sees a kept
inline and recurses into the paragraph), Cites differing only in `citations`
(id "a"/prefix "x" vs id "b"/prefix "y"). Fails at HEAD with the result keeping
citation "a"/"x". Verified 2026-08-19 by temporarily inserting it into
`lib.rs`'s test module (then reverted; the fix session should re-add it TDD-style).

Important subtlety discovered on the way: with the Cite as the paragraph's *only*
inline, the test **passes** — block-level phase 2 requires at least one
`KeepBefore` inline (`has_kept_inlines`) to recurse into a paragraph; otherwise the
whole block is `UseAfter` and the bug is masked. This is also why the proptest hits
it rarely (hence "flaky"): it needs two Cites with differing citations aligned in
the same inline list *plus* a hash-identical sibling inline in the same block.

## Why it was "flaky"

Nothing nondeterministic: any given seed either generates the shape or not. Fresh
proptest runs (256 cases) rarely generate it, so the May 2026 failure looked flaky
and didn't reproduce with fresh seeds. Both saved seeds fail deterministically.

## Sibling audit (same bug class)

| Container | Step-2 identity guard | apply copies exec's structural fields | AST-level status |
|---|---|---|---|
| Link / Image | target+attr checked | attr+target copied | OK (belt and braces) |
| Span | attr checked | attr copied | OK |
| Custom | type_name checked | (slot plan) | OK |
| Quoted | none | quote_type copied | OK at AST level (see open Q on source splice) |
| Insert/Delete/Highlight/EditComment | none | attr copied | OK at AST level (same open Q) |
| **Cite** | **none** | **nothing copied** | **BUG (this strand)** |

Open question for the fix session: for Quoted/Insert/Delete/Highlight/EditComment,
the AST ends up right, but if the incremental writer splices the *original* source
delimiters verbatim, a changed quote_type/attr may be wrong in the emitted source
even though the AST is right. That's a separate potential bug (source-level, not
caught by this proptest) — decide whether to file it.
