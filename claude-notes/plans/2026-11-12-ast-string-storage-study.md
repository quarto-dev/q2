---
title: "Study: arena / borrowed string storage for the Pandoc AST"
strand: bd-akgst1fi
related: [bd-is4q72tt]
status: "Direction agreed (no lifetimes; node layout first; text runs deferred). Step 1 (box cold Inline variants) done: AST heap 88x -> 26x of source. Steps 2-5 open under epic bd-4x6kih4c."
date: 2026-11-12
---

# Arena / borrowed string storage for the Pandoc AST: feasibility study

## Question

Pampa stores every `Str` (and all other AST text) as an owned `String`. Could
an arena or borrowing discipline — for example `&str` slices into `.qmd`
buffers that are kept alive for the life of the project — significantly cut
allocation count and memory use, and is the lifetime complexity worth it?

## Short answer

1. **Borrowing source slices is feasible only at a very high cost, and it
   targets the wrong 2%.** On the benchmark corpus, `Str.text` accounts for
   70% of the AST's heap *blocks* but only 1.9% of its heap *bytes*.
2. **The memory problem is node layout.** The retained AST is **88× the size
   of the source**. 89% of that is `Vec<Inline>` buffers, because `Inline` is
   360 bytes and a `Space` — 44% of all inlines — occupies a full 360-byte
   slot.
3. **Lifetime-free changes capture nearly all of the available win.** Boxing
   the cold enum variants, slimming `Attr`, a small-string type, and a
   compact `SourceInfo` should bring the AST from about 88× to about 12× of
   source and remove about 75% of heap blocks (arithmetic estimates, not yet
   prototyped). Borrowing would add roughly 4% on top of that.
4. **Recommendation:** do not introduce `Inline<'a>`. Do the layout work
   in the order given under "Recommended path".

## Measurements

Tool: `crates/perf-harness/src/bin/ast_memory.rs` (new, uncommitted). It
parses every `*.md` / `*.qmd` under a directory with
`pampa::readers::qmd::read`, retains every `(Pandoc, ASTContext)`, and counts
through a wrapping `GlobalAlloc`.

```bash
cargo build --release -p perf-harness --bin ast-memory
./target/release/ast-memory claude-notes/plans
```

Corpus: `claude-notes/plans`, 1,695 files, 26.30 MiB. 1,673 files parsed
(26.27 MiB); 22 small files (0.03 MiB) failed to parse and are excluded.
Platform: macOS arm64, system allocator, release build.

### Totals

| | Value |
|---|---|
| Source parsed | 26.27 MiB |
| Retained AST heap (freed by dropping all `Pandoc`s) | 2,304 MiB in 4.06 M blocks |
| AST heap / source bytes | **87.7×** |
| Live heap blocks per KiB of source | 151 |
| Retained `ASTContext`s (a source copy plus line index each) | 33 MiB |
| Peak RSS of the run | 2.75 GiB |
| Allocator calls during parsing | 27.0 M allocs + 3.7 M reallocs, 22.9 M frees |
| Wall time | 7.7 s (about 3.4 MiB/s) |

Parsing makes 6.6 allocations for every block it retains.

### Where the bytes are

| Component | MiB | Share of AST heap |
|---|---|---|
| `Vec<Inline>` buffers (5,995,451 inlines × 360 B) | 2,058 | 89.3% |
| `Vec<Block>` buffers (202 K blocks × 824 B) | 160 | 6.9% |
| All text fields, malloc-rounded | 57 | 2.5% |
| — of which `Str.text` | 44 | 1.9% |
| Everything else (`Concat` source infos, `Arc`s, nested vecs, metadata) | 29 | 1.3% |

Type sizes that drive this:

| Type | Bytes | Note |
|---|---|---|
| `Inline` | 360 | sized by `Link` (352) |
| `Block` | 824 | sized by `Table` (824) |
| `Str` | 56 | `String` 24 + `SourceInfo` 32 |
| `Space` | 32 | just a `SourceInfo` |
| `Code` | 240 | `Attr` 104 + `AttrSourceInfo` 80 + text + source info |
| `Attr` | 104 | |
| `SourceInfo` | 32 | three `usize` plus tag |

Inline census: 2.84 M `Str`, 2.64 M `Space`, 277 K `Code`, 158 K `SoftBreak`,
59 K `Strong`; everything else is under 13 K each. `Link` and `Image`
together are 1,752 nodes — 0.03% of inlines set the size of the other 99.97%.

