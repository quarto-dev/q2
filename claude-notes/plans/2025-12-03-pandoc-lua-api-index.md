# Pandoc Lua API Index

This document provides a navigable index to `external-sources/pandoc/doc/lua-filters.md` (7412 lines). Use this to quickly locate specific API documentation when implementing the Lua runtime.

**Important**: All implementations must go through the `LuaRuntime` abstraction layer. See `claude-notes/plans/2025-12-03-lua-runtime-abstraction-layer.md`.

## Quick Reference: Line Number Ranges

| Section | Lines | Description |
|---------|-------|-------------|
| Introduction & Filter Basics | 1-500 | How filters work, traversal, globals |
| Examples | 473-1003 | Real-world filter examples |
| Type Reference | 1004-2640 | All Pandoc AST types |
| Module pandoc | 2643-4021 | Main module, constructors |
| Module pandoc.cli | 4025-4095 | CLI options parsing |
| Module pandoc.utils | 4099-4492 | Utility functions |
| Module pandoc.mediabag | 4496-4746 | Media storage |
| Module pandoc.List | 4748-4995 | List type and methods |
| Module pandoc.format | 4998-5084 | Format extensions |
| Module pandoc.image | 5088-5144 | Image utilities |
| Module pandoc.json | 5149-5213 | JSON encode/decode |
| Module pandoc.log | 5217-5273 | Logging functions |
| Module pandoc.path | 5277-5523 | Path manipulation |
| Module pandoc.structure | 5528-5708 | Document structure |
| Module pandoc.system | 5712-6079 | System/file operations |
| Module pandoc.layout | 6084-6782 | Doc layout (writers) |
| Module pandoc.scaffolding | 6787-6798 | Writer scaffolding |
| Module pandoc.text | 6802-7009 | UTF-8 text functions |
| Module pandoc.template | 7013-7143 | Template handling |
| Module pandoc.types | 7147-7170 | Version constructor |
| Module pandoc.zip | 7174-7375 | Zip archive handling |

---

## 1. Introduction & Filter Basics (Lines 1-500)

### Filter Structure (Lines 78-180)
- How to define filter functions for each element type
- Return values: nil (keep), replacement, empty list (delete)
- Keywords: `traverse`, `filter`, `Pandoc`, `Meta`

### Filters on Element Sequences (Lines 145-180)
- `Inlines` and `Blocks` filter functions
- Process entire sequences at once

### Traversal Order (Lines 182-248)
- Default: typewise traversal (all instances of one type, then next)
- Alternative: topdown/bottomup traversal
- `traverse` field controls order: `'topdown'`, `'typewise'`

### Global Variables (Lines 250-330)
Important globals exposed to filters:

| Variable | Lines | Description |
|----------|-------|-------------|
| `FORMAT` | 252-259 | Output format string |
| `PANDOC_READER_OPTIONS` | 260-270 | ReaderOptions object |
| `PANDOC_WRITER_OPTIONS` | 271-281 | WriterOptions object |
| `PANDOC_VERSION` | 282-288 | Version object |
| `PANDOC_API_VERSION` | 289-297 | pandoc-types version |
| `PANDOC_SCRIPT_FILE` | 298-303 | Path to current filter |
| `PANDOC_STATE` | 304-313 | CommonState object |

### Pandoc Module Overview (Lines 332-370)
- Element creation shortcuts
- Exposed Pandoc functionality

### Lua Interpreter Initialization (Lines 371-391)
- Custom init scripts via `--lua`
- Path configuration

### Debugging Lua Filters (Lines 392-471)
- `--verbose` flag
- `pandoc.cli.repl` for interactive debugging

---

## 2. Examples (Lines 473-1003)

Practical filter examples to study:

| Example | Lines | Description |
|---------|-------|-------------|
| Macro substitution | ~480 | Simple text replacement |
| Center images (HTML) | ~530 | Div wrapping |
| Pagebreaks (LaTeX/HTML) | ~580 | RawBlock insertion |
| Capitalizing headings | ~640 | text.upper usage |
| Removing links | ~690 | Replace Link with content |
| Removing links (preserve) | ~720 | pandoc.walk_inline |
| Counting words | ~800 | Traverse and count |

