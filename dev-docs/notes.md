## Source tracking

We really want to have relatively precise location tracking.
But Markdown, as always, makes things complicated.

Simple inline nodes are easy to handle.
But footnotes offer a good counter-example. Consider:

```markdown
Paragraph[^1].

[^1]: footnote.
```

Pandoc's `markdown` reader produces this native AST:

```
[ Para
    [ Str "Paragraph"
    , Note [ Para [ Str "footnote." ] ]
    , Str "."
    ]
]
```

The note contents are in a completely different block, and get incorporated into the actual paragraph itself.

As I think about this more carefully, I think we actually have a solid opportunity here.
I think quarto-markdown-pandoc should emit special Pandoc AST syntax that separate "footnote definitions" and "footnote references", which could then be handled correctly by Quarto's pipeline.

# TODO

- Link reference definitions

- Inlines

  - TODO I'm pretty sure hard code spans like `` foo` `` are broken.

  - `markdown+smart` extension

    - question: do we enable/disable this?

  - Process noteNum counts, or ignore it and knowingly produce output that doesn't quite match Pandoc?
  
    - precise matching of noteNum will be hard because that state is shared across a number of other nodes
      (though I don't understand _why_ pandoc does this.)

- blocks

  Missing from spec:

  - note definitions

  Missing from grammar:

  - LineBlock

  Missing from tree-sitter -> pandoc:

  - 
  
- Filter for note definition/note reference resolution

  - I actually think this should happen at quarto-cli

## Quirks

### Lists

The tree-sitter-qmd block parser interprets lists with different markers
as separate lists, while Pandoc interprets them as a single list.

In this, tree-sitter-qmd matches Commonmark instead of Pandoc `markdown`.
  
https://github.github.com/gfm/#lists

The Commonmark parser and GFM spec only allow singleparen ordered lists
and only allow decimal lists. We follow that as well here.

