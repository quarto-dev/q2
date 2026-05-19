# Next-step options for the inline-scope state-collision bug

This document outlines two paths forward from the work-in-progress committed alongside it. Background is in `inline_issue.md`. The currently-committed implementation (Attempt 5, "Option 1") adds a stack-aware external scanner that:

- Maintains an `inline_scopes` stack inside the scanner, mirroring the open inline scopes the parser is in the middle of
- Pushes on each open-token emission, pops on each close-token emission, clears on every block-boundary emission
- Gates close emissions: refuses to emit `X_CLOSE` when `X` is on the scope stack but not on top (which would orphan an inner scope), falling through to the open variant in that case
- Serialises the stack so GLR rollback restores it correctly

The work is in place and builds clean. Phase 0–3 of the previous plan ran end-to-end. The result: the scanner correctly emits `OPEN` instead of `CLOSE` for the second `'` in `The '_blank' word.`, the `*` at the end of `*a" b.*`, etc. The gate is observably firing.

But the bug is not fixed. The eight bug-describing tests are still failing because tree-sitter's LR state minimisation has merged the post-`OPEN` and post-`CLOSE` states in the relevant configurations — both terminals transition to the same destination state from the same source state. The corpus lookup keys on `(state, sym)`, so the diagnostic that fires is the same regardless of which terminal the scanner emitted.

Concretely, for `The '_blank' word.`:
- Source state 1996 (after "blank") + `SINGLE_QUOTE_OPEN` → destination state 704
- Source state 1996 (after "blank") + `SINGLE_QUOTE_CLOSE` → destination state 704

The scanner's emission choice does not change the parser's eventual error key, so the corpus's only entry for `(704, _whitespace)` (`Q-2-10/simple-14`) wins, and the test sees `Q-2-10` instead of `Q-2-5`.

Two paths to get past this wall.

## Option A: Option 1 + corpus rewrite

Keep all the scanner-stack-aware work that is currently committed. Treat the scanner gate as a behaviour-changing intervention and rebuild the corpus on top of the new behaviour.

The idea: when the scanner emits `OPEN` instead of `CLOSE` for the second `'` in `The '_blank' word.`, the parser shifts a different terminal. Even if the destination state is the same (state 704), the *path* by which the parser reached that state, and the consumed-token stack at that point, are different. More importantly, the build-script's existing corpus generation can now be re-run, and the resulting autogen table reflects whatever `(state, sym)` keys the gated emissions produce.

If those new keys are different from today's keys for some of the failing inputs, we can attach new diagnostics to them. If they are not different, this option doesn't help and we fall back to Option B.

### What it would take

1. Re-run `crates/pampa/scripts/build_error_table.ts` with the gated scanner active. Inspect the regenerated `_autogen-table.json` and compare it to the version on `main`. Specifically, look at what `(state, sym)` keys the existing test inputs land on now:
   - `'__` prefix variants of Q-2-15 cases
   - `'_`, `'*`, `'**` prefix variants of Q-2-5, Q-2-12, Q-2-13 cases
   - `*'`, `**'`, `_'`, `__'` prefix variants of Q-2-10 cases
   - `*"`, `**"`, `_"`, `__"` prefix variants of Q-2-11 cases

2. If any of those inputs reach a different `(state, sym)` than before, add corpus cases in `crates/pampa/resources/error-corpus/Q-2-X.json` so the new keys map to the desired diagnostics. The build script's `prefixesAndSuffixes` mechanism already generates many variants automatically; the question is whether the gate makes the right inputs reach distinguishable keys.

3. If the same `(state, sym)` is still reached by multiple-classes-of-error inputs (which is what `inline_issue.md` calls out as the underlying problem), this option is dead. Move on to Option B.

4. Verify the full focused test loop (`cargo nextest run -p pampa --test test_emphasis_in_quote`) passes.

5. Run the wider workspace check and audit any snapshot churn.

### Risks