Of 365,800 `Attr` sites, 28,735 (7.9%) are non-empty. The other 92% each
carry 184 bytes of empty `Attr` + `AttrSourceInfo`.

### Where the heap blocks are

| Field | Count | Heap blocks | Content bytes | Malloc bytes |
|---|---|---|---|---|
| `Str.text` | 2,839,066 | 2,839,066 | 14.5 M | 46.2 M |
| `Code.text` | 276,738 | 276,718 | 4.9 M | 7.0 M |
| `attr.id` | 365,800 | 23,569 | 0.6 M | 0.8 M |
| `meta.key` | 7,017 | 7,017 | 0.05 M | 0.1 M |
| `CodeBlock.text` | 6,550 | 6,547 | 3.2 M | 5.0 M |
| other | 9,075 | 7,321 | 0.1 M | 0.2 M |
| **All text** | 3,504,246 | 3,160,238 | 23.4 M | 59.3 M |

Text is 78% of the AST's heap blocks. `Str.text` alone is 70%. Each `Str`
string averages 5.1 bytes of content in a 16-byte minimum malloc block.

### `Str` shape

| Length | Share |
|---|---|
| ≤ 7 bytes | 80.1% |
| 8–15 | 19.1% |
| 16–23 | 0.7% |
| ≥ 24 | 0.1% |

- **Borrowable:** 89.7% of `Str` texts are byte-identical to the source range
  their `source_info` resolves to. 1.7% differ (smart typography, escapes,
  entities). 8.6% have a `Concat` source info that does not resolve to one
  range; these are tokens glued by `merge_strs`, such as `(word)`.
- **Repetition:** 130 K distinct texts among 2.39 M `Str` nodes in ordinary
  paragraphs, headers and list items (1.27 MB of distinct content).

### wasm32 type sizes

The hub client is wasm32, where `usize` and pointers are 4 bytes. Sizes
there (from a `cargo check --target wasm32-unknown-unknown` probe):

| Type | 64-bit | wasm32 |
|---|---|---|
| `Inline` | 360 | 192 |
| `Block` | 824 | 432 |
| `Str` | 56 | 28 |
| `SourceInfo` | 32 | 16 |
| `Attr` | 104 | 56 |
| `AttrSourceInfo` | 80 | 40 |
| `Code` | 240 | 128 |

Two consequences:

- The same structural waste exists in WASM at about half the absolute size:
  roughly 47× source instead of 88× (scaled from the native census, not
  measured in WASM). Boxing cold variants takes `Inline` from 192 to 32
  bytes there, a 6× cut.
- `SourceInfo` is already 16 bytes on wasm32, so `u32` offsets (step D) do
  nothing for the hub. They help native only.

### Text-run what-if

Collapsing each maximal run of adjacent `Str`/`Space` inside one
`Vec<Inline>` into a single node:

| | Inlines | Runs | Mean run text |
|---|---|---|---|
| Today | 5,995,451 | — | — |
| `Str` + `Space` runs | 1,165,598 (5.1× fewer) | 647,608 | 26.5 bytes |
| also absorbing `SoftBreak` | 895,023 (6.7× fewer) | 535,198 | — |

Run text totals 17.2 MB. A run of that mean length no longer fits a small
string inline, so each run is one heap block (about 650 K blocks).

### Caveats

- This measures the reader's output only. Transforms, Lua filters, the
  several whole-AST clones in the pipeline, and writers are not included.
- `q2` uses mimalloc, whose size classes differ from macOS malloc. The
  struct-size results are allocator-independent; the per-string rounding
  figures are not.
- A project render does not retain every AST at once. The per-document ratio
  is what matters there: a 1 MB `.qmd` implies roughly 90 MB of AST per live
  copy. This is consistent with, and probably a large part of, bd-is4q72tt
  (1.1 MB `api/index.qmd` peaking at 1.1 GB RSS).
- The corpus is prose-heavy Markdown with much inline code. Notebook-style
  documents will have a larger `CodeBlock` share.

## Why borrowing from the source buffer is expensive here

The codebase survey found these constraints. Each one needs an answer before
`Inline<'a>` could exist.

### The reader does not produce slices

