# Quarto Markdown

The main documentation for this repository is located at:
[crates/quarto-markdown-pandoc/CLAUDE.md](crates/quarto-markdown-pandoc/CLAUDE.md)
- in this repository, "qmd" means "quarto markdown", the dialect of markdown we are developing. Although we aim to be largely compatible with Pandoc, it is not necessarily the case that a discrepancy in the behavior is a bug.
- the qmd format only supports the inline syntax for a link [link](./target.html), and not the reference-style syntax [link][1].