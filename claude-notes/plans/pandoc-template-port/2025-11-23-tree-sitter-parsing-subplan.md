# Tree-Sitter Based Template Parsing - Subplan

**Date**: 2025-11-23
**Context**: Evaluation of using tree-sitter for template parsing instead of hand-written recursive descent
**Parent Plan**: [2025-11-23-initial-analysis.md](./2025-11-23-initial-analysis.md)

## Executive Summary

After studying the error generation system in `quarto-markdown-pandoc`, using **tree-sitter for template parsing is feasible and strongly recommended**. The existing error generation infrastructure can be extracted into a shared crate (`quarto-parse-errors`) and reused for both qmd and templates.

**Key Benefits**:
- Consistent, high-quality error messages across the project
- Automatic source location tracking
- Potential syntax highlighting in editors
- Reuse of battle-tested error generation system
- Consistency with existing codebase patterns

## 1. Current Error Generation System Analysis

### 1.1 Architecture Overview

The qmd error system consists of several components:

```
Error Corpus (JSON files)
    ↓ (processed by build_error_table.ts)
Auto-generated Table (state, sym) → ErrorInfo
    ↓ (embedded via macro at compile time)
Rust Error Table (lookup functions)
    ↓ (used during parsing)
Tree-sitter Log Observer (captures parse events)
    ↓ (matched against error table)
Diagnostic Messages (via quarto-error-reporting)
    ↓ (rendered via Ariadne or JSON)
Beautiful Error Output
```

### 1.2 Key Components

#### A. Error Corpus (`resources/error-corpus/*.json`)

Example structure:
```json
{
  "code": "Q-2-10",
  "title": "Closed Quote Without Matching Open Quote",
  "message": "A space is causing a quote mark to be interpreted as a quotation close.",
  "notes": [{
    "message": "This is the opening quote...",
    "label": "quote-start",
    "noteType": "simple"
  }],
  "cases": [{
    "name": "simple",
    "content": "a' b.",
    "captures": [{
      "label": "quote-start",
      "row": 0,
      "column": 1,
      "size": 1
    }]
  }]
}
```

#### B. Build Script (`scripts/build_error_table.ts`)

**Process**:
1. Read error corpus JSON files
2. For each test case, write content to a `.qmd` file
3. Run parser with `--_internal-report-error-state` flag
4. Capture `(state, sym)` pairs when errors occur
5. Match captures with LR states from consumed tokens
6. Build `_autogen-table.json` mapping `(state, sym)` → `ErrorInfo`

**Output format**:
```json
[{
  "state": 1425,
  "sym": "_close_block",
  "errorInfo": {
    "code": "Q-2-1",
    "title": "Unclosed Span",
    "message": "I reached the end...",
    "captures": [{ "lrState": 171, "row": 0, "column": 3, ... }],
    "notes": [...],
    "hints": []
  }
}]
```

#### C. Rust Integration

**`qmd_error_message_table.rs`**:
- Defines `ErrorTableEntry`, `ErrorInfo`, `ErrorCapture`, `ErrorNote` types
- Uses `include_error_table!` macro to embed JSON at compile time
- Provides `lookup_error_entry(state, sym)` function

**`tree_sitter_log_observer.rs`**:
- Implements tree-sitter logging callback
- Captures consumed tokens with `(lr_state, sym, row, column, size)`
- Records error states during parsing
- Provides parse logs for error generation

**`qmd_error_messages.rs`**:
- Converts parse errors to `DiagnosticMessage` objects
- Matches error states with error table entries
- Finds captured tokens in parse log
- Creates source locations using `quarto-source-map`
- Builds structured diagnostics via `DiagnosticMessageBuilder`

#### D. quarto-error-reporting Integration

The diagnostic messages use:
- `DiagnosticMessageBuilder` for fluent API
- `SourceInfo` for precise source locations
- `DetailItem` for notes and hints
- Ariadne rendering for beautiful terminal output
- JSON serialization for machine-readable errors

### 1.3 Why This System is Excellent

1. **Example-based error messages**: Inspired by Jeffery's TOPLAS 2003 paper "Generating Syntax Errors from Examples"
2. **Maintainable**: Error messages are in JSON, not scattered in parser code
3. **Testable**: Each error has test cases that verify the error triggers correctly
4. **Precise**: Source locations automatically tracked via tree-sitter
5. **Evolvable**: Grammar changes don't break error messages (script rebuilds table)
6. **Beautiful**: Ariadne provides compiler-quality error output

