# Plan 7d — edit queue — ✅ APPLIED 2026-06-03

All items below were applied to `2026-05-26-q2-preview-plan-7d-algebraic-soundness.md`
(and, for items 3/9, to `2026-05-25-q2-preview-plan-7c-closure-gaps.md`) on 2026-06-03.
This file is kept as the record of what changed and why.

**Correction logged during the pass:** item 9 originally said "7d Phase 3 should
*preserve* the positional proxy, so 7d *inherits* the imprecision." That was wrong.
Reading 7c Phases 7/7b against the block-level `e584428d` code showed 7d's inline
`UseAfter` dispatches on the **new** node's own `source_info` and therefore *removes*
the proxy — which is exactly why 7c Phase 7 is redundant. The applied edits reflect the
corrected understanding (proxy eliminated; 7c Phases 7 and 7b obsoleted, not "preserved"
or "defense-in-depth"). Item 9's text below has been corrected accordingly.

## Queue (all applied)

1. **Record 7g as a precursor (sequencing).** 7g (source-range tiling / P4) was
   inserted *before* 7d when we discovered source_info does not properly tile the
   input; the BP/completeness proof now depends on P4 as a premise. 7g is **complete
   on `feature/provenance`**. Update:
   - Header (line 4): "ships after 7f; before 7e" → "ships after **7f and 7g**; before 7e".
   - Epic-context table (lines 12–20): add a 7g row (source-range tiling, P4 producer
     precondition, complete).
   - Line 45: "after 7f has landed" → "after 7f **and 7g** have landed".
   - (The Premises section, lines 332–361, already references 7g; just make the
     front-matter consistent with it.)

2. **Phase 4 generator must exclude CustomNodes (run 7d before 7e).** The proptest
   generator (`gen_pandoc_with_atomic_descendants`, extending
   `crates/quarto-ast-reconcile/src/generators.rs`) inherits a `features.custom`
   flag that emits non-atomic `"Callout"`/`"CustomWidget"` nodes via `gen_custom_block`
   / `gen_custom_inline`. Under 7d the qmd `Block::Custom` arm is still empty, so a
   generated CustomNode serializes to empty/soft-drop and **`completeness_holds`
   (`parse(Source') ≡ AST_new`) would fail** (soundness `bp_holds` is unaffected).
   Edit Phase 4: set `custom: false` in the generator's `GenConfig`, and **move
   CustomNode property/completeness coverage to Plan 7e** (where the Custom arm is
   filled). One-line config, not a generator rewrite.