---

## 3. Type Reference (Lines 1004-2640)

### Shared Properties (Lines 1010-1023)
- `t` / `tag` - element type string
- `clone()` - deep copy method
- `walk(filter)` - apply filter to element

### Pandoc Document Type (Lines 1024-1087)
```
Pandoc {
  blocks: Blocks,
  meta: Meta
}
```
Anchor: `#type-pandoc`

### Meta Type (Lines 1088-1096)
Anchor: `#type-meta`
- String-indexed table of MetaValues

### MetaValue Types (Lines 1097-1127)
Anchor: `#type-metavalue`
- MetaBool, MetaString, MetaInlines, MetaBlocks, MetaList, MetaMap

### Block Type (Lines 1128-1157)
Anchor: `#type-block`
- Common properties: `t`, `tag`, `clone()`, `walk()`

#### Individual Block Types

| Type | Lines | Anchor | Key Fields |
|------|-------|--------|------------|
| BlockQuote | 1158-1168 | `#type-blockquote` | content |
| BulletList | 1169-1180 | `#type-bulletlist` | content (list of items) |
| CodeBlock | 1181-1206 | `#type-codeblock` | text, attr, identifier, classes, attributes |
| DefinitionList | 1207-1230 | `#type-definitionlist` | content |
| Div | 1231-1254 | `#type-div` | content, attr, identifier, classes, attributes |
| Figure | 1255-1287 | `#type-figure` | content, caption, attr |
| Header | 1288-1316 | `#type-header` | level, content, attr, identifier, classes, attributes |
| HorizontalRule | 1317-1324 | `#type-horizontalrule` | (none) |
| LineBlock | 1325-1336 | `#type-lineblock` | content |
| OrderedList | 1337-1360 | `#type-orderedlist` | content, listAttributes, start, style, delimiter |
| Para | 1361-1372 | `#type-para` | content |
| Plain | 1373-1384 | `#type-plain` | content |
| RawBlock | 1385-1404 | `#type-rawblock` | format, text |
| Table | 1405-1483 | `#type-table` | caption, colspecs, head, bodies, foot, attr |

### Blocks Type (Lines 1484-1534)
Anchor: `#type-blocks`
- List of Block elements
- Methods: `clone()`, `walk(filter)`, `find()`, `find_if()`, `filter()`, `map()`, `insert()`, `remove()`

### Inline Type (Lines 1535-1566)
Anchor: `#type-inline`
- Common properties like Block

#### Individual Inline Types

| Type | Lines | Anchor | Key Fields |
|------|-------|--------|------------|
| Cite | 1567-1583 | `#type-cite` | content, citations |
| Code | 1584-1609 | `#type-code` | text, attr |
| Emph | 1610-1621 | `#type-emph` | content |
| Image | 1622-1660 | `#type-image` | caption, src, title, attr |
| LineBreak | 1661-1668 | `#type-linebreak` | (none) |
| Link | 1669-1707 | `#type-link` | content, target, title, attr |
| Math | 1708-1729 | `#type-math` | mathtype, text |
| Note | 1730-1743 | `#type-note` | content |
| Quoted | 1744-1764 | `#type-quoted` | quotetype, content |
| RawInline | 1765-1784 | `#type-rawinline` | format, text |
| SmallCaps | 1785-1796 | `#type-smallcaps` | content |
| SoftBreak | 1797-1804 | `#type-softbreak` | (none) |
| Space | 1805-1812 | `#type-space` | (none) |
| Span | 1813-1841 | `#type-span` | content, attr |
| Str | 1842-1856 | `#type-str` | text |
| Strikeout | 1857-1868 | `#type-strikeout` | content |
| Strong | 1869-1880 | `#type-strong` | content |
| Subscript | 1881-1892 | `#type-subscript` | content |
| Superscript | 1893-1904 | `#type-superscript` | content |
| Underline | 1905-1916 | `#type-underline` | content |

