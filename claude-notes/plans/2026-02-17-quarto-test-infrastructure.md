# Quarto Test Infrastructure Plan

## Overview

Implement smoke-all style document testing for q2, enabling tests to be embedded directly in QMD files via `_quarto.tests` YAML metadata. This mirrors Quarto 1's testing approach while providing idiomatic Rust integration.

**Goals:**
- `quarto call test <files...>` CLI command for manual/CI use
- `quarto-test` crate with assertion/verification infrastructure
- Cargo test integration for `cargo nextest run` compatibility
- Compatible YAML format with Quarto 1 smoke-all tests

---

## Phase 1: Core Infrastructure

### 1.1 Create `quarto-test` crate

- [x] Create `crates/quarto-test/Cargo.toml` with dependencies:
  - `regex` for pattern matching
  - `anyhow` for error handling
  - `serde`, `serde_yaml` for YAML parsing
  - `quarto-yaml` for frontmatter extraction
  - `quarto-core` for rendering

- [x] Create `crates/quarto-test/src/lib.rs` with public API:
  ```rust
  pub fn run_test_file(path: &Path) -> Result<TestResult>;
  pub fn run_test_files(paths: &[PathBuf]) -> Result<TestSummary>;
  ```

### 1.2 Test Specification Parsing

- [x] Create `crates/quarto-test/src/spec.rs`:
  - `TestSpec` struct representing a single format's test configuration
  - `RunConfig` struct for skip/ci/os conditions
  - `parse_test_specs(yaml: &Value) -> Result<Vec<TestSpec>>`
  - Support for `_quarto.tests.run` skip conditions

### 1.3 Assertion System

- [x] Create `crates/quarto-test/src/assertions/mod.rs`:
  ```rust
  pub trait Assertion {
      fn name(&self) -> &str;
      fn verify(&self, context: &VerifyContext) -> Result<()>;
  }

  pub struct VerifyContext {
      pub output_path: PathBuf,
      pub input_path: PathBuf,
      pub format: String,
  }
  ```

- [x] Create `crates/quarto-test/src/assertions/file_regex.rs`:
  - `EnsureFileRegexMatches { matches: Vec<Regex>, no_matches: Vec<Regex> }`
  - Reads output file, tests all patterns
  - Clear error messages showing which pattern failed

### 1.4 Test Runner

- [x] Create `crates/quarto-test/src/runner.rs`:
  - `TestRunner` that orchestrates: parse specs → render → verify
  - `TestResult` enum: `Pass`, `Fail(Vec<FailureDetail>)`, `Skipped(String)`
  - `TestSummary` for multiple files: passed/failed/skipped counts

---

## Phase 2: CLI Integration

### 2.1 Update `quarto call` command

- [x] Modify `crates/quarto/src/main.rs`:
  - Change `Call` variant to capture function name and args properly
  - Pass function name to `commands::call::execute`

- [x] Rewrite `crates/quarto/src/commands/call/mod.rs`:
  ```rust
  pub fn execute(function: Option<String>, args: Vec<String>) -> Result<()> {
      match function.as_deref() {
          Some("test") => test::execute(args),
          Some(other) => Err(anyhow!("Unknown function: {}", other)),
          None => Err(anyhow!("Usage: quarto call <function> [args...]")),
      }
  }
  ```

- [x] Create `crates/quarto/src/commands/call/test.rs`:
  - Parse file arguments
  - Call `quarto_test::run_test_files`
  - Report results to stdout
  - Exit with appropriate code (0 = all pass, 1 = failures)

### 2.2 Output Format

- [x] Implement clear CLI output:
  ```
  Running tests for 1 file(s)...

  Results: 1 passed, 0 failed, 0 skipped
  ```

- [ ] Add `--verbose` flag for detailed assertion output (future)
- [ ] Add `--json` flag for machine-readable output (future)

---

## Phase 3: Cargo Test Integration

### 3.1 Integration Test Discovery

- [x] Create `crates/quarto/tests/smoke_all.rs`:
  ```rust
  macro_rules! smoke_test {
      ($name:ident, $path:literal) => {
          #[test]
          fn $name() {
              // ... run test and assert
          }
      };
  }

  smoke_test!(basic_render, "basic-render.qmd");
  ```

### 3.2 Test Discovery Script (optional)

- [ ] Create `scripts/generate-smoke-tests.sh` to auto-generate test declarations
- [ ] Or: use `#[test_case]` from `test-case` crate for data-driven tests

---

## Phase 4: Initial Test Files

### 4.1 Create smoke-all directory

- [x] Create `smoke-all/` at repo root (next to `crates/`)

### 4.2 Create initial test files

- [x] `smoke-all/basic-render.qmd`: Basic HTML render test
- [ ] `smoke-all/callout-note.qmd`: Test callout rendering
- [ ] `smoke-all/code-block.qmd`: Test code block rendering

---

## Phase 5: Additional Assertions (Future)

### 5.1 HTML-specific assertions

- [x] `ensureHtmlElements(selectors, noMatchSelectors)` - CSS selector presence (scraper crate)
- [ ] `ensureHtmlElementContents(selector, matches, noMatches)` - element text content
- [ ] `ensureHtmlElementCount(selector, count)` - element counting

### 5.2 Error handling assertions

- [ ] `noErrors` - no ERROR level messages during render
- [ ] `noErrorsOrWarnings` - no ERROR or WARN messages
- [ ] `shouldError` - expect render to fail

### 5.3 Snapshot testing

- [ ] `ensureSnapshotMatches` - compare output to `.snapshot` file

---

## File Structure

```
q2/
└── crates/
    ├── quarto/
    │   ├── src/
    │   │   ├── commands/
    │   │   │   └── call/
    │   │   │       ├── mod.rs       # Dispatch to subcommands
    │   │   │       └── test.rs      # CLI test runner
    │   │   └── main.rs
    │   └── tests/
    │       ├── smoke_all.rs         # Cargo test integration
    │       └── smoke-all/           # Test documents
    │           ├── basic-render.qmd
    │           ├── callout-note.qmd
    │           └── code-block.qmd
    └── quarto-test/
        ├── Cargo.toml
        └── src/
            ├── lib.rs               # Public API
            ├── spec.rs              # Test spec parsing
            ├── runner.rs            # Test execution
            └── assertions/
                ├── mod.rs           # Assertion trait
                └── file_regex.rs    # ensureFileRegexMatches
```

---

## Dependencies

```toml
# crates/quarto-test/Cargo.toml
[dependencies]
anyhow = "1.0"
regex = "1.10"
serde = { version = "1.0", features = ["derive"] }
serde_yaml = "0.9"
quarto-yaml = { path = "../quarto-yaml" }
quarto-core = { path = "../quarto-core" }
thiserror = "1.0"
```

---

## Success Criteria

1. ✅ `quarto call test smoke-all/basic-render.qmd` renders and verifies assertions
2. ✅ `cargo nextest run -p quarto --test smoke_all` runs all smoke tests
3. ✅ Clear error messages when assertions fail
4. ✅ Skip conditions work (os, ci, explicit skip)
5. ✅ Exit codes appropriate for CI integration
