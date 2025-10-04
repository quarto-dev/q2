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

## Binary usage

The `quarto-markdown-pandoc` binary accepts the following options:
- `-t, --to <TO>`: Output format (default: native)
- `-v, --verbose`: Verbose output
- `-i, --input <INPUT>`: Input file (default: stdin)
- `--loose`: Loose parsing mode
- `--json-errors`: Output errors as JSON
- `-h, --help`: Show help

## Instructions

- in this repository, "qmd" means "quarto markdown", the dialect of markdown we are developing. Although we aim to be largely compatible with Pandoc, it is not necessarily the case that a discrepancy in the behavior is a bug.
- the qmd format only supports the inline syntax for a link [link](./target.html), and not the reference-style syntax [link][1].
- Always strive for test documents as small as possible. Prefer a large number of small test documents instead of small number of large documents.
- When fixing bugs, always try to isolate and fix one bug at a time.
- When fixing bugs using tests, run the failing test before attempting to fix issues. This helps ensuring that tests are exercising the failure as expected, and fixes actually fix the particular issue.
- If you need to fix parser bugs, you will find use in running the application with "-v", which will provide a large amount of information from the tree-sitter parsing process, including a print of the concrete syntax tree out to stderr.
- use "cargo run --" instead of trying to find the binary location, which will often be outside of this crate.
- If you need to fix parser bugs, you will find use in running the application with "-v", which will provide a large amount of information from the tree-sitter parsing process, including a print of the concrete syntax tree out to stderr.
- When fixing inconsistency bugs, use `pandoc -t json -i <input_file>` to get Pandoc's output, and `cargo run -- -t json -i <input_file>` to get our output.
- When fixing roundtripping bugs, make sure to always add a roundtripping test to `tests/roundtrip_tests/qmd-json-qmd`.
- When I say "@doit", I mean "create a plan, and work on it item by item."
- When you're done editing a Rust file, run `cargo fmt` on it.
- If I ask you to write notes to yourself, do it in markdown and write the output in the `docs/for-claude` directory.
