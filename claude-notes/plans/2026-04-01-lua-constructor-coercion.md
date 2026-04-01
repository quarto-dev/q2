# Plan: Pandoc Lua Constructor Type Coercion

## Status: Complete

---

## Overview

Pandoc's Lua API performs automatic type coercion ("fuzzy peeking") when
constructors receive arguments. q2's constructors are strict — they only
accept tables of the exact userdata type. This causes real-world extensions
(e.g., lipsum) to fail because they rely on coercion behaviors like
`pandoc.Para("text")` or `pandoc.Para(pandoc.Str("x"))`.

This plan brings q2's coercion in line with real Pandoc's `pandoc-lua-marshal`
package, specifically the `peekInlinesFuzzy`, `peekInlineFuzzy`,
`peekBlocksFuzzy`, and `peekBlockFuzzy` functions.

## Codebase Context

### Where coercion happens in real Pandoc

Source: `pandoc-lua-marshal` Haskell package
(`~/src/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/`).

**`peekInlinesFuzzy`** (Inline.hs:138-147) — dispatches on Lua type:
1. `TypeString` → word-split via `B.text` into `Str`/`Space`/`SoftBreak` list
2. `TypeTable` → try `__toinline` metamethod (→ singleton), else `peekList peekInlineFuzzy`
3. `TypeUserdata` → singleton via `peekInlineFuzzy`
4. Otherwise → error

**`peekInlineFuzzy`** (Inline.hs:127-134) — dispatches on Lua type:
1. `TypeString` → `Str(text)` (NO word splitting)
2. `TypeTable` → try `__toinline` metamethod, else `peekInline`
3. `TypeUserdata` → `peekInline` or `__toinline` metamethod
4. Otherwise → error

**`peekBlocksFuzzy`** (Block.hs:145-153) — tries in order:
1. `__toblock` metamethod → singleton list
2. `peekList peekBlockFuzzy` (each element via `peekBlockFuzzy`)
3. Single `peekBlockFuzzy` → singleton list
4. Otherwise → error

**`peekBlockFuzzy`** (Block.hs:133-141) — tries in order:
1. `peekBlock` (exact Block userdata)
2. `__toblock` metamethod
3. `Plain <$!> peekInlinesFuzzy` (any inlines-like value → wrap in Plain)
4. Otherwise → error

**`B.text`** (pandoc-types Builder.hs:334-350) — word-splitting:
- Groups consecutive characters by space/non-space category
- Space chars: ` `, `\r`, `\n`, `\t`
- Non-space runs → `Str`
- Space-only runs → `Space`, unless the run contains `\n` or `\r` → `SoftBreak`
- Multiple consecutive spaces collapse to a single `Space`/`SoftBreak`
- Empty string → empty list

### Where coercion happens in q2

- `crates/pampa/src/lua/types.rs` — `lua_table_to_inlines()` (line ~1343)
  and `lua_table_to_blocks()` (line ~1367). Both only accept `Value::Table`
  containing the exact userdata type.
- `crates/pampa/src/lua/constructors.rs` — all constructors call one of
  these two functions. The `pandoc.Inlines()` and `pandoc.Blocks()`
  constructors have their own coercion logic that is partially correct.

### Which constructors use which peek functions in real Pandoc

Every constructor in Pandoc that takes inlines or blocks uses the fuzzy
variants — no exceptions. Full mapping from `pandoc-lua-marshal`:

| Constructor | Parameter | Pandoc peek function |
|---|---|---|
| Para, Plain | content | `peekInlinesFuzzy` |
| Header | content | `peekInlinesFuzzy` |
| Emph, Strong, Underline, Strikeout, Superscript, Subscript, SmallCaps | content | `peekInlinesFuzzy` |
| Quoted | content | `peekInlinesFuzzy` |
| Link, Image | content | `peekInlinesFuzzy` |
| Span | content | `peekInlinesFuzzy` |
| Cite | content | `peekInlinesFuzzy` |
| Note, BlockQuote, Div | content | `peekBlocksFuzzy` |
| Figure | content | `peekBlocksFuzzy` |
| BulletList, OrderedList | items | `peekItemsFuzzy` = `peekList peekBlocksFuzzy \|\| singleton peekBlocksFuzzy` |
| DefinitionList | items | `peekList peekDefinitionItem` where term = `peekInlinesFuzzy`, defs = `peekList peekBlocksFuzzy \|\| singleton peekBlocksFuzzy` |
| LineBlock | lines | `peekList peekInlinesFuzzy` |
| Caption | long | `peekBlocksFuzzy` |
| Caption | short | `peekInlinesFuzzy` |
| Caption (fuzzy peek) | fallback | tries Caption, then table, then `peekBlocksFuzzy` |
| Citation | prefix | `peekInlinesFuzzy` |
| Citation | suffix | `peekInlinesFuzzy` |
| pandoc.Inlines | content | `peekInlinesFuzzy` (delegates entirely) |
| pandoc.Blocks | content | `peekBlocksFuzzy` (delegates entirely) |

