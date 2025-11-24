# K-380: Error Infrastructure Extraction - Deep Analysis

**Date**: 2025-11-24
**Issue**: k-380
**Epic**: k-379 (Pandoc Template Port)
**Phase**: 0.1 - Extract quarto-parse-errors crate

## Executive Summary

After deep analysis of the current error infrastructure in `quarto-markdown-pandoc`, I've found that **the system is already remarkably generic** and the extraction will be simpler than initially anticipated. The main work is organizational (moving code) rather than algorithmic (making code generic).

### Key Findings

1. ✅ **TreeSitterLogObserver is already 100% generic** - no changes needed, just move it
2. ✅ **Error generation logic is 95% generic** - minimal changes to function signatures
3. ✅ **Error table types are fully generic** - just need to parameterize the macro
4. ⚠️ **Build script needs moderate changes** - add CLI parameters for paths and extensions
5. ⚠️ **Macro needs one new parameter** - module path prefix for generated code

### Complexity Assessment

- **Original estimate**: Medium complexity
- **Revised estimate**: Low-Medium complexity
- **Risk**: Very Low - mostly code movement with tests to verify

## Current State Analysis

### 1. Error Table Infrastructure

**Location**: `crates/quarto-markdown-pandoc/src/readers/qmd_error_message_table.rs`

**Types** (already generic):
```rust
pub struct ErrorCapture {
    pub column: usize,
    pub lr_state: usize,  // LR parser state
    pub row: usize,
    pub size: usize,      // Token size in characters
    pub sym: &'static str, // Token symbol from parser
    pub label: &'static str, // Label for error message references
}

pub struct ErrorNote {
    pub message: &'static str,
    pub label: Option<&'static str>,
    pub note_type: &'static str,
    // ... trimming options
}

pub struct ErrorInfo {
    pub code: Option<&'static str>,
    pub title: &'static str,
    pub message: &'static str,
    pub captures: &'static [ErrorCapture],
    pub notes: &'static [ErrorNote],
    pub hints: &'static [&'static str],
}

pub struct ErrorTableEntry {
    pub state: usize,      // LR parser state
    pub sym: &'static str, // Lookahead symbol
    pub row: usize,
    pub column: usize,
    pub error_info: ErrorInfo,
    pub name: &'static str,
}
```

**Functions**:
- `get_error_table()` - Returns static error table
- `lookup_error_message()` - Finds message by (state, sym)
- `lookup_error_entry()` - Finds full entry by (state, sym)

**Status**: ✅ **Already generic** - no grammar-specific dependencies

### 2. Tree-Sitter Log Observer

**Location**: `crates/quarto-markdown-pandoc/src/utils/tree_sitter_log_observer.rs`

**Types**:
```rust
pub enum TreeSitterLogState {
    Idle,
    InParse,
    JustReduced,
}

pub struct ProcessMessage {
    pub version: usize,
    pub state: usize,    // LR state
    pub row: usize,
    pub column: usize,
    pub sym: String,     // Symbol
    pub size: usize,
}

pub struct ConsumedToken {
    pub row: usize,
    pub column: usize,
    pub size: usize,
    pub lr_state: usize,
    pub sym: String,
}

pub struct TreeSitterParseLog {
    pub messages: Vec<String>,
    pub current_process: Option<usize>,
    pub current_lookahead: Option<(String, usize)>,
    pub processes: HashMap<usize, TreeSitterProcessLog>,
    pub all_tokens: Vec<ConsumedToken>,
    pub consumed_tokens: Vec<ConsumedToken>,
}

pub trait TreeSitterLogObserverTrait {
    fn had_errors(&self) -> bool;
    fn log(&mut self, log_type: tree_sitter::LogType, message: &str);
}

pub struct TreeSitterLogObserver {
    pub parses: Vec<TreeSitterParseLog>,
    state: TreeSitterLogState,
}
```

**Status**: ✅ **Already 100% generic** - works with any tree-sitter parser

**Dependencies**: Only `tree-sitter` crate (generic logging API)

### 3. Error Generation

**Location**: `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs`

**Main function**:
```rust
pub fn produce_diagnostic_messages(
    input_bytes: &[u8],
    tree_sitter_log: &crate::utils::tree_sitter_log_observer::TreeSitterLogObserver,
    filename: &str,
    source_context: &quarto_source_map::SourceContext,
) -> Vec<quarto_error_reporting::DiagnosticMessage>
```

