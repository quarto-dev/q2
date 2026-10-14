---
title: 'lmd ("lua+md"): a JSX-like Lua + Markdown mode for Quarto 2 (bd-lmd-exploration-jqv2nbl4)'
date: 2026-10-12
description: 'Exploration of a syntax that puts filter logic next to the markdown it acts on: executable lmd cells whose Lua can contain markdown fragment expressions, desugared into pandoc constructor calls. Spike 1 vendors a tree-sitter-lua fork with the fragment syntax and shows the desugaring in rendered output.'
status: in-progress
braid:
  strand: bd-lmd-exploration-jqv2nbl4
---

**Branch:** `braid/bd-lmd-exploration-jqv2nbl4-lmd-exploration` (topic branch in the main checkout, based on `main` @ `a0975a7cc`; no worktree).

## The idea

Pandoc filters are a good foundation for AST-directed document processing, but the filter ends up far from the markdown that wants it. The Quarto 1 shape:

````qmd
---
filters: [foo.lua]
stuff: [a, b, c, d]
---

::: foo
:::
````

with a separate `foo.lua` that matches `Div`s by class, reaches back into metadata, and hopes nobody else uses `.foo`.

lmd puts the two in one place:

````qmd
---
stuff: [a, b, c, d]
---

::: foo

```{lmd}
return tmap(quarto.metadata.get("stuff"), function(v)
  return {=: **`{lmd} v`** :=}
end)
```

:::
````

Two pieces of syntax:

1. **The `{lmd}` cell.** An executable cell whose body is Lua. The eventual design desugars it into a Lua filter that dispatches the cell's result to the cell's own position in the document, so the author never names a class to match on. Inline form `` `{lmd} expr` ``.
2. **The markdown fragment expression.** Inside lmd Lua, a markdown fragment is an expression. The fragment is parsed with the qmd parser and becomes a Pandoc AST value, written out as `pandoc.*` constructor calls. Three forms: `{=: … :=}` for blocks, `{=[ … ]=}` for inlines, and the unmarked `{= … =}` whose context is meant to be decided by the caller (see the delimiter design notes). Markdown inside a fragment can contain lmd code again, so the languages nest in both directions.

The result should approach the ergonomics of Hugo, Astro and Docusaurus templates while plain Lua filter extensions keep working alongside, because the whole thing is desugaring: the output is ordinary qmd plus ordinary Lua filters.

## Spike 1 (done 2026-10-12): the grammar and a visible desugaring

