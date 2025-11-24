# quarto-parse-errors

Generic error reporting infrastructure for tree-sitter based parsers.

This crate provides a complete system for generating high-quality error messages from tree-sitter parse failures using the "generating syntax errors from examples" approach (Jeffery, TOPLAS 2003).

## Components

1. **Error Corpus**: JSON files mapping parser states to human-readable error messages
2. **TreeSitterLogObserver**: Captures parser state during failed parses
3. **Error Table**: Compile-time embedded error message database
4. **Error Generation**: Converts parser states to user-friendly diagnostics

## Usage

### 1. Set up your parser with error observation

```rust
use quarto_parse_errors::{TreeSitterLogObserver, TreeSitterLogObserverTrait, produce_diagnostic_messages};

let mut parser = tree_sitter::Parser::new();
parser.set_language(your_language)?;

let mut observer = TreeSitterLogObserver::default();
parser.set_logger(Some(Box::new(move |log_type, message| {
    observer.log(log_type, message);
})));

let tree = parser.parse(source_code, None)?;

if observer.had_errors() {
    let diagnostics = produce_diagnostic_messages(
        source_code.as_bytes(),
        &observer,
        &error_table,
        "filename.ext",
        &source_context,
    );
    // Handle diagnostics...
}
```

### 2. Create error corpus

Create JSON files in your error corpus directory (e.g., `resources/error-corpus/E-001.json`):

```json
{
  "code": "E-001",
  "title": "Unclosed Bracket",
  "message": "Expected closing bracket ']' before end of line",
  "notes": [{
    "message": "Opening bracket is here",
    "label": "open-bracket",
    "noteType": "simple"
  }],
  "cases": [{
    "name": "simple",
    "content": "foo [bar\n",
    "captures": [{
      "label": "open-bracket",
      "row": 0,
      "column": 4,
      "size": 1
    }]
  }]
}
```

### 3. Generate error table

**NOTE**: The build script (`scripts/build_error_table.ts`) needs generalization work.
For now, see `crates/quarto-markdown-pandoc/scripts/build_error_table.ts` for a working example.

The script should be invoked like:

```bash
./scripts/build_error_table.ts \
  --cmd '../../target/debug/my-parser --_internal-report-error-state -i' \
  --corpus resources/error-corpus \
  --output resources/error-corpus/_autogen-table.json \
  --extension .ext
```

### 4. Embed error table in your code

```rust
use error_message_macros::include_error_table;
use quarto_parse_errors::ErrorTableEntry;

pub fn get_error_table() -> &'static [ErrorTableEntry] {
    include_error_table!(
        "./resources/error-corpus/_autogen-table.json",
        "quarto_parse_errors"
    )
}
```

## Binary Interface Contract

Your parser binary must support error state reporting for the build script:

```bash
my-parser --_internal-report-error-state -i <file>
```

Output format (JSON to stdout):

```json
{
  "tokens": [
    {
      "row": 0,
      "column": 4,
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
      "column": 8
    }
  ]
}
```

## Status

**Phase 0.1**: Initial extraction from quarto-markdown-pandoc

- ✅ Error table types extracted
- ✅ TreeSitterLogObserver extracted
- ✅ Error generation logic extracted
- ✅ Proc macro updated with module prefix parameter
- ⚠️  Build script needs full generalization (currently WIP)

The build script in `scripts/build_error_table.ts` has been started but needs more work to:
- Replace all hardcoded paths with config parameters
- Support the new `--cmd` parameter for flexible command invocation
- Handle different file extensions properly

For now, parsers can copy and adapt the qmd version from `crates/quarto-markdown-pandoc/scripts/build_error_table.ts`.

## License

Same as parent project
