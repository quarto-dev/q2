# Quarto Markdown Syntax

## Goals

We aim to be largely compatible with Pandoc's `markdown` and `Commonmark` formats.

## Syntax extensions

Syntax extensions are handled by [desugaring](https://cs.brown.edu/courses/cs173/2012/book/Desugaring_as_a_Language_Feature.html) into regular Pandoc AST nodes.

### Scoped metadata

Our intermediate representation can store a metadata block inside the document, allowing (in principle)
for metadata in the document to be scoped to a particular portion of the document.

### Shortcodes

We have "native" shortcode support in the "Pandoc" AST in pandoc.rs, and
we desugar them to Pandoc spans in a Rust filter.

### Footnotes

We parse footnotes differently from Pandoc.
We use NoteReference (Inline), NoteDefinitionPara (single paragraph), and NoteDefinitionBlock (multiple paragraph) nodes.
These are desugared into spans and divs in a Rust filter.

### Editor markup

Inspired by [CriticMarkup](https://fletcher.github.io/MultiMarkdown-6/syntax/critic.html) and [djot](https://djot.net), Quarto offers syntax for edit marks:

- Insertions: `[++ Insert this markdown content]`
- Deletions: `[-- Delete this sentence]`
- Highlighting: `[!! this will be highlighted in rendering]`
- Comment: `[>> this will show up as a comment]`

### Reader raw blocks

Quarto Markdown supports the following syntax:

````
```{<pandoc}
| This will become a line block
| Line blocks are not supported by Quarto Markdown but
| can be supported via this fallback syntax
```
````

Reader raw blocks of the form `{<READER}` desugared into regular raw blocks of the form `{=pandoc-reader:READER}`.
This syntax effectively bypasses Quarto Markdown's syntax, and provides authors with an escape hatch into specific features in Pandoc readers.

## Pandoc syntax quirks

### Cites

Pandoc uses backtracking for its complex cite nodes, and puts strange content into the Cite node. 

Consider this:

```
$ echo '[prefix @c1 suffix; @c2; @c3]' | pandoc -t native
[ Para
    [ Cite
        [ Citation
            { citationId = "c1"
            , citationPrefix = [ Str "prefix" ]
            , citationSuffix = [ Space , Str "suffix" ]
            , citationMode = NormalCitation
            , citationNoteNum = 1
            , citationHash = 0
            }
        , Citation
            { citationId = "c2"
            , citationPrefix = []
            , citationSuffix = []
            , citationMode = NormalCitation
            , citationNoteNum = 1
            , citationHash = 0
            }
        , Citation
            { citationId = "c3"
            , citationPrefix = []
            , citationSuffix = []
            , citationMode = NormalCitation
            , citationNoteNum = 1
            , citationHash = 0
            }
        ]
        [ Str "[prefix"
        , Space
        , Str "@c1"
        , Space
        , Str "suffix;"
        , Space
        , Str "@c2;"
        , Space
        , Str "@c3]"
        ]
    ]
]
```

The content array has Str "[prefix" and Str "@c2;", but the citation entries correctly remove the semicolon and brackets.

Currently, we emit empty content for the Cite node.
The citation entries themselves are handled.

Pandoc "uses some heuristics to separate the locator from the rest of the subject".
Empirically, what this means is that one of the Str nodes inside the suffix has the entirety of (eg) "pp. 33".
We don't support that yet.

### Superscript

Superscript in `-f markdown` behaves sort of magically, and I think it involves backtracking. Consider:

```
$ echo 'a^a*a^a^a*a^a' | pandoc -t native
[ Para
    [ Str "a"
    , Superscript
        [ Str "a"
        , Emph [ Str "a" , Superscript [ Str "a" ] , Str "a" ]
        , Str "a"
        ]
    , Str "a"
    ]
]
```

How does it know to match the carets in the way it does? `-f commonmark+superscript` doesn't support this:

```
$ echo 'a^a*a^a^a*a^a' | pandoc -t native -f commonmark+superscript
[ Para
    [ Str "a"
    , Superscript [ Str "a*a" ]
    , Str "a"
    , Superscript [ Str "a*a" ]
    , Str "a"
    ]
]
```

This inconsistency gives me moral space for our parser to be inconsistent here as well.

### Line Blocks

tl;dr: Quarto Markdown will not support Pandoc LineBlock parsing.

Pandoc supports ["line blocks"](https://pandoc.org/demo/example33/8.6-line-blocks.html), syntax like this:

```
| The limerick packs laughs anatomical
| In space that is quite economical.
|    But the good ones I've seen
|    So seldom are clean
| And the clean ones so seldom are comical
```

The AST type is `LineBlock [[Inline]]` (each line in the line block is a list of `Inline`).
Unfortunately, this syntax interacts very badly with pipe tables under any fixed lookahead parsing strategy.
Consider:

- ```
  | This is a line block
  | No problem, right?
  ```

  This is a line block.

- ```
  | This is still a line block |
  | -
  ```

  This is a line block.

- ```
  | Oh, oh no |
  | - |
  ```
  
  This is a table.

Quarto Markdown is designed to be efficiently parseable (via `tree-sitter` grammars).
`tree-sitter` is (mostly) a LALR(1) parser, which means it needs to decide rules based on 1-token lookahead.
We don't see how to do distinguish pipe tables and line blocks with fixed lookahead.
We also don't see line blocks commonly used in the wild (they don't exist in CommonMark, for example).

### Definition lists

tl;dr: Quarto Markdown will not support Pandoc DefinitionList parsing.

Definition lists offer the same problem.
There's no way to know that the following construct isn't a paragraph followed by something else without parsing the entire paragraph first:

- ```
  A term
  
  :    a definition
  ```

  This is a definition list.

- ```
  A term
  ```

  This is a paragraph.

We will also not support definition lists directly.

### Superscript + note vs span ambiguity

Consider `^[footnote-or-span]{.class}^`. `^[` denotes both the start of a footnote and potentially the combination of a superscript block with a span; this parse is ambiguous.

Quarto-markdown's parser prefers the footnote interpretation. In case an immediately nested span is needed, use a space between `^` and `[`.
Superscript nodes with leading spaces are disallowed in Pandoc, but Quarto-markdown will trim spaces.

## Differences from Pandoc

### Single link syntax

Links always need to be defined fully. `[LaTeX Output]` doesn't work; `[LaTeX Output](#latex-output)` does.

We have no support for wikilink syntax.

We have no shortcut reference link support: that syntax is used for spans.

Similarly, the only image syntax supported is the one corresponding to inline links: `![text](image-name title)`

### Quoting differences

`''` and `""` are parsed as empty `Quoted` objects by `quarto-markdown`.

### No naked HTML support

In Pandoc, you can intersperse HTML and Markdown, and Pandoc will (attempt to) parse the HTML into
its AST format. This is brittle and inefficient because it relies heavily on backtracking through
parser combinators. In `quarto-markdown`, use the raw block and inline syntax directly.

Naked HTML is nonetheless *accepted*, with a `Q-2-9` warning, and the reader
tries to give it the same shape Pandoc would. Two deliberate gaps remain.

**No `native_divs` / `native_spans`.** Pandoc promotes a balanced
`<div>…</div>` to a `Div` node, which wraps its contents in `<p>`. Finding the
matching close tag is the backtracking this section rejects, so we do not do
it: the tags stay `RawBlock`s and the contents are not wrapped. The visible
difference is a missing `<p>`.

**`Plain` where Pandoc sometimes writes `Para`.** Markdown *is* parsed inside a
naked HTML block — Pandoc's `markdown_in_html_blocks`, which is a separate
extension from `native_divs` and which we do implement. Pandoc chooses between
`Plain` and `Para` for that content by tracking which element is still open;
lacking that, we always emit `Plain`. The visible difference is again only a
`<p>`, and only for text that trails a raw tag rather than sitting between a
pair. Analysis:
`claude-notes/research/2026-09-03-block-html-adjacent-markdown-unparsed.md`.

Content inside `<pre>`, `<script>`, `<style>` and `<textarea>` is *not* parsed
as markdown — they are CommonMark's raw-text elements, and Pandoc agrees. The
exemption covers one paragraph, because that is the extent of the lift: if such
an element's content contains a blank line, only the part before it is kept
verbatim and the rest is read as markdown.

**Writing the AST back out does not reproduce the `Plain`.** The writer always
separates blocks with a blank line, so a lifted interior read back as `Plain`
comes out as a `Paragraph` on the next read — gaining a `<p>`. This is
canonicalization, like the table and definition-list rewrites, not a lossy
step: the tags stay in block position and the markdown stays parsed, and the
shape is stable from the first cycle onward.

Preserving the `Plain` would mean writing it *tight* against the neighbouring
tag, which requires knowing the two blocks came from one paragraph. The AST
does not record that, and a rule that inferred it from block types alone was
wrong in both directions: it merged a `Plain` with a following *unrelated*
`<script>`, so the script's contents were parsed as markdown, and it let a
closing tag fall back inside a paragraph — the `<p><div>` shape the lift
exists to prevent. The information needed is provenance the AST cannot carry,
so the writer does not guess.

One consequence is worth knowing: for an element whose content model is
phrasing — `<summary>`, `<td>` — the added `<p>` is not valid there. The first
render is correct; only a write/read cycle introduces it. Naked HTML is an
unsupported authoring form that already warns (`Q-2-9`) and points at
`::: {.class}` or a `{=html}` fence, which have neither problem.


