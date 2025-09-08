# quarto-markdown

> [!WARNING]
> This parser isn't used in Quarto yet. This code isn't ready for public consumption. If you stumbled into this repo, then it's very likely not in a state where you'll benefit from it.

This repository hosts a standalone parser for "Quarto Markdown" ("QMD"), the Markdown dialect recognized by [Quarto](https://quarto.org).

## Features

- standalone Rust binary
- built from a pair of tree-sitter grammars forked from the [tree-sitter grammar repo](https://github.com/tree-sitter-grammars/tree-sitter-markdown/)
- recognizes syntax errors
- supports Quarto-specific syntax constructs:
  - code cell blocks with `{language}` syntax
  - shortcodes
  - "reader syntax": `{<html}` for transparently using other Pandoc reader formats inside Markdown
- emits parse tree in Pandoc's `json` and `native` formats
  - as a consequence, the Quarto-specific syntax constructs are "desugared" into standard Pandoc AST nodes

## Background

### Syntax drift from Pandoc

Pandoc 3 doesn't support the following syntax:

````
```{python}
print("hello, world")
```
````

### Syntax Errors are good, actually

In a number of ways, Quarto Markdown _isn't_ Markdown. Most importantly, Quarto Markdown gives syntax errors in malformed documents.
Modern markdown dialects include a number of syntax constructs that can be the source of mistakes.
Standards such as Commonmark dictate that [no documents ever contain mistakes](https://spec.commonmark.org/0.31.2/#preliminaries):

> Any sequence of characters is a valid CommonMark document.

This isn't a tenable situation in large documents.

### First-class support for external tooling

A robust parser for QMD documents will enable more robust treatment in editors, IDEs, and external tooling that needs to inspect documents.

## Important differences

These are non-exhaustive and will only list intentional differences.
We will, aspirationally, treat unintentional differences as bugs.

- no naked HTML support: use `{=html}` raw blocks an inlines
- no grid tables: use `{<markdown}`, [list tables](https://github.com/pandoc-ext/list-table), or Quarto's [HTML-as-table-AST mode](https://quarto.org/docs/authoring/tables.html#html-tables)

## Syntax escape hatches

The "reader" syntax allows users to recover the exact Pandoc markdown behavior when desired.
With this feature, however, other quarto-markdown conveniences will be absent: no error messages, source tracking, etc.

## Current state

Parses [quarto-web](https://github.com/quarto-dev/quarto-web) with a small number of changes

## TODOs

- actually good error messages
- fine-grained source location tracking
- solve glaring performance issues
