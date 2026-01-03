# Code Coverage Improvement Workflow

This document provides instructions for improving code coverage in the Quarto Rust workspace. Read this at the beginning of a coverage improvement session.

## Quick Reference

```bash
# Run full coverage report (record baseline at session start!)
./scripts/coverage.sh --summary

# Generate HTML report for detailed line-by-line analysis
./scripts/coverage.sh --html --open

# Run tests for a specific crate
cargo nextest run -p <crate-name>

# Run a specific test
cargo nextest run -p <crate-name> <test-name>
```

## Session Goal

**The goal of every coverage session is simple: end with higher coverage than you started.**

This creates a ratchet effect - as coverage improves over time, each session naturally targets increasingly difficult areas. There are no fixed percentage targets; we aim as close to 100% as practical, and each session moves us closer.

## Session Startup Checklist

### 1. Record the Baseline

```bash
./scripts/coverage.sh --summary 2>&1 | grep "^TOTAL"
```

Record the line coverage percentage. This is your target to beat.

Example output:
```
TOTAL    100054   30400   69.62%   5304   1385   73.89%   ...
```

Your session goal: end with line coverage > 69.62%

### 2. Identify High-Impact Targets

Run the HTML report and look for:
- **Large files with low coverage**: These offer the most improvement potential
- **Core library code**: More valuable than CLI/binary code
- **Files you can understand quickly**: Faster to write correct tests

```bash
./scripts/coverage.sh --html --open
```

### 3. Create a Tracking Issue

```bash
bd create "Improve coverage: <target description>" \
  -t task -p 2 \
  --deps parent-child:k-uoc5 \
  -d "Session baseline: X.XX% line coverage. Target: beat baseline."
```

### 4. Understand Before Testing

Read the target code thoroughly before writing tests. Understanding what the code does prevents writing tests that pass but don't actually verify correct behavior.

## Code Categories and Testing Strategies

### Category A: Pure Functions (Easiest)

**Characteristics**: No side effects, no external dependencies, clear inputs/outputs

