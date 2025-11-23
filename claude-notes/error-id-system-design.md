# Error ID System Design for Quarto

<!-- quarto-error-code-audit-ignore-file -->

## Executive Summary

This document proposes a TypeScript-inspired error ID system for Quarto, providing stable, searchable error codes that improve user experience and documentation.

## Motivation

### Benefits of Error IDs

1. **Searchability**: Users can Google "Q-2301" instead of trying to search for error message text
2. **Stability**: Error IDs remain stable across versions even if message wording improves
3. **Documentation**: Each error code maps to detailed explanation with examples and solutions
4. **Filtering/Suppression**: Users can suppress specific errors by code (future feature)
5. **Analytics**: Track which errors are most common to prioritize UX improvements
6. **Internationalization**: Message text can be translated while code remains universal

### TypeScript's Approach

TypeScript uses a proven system:
- **Format**: `TS####` (e.g., `TS2322`, `TS1005`)
- **Registry**: Central `diagnosticMessages.json` file (~2000 entries)
- **Organization**: Code ranges by subsystem (1000s for syntax, 2000s for type checking, etc.)
- **Documentation**: Each code can link to detailed explanations
- **Generation**: Compiled into generated TypeScript code from JSON source

## Proposed Design for Quarto

### Error Code Format

**Format**: `Q-<subsystem>-<number>` where both are unpadded integers

**Examples**:
- `Q-1-1`: First YAML/config error
- `Q-1-2`: Second YAML/config error
- `Q-2-301`: Unclosed code block (can jump numbers)
- `Q-3-405`: Jupyter execution failed
- `Q-4-102`: Invalid format configuration