### Inlines Type (Lines 1943-1996)
Anchor: `#type-inlines`
- Same methods as Blocks

### Element Components (Lines 1997-2216)

| Type | Lines | Anchor | Description |
|------|-------|--------|-------------|
| Attr | 1997-2028 | `#type-attr` | identifier, classes, attributes |
| Attributes | 2029-2036 | `#type-attributes` | key/value pairs |
| Caption | 2037-2048 | `#type-caption` | long, short |
| Cell | 2049-2080 | `#type-cell` | Table cell |
| Citation | 2081-2111 | `#type-citation` | Citation entry |
| ColSpec | 2112-2122 | `#type-colspec` | Column spec (pair) |
| ListAttributes | 2123-2143 | `#type-listattributes` | start, style, delimiter |
| Row | 2144-2155 | `#type-row` | Table row |
| TableBody | 2156-2175 | `#type-tablebody` | Table body |
| TableFoot | 2176-2196 | `#type-tablefoot` | Table foot |
| TableHead | 2197-2216 | `#type-tablehead` | Table head |

### Other Types

| Type | Lines | Anchor | Description |
|------|-------|--------|-------------|
| ReaderOptions | 2218-2255 | `#type-readeroptions` | Parser options |
| WriterOptions | 2256-2375 | `#type-writeroptions` | Writer options |
| CommonState | 2377-2414 | `#type-commonstate` | Pandoc state |
| Doc | 2415-2452 | `#type-doc` | Layout document |
| List | 2453-2467 | `#type-list` | Generic list |
| LogMessage | 2468-2472 | `#type-logmessage` | Log message |
| SimpleTable | 2473-2503 | `#type-simpletable` | Pre-2.10 table |
| Template | 2504-2507 | `#type-template` | Compiled template |
| Version | 2508-2555 | `#type-version` | Version object |
| Chunk | 2578-2618 | `#type-chunk` | Document chunk |
| ChunkedDoc | 2619-2640 | `#type-chunkeddoc` | Chunked document |

---

## 4. Module pandoc (Lines 2643-4021)

Main module with constructors and utilities.

### Fields (Lines 2649-2660)
- `readers` - Set of input format names
- `writers` - Set of output format names

### Element Constructors (Lines 2663-3629)

#### Document Constructors
| Function | Lines | Anchor |
|----------|-------|--------|
| Pandoc | 2663-2678 | `#pandoc.Pandoc` |
| Meta | 2679-2691 | `#pandoc.Meta` |

#### MetaValue Constructors
| Function | Lines | Anchor |
|----------|-------|--------|
| MetaBlocks | 2692-2708 | `#pandoc.MetaBlocks` |
| MetaBool | 2709-2721 | `#pandoc.MetaBool` |
| MetaInlines | 2722-2738 | `#pandoc.MetaInlines` |
| MetaList | 2739-2755 | `#pandoc.MetaList` |
| MetaMap | 2756-2772 | `#pandoc.MetaMap` |
| MetaString | 2773-2789 | `#pandoc.MetaString` |

#### Block Constructors
| Function | Lines | Anchor |
|----------|-------|--------|
| BlockQuote | 2790-2804 | `#pandoc.BlockQuote` |
| BulletList | 2805-2819 | `#pandoc.BulletList` |
| CodeBlock | 2820-2837 | `#pandoc.CodeBlock` |
| DefinitionList | 2838-2853 | `#pandoc.DefinitionList` |
| Div | 2854-2871 | `#pandoc.Div` |
| Figure | 2872-2892 | `#pandoc.Figure` |
| Header | 2893-2913 | `#pandoc.Header` |
| HorizontalRule | 2914-2923 | `#pandoc.HorizontalRule` |
| LineBlock | 2924-2938 | `#pandoc.LineBlock` |
| OrderedList | 2939-2956 | `#pandoc.OrderedList` |
| Para | 2957-2971 | `#pandoc.Para` |
| Plain | 2972-2986 | `#pandoc.Plain` |
| RawBlock | 2987-3004 | `#pandoc.RawBlock` |
| Table | 3005-3034 | `#pandoc.Table` |
| Blocks | 3035-3050 | `#pandoc.Blocks` |

#### Inline Constructors
| Function | Lines | Anchor |
|----------|-------|--------|
| Cite | 3051-3068 | `#pandoc.Cite` |
| Code | 3069-3086 | `#pandoc.Code` |
| Emph | 3087-3101 | `#pandoc.Emph` |
| Image | 3102-3125 | `#pandoc.Image` |
| LineBreak | 3126-3135 | `#pandoc.LineBreak` |
| Link | 3136-3159 | `#pandoc.Link` |
| Math | 3160-3177 | `#pandoc.Math` |
| Note | 3178-3192 | `#pandoc.Note` |
| Quoted | 3193-3211 | `#pandoc.Quoted` |
| RawInline | 3212-3229 | `#pandoc.RawInline` |
| SmallCaps | 3230-3244 | `#pandoc.SmallCaps` |
| SoftBreak | 3245-3254 | `#pandoc.SoftBreak` |
| Space | 3255-3264 | `#pandoc.Space` |
| Span | 3265-3282 | `#pandoc.Span` |
| Str | 3283-3297 | `#pandoc.Str` |
| Strikeout | 3298-3312 | `#pandoc.Strikeout` |
| Strong | 3313-3328 | `#pandoc.Strong` |
| Subscript | 3329-3343 | `#pandoc.Subscript` |
| Superscript | 3344-3358 | `#pandoc.Superscript` |
| Underline | 3359-3373 | `#pandoc.Underline` |
| Inlines | 3374-3395 | `#pandoc.Inlines` |

#### Other Constructors
| Function | Lines | Anchor |
|----------|-------|--------|
| Attr | 3396-3417 | `#pandoc.Attr` |
| Caption | 3418-3437 | `#pandoc.Caption` |
| Cell | 3438-3466 | `#pandoc.Cell` |
| AttributeList | 3467-3479 | `#pandoc.AttributeList` |
| Citation | 3480-3510 | `#pandoc.Citation` |
| ListAttributes | 3511-3532 | `#pandoc.ListAttributes` |
| Row | 3533-3550 | `#pandoc.Row` |
| TableFoot | 3551-3568 | `#pandoc.TableFoot` |
| TableHead | 3569-3586 | `#pandoc.TableHead` |
| SimpleTable | 3587-3630 | `#pandoc.SimpleTable` |

### Constants (Lines 3633-3774)

| Constant | Lines | Category |
|----------|-------|----------|
| AuthorInText | 3635-3639 | CitationMode |
| SuppressAuthor | 3641-3645 | CitationMode |
| NormalCitation | 3647-3651 | CitationMode |
| DisplayMath | 3653-3658 | MathType |
| InlineMath | 3660-3665 | MathType |
| SingleQuote | 3667-3672 | QuoteType |
| DoubleQuote | 3674-3679 | QuoteType |
| AlignLeft | 3681-3685 | Alignment |
| AlignRight | 3687-3691 | Alignment |
| AlignCenter | 3693-3697 | Alignment |
| AlignDefault | 3699-3703 | Alignment |
| DefaultDelim | 3705-3709 | ListDelimiter |
| Period | 3711-3715 | ListDelimiter |
| OneParen | 3717-3721 | ListDelimiter |
| TwoParens | 3723-3727 | ListDelimiter |
| DefaultStyle | 3729-3733 | ListStyle |
| Example | 3735-3739 | ListStyle |
| Decimal | 3741-3745 | ListStyle |
| LowerRoman | 3747-3751 | ListStyle |
| UpperRoman | 3753-3757 | ListStyle |
| LowerAlpha | 3759-3763 | ListStyle |
| UpperAlpha | 3765-3769 | ListStyle |

### Other Constructors (Lines 3776-3825)
| Function | Lines | Anchor |
|----------|-------|--------|
| ReaderOptions | 3778-3801 | `#pandoc.readeroptions` |
| WriterOptions | 3802-3825 | `#pandoc.writeroptions` |

### Helper Functions (Lines 3826-4021)

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| pipe | 3828-3858 | `#pandoc.pipe` | Run external command |
| walk_block | 3859-3875 | `#pandoc.walk_block` | Apply filter to block |
| walk_inline | 3876-3892 | `#pandoc.walk_inline` | Apply filter to inline |
| read | 3893-3954 | `#pandoc.read` | Parse markup to Pandoc |
| write | 3956-3990 | `#pandoc.write` | Convert Pandoc to string |
| write_classic | 3991-4021 | `#pandoc.write_custom` | Classic custom writer |

---

## 5. Module pandoc.cli (Lines 4025-4095)

### Functions
| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| parse_options | 4037-4054 | `#pandoc.cli.parse_options` | Parse CLI args |
| repl | 4056-4094 | `#pandoc.cli.repl` | Interactive REPL |

### Fields
- `default_options` (line 4031) - Default CLI options table

---

## 6. Module pandoc.utils (Lines 4099-4492)

**Critical module** - many essential utilities.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| blocks_to_inlines | 4106-4141 | `#pandoc.utils.blocks_to_inlines` | Squash blocks to inlines |
| citeproc | 4143-4167 | `#pandoc.utils.citeproc` | Process citations |
| equals | 4169-4192 | `#pandoc.utils.equals` | Test equality (deprecated) |
| from_simple_table | 4194-4219 | `#pandoc.utils.from_simple_table` | SimpleTable to Table |
| make_sections | 4221-4250 | `#pandoc.utils.make_sections` | Create section divs |
| normalize_date | 4252-4271 | `#pandoc.utils.normalize_date` | Normalize date format |
| references | 4273-4305 | `#pandoc.utils.references` | Get cited references |
| run_json_filter | 4307-4329 | `#pandoc.utils.run_json_filter` | Run JSON filter |
| run_lua_filter | 4331-4354 | `#pandoc.utils.run_lua_filter` | Run Lua filter |
| sha1 | 4356-4371 | `#pandoc.utils.sha1` | Compute SHA1 hash |
| stringify | 4373-4390 | `#pandoc.utils.stringify` | Element to plain string |
| to_roman_numeral | 4392-4414 | `#pandoc.utils.to_roman_numeral` | Int to Roman numeral |
| to_simple_table | 4416-4439 | `#pandoc.utils.to_simple_table` | Table to SimpleTable |
| type | 4441-4474 | `#pandoc.utils.type` | Pandoc-aware type function |
| Version | 4476-4491 | `#pandoc.utils.Version` | Create Version object |

---

## 7. Module pandoc.mediabag (Lines 4496-4746)

Media storage for images and files.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| delete | 4510-4522 | `#pandoc.mediabag.delete` | Remove entry |
| empty | 4524-4530 | `#pandoc.mediabag.empty` | Clear all entries |
| fetch | 4532-4562 | `#pandoc.mediabag.fetch` | Fetch from URL/file |
| fill | 4564-4584 | `#pandoc.mediabag.fill` | Fill from document |
| insert | 4586-4611 | `#pandoc.mediabag.insert` | Add entry |
| items | 4613-4642 | `#pandoc.mediabag.items` | Iterator over entries |
| list | 4644-4667 | `#pandoc.mediabag.list` | List entry summaries |
| lookup | 4669-4692 | `#pandoc.mediabag.lookup` | Look up entry |
| make_data_uri | 4694-4724 | `#pandoc.mediabag.make_data_uri` | Create data URI |
| write | 4726-4744 | `#pandoc.mediabag.write` | Write to directory |

---

## 8. Module pandoc.List (Lines 4748-4995)

Generic list type with methods.

### Constructor
- `pandoc.List([table])` (line 4755) - Create List

### Metamethods
| Method | Lines | Description |
|--------|-------|-------------|
| __concat | 4763-4773 | Concatenate lists |
| __eq | 4775-4790 | Compare lists |

### Instance Methods
| Method | Lines | Description |
|--------|-------|-------------|
| at | 4793-4814 | Get element at index |
| clone | 4815-4819 | Shallow copy |
| extend | 4820-4828 | Append list |
| find | 4829-4844 | Find element |
| find_if | 4845-4860 | Find by predicate |
| filter | 4861-4873 | Filter by predicate |
| includes | 4874-4888 | Check if contains |
| insert | 4889-4904 | Insert element |
| iter | 4905-4926 | Create iterator |
| map | 4927-4936 | Transform elements |
| new | 4937-4952 | Constructor |
| remove | 4953-4968 | Remove element |
| sort | 4969-4995 | Sort in-place |

---

## 9. Module pandoc.format (Lines 4998-5084)

Format extension handling.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| all_extensions | 5004-5023 | `#pandoc.format.all_extensions` | All valid extensions |
| default_extensions | 5025-5043 | `#pandoc.format.default_extensions` | Default extensions |
| extensions | 5045-5067 | `#pandoc.format.extensions` | Extension config table |
| from_path | 5069-5082 | `#pandoc.format.from_path` | Detect format from path |

---

## 10. Module pandoc.image (Lines 5088-5144)

Image utilities.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| size | 5094-5122 | `#pandoc.image.size` | Get image dimensions |
| format | 5124-5143 | `#pandoc.image.format` | Detect image format |

---

## 11. Module pandoc.json (Lines 5149-5213)

JSON encoding/decoding.

### Fields
- `null` (line 5155) - JSON null value

### Functions
| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| decode | 5161-5189 | `#pandoc.json.decode` | Parse JSON string |
| encode | 5191-5211 | `#pandoc.json.encode` | Encode to JSON |

---

## 12. Module pandoc.log (Lines 5217-5273)

Logging system access.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| info | 5223-5234 | `#pandoc.log.info` | Log info message |
| silence | 5236-5256 | `#pandoc.log.silence` | Suppress logging |
| warn | 5258-5271 | `#pandoc.log.warn` | Log warning |

---

## 13. Module pandoc.path (Lines 5277-5523)

File path manipulation (cross-platform).

### Fields
- `separator` (line 5283) - Directory separator
- `search_path_separator` (line 5287) - PATH separator

### Functions
| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| directory | 5294-5310 | `#pandoc.path.directory` | Get directory part |
| exists | 5312-5336 | `#pandoc.path.exists` | Check path exists |
| filename | 5338-5353 | `#pandoc.path.filename` | Get filename part |
| is_absolute | 5355-5371 | `#pandoc.path.is_absolute` | Check if absolute |
| is_relative | 5373-5389 | `#pandoc.path.is_relative` | Check if relative |
| join | 5391-5406 | `#pandoc.path.join` | Join path parts |
| make_relative | 5408-5432 | `#pandoc.path.make_relative` | Make path relative |
| normalize | 5434-5456 | `#pandoc.path.normalize` | Normalize path |
| split | 5458-5472 | `#pandoc.path.split` | Split by separator |
| split_extension | 5475-5494 | `#pandoc.path.split_extension` | Split extension |
| split_search_path | 5496-5513 | `#pandoc.path.split_search_path` | Split PATH |
| treat_strings_as_paths | 5515-5522 | `#pandoc.path.treat_strings_as_paths` | Augment string |

---

## 14. Module pandoc.structure (Lines 5528-5708)