**Status**: ⚠️ **98% generic** - only needs path adjustments

**Changes needed**:
1. Update module paths in function signature
2. Update call to error table lookup functions

**Algorithm**: Completely generic - works with any parser's error states

### 4. Build Script

**Location**: `crates/quarto-markdown-pandoc/scripts/build_error_table.ts`

**Current behavior**:
1. Reads error corpus from `resources/error-corpus/Q-*.json`
2. Runs `../../target/debug/quarto-markdown-pandoc` with `--_internal-report-error-state`
3. Generates `.qmd` test files in `resources/error-corpus/case-files/`
4. Writes `resources/error-corpus/_autogen-table.json`

**Status**: ⚠️ **Needs parameterization**

**Required changes**:
```typescript
// Add CLI arguments:
interface BuildConfig {
    cmd: string;              // Command to run parser with error reporting
                              // e.g., "../../target/debug/quarto-markdown-pandoc --_internal-report-error-state -i"
    corpus: string;           // Path to error corpus directory
    output: string;           // Path to output JSON file
    extension: string;        // File extension (e.g., ".qmd", ".template")
    errorPattern?: string;    // Glob pattern for error files (default: "*.json")
}
```

**Note**: Using `cmd` instead of `binary` allows for flexible command construction, especially when multiple parsers or additional flags are needed.

**Complexity**: Low - straightforward CLI parameter addition

### 5. Proc Macro

**Location**: `crates/quarto-markdown-pandoc/error-message-macros/src/lib.rs`

**Current invocation**:
```rust
include_error_table!("./resources/error-corpus/_autogen-table.json")
```

**Generated code includes**:
```rust
crate::readers::qmd_error_message_table::ErrorCapture { ... }
```

**Status**: ⚠️ **Needs module path parameter**

**Proposed change**:
```rust
// Add second parameter for module path prefix
include_error_table!(
    "./resources/error-corpus/_autogen-table.json",
    "crate::readers::qmd_error_message_table"
)

// For template parser:
include_error_table!(
    "./resources/error-corpus/_autogen-table.json",
    "crate::error_table"
)
```

**Complexity**: Low - add one parameter, use it in quote! macro

## Proposed Architecture

### Directory Structure

```
crates/quarto-parse-errors/
├── Cargo.toml
├── src/
│   ├── lib.rs                  # Public API
│   ├── error_table.rs          # ErrorTable types (moved from qmd)
│   ├── tree_sitter_log.rs      # TreeSitterLogObserver (moved from qmd)
│   └── error_generation.rs     # produce_diagnostic_messages (moved from qmd)
├── error-message-macros/       # Nested crate
│   ├── Cargo.toml
│   └── src/
│       └── lib.rs              # include_error_table! macro
└── scripts/
    └── build_error_table.ts    # Generic build script

crates/quarto-markdown-pandoc/
├── resources/
│   └── error-corpus/           # QMD-specific error corpus (stays here)
│       ├── Q-*.json
│       ├── case-files/
│       └── _autogen-table.json
└── src/
    └── readers/
        └── qmd_error_message_table.rs  # Now just re-exports + get_error_table()
```

### Public API

```rust
// crates/quarto-parse-errors/src/lib.rs
pub mod error_table;
pub mod tree_sitter_log;
pub mod error_generation;

// Re-export commonly used types
pub use error_table::{
    ErrorTable, ErrorTableEntry, ErrorInfo,
    ErrorCapture, ErrorNote
};

pub use tree_sitter_log::{
    TreeSitterLogObserver,
    TreeSitterLogObserverTrait,
    TreeSitterLogObserverFast,
    TreeSitterParseLog,
    TreeSitterProcessLog,
    ConsumedToken,
    ProcessMessage,
    TreeSitterLogState,
};

pub use error_generation::{
    produce_diagnostic_messages,
    collect_error_node_ranges,
    get_outer_error_nodes,
    prune_diagnostics_by_error_nodes,
};
```

### Dependencies