- **Per-token rewriting.** The main `Str` path is
  `apply_smart_typography(process_backslash_escapes(raw.to_string()))`
  (`crates/pampa/src/pandoc/treesitter.rs:902-913`). Entities, numeric
  references, task-list boxes and cite suffixes are synthesized.
- **Merging.** `merge_strs` (`postprocess.rs:2012-2067`) concatenates
  adjacent tokens into a new `String`; `coalesce_abbreviations` inserts
  U+00A0.
- **Metadata.** Every untagged YAML string is re-parsed as Markdown from the
  *decoded YAML string*, not from the `.qmd` buffer (`meta.rs:144-176`).
- **Code and math.** `CodeBlock.text` is verbatim only at top level with an
  unindented fence; block-quote and list gutters are stripped. `Code.text`
  is trimmed and has soft breaks collapsed.

So an owned or arena path is required for at least 10% of `Str`s and for
most other text. The type would be `Cow<'a, str>` or an arena string, not
`&'a str`.

### Consumers need `'static` or cross-buffer data

- **Lua filters.** Nodes live inside mlua userdata as
  `Rc<RefCell<Inline>>` (`crates/pampa/src/lua/types.rs:196,1116`).
  `create_userdata` requires `T: 'static`, and `lua.scope` is not used. Every
  crossing already deep-clones the node.
- **Cross-document sharing.** `Arc<ProjectIndex>` holds `DocumentProfile`s
  with `Option<Inlines>` and `ConfigValue` fields, read by other documents'
  renders on other rayon workers (`project/pass2_renderer.rs:542-571`,
  `document_profile.rs:706-717`). Profiles are also deserialized from the
  on-disk profile cache, where no source buffer exists.
- **Trees from several buffers.** `reconcile(original, executed)` moves
  nodes from two parses of two different buffers into one tree
  (`quarto-ast-reconcile/src/apply.rs:24-63`). Includes splice blocks parsed
  from other files; book rendering merges chapter ASTs.
- **Trees from no buffer.** The Pandoc JSON reader, JSON filters,
  `apply_node_edit`, citeproc output and about 800 synthesized-`Str` sites in
  transforms produce text with no source.
- **Async pipeline.** `PipelineData` and `StageContext` are deliberately
  lifetime-free so stages can be boxed trait objects
  (`claude-notes/plans/2026-01-06-pipeline-stage-design.md:310`).

### The never-freed pool does not fit the long-running hosts

- The hub client re-parses the document on every keystroke, in WASM, where
  linear memory never shrinks.
- `q2 preview` re-renders on every file change; the LSP re-parses on each
  analysis.

A pool that is never freed is correct only for one-shot `q2 render`. The
other three hosts would need generation-scoped arenas, which brings back the
lifetime (or a refcount).

### Scale of the change

- 16 crates depend on `quarto-pandoc-types`.
- About 10,900 `Inline::` / `Block::` sites, 1,264 `Inline::Str(` sites,
  91 `AstTransform` impls, and about 1,900 function-signature lines that
  mention `Pandoc`, `Block` or `Inline`.
- A previous, smaller attempt at the same trade — borrowed YAML — was
  designed and then abandoned for owned data
  (`claude-notes/yaml-with-source-info-lifetime-approach.md`).

### What it would buy

After the layout fixes below, borrowing saves the residual text heap: about
12 MiB of about 315 MiB on this corpus, roughly 4%. Against a 16-byte
inline small string, a 16-byte `&str` saves no node space at all.

## Precedent

No project found combines an AST borrowed from the source with a
mutation-heavy filter pipeline. (Checked against current docs and source on
2026-11-12; URLs are under "Sources" at the end.)

| Project | Text storage | Relevance |
|---|---|---|
| pulldown-cmark | `CowStr<'a>`: borrowed, boxed, or inline | Event stream, not a tree; filters emit owned strings |
| jotdown | `Cow<'s, str>` events | Same model |
| comrak | Nodes in a `typed_arena`; text is `Cow<'static, str>`, never borrowed from the source | Closest analogue: arena for structure, owned text, interior mutability. 0.45 reported 1.32× from removing clones |
| markdown-rs | Owned `String` mdast | No lifetimes |
| Typst | `EcoString` leaves (15 bytes inline), `Arc` inner nodes | Lifetime-free, small strings inline |
| rowan / rust-analyzer | Immutable green tree, tokens interned per file behind `Arc` | Lifetime-free; mutation rebuilds spines |
| oxc | Whole AST in a bump arena with `'a` | Reported about 20% from the arena, 10% from boxing enum variants to 16 bytes, up to 5% from `u32` spans. Plain source slices failed for escaped text. Took "five attempts" |
| swc | Owned AST, interned 8-byte `Atom` | Lifetime-free |
| Ruff | `Name(CompactString)`, later a 16-byte inline type | +4–8% from the small-string swap alone; `smol_str` regressed |
| serde | `&'de str` zero-copy | Documents that escaped strings cannot be borrowed |

