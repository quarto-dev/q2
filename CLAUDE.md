# Quarto Markdown

The main documentation for this repository is located at:
[crates/quarto-markdown-pandoc/CLAUDE.md](crates/quarto-markdown-pandoc/CLAUDE.md)
- in this repository, "qmd" means "quarto markdown", the dialect of markdown we are developing. Although we aim to be largely compatible with Pandoc, it is not necessarily the case that a discrepancy in the behavior is a bug.
- the qmd format only supports the inline syntax for a link [link](./target.html), and not the reference-style syntax [link][1].
- Always strive for test documents as small as possible. Prefer a large number of small test documents instead of small number of large documents.
- When fixing bugs, always try to isolate and fix one bug at a time.
- When fixing bugs using tests, run the failing test before attempting to fix issues. This helps ensuring that tests are exercising the failure as expected, and fixes actually fix the particular issue.
- If you need to fix parser bugs, you will find use in running the application with "-v", which will provide a large amount of information from the tree-sitter parsing process, including a print of the concrete syntax tree out to stderr.
- use "cargo run --" instead of trying to find the binary location, which will often be outside of this crate.
