# Issue #222 — Non-deterministic diagnostic output

- **GitHub**: https://github.com/quarto-dev/q2/issues/222
- **Reporter**: @rundel (Colin Rundel), 2026-05-21
- **Triage date**: 2026-05-20
- **Worktree**: `.worktrees/issue-222` (branch `issue-222`, based on `main` @ `99e7f89c`)
- **Beads issue**: bd-hwdlq
- **Scope**: the variable second diagnostic (Q-2-5 Underscore vs. second Q-2-11 Double Quote) on the reported input. Both variants share the same first diagnostic, which is not in scope.

## Summary

Reproduced. The tree-sitter parse trace is byte-identical across runs (verified across 15 runs), so the GLR parser itself is deterministic on this input. **All of the nondeterminism is downstream**, in `quarto-parse-errors`: a `HashMap<usize, TreeSitterProcessLog>` keyed by GLR version number is iterated by `.values()` to extract diagnostic states, and a `(row, column)` dedupe drops every state but the first. With three GLR branches all hitting `detect_error` at `(row=0, col=18)`, whichever HashMap bucket is iterated first wins. Default `RandomState` randomizes per process. Confirmed by swapping `HashMap` → `BTreeMap` locally — 30/30 runs then produce Variant A. Fix is one line; the recommendation below sticks with BTreeMap, but lists the alternatives and the audit obligations.

The user's clarification on the ticket is "either diagnostic is acceptable, as long as the tie is always broken consistently to one of them." Both `HashMap`→`BTreeMap` (deterministically pick lowest GLR version) and `HashMap`→sort-then-iterate satisfy that.

## Reproduction

```bash
printf -- 'The "_blank" word.' | cargo run --bin pampa -- --no-prune-errors
```

Run repeatedly. Across 30 runs at `99e7f89c` on macOS arm64:

| Variant | Second diagnostic | Count |
|---|---|---|
| A | `Q-2-5` Unclosed Underscore Emphasis @ col 19 | 19 / 30 (63%) |
| B | `Q-2-11` Unclosed Double Quote with opener at col 5 @ col 19 | 11 / 30 (37%) |

The first diagnostic — `Q-2-11` Unclosed Double Quote with opener at col 13 — is the same in both.

Fixture: `repro.qmd` next to this file.

## Investigation

### Step 1 — is the tree-sitter parse itself deterministic?

Captured the verbose tree-sitter parse trace (`-v`) into `/tmp/issue222/run-1.txt … run-15.txt`. Diffed the trace portion (`tree-sitter parse:` … `---`) across all 15 runs. **Byte-identical, every pair.** Runs that produced Variant A and runs that produced Variant B have the same parse trace.

This rules out tree-sitter (the upstream C library), the `tree-sitter-qmd` grammar, and the GLR scheduling — those all run identically every time. The trace itself records three concurrent GLR versions (`version_count:3`) splitting at column 12 after the `_whitespace` recovery, then condensing at the close of the block.

### Step 2 — where does the variation enter?

The diagnostic generator pulls error states off the parse log, not off the final concrete syntax tree:

- `crates/pampa/src/readers/qmd.rs:124` calls `produce_diagnostic_messages(input_bytes, &log_observer, …)`.
- `crates/quarto-parse-errors/src/error_generation.rs:43-62`:

  ```rust
  for parse in &tree_sitter_log.parses {
      for process_log in parse.processes.values() {     // <-- HashMap iteration
          for state in process_log.error_states.iter() {
              if seen_errors.contains(&(state.row, state.column)) {
                  continue;
              }
              seen_errors.insert((state.row, state.column));
              let diagnostic = error_diagnostic_from_parse_state(…);
              result.push(diagnostic);
          }
      }
  }
  result.sort_by_key(|diag| diag.location.as_ref().map_or(0, |loc| loc.start_offset()));
  ```

- `crates/quarto-parse-errors/src/tree_sitter_log.rs:48`:

  ```rust
  pub processes: HashMap<usize, TreeSitterProcessLog>,
  ```

  with `use std::collections::HashMap` at line 11. Default `RandomState` ⇒ iteration order varies per process.

The three GLR branches each push a `ProcessMessage` to their own `error_states` at the same `(row=0, column=18)` (this is what tree-sitter reports for col 19 1-indexed). The dedupe lets exactly one through. *Which one* depends on HashMap iteration order — which is the bug.

The final `sort_by_key` on `start_offset` is a red herring: the first diagnostic is at col 13, the second at col 19, so the sort never changes the order; it just confirms the second slot is always "whatever survives the dedupe."

### Step 3 — confirm the fix