Three patterns recur:

- **Escapes defeat pure borrowing.** oxc and serde both say so explicitly.
  Markdown has the same problem.
- **Arena wins come from node structure**, not from borrowed text: fewer
  allocations, no `Drop`, smaller enums.
- **Mutating pipelines use lifetime-free cheap text**: inline small strings,
  interning, or refcounted buffers.

## Options

| Option | AST bytes (est.) | Heap blocks (est.) | Lifetimes | Churn |
|---|---|---|---|---|
| Today | 2,304 MiB (88×) | 4.06 M | none | — |
| A. Box cold variants | ~535 MiB (20×) | ~4.35 M | none | Pattern matches on boxed variants |
| B. A + slim `Attr` | ~490 MiB (19×) | ~4.1 M | none | `Attr` accessors |
| C. B + 24-byte small string | ~440 MiB (17×) | ~1.0 M | none | `String` → new type at text fields |
| D. C + 16-byte `SourceInfo` and 16-byte string | ~315 MiB (12×) | ~1.0 M | none | quarto-source-map API change |
| E. D + borrowed / arena text | ~300 MiB (11×) | ~0.7 M | `'a` on every AST type | Whole workspace |
| F. D + text runs instead of `Str`/`Space` | ~140 MiB (5×) | ~1.0 M | none | Changes the Pandoc data model |

All estimates are arithmetic on the measured census, not prototypes.

### A. Box the cold variants

Make `Inline` 64 bytes and `Block` about 64 bytes by boxing the payloads of
`Link`, `Image`, `Code`, `Cite`, `Custom`, `Shortcode`, the editorial marks
and `Attr` for inlines, and `Table`, `Figure`, `Header`, `CodeBlock`, `Div`
and `Custom` for blocks. `Vec<Inline>` falls from 2,058 to 366 MiB. Boxed
payloads add back about 72 MiB and about 290 K blocks.

### B. Slim `Attr`

Store `Attr` + `AttrSourceInfo` behind one `Option<Box<…>>`, or as a
thin-vec-style handle. 92% of sites are empty and would cost 8 bytes instead
of 184. `Code` drops from 240 to about 64 bytes and may not need boxing.

### C. Small-string type for text

Replace `String` in text fields with a type that stores short strings
inline. 99.9% of `Str` texts fit in 23 bytes and 99.2% in 15. This removes
about 2.84 M `Str` blocks and most `Code.text` blocks, with no lifetime.

Candidates:

| Crate | Size | Inline | Clone | Note |
|---|---|---|---|---|
| `compact_str` | 24 | 24 | O(n) | Drop-in for `String`; mutable; Ruff's choice |
| `smol_str` | 24 | 23 | O(1) | Immutable; regressed in Ruff |
| `ecow` | 16 | 15 | O(1) | Typst's; already in the lockfile |
| `hipstr` | 24 | 23 | O(1) | O(1) substrings of a shared buffer |

`hipstr` (or `arcstr::Substr`) is the lifetime-free version of the original
idea: long text such as `CodeBlock.text` can be a refcounted slice of the
source buffer, and the buffer is freed when the last slice goes. It suits
preview and the hub, where a leaked pool does not.

### D. Compact `SourceInfo`

See "quarto-source-map changes". With a 16-byte `SourceInfo` and a 16-byte
string, `Str` is 32 bytes and `Inline` 40.

### E. Borrowed or arena text

Feasible only by threading `'a` through 16 crates, copying to owned at the
Lua, profile-cache and reconcile boundaries, and scoping arenas per parse
generation in the hub. Not recommended.

A cheaper variant is `&'static str` from deliberately leaked buffers
(`String::leak`). It needs no lifetime parameter, but it is correct only in
one-shot `q2 render`, and the type must still have an owned arm.

### F. Text runs