```toml
[dependencies]
quarto-error-reporting = { path = "../quarto-error-reporting" }
quarto-source-map = { path = "../quarto-source-map" }
tree-sitter = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = "1.0"

[build-dependencies]
# None - build script is external Deno script
```

## Overlooked Aspects & Recommendations

### 1. Missing from Original Plan: Documentation

**Issue**: Plan doesn't specify documentation requirements.

**Recommendation**: Add comprehensive docs:

```rust
//! # quarto-parse-errors
//!
//! Generic error reporting infrastructure for tree-sitter based parsers.
//!
//! ## Overview
//!
//! This crate provides a complete system for generating high-quality error messages
//! from tree-sitter parse failures using the "generating syntax errors from examples"
//! approach (Jeffery, TOPLAS 2003).
//!
//! ## Components
//!
//! 1. **Error Corpus**: JSON files mapping parser states to error messages
//! 2. **TreeSitterLogObserver**: Captures parser state during failed parses
//! 3. **Error Table**: Compile-time embedded error message database
//! 4. **Error Generation**: Converts parser states to user-friendly diagnostics
//!
//! ## Usage
//!
//! ### Setting up error corpus
//! [... detailed instructions ...]
//!
//! ### Integrating with your parser
//! [... code examples ...]
```

### 2. Missing from Original Plan: Build Script Integration

**Issue**: How do parsers run the build script?

**Recommendation**: Provide a `build.rs` helper:

```rust
// crates/quarto-parse-errors/src/build_helpers.rs
pub fn generate_error_table(
    binary: &str,
    corpus_dir: &str,
    output_file: &str,
    extension: &str,
) -> std::io::Result<()> {
    // Run the Deno script with proper arguments
    // This can be called from a parser's build.rs
}
```

Actually, on second thought, keep it simple - just document the command line invocation. Build scripts are often manual steps anyway.

### 3. Missing from Original Plan: Binary Interface Contract

**Issue**: Parsers need to implement `--_internal-report-error-state` flag.

**Recommendation**: Document the interface:

```markdown
## Binary Interface Contract

Your parser binary must support:

```
--_internal-report-error-state -i <file>
```

Output format (JSON to stdout):
```json
{
  "tokens": [
    {
      "row": 0,
      "column": 3,
      "size": 1,
      "lrState": 42,
      "sym": "["
    }
  ],
  "errorStates": [
    {
      "state": 42,
      "sym": "EOF",
      "row": 0,
      "column": 18
    }
  ]
}
```
```

### 4. Missing from Original Plan: Error Corpus File Format Spec

**Issue**: No formal spec for Q-*.json format.

**Recommendation**: Document with schema:

```markdown
## Error Corpus File Format

Each error code has one JSON file with this structure:

```json
{
  "code": "Q-2-1",              // Error code (optional)
  "title": "Unclosed Span",     // Short title
  "message": "...",              // Main error message
  "notes": [                     // Additional context (optional)
    {
      "message": "...",
      "label": "span-start",    // References capture below
      "noteType": "simple"
    }
  ],
  "hints": ["..."],              // Suggestions (optional)
  "cases": [                     // Test cases
    {
      "name": "simple",
      "description": "...",
      "content": "...",          // Test input
      "captures": [              // Token positions to highlight
        {
          "label": "span-start",
          "row": 0,
          "column": 3,
          "size": 1
        }
      ],
      "prefixes": ["..."],       // Optional: test in different contexts
      "suffixes": ["..."]
    }
  ]
}
```
```

### 5. Missing from Original Plan: Testing Strategy Details

**Issue**: Plan says "comprehensive tests" but doesn't specify.

**Recommendation**: Specific test categories:

```rust
#[cfg(test)]
mod tests {
    // 1. TreeSitterLogObserver tests
    #[test]
    fn test_observer_detects_errors() { }

    #[test]
    fn test_observer_tracks_token_consumption() { }

    #[test]
    fn test_observer_handles_error_recovery() { }

    // 2. Error table tests
    #[test]
    fn test_error_table_loading() { }

    #[test]
    fn test_error_lookup_by_state_sym() { }

    #[test]
    fn test_error_table_completeness() { }

    // 3. Error generation tests
    #[test]
    fn test_produce_diagnostic_from_parse_state() { }

    #[test]
    fn test_capture_matching() { }

    #[test]
    fn test_byte_offset_calculation() { }

    #[test]
    fn test_utf8_handling() { }

    // 4. Integration tests
    #[test]
    fn test_end_to_end_error_generation() { }
}
```