**Strategy**: Direct unit tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_version_valid() {
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
    }

    #[test]
    fn test_parse_version_invalid() {
        assert_eq!(parse_version("not-a-version"), None);
    }
}
```

### Category B: Tree-sitter Processing Functions

**Characteristics**: Require parser context, operate on AST nodes

**Strategy**: Integration tests through higher-level parsing APIs

For files like `pampa/src/pandoc/treesitter_utils/*.rs`:

```rust
// These functions are called during parsing, so test via the parser
#[test]
fn test_code_span_processing() {
    let input = "`code here`";
    let (pandoc, _) = pampa::readers::qmd::read(
        input.as_bytes(),
        false,
        "test.qmd",
        &mut std::io::sink(),
        true,
        None,
    ).unwrap();

    // Assert the parsed output has the expected Code inline
    // This exercises the code_span processing code
}
```

### Category C: Error Paths

**Characteristics**: Code only runs when things go wrong

**Strategy**: Deliberately trigger error conditions

```rust
#[test]
fn test_invalid_yaml_schema_error() {
    let bad_schema = r#"
    type: invalid_type_name
    "#;
    let result = parse_schema(bad_schema);
    assert!(result.is_err());
    // Check specific error type/message
}
```

### Category D: Complex State Machines / Algorithms

**Characteristics**: Many branches, complex control flow

**Strategy**: Property-based testing with proptest

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn roundtrip_preserves_content(input in ".*") {
        let parsed = parse(&input);
        if let Ok(ast) = parsed {
            let output = render(&ast);
            // Property: parsing and rendering should roundtrip
        }
    }
}
```

### Category E: CLI, Server, and Binary Code

**Characteristics**: Entry points, I/O-heavy, often mix logic with interaction

**Strategy**: Refactor for testability, then test the extracted logic

This is a key insight: rather than writing complex integration tests or mock infrastructure, **refactor the code to separate concerns**:

1. **Identify the logic**: What decisions does the code make? What transformations?
2. **Extract to library functions**: Move logic into pure functions that take data and return data
3. **Leave thin wrappers**: The entry point becomes a thin shell that handles I/O and calls library code
4. **Test the library functions**: These are now easy to unit test

#### Example: Refactoring a CLI Command

**Before** (hard to test):
```rust
// src/commands/render.rs
pub fn execute(args: RenderArgs) -> Result<()> {
    let input = std::fs::read_to_string(&args.input)?;  // I/O
    let config = parse_config(&input)?;                  // Logic
    let validated = validate_config(&config)?;           // Logic
    let output = render_document(&validated)?;           // Logic
    std::fs::write(&args.output, &output)?;              // I/O
    println!("Rendered to {}", args.output);             // I/O
    Ok(())
}
```

**After** (testable):
```rust
// src/commands/render.rs - thin wrapper, hard to test but simple
pub fn execute(args: RenderArgs) -> Result<()> {
    let input = std::fs::read_to_string(&args.input)?;
    let output = render_pipeline(&input)?;  // All logic in one call
    std::fs::write(&args.output, &output)?;
    println!("Rendered to {}", args.output);
    Ok(())
}

// src/render_pipeline.rs - library code, easy to test
pub fn render_pipeline(input: &str) -> Result<String> {
    let config = parse_config(input)?;
    let validated = validate_config(&config)?;
    render_document(&validated)
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_render_pipeline_basic() {
        let input = "---\ntitle: Test\n---\n# Hello";
        let output = render_pipeline(input).unwrap();
        assert!(output.contains("<h1>Hello</h1>"));
    }
}
```

#### Benefits of This Approach

- **No mocks needed**: Pure functions don't need mock filesystems or servers
- **Faster tests**: No I/O means tests run quickly
- **Better design**: Separation of concerns improves the codebase
- **Incremental**: Can refactor one function at a time
- **Coverage improves naturally**: As logic moves to testable code, coverage increases

#### When to Apply This Strategy

Look for these signs that code needs refactoring for testability:
- Functions longer than ~50 lines mixing I/O and logic
- Hard-to-test code that "does a lot"
- 0% coverage on files that clearly have important logic
- Code where you'd need mocks to test it

## Writing Effective Tests

### Principles

1. **One behavior per test**: Each test should verify one specific behavior
2. **Minimal inputs**: Use the smallest input that exercises the code path
3. **Clear assertions**: Assert on specific values, not just "no error"
4. **Document the "why"**: Add a brief comment explaining what path is being tested

### Anti-Patterns to Avoid

```rust
// BAD: Testing presence, not correctness
assert!(result.contains("output"));

// GOOD: Testing specific expected values
assert_eq!(result.lines().count(), 3);
assert!(result.contains("expected: heading"));
```

```rust
// BAD: Giant test with many assertions
#[test]
fn test_everything() {
    // 100 lines of setup and assertions
}

// GOOD: Focused tests
#[test]
fn test_heading_level_1() { ... }

#[test]
fn test_heading_level_2() { ... }
```

## Session Workflow

### 1. Record Baseline

```bash
./scripts/coverage.sh --summary 2>&1 | grep "^TOTAL"
# Note the line coverage percentage
```

### 2. Pick a Target

Open the HTML report and find a file to improve:
```bash
./scripts/coverage.sh --html --open
```

Consider:
- What's the coverage gap? (More gap = more potential improvement)
- Is it core library code? (Higher value)
- Can you understand it quickly? (Faster progress)
- Does it need refactoring for testability? (May be valuable work regardless)

### 3. Create Tracking Issue

```bash
bd create "Improve coverage: <target>" -t task -p 2 \
  --deps parent-child:k-uoc5 \
  -d "Session baseline: XX.XX%"
bd update <issue-id> --status in_progress
```

### 4. Understand the Code

- Read the target file(s)
- Trace call sites (who calls this?)
- Look at existing tests for patterns
- Identify which lines/branches are uncovered (HTML report)

### 5. Write Tests (or Refactor + Write Tests)

For each uncovered path:
- Write the smallest test that exercises it
- Run: `cargo nextest run -p <crate>`
- Verify it passes

If the code is hard to test:
- Consider extracting testable logic
- Refactor, then write tests for the extracted code
- This counts as valid coverage work!

### 6. Verify Improvement

```bash
./scripts/coverage.sh --summary 2>&1 | grep "^TOTAL"
# Compare to baseline - coverage should be higher
```

### 7. Complete Session

```bash
bd close <issue-id> --reason "Coverage: XX.XX% -> YY.YY%"
```

## Common Patterns in This Codebase

### Pattern 1: Insta Snapshot Tests

Many tests use `insta` for snapshot testing:

```rust
use insta::assert_snapshot;

#[test]
fn test_error_message_format() {
    let error = create_error();
    assert_snapshot!(error.to_string());
}
```

Run `cargo insta review` to review/accept snapshot changes.

### Pattern 2: Test Resource Files

Tests often use files from `resources/` directories:

```rust
let input = include_str!("../resources/test-cases/example.qmd");
```

### Pattern 3: Parameterized Tests via Macros

Some tests use macros to generate test cases:

```rust
macro_rules! test_case {
    ($name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $name() {
            assert_eq!(process($input), $expected);
        }
    };
}

test_case!(test_simple, "input", "output");
test_case!(test_complex, "other", "result");
```

## Special Cases

### WASM Code (`wasm-qmd-parser/`)

WASM modules require Node.js or browser testing. See `claude-notes/instructions/testing.md` for the WASM testing workflow. This is a valid coverage target but requires the WASM test harness.

### Async Server Code (`quarto-hub/src/server.rs`)

Async code can often be refactored using the same principle as CLI code:
- Extract request handling logic into pure functions
- Test the logic separately from the async machinery
- The async wrappers become thin and less critical to test

## Escalation

If you encounter:

- **Untestable code**: Code that seems impossible to test without major refactoring - note it and ask the user. This might be a candidate for the refactoring strategy.
- **Unclear behavior**: Code that doesn't do what you expect - investigate before writing tests that might encode wrong behavior
- **Missing infrastructure**: Need test utilities that don't exist - consider whether building them is worthwhile for the coverage gain

Stop and report to the user rather than writing incorrect tests or spending excessive time on low-value targets.

## Handling Intentionally Unreachable Code

Some code paths are legitimately unreachable during normal operation - internal consistency checks, panic branches for impossible states, etc. These inflate the "uncovered lines" count but can't be meaningfully tested.

### Identifying Intentionally Unreachable Code

Common patterns:
- `panic!("Internal error: ...")` in match arms that should never be reached
- `unreachable!()` macros
- Error branches that indicate programming errors rather than user errors
- Defensive checks that guard against impossible states

### Option 1: Exclude from Coverage with `#[coverage(off)]`

Use the `#[coverage(off)]` attribute (nightly only) to exclude specific functions:

```rust
#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

#[cfg_attr(coverage_nightly, coverage(off))]
fn internal_consistency_check() {
    // This panic indicates a bug in our code, not user error
    panic!("Internal error: state should never be None here");
}
```

**Required Cargo.toml configuration** (to silence `unexpected_cfgs` warnings):

```toml
[lints.rust]
unexpected_cfgs = { level = "warn", check-cfg = ['cfg(coverage,coverage_nightly)'] }
```

### Option 2: Refactor to Return Result/Option

If the panic could be replaced with proper error handling:

```rust
// Before: panic branch that's hard to test
fn get_value(map: &HashMap<String, Value>, key: &str) -> Value {
    map.get(key).cloned().expect("key must exist")  // Uncovered panic
}

// After: caller handles the error case
fn get_value(map: &HashMap<String, Value>, key: &str) -> Option<Value> {
    map.get(key).cloned()  // No panic to cover
}
```

### When to Use Each Approach

| Situation | Approach |
|-----------|----------|
| True internal invariant violation | `#[coverage(off)]` |
| Could legitimately fail at runtime | Refactor to `Result`/`Option` |
| Match arm that "can't happen" | Consider if it actually can, then `#[coverage(off)]` |
| Defensive programming check | `#[coverage(off)]` with comment explaining why |

### Documenting Coverage Exclusions

When using `#[coverage(off)]`, add a comment explaining why:

```rust
#[cfg_attr(coverage_nightly, coverage(off))]
// Coverage exclusion: This panic is an internal consistency check.
// The grammar guarantees this node always has a child, so the else
// branch is unreachable during normal parsing.
fn process_node(node: &Node) -> Result {
    if let Some(child) = node.child(0) {
        process_child(child)
    } else {
        panic!("Grammar guarantees child exists")
    }
}
```

## Investigating Apparent Limitations

When you encounter code that "can't be tested" or a feature that "doesn't work," **investigate thoroughly before concluding it's a fundamental limitation**.

### The Investigation Process

1. **Use verbose/debug output**: Many tools have `-v` flags that show internal state
2. **Trace the data flow**: Where does the input come from? What transforms it?
3. **Check the grammar/schema**: Is the structure what you expect?
4. **Read the processing code**: Does it handle the structure correctly?

### Example: A "Parser Limitation" That Wasn't

During a coverage session, quoted strings in shortcodes appeared to produce empty output:

```
{{< include "file with spaces.qmd" >}}
→ data-value: ""  (empty!)
```

**Initial (wrong) conclusion**: "The parser has a limitation with quoted strings."

**Proper investigation**:
1. Ran with `-v` flag - saw the grammar correctly produced a `shortcode_string` node
2. Checked the grammar - it properly defined quoted strings with escape handling
3. Read the processing code - found it was calling `node.child(0)` expecting a named child
4. Realized anonymous grammar rules (`_commonmark_*_quote_string`) don't create named children
5. **Actual bug**: Code assumed child nodes existed when the grammar inlined them

**The fix**: Extract full node text and strip quotes, rather than accessing non-existent children.

### Red Flags That Warrant Investigation

- "The parser doesn't support X" - Did you verify with verbose output?
- "This can't be tested" - Is it the code or your test approach?
- "The grammar is wrong" - Did you trace through the processing code?
- Empty or unexpected output - Where does the data get lost?

### Reporting Findings

When you investigate and find an actual bug:

1. **Document the symptoms**: What behavior did you observe?
2. **Document the root cause**: What was actually wrong?
3. **Write a test that fails first**: Confirm the bug exists
4. **Fix the bug**: Address the root cause
5. **Verify the test passes**: Confirm the fix works
6. **Add regression tests**: Prevent future reoccurrence

This turns a "coverage session" into genuine bug discovery - high-value work!

## Dead Code Discovery

When investigating files with 0% coverage, you may discover **dead code** - code that is declared but never used. This is valuable discovery work.

### How to Identify Dead Code

A file is likely dead code if:
1. It's declared as a module in `mod.rs` but never imported anywhere
2. A similar `*_helpers.rs` file exists that supersedes it
3. No other file contains `use <module>::` or `<module>::function()` patterns

### Investigation Steps

1. Search for imports of the module:
   ```bash
   grep -r "module_name::" crates/
   ```

2. Check if there's a replacement file (common pattern: `foo.rs` replaced by `foo_helpers.rs`)

3. Verify the module is declared but unused in `mod.rs`

### Reporting Dead Code

When you find dead code:

1. **Create a beads issue** documenting the finding:
   ```bash
   bd create "Remove dead code: <filename>" -t task -p 3 \
     -d "File is never imported or used. [Reason it's dead, e.g., 'Superseded by foo_helpers.rs']"
   ```

2. **Write a brief report** explaining:
   - What file(s) are dead code
   - Evidence they're unused (no imports found)
   - What replaced them (if applicable)
   - Recommendation for removal

3. **Do not remove the code** in the coverage session - that's a separate task

### Example Dead Code Report

```
## Dead Code Finding: code_span.rs

**File**: `pampa/src/pandoc/treesitter_utils/code_span.rs` (107 lines)

**Evidence**:
- Module declared in mod.rs (line 13)
- Zero imports found: `grep -r "code_span::" crates/` returns nothing
- Superseded by: `code_span_helpers.rs` which IS imported and used

**Recommendation**: Remove `code_span.rs` and its declaration in mod.rs.
The functionality is fully handled by `code_span_helpers.rs`.

**Issue created**: k-js4l
```

Dead code artificially inflates the "uncovered lines" count, making coverage percentages appear worse than they are. Identifying and removing dead code is a legitimate form of coverage improvement.

## Tracking Progress Over Time

The epic `k-uoc5` tracks overall coverage work. Each session should:

1. Create a child task under the epic
2. Record baseline in the task description
3. Record final coverage when closing

This creates a history of coverage improvements that shows progress toward the goal of comprehensive test coverage.
