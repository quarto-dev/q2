# JSON Filter Support Design Plan

**Issue:** k-408
**Epic:** k-407 (Extensible filters for quarto-markdown-pandoc)
**Created:** 2025-11-26

## Overview

Implement JSON filter support following Pandoc's well-established protocol. JSON filters are external processes that receive the Pandoc AST as JSON on stdin and output the modified AST as JSON on stdout. This provides language-agnostic extensibility.

## Background Research

### Pandoc's JSON Filter Protocol

From `/external-sources/pandoc/src/Text/Pandoc/Filter/JSON.hs`:

```
stdin:  Pandoc AST as JSON
stdout: Modified Pandoc AST as JSON
args:   [target_format, ...]
env:    PANDOC_VERSION, PANDOC_READER_OPTIONS
```

### Existing Infrastructure

We already have:
- **JSON writer** (`src/writers/json.rs`): Serializes `Pandoc` to JSON
- **JSON reader** (`src/readers/json.rs`): Deserializes JSON to `Pandoc`
- **Pandoc AST types** (`src/pandoc/`): Full Rust AST representation

The hard work of JSON serialization is done; we just need the filter execution layer.

## Design

### 1. CLI Interface

Add two new CLI arguments:

```rust
// In main.rs Args struct
#[arg(long = "filter", short = 'F', action = clap::ArgAction::Append)]
filters: Vec<PathBuf>,

#[arg(long = "lua-filter", short = 'L', action = clap::ArgAction::Append)]
lua_filters: Vec<PathBuf>,
```

Filters execute in the order specified on the command line. Multiple `--filter` and `--lua-filter` can be interleaved:

```bash
quarto-markdown-pandoc -i doc.qmd --filter a.py --lua-filter b.lua --filter c.py
```

### 2. Filter Abstraction

Create a new module `src/external_filters/` with:

```rust
// src/external_filters/mod.rs
pub mod json_filter;
pub mod filter_path;

/// Types of external filters (distinct from internal Filter in filters.rs)
pub enum ExternalFilter {
    Json(PathBuf),
    Lua(PathBuf),  // For future k-409
}

/// Apply a sequence of external filters to a document
pub fn apply_filters(
    doc: Pandoc,
    filters: &[ExternalFilter],
    target_format: &str,
    context: &FilterContext,
) -> Result<Pandoc, FilterError>;

/// Context for filter execution
pub struct FilterContext {
    pub pandoc_version: String,      // Our version string
    pub reader_options: ReaderOptions,
}
```

### 3. JSON Filter Implementation

```rust
// src/external_filters/json_filter.rs

use std::process::{Command, Stdio};
use std::io::Write;

pub fn apply_json_filter(
    doc: Pandoc,
    filter_path: &Path,
    target_format: &str,
    context: &FilterContext,
) -> Result<Pandoc, FilterError> {
    // 1. Resolve filter path and interpreter
    let (program, args) = resolve_filter_invocation(filter_path)?;

    // 2. Serialize document to JSON
    let json_input = serialize_to_pandoc_json(&doc)?;

    // 3. Spawn subprocess
    let mut child = Command::new(&program)
        .args(&args)
        .arg(target_format)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())  // Pass through filter stderr
        .env("PANDOC_VERSION", &context.pandoc_version)
        .env("PANDOC_READER_OPTIONS", serialize_reader_options(&context.reader_options)?)
        .spawn()
        .map_err(|e| FilterError::SpawnFailed(filter_path.to_owned(), e))?;

    // 4. Write JSON to stdin
    child.stdin.take().unwrap().write_all(json_input.as_bytes())?;

    // 5. Wait for completion and read stdout
    let output = child.wait_with_output()?;

    if !output.status.success() {
        return Err(FilterError::NonZeroExit(
            filter_path.to_owned(),
            output.status.code().unwrap_or(-1),
        ));
    }

    // 6. Parse output JSON
    let json_output = String::from_utf8(output.stdout)
        .map_err(|_| FilterError::InvalidUtf8Output(filter_path.to_owned()))?;

    deserialize_from_pandoc_json(&json_output)
        .map_err(|e| FilterError::JsonParseError(filter_path.to_owned(), e))
}
```