Represent a run of words and spaces as one inline, splitting into
`Str`/`Space` lazily where Pandoc semantics are needed (Lua, JSON writer).
Measured: inlines fall 5.1× (6.7× if soft breaks are absorbed). On top of
D that is about 315 → 140 MiB on this corpus. Applied to today's layout
alone it would be about 2,300 → 650 MiB, so it is worth less than step A
and stacks with it.

Costs beyond the data-model change: per-word source locations become
derived (offset arithmetic, exact only when the run is a verbatim source
slice), and the heap-block count does not fall further, because each run
needs a heap string.

**Decision (2026-11-12): deferred until the node-layout work is done.** It
would need a feature flag, and a reader-level flag cannot live in document
metadata, which is only available after parsing. Where such a flag would
live (project config, CLI, or a lazily split representation that needs no
flag) is an open design question for then.

## quarto-source-map changes

These are independent of the string question and useful on their own.

1. **Compact offsets.** `Original { file_id: u32, start: u32, end: u32 }`
   gives a 16-byte `SourceInfo`. Files over 4 GiB are out of scope. Every
   AST node carries at least one `SourceInfo`, so this is worth 16 bytes × 6
   M nodes on this corpus.
2. **Hull instead of `Concat` when pieces are adjacent.** `combine()`
   (`source_info.rs:370`) always builds a two-piece `Concat`, and
   `merge_strs` chains it, so N tokens give N−1 nested `Concat`s. 236,635
   `Str` nodes (8.3%) carry one. Each costs at least one 96-byte heap block
   — more than the string it describes — and makes `resolve_byte_range`
   return `None`. When both sides are `Original` in the same file and
   byte-adjacent, return one `Original`. `contiguous_hull_for_run` in
   `postprocess.rs` already does this for abbreviations.
3. **Shared source text.** `SourceFile.content: Option<String>` could be
   `Option<Arc<str>>`. Today a parsed document holds two full copies of its
   source (`DocumentAst.ast_context.source_context` and
   `DocumentAst.source_context`), and `SourceContext: Clone` deep-copies
   them again.
4. **Text lookup.** A `SourceContext::text_of(&SourceInfo) -> Option<&str>`
   helper would let consumers that want the raw slice get it without the
   AST storing it.

## Other findings

- **Transient churn in the reader.** Each word token costs four allocations
  to keep one: `to_string()`, the output of `process_backslash_escapes`, and
  a `Vec<char>` plus output in `apply_smart_typography`
  (`text_helpers.rs:259-359`). A fast path that returns the input unchanged
  when it contains no `\`, `'`, `-` or `.` would remove most of the 22.9 M
  frees.
- **Input copies.** The pipeline copies the input three to four times
  before parsing (`pipeline.rs:1180`, `stage/data.rs:345`,
  `readers/qmd.rs:147-190`).
- **Metadata re-parse.** Each metadata string re-parse copies the string,
  usually twice, and builds a line index that is then discarded.
- **Lua boundary.** Every node crossing deep-clones in both directions.
  Smaller nodes make this proportionally cheaper.

## Recommended path

Priority, as agreed on 2026-11-12: hub/WASM memory first, then peak RSS of
`q2 render`, then wall time; the three are roughly one class. Since `u32`
offsets do nothing on wasm32, D moves behind the small-string step. A
breaking `quarto-source-map` release is acceptable.

Each step is separately shippable and measurable with `ast-memory`.
Tracked under epic bd-4x6kih4c: bd-xrfun6mn (step 1), bd-09g8zl30 and
bd-gz80k6c6 (step 2), bd-cwprgmo2 (step 3), bd-v6cug77d (step 4),
bd-9iyh0220 (step 5), bd-r17a0k7v (wasm32 tracking), bd-jmn62md8 (text
runs, deferred).

1. **Prototype A on `Inline` only** and re-measure. This is the test of the
   central estimate; if `Vec<Inline>` does not fall to about 370 MiB, stop
   and re-examine.
2. **A for `Block`, then B.**
3. **Reader fast paths** (typography/escape no-op path, `Concat` hull).
4. **C**, starting with `ecow`, benchmarked on this corpus and on a
   Lua-heavy render. For the Lua-heavy case, convert the listing of the
   `claude-notes/plans` website to an lmd-based setup that builds (say) a
   bullet list of the entries, instead of the listing we have now.
5. **D**, as a quarto-source-map minor release.
6. Re-measure bd-is4q72tt's 1.1 MB document end to end. Decide then whether
   F is worth a design.