## Current q2 behavior vs expected

### Inlines constructors (Para, Emph, Strong, etc.)

| Input | Real Pandoc | q2 now | Gap |
|---|---|---|---|
| `{pandoc.Str("x"), pandoc.Space()}` | works | works | — |
| `pandoc.Str("x")` (single userdata) | `{Str("x")}` | **error** | fix |
| `"hello"` (string) | `{Str("hello")}` | **error** | fix |
| `"hello world"` (multi-word string) | `{Str("hello"), Space, Str("world")}` | **error** | fix |
| `{"hello", pandoc.Space(), "world"}` (mixed) | `{Str("hello"), Space, Str("world")}` | **error** | fix |

### Blocks constructors (Div, BlockQuote, Figure, Note)

| Input | Real Pandoc | q2 now | Gap |
|---|---|---|---|
| `{pandoc.Para(...)}` (table of blocks) | works | works | — |
| `pandoc.Para(...)` (single userdata) | `{Para(...)}` | **error** | fix |
| `"text"` (string) | `{Plain({Str("text")})}` | **error** | fix |
| `{pandoc.Str("x")}` (inlines-like) | `{Plain({Str("x")})}` | **error** | fix |
| `{pandoc.Str("x"), pandoc.Str("y")}` (multiple inlines) | `{Plain({Str("x")}), Plain({Str("y")})}` | **error** | fix |

Note: A table of inlines passed to a blocks constructor produces **one Plain
block per element**, NOT one Plain wrapping all inlines. This is because
`peekBlockFuzzy` is applied per-element, and each inline individually becomes
`Plain([that_inline])`.

### `pandoc.Inlines()` constructor

| Input | Real Pandoc | q2 now | Gap |
|---|---|---|---|
| `"hello world"` | `{Str("hello"), Space, Str("world")}` | `{Str("hello world")}` | fix |
| `{"hello", pandoc.Str("!")}` (mixed) | `{Str("hello"), Str("!")}` | `{Str("hello"), Str("!")}` | — |
| Single Inline userdata | wraps in list | wraps in list | — |
| `nil` | empty list | empty list | — |

### `pandoc.Blocks()` constructor

| Input | Real Pandoc | q2 now | Gap |
|---|---|---|---|
| Single Block userdata | wraps in list | wraps in list | — |
| `nil` | empty list | empty list | — |
| String | `{Plain(word-split inlines)}` | **error** | fix |
| Inlines-like | `{Plain(inlines)}` | **error** | fix |

### Helper constructors

| Constructor | Parameter | Real Pandoc | q2 now | Gap |
|---|---|---|---|---|
| BulletList | each item | `peekBlocksFuzzy` (string → `[Plain(word-split)]`) | strict blocks only | fix |
| BulletList | items arg | list of items OR single item → singleton | list only | fix |
| OrderedList | each item | same as BulletList | strict blocks only | fix |
| DefinitionList | term | `peekInlinesFuzzy` (string → word-split) | strict inlines only | fix |
| DefinitionList | definitions | `peekList peekBlocksFuzzy` or single → singleton | strict blocks only | fix |
| LineBlock | each line | `peekInlinesFuzzy` (string → word-split) | strict inlines only | fix |
| Caption | long | `peekBlocksFuzzy` | strict blocks only | fix |
| Caption | short | `peekInlinesFuzzy` | strict inlines only | fix |
| Citation | prefix | `peekInlinesFuzzy` | strict inlines only | fix |
| Citation | suffix | `peekInlinesFuzzy` | strict inlines only | fix |

---

## Work Items

### Phase 1: Core coercion functions (types.rs)

- [x] **1.1** Add `split_string_to_inlines(s: &str) -> Vec<Inline>` utility
  that splits a string on whitespace, producing `Str`/`Space`/`SoftBreak`
  elements matching Pandoc's `B.text` behavior:
  - Group consecutive characters by space vs non-space
  - Space characters: ` `, `\r`, `\n`, `\t`
  - Non-space runs → `Str(text)`
  - Space-only runs → `SoftBreak` if run contains `\n` or `\r`, else `Space`
  - Multiple consecutive spaces collapse into a single `Space`/`SoftBreak`
  - Empty string → empty vec

