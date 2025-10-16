# Quarto Markdown

The main documentation for this repository is located at:
[crates/quarto-markdown-pandoc/CLAUDE.md](crates/quarto-markdown-pandoc/CLAUDE.md)

## **CRITICAL - TEST-DRIVEN DEVELOPMENT**

When fixing ANY bug:
1. **FIRST**: Write the test
2. **SECOND**: Run the test and verify it fails as expected
3. **THIRD**: Implement the fix
4. **FOURTH**: Run the test and verify it passes

**This is non-negotiable. Never implement a fix before verifying the test fails.**

## General Instructions

- in this repository, "qmd" means "quarto markdown", the dialect of markdown we are developing. Although we aim to be largely compatible with Pandoc, it is not necessarily the case that a discrepancy in the behavior is a bug.
- the qmd format only supports the inline syntax for a link [link](./target.html), and not the reference-style syntax [link][1].
- Always strive for test documents as small as possible. Prefer a large number of small test documents instead of small number of large documents.
- When fixing bugs, always try to isolate and fix one bug at a time.
- **CRITICAL - TEST FIRST**: When fixing bugs using tests, you MUST run the failing test BEFORE implementing any fix. This is non-negotiable. Verify the test fails in the expected way, then implement the fix, then verify the test passes.
- If you need to fix parser bugs, you will find use in running the application with "-v", which will provide a large amount of information from the tree-sitter parsing process, including a print of the concrete syntax tree out to stderr.
- use "cargo run --" instead of trying to find the binary location, which will often be outside of this crate.
- when calling shell scripts, ALWAYS BE MINDFUL of the current directory you're operating in. use `pwd` as necessary to avoid confusing yourself over commands that use relative paths.
- When a cd command fails for you, that means you're confused about the current directory. In this situations, ALWAYS run `pwd` before doing anything else.
- use `jq` instead of `python3 -m json.tool` for pretty-printing. When processing JSON in a shell pipeline, prefer `jq` when possible.
- Always create a plan. Always work on the plan one item at a time.
- In the tree-sitter-markdown and tree-sitter-markdown-inline directories, you rebuild the parsers using "tree-sitter generate; tree-sitter build". Make sure the shell is in the correct directory before running those. Every time you change the tree-sitter parsers, rebuild them and run "tree-sitter test". If the tests fail, fix the code. Only change tree-sitter tests you've just added; do not touch any other tests. If you end up getting stuck there, stop and ask for my help.
- When attempting to find binary differences between files, always use `xxd` instead of other tools.
- .c only works in JSON formats. Inside Lua filters, you need to use Pandoc's Lua API. Study https://raw.githubusercontent.com/jgm/pandoc/refs/heads/main/doc/lua-filters.md and make notes to yourself as necessary (use docs/for-claude in this directory)
- Sometimes you get confused by macOS's weird renaming of /tmp. Prefer to use temporary directories local to the project you're working on (which you can later clean)
- The documentation in docs/ is a user-facing Quarto website. There, you should document usage and not technical details.