**Rationale**:
- **Prefix `Q-`**: Short, memorable, unambiguous (won't collide with TypeScript, Rust, etc.)
- **Subsystem number**: Clearly identifies which part of Quarto raised the error
- **No padding**: Simpler, no need to worry about running out of digits (Q-1-9999 vs Q-1-10000)
- **Flexible numbering**: Can leave gaps, use meaningful numbers (301 for code block errors)
- **Two dashes**: Clear separation of the three components

### Subsystem Number Organization

Organize codes by subsystem using the first number after `Q-`:

| Subsystem | Number | Examples |
|-----------|--------|----------|
| YAML and Configuration | 1 | Q-1-1: YAML syntax error<br>Q-1-2: Invalid schema<br>Q-1-50: Config merge conflict |
| Markdown and Parsing | 2 | Q-2-1: Markdown syntax error<br>Q-2-301: Unclosed code block<br>Q-2-450: Invalid div syntax |
| Engines and Execution | 3 | Q-3-1: Engine not found<br>Q-3-405: Jupyter execution failed<br>Q-3-701: Knitr error |
| Rendering and Formats | 4 | Q-4-1: Unknown format<br>Q-4-102: Invalid PDF config<br>Q-4-550: HTML template error |
| Projects and Structure | 5 | Q-5-1: Invalid project structure<br>Q-5-201: Missing _quarto.yml<br>Q-5-403: Circular reference |
| Extensions and Plugins | 6 | Q-6-1: Extension not found<br>Q-6-234: Filter error<br>Q-6-501: Shortcode error |
| CLI and Tools | 7 | Q-7-1: Invalid command<br>Q-7-301: LSP error<br>Q-7-502: Preview server error |
| Publishing and Deployment | 8 | Q-8-1: Publish target not found<br>Q-8-234: Authentication failed<br>Q-8-501: Deployment error |
| Internal/System Errors | 0 | Q-0-1: Internal error (unreachable code)<br>Q-0-2: Assertion failed |
| Reserved for Future | 9+ | Available for new subsystems |

**Numbering within subsystems**:
- Start at 1 (not 0, except for subsystem 0)
- Leave gaps for related errors (e.g., 300-399 for code blocks, 400-499 for divs)
- No padding needed - Q-2-5 and Q-2-500 are both valid
- Document grouping strategy in catalog comments

### Error Catalog Structure

**File**: `crates/quarto-error-reporting/error_catalog.json`

Similar to TypeScript's `diagnosticMessages.json`, using JSON:

**JSON Format** (`error_catalog.json`):

```json
{
  "Q-0-1": {
    "subsystem": "internal",
    "title": "Internal Error",
    "message_template": "An internal error occurred. This is a bug in Quarto.",
    "docs_url": "https://quarto.org/docs/errors/Q-0-1",
    "since_version": "99.9.9"
  },
  "Q-1-1": {
    "subsystem": "yaml",
    "title": "YAML Syntax Error",
    "message_template": "Invalid YAML syntax",
    "docs_url": "https://quarto.org/docs/errors/Q-1-1",
    "since_version": "99.9.9"
  },
  "Q-1-2": {
    "subsystem": "yaml",
    "title": "Invalid Schema",
    "message_template": "YAML value does not match expected schema",
    "docs_url": "https://quarto.org/docs/errors/Q-1-2",
    "since_version": "99.9.9"
  }
}
```

**Rust Loader** (`catalog.rs`):

```rust
use std::collections::HashMap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

/// Metadata for an error code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorCodeInfo {
    /// Subsystem name (e.g., "yaml", "markdown", "engine")
    pub subsystem: String,

    /// Short title for the error
    pub title: String,

    /// Default message template (may include placeholders)
    pub message_template: String,

    /// URL to documentation (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docs_url: Option<String>,

    /// When this error was introduced (version)
    pub since_version: String,
}

/// Global error catalog, loaded lazily from JSON at compile time.
pub static ERROR_CATALOG: Lazy<HashMap<String, ErrorCodeInfo>> = Lazy::new(|| {
    let json_data = include_str!("../error_catalog.json");
    serde_json::from_str(json_data).expect("Invalid error catalog JSON")
});

/// Look up error code information.
pub fn get_error_info(code: &str) -> Option<&ErrorCodeInfo> {
    ERROR_CATALOG.get(code)
}

/// Get documentation URL for an error code.
pub fn get_docs_url(code: &str) -> Option<&str> {
    ERROR_CATALOG.get(code).and_then(|info| info.docs_url.as_deref())
}
```

**Benefits of JSON approach**:
- Can use the same YAML/JSON parsing infrastructure we're building
- Can validate the catalog JSON with a schema (using quarto-yaml-validation!)
- External tooling can process (error doc generators, linters, etc.)
- Community contributions via PRs (just edit JSON, no Rust knowledge needed)
- Closer to TypeScript's approach

### Integration with DiagnosticMessage

Update `DiagnosticMessage` to include optional error code:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticMessage {
    /// Brief title for the error
    pub title: String,

    /// The kind of diagnostic (Error, Warning, Info)
    pub kind: DiagnosticKind,

    /// Optional error code (e.g., "Q-1001")
    pub code: Option<String>,

    /// The problem statement (the "what" - using "must" or "can't")
    pub problem: Option<MessageContent>,

    /// Specific error details (the "where/why" - max 5 per tidyverse)
    pub details: Vec<DetailItem>,

    /// Optional hints for fixing (ends with ?)
    pub hints: Vec<MessageContent>,
}
```

**Usage**:

```rust
let error = DiagnosticMessage::error("YAML Syntax Error")
    .with_code("Q-1001")
    .problem("Invalid YAML syntax in configuration file")
    .add_detail("Unexpected character at line 42")
    .build();
```

Or using catalog:

```rust
let error = DiagnosticMessage::from_code("Q-1001")
    .add_detail("Unexpected character at line 42")
    .build();
```

### Rendering Error Codes

**Terminal output** (via ariadne):
```
Error Q-1001: YAML Syntax Error
  ┌─ _quarto.yml:42:5
  │
42│   format: { html
  │           ^ Unexpected character

  = Invalid YAML syntax in configuration file
  = See https://quarto.org/docs/errors/Q-1001 for more information
```

**JSON output**:
```json
{
  "kind": "error",
  "code": "Q-1001",
  "title": "YAML Syntax Error",
  "message": "Invalid YAML syntax in configuration file",
  "details": [
    {
      "kind": "error",
      "content": "Unexpected character at line 42"
    }
  ],
  "source": {
    "file": "_quarto.yml",
    "line": 42,
    "column": 5
  },
  "docs_url": "https://quarto.org/docs/errors/Q-1001"
}
```

### Documentation Strategy

1. **Error Reference Website**: `https://quarto.org/docs/errors/`
   - Index page listing all error codes by category
   - Individual pages for each error: `/docs/errors/Q-1001`

2. **Page Structure** (following TypeScript TV pattern):
   ```markdown
   # Q-1001: YAML Syntax Error

   ## Description
   This error occurs when Quarto encounters invalid YAML syntax in a configuration file.

   ## Common Causes
   - Missing closing bracket/brace
   - Invalid indentation
   - Unquoted special characters

   ## Examples

   ### ❌ Incorrect
   ```yaml
   format: { html
   ```

   ### ✅ Correct
   ```yaml
   format:
     html: default
   ```

   ## Related Errors
   - Q-1002: Invalid Schema
   - Q-1050: Config Merge Conflict

   ## See Also
   - [YAML Configuration Guide](...)
   ```

3. **Auto-generation**: Generate error documentation from the catalog
   - Script that reads catalog and generates markdown stubs
   - Humans fill in detailed explanations and examples

### Error Code Allocation Process

**For developers adding new errors**:

1. **Choose appropriate range** based on subsystem (see table above)
2. **Find next available code** in that range (leave gaps!)
3. **Add to catalog** with metadata
4. **Document in code** where the error is raised
5. **Create error docs page** (can be stub initially)
6. **Update CHANGELOG** mentioning new error codes

**Example**:
```rust
// In quarto-yaml-validation/src/validate.rs

use quarto_error_reporting::{DiagnosticMessage, ERROR_CATALOG};

pub fn validate_boolean(value: &Yaml) -> Result<(), DiagnosticMessage> {
    if !value.is_bool() {
        return Err(DiagnosticMessage::from_code("Q-1002")
            .problem(format!("Expected boolean, got {}", value.type_name()))
            .add_detail(format!("Value: {}", value))
            .build());
    }
    Ok(())
}
```

### Migration Strategy

**Phase 1: Infrastructure** (Current)
- Add `code: Option<String>` to `DiagnosticMessage`
- Create `catalog.rs` with initial errors
- Update rendering to display codes

**Phase 2: Gradual Adoption**
- Add error codes to new errors (required)
- Add codes to existing high-impact errors (optional)
- Prioritize errors users encounter most frequently

**Phase 3: Documentation**
- Generate error reference site structure
- Write detailed explanations for top 20 errors
- Fill in remaining errors incrementally

**Phase 4: Tooling**
- Error code linting (ensure all errors have codes)
- Code allocation helper (suggest next available code)
- Documentation completeness checking

**Not required**: Retrofitting all existing errors immediately. This is optional and can happen gradually.

## Implementation Plan

### Step 1: Update Core Types (bd-1 / Phase 1)

- Add `code: Option<String>` field to `DiagnosticMessage`
- Add `with_code()` builder method
- Update serialization to include code

### Step 2: Create Error Catalog (New Issue)

- Create `catalog.rs` with `ErrorCodeInfo` struct
- Add initial set of ~20-30 common error codes
- Implement lookup functions

### Step 3: Update Rendering (bd-2 / Phase 2)

- Display error codes in ariadne output
- Include codes in JSON serialization
- Add docs URL to output if available

### Step 4: Builder Enhancements (bd-4 / Phase 4)

- Add `DiagnosticMessage::from_code()` constructor
- Load default message from catalog
- Allow overriding default message

### Step 5: Documentation (Future)

- Generate error reference site structure
- Write explanations for initial error set
- Set up auto-generation tooling

## Open Questions

### 1. Code Format: `Q-<subsystem>-<number>` ✅ DECIDED

**Decision**: Use `Q-<subsystem>-<number>` with unpadded integers

**Rationale**:
- Subsystem number clearly identifies error origin
- No padding simplifies allocation (no worry about running out)
- Two dashes provide clear visual separation

### 2. Catalog: Rust vs JSON? ✅ DECIDED

**Decision**: Use JSON catalog loaded at compile-time

**Rationale**:
- Can validate with the same infrastructure we're building (quarto-yaml-validation)
- External tooling can process (doc generators, linters)
- Community-friendly (edit JSON, no Rust knowledge needed)
- Similar to TypeScript's approach
- `include_str!()` loads at compile-time, so no runtime overhead

### 3. Required vs Optional? ✅ DECIDED

**Decision**: Optional but encouraged, with static analysis

**Rationale**:
- Gradual adoption doesn't disrupt existing code
- Can statically analyze codebase to find unnumbered errors
- CI can warn on new errors without codes
- Eventually can require codes via linting rules

### 4. Initial Error Set? ✅ DECIDED

**Decision**: Start with mostly-empty catalog containing only internal error

**Rationale**:
- Populate file with Q-0-1 (internal error) as template
- Add validation subsystem errors as we implement validators
- Don't block on writing comprehensive error messages upfront
- Can add errors incrementally as needed

## Comparison with TypeScript

| Aspect | TypeScript | Quarto (Proposed) |
|--------|-----------|-------------------|
| Format | `TS####` | `Q-####` |
| Count | ~2000 codes | Start with ~30, grow organically |
| Storage | JSON (`diagnosticMessages.json`) | Rust (initially) |
| Organization | By code range | By code range |
| Documentation | TypeScript website | Quarto website |
| Required? | Yes (all errors have codes) | Optional but encouraged |
| Generation | Compile-time from JSON | Direct Rust code |

## References

- **TypeScript diagnosticMessages.json**: https://github.com/microsoft/TypeScript/blob/main/src/compiler/diagnosticMessages.json
- **TypeScript TV (Error Reference)**: https://typescript.tv/errors/
- **TypeScript Understanding Errors**: https://www.typescriptlang.org/docs/handbook/2/understanding-errors.html
- **Tidyverse Style Guide**: https://style.tidyverse.org/errors.html (error message content, not codes)
- **Rust Error Codes**: https://doc.rust-lang.org/error_codes/error-index.html (similar concept: `E0308`, `E0425`)

## Next Steps

1. Review and approve this design
2. Create Beads issue for error catalog implementation
3. Update Phase 1 (bd-1) to include error code field
4. Implement catalog with initial error codes
5. Document error code allocation process for contributors
