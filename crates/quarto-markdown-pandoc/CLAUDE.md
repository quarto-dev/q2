This repository contains a Rust library and binary crate that converts Markdown text to
Pandoc's AST representation using a custom tree-sitter grammar for Markdown.

This tree-sitter setup is somewhat unique because Markdown requires a two-step process:
one tree-sitter grammar to establish the block structure, and another tree-sitter grammar
to parse the inline structure within each block.

As a result, in this repository all traversals of the tree-sitter data structure
need to be done with the traversal helpers in traversals.rs.

## Best practices in this repo

- If you want to create a test file, do so in the `tests/` directory.
- When making changes to the code, always run both `cargo check` AND `cargo test` to ensure changes compile and don't affect behavior. The test suite is fast enough to run after each change.
- **CRITICAL**: Do NOT assume changes are safe if ANY tests fail, even if they seem unrelated. Some tests require pandoc to be properly installed to pass. Always ensure ALL tests pass before and after changes.

## Environment setup

- Rust toolchain is installed at `/home/claude-sandbox/.cargo/bin`
- Pandoc is installed at `/home/claude-sandbox/local/bin`