### 4. Interpreter Detection

Follow Pandoc's conventions:

```rust
// src/external_filters/filter_path.rs

fn resolve_filter_invocation(path: &Path) -> Result<(OsString, Vec<OsString>), FilterError> {
    // Check if executable
    if is_executable(path) {
        return Ok((path.as_os_str().to_owned(), vec![]));
    }

    // Otherwise, determine interpreter from extension
    let interpreter = match path.extension().and_then(|e| e.to_str()) {
        Some("py") => "python3",  // or "python" on Windows
        Some("hs") => "runhaskell",
        Some("pl") => "perl",
        Some("rb") => "ruby",
        Some("php") => "php",
        Some("js") => "node",
        Some("r") | Some("R") => "Rscript",
        Some(ext) => return Err(FilterError::UnknownExtension(ext.to_string())),
        None => return Err(FilterError::NoExtension(path.to_owned())),
    };

    Ok((OsString::from(interpreter), vec![path.as_os_str().to_owned()]))
}
```

### 5. Filter Discovery

Search order (matching Pandoc):

1. Absolute or relative path as given
2. `$DATADIR/filters/` (user data directory)
3. `$PATH` (for executable filters only)

```rust
fn find_filter(name: &str) -> Result<PathBuf, FilterError> {
    let path = Path::new(name);

    // 1. Direct path
    if path.exists() {
        return Ok(path.to_owned());
    }

    // 2. Data directory
    if let Some(data_dir) = get_data_dir() {
        let data_path = data_dir.join("filters").join(name);
        if data_path.exists() {
            return Ok(data_path);
        }
    }

    // 3. PATH (executable only)
    if let Ok(which_path) = which::which(name) {
        return Ok(which_path);
    }

    Err(FilterError::NotFound(name.to_string()))
}
```

### 6. Error Types

```rust
// src/external_filters/mod.rs

#[derive(Debug, thiserror::Error)]
pub enum FilterError {
    #[error("Filter not found: {0}")]
    NotFound(String),

    #[error("Failed to spawn filter {0}: {1}")]
    SpawnFailed(PathBuf, std::io::Error),

    #[error("Filter {0} exited with status {1}")]
    NonZeroExit(PathBuf, i32),

    #[error("Filter {0} produced invalid UTF-8 output")]
    InvalidUtf8Output(PathBuf),

    #[error("Failed to parse JSON output from filter {0}: {1}")]
    JsonParseError(PathBuf, String),

    #[error("Unknown filter extension: {0}")]
    UnknownExtension(String),

    #[error("Filter has no extension: {0}")]
    NoExtension(PathBuf),

    #[error("JSON serialization error: {0}")]
    SerializationError(String),
}
```

### 7. Integration with Main Pipeline

```rust
// In main.rs, after reading document

let doc = read_document(&input, &args)?;

// Build filter list from CLI args (interleaved order)
let filters = build_filter_list(&args.filters, &args.lua_filters)?;

// Apply external filters
let doc = if filters.is_empty() {
    doc
} else {
    let context = FilterContext {
        pandoc_version: env!("CARGO_PKG_VERSION").to_string(),
        reader_options: build_reader_options(&args),
    };
    apply_filters(doc, &filters, &args.to, &context)?
};

// Continue with output
write_document(&doc, &args)?;
```

## JSON Format Considerations

### Pandoc JSON Compatibility

Our JSON writer already produces Pandoc-compatible JSON. Key points:

1. **API version header**: Include `pandoc-api-version` field
2. **Type tags**: Elements use `{"t": "Type", "c": content}` format
3. **Metadata format**: `MetaInlines`, `MetaBlocks`, `MetaString`, etc.

