# Architecture Documentation

## Module Structure

### Core Modules

#### `lib.rs`
- Entry point for the library
- Exports all public modules: `traversals`, `errors`, `pandoc`, `writers`, `filters`

#### `main.rs`
- Command-line interface implementation
- Handles stdin input and stdout output
- Supports multiple output formats via `--to` parameter
- Default output format is "native"

#### `traversals.rs`
- **Critical Component**: All tree-sitter traversals must use helpers from this module
- Provides two main traversal patterns:
  - `topdown_traverse_concrete_tree`: For visiting nodes in pre-order
  - `bottomup_traverse_concrete_tree`: For building results from leaves up
- Implements `TraversePhase` enum for Enter/Exit phases

#### `pandoc.rs`
- Defines Rust data structures that mirror Pandoc's AST types
- Core types include:
  - `Pandoc`: Root document structure
  - `Block`: Block-level elements (paragraphs, headers, lists, etc.)
  - `Inline`: Inline elements (text, emphasis, links, etc.)
  - `Attr`: Attributes tuple format
- Contains the main conversion function `treesitter_to_pandoc`

#### `errors.rs`
- Error handling and validation
- `parse_is_good` function checks for parsing errors
- Provides error message formatting

#### `filters.rs`
- AST transformation pipeline
- Implements filter pattern for processing Pandoc AST
- Used for applying transformations before output

### Writers Module (`writers/`)

#### `mod.rs`
- Module organization for different output writers

#### `native.rs`
- Generates Pandoc's native format output
- Human-readable representation of the AST

#### `json.rs`
- JSON serialization of Pandoc AST
- Compatible with Pandoc's JSON format

## Data Flow

```
Markdown Input → Tree-sitter Parser → Tree-sitter AST → Pandoc AST → Writer → Output
```

1. **Input**: Raw markdown text from stdin
2. **Parsing**: `tree-sitter-qmd` creates syntax tree
3. **Error Check**: Validate parse tree for errors
4. **Conversion**: Transform tree-sitter nodes to Pandoc AST using bottom-up traversal
5. **Filtering**: Apply transformations via filter pipeline
6. **Output**: Format as JSON or native representation

## Design Patterns

### Two-Stage Parsing
- Block structure parsing first
- Inline structure parsing within each block
- Requires custom traversal helpers due to complexity

### Bottom-Up Processing
- AST construction starts from leaf nodes
- Parent nodes built from processed children
- Enables proper context handling

### Filter Pipeline
- Modular transformation system
- Each filter can modify the AST
- Composable and extensible design

## Key Design Decisions

### Source Location Tracking
- Maintains precise location information from original source
- Critical for error reporting and IDE integration
- Handles complex cases like footnote references

### Pandoc Compatibility
- Data structures exactly mirror Pandoc's types
- Ensures seamless integration with existing Pandoc ecosystem
- Supports standard Pandoc output formats