- The state-minimisation that merged `OPEN` and `CLOSE` transitions probably also merges other distinctions we'd want. The most likely outcome is that the regenerated corpus has the same collisions as today's, just shifted around.
- `qmd-syntax-helper`'s `apostrophe_quotes` rule depends on `Q-2-10` firing for `*a' b.*` etc.; the gate already preserves this for those inputs (the gate doesn't refuse `SINGLE_QUOTE_CLOSE` when `SINGLE_QUOTE` is not on the stack at all), but regenerated corpus entries may move keys around in a way that breaks the score-based tie-breaker.
- Snapshot churn across the wider workspace will be significant.

### Effort estimate

Half a day to run the regen and inspect outcomes. If the diagnosis is "same collisions, no progress," cut the experiment quickly. If the diagnosis is "new keys, can add corpus entries," another half-day to a day to add entries and verify.

## Option B: Hybrid — Option 1 tracking, plus Option 4 lookup discrimination

Combine the scanner's per-block-bounded `inline_scopes` stack with an extension to the corpus lookup that uses it. This is what the abandoned Attempt 4 tried to do, but with a more reliable source for the outer-scope value.

Attempt 4 read the outer scope by walking `parse.consumed_tokens` from the `TreeSitterLogObserver` after parsing finished. That walker broke on multi-paragraph inputs because `consumed_tokens` at end of parsing accumulates leftover unreduced terminals from earlier blocks where the parse failed; the walker saw a row-0 single-quote opener and reported it as the outer scope for a row-2 error.

The scanner's `inline_scopes` stack, as currently committed, does *not* have that problem because it is explicitly cleared on every block-boundary emission (`CLOSE_BLOCK`, `BLOCK_CLOSE`, `TOKEN_EOF`, `BLANK_LINE_START`, hard `LINE_ENDING`). Each block's scope tracking is independent. If we expose this stack to the diagnostic-generation pipeline and use it as a third component of the corpus-lookup key, we get the discrimination Attempt 4 wanted, without its multi-paragraph bug.

### What it would take

1. **Decide on gate-or-no-gate.** Two sub-options:
   - B-tracked-only: revert the gate (`can_close_scope` check) but keep the push/pop tracking. The scanner emits exactly what it does today; only the stack is new. Lowest risk of changing existing behaviour. The stack is purely informational.
   - B-gated: keep the gate. Behaviour changes for the bug-shape inputs, and the stack contents at error time differ from B-tracked-only.

   I'd default to B-tracked-only — the gate's effect is not observable at the corpus-key level anyway (state-minimisation wall), so removing it eliminates a behaviour change with no downside.

2. **Expose the scanner stack at error time.** The scanner state is serialisable; the runtime can pass the stack contents through to diagnostic generation. Options:
   - Add a small Rust-side accessor that, given the tree-sitter parser's current scanner-state buffer, deserialises the inline-scope stack. This requires the parser to expose the buffer, which `tree-sitter-rs` does via internal APIs (may need a small unsafe block).
   - Alternatively, after `TreeSitterLogObserver` captures an error state, walk `consumed_tokens` *in reverse* from the error position, but with the same push/pop logic as the scanner — i.e., reconstruct the scanner's stack from the public log. This avoids tree-sitter internals but duplicates logic. Should be the same answer as the scanner's stack if the log is faithful, which it appears to be.

   The reconstruction approach is probably simpler and avoids unsafe code.

3. **Extend the corpus-lookup key from `(state, sym)` to `(state, sym, outer_scope)`.** This was already designed in `claude-notes/plans/2026-05-18-merr-outer-scope-key.md` (now deleted but recoverable from git history). Changes are in `crates/quarto-parse-errors/src/error_table.rs`, the corpus JSON files, the `include_error_table!` macro, and the build script.

4. **Add corpus entries** for the new `(state, sym, outer_scope)` triples that the failing tests would now produce. This is the Phase E work from the previous plan.

5. **Verify** focused tests, then workspace tests, then audit snapshot churn.

### Why this avoids Attempt 4's failure

Attempt 4's walker on `consumed_tokens`:
- For `First apostrophe: a' b.\n\nSecond in bold: **c' d.**\n` it computed the outer scope of the row-2 error as `single_quote` (the row-0 leftover) instead of `strong_star` (the row-2 reality).
- Because `consumed_tokens` at end of parse contained the unreduced row-0 `single_quote` token, which the walker saw before reaching the row-2 error position.

The scanner's stack:
- After the row-0 parse fails and the parser scans through to the row-1 blank line, `clear_inline_scopes(s)` is invoked at the `BLANK_LINE_START` emission. The stack becomes empty.
- The row-2 parse begins with an empty stack. The `**` push gives `[STRONG_EMPH_STAR]`. At the row-2 error position, the stack is `[STRONG_EMPH_STAR]`.
- The outer scope is unambiguously `strong_star`, no leftover contamination.

### Risks

- The reconstruction approach (option 2b) depends on `consumed_tokens` faithfully recording every external-scanner emission. If tree-sitter elides some emissions (e.g., for tokens reduced into a non-terminal before being shifted), the reconstruction may diverge from the scanner's stack. Need to check empirically.
- The corpus JSON gets a new field. The build script and the `include_error_table!` macro need updates. The previous plan had this fully designed; it's not novel work.
- Some existing `(state, sym)` collisions in the corpus that were tolerated under tie-breaking may now produce *no* match if the new outer-scope discriminator is too strict. Need to default-fallback intelligently.
- Snapshot churn on the autogen table is unavoidable (new column).

### Effort estimate

If we choose B-tracked-only and the reconstruction-from-log approach for exposing the scope:
- Revert the gate: 5 minutes (delete the `can_close_scope` calls).
- Reconstruct walker on `consumed_tokens` using the scanner's push/pop logic: 1–2 hours, well-tested by the existing 17 + multi-paragraph cases.
- `ErrorTableEntry` + lookup extension: ~2 hours (the previous plan's Phase C work, well-scoped).
- Build-script + macro updates: ~2 hours (Phase D).
- New corpus entries for the failing cases: ~1 hour (Phase E).
- Snapshot review: variable, probably half a day.

Total: 1–2 days of focused work. Higher confidence in outcome than Option A because the discrimination mechanism is explicit rather than emergent.

## Recommendation

Spend half a day on Option A first — it's cheap to test, and if it works it's a much smaller patch than Option B. If Option A doesn't yield new distinguishable keys for the failing inputs, commit to Option B.

If we have to pick one without experimenting: Option B is more likely to succeed. The current Attempt-5 work is largely reusable in Option B (the stack tracking is exactly what Option B needs), so the path forward isn't a rewrite.

## What is NOT recommended

- Going back to Attempt 2 (grammar production duplication) or Attempt 3 (scope-tagged tokens). Both fought tree-sitter's state minimisation directly and lost.
- Modifying tree-sitter's LR generator to defeat minimisation. Out of scope for this project.
- Hand-coded post-processing of error messages keyed on input text patterns. Brittle and exactly the "ad hoc / specific" solution the user rejected.