## Progress

### Step 1: box cold `Inline` variants (bd-xrfun6mn) — measured

Boxed the payloads of `Quoted`, `Cite`, `Code`, `Math`, `RawInline`, `Link`,
`Image`, `Span`, `Shortcode`, `Attr`, `Insert`, `Delete`, `Highlight`,
`EditComment` and `Custom`. A compile-time assertion in `inline.rs` holds
`Inline` at 8 words (64 bytes native, 32 on wasm32).

Same corpus, same tool:

| | Before | After | Estimate |
|---|---|---|---|
| `size_of::<Inline>()` | 360 | 64 | 64 |
| `Vec<Inline>` buffers | 2,058 MiB | 366 MiB | ~366 MiB |
| AST heap | 2,304 MiB (87.7×) | 678 MiB (25.8×) | — |
| AST heap blocks | 4.06 M | 4.36 M | ~4.35 M |
| Peak RSS of the run | 2.75 GiB | 0.87 GiB | — |
| Wall time | 7.7 s | 6.5 s | — |

The central estimate held, so the remaining steps proceed as planned. The
297 K extra heap blocks are almost all boxed `Code` nodes (277 K, 240 bytes
each); step 2's `Attr` slimming shrinks those.

Notes for the next steps, from doing this one:

- Construction sites (`Inline::Link(Link { .. })`) and box/unbox
  mismatches are mechanical: a bracket-matching codemod plus rustc's own
  `Box::new` / unboxing suggestions handled about 650 sites.
- Patterns that destructure a boxed payload cannot stay as written. Most
  code binds the payload and uses field access, which auto-derefs and
  needed no change; only about 30 sites destructured.
- Or-patterns that mix boxed and unboxed variants must be split into one
  arm per boxed variant.

## Decisions (2026-11-12)

- **Goal order:** hub/WASM memory > `q2 render` peak RSS > wall time.
- **quarto-source-map:** a breaking release with `u32` offsets is acceptable.
- **`ast_memory.rs`:** committed to `perf-harness` as the regression tool.
- **Text runs (F):** deferred to after the layout work; needs a flag design.
- **Small-string type:** try `ecow` first (16 bytes on both targets, 15
  inline). Typst uses it and has substantial WASM exposure. Fall back to
  comparing `compact_str` / `hipstr` only if it disappoints.
- **wasm32 tracking:** a work item, not a question (bd-r17a0k7v). WASM
  targets build here; the tool is the implementer's choice. `ast-memory`
  measures a native build only, so the primary goal needs its own number.

## Open questions

None blocking. The text-run flag design is deferred (see F).

## Sources

- pulldown-cmark `CowStr`: https://docs.rs/pulldown-cmark/latest/pulldown_cmark/enum.CowStr.html
- comrak `NodeValue` and changelog (0.45.0): https://docs.rs/comrak/latest/comrak/nodes/enum.NodeValue.html, https://github.com/kivikakk/comrak/blob/main/CHANGELOG.md
- jotdown `Event`: https://docs.rs/jotdown/latest/jotdown/enum.Event.html
- markdown-rs mdast `Text`: https://docs.rs/markdown/latest/markdown/mdast/struct.Text.html
- Typst `SyntaxNode`: https://github.com/typst/typst/blob/main/crates/typst-syntax/src/node.rs
- rust-analyzer syntax design: https://github.com/rust-lang/rust-analyzer/blob/master/docs/book/src/contributing/syntax.md
- oxc performance notes and AST docs: https://oxc.rs/docs/learn/performance.html, https://oxc.rs/docs/contribute/parser/ast
- swc string management, `hstr`: https://swc.rs/docs/contributing/commons/string-management, https://docs.rs/hstr
- Ruff `CompactString` names (PR 12101): https://github.com/astral-sh/ruff/pull/12101
- serde lifetimes: https://serde.rs/lifetimes.html
- Small-string comparison: https://github.com/rosetta-rs/string-rosetta-rs
- `arcstr`, `hipstr`, `self_cell`: https://docs.rs/arcstr, https://docs.rs/hipstr, https://docs.rs/self_cell
- Adrian Sampson, "Flattening ASTs": https://www.cs.cornell.edu/~asampson/blog/flattening.html

Not verified: the exact inline capacity of pulldown-cmark's `InlineStr`, and
whether oxc's `Atom<'a>` still exists under that name.