## 2. Extracting to Shared Crate

### 2.1 Proposed Crate Structure

```
crates/quarto-parse-errors/        (NEW shared crate)
├── src/
│   ├── lib.rs
│   ├── error_table.rs             # Generic error table types
│   ├── tree_sitter_observer.rs    # Tree-sitter logging (generic)
│   ├── error_generation.rs        # Diagnostic message generation
│   └── macros.rs                  # include_error_table! macro
├── build-scripts/
│   └── build_error_table.ts       # Generic script (parameterized)
└── Cargo.toml

Dependencies:
  - quarto-error-reporting (for DiagnosticMessage)
  - quarto-source-map (for SourceInfo)
  - tree-sitter (for logging)
  - serde, serde_json
```

### 2.2 Generic Error Table Types

```rust
// crates/quarto-parse-errors/src/error_table.rs

/// Generic error capture - doesn't depend on specific grammar
#[derive(Debug, Clone)]
pub struct ErrorCapture {
    pub column: usize,
    pub lr_state: usize,
    pub row: usize,
    pub size: usize,
    pub sym: String,  // Not &'static str - allows dynamic loading
    pub label: String,
}

/// Generic error note
#[derive(Debug, Clone)]
pub struct ErrorNote {
    pub message: String,
    pub label: Option<String>,
    pub note_type: String,
    // ... other fields
}

/// Generic error info - independent of grammar
#[derive(Debug, Clone)]
pub struct ErrorInfo {
    pub code: Option<String>,
    pub title: String,
    pub message: String,
    pub captures: Vec<ErrorCapture>,
    pub notes: Vec<ErrorNote>,
    pub hints: Vec<String>,
}

/// Generic error table entry
#[derive(Debug, Clone)]
pub struct ErrorTableEntry {
    pub state: usize,
    pub sym: String,
    pub row: usize,
    pub column: usize,
    pub error_info: ErrorInfo,
    pub name: String,
}

/// Error table - can be loaded from any grammar
pub struct ErrorTable {
    entries: Vec<ErrorTableEntry>,
}

impl ErrorTable {
    /// Look up error entry by (state, sym) pair
    pub fn lookup(&self, state: usize, sym: &str) -> Vec<&ErrorTableEntry> {
        self.entries.iter()
            .filter(|e| e.state == state && e.sym == sym)
            .collect()
    }

    /// Load from JSON (for dynamic loading)
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        let entries = serde_json::from_str(json)?;
        Ok(ErrorTable { entries })
    }

    /// Load from static JSON (for compile-time embedding)
    pub fn from_static_json(json: &'static str) -> Self {
        Self::from_json(json).expect("Invalid error table JSON")
    }
}
```

### 2.3 Generic Tree-Sitter Observer

```rust
// crates/quarto-parse-errors/src/tree_sitter_observer.rs

/// Consumed token - generic across grammars
#[derive(Debug, Clone)]
pub struct ConsumedToken {
    pub row: usize,
    pub column: usize,
    pub size: usize,
    pub lr_state: usize,
    pub sym: String,
}

/// Parse state message
#[derive(Debug, Clone)]
pub struct ProcessMessage {
    pub version: usize,
    pub state: usize,
    pub row: usize,
    pub column: usize,
    pub sym: String,
    pub size: usize,
}

/// Tree-sitter parse log - generic
pub struct TreeSitterParseLog {
    pub consumed_tokens: Vec<ConsumedToken>,
    pub all_tokens: Vec<ConsumedToken>,
    pub error_states: Vec<ProcessMessage>,
    // ... other fields
}

/// Tree-sitter observer - works with any grammar
pub struct TreeSitterLogObserver {
    pub parses: Vec<TreeSitterParseLog>,
    // ... internal state
}

impl TreeSitterLogObserver {
    pub fn new() -> Self { ... }

    pub fn had_errors(&self) -> bool { ... }

    /// Tree-sitter logging callback
    pub fn log(&mut self, log_type: tree_sitter::LogType, message: &str) {
        // Same implementation as current qmd version
        // This is grammar-agnostic - just parses tree-sitter log messages
    }
}
```

### 2.4 Generic Error Generation