### 6. Missing from Original Plan: Backward Compatibility Verification

**Issue**: No explicit test that qmd continues working.

**Recommendation**: Add to Phase 0.2 deliverables:

```markdown
### Phase 0.2 Success Criteria

- ✅ All qmd tests pass without changes
- ✅ Error messages are byte-for-byte identical
- ✅ No performance regression
- ✅ Build process unchanged for qmd users
```

### 7. Potential Issue: Error Table Entry Duplication

**Current**: Error table is a flat array with potential duplicates for same (state, sym)

**Observation**: `lookup_error_entry()` returns `Vec<&'static ErrorTableEntry>` suggesting multiple entries are possible.

**Question**: Is this intentional for different contexts? Or should we use a better data structure?

**Recommendation**: Keep as-is for Phase 0.1 (don't change algorithms), but document the behavior. Consider optimization in future phase.

### 8. Simplification Opportunity: Macro Design

**Current plan**: Make macro generic with module path parameter.

**Alternative**: Use a trait-based approach where each parser implements a marker trait?

**Analysis**: No, the current approach is simpler. Traits add complexity without benefit here.

**Keep**: Module path parameter approach is cleanest.

### 9. Build Script: cmd vs binary Parameter

**User feedback**: The `binary` field should be `cmd` to include both the binary path and command-line options.

**Rationale**:
- Current: `../../target/debug/quarto-markdown-pandoc --_internal-report-error-state -i`
- Future: May need to expose multiple tree-sitter parsers or add other flags
- Flexibility: Different parsers may need different invocation patterns

**Accepted**: ✅ Use `cmd` parameter instead of `binary`

### 10. Simplification: Combined Phases 0.1 + 0.2?

**Question**: Should we extract and migrate in one phase?

**Analysis**:
- Pro: Faster, less intermediate state
- Con: Harder to debug if something breaks, larger changeset

**Recommendation**: **Keep separate phases** - safer approach, easier code review.

### 10. Build Script Location

**Question**: Where should build script live?

**Options**:
- A: `crates/quarto-parse-errors/scripts/build_error_table.ts`
- B: `scripts/build_error_table.ts` (workspace root)
- C: Inside the crate but at root: `crates/quarto-parse-errors/build_error_table.ts`

**Recommendation**: **Option A** - keeps script with the crate it supports. Parsers invoke like:

```bash
./crates/quarto-parse-errors/scripts/build_error_table.ts \
  --binary target/debug/my-parser \
  --corpus resources/errors \
  --output resources/errors/_autogen.json \
  --extension .ext
```

## Detailed Implementation Plan for Phase 0.1

### Step 1: Create New Crate Structure

```bash
mkdir -p crates/quarto-parse-errors/src
mkdir -p crates/quarto-parse-errors/error-message-macros/src
mkdir -p crates/quarto-parse-errors/scripts
```

### Step 2: Move Error Table Types

**From**: `crates/quarto-markdown-pandoc/src/readers/qmd_error_message_table.rs`
**To**: `crates/quarto-parse-errors/src/error_table.rs`

**Changes**:
- Remove `use crate::utils::...` - not needed yet
- Update proc macro path reference (done in Step 5)
- Add comprehensive documentation

### Step 3: Move Tree-Sitter Log Observer

**From**: `crates/quarto-markdown-pandoc/src/utils/tree_sitter_log_observer.rs`
**To**: `crates/quarto-parse-errors/src/tree_sitter_log.rs`

**Changes**: **NONE** - it's already 100% generic!

### Step 4: Move Error Generation

**From**: `crates/quarto-markdown-pandoc/src/readers/qmd_error_messages.rs`
**To**: `crates/quarto-parse-errors/src/error_generation.rs`

**Changes**:
1. Update imports:
   ```rust
   // Old
   use crate::utils::tree_sitter_log_observer::{TreeSitterLogObserver, ConsumedToken};
   use crate::readers::qmd_error_message_table::{ErrorCapture, lookup_error_entry};

   // New
   use crate::tree_sitter_log::{TreeSitterLogObserver, ConsumedToken};
   use crate::error_table::{ErrorCapture, lookup_error_entry};
   ```

2. Make functions public
3. Add documentation

### Step 5: Move and Update Proc Macro

**From**: `crates/quarto-markdown-pandoc/error-message-macros/`
**To**: `crates/quarto-parse-errors/error-message-macros/`

**Changes**:
```rust
#[proc_macro]
pub fn include_error_table(input: TokenStream) -> TokenStream {
    // Parse two arguments now: path and module prefix
    let input = parse_macro_input!(input as IncludeErrorTableInput);
    let path_str = input.path.value();
    let module_prefix = input.module_prefix.value();

    // ... read and parse JSON ...

    // Use module_prefix in generated code:
    quote! {
        #module_prefix::ErrorCapture {
            column: #cap_column,
            // ...
        }
    }
}

// Add input parser:
struct IncludeErrorTableInput {
    path: LitStr,
    _comma: Token![,],
    module_prefix: LitStr,
}

impl Parse for IncludeErrorTableInput {
    fn parse(input: ParseStream) -> Result<Self> {
        Ok(IncludeErrorTableInput {
            path: input.parse()?,
            _comma: input.parse()?,
            module_prefix: input.parse()?,
        })
    }
}
```

### Step 6: Make Build Script Generic

**File**: `crates/quarto-parse-errors/scripts/build_error_table.ts`

**Add CLI parsing**:
```typescript
import { parse } from "jsr:@std/flags";

const args = parse(Deno.args, {
  string: ["cmd", "corpus", "output", "extension", "pattern"],
  default: {
    extension: ".qmd",
    pattern: "*.json",
  },
});

if (!args.cmd || !args.corpus || !args.output) {
  console.error("Usage: build_error_table.ts --cmd <command> --corpus <dir> --output <file> [--extension <ext>] [--pattern <glob>]");
  console.error("Example: build_error_table.ts --cmd '../../target/debug/quarto-markdown-pandoc --_internal-report-error-state -i' --corpus resources/error-corpus --output resources/error-corpus/_autogen-table.json");
  Deno.exit(1);
}

const config = {
  cmd: args.cmd,
  corpus: args.corpus,
  output: args.output,
  extension: args.extension,
  pattern: args.pattern,
};
```

**Update file operations**:
```typescript
// Old
const jsonFiles = Array.from(fs.globSync("resources/error-corpus/*.json"))

// New
const jsonFiles = Array.from(fs.globSync(`${config.corpus}/${config.pattern}`))
```

### Step 7: Write Tests

```rust
// crates/quarto-parse-errors/src/lib.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tree_sitter_log_observer_creation() {
        let observer = tree_sitter_log::TreeSitterLogObserver::default();
        assert!(!observer.had_errors());
    }

    // ... more tests ...
}
```

### Step 8: Create Documentation

Write `crates/quarto-parse-errors/README.md` with:
- Overview
- Architecture diagram
- Usage examples
- Error corpus format
- Build script usage
- Integration guide

### Step 9: Update Cargo.toml Files

```toml
# crates/quarto-parse-errors/Cargo.toml
[package]
name = "quarto-parse-errors"
version = "0.1.0"
edition = "2021"

[dependencies]
quarto-error-reporting = { path = "../quarto-error-reporting" }
quarto-source-map = { path = "../quarto-source-map" }
tree-sitter = { workspace = true }
serde = { workspace = true, features = ["derive"] }
serde_json = "1.0"
error-message-macros = { path = "./error-message-macros" }

# crates/quarto-parse-errors/error-message-macros/Cargo.toml
[package]
name = "error-message-macros"
version = "0.1.0"
edition = "2021"

[lib]
proc-macro = true

[dependencies]
proc-macro2 = "1.0"
quote = "1.0"
syn = { version = "2.0", features = ["full", "extra-traits"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
```

## Revised Phase 0.1 Checklist

```markdown
### Deliverables

- [ ] New `quarto-parse-errors` crate created
  - [ ] src/lib.rs with public API
  - [ ] src/error_table.rs (moved from qmd)
  - [ ] src/tree_sitter_log.rs (moved from qmd)
  - [ ] src/error_generation.rs (moved from qmd)
  - [ ] Cargo.toml with dependencies

- [ ] Proc macro crate updated
  - [ ] error-message-macros/src/lib.rs (moved and updated)
  - [ ] Add module_prefix parameter support
  - [ ] error-message-macros/Cargo.toml

- [ ] Build script generalized
  - [ ] scripts/build_error_table.ts (moved and parameterized)
  - [ ] Add CLI argument parsing
  - [ ] Update all hardcoded paths to use config
  - [ ] Update file extension handling

- [ ] Documentation
  - [ ] README.md with usage guide
  - [ ] Rustdoc for all public APIs
  - [ ] Error corpus format specification
  - [ ] Build script usage documentation
  - [ ] Integration guide for new parsers

- [ ] Tests
  - [ ] TreeSitterLogObserver unit tests
  - [ ] Error table loading tests
  - [ ] Error generation tests
  - [ ] UTF-8 handling tests
  - [ ] Integration test with mock parser states

- [ ] Verify
  - [ ] `cargo check` passes
  - [ ] `cargo test` passes
  - [ ] `cargo doc` generates complete docs
  - [ ] No qmd code is broken (nothing uses new crate yet)
```

## Simplifications Identified

1. **TreeSitterLogObserver is already perfect** - no generics needed, just move it
2. **Error table types need zero algorithmic changes** - just move them
3. **Macro only needs one new parameter** - simpler than trait-based approaches
4. **No trait-based generics needed** - concrete types work fine
5. **Build script stays as external script** - no need for Rust build.rs integration
6. **Can skip complex build system integration** - just document the CLI

## Risk Assessment

| Component | Risk Level | Mitigation |
|-----------|-----------|------------|
| TreeSitterLogObserver extraction | **Very Low** | Already generic, just move |
| Error table extraction | **Very Low** | Already generic, just move |
| Error generation extraction | **Low** | Simple path updates |
| Proc macro update | **Low** | Add one parameter, well-tested |
| Build script changes | **Medium** | CLI parsing is straightforward, test thoroughly |
| Integration issues | **Low** | Phase 0.2 will catch issues with tests |

**Overall Risk**: **Low**

## Timeline Estimate

- **Phase 0.1**: 1-2 days
  - Day 1 AM: Create structure, move code
  - Day 1 PM: Update macro, generalize build script
  - Day 2 AM: Write tests
  - Day 2 PM: Documentation, verification

- **Phase 0.2**: 0.5-1 day
  - Update qmd to use new crate
  - Run all tests
  - Fix any issues

**Total**: 1.5-3 days for both phases

## Recommendations

### Do These Simplifications

1. ✅ **Keep phases separate (0.1 and 0.2)** - safer, easier review
2. ✅ **Keep build script as Deno script** - no need for Rust build integration
3. ✅ **Use simple module path parameter in macro** - avoid trait complexity
4. ✅ **Move macro as nested crate** - keeps it coupled with types it generates
5. ✅ **Don't add runtime configuration** - compile-time is sufficient

### Add These Missing Pieces

1. ✅ **Comprehensive documentation** - README, rustdoc, integration guide
2. ✅ **Error corpus format spec** - formal JSON schema documentation
3. ✅ **Binary interface contract** - document expected CLI interface
4. ✅ **Explicit backward compatibility tests** - verify qmd unchanged
5. ✅ **Build script usage documentation** - clear CLI examples

### Don't Do These

1. ❌ **Don't add trait-based generics** - unnecessary complexity
2. ❌ **Don't create Rust build.rs helper** - external script is fine
3. ❌ **Don't optimize error table data structure** - keep it simple for now
4. ❌ **Don't combine phases 0.1 and 0.2** - keep them separate for safety
5. ❌ **Don't create separate top-level macro crate** - nest it in quarto-parse-errors

## Conclusion

The extraction is **simpler than originally anticipated** because the code is already remarkably generic. The main work is:

1. **Organizational**: Moving files to new crate
2. **Configuration**: Adding CLI parameters to build script
3. **Parameterization**: Adding one parameter to proc macro
4. **Documentation**: Writing comprehensive guides

The original plan is solid and well-thought-out. The main additions needed are:
- Better documentation
- More specific testing strategy
- Binary interface specification
- Error corpus format specification

With these additions, Phase 0.1 should proceed smoothly with low risk.