The goal of the first spike is to **read** the desugaring, not run it. A pipeline stage takes each `{lmd}` cell, desugars its markdown fragments into `pandoc.*` calls, and leaves the result in the document as a non-executable ```` ```lmd ```` block so `q2 render --to markdown` shows it.

Spike 1 first shipped with a single `{= … =}` form that always produced blocks; the same day it was revised to the three forms decided under "Delimiter design notes" below. The description that follows is the revised state.

### What landed

- **`crates/tree-sitter-lmd`**: a vendored fork of [tree-sitter-grammars/tree-sitter-lua](https://github.com/tree-sitter-grammars/tree-sitter-lua) v0.5.0 (full Lua 5.4, plus the 5.5 `global` keyword), renamed to grammar `lmd` so it coexists with the upstream `tree-sitter-lua` crate `quarto-highlight` links. One rule added, `markdown_fragment`, hung off `expression`, in three forms:

  ```
  markdown_fragment: '{='  markdown_content? '=}'
                   | '{=:' markdown_content? ':=}'
                   | '{=[' markdown_content? ']=}'
  ```

  All nine tokens come from the external scanner; the Rust side tells the forms apart by the node kind of the `start` field. `{=` cannot begin any Lua construct (a table-constructor field never starts with `=`), so the openers are unambiguous wherever an expression is valid; on a lone `{` the scanner returns false and the internal lexer's `{` (table constructor) wins. The content token is raw text up to this form's closer, balancing nested fragments of any form so a fragment whose markdown has inline lmd code with its own fragment still ends in the right place. Content keeps its whitespace; the Rust side trims.

  Layout mirrors `crates/tree-sitter-doctemplate`: `grammar/` holds the upstream sources (only `grammar.js`, `src/`, `test/corpus/`, `queries/`, `tree-sitter.json`, the license), `build.rs` and `src/lib.rs` are ours. `cargo xtask ts-test lmd` regenerates and tests. The corpus has 53 tests, 42 upstream and 11 new; the Rust crate has 3.

- **`quarto_core::lmd`**: `desugar_lua(src) -> Result<String, LmdError>`. Parses the Lua with tree-sitter-lmd (a parse error fails the whole cell, with a position), finds every `markdown_fragment` in source order, parses each fragment's markdown with `pampa::readers::qmd::read`, and splices in the emitted expression: `pandoc.Blocks({ … })` for `{=: :=}`, `pandoc.Inlines({ … })` for `{=[ ]=}` (the markdown must parse to one paragraph or nothing, else `LmdError::NotInlines`), and `pandoc.Nodes({ … })` for the unmarked form. Continuation lines are indented past the column the opener sat at, so the splice reads as one expression inside whatever Lua surrounded it.

- **`pandoc.Nodes`** exists in pampa's Lua API as a stub that raises an error naming the two marked forms, so the desugared Lua is well-formed but an unmarked fragment cannot run until the context-deferred semantics are designed.

  The emitter (`lmd/emit.rs`) covers every `Block` and `Inline` that has a pandoc constructor, with pandoc's argument order (`Link(content, target, title, attr)`, `Header(level, content, attr)`, ...). An empty `Attr` is omitted. Unsupported nodes (`Table`, note definitions, `Shortcode`, the CriticMarkup inlines, `Custom`) fail the cell with `LmdError::Unsupported`.

  The escape hatches back to Lua: an inline `` `{lmd} expr` `` (a `Code` whose text starts with `{lmd}`) becomes `expr` in the enclosing inline list, parsed as a Lua *expression* (wrapped in `return` for the parse) and desugared recursively; a block `` ```{lmd} `` cell inside a fragment becomes `(function() body end)()` in the enclosing block list.

- **`LmdDesugarStage`** (`stage/stages/lmd_desugar.rs`), placed right after `PreEngineSugaringStage` and before `EngineExecutionStage` in `build_html_pipeline_stages_with_options`, so every pipeline derived from it (HTML, the pandoc leg, `--to markdown`) gets it and no engine sees an `{lmd}` cell. A cell that fails to desugar is left untouched and reported as a warning.

### What the output looks like

The motivating example renders to:

```lua
return tmap(quarto.metadata.get("stuff"), function(v)
  return pandoc.Blocks({
           pandoc.Para({
             pandoc.Strong({
               v,
             }),
           }),
         })
end)
```

The three forms side by side, from

````qmd
```{lmd}
local unmarked = {= **hello** =}
local blocks = {=: **hello** :=}
local inlines = {=[ **hello** ]=}
local strong = pandoc.Strong({=[ *inner* ]=})
```
````

render to:

```lua
local unmarked = pandoc.Nodes({
                   pandoc.Para({
                     pandoc.Strong({
                       pandoc.Str("hello"),
                     }),
                   }),
                 })
local blocks = pandoc.Blocks({
                 pandoc.Para({
                   pandoc.Strong({
                     pandoc.Str("hello"),
                   }),
                 }),
               })
local inlines = pandoc.Inlines({
                  pandoc.Strong({
                    pandoc.Str("hello"),
                  }),
                })
local strong = pandoc.Strong(pandoc.Inlines({
                               pandoc.Emph({
                                 pandoc.Str("inner"),
                               }),
                             }))
```

Nesting in both directions, from

`````qmd
````{lmd}
local function card(title, body)
  return {=:
::: {.card}
### `{lmd} title`

```{lmd}
return body
```
:::
:=}
end
return card({=[ Hello ]=}, {=: Some *body* text. :=})
````
`````

renders to:

```lua
local function card(title, body)
  return pandoc.Blocks({
           pandoc.Div({
             pandoc.Header(3, {
               title,
             }, pandoc.Attr("lmd-title", {}, {})),
             (function()
               return body
             end)(),
           }, pandoc.Attr("", {
             "card",
           }, {})),
         })
end
return card(pandoc.Inlines({
              pandoc.Str("Hello"),
            }), pandoc.Blocks({
                             pandoc.Para({
                               pandoc.Str("Some"),
                               pandoc.Space(),
                               pandoc.Emph({
                                 pandoc.Str("body"),
                               }),
                               pandoc.Space(),
                               pandoc.Str("text."),
                             }),
                           }))
```

Headings, bullet lists, links with titles, inline code, math, quotes, line breaks and attributes all come through (`scratchpad/lmd-demo.qmd` during the session; worth turning into a fixture in spike 2).

### Observations from reading it

1. **Context is now explicit.** `{=[ … ]=}` can sit inside `pandoc.Strong(...)` and `{=: … :=}` inside `pandoc.Div(...)`; the unmarked form waits on `pandoc.Nodes`. (The first cut of spike 1 forced block context; the delimiter design notes record how that was resolved.)
2. **Inline splices have no coercion contract.** `v` lands as a bare list element, so the Lua value must already be an `Inline` (or whatever pampa's `pandoc.Strong` accepts in a content list). A string works in pandoc proper by coercion to `Str`, but spaces would not become `Space`. Something like `lmd.inlines(v)` at the splice may be needed; `pandoc.Nodes` is the natural home for the same question.
3. **Spliced headings get a derived id.** The qmd reader auto-generates `lmd-title` from the heading text `` `{lmd} title` `` before the splice happens. Fixed in spike 5: fragments carry no derived ids and the `heading-id` transform derives them from the final text.
4. **Footnotes do not work yet.** The reader emits `NoteDefinitionPara` blocks that are resolved later in the pipeline, so a fragment with a footnote is `Unsupported`. Either resolve notes inside the fragment parse or run the fragment through the same early transforms the document gets.
5. **Fence nesting is on the author.** A cell containing a block-level `` ```{lmd} `` must itself use four backticks. Normal markdown, but it will surprise people.
6. **Inline lmd outside a fragment is untouched.** `` `{lmd} v` `` in a document paragraph (not inside a cell) is left as a `Code` node. Whether it should be an inline cell is a design question for the dispatch half.
7. **Continuation indent is computed from the source column.** When two fragments share a line and the first expands to several lines, the second is still indented to its *source* column (see the `card(...)` call above). Cosmetic; fixing it means tracking the output column during splicing.

## Spike 2 (done 2026-10-12): running the cells

The desugared Lua now runs. A second stage, `LmdExecuteStage`, sits after engine execution and right before `user-filters-pre`; it runs every cell `LmdDesugarStage` marked, in document order, in one Lua state per document, and splices the returned blocks in place of the cell.

### Decisions

Taken as proposed on 2026-10-12, with the user's explicit yes on the last two:

1. **A cell may return a `Block`, a list of `Block`s, or `nil`** (the cell vanishes). `Inlines`, a single `Inline`, and strings are errors that name what came back, not wrapped in a `Plain`: a cell is block context, and `{=: … :=}` already says "blocks". Multiple return values are ignored past the first.
2. **One Lua state per document.** Globals set by one cell are visible to later cells, like a notebook. Cells run in pre-order document order, so a cell inside a div runs before a top-level cell that follows the div.
3. **After the engine, before user pre-filters.** `LmdDesugarStage` (pre-engine) rewrites the cell in place, drops the `{lmd}` class for a plain `lmd` one, and sets `lmd="pending"`; an unbraced class is plain code to every engine, so knitr and jupyter never see a cell. `LmdExecuteStage` finds the marked cells.
4. **Metadata stays out** of spike 2; the spike runs in immediate mode. (Ported in spike 6, below.)
5. **Echo is a cell option**, `--| echo: true` in Lua comment syntax, off by default. It keeps the (option-stripped) desugared Lua as a plain `lmd` code block above the result.
6. **A failing cell is an error diagnostic, located at the cell**, and the cell stays in the document as that displayed block, so the rest of the page renders and the failing Lua can be read. Three failure kinds: the chunk does not compile or raises, or it returns the wrong kind of value (the hint names the three accepted shapes and `{=: … :=}`).
7. **Inline `{lmd}` code in ordinary paragraphs stays out of scope** for this spike. (Done in spike 3, below.)

### What landed

- **`pampa::lua::lmd`**: `LmdRuntime` wraps the same Lua environment a user filter gets (`create_filter_environment`: `pandoc.*`, `quarto.*`, `FORMAT`, scoped `require`), with the document path standing in for the script path. `run_cell(source, chunk_name)` loads the chunk as a block (`call_async`, like filters), applies the return contract (`LmdCellValue::{Nothing, Blocks}` / `LmdCellError::{Lua, NotBlocks}`), and `take_diagnostics` drains `quarto.warn` / `quarto.error` records. 6 tests.
- **`LmdExecuteStage`** (`stage/stages/lmd_execute.rs`): a pre-order walk numbers the marked cells, an async loop runs them (partitioning `--|` options first), and a post-order splice replaces each cell by its output list. Skips building a Lua state when the document has no cells. 7 tests, run through `execute_lmd_cells` on parsed documents with no stage context.
- **`LmdDesugarStage`** now marks instead of merely displaying.
- Stage lists: 29 stages on the HTML list; `lmd-execute` also survives into the pandoc leg.

### What the output looks like

The motivating example in immediate mode,

````qmd
::: foo

```{lmd}
local out = pandoc.Blocks({})
for _, v in ipairs({ "a", "b", "c", "d" }) do
  out:extend({=: **`{lmd} v`** :=})
end
return out
```

:::
````

renders to a div holding four paragraphs, `**a**` through `**d**`. A cell that sets `greeting = "Hello"` followed by

````qmd
```{lmd}
--| echo: true
return {=:
### `{lmd} greeting`, world

A paragraph with *emphasis*.
:=}
```
````

renders to the echoed Lua, then `### Hello, world {#lmd-greeting-world}` and the paragraph. A cell with no `return` leaves nothing behind. The two failure cases print, respectively,

```
Error: lmd cell did not return blocks
  … an lmd cell must return a Block, a list of Blocks, or nil; this one returned Inlines
  ℹ Return a Block, a list of Blocks, or nil. Write `{=: … :=}` for a markdown fragment that is blocks.
Error: lmd cell failed
  … runtime error: [string "lmd cell 6"]:1: boom
```

each underlining the cell in the source, and leave the desugared cell displayed.

### Observations

1. **String splices already work, by accident of pampa.** `greeting` is a Lua string spliced into a `Header` content list; pampa's fuzzy inline coercion turned it into `Str("Hello")`, and a string with spaces would become `Str`/`Space` runs. So observation 2 of spike 1 (no coercion contract for inline splices) is partly answered by the runtime: strings and `Inline`s work in inline lists. What is still undefined is a *list* value or a block in an inline list, and anything in block lists other than blocks.
2. **The derived heading id is now visible in output**: `{#lmd-greeting-world}` is the reader's auto id for the placeholder text. Spike 1's observation 3 stands and now has a user-visible effect.
3. **Lua error messages name the cell by number** (`lmd cell 6`), which is the cell's position among lmd cells, not a line in the document. The diagnostic's location underlines the right cell, so this is cosmetic, but a `file:line` chunk name would read better.
4. **`pandoc.Nodes` failed at run time** at the time of spike 2, as designed; spike 4 implemented it.

## Spike 3 (done 2026-10-12): inline cells in paragraphs

`` `{lmd} expr` `` anywhere inline (paragraphs, headings, list items, emphasis, captions) is now an inline cell: the desugar stage strips the tag, desugars the expression (so it may contain `{=[ … ]=}` fragments) and marks the `Code` node; the execute stage evaluates it in the same Lua state as the block cells and splices the inlines in.

### Contract

The mirror image of the block cell contract: the expression may evaluate to an `Inline`, a list of `Inline`s (a `pandoc.Inlines` or a plain table that may also hold strings), a string, or `nil` (the inline vanishes). A string is split the way pandoc's own coercion does it, into `Str` and `Space` runs. A `Block`, `Blocks`, a number, or a function is an error naming the value, with a hint pointing at `{=[ … ]=}`.

This settles spike 1's observation 2 for inline positions, and it is the same rule pampa's constructors already apply to spliced values inside fragments, so `pandoc.Strong(name)` and an inline-fragment markdown that holds `` `{lmd} name` `` agree.

### Ordering

Block and inline cells are numbered by one pre-order walk (`ast_walk::ListVisitor` over `block_children_mut` / `inline_children_mut`), so document order holds across kinds: an inline cell in the paragraph above a block cell runs first and sees `nil` for a global the block cell sets; the one below sees the value. The hand-rolled block-only walk from spike 2 is gone.

### Failures

Same policy as block cells: an error diagnostic located at the code span (the underline lands on the span, not the paragraph), and the span stays in the document as a plain `lmd` code span holding the desugared expression. There is no `echo` for inline cells; there is nowhere to put options.

### Mentions versus cells

This plan was the first casualty: its prose says `` `{lmd}` `` in several places, and a bare tag with no expression became an empty cell that failed to parse. The rule is now that `` `{lmd}` `` alone is a *mention* and is left as code, the same convention the docs rely on when they write `` `{python}` ``. To show the inline-cell syntax itself, write it as code containing backticks, `` `{lmd} expr` `` inside a double-backtick span: the code's text then starts with a backtick, not the tag. A `{lmd}` tag followed by an expression in prose *will* run, exactly as `` `{python} expr` `` does in a Quarto 1 document; there is no inline equivalent of wrapping a fence in a `markdown` block, which is a gap shared with the other engines.

Desugar failures are now located diagnostics (warning at the cell, with the cell left as written); they used to be bare messages, which is why the empty cell above took a search to find.

### What the output looks like

````qmd
```{lmd}
name = "world"
count = 3
```

## Hello, `{lmd} name`

There are `{lmd} tostring(count)` things, and the third is
`{lmd} {=[ *emphasised* ]=}`. A bold one: `{lmd} pandoc.Strong(name)`.
````

renders to `## Hello, world {#hello-lmd-name}` and "There are 3 things, and the third is *emphasised*. A bold one: **world**." The heading id is the reader's, derived from the placeholder text (spike 1, observation 3; still open).

### Verification

`cargo nextest run -p quarto-core -p pampa -E 'test(lmd)'`: 47 tests (18 desugaring, 12 execute-stage, 8 runtime, the rest grammar and stub).

## Spike 4 (done 2026-10-12): `pandoc.Nodes`

The unmarked `{= … =}` fragment now runs. `pandoc.Nodes` is a real value in pampa (`crates/pampa/src/lua/nodes.rs`): a container whose shape is fixed at construction, empty, all blocks, or all inlines, and whose meaning is decided by the context it lands in.

### Where it is resolved

The entry point is the fuzzy coercion layer in pampa's `types.rs`, `peek_inlines_fuzzy` and `peek_blocks_fuzzy` and their singleton forms, which every constructor's `content` argument (41 call sites), every filter-function return value (8), shortcodes, and `quarto.utils.as_inlines` / `as_blocks` already go through. So a `Nodes` works anywhere a filter author could already return a bare string. Three hooks: a `Nodes` passed as the whole content, a `Nodes` as an element of a content table (which splices, several nodes rather than one), and the singleton positions, where it must resolve to exactly one node. The lmd cell contracts resolve it the same way: a block cell is block context, an inline cell is inline context.

### The resolution table

| `Nodes` holds | inline context | block context |
|---|---|---|
| blocks (what the desugarer emits) | one `Para` or `Plain`: its inlines. Empty: nothing. Otherwise an error naming the block count, or the block kind if it is a single non-paragraph. | the blocks, unchanged: a `Para` stays a `Para` |
| inlines (hand-built, or a string) | the inlines | one `Plain`, pandoc's own convention for bare inlines |
| nothing | nothing | nothing |

The asymmetry was the one point decided against the first proposal (which was `Plain` in block context for every `Nodes`). A fragment that came from the markdown reader carries the reader's decision that its text is a paragraph; dropping it to `Plain` would make `pandoc.Div({= hello =})` render without a `<p>` while `pandoc.Div({=: hello :=})` renders with one, the kind of difference between the marked and unmarked forms the three-form syntax exists to avoid. A bare list of inlines carries no such information, so it gets pandoc's `Plain`.

### Construction

`pandoc.Nodes(x)` accepts a `Block`, an `Inline`, another `Nodes`, a string (word-split, like `pandoc.Inlines("…")`), or a table: a `pandoc.Blocks`, a `pandoc.Inlines`, or a plain list of blocks, inlines, strings (single `Str`s, element-wise, like every other list), and `Nodes` of a matching shape, which flatten. Mixed blocks and inlines are rejected: "pandoc.Nodes cannot hold blocks and inlines together". `pandoc.utils.type` says `Nodes`; `tostring` says `Nodes(2 blocks)`; `#` is the element count.

### Tests that pin the table

Each row and each hook is pinned, so the semantics cannot drift unnoticed:

- `pampa::lua::nodes::tests`, 12 tests, one behaviour class each: blocks shape in inline context (unwrap, and the two error forms), blocks shape in block context (Para preserved), inlines shape in both contexts, empty in both, element splicing in both, the singleton positions, construction rejects and accepts.
- `pampa::lua::filter::tests::test_handle_{inline,block}_return_nodes`: the same table on the filter-return path.
- `pampa::lua::lmd::tests::nodes_in_{a_block,an_inline}_cell_resolve_as_*_context`: the cell contracts.
- `quarto_core::stage::stages::lmd_execute::tests::unmarked_fragment_*` and `a_hand_built_nodes_*`: end to end from `{= … =}` syntax, including nesting through inline lmd code (`` {= a `{lmd} {= b =}` c =} `` gives "a b c").

### What changed for the lmd syntax

Nothing in the grammar or the desugarer. `{= **hello** =}` still emits `pandoc.Nodes({ pandoc.Para({ … }) })`; the stub and its "not implemented" error are gone. Spike 1's observation 2 (no coercion contract for splices) is now closed in full: strings, `Inline`s and `Nodes` all have defined meanings in both contexts.

One usability note from the demo: an inline lmd code span whose own fragment holds another code span must be written with a double-backtick delimiter, because single backticks pair up left to right and the inner span would close the outer one. Ordinary markdown, but the nesting lmd invites makes it easy to hit.

## Spike 5 (done 2026-10-12): heading ids follow the final text

### The problem

The qmd reader derives a heading's auto id in its postprocess pass from the text as written, de-duplicated within that parse. Two things rewrite heading text afterwards, and both left ids derived from placeholders:

- an inline lmd cell in a document heading: `` ## Hello, `{lmd} name` `` rendered as "Hello, world" with the anchor `hello-lmd-name`;
- a heading inside a fragment: parsed on its own, so a component like `card(title)` with `` ### `{lmd} title` `` gave every instance the same `lmd-title`, duplicated anchors, TOC links and crossref targets included.

Shortcodes had the same problem already, tracked as bd-2wv8431v: `## before {{{< meta foo >}}} after` got `before-after` because the unexpanded shortcode contributes no text, where Quarto 1 folds the expansion in.

### The fix

One transform, `heading-id`, registered immediately after `shortcode-resolve` (lmd cells ran earlier, in `lmd-execute`), before sectionize and the TOC. It re-derives the id of every heading whose id is **stale**: empty, or reader-derived but no longer what `auto_generated_id` gives for the current content. The work is pampa's existing `dedup_scoped_heading_ids` (Pandoc's `uniqueIdent` probe seeded with every id that must not be renamed), scoped by that predicate, so the reader keeps deriving provisional ids and nothing moved out of it. The qmd writer's round-trip rule, omit an id equal to `auto_generated_id(content)`, still holds, since that is what the pass writes.

Two kinds of id are never touched. Author-written `{#x}` carries a source location on `attr_source.id`. A heading a filter or cell built with `pandoc.Header(…, pandoc.Attr("x"))` has no such location, but its `source_info` is generated rather than original, which is how it is told apart; renaming those would break every Q1 filter that builds sections. A heading with an *empty* id is filled whatever its origin, since an unlinkable heading helps nobody (pandoc leaves filter-created headings id-less; this is one place Q2 does better).

On the lmd side the desugarer now strips reader-derived ids from fragment headings, so a fragment heading reaches the document with no id and the transform assigns one from its final text, unique document-wide. Explicit ids inside fragments are kept, duplicates and all: reusing a fragment with a pinned id is the author's call.

### Pins

- `transforms::heading_id::tests`: untouched when ids match; stale reader id re-derived; empty id filled whatever the origin; explicit id never renamed; filter-built heading keeps its given id; re-derived ids unique against explicit ones (`hello-1`, `hello-2`, `hello`); idempotent.
- `tests/integration/heading_id_after_expansion.rs`, through the real HTML pipeline: `before-hello-after` for the shortcode case; `hello-world` for an inline cell in a heading; `hello`, `bye`, `hello-1` for a fragment component used three times; a pinned `{#pinned}` inside a fragment survives.
- `lmd::desugar::tests::fragment_headings_carry_no_derived_id_but_keep_explicit_ones`.

### Noticed on the way

`{{{< meta foo >}}}` with `foo: 1.2` (a YAML number) expanded to nothing in the heading, so the fixture uses a string. Not investigated here.

## Spike 6 (done 2026-10-12): `quarto.metadata` and the `meta` proxy

The motivating example now runs as it was first written: metadata is reachable from a cell.

### `quarto.metadata.get(key)`

Quarto 1's API (quarto-cli PR 13246; the vendored tree has its type stub at `src/resources/lua-types/quarto/metadata.lua` and one caller in `email.lua`): the value at a dotted key, or `nil` when the key or any part of it is missing. Values come back in the shapes a filter's `Meta` handler already sees (`push_config_value`): strings are `Inlines` (a YAML string in document metadata is markdown, as pandoc's `MetaInlines`), numbers and booleans are themselves, arrays are `pandoc.List`s, maps are plain tables. q2 extends the path syntax with numeric segments that index arrays 1-based (`"authors.1.name"`), which the shortcode engine's `meta["author.1"]` fallback already did. A segment is read as a key when the node is a map, so `'2024': year` is reachable as `get("2024")`.

Registered in three environments: every Lua filter (a snapshot of the metadata as that filter starts), every shortcode handler call, and the lmd cell runtime. Implementation: `crates/pampa/src/lua/metadata.rs`.

### `meta`

The same lookup as a global object you navigate instead of spelling a key: `meta.a.b` is `get("a.b")`, `meta.authors[1].name` is `get("authors.1.name")`. The design question was arrays, and the answer is that maps and arrays are treated differently on purpose:

| value reached through `meta` | what you get | why |
|---|---|---|
| map | a lazy, read-only proxy (`LuaMetaProxy`) | navigation keeps going; `pairs`, `#`, `tostring` (`meta.author.affil`) work; a write is an error instead of silently going nowhere |
| array | a real `pandoc.List` of its converted items; map items are proxies again | arrays are *iterated*: `#`, `ipairs`, `table.concat` and `List:map` all want a real table, and no proxy can offer `List` methods |
| anything else | the leaf value, as `get` returns it | |

So `meta.stuff:map(function(v) … end)` works with no helper, which was the `tmap` in the original sketch. A missing key is `nil` at that point, so `meta.missing.inner` raises Lua's usual "attempt to index a nil value"; `get("missing.inner")` is the form for a chain that may be absent. `pandoc.utils.type(meta.x)` says `Meta` for proxies.

An "absent proxy" that never errors was considered and rejected: `if meta.missing then` would be truthy.

### The splice fix it forced

`meta.stuff` items are `Inlines`, and the desugarer splices an inline cell's value as a *list element*: `` **`{lmd} v`** `` becomes `pandoc.Strong({ v })`. Pandoc rejects a list inside a list, so that failed. The coercion layer now splices a tagged `pandoc.Inlines` element into the enclosing inline list, and a tagged `pandoc.Blocks` element into the enclosing block list (so `pandoc.Blocks({ card(1), card(2) })` from spike 5 also just works). The lmd cell contracts do the same, so `return meta.stuff:map(function(v) return {=: … :=} end)`, a `List` of `Blocks`, is accepted by a block cell. Untagged plain tables as elements still error. No valid pandoc program changes meaning; this is pinned next to the `Nodes` rows in `nodes.rs` and in `lmd.rs`.

### Pins

`pampa::lua::metadata::tests` (8): leaf shapes, dotted and numeric paths, every missing-part case is `nil`, lists and tables from `get`; `meta` navigation agrees with `get`, arrays are real lists with proxy items and `:map`, missing keys are `nil` and writes error, `pairs`/`#`/type. `lmd_execute::tests::the_motivating_example_against_document_metadata` runs the original example through `meta.stuff` and `quarto.metadata.get`.

## Source mapping assessment (2026-10-12)

Written before any code, as the design for the next spike. Two questions: can a Lua error name the document line it came from, and can a markdown syntax error inside a `{=: … :=}` fragment be underlined in the document. Both turn out to be mostly plumbing: the facilities exist, and lmd would be the first engine in q2 to map an execution error to a line.

### Where errors point today

| error | raised in | message carries | diagnostic location |
|---|---|---|---|
| Lua syntax error in a cell | desugar | line and column *within the cell text* | the whole cell |
| markdown parse error inside a fragment | desugar | the inner diagnostics\' titles, joined | the whole cell |
| unsupported node inside a fragment | desugar | the node kind | the whole cell |
| Lua compile or runtime error | execute | `[string "lmd cell 6"]:3:` plus a traceback with the same names | the whole cell |
| wrong return shape | execute | what came back | the whole cell |
| cell options not valid YAML | execute | the YAML error | located, but against the *block* span, so one fence line off (a bug; see L0) |
| `quarto.warn` / `quarto.error` from a cell | execute | the message | `By::filter("lmd cell 6", line)`: a chunk name, not a file |

"The whole cell" is honest but blunt: a ten-line cell with a typo on line 7 underlines all ten lines, and an error inside a fragment's markdown underlines the Lua that contains it. Inline cells fare better only because a code span is short.

### What already exists

- **`SourceInfo::substring` chains, and `read` reroots through them.** `pampa::readers::qmd::read(…, parent_source_info)` makes every node span of the nested parse a `Substring` of the parent, and `reroot_diagnostics_into_parent` does the same to the Err-path diagnostics. This is exactly how a markdown string inside front matter gets its errors underlined in the YAML today. A fragment is the same situation: markdown embedded in a span of the document. The user's half-hope is a yes.
- **Anchoring a cell's text in the document.** `crossref::codeblock_shorthand::body_source_for` finds a `CodeBlock`\'s text inside its own fence region and returns a `Substring` of the block span: byte-exact for a top-level fence, and an honest fallback to the block span when a blockquote or list gutter makes the text not a substring of the source. `partition_cell_options` then composes option-line provenance over whatever it is given. The reader records `Math.text_source` for the same problem on math; `CodeBlock` has no such field yet, which is the "substantial" version of the fix.
- **Lua chunk names.** mlua passes `set_name` through unchanged; a name of the form `@path` is formatted by Lua as `path:line:` in messages and tracebacks, and `filter_source_info` reads the same name and line back when a cell constructs AST nodes.

### Proposal, in three layers

**L0, anchor the cell text (small).** Lift `body_source_for` into a shared helper next to `partition_cell_options`, use it for block cells, and anchor inline cells by finding the text inside the code span. Fixes the one-fence-line offset in cell-option diagnostics as a side effect. Under a gutter the fallback is the block span, as today.

**L1, fragment markdown errors (small).** The desugarer takes the source of the text it is desugaring (`Option<SourceInfo>`) and passes `substring(body, fragment content range)` as the `parent_source_info` of each fragment parse, adjusting for the whitespace it trims. Markdown errors then underline the offending markdown inside the fragment. Nesting composes: a fragment's inline lmd code is a `Code` whose span is already a `Substring` of the fragment's parent, so the recursive desugar of that code passes it on, and a fragment inside it reroots one level deeper. Unsupported-node errors get the node's own span for free, since the node carries it.

**L2, Lua errors to document lines (medium).** Three pieces, no mapping table:

1. *Line-preserving emission.* A second emitter mode in which every fragment expansion occupies exactly the source lines the fragment occupied: a one-line fragment expands on one line, a multi-line fragment pads its expression with newlines (Lua allows them between tokens) so the lines after it keep their numbers. The pretty multi-line form stays for `echo`. Within a fragment, spliced inline cells are compressed onto the fragment's lines; an error inside one points at the fragment, which is the honest resolution available without a column map.
2. *Chunk name and padding.* Name the chunk `@<document path>` and prepend as many newlines as the cell body's start line minus one. Lua then reports `doc.qmd:42:` for an error on document line 42, in the message and in every traceback frame, and `filter_source_info` records `By::filter("doc.qmd", 42)` for nodes the cell builds, so lmd-built nodes become located for free. Only the body's *start line* is needed, which `resolve_byte_range` on the block span gives even where byte-exact anchoring fails, so line mapping survives gutters; columns shift by the gutter width there.
3. *A located diagnostic.* Parse `path:LINE:` out of the error and attach that line's span (a `Substring` of the body source) as the diagnostic location, so the underline lands on the line. The message keeps the full traceback.

Inline cells are one line; the padding is the paragraph's line and the `return ` prefix shifts columns by seven. The `--|` option lines are part of the body and count toward line numbers, which is what an author expects.

### Effort and order

L0 and L1 together are about a day and change two files plus tests; L2 is a few days, mostly the emitter mode and its tests. L0 should go first in any case since it fixes a wrong location that ships today. A snapshot fixture of the demo documents would pay for itself here: every layer changes what the diagnostics say.

### What this does not do

Column-exact positions under blockquote and list gutters, and positions inside a splice inside a fragment, wait for `CodeBlock.text_source` (the `Math.text_source` treatment) and a column map respectively; both are the same shape of work as `ProvenanceBuilder` and neither is needed for the error messages to be useful.

## Spike 7 (done 2026-10-12): source mapping, layers L0 to L2

All three layers of the assessment above landed, in the shape proposed, with two findings on the way.

### What a render shows now

For a document with each kind of failure, `q2 render` prints:

- a markdown error inside a `{=: … :=}` fragment as the reader's own diagnostic (here Q-2-10, the bare apostrophe), underlined at the offending byte *inside the fragment*, with a hint that it sits in an lmd fragment and the cell will not run;
- a Lua runtime error as `…/doc.qmd:9: boom` with a traceback whose frames carry the same name, underlined on line 9, not on the whole cell;
- the same for an inline cell, on the paragraph's line, and for a cell inside a blockquote, where the body is not byte-identical to the source but the line is still right.

### How

- **L0** (`lmd/anchor.rs`): an `Anchor` is the span of a cell's text plus whether it is byte-exact. Block cells use `body_source_for` (lifted to `pub(crate)` from the crossref shorthand), inline cells find their expression in the code span's bytes. An approximate anchor still knows the text's first document line, since a fenced block's body starts one line below its span. The cell-option diagnostic that used to be one fence line off is now on the option line.
- **L1**: the desugarer takes an anchor and parses each fragment's markdown with `parent_source_info` set to the trimmed content's span, so the reader reroots node spans and Err-path diagnostics into the document (the front-matter markdown path). `LmdError::MarkdownParse` now carries those diagnostics and the stage reports them as they are; `Unsupported` carries the node's span; a Lua syntax error is located on its line. Nested splices pass the code span down as an approximate anchor.
- **L2**: a second emitter mode, line-preserving, keeps lists on one line and pads a multi-line fragment's expansion with newlines after its opening parenthesis, so every expansion occupies exactly the lines the fragment did; the lines inside a spliced block cell are the fragment's own lines and count toward the budget. The execute stage runs each cell under the chunk name `@<path>` with the body's start line (plus any option lines) of newline padding, so Lua's `path:line:` is the document position; the diagnostic location is that line's span.

### Findings

1. **Lua abbreviates long chunk names.** Anything over `LUA_IDSIZE` (60 bytes) prints as `...` plus a tail, so the message says `...scratchpad/doc.qmd:9:` for an absolute path. The locator therefore matches on the file name, not the full path. A path relative to the project directory would read better and rarely be truncated; left for later since the file name is enough to locate.
2. **The echoed form is the line-preserving one.** The assessment said the pretty form would stay for `echo`. Keeping both would mean storing the original cell text somewhere that survives the engine round trip, and a multi-line attribute value does not; so `--| echo: true` shows the single-line-per-fragment form. `desugar_lua` (pretty) remains for the plan's samples and tests.

Both include cases from the review hold and are pinned end to end in `tests/integration/lmd_error_locations.rs`: a cell inside an included file names the included file and its line (include expansion registers the file under its own id), and a cell after a plain include keeps its main-file line. The engine round trip keeps a desugared cell's span because reconcile keeps unchanged nodes; a notebook converted by the source-conversion stage has spans in the converted text, so its lines refer to that text, which is the one known limit.

### Pins

`lmd::anchor::tests` (4), `lmd::desugar::tests` line-preserving (3) and unanchored-error (2), `lmd_desugar::tests` located failures (3), `lmd_execute::tests` source mapping (5: runtime line, lines across a fragment and option lines, inline line, blockquote line, option-line location), `pampa::lua::lmd::tests` chunk name and padding (1), and the two include cases.

## Snapshot suite (2026-10-12)

`crates/quarto/tests/integration/lmd_snapshots.rs` drives the real binary over the four fixtures in `tests/integration/fixtures/lmd/` with `--to markdown` and pins, per fixture, the markdown the qmd writer produced and the diagnostics on stderr (ANSI stripped, the scratch directory and Lua's abbreviated chunk tails normalized to `<dir>`). Snapshots live in `tests/integration/snapshots/lmd/`; update with `INSTA_UPDATE=always cargo nextest run -p quarto -E 'test(lmd_snapshots)'` and review the diff.

| fixture | pins |
|---|---|
| `fragments.qmd` | the three forms, echoed in the line-preserving form; nesting in both directions; headings, lists, links, code, math, quotes, line breaks, attributes |
| `cells.qmd` | the motivating example as first written (`meta.stuff:map`); shared state across cells; inline cells reading `meta` and `quarto.metadata.get`; a vanishing cell; echo with a padded multi-line fragment |
| `nodes-and-ids.qmd` | every `pandoc.Nodes` row; a reused fragment component getting `hello`, `bye`, `hello-1`; a pinned id surviving; a document heading with an inline cell getting `hello-world` |
| `errors.qmd` | one of each failure: Lua syntax error on its line, markdown error at its byte inside a fragment, unsupported node at the node, runtime error with `<dir>/errors.qmd:32:` and a traceback, wrong return shape, inline runtime error and wrong shape on their code spans, a blockquoted cell on its line, invalid cell options on the option line; every failing cell left displayed |

The stderr snapshots are the guard for source mapping: a regression in line padding, chunk naming or rerooting changes a line number or an underline, and the diff says where.

CI caught one thing the suite's first run missed locally: a cell that fails to *desugar* used to be left entirely as written, braced `{lmd}` class included, so the engine stage saw it as an unclaimed engine cell and fell back to jupyter; on a machine with no engine runtime (CI) that fails the whole render before any output. Locally jupyter is installed, which hid it. A failed cell now keeps its text but loses the braced class (an inline cell keeps its expression and gets the tag as a class), the same shape an execute-time failure already had.

### Follow-ups filed

- bd-f6tt2exg: `CodeBlock.text_source` for exact anchors under blockquote and list gutters (the `Math.text_source` treatment).
- bd-o7ujtnk3: project-relative chunk names, so Lua's 60-byte `LUA_IDSIZE` stops abbreviating paths in messages.

## Delimiter design notes (2026-10-12)

Spike 1 uses one fragment form, `{= … =}`, and always produces blocks. The next step wants two forms, so the block-versus-inline context is explicit in the syntax, matching the pandoc habit that `[...]` means inline (spans, links) and `:::` means block (divs, the block-footnote extension). This section records what the Lua side allows, so the choice can be made later without re-deriving it.

### What can follow `{` in Lua

A fragment opener has to be a two-character sequence that no valid Lua program contains at an expression start, because the external scanner is consulted first wherever an expression can begin. After `{` (a table constructor) Lua accepts a field start: a name or `_`, a digit, a string quote, `[` (bracketed key or long string), `{`, `(`, `-`, `#`, `~`, `.` (for `...`), `}`, or a `--` comment. Every other ASCII punctuation character is free as the second character: `=` `:` `|` `!` `@` `$` `?` `<` `>` `/` `*` `^` `&` `%` `+` `,` `;` `` ` `` `\`.

So `{=` (current), `{:`, `{|`, `{!`, `{$`, and so on are all unambiguous openers. Once the first two characters are free, the third is unconstrained: `{|:` and `{|[` are as safe as `{|`.

### Why `{[ … ]}` does not work

`{[` is valid Lua twice over: a bracketed key, `{[k] = v}`, and a long string in a table, `{[[text]]}`. The scanner cannot rescue it with lookahead either, because ordinary code contains the closer: `{[k] = t[1]}` ends in `]}` and would lex as a fragment. Keep this example in mind whenever a `[`-led opener comes up again.

### `[: … :]` is available but reads wrong

An expression can begin with `[` only as a long string (`[[` or `[=`), and the index form `a[...]` is only valid after a prefix expression, where no expression can start. So `[:` never competes with existing Lua for the same lookahead. It was rejected on reading grounds: it combines the span character `[` with the div character `:`, so it has to be taught as *not* div-related.

### Closer collisions, measured

Counted over the 2139 `.qmd` and `.md` files under `claude-notes/` and `docs/`, counting lines containing the sequence anywhere (every hit is inside code or YAML, not prose; a fragment containing such a code block would collide with any delimiter equally):

| sequence | files | lines |
|---|---|---|
| `]}` | 78 | 164 |
| `:}` | 13 | 26 |
| `*}` | 5 | 7 |
| `:]` | 5 | 7 |
| `|}` | 1 | 1 |
| `:|}` `]|}` `:=}` `]=}` | 0 | 0 |

`]}` is the worst closer on the markdown side as well as the Lua side.

### Candidates on the table

| blocks | inlines | expands to | notes |
|---|---|---|---|
| `{: … :}` | `[: … :]` | `pandoc.Blocks`, `pandoc.Inlines` | shortest; `[:` has the teaching problem above |
| `{|: … :|}` | `{|[ … ]|}` | same | a discriminator `|` then the context character; zero corpus collisions; more typing; OCaml quoted strings `{| … |}` and `{id| … |id}` are a precedent for the shape |
| `{=: … :=}` | `{=[ … ]=}` | same | same idea with `=`, keeping continuity with spike 1; zero corpus collisions |

Whichever pair is chosen, nested pairs balance in the scanner the way `{=`/`=}` do today, and `Para` versus `Plain` stops being a syntax question: the block form yields whatever the qmd reader produces (a top-level paragraph is `Para`), and an explicit `pandoc.Plain(…)` covers the rest. The inline form should reject markdown that parses to more than one block, since the author asked for inlines.

### Decision (2026-10-12): `{=`, `{=:`, `{=[`

The third row, with one addition: the unmarked `{= … =}` stays, as a third form. Adding `:` or `[` to the opener adds exactly the specificity it names.

| form | expands to | status |
|---|---|---|
| `{= … =}` | `pandoc.Nodes({ blocks })` | works since spike 4 (`pandoc.Nodes` resolves by context) |
| `{=: … :=}` | `pandoc.Blocks({ blocks })` | works |
| `{=[ … ]=}` | `pandoc.Inlines({ inlines })` | works; the markdown must parse to one paragraph or nothing |

`pandoc.Nodes` is the interesting one: a value that defers the blocks-versus-inlines decision to the surrounding context, so `pandoc.Para({= text =})` and `pandoc.Div({= text =})` could both work. Its semantics are not designed yet, so the desugaring emits the call and pampa's entry raises an error naming the two marked forms. The emitter hands it the reader's blocks, which carry enough to become either (a lone `Para` unwraps to inlines).

Scanner rules, so they are not re-derived: every `{=` opens a nested fragment whatever follows it, every `=}` closes one when nested, and at depth zero only the current kind's closer (`=}`, `:=}`, `]=}`) ends the content. A bare `=}` at depth zero inside a marked fragment is content. The character after `{=` decides the kind, so `{=[` opens an inline fragment while `{= [link](u)` (with a space) opens an unmarked one.

## Open questions

1. ~~The semantics of `pandoc.Nodes`~~ Decided and built in spike 4.
2. The splice coercion contract for inline and block lmd code inside a fragment (observation 2).
3. The dispatch half: how an `{lmd}` cell's result replaces the cell at its own AST position, and how the desugared Lua becomes a filter that existing Lua filters compose with. Also whether the cell runs at render time only or has a preview-time story.
4. Error locations. `LmdError` carries a position inside the cell text; the warning does not yet map it back to the document.
5. The name.

## Next spikes

- Filed as follow-up strands: bd-o7ujtnk3 (project-relative chunk names) and bd-f6tt2exg (`CodeBlock.text_source`). The snapshot suite is in.
- Turn the demo documents into integration fixtures with snapshots of the `--to markdown` output.
- The dispatch half proper: whether a cell's Lua should become a filter that other Lua filters compose with, rather than a chunk that runs once at its position.

## Verification (spike 1)

- `cargo xtask ts-test lmd`: 53 of 53 corpus tests.
- `cargo nextest run -p quarto-core -p pampa -E 'test(lmd)'`: 18 desugaring tests, 7 execute-stage tests, 6 runtime tests, plus the `pandoc.Nodes` stub test.
- `cargo test -p xtask tree_sitter`, `cargo clippy -p quarto-core -p tree-sitter-lmd -p xtask --all-targets -- -D warnings`, `cargo xtask lint`: green.
- WASM: not built locally in this session; the crate follows `tree-sitter-doctemplate`\'s build exactly, and `quarto-core` already pulls two other C-backed grammars into the wasm32 tree. CI's build-wasm job is the check.