```rust
// crates/quarto-parse-errors/src/error_generation.rs

use quarto_error_reporting::DiagnosticMessage;
use quarto_source_map::SourceInfo;

/// Generate diagnostic messages from parse errors
/// Generic - works with any grammar's error table
pub fn produce_diagnostic_messages(
    input: &str,
    log_observer: &TreeSitterLogObserver,
    error_table: &ErrorTable,
    filename: &str,
) -> Vec<DiagnosticMessage> {
    let mut result = Vec::new();

    for parse in &log_observer.parses {
        for error_state in &parse.error_states {
            // Look up in error table
            let entries = error_table.lookup(error_state.state, &error_state.sym);

            for entry in entries {
                // Build diagnostic message (same logic as qmd version)
                let diagnostic = build_diagnostic(
                    input,
                    error_state,
                    &parse.consumed_tokens,
                    &parse.all_tokens,
                    entry,
                    filename,
                );
                result.push(diagnostic);
            }
        }
    }

    result
}

fn build_diagnostic(
    input: &str,
    error_state: &ProcessMessage,
    consumed_tokens: &[ConsumedToken],
    all_tokens: &[ConsumedToken],
    entry: &ErrorTableEntry,
    filename: &str,
) -> DiagnosticMessage {
    // Same implementation as qmd_error_messages.rs
    // Just uses generic types instead of qmd-specific ones
    ...
}
```

### 2.5 Macro for Compile-Time Embedding

```rust
// crates/quarto-parse-errors/src/macros.rs

/// Macro to embed error table JSON at compile time
/// Used by both qmd and templates
#[macro_export]
macro_rules! include_error_table {
    ($path:expr) => {{
        const ERROR_TABLE_JSON: &'static str = include_str!($path);
        $crate::ErrorTable::from_static_json(ERROR_TABLE_JSON)
    }};
}
```

### 2.6 Generic Build Script

The TypeScript build script becomes parameterized:

```typescript
// crates/quarto-parse-errors/build-scripts/build_error_table.ts

interface BuildConfig {
    corpusDir: string;           // e.g., "resources/error-corpus"
    caseFilesDir: string;        // e.g., "resources/error-corpus/case-files"
    parserBinary: string;        // e.g., "../target/debug/quarto-markdown-pandoc"
    outputFile: string;          // e.g., "resources/error-corpus/_autogen-table.json"
    fileExtension: string;       // e.g., ".qmd" or ".tmpl"
}

async function buildErrorTable(config: BuildConfig) {
    // Same logic as current script, but parameterized
    ...
}
```

Each crate can then call this with its own config:

```typescript
// crates/quarto-markdown-pandoc/scripts/build_error_table.ts
import { buildErrorTable } from '../../quarto-parse-errors/build-scripts/build_error_table.ts';

await buildErrorTable({
    corpusDir: "resources/error-corpus",
    caseFilesDir: "resources/error-corpus/case-files",
    parserBinary: "../../target/debug/quarto-markdown-pandoc",
    outputFile: "resources/error-corpus/_autogen-table.json",
    fileExtension: ".qmd"
});
```

## 3. Tree-Sitter Grammar for Templates

### 3.1 Grammar Design

Templates have simpler syntax than qmd, making tree-sitter a perfect fit:

```javascript
// crates/tree-sitter-template/grammar.js

module.exports = grammar({
    name: 'template',

    rules: {
        template: $ => repeat($._node),

        _node: $ => choice(
            $.literal,
            $.variable,
            $.conditional,
            $.for_loop,
            $.partial,
            $.comment,
            $.nesting_directive,
            $.breakable_space,
        ),

        // Literal text (anything not a template directive)
        literal: $ => /[^$]+|\$(?!\$|if|for|endfor|endif|else|elseif|--|\{|\^|~)/,

        // $$ for literal $
        literal_dollar: $ => '$$',

        // Comment: $-- ... \n
        comment: $ => seq('$--', /[^\n]*/, '\n'),

        // Variable: $foo$ or ${foo} or $foo.bar.baz/pipe1/pipe2$
        variable: $ => choice(
            seq('$', $.variable_ref, '$'),
            seq('${', $.variable_ref, '}')
        ),

        variable_ref: $ => seq(
            $.variable_path,
            optional($.pipes)
        ),

        variable_path: $ => seq(
            $.identifier,
            repeat(seq('.', $.identifier))
        ),

        identifier: $ => /[a-zA-Z][a-zA-Z0-9_-]*/,

        pipes: $ => repeat1(seq('/', $.pipe)),

        pipe: $ => seq(
            $.pipe_name,
            optional($.pipe_args)
        ),

        pipe_name: $ => choice(
            'pairs', 'uppercase', 'lowercase', 'length', 'reverse',
            'first', 'last', 'rest', 'allbutlast', 'chomp', 'nowrap',
            'alpha', 'roman', 'left', 'right', 'center'
        ),

        pipe_args: $ => repeat1(choice(
            $.number,
            $.quoted_string
        )),

        // Conditional: $if(foo)$...$endif$
        conditional: $ => seq(
            $.if_branch,
            repeat($.elseif_branch),
            optional($.else_branch),
            $.endif
        ),

        if_branch: $ => seq(
            choice(
                seq('$if(', $.variable_path, ')$'),
                seq('${if(', $.variable_path, ')}')),
            repeat($._node)
        ),

        elseif_branch: $ => seq(
            choice(
                seq('$elseif(', $.variable_path, ')$'),
                seq('${elseif(', $.variable_path, ')}')),
            repeat($._node)
        ),

        else_branch: $ => seq(
            choice('$else$', '${else}'),
            repeat($._node)
        ),

        endif: $ => choice('$endif$', '${endif}'),

        // For loop: $for(items)$...$sep$...$endfor$
        for_loop: $ => seq(
            $.for_start,
            repeat($._node),
            optional($.separator),
            $.endfor
        ),

        for_start: $ => choice(
            seq('$for(', $.variable_path, ')$'),
            seq('${for(', $.variable_path, ')}')),

        separator: $ => seq(
            choice('$sep$', '${sep}'),
            repeat($._node)
        ),

        endfor: $ => choice('$endfor$', '${endfor}'),

        // Partial: ${partial()} or ${data:partial()} or ${items:partial()[sep]}
        partial: $ => choice(
            seq('${', $.partial_name, '(', ')', '}'),
            seq('${', $.variable_path, ':', $.partial_name, '(', ')', '}'),
            seq('${', $.variable_path, ':', $.partial_name, '(', ')', '[', $.literal_separator, ']', '}'),
            seq('${', $.variable_path, '[', $.literal_separator, ']', '}')
        ),

        partial_name: $ => /[a-zA-Z][a-zA-Z0-9_.-]*/,

        literal_separator: $ => /[^]]*/,

        // Nesting directive: $^$
        nesting_directive: $ => choice('$^$', '${^}'),

        // Breakable space: $~$...$~$
        breakable_space: $ => seq(
            choice('$~$', '${~}'),
            repeat($._node),
            choice('$~$', '${~}')
        ),

        // Helpers
        number: $ => /\d+/,
        quoted_string: $ => /"(?:[^"\\]|\\.)*"/,
    }
});
```

### 3.2 Error Examples for Templates

Create error corpus for common template errors:

**`resources/error-corpus/T-1-1.json`**: (T for Template)
```json
{
  "code": "T-1-1",
  "title": "Unclosed Variable",
  "message": "Variable reference started but not closed",
  "notes": [{
    "message": "This is where the variable starts",
    "label": "var-start",
    "noteType": "simple"
  }],
  "cases": [{
    "name": "simple",
    "content": "$foo",
    "captures": [{
      "label": "var-start",
      "row": 0,
      "column": 0,
      "size": 1
    }]
  }]
}
```

**`resources/error-corpus/T-1-2.json`**:
```json
{
  "code": "T-1-2",
  "title": "Unmatched Endif",
  "message": "Found $endif$ without matching $if()$",
  "notes": [{
    "message": "This endif has no matching if",
    "label": "endif",
    "noteType": "simple"
  }],
  "cases": [{
    "name": "simple",
    "content": "text $endif$",
    "captures": [{
      "label": "endif",
      "row": 0,
      "column": 5,
      "size": 7
    }]
  }]
}
```

**`resources/error-corpus/T-1-3.json`**:
```json
{
  "code": "T-1-3",
  "title": "Unclosed Conditional",
  "message": "Conditional block started but never closed",
  "notes": [{
    "message": "This is the opening $if()$",
    "label": "if-start",
    "noteType": "simple"
  }],
  "cases": [{
    "name": "simple",
    "content": "$if(foo)$ text",
    "captures": [{
      "label": "if-start",
      "row": 0,
      "column": 0,
      "size": 9
    }]
  }]
}
```

### 3.3 Template Parser Integration