3. **Treat custom nodes as opaque in 7d; resolve the R3-vs-soft-drop contradiction by
   deleting the "partial handling" prose.** 7d deletes `Rewrite`, and the dispatch is
   total, so customs must land *somewhere* — but the landing spot needs **zero
   custom-content code**. Route a non-atomic CustomNode to the **soft-drop** rules 7d
   builds anyway: copy its original source bytes verbatim (R1') if it has a preimage,
   else Omit + Q-3-43 (R2'). The writer treats the custom node as an **opaque block**
   and never descends into it.

   Why opaque/soft-drop, not "R3 with empty shell + recurse children": the writer has
   **no `custom_node_plans` recursion today** (grep: zero hits in `crates/pampa/src/
   writers/`). Recursing into a custom node's children would be *new* 7d wiring — more
   work, and exactly the custom-specific effort we want to defer. Soft-drop needs no
   recursion, is less code, and gives a *better* interim bug (callout preserved intact,
   edit refused + warning — instead of degrading to a plain paragraph or vanishing).

   Edits:
   - Dispatch table (line 147): the "editable inside, block container → R3" row should
     not apply to CustomNodes until 7e. Either gate it ("container kind has a writable
     shell helper") or make the R3 implementation degrade to soft-drop when no shell
     helper exists for the kind. Simplest mechanism: have the writer-side editability
     check treat non-atomic CustomNodes as not-editable-inside until 7e, so they hit the
     existing RecurseIntoContainer → not-editable → R1'/R2' path.
   - Phase 1 (line 213) prose is fine in spirit ("custom-node editing remains broken")
     but should say plainly: *7d treats custom nodes as opaque (verbatim-preserve or
     omit); 7e fills the `Block::Custom` shell helper AND wires `custom_node_plans`
     recursion to make interior edits round-trip.*

   Division of labor: **7d** = custom node is a black box (copy verbatim / omit).
   **7e** = open the box (shell helper + `custom_node_plans` recursion + flip the gate),
   after which customs match R3 naturally.

4. **Reword Property #9 to match implementation + L3 decision.** "Properties enforced"
   (line 166) claims coalescing collapses "any future N-to-1 producer" to a single
   Verbatim, but Phase 2 (line 222) implements **consecutive-only** coalescing and the
   proof §6 / Premises (lines 345–353) explicitly hold L3 "by design, not implemented"
   because consecutive-only does not satisfy general L3. Reword line 166 to
   "consecutive runs" and cross-reference the L3 held-by-design decision.

5. **Dispatch table = explicitly ordered match (AGREED).** Precedence among the
   (atomicity, preimage, container/leaf) rows is currently implicit. Make Phase 2's
   `dispatch()` an ordered match with a stated decision procedure; totality + Property #2
   then read off the order rather than relying on the reader to apply qualifiers.

6. **`SeparatorRule::OriginalGap` / `TrailingState` / Property #9 composition (RESOLVED — recommendation).**
   - **Coalescing (Property #9) is a *planner* step** inside `plan_user_writes`: collapse a
     maximal run of same-`preimage_in` children into one `Verbatim` *before* `assemble`
     sees them. `assemble` therefore never computes a separator inside a coalesced run.
   - **`OriginalGap` is an *assemble* per-adjacent-pair decision:** if neighbors `i-1,i`
     both expose target preimages `r_prev,r_curr` with `r_prev.end <= r_curr.start` and a
     same-container-consecutive origin → emit `Source[r_prev.end .. r_curr.start]`. Guard
     with `debug_assert!(gap.start <= gap.end)` + graceful fallback to the canonical
     separator (this is the reversed-slice guard the BP audit flagged at plan lines 357–361).
   - **`TrailingState` is derived from the actually-emitted bytes' tail** after each entry
     (None / EndsWithText / EndsWithNewline / EndsWithBlankLine) — not tracked symbolically
     (avoids drift). The canonical `SeparatorRule` consults it: `StandardBlock{tight:false}`
     → blank⇒"", newline⇒"\n", text⇒"\n\n". This is the principled form of today's
     `prev_block_text.ends_with("\n\n")` special case (`compute_separator:1059`).
   - **Precedence in `assemble`:** try `OriginalGap` first (most faithful — reproduces the
     user's original whitespace, P1 bytes); else fall to the enclosing `Recurse`'s
     `SeparatorRule` resolved against `TrailingState` (P2 bytes). Coalesce(planner) →
     separate(assemble) never interleave, so there's no shared-logic ambiguity.

7. **`serialize_leaf` non-leaf guard = runtime, not type-level (RESOLVED — recommendation).**
   `Block` (`block.rs:16`) and `Inline` (`inline.rs:13`) are each a single flat enum with
   no leaf/container split, so type-enforcement would require carving `LeafBlock`/
   `ContainerBlock` newtypes across the whole AST — too invasive, not worth it. Instead:
   `serialize_leaf` is an **exhaustive match over leaf variants only**; container variants
   hit `debug_assert!(false, …)` + return a diagnostic-error (Q-3-x) in release (do **not**
   panic in the writer). Property #5 + totality guarantee it's never reached in practice;
   the guard is belt-and-suspenders that gives a precise error if the dispatch regresses.

8. **Phase 4 coverage thresholds = reachability gate + empirical floors + visible report (RESOLVED — recommendation).**
   The magic absolute floors (`r1>=100`, …) are brittle and arbitrary pre-calibration.
   Replace with: (a) **primary gate = each reachable row ≥ 1** (catches a row going
   unreachable — the real regression signal); (b) express any magnitude floors as
   **fractions of the proptest case count N** (e.g. `r1 >= N/10`) so they scale with
   iteration count; (c) **calibrate the fractions empirically** — run the generator once,
   record observed per-row counts, set floors at ~⅓–½ of observed; (d) **always print the
   observed per-row distribution** under the `dispatch-coverage` feature so drift is visible
   even when the test passes.

9. **7c interaction — run 7d before 7c, same defer strategy (RESOLVED).** 7d does **not**
   depend on 7c. 7c closes Plan-7 *gaps* (Q-3-41 diagnostic, TS-side gate parity,
   debug-assert test, per-kind soft-drop tests) plus Phases 7/7b (`displaced_before_idx`,
   inline atomic-Generated check). The allowlist algebra **subsumes 7c Phases 7/7b** — it
   catches those cases by construction, so if 7c hasn't landed they are *born redundant*.
   - The only coupling is **7d Phase 5** ("audit/retire 7c branches"). If 7c hasn't shipped,
     Phase 5 has nothing to retire → rewrite it as a **no-op + forward note**: "when 7c is
     written, implement its Phases 7/7b as defense-in-depth *tests only* (the algebra already
     handles them), or skip."
   - The other 7c items (Q-3-41, TS gate, debug-assert test, per-kind tests) are orthogonal —
     7d neither needs nor breaks them.
   - **Proxy ELIMINATED in Phase 3 (corrected):** today's `assemble_inline_content` uses
     `result_idx` as a *positional proxy* for the displaced original inline
     (`incremental.rs:1376–1378`). 7d Phase 3 dispatches the inline `UseAfter` on the **new**
     node's own `source_info` (mirroring the block-level `e584428d` fix at `:392–439`, which
     never consults the original), so the proxy is **removed**, not preserved. That is
     precisely why **7c Phase 7 (`displaced_before_idx`) is obsolete** — no original-side
     lookup remains to make precise — and why **7c Phase 7b** is subsumed (R1' is its
     new-side atomicity check). Both 7c phases are tombstoned in 7c. The 7d implementer must
     NOT assume `displaced_before_idx` exists nor re-introduce the proxy.

10. **R5 trust point IS e2e-testable — add Playwright tests (RESOLVED; supersedes the earlier
    "can't test e2e" note).** R5 emits `serialize_leaf(n)` trusting the producer's source_info
    classification (nodes reaching R5 are attested user-authored; atomic-Generated routes to
    R1'/R2'). The strict form: one shape `Generated{by: user_edit}`, stamped client-side by the
    React framework (7f Phase 3); 7d trusts it. The trust point's **consequences are directly
    observable** via the existing write-path harness
    `hub-client/e2e/q2-preview-render-components-write.spec.ts` (Automerge→hub→browser→WASM→
    `incremental_write_qmd`). Add to 7d Phase 4 (or a Phase 4 sibling):
    - **Soundness / no-leak:** fixture with `{{< lipsum 3 >}}`; in-browser type into the
      resolved paragraph, trigger the write; assert written qmd still contains
      `{{< lipsum 3 >}}` and NOT the resolved lorem-ipsum + typed word. (The canonical
      contract example, tested for real.)
    - **R5 authored-leaf / completeness:** plain paragraph; type a new word; assert it appears
      in the written qmd.
    - **Soft-drop signal:** assert Q-3-43 surfaces on the shortcode edit.
    What is **not** mechanically testable: the trust point's *universal premise* (no producer
    ever mis-stamps) — that's a contract/audit obligation (7f `SourceInfo::default()` audit +
    "new kinds default to non-atomic"), not a runtime test. Honest status: consequences
    e2e-tested; premise audited.

## Decided cleanups

- **Stale comment at `incremental.rs:434`** ("the qmd writer's CustomNode arm serializes the
  fresh plain_data" — the arm is empty): **deferred to 7d *implementation***, not applied as a
  plan edit. The whole let-user-win block is deleted when Phase 2 replaces the cascade with the
  dispatch table, so the false comment dies with the refactor; the 7d implementer must not
  carry it forward. 7e comments as it pleases. (User decision — "remove that comment in 7d".)
- ✅ **Applied:** refreshed stale helper line numbers in Phase 2 (`emit_metadata_prefix`
  942→950, `find_metadata_trailing_gap` 998→1006, `ensure_trailing_newline` 1103→1111).