Higher-level document structure.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| make_sections | 5535-5575 | `#pandoc.structure.make_sections` | Create section divs |
| slide_level | 5577-5594 | `#pandoc.structure.slide_level` | Find slide level |
| split_into_chunks | 5596-5643 | `#pandoc.structure.split_into_chunks` | Split to chunks |
| table_of_contents | 5645-5664 | `#pandoc.structure.table_of_contents` | Generate TOC |
| unique_identifier | 5666-5706 | `#pandoc.structure.unique_identifier` | Generate unique ID |

---

## 15. Module pandoc.system (Lines 5712-6079)

System and file operations.

### Fields
- `arch` (line 5718) - Machine architecture
- `os` (line 5722) - Operating system

### Functions
| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| cputime | 5731-5743 | `#pandoc.system.cputime` | CPU time in picoseconds |
| command | 5745-5773 | `#pandoc.system.command` | Execute command |
| copy | 5775-5790 | `#pandoc.system.copy` | Copy file |
| environment | 5792-5803 | `#pandoc.system.environment` | Get environment |
| get_working_directory | 5805-5815 | `#pandoc.system.get_working_directory` | Get CWD |
| list_directory | 5817-5834 | `#pandoc.system.list_directory` | List directory |
| make_directory | 5836-5858 | `#pandoc.system.make_directory` | Create directory |
| read_file | 5860-5873 | `#pandoc.system.read_file` | Read file contents |
| rename | 5875-5900 | `#pandoc.system.rename` | Rename path |
| remove | 5902-5913 | `#pandoc.system.remove` | Remove file |
| remove_directory | 5915-5930 | `#pandoc.system.remove_directory` | Remove directory |
| times | 5932-5949 | `#pandoc.system.times` | Get file times |
| with_environment | 5951-5974 | `#pandoc.system.with_environment` | Run with env |
| with_temporary_directory | 5976-6001 | `#pandoc.system.with_temporary_directory` | Temp directory |
| with_working_directory | 6003-6025 | `#pandoc.system.with_working_directory` | Change CWD |
| write_file | 6027-6041 | `#pandoc.system.write_file` | Write file |
| xdg | 6043-6077 | `#pandoc.system.xdg` | XDG directories |

---

## 16. Module pandoc.layout (Lines 6084-6782)

Plain-text document layout (for custom writers).

### Fields
- `blankline` (line 6090) - Blank line Doc
- `cr` (line 6094) - Carriage return Doc
- `empty` (line 6099) - Empty Doc
- `space` (line 6103) - Breaking space Doc

### Layout Functions
| Function | Lines | Description |
|----------|-------|-------------|
| after_break | 6109-6128 | Conditional after break |
| before_non_blank | 6130-6146 | Conditional before non-blank |
| blanklines | 6148-6163 | Insert blank lines |
| braces | 6165-6180 | Wrap in {} |
| brackets | 6182-6197 | Wrap in [] |
| cblock | 6199-6219 | Centered block |
| chomp | 6221-6236 | Remove trailing blanks |
| concat | 6238-6256 | Concatenate Docs |
| double_quotes | 6258-6273 | Wrap in "" |
| flush | 6275-6290 | Flush left |
| hang | 6292-6314 | Hanging indent |
| inside | 6316-6337 | Enclose in start/end |
| lblock | 6339-6358 | Left-aligned block |
| literal | 6360-6375 | Create literal Doc |
| nest | 6377-6395 | Indent Doc |
| nestle | 6397-6412 | Remove leading blanks |
| nowrap | 6414-6429 | Prevent wrapping |
| parens | 6431-6446 | Wrap in () |
| prefixed | 6448-6467 | Prefix each line |
| quotes | 6469-6484 | Wrap in '' |
| rblock | 6486-6506 | Right-aligned block |
| vfill | 6508-6524 | Expandable border |
| render | 6526-6553 | Render to string |
| is_empty | 6555-6571 | Check if empty |
| height | 6573-6588 | Get Doc height |
| min_offset | 6590-6607 | Get min width |
| offset | 6609-6624 | Get Doc width |
| real_length | 6626-6643 | UTF-8 aware length |
| update_column | 6645-6664 | Track column |
| bold | 6666-6681 | Bold styling |
| italic | 6683-6698 | Italic styling |
| underlined | 6700-6715 | Underline styling |
| strikeout | 6717-6732 | Strikeout styling |
| fg | 6734-6753 | Foreground color |
| bg | 6755-6774 | Background color |

