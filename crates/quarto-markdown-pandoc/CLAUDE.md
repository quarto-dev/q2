This repository contains a Rust library and binary crate that converts Markdown text to
Pandoc's AST representation using custom tree-sitter grammars for Markdown.

The Markdown variant in this repository is close but **not identical** to Pandoc's grammar.

Crucially, this converter is willing and able to emit error messages when Markdown constructs
are written incorrectly on disk.

This tree-sitter setup is somewhat unique because Markdown requires a two-step process:
one tree-sitter grammar to establish the block structure, and another tree-sitter grammar
to parse the inline structure within each block.

As a result, in this repository all traversals of the tree-sitter data structure
need to be done with the traversal helpers in traversals.rs.

## Best practices in this repo

- If you want to create a test file, do so in the `tests/` directory.
- **IMPORTANT**: When making changes to the code, ALWAYS run both `cargo check` AND `cargo test` to ensure changes compile and don't affect behavior. The test suite is fast enough to run after each change. Never skip running `cargo test` - it must always be executed together with `cargo check`.
- **CRITICAL**: Do NOT assume changes are safe if ANY tests fail, even if they seem unrelated. Some tests require pandoc to be properly installed to pass. Always ensure ALL tests pass before and after changes.

## Environment setup

- Rust toolchain is installed at `/home/claude-sandbox/.cargo/bin`
- Pandoc is installed at `/home/claude-sandbox/local/bin`

# Error messages

The error message infrastructure is based on Clinton Jeffery's TOPLAS 2003 paper "Generating Syntax Errors from Examples". You don't need to read the entire paper to understand what's happening. The abstract of the paper is:

LR parser generators are powerful and well-understood, but the parsers they generate are not suited to provide good error messages. Many compilers incur extensive modifications to the source grammar to produce useful syntax error messages. Interpreting the parse state (and input token) at the time of error is a nonintrusive alternative that does not entangle the error recovery mechanism in error message production. Unfortunately, every change to the grammar may significantly alter the mapping from parse states to diagnostic messages, creating a maintenance problem. Merr is a tool that allows a compiler writer to associate diagnostic messages with syntax errors by example, avoiding the need to add error productions to the grammar or interpret integer parse states. From a specification of errors and messages, Merr runs the compiler on each example error to obtain the relevant parse state and input token, and generates a yyerror() function that maps parse states and input tokens to diagnostic messages. Merr enables useful syntax error messages in LR-based compilers in a manner that is robust in the presence of grammar changes.

We're not using "merr" here; we are implementing the same technique.

## Creating error examples

The corpus of error examples in this repository exists in resources/error-corpus.

## Recompiling

After changing any of the resources/error-corpus/*.{json,qmd} files, run the script `scripts/build_error_table.ts`. It's executable with a deno hashbang line. Deno is installed on the environment you'll be running.

## Currently Working On: Markdown Writer (qmd.rs)

### Overview
We are implementing a Markdown writer that converts the Pandoc AST back to Markdown format. The writer is located at `src/writers/qmd.rs`.

### Current Status

#### Implemented Elements

**Block Elements:**
- ✅ Plain
- ✅ Paragraph  
- ✅ BlockQuote (with proper `> ` prefixing for nested content)
- ✅ BulletList (with tight/loose list detection)
- ✅ Div (fenced div syntax with attributes)

**Inline Elements:**
- ✅ Str (basic text)
- ✅ Space
- ✅ SoftBreak

#### Missing Elements to Implement

**Block Elements (10 remaining):**
1. LineBlock - Lines of text with preserved line breaks
2. CodeBlock - Fenced code blocks with optional language/attributes
3. RawBlock - Raw content in a specific format
4. OrderedList - Numbered lists with configurable start/style
5. DefinitionList - Term/definition pairs
6. Header - Headings with levels 1-6
7. HorizontalRule - Thematic breaks (`---`)
8. Table - Tables with alignment and captions
9. Figure - Figures with captions
10. BlockMetadata - Quarto-specific metadata blocks

**Inline Elements:**
✅ All inline elements have been implemented!

### Implementation Strategy

1. **Priority Order (UPDATED):**
   - ✅ **COMPLETED: ALL inline elements** 
   - ✅ **COMPLETED: Major block elements** (Header, CodeBlock, OrderedList, Table, etc.)
   - REMAINING: Complex blocks (Figure, LineBlock, DefinitionList, etc.)

2. **Key Considerations:**
   - Proper escaping of special characters in different contexts
   - Handling of attributes (Pandoc/Quarto style)
   - Nesting and context management (e.g., lists within blockquotes)
   - Tight vs loose list formatting
   - Preserving roundtrip fidelity where possible

3. **Testing Approach:**
   - Write unit tests for each element type
   - Test nested structures
   - Verify escaping rules
   - Compare output with Pandoc's markdown writer for compatibility
