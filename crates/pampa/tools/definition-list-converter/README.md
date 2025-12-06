# Definition List Converter

This tool converts Pandoc's native DefinitionList AST nodes to the div-based definition list syntax used by quarto-markdown.

## Background

Pandoc supports definition lists using the following syntax:

```markdown
Term 1
: Definition 1

Term 2
: Definition 2a
: Definition 2b
```

However, quarto-markdown uses a div-based syntax with explicit structure:

```markdown
::: {.definition-list}
* Term 1
  - Definition 1
* Term 2
  - Definition 2a
  - Definition 2b
:::
```

This Lua filter converts from Pandoc's native definition list syntax to the quarto-markdown div syntax.

## Usage

To convert a document containing Pandoc-style definition lists:

```bash
pandoc -f markdown -t markdown \
  --lua-filter=definition-list-to-div.lua \
  input.md -o output.md
```

### Example

Given this input (`test-input.md`):

```markdown
Term 1
: Definition 1

Term 2
: Definition 2a
: Definition 2b
```

Running the filter:

```bash
pandoc -f markdown -t markdown \
  --lua-filter=definition-list-to-div.lua \
  test-input.md
```

Produces:

```markdown
::: definition-list
- Term 1

  - Definition 1

- Term 2

  - Definition 2a
  - Definition 2b
:::
```

## Features

- Converts all DefinitionList AST nodes to div-based syntax
- Preserves term formatting (bold, emphasis, code, etc.)
- Handles multiple definitions per term
- Output can be parsed by quarto-markdown back into DefinitionList nodes

## Requirements

- Pandoc 2.11 or later

## See Also

- [Definition Lists Documentation](../../../../docs/syntax/definition-lists.qmd)
- Similar tool: [Grid Table Fixer](../grid-table-fixer/)
