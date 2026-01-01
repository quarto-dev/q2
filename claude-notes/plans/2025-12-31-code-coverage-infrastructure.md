# Plan: Code Coverage Infrastructure

## Problem Statement

We need to understand the current test coverage of our Rust crates to identify gaps and prioritize future testing efforts. Currently, we have no visibility into which parts of the codebase are exercised by tests.

## Goals

1. Set up tooling to measure code coverage for the workspace
2. Generate coverage reports that show per-crate and per-file coverage
3. Establish a baseline for current coverage
4. Create a workflow for monitoring coverage changes

## Rust Code Coverage Options

### Option A: cargo-llvm-cov (Recommended)

**Tool**: [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov)

**Pros**:
- Uses LLVM's native coverage instrumentation (accurate)
- Well-maintained, active development
- Supports multiple output formats (text, HTML, lcov, JSON)
- Works with cargo nextest
- Can generate per-crate and aggregate reports

**Cons**:
- Requires nightly Rust or specific LLVM version
- Slower compilation with instrumentation

**Installation**:
```bash
cargo install cargo-llvm-cov
```

**Basic Usage**:
```bash
# Run tests and show summary
cargo llvm-cov nextest

# Generate HTML report
cargo llvm-cov nextest --html --output-dir coverage

# Generate lcov format (for CI integration)
cargo llvm-cov nextest --lcov --output-path lcov.info
```

### Option B: grcov + source-based coverage

**Tool**: [grcov](https://github.com/mozilla/grcov)

**Pros**:
- Mozilla-maintained
- Multiple output formats
- Can work with stable Rust (using gcov-style instrumentation)

**Cons**:
- More complex setup
- Less accurate than LLVM instrumentation

### Option C: Tarpaulin

**Tool**: [cargo-tarpaulin](https://github.com/xd009642/tarpaulin)

**Pros**:
- Simple to use
- Good CI integration

**Cons**:
- Linux only (doesn't work on macOS)
- Less actively maintained

## Recommended Approach: cargo-llvm-cov

Given that we're on macOS and using cargo nextest, cargo-llvm-cov is the best choice.

## Implementation Plan

### Step 1: Install cargo-llvm-cov

```bash
cargo install cargo-llvm-cov
```

Verify installation:
```bash
cargo llvm-cov --version
```

### Step 2: Run initial coverage measurement

```bash
# Full workspace coverage with nextest
cargo llvm-cov nextest --workspace

# Generate HTML report for detailed analysis
cargo llvm-cov nextest --workspace --html --output-dir coverage
```

### Step 3: Create coverage script

Create a script `scripts/coverage.sh` to standardize coverage runs:

```bash
#!/bin/bash
set -euo pipefail

# Run coverage and generate reports
cargo llvm-cov nextest --workspace --html --output-dir coverage

echo "Coverage report generated in coverage/"
echo "Open coverage/index.html to view"
```

### Step 4: Add .gitignore entries

Add to `.gitignore`:
```
# Code coverage
coverage/
lcov.info
*.profraw
*.profdata
```

### Step 5: Document coverage workflow

Add documentation for:
- How to run coverage locally
- How to interpret reports
- Coverage goals and thresholds

### Step 6: (Optional) CI Integration

For future CI integration, we could:
- Generate lcov format: `cargo llvm-cov nextest --lcov --output-path lcov.info`
- Upload to codecov.io or similar service
- Set coverage thresholds for PRs

## Coverage Report Interpretation

### Key Metrics

1. **Line coverage**: Percentage of lines executed during tests
2. **Function coverage**: Percentage of functions called during tests
3. **Branch coverage**: Percentage of branches (if/else) taken

### Priority Areas

Focus coverage improvements on:
1. Core parsing logic (`quarto-yaml`, `tree-sitter-qmd`)
2. Error handling paths
3. Public API functions
4. Edge cases in complex algorithms

### Expected Baseline

For a project of this size, initial coverage might be:
- 40-60% overall (typical for projects without coverage focus)
- Higher in utility crates
- Lower in complex parsing/rendering code

## Future Enhancements

1. **Coverage trends**: Track coverage over time
2. **Coverage gates**: Require minimum coverage for new code
3. **Uncovered code reports**: Automated reports of untested code paths
4. **Per-PR coverage diff**: Show coverage impact of changes

## Success Criteria

1. Can run coverage measurement locally
2. HTML report shows per-file coverage
3. Baseline coverage numbers documented
4. Coverage artifacts properly gitignored

---

## Implementation Status: COMPLETE

### Baseline Coverage (2025-12-31)

| Metric | Coverage |
|--------|----------|
| **Line coverage** | 69.62% |
| **Function coverage** | 73.89% |
| **Region coverage** | 70.96% |

### Notable Coverage by Crate

**High coverage (>90%)**:
- `quarto-source-map`: 96-100%
- `quarto-yaml/parser.rs`: 93.46%
- `quarto-csl/parser.rs`: 92.68%
- `quarto-doctemplate/resolver.rs`: 100%
- `quarto-system-runtime/native.rs`: 92.22%

**Medium coverage (50-90%)**:
- `quarto-core`: ~80%
- `quarto-yaml-validation`: 61-87%
- `quarto-pandoc-types`: 50-93% (varies by module)

**Low coverage (<50%)**:
- `quarto/src/commands/*`: 0% (CLI entry points, expected)
- `quarto-hub/src/server.rs`: 0% (async server code)
- `wasm-qmd-parser`: 0% (WASM module)
- `validate-yaml/src/main.rs`: 0% (CLI tool)

### Files Installed

1. **`scripts/coverage.sh`** - Coverage script with options:
   - `--html` - Generate HTML report (default)
   - `--lcov` - Generate lcov.info for CI
   - `--json` - Generate JSON report
   - `--summary` - Quick summary only
   - `--open` - Open HTML in browser

2. **`.gitignore`** - Updated with coverage entries

3. **HTML report** - Available at `coverage/html/index.html`