### Source Information

By default, our JSON writer includes source location information. For filter compatibility:

- Use `--json-source-location none` mode for filter I/O
- Or define a "filter JSON" mode that strips source info

**Recommendation:** Initially, strip source info for filter JSON since:
1. Pandoc filters don't expect it
2. It reduces JSON size significantly
3. Filters can corrupt source info inadvertently

We can add source info preservation later if needed.

## Implementation Phases

### Phase 1: Basic JSON Filter Support
- [ ] Add `--filter` CLI argument
- [ ] Create `external_filters` module
- [ ] Implement `apply_json_filter` function
- [ ] Implement interpreter detection
- [ ] Basic error handling
- [ ] Integration test with simple Python filter

### Phase 2: Enhanced Features
- [ ] Filter path discovery (data dir, PATH)
- [ ] Multiple filter composition
- [ ] Environment variable support
- [ ] Improved error messages with filter output

### Phase 3: Polish
- [ ] Document the filter interface
- [ ] Verify compatibility with existing Pandoc filters
- [ ] Performance testing

## Testing Strategy

### Unit Tests

```rust
#[test]
fn test_interpreter_detection() {
    assert_eq!(get_interpreter("filter.py"), Some("python3"));
    assert_eq!(get_interpreter("filter.hs"), Some("runhaskell"));
    // etc.
}

#[test]
fn test_filter_discovery() {
    // Set up temp data dir with filters
    // Test discovery order
}
```

### Integration Tests

Create test filters in various languages:

```python
# tests/filters/identity.py
#!/usr/bin/env python3
import sys
import json

doc = json.load(sys.stdin)
json.dump(doc, sys.stdout)
```

```python
# tests/filters/uppercase.py
#!/usr/bin/env python3
import sys
import json

def uppercase_strs(obj):
    if isinstance(obj, dict):
        if obj.get('t') == 'Str':
            obj['c'] = obj['c'].upper()
        else:
            for v in obj.values():
                uppercase_strs(v)
    elif isinstance(obj, list):
        for item in obj:
            uppercase_strs(item)
    return obj

doc = json.load(sys.stdin)
json.dump(uppercase_strs(doc), sys.stdout)
```

```rust
#[test]
fn test_identity_filter() {
    let input = "Hello *world*";
    let doc = parse_qmd(input);
    let filtered = apply_json_filter(&doc, "tests/filters/identity.py", "html")?;
    assert_eq!(doc, filtered);
}

#[test]
fn test_uppercase_filter() {
    let input = "Hello world";
    let doc = parse_qmd(input);
    let filtered = apply_json_filter(&doc, "tests/filters/uppercase.py", "html")?;
    // Verify all Str elements are uppercase
}
```

## Risks and Mitigations

| Risk | Impact | Mitigation |
|------|--------|------------|
| JSON format incompatibility | Filters fail | Test with real Pandoc filters |
| Performance overhead | Slow for large docs | Document overhead; use Lua for perf |
| Security (arbitrary code exec) | High | Filters are explicitly user-specified |
| Cross-platform issues | Windows support | Use `which` crate; test on CI |

## Dependencies

- `which`: For finding executables in PATH
- `thiserror`: For error types
- (Optional) `serde_json`: Already used

## Open Questions

1. **Should we support `--filter=citeproc`?** Pandoc has a built-in citeproc filter. We could add this later.

2. **Filter argument passing**: Pandoc allows `--filter foo.py --metadata key=value`. Do we need this?

3. **Source info in filtered output**: Should we try to preserve/reconstruct source info after filtering?

## References

- Pandoc filter documentation: `external-sources/pandoc/doc/filters.md`
- Pandoc JSON filter implementation: `external-sources/pandoc/src/Text/Pandoc/Filter/JSON.hs`
- Explorer notes: `FILTER_SUMMARY.md`, `FILTER_ARCHITECTURE_FINDINGS.md`