---

## 17. Module pandoc.scaffolding (Lines 6787-6798)

Writer scaffolding for custom writers.

### Fields
- `Writer` (line 6793) - Writer scaffolding object

---

## 18. Module pandoc.text (Lines 6802-7009)

UTF-8 text manipulation.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| fromencoding | 6819-6845 | `#pandoc.text.fromencoding` | Convert to UTF-8 |
| len | 6847-6863 | `#pandoc.text.len` | Get string length |
| lower | 6865-6880 | `#pandoc.text.lower` | To lowercase |
| reverse | 6882-6897 | `#pandoc.text.reverse` | Reverse string |
| sub | 6899-6921 | `#pandoc.text.sub` | Substring |
| subscript | 6923-6942 | `#pandoc.text.subscript` | Unicode subscript |
| superscript | 6944-6963 | `#pandoc.text.superscript` | Unicode superscript |
| toencoding | 6965-6990 | `#pandoc.text.toencoding` | Convert from UTF-8 |
| upper | 6992-7007 | `#pandoc.text.upper` | To uppercase |

---

## 19. Module pandoc.template (Lines 7013-7143)

Template handling.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| apply | 7019-7041 | `#pandoc.template.apply` | Apply template |
| compile | 7043-7071 | `#pandoc.template.compile` | Compile template |
| default | 7073-7090 | `#pandoc.template.default` | Get default template |
| get | 7092-7112 | `#pandoc.template.get` | Get template text |
| meta_to_context | 7114-7137 | `#pandoc.template.meta_to_context` | Meta to context |

---

## 20. Module pandoc.types (Lines 7147-7170)

Type constructors.

| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| Version | 7153-7168 | `#pandoc.types.Version` | Create Version |

---

## 21. Module pandoc.zip (Lines 7174-7375)

Zip archive handling.

### Functions
| Function | Lines | Anchor | Description |
|----------|-------|--------|-------------|
| Archive | 7195-7213 | `#pandoc.zip.Archive` | Create/read archive |
| Entry | 7215-7237 | `#pandoc.zip.Entry` | Create entry |
| read_entry | 7239-7257 | `#pandoc.zip.read_entry` | Read entry from file |
| zip | 7259-7278 | `#pandoc.zip.zip` | Create archive from files |

### Types
- `zip.Archive` (lines 7282-7323) - Archive with `entries`, `bytestring()`, `extract()`
- `zip.Entry` (lines 7324-7374) - Entry with `modtime`, `path`, `contents()`, `symlink()`

---

## Implementation Priority Groups

Based on `claude-notes/plans/2025-12-02-lua-api-port-plan.md`:

### Phase 1: Core AST Types and Constructors
- All Block/Inline types and their constructors
- Attr, Blocks, Inlines types
- pandoc.List module

### Phase 2: Essential Utilities
- pandoc.utils (stringify, type, etc.)
- pandoc.path
- pandoc.text

### Phase 3: Document Processing
- pandoc.read / pandoc.write
- walk_block / walk_inline
- Global variables (FORMAT, PANDOC_VERSION, etc.)

### Phase 4: Advanced Features
- pandoc.mediabag
- pandoc.system
- pandoc.json

### Phase 5: Writer Support (if needed)
- pandoc.layout
- pandoc.template
- pandoc.scaffolding

---

## Search Keywords

When searching lua-filters.md:

- **Type definitions**: Search for `## TypeName {#type-`
- **Constructors**: Search for `### FunctionName {#pandoc.`
- **Module functions**: Search for `### function_name {#pandoc.module.`
- **Constants**: Search for `[\`ConstantName\`]`
- **Examples**: Look in lines 473-1003
- **Version requirements**: Search for `*Since:`