Local experimental change to `tree_sitter_log.rs`: `use std::collections::BTreeMap as HashMap;` (so `processes` becomes a `BTreeMap<usize, …>` everywhere it's used).

Result: 30/30 runs produce Variant A (Q-2-5 Underscore Emphasis). BTreeMap iterates keys in ascending order ⇒ GLR version 0 wins ⇒ Variant A. Reverted the change before recording this triage.

### Step 4 — audit for other order-dependent HashMap/HashSet in the diagnostic path

Grep across `crates/quarto-parse-errors/`, `crates/pampa/src/readers/`, and `crates/quarto-error-reporting/`. Findings:

- `tree_sitter_log.rs:48` — `processes: HashMap<usize, _>`. **Iterated in `error_generation.rs:44` and `tree_sitter_log.rs:72` (`is_good`).** The `is_good` iteration is a boolean AND over all values — order-independent. The `error_generation.rs:44` iteration is the bug.
- `error_generation.rs:40` — `seen_errors: HashSet<(usize, usize)>`. Used only for `contains` / `insert`. Set membership is order-independent.
- `error_generation.rs:358` and `pampa/.../qmd_error_messages.rs:100` — both reach into `parse.processes[&0]` (hardcoded GLR version 0). Already deterministic.
- `error_generation.rs:648` — `kept_set: HashSet<usize>` built from a `kept_indices` Vec; used for membership only.
- `error_generation.rs:666` — HashMap inside a unit test. Not user-facing.

So the production fix is one site. The audit conclusion is that there is no other latent non-determinism in this pipeline.

## Localization

- **The bug**: `crates/quarto-parse-errors/src/tree_sitter_log.rs:48` (HashMap declaration) → `crates/quarto-parse-errors/src/error_generation.rs:44` (iteration site).
- **Working analogue**: `error_generation.rs:361` and `qmd_error_messages.rs:103` already pin to `parse.processes[&0]`, which sidesteps the issue but loses information from non-zero GLR versions. Fine for the JSON-corpus path which only needs the main parse; not appropriate for `produce_diagnostic_messages` which needs to see all versions.

## Open questions — resolved during triage

- *Is the tree-sitter parse non-deterministic?* No. Trace is byte-identical across 15 runs.
- *Is the variation only in the second diagnostic?* Yes. The first diagnostic (Q-2-11 at col 13) comes from a process that hits its `detect_error` at a different `(row, column)` than the others (col 12-ish vs col 18), so the dedupe doesn't apply. Only the col-18 trio competes.
- *Does the final `sort_by_key` matter?* No. The first/second diagnostics are at different columns, so sorting never reorders them.
- *Is BTreeMap (version 0 wins) the right tie-break?* Acceptable per the user's clarification ("either diagnostic is acceptable, as long as the tie is always broken consistently"). Variant A is also the trace's "main line" (version 0 is the version that continued through the recovery), so it has weak intuitive grounding.

## Outcome / recommended next step

Filed bd-hwdlq with the fix scope:

1. **Fix**: change `processes: HashMap<usize, TreeSitterProcessLog>` to `BTreeMap<usize, TreeSitterProcessLog>` in `crates/quarto-parse-errors/src/tree_sitter_log.rs:48`. Localized, one import + one type swap. Adjust `processes: HashMap::new()` at the call sites accordingly (or keep an `HashMap as` re-alias).

2. **Regression test**: add a test that parses the issue-222 input N times in-process (e.g. N=20) and asserts that all N runs produce byte-identical diagnostic output. The test will fail on the current code (probabilistically — N=20 has ≥99.9% chance of failure with the observed 63/37 split) and pass under the fix. Place under `crates/pampa/tests/` or in a new test on `quarto-parse-errors`.

3. **Audit follow-up** *(optional, not required for this fix)*: grep for `HashMap<.*Log\|HashMap<.*Diagnostic\|HashMap<.*Error` across the crate tree and verify each iteration site either preserves insertion order or sorts. We did the diagnostic-path audit here; broader codebase audit is orthogonal.

## Verification commands used

```bash
# Pre-flight (Rust side only — hub-client tests fail on this fresh worktree
# due to wasm-quarto-hub-client not being built, unrelated to issue #222)
cargo xtask verify --skip-hub-build --skip-hub-tests --skip-shared-package-tests \
                   --skip-trace-viewer-build --skip-trace-viewer-tests \
                   --skip-q2-preview-spa-build

# Reproduce (with variant counter)
for i in $(seq 1 30); do
  out=$(printf -- 'The "_blank" word.' | cargo run --bin pampa --quiet -- --no-prune-errors 2>&1)
  if echo "$out" | grep -q "Q-2-5"; then echo "A"; else echo "B"; fi
done | sort | uniq -c

# Trace determinism check
for i in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
  printf -- 'The "_blank" word.' | cargo run --bin pampa --quiet -- --no-prune-errors -v \
    > /tmp/issue222/run-$i.txt 2>&1
done
for i in 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
  diff -q <(awk '/tree-sitter parse:/,/^---$/' /tmp/issue222/run-1.txt) \
          <(awk '/tree-sitter parse:/,/^---$/' /tmp/issue222/run-$i.txt)
done

# Fix-confirmation experiment (REVERTED before committing this triage):
#   `use std::collections::HashMap;` -> `use std::collections::BTreeMap as HashMap;`
#   in crates/quarto-parse-errors/src/tree_sitter_log.rs
#   then re-run the variant counter -> 30/30 Variant A.
```

## Cross-references

- Issue: https://github.com/quarto-dev/q2/issues/222
- Reporter: @rundel
- Code: `crates/quarto-parse-errors/src/{tree_sitter_log.rs, error_generation.rs}`
- Caller: `crates/pampa/src/readers/qmd.rs:124`
- Project rule on HashMap nondeterminism: noted in passing in the conversation; no explicit `CLAUDE.md` rule yet (recommend one if the audit follow-up surfaces more instances).
