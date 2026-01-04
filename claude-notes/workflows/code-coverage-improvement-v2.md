# Code Coverage Improvement Workflow (v2)

This document provides instructions for systematically improving code coverage across the Quarto Rust workspace using a file-by-file approach.

## Goal

**Achieve 100% line coverage on all testable code**, with explicit `#[coverage(off)]` annotations for genuinely untestable paths. Every line of code should either be covered by tests or explicitly marked as untestable with documented justification.

## Quick Reference

```bash
# IMPORTANT: Always use per-crate coverage during active work (much faster!)
# Only run full workspace coverage for periodic verification

# Run coverage for a SINGLE CRATE (fast - use this during active work)
cargo llvm-cov nextest -p <crate-name>

# Run coverage for a single crate and grep for specific file
cargo llvm-cov nextest -p <crate-name> 2>&1 | grep "path/to/file.rs"

# Run full workspace coverage (slow - only for periodic verification)
./scripts/coverage.sh --summary

# Run tests for a specific crate (no coverage, even faster)
cargo nextest run -p <crate-name>

# Run a specific test
cargo nextest run -p <crate-name> <test-name>

# Find all coverage exclusions in codebase
grep -r "coverage(off)" crates/ --include="*.rs"

# Count excluded functions
grep -r "coverage(off)" crates/ --include="*.rs" | wc -l

# Run coverage WITHOUT exclusions (see what we're hiding)
cargo llvm-cov nextest --workspace --no-cfg-coverage-nightly
```

**CRITICAL: Use per-crate coverage during active work.** Running `cargo llvm-cov nextest -p <crate-name>` is dramatically faster than running full workspace coverage. Only run full workspace coverage during periodic verification (once per session start).

**Comparing coverage with vs without exclusions:**

The `--no-cfg-coverage-nightly` flag disables the `coverage_nightly` cfg, which means `#[cfg_attr(coverage_nightly, coverage(off))]` annotations are ignored. Comparing the two reports shows how much coverage we're excluding.

## Progress Tracking

Maintain a checklist at `claude-notes/coverage/progress.md` with this format:

```markdown
# Coverage Progress

Last verified against coverage report: YYYY-MM-DD

## Crate: crate-name

| File | Status | Coverage | Notes |
|------|--------|----------|-------|
| src/foo.rs | done | 100% | |
| src/bar.rs | done | 98% | 2 lines excluded: panic in unreachable match arm |
| src/baz.rs | in_progress | 67% | Working on error paths |
| src/qux.rs | blocked | 45% | Requires mock filesystem - needs design decision |
| src/main.rs | skipped | 0% | Thin CLI wrapper, logic tested via library |
```

**Status values:**
- `not_started` - Not yet worked on
- `in_progress` - Currently being worked on
- `done` - 100% coverage (or 100% of testable code with exclusions documented)
- `blocked` - Cannot progress without external input/decision
- `skipped` - Intentionally not covered (e.g., thin CLI wrappers)

**File ordering:** Process files by crate (alphabetically), then by file path (alphabetically) within each crate.

## Session Workflow

### 1. Pick the Next File

Open `claude-notes/coverage/progress.md` and find the first file with status `not_started` or `in_progress`. If resuming an `in_progress` file, read any notes from the previous session.

### 2. Check Current Coverage

Use per-crate coverage for speed:

```bash
# Fast: per-crate coverage (use this!)
cargo llvm-cov nextest -p <crate-name> 2>&1 | grep "path/to/file.rs"

# Slow: only if you need full workspace context
./scripts/coverage.sh --summary 2>&1 | grep "path/to/file.rs"
```

Note the current line coverage percentage.

### 3. Study the File

Read the file thoroughly before writing tests. Understand:
- What the code does
- Which functions are public vs private
- What the expected behavior is
- Which code paths exist (success, error, edge cases)

### 4. Identify Uncovered Lines

Generate the HTML report to see exactly which lines are uncovered:

```bash
./scripts/coverage.sh --html --open
```

Navigate to the file and note the red (uncovered) lines.

### 5. Write Tests

For each uncovered code path:
1. Write a test that exercises it
2. Run the test to verify it passes: `cargo nextest run -p <crate-name> <test-name>`
3. Re-check coverage using per-crate command: `cargo llvm-cov nextest -p <crate-name>`

See "Writing Tests" section below for patterns.

### 6. Handle Untestable Code

If you encounter code that genuinely cannot be tested, see "Handling Untestable Code" section below.

### 7. Update Checklist

When done with the file (or blocked), update `claude-notes/coverage/progress.md`:
- Set status to `done`, `blocked`, or leave as `in_progress`
- Update coverage percentage
- Add notes explaining any exclusions or blockers

### 8. Move to Next File

If the file is `done` or `blocked`, proceed to the next file in order.

## Writing Tests