- [x] **1.2** Rewrite `lua_table_to_inlines()` as `peek_inlines_fuzzy()`:
  Accept (in this priority order, matching Pandoc's type dispatch):
  1. `Value::String` — word-split via `split_string_to_inlines()`
  2. `Value::Table` — iterate sequence values, each via `peek_inline_fuzzy()`
  3. `Value::UserData` containing `LuaInline` — wrap in singleton vec
  4. Otherwise → error

  Add helper `peek_inline_fuzzy(val: Value) -> Result<Inline>`:
  1. `Value::String` — wrap in single `Str` (NO word splitting)
  2. `Value::UserData` containing `LuaInline` — extract
  3. Otherwise → error

- [x] **1.3** Rewrite `lua_table_to_blocks()` as `peek_blocks_fuzzy()`:
  Accept (in this priority order, matching Pandoc):
  1. `Value::Table` — iterate sequence values, each via `peek_block_fuzzy()`
  2. `Value::UserData` containing `LuaBlock` — wrap in singleton vec
  3. Any value that `peek_inlines_fuzzy()` accepts — wrap in
     `Plain(inlines)` as singleton vec
  4. Otherwise → error

  Add helper `peek_block_fuzzy(val: Value) -> Result<Block>`:
  1. `Value::UserData` containing `LuaBlock` — extract
  2. Any value that `peek_inlines_fuzzy()` accepts — wrap in `Plain(inlines)`
  3. Otherwise → error

- [x] **1.4** Write tests for each coercion path:
  - `split_string_to_inlines`: empty, single word, multi-word, newlines,
    tabs, multiple consecutive spaces, mixed space/newline runs,
    leading/trailing whitespace
  - `peek_inlines_fuzzy`: table of inlines, table with mixed strings,
    single inline, single string, multi-word string
  - `peek_blocks_fuzzy`: table of blocks, single block, string→Plain,
    inlines-like→Plain, table of inlines→multiple Plains

### Phase 2: Update all constructors (constructors.rs)

- [x] **2.1** Replace all calls to `lua_table_to_inlines()` with
  `peek_inlines_fuzzy()` in constructors: Para, Plain, Header, Emph,
  Strong, Underline, Strikeout, Superscript, Subscript, SmallCaps,
  Quoted, Link, Image, Span, Cite.

- [x] **2.2** Replace all calls to `lua_table_to_blocks()` with
  `peek_blocks_fuzzy()` in constructors: Note, BlockQuote, Div, Figure.

- [x] **2.3** Update helper parsing functions that call the old functions:
  - `parse_list_items()` → use `peek_blocks_fuzzy()` for each item,
    AND accept a single blocks-like value (not just a table of items),
    matching Pandoc's `peekItemsFuzzy`.
  - `parse_definition_list_items()` → use `peek_inlines_fuzzy()` for terms,
    `peek_blocks_fuzzy()` for definitions (already via parse_list_items).
  - `parse_line_block_content()` → use `peek_inlines_fuzzy()` for each line.
  - `parse_caption()` → use `peek_inlines_fuzzy()` for short,
    `peek_blocks_fuzzy()` for long.
  - `parse_single_citation()` → use `peek_inlines_fuzzy()` for prefix
    and suffix.

- [x] **2.4** Update `pandoc.Inlines()` constructor: delegate entirely to
  `peek_inlines_fuzzy()` for the content argument, then wrap results as
  LuaInline userdata in a Lua table with the Inlines metatable. This
  replaces the current inline coercion logic and adds word-splitting
  for top-level strings.

- [x] **2.5** Update `pandoc.Blocks()` constructor: delegate to
  `peek_blocks_fuzzy()` for the content argument, then wrap results as
  LuaBlock userdata in a Lua table with the Blocks metatable. This adds
  support for strings and inlines-like values (wrapped in Plain).

### Phase 3: Constructor-level tests

- [x] **3.1** Add tests for inlines constructors with coerced input types:
  - `pandoc.Para("hello world")` → `Para([Str("hello"), Space, Str("world")])`
  - `pandoc.Para(pandoc.Str("x"))` → `Para([Str("x")])`
  - `pandoc.Emph("text")` → `Emph([Str("text")])`
  - `pandoc.Header(1, "title")` → `Header(1, [Str("title")])`

- [x] **3.2** Add tests for blocks constructors with coerced input types:
  - `pandoc.Div(pandoc.Para(...))` → `Div([Para(...)])`
  - `pandoc.Div("text")` → `Div([Plain([Str("text")])])`
  - `pandoc.BlockQuote("text")` → `BlockQuote([Plain([Str("text")])])`

- [x] **3.3** Add tests for Inlines/Blocks constructors:
  - `pandoc.Inlines("hello world")` → word-split
  - `pandoc.Blocks("text")` → `[Plain([Str("text")])]`
  - `pandoc.Blocks(pandoc.Str("x"))` → `[Plain([Str("x")])]`

- [x] **3.4** Add tests for helper constructors:
  - `pandoc.BulletList({"text", "more"})` — each string becomes blocks
  - `pandoc.BulletList(pandoc.Para(...))` — single item wrapping
  - `pandoc.LineBlock({"line one", "line two"})` — string lines
  - `pandoc.Citation("id", mode, "prefix")` — string prefix/suffix
  - Caption with string long/short

- [x] **3.5** Add a test reproducing the lipsum pattern:
  ```lua
  local json = quarto.json.decode('["Lorem ipsum dolor sit amet"]')
  return pandoc.Para(json[1])
  ```
  Verify it produces `Para([Str("Lorem"), Space, Str("ipsum"), ...])`.

### Phase 4: Verify

- [x] **4.1** Run `cargo nextest run -p pampa` — all constructor and
  shortcode tests pass
- [x] **4.2** Run `cargo nextest run --workspace` — no regressions
- [x] **4.3** Verify the lipsum smoke test still works (it uses the
  `pandoc.Para({pandoc.Str(...)})` explicit form, which must keep working)

## Design Notes

### Why word-splitting matters

Real Pandoc's `peekInlinesFuzzy` doesn't just wrap a string in `Str` — it
splits on whitespace. This is because `pandoc.Para("hello world")` should
produce the same AST as Pandoc would from parsing markdown `hello world`:
multiple `Str` nodes separated by `Space`.

This distinction matters for rendering: a single `Str("hello world")` with
an embedded space may render differently than `Str("hello") Space Str("world")`
in some output formats.

### `peekInlineFuzzy` vs `peekInlinesFuzzy` string handling

These behave differently for strings:
- `peekInlinesFuzzy("hello world")` → `{Str("hello"), Space, Str("world")}`
  (word split — used when a string is the ENTIRE content argument)
- `peekInlineFuzzy("hello world")` → `Str("hello world")`
  (no split — used when a string is ONE ELEMENT in a table)

This is because in `{"hello", pandoc.Space(), "world"}`, each string
element is treated as a single `Str` node. Word-splitting only applies
at the top level.

### Per-element block coercion

When a table of inlines is passed to a blocks constructor, each element
is independently coerced via `peek_block_fuzzy`. This means
`pandoc.Div({pandoc.Str("x"), pandoc.Str("y")})` produces
`Div([Plain([Str("x")]), Plain([Str("y")])])` — two separate Plain blocks,
NOT one Plain containing both inlines.

### Metamethods (`__toinline`, `__toblock`) — deferred

Real Pandoc supports `__toinline` and `__toblock` metamethods for custom
type coercion. We don't implement these yet and they're not needed for
any current extension. This can be added later when needed.

### Migration: old function names

After renaming `lua_table_to_inlines` → `peek_inlines_fuzzy` (and blocks),
grep for any remaining callers. The rename makes the behavior change
visible and matches Pandoc's terminology.

## Pandoc Source References

| File | Content |
|---|---|
| `~/src/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Inline.hs` | `peekInlineFuzzy` (L127), `peekInlinesFuzzy` (L138), `mkInlines` (L444), all inline constructors |
| `~/src/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Block.hs` | `peekBlockFuzzy` (L133), `peekBlocksFuzzy` (L145), `mkBlocks` (L477), `peekItemsFuzzy` (L469), all block constructors |
| `~/src/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Content.hs` | `peekDefinitionItem` (L73) |
| `~/src/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Caption.hs` | `peekCaptionFuzzy` (L74), `mkCaption` (L83) |
| `~/src/pandoc-lua-marshal/src/Text/Pandoc/Lua/Marshal/Citation.hs` | `mkCitation` (L83) — prefix/suffix use `peekInlinesFuzzy` |
| `~/src/pandoc-types/src/Text/Pandoc/Builder.hs` | `B.text` (L334) — word-splitting algorithm |

## Files Touched

| File | Change |
|---|---|
| `crates/pampa/src/lua/types.rs` | Rewrite `lua_table_to_inlines/blocks` as fuzzy variants, add `split_string_to_inlines` |
| `crates/pampa/src/lua/constructors.rs` | Update all constructor calls, all helper functions, update `Inlines`/`Blocks` constructors |