```rust
// crates/quarto-templates/src/parser.rs

use quarto_parse_errors::{
    ErrorTable, TreeSitterLogObserver, produce_diagnostic_messages
};

pub struct TemplateParser {
    error_table: ErrorTable,
}

impl TemplateParser {
    pub fn new() -> Self {
        // Load error table at startup
        let error_table = ErrorTable::from_static_json(
            include_str!("../resources/error-corpus/_autogen-table.json")
        );
        TemplateParser { error_table }
    }

    pub fn parse(&self, input: &str, filename: &str) -> Result<Template, Vec<DiagnosticMessage>> {
        // Set up tree-sitter
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&tree_sitter_template::language())
            .expect("Error loading template grammar");

        // Set up logging to capture parse events
        let mut observer = TreeSitterLogObserver::new();
        parser.set_logger(Some(Box::new(|log_type, message| {
            observer.log(log_type, message);
        })));

        // Parse
        let tree = parser.parse(input, None)
            .expect("Tree-sitter parse failed");

        // Check for errors
        if observer.had_errors() {
            let diagnostics = produce_diagnostic_messages(
                input,
                &observer,
                &self.error_table,
                filename
            );
            return Err(diagnostics);
        }

        // Convert tree to Template AST
        let template = self.tree_to_ast(&tree, input)?;
        Ok(template)
    }

    fn tree_to_ast(&self, tree: &tree_sitter::Tree, input: &str) -> Result<Template, Vec<DiagnosticMessage>> {
        // Walk tree-sitter CST and build our Template AST
        ...
    }
}
```

## 4. Crate Structure Overview

```
crates/
├── quarto-parse-errors/          ← NEW shared crate
│   ├── src/
│   │   ├── lib.rs
│   │   ├── error_table.rs
│   │   ├── tree_sitter_observer.rs
│   │   ├── error_generation.rs
│   │   └── macros.rs
│   ├── build-scripts/
│   │   └── build_error_table.ts
│   └── Cargo.toml
│
├── tree-sitter-template/         ← NEW grammar crate
│   ├── grammar.js
│   ├── src/
│   │   ├── parser.c              (generated)
│   │   └── lib.rs
│   ├── test/
│   │   └── corpus/
│   └── Cargo.toml
│
├── quarto-templates/             ← Template engine crate
│   ├── src/
│   │   ├── lib.rs
│   │   ├── parser.rs             (uses tree-sitter-template)
│   │   ├── ast.rs
│   │   ├── evaluator.rs
│   │   └── ...
│   ├── resources/
│   │   └── error-corpus/
│   │       ├── T-*.json          (template errors)
│   │       ├── case-files/       (generated)
│   │       └── _autogen-table.json (generated)
│   ├── scripts/
│   │   └── build_error_table.ts  (calls shared script)
│   └── Cargo.toml
│       dependencies:
│         - quarto-parse-errors
│         - tree-sitter-template
│         - quarto-error-reporting
│         - quarto-source-map
│
└── quarto-markdown-pandoc/       ← Existing qmd crate (refactored)
    ├── src/
    │   ├── readers/
    │   │   ├── qmd.rs            (uses quarto-parse-errors)
    │   │   └── ...
    │   └── ...
    ├── resources/
    │   └── error-corpus/
    │       ├── Q-*.json          (qmd errors)
    │       └── _autogen-table.json (generated)
    ├── scripts/
    │   └── build_error_table.ts  (calls shared script)
    └── Cargo.toml
        dependencies:
          - quarto-parse-errors  ← NEW dependency
          - tree-sitter-qmd
          - quarto-error-reporting
          - quarto-source-map
```

## 5. Feasibility Analysis

### 5.1 Advantages

1. **Consistency**: Same error message quality across qmd and templates
2. **Maintainability**: Error messages in JSON, not scattered in code
3. **Testability**: Each error has test cases
4. **Source tracking**: Free with tree-sitter
5. **Syntax highlighting**: Potential for editor support
6. **Proven system**: Already works well for qmd
7. **Code reuse**: 80% of error generation code can be shared
8. **Evolvability**: Grammar changes don't break error messages

### 5.2 Challenges

#### Challenge 1: Extracting Generic Error System

**Complexity**: Medium

**Approach**:
1. Create `quarto-parse-errors` crate
2. Move generic types (ErrorTable, TreeSitterObserver, etc.)
3. Keep grammar-specific parts in qmd/template crates
4. Test with qmd first (no behavior changes)
5. Then use for templates

**Risk**: Low - mostly code movement, no algorithm changes

#### Challenge 2: Tree-Sitter Grammar for Templates