### Test Location

- For library code: Add tests in the same file using `#[cfg(test)] mod tests { ... }`
- For complex test scenarios: Create a test file in `tests/` directory
- Follow existing patterns in the crate

### Test Principles

1. **One behavior per test** - Each test verifies one specific behavior
2. **Minimal inputs** - Use the smallest input that exercises the code path
3. **Clear assertions** - Assert on specific values, not just "no error"
4. **Document the path** - Brief comment explaining which code path is being tested

### Common Patterns

**Testing error paths:**
```rust
#[test]
fn test_invalid_input_returns_error() {
    let result = parse("invalid input");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("expected"));
}
```

**Testing match arms:**
```rust
#[test]
fn test_handles_variant_a() {
    let input = MyEnum::VariantA(42);
    let result = process(input);
    assert_eq!(result, expected_for_a);
}
```

**Testing through public APIs (for private functions):**
```rust
// If private_helper() is called by public_function(), test via public_function()
#[test]
fn test_public_function_exercises_helper() {
    let result = public_function(input_that_triggers_helper);
    // Assert on observable outcome
}
```

## Handling Untestable Code

### When to Use `#[coverage(off)]`

Use coverage exclusions **only** for code that is genuinely untestable:

1. **Unreachable match arms** - When type system/grammar guarantees a branch can't be taken
2. **Internal consistency panics** - `panic!("internal error: ...")` for impossible states
3. **Platform-specific code** - Code that only runs on platforms not in CI

### How to Apply Exclusions

```rust
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
fn internal_consistency_check() {
    // This panic indicates a bug in our code, not user error
    panic!("Internal error: state should never be None here");
}
```

For single match arms:
```rust
match node.kind() {
    "expected_kind" => process(node),
    #[cfg_attr(coverage_nightly, coverage(off))]
    _ => panic!("Grammar guarantees this is unreachable"),
}
```

### Required Documentation

When adding `#[coverage(off)]`, you MUST:
1. Add a comment explaining why the code is untestable
2. Note it in the checklist for that file
3. The comment should explain what would need to change for this to become testable

### What NOT to Exclude

Do NOT use `#[coverage(off)]` for:
- Code that's "hard to test" but technically testable
- Error handling that could be triggered by user input
- Code that would require refactoring to test (refactor it instead)
- Code that needs mocks/fixtures you haven't built yet

### Dead Code

If you discover code that is never called:
1. Verify it's truly dead (search for usages)
2. Do NOT write tests for dead code
3. Create a beads issue for removal: `bd create "Remove dead code: <file/function>" -t task -p 3`
4. Note in checklist as "dead code - removal issue created"

## Binary and CLI Code

### Thin Wrappers

Files like `main.rs` or `commands/*.rs` that are thin wrappers around library code can be marked as `skipped`:
- They typically just parse args and call library functions
- The library functions should have full coverage
- Testing the wrapper provides little value

### Logic in CLI Code

If CLI code contains significant logic (not just arg parsing and delegation):
1. **Prefer refactoring**: Extract logic into testable library functions
2. If refactoring is impractical, test the logic where it is
3. Document in checklist why the code lives in CLI layer

## Periodic Verification

At least once per day (or when starting a new session):

1. Run full coverage report:
   ```bash
   ./scripts/coverage.sh --summary
   ```

2. Compare against checklist - verify files marked `done` still show expected coverage

3. Update "Last verified" date in checklist

4. Review coverage exclusions:
   ```bash
   grep -r "coverage(off)" crates/ --include="*.rs" -B2 -A2
   ```
   Ensure each exclusion still has valid justification.

5. **Weekly: Audit exclusion impact**
   ```bash
   # Coverage WITH exclusions
   ./scripts/coverage.sh --summary 2>&1 | grep "^TOTAL"

   # Coverage WITHOUT exclusions (rebuilds, takes longer)
   cargo llvm-cov nextest --workspace --no-cfg-coverage-nightly 2>&1 | grep "^TOTAL"
   ```

   If the gap is growing too large (e.g., >5%), review recent exclusions to ensure they're justified.

## Escalation

Stop and ask for help when:

1. **Unclear behavior** - Code does something unexpected; don't encode wrong behavior in tests
2. **Architectural blocker** - Testing requires infrastructure that doesn't exist (mocks, fixtures)
3. **Excessive exclusions** - If a file needs >10% exclusions, something may be wrong
4. **Refactoring needed** - Code should be restructured but change is significant

When escalating:
- Document what you tried
- Explain why you're blocked
- Propose options if you have ideas

## Initial Setup

To initialize the progress checklist:

1. Run coverage report and extract all files with <100% coverage
2. Group by crate, sort alphabetically
3. Create initial checklist with all files as `not_started`
4. Commit the checklist

This is a one-time setup; after that, just follow the session workflow.