**Complexity**: Low

**Rationale**:
- Template syntax is simpler than Markdown
- No complex nesting (unlike qmd's block/inline two-phase parsing)
- Clear delimiters (`$...$` or `${...}`)
- Context-free grammar

**Risk**: Very Low - straightforward grammar

#### Challenge 3: Build Script Generalization

**Complexity**: Low

**Approach**:
- Extract common logic to shared TypeScript module
- Parameterize file extensions, binary paths, etc.
- Each crate calls with its own config

**Risk**: Low - mostly refactoring

### 5.3 Migration Path

**Phase 1: Extract quarto-parse-errors** (1-2 weeks)
1. Create new crate with generic types
2. Move TreeSitterLogObserver (make generic)
3. Move error generation logic (make generic)
4. Add error table loading/lookup
5. Add compile-time embedding macro
6. Test thoroughly

**Phase 2: Refactor qmd to use shared crate** (1 week)
1. Update qmd to depend on quarto-parse-errors
2. Replace qmd-specific types with generic ones
3. Use shared error generation
4. Verify no behavior changes (all tests pass)
5. Clean up old code

**Phase 3: Create tree-sitter-template** (1 week)
1. Write grammar.js for template syntax
2. Generate parser with tree-sitter-cli
3. Create Rust bindings
4. Write grammar tests
5. Verify parser works correctly

**Phase 4: Template error corpus** (1 week)
1. Create error corpus for templates (T-*.json files)
2. Write build script (calls shared script)
3. Generate error table
4. Test error messages

**Phase 5: Template parser integration** (1 week)
1. Implement TemplateParser using tree-sitter-template
2. Convert CST to Template AST
3. Integrate error generation
4. Test with error corpus
5. Verify beautiful error messages

**Total estimated time**: 5-6 weeks

### 5.4 Comparison: Tree-Sitter vs Hand-Written

| Aspect | Tree-Sitter | Hand-Written Parser |
|--------|-------------|---------------------|
| Error messages | Excellent (via error corpus) | Good (manual error handling) |
| Source tracking | Automatic | Manual (error-prone) |
| Syntax highlighting | Yes (via grammar) | No |
| Development time | Longer upfront | Shorter upfront |
| Maintenance | Easy (grammar file) | Medium (scattered code) |
| Consistency | High (with qmd) | Varies |
| Error quality | High (example-based) | Depends on care taken |
| Code reuse | High (shared system) | Low |

## 6. Recommendation

**Strongly recommend using tree-sitter for template parsing.**

### Rationale:

1. **Consistency matters**: Templates are part of Quarto. Users should get the same quality error messages for templates as for qmd.

2. **System already exists**: The error generation system is battle-tested and works beautifully for qmd. Reusing it for templates is natural.

3. **Long-term maintainability**: Extracting the error system to a shared crate benefits both qmd and templates, and potentially future parsers.

4. **Grammar simplicity**: Template syntax is actually simpler than qmd, so tree-sitter is not overkill - it's appropriate.

5. **Syntax highlighting bonus**: Once we have a tree-sitter grammar, editor syntax highlighting comes nearly for free (via tree-sitter queries).

6. **Investment pays off**: While the upfront work is more than a hand-written parser, the long-term benefits (maintainability, consistency, error quality) far outweigh the cost.

### Trade-offs:

- **More initial work**: 5-6 weeks vs 2-3 weeks for hand-written
- **More dependencies**: tree-sitter, shared error crate
- **Learning curve**: Team needs to understand tree-sitter grammars

But these are one-time costs for long-term benefits.

## 7. Next Steps

If approved:

1. **Week 1-2**: Create `quarto-parse-errors` crate and extract generic error system
2. **Week 3**: Refactor qmd to use shared crate (verify no regressions)
3. **Week 4**: Create `tree-sitter-template` grammar
4. **Week 5**: Build template error corpus and generation
5. **Week 6**: Integrate template parser with error system

Then proceed with main template port plan (phases 2-7).

## 8. Conclusion

Using tree-sitter for template parsing is **feasible, beneficial, and recommended**. The existing error generation system can be extracted into a shared crate with moderate effort, and the template grammar is straightforward. The result will be:

- **Beautiful, consistent error messages** across qmd and templates
- **Maintainable codebase** with reusable infrastructure
- **Future-proof** design that can support additional parsers
- **Professional-quality** user experience

This aligns with the project's goals of providing excellent error messages and maintaining high code quality.
