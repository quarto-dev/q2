# Block-level editorial marks: `::: ++` / `::: --` / `::: >>`

**Status:** Plan agreed (decisions recorded 2026-09-24). **Awaiting go-ahead to execute** — no code written yet.
**Braid:** bd-an9gkxnp · follow-up: bd-t1avfz0n (q2-preview block-comment chrome)

## Goal

Add a block-level counterpart to the inline editorial marks (`[++ …]`,
`[-- …]`, `[>> …]`, and `[!! …]`):

```
::: --

delete all of these.

paragraphs.

:::

::: ++

add all of these paragraphs.

:::

::: >>

This is a really long comment.

Many paragraphs

:::
```

Build it the way `::: ^note-id` note definitions are already built, and
refactor the opener so that the next "sigil after `:::`" construct is a
small, local change.

## Verdict: feasible, and cheap at the grammar level

The grammar already has the extension point we need. Neither the
`_fenced_div_start` token nor the block-stack machinery (`FENCED_DIV`
entry, `_close_block`/`_block_close`, the `:::` closer) needs to change.
The new constructs differ from `pandoc_div` only in what comes **after**
`::: `, and that is exactly where `note_definition_fenced_block` already
branches off.

### How `::: ^id` works today

- `grammar.js:1002` `pandoc_div` and `grammar.js:1021`
  `note_definition_fenced_block` both start with `$._fenced_div_start`.
  The scanner (`parse_fenced_div_marker`, `scanner.c:622`) consumes the
  colons, pushes a `FENCED_DIV` block, and emits `FENCED_DIV_START`. The
  scanner does **not** decide which rule applies.
- After `_fenced_div_start` and `_whitespace`, the LR state is shared by
  both rules. In that state the external token `fenced_div_note_id` is
  valid, and nowhere else. The scanner's `'^'` case (`scanner.c:2831`)
  checks `valid_symbols[FENCED_DIV_NOTE_ID]` and emits it
  (`parse_fenced_div_note_id`, `scanner.c:1950`). Otherwise the internal
  lexer produces `_commonmark_naked_value` (the info string) or `{`.
- The body (`repeat($._block)`), the optional closer, and `_block_close`
  are copied verbatim from `pandoc_div`.

This means that deciding "which kind of fenced block is this?" is done by
**one token that is valid only in one parse state**. Adding
`++`/`--`/`>>` works the same way.

### What happens today with the proposed syntax (probed with `tree-sitter parse`)

| Opener              | Today                                         |
|---------------------|-----------------------------------------------|
| `::: ++`            | **parse ERROR**: free syntax room             |
| `::: >>`            | **parse ERROR**: free syntax room             |
| `::: !!`            | **parse ERROR**: free syntax room             |
| `::: --`            | `pandoc_div` with `info_string` `--`, i.e. `<div class="--">` |
| `::: --foo`, `::: ---` | `pandoc_div`, info string `--foo` / `---` |
| `::: -- {.x}`       | parse ERROR (an info string and attrs can't both appear) |

`--` is the one case that is not free. `_commonmark_naked_value` is
`/[A-Za-z0-9_-]+/`, so `::: --` is valid today and means a div with the
class `--`. Taking it over is technically a breaking change. **I recommend
accepting it.** A class literally named `--` is not something anyone
writes on purpose, and Pandoc's own reading of it is equally useless.
The new token will require whitespace, a newline, or EOF right after the
two-character marker, so `::: --foo` and `::: ---` keep their current
meaning.

**Blast-radius evidence (2026-09-24):**
- *Local corpora:* 5,568 div openers across every `.qmd`/`.md`/`.Rmd` in
  the repo and `external-sources/` (quarto-cli, quarto-web, pandoc,
  connect-docs, …). **Zero** start with `-`, `+`, `>` or `!` after the
  colons.
- *GitHub code search:* ~300 matched fragments for `"::: --"` across
  `.qmd`/`.Rmd`/`.md`. GitHub search ignores punctuation, so this is a
  sample, not a census. Every real hit (46) is **`::: -->`**, which closes
  an HTML comment wrapped around a div to hide it (`<!--` … `::: -->`).
  None is a dash-prefixed class.
- `::: -->` is safe twice over. First, qmd's scanner lexes the whole
  `<!-- … -->` as one `comment` token, so its inner `:::` lines never
  reach the div scanner (verified with `tree-sitter parse`). Second, the
  boundary rule rejects `--` followed by `>`.

**Stricter variant (adopted, see Decisions):** also forbid info strings that *start* with
`-` (`::: --foo`), so that `++`, `--` and `>>` behave the same way (today
`::: ++foo` is an error but `::: --foo` is a class). The evidence shows
no users. It would cost an error-corpus entry to give `::: --foo` a
useful message ("did you mean `::: -- `?"). This is not needed for the
feature, but it is being done for consistency (Phase 2b).

## Design

### Grammar (`grammar.js`)

Add **one** rule that covers all editorial kinds, instead of one rule per
kind. The kind is carried by the marker token:

```js
editorial_div: $ => seq(
    $._fenced_div_start,
    $._whitespace,
    choice(
        alias($._fenced_div_insert_marker,       $.insert_delimiter),
        alias($._fenced_div_delete_marker,       $.delete_delimiter),
        alias($._fenced_div_edit_comment_marker, $.edit_comment_delimiter),
        alias($._fenced_div_highlight_marker,    $.highlight_delimiter),   // see Q2
    ),
    optional(seq(optional($._whitespace),
                 alias($._pandoc_attr_specifier, $.attribute_specifier))),
    $._newline,
    repeat($._block),
    optional(seq($._fenced_div_end, $._close_block, choice($._newline, $._eof))),
    $._block_close,
),
```

- Reusing the inline `*_delimiter` node names lets queries and pampa code
  treat the inline and block forms the same way. If that turns out to be
  confusing, we can use `block_insert_marker` and similar names instead.
  This is cosmetic.
- An **optional attribute specifier** mirrors `[>> note ]{author="…"}`.
  Block comments need `author=`/`date=` for the same reasons inline ones
  do (the document profile and hub comment chrome read them).
- Register it in `_block_not_section` next to
  `note_definition_fenced_block`.
- The body/closer tail is now identical in three rules. Factor it out
  (e.g. `_fenced_div_tail`) so a fourth construct doesn't copy it a
  fourth time. **This is the "extensibility" refactor.** It has no effect
  on the CST, because a hidden rule is inlined into its parents.

### Scanner (`scanner.c`): one "fenced-div sigil" dispatch

Today the only sigil (`^`) is handled deep inside the main lookahead
switch, mixed with superscript handling. The `+`, `-`, and `>` cases each
already route to unrelated parsers (list markers, thematic breaks, block
quotes, shortcode close). Rather than add a guard to each of those cases,
put a single early dispatch before the main switch:

```c
if (any_fenced_div_sigil_valid(valid_symbols)) {
    // only valid immediately after `::: ` — no other construct competes
    if (parse_fenced_div_sigil(s, lexer, valid_symbols)) return true;
    // on false, fall through: tree-sitter resets the lexer and the
    // internal lexer handles naked-value / `{` as today
}
```

`parse_fenced_div_sigil` switches on the first character:

- `^` → the existing `parse_fenced_div_note_id` (moved here, unchanged).
- `+`, `-`, `>`, `!` → require the same character twice, then require
  that the next character is `' '`, `'\t'`, `'\n'`, `'\r'`, or EOF. Call
  `mark_end` after the two characters and emit the matching token.
  Otherwise return false so the internal lexer takes over (`::: --foo`
  stays an info string).

The next sigil construct will then need one `case` in that function, one
external token, and one `choice` arm. That is the shape the grammar
should have.

Mechanical requirements:
- The external-token enum in `scanner.c` (and its debug name table at
  `scanner.c:205`) must stay in the **same order** as the `externals`
  array in `grammar.js`. Append the new tokens next to
  `fenced_div_note_id`.
- The sigil tokens are valid only in the state right after
  `_fenced_div_start _whitespace`. Block starts, inline parsing, and
  continuation matching are unaffected. Nothing new is pushed on the
  block stack, because `FENCED_DIV_START` already pushed `FENCED_DIV`.

### pampa (reader): desugar straight to `Div`, with no new AST node

The inline marks are parsed into `Inline::Insert` and similar nodes, and
then desugared in `postprocess.rs:1500` into
`Span.quarto-insert` / `.quarto-delete` / `.quarto-highlight` /
`.quarto-edit-comment`, with any user attributes merged in. Everything
downstream (CSS in `_bootstrap-rules.scss:1166`, the document profile's
comment extraction, the q2-preview `CommentBlock` chrome, and the qmd
writer's decorated-syntax round-trip) works on those **classes**, not on
the custom inline types.

For blocks, **I recommend not adding a `Block::Insert`-style variant.**
`NoteDefinitionFencedBlock` shows what that costs: it touches
**49 files** (every walker, writer, Lua type, JSON reader/writer,
reconcile hash, and so on). The tree-sitter processing function should
emit `Block::Div` directly, with class `quarto-insert` (etc.) prepended to
any user classes. This matches exactly what the inline marks become
after postprocess, so the "desugared into spans at the proper stages"
contract becomes "desugared into divs" for the block form, with no new
stage.

- New `treesitter_utils/editorial_div.rs::process_editorial_div`, similar
  to `note_definition_fenced_block.rs`. It reads the delimiter child to
  choose the class, merges the attribute specifier, collects the blocks,
  and sets `source_info` and `attr_source` like
  `process_fenced_div_block` does.
- Dispatch `"editorial_div"` in `treesitter.rs` (next to `pandoc_div` at
  `:1499`).
- **qmd writer** (`writers/qmd.rs:642` `write_div`): mirror `write_span`'s
  rule. A Div whose *first* class is `quarto-insert` (etc.) is written as
  `::: ++` plus a trailing `{…}` for the remaining attributes. (The inline
  rule requires exactly one class and no attributes. We can relax both
  forms together now that attributes are grammatical.)

### Downstream consumers (render correctness beyond "it parses")

| Consumer | Needed change |
|---|---|
| HTML writer / CSS | `.quarto-insert` etc. rules already apply to any element, so a block `div` gets the background, strike-through, and italic styling. Check that `text-decoration: line-through` on a block that contains lists or code looks acceptable. Possibly add `div.quarto-*` block tweaks (left border instead of full background). |
| `document_profile.rs` comment extraction (`:1296`) | Today it only matches `Inline::Span`. Add a `Block::Div` arm. `ProfileComment.text` is a string, so flatten the blocks to plain text and keep the source span. |
| q2-preview `CommentBlock` chrome (`ts-packages/preview-renderer/.../CommentBlock.tsx`) | It assumes comments are inline spans inside a host block (and uses the `quarto-edit-comment-container` Div for code blocks). A block `::: >>` Div needs a decision: render it as a comment "block" with its own bubble, or at least render it plainly without crashing. **Out of scope: follow-up strand bd-t1avfz0n.** |
| Tree-sitter queries (`queries/highlights.scm`) | Add highlighting for the new delimiters. |
| Error corpus (`crates/pampa/resources/error-corpus/_autogen-table.json`) | Any grammar change renumbers LR states. **Must** re-run `scripts/build_error_table.ts` and accept the error-corpus snapshots. |

## Decisions (2026-09-24)

1. **`::: --` is taken over, and the stricter variant is adopted.** No
   usage was found (see the blast-radius evidence above). Div info
   strings may no longer start with `-`. `::: --foo`, `::: -foo` and
   `::: ---` become parse errors with a dedicated error code suggesting
   `::: -- ` (Phase 2b). This makes `++`, `--`, `>>` and `!!` behave the
   same way.
2. **`::: !!` (block highlight) is included.**
3. **Attributes on the opener** (`::: >> {author="cs" date="…"}`) are
   supported.
4. **AST: `Div` with a `quarto-*` class**, not a new `Block` variant. The
   round-trip canonicalization is accepted: a hand-written
   `::: {.quarto-delete}` is written back as `::: --`, as spans already
   do.
5. **Hub-client / q2-preview chrome for block comments** is a follow-up:
   bd-t1avfz0n (blocked by this strand).

## Implementation phases

Each phase ends green. Tree-sitter work follows AGENTS.md: in
`crates/tree-sitter-qmd/tree-sitter-markdown`, run `tree-sitter generate;
tree-sitter build; tree-sitter test`, and never edit existing corpus
expectations.

### Phase 1: grammar refactor with no behavior change
- [x] Factor the shared fenced-div tail out of `pandoc_div` and
      `note_definition_fenced_block`. Done as a **JS helper**
      (`fencedDivTail($)`), not a hidden rule: a hidden nonterminal adds an
      LR reduction, while the helper leaves `parser.c` **byte-identical**
      (verified: only `src/grammar.json` changed).
- [x] Move `^` handling into a new `parse_fenced_div_sigil` early dispatch
      (`any_fenced_div_sigil_valid` guard, placed just before the main
      lookahead switch; `parse_caret` now only handles superscript and
      inline notes). I checked `ts_external_scanner_states` to confirm the
      sigil token shares a scanner state with no other external token
      (only state 72, plus the all-valid error-recovery state 1, which
      returns `CLOSE_BLOCK` before the dispatch). So a failed sigil scan
      can safely `return false` and hand over to the internal lexer.
- [x] `tree-sitter test` passes with **no** changes to existing corpus
      expectations (626/626, including the new test below).
- [x] **Found and fixed two pre-existing bugs in `parse_fenced_div_note_id`:**
  - `::: ^id` as the last bytes of the input (no trailing newline)
    **hung the scanner forever**: at EOF `lookahead` is 0 and `advance`
    does nothing. Only direct tree-sitter consumers hit this, because
    pampa appends a missing final newline (Q-7-1) before parsing.
    Regression test: `test/corpus/fenced_div_sigils.txt` (it hung before
    the fix).
  - With CRLF input, the id kept the `\r` (`"foo\r"`). Regression test:
    `crates/pampa/tests/integration/test_fenced_div_sigils.rs` (it failed
    with `["foo\r"]` before the fix).

### Phase 2: new tokens + `editorial_div` rule
- [x] Add the 4 external tokens (grammar `externals` + scanner enum + name
      table, same order).
- [x] Add sigil cases for `++ -- >> !!` with the boundary rule. The marker
      must be followed by whitespace, a line ending, EOF, **or `{`**, so
      `::: ++{.x}` works like `:::{.x}` does for plain divs (`{` can't start
      an info string, so this is unambiguous).
- [x] Add the `editorial_div` rule with an optional attribute specifier;
      register it in `_block_not_section`. It generated with no conflicts.
- [x] Corpus tests live in `test/corpus/fenced_div_sigils.txt` (one file
      for all sigil constructs, not `editorial_div.txt`): each marker; with
      attrs, with and without a space before them; trailing whitespace; no
      blank lines in the body; nested (`::: >>` inside `:::: ++`); inside a
      list item and a block quote (the same structure as a plain div there,
      checked by diffing against `::: {.x}`); inline marks inside a block
      mark; unclosed at EOF; opener at EOF without a newline; `- item` /
      `---` / `-@cite` right after the opener; `::: ^id` unchanged; `::: -->`
      inside an HTML comment unchanged.
- [x] `queries/highlights.scm`: block markers are captured as
      `@punctuation.special`. Only the marker is captured, because the body
      can hold code cells whose interior the query file must never cover.

### Phase 2b: forbid div info strings starting with `-`
- [x] `pandoc_div` has its own `_div_info_string: /[A-Za-z0-9_][A-Za-z0-9_-]*/`
      (aliased to `info_string`). `_commonmark_naked_value` is unchanged for
      code blocks.
- [x] Corpus: `::: --foo`, `::: ---`, `::: -x` are asserted with
      tree-sitter's `:error` attribute (not a full ERROR tree, which is
      recovery noise that would make the tests brittle). I checked that
      `:error` is really enforced. `::: foo-bar` is still a `pandoc_div`.
- [x] New error **Q-2-53** ("Fenced div info string starts with `-`"). Q-2-38
      was taken; codes up to Q-2-52 are all used. Added:
  - the corpus JSON, with one case per distinct LR state: top level (2800),
    inside a list item or block quote (2808), and `:::--foo` with no space
    (3205);
  - `desynchronizes: true`, because without it a valid table after the bad
    opener produced two cascade "Parse error"s;
  - the catalog entry;
  - `docs/errors/markdown/Q-2-53.qmd` (status complete);
  - a sidebar entry in `docs/_quarto.yml`.

  `cargo xtask lint` passes, including the error-docs rules.
  Note: `{.--foo}` is **not** valid qmd (the class shorthand rejects a
  leading `-` too), so the page points class-name users at
  `{class="--foo"}` (verified).

### Phase 3: pampa reader + writer
- [x] `editorial_div.rs::process_editorial_div` → `Block::Div` whose first
      class is the mark's `quarto-*` class, followed by the user's attrs. A
      user duplicate of the mark class is dropped. `attr_source.classes`
      stays aligned, and the synthesized class's source is the **marker
      bytes** (`>>`).
- [x] qmd writer: a Div whose *first* class is a mark class is written as
      `::: <marker>` plus `{rest}`. `editorial_marker_for_class` is now
      shared with the span writer. `::: {.quarto-delete .x}` is
      canonicalized to `::: -- {.x}` (accepted in Decisions).
- [x] Tests:
  - `test_fenced_div_sigils.rs` (lowering, attrs + provenance, dedupe,
    `++{`, CRLF, writer round-trip and canonicalization);
  - native and qmd snapshots (`editorial-divs`);
  - qmd→JSON→qmd round-trip fixture (`editorial_divs.qmd`);
  - smoke fixture `030.qmd`, which feeds location-health, the tiling
    auditor and the incremental-writer tests.

  The existing smoke and coverage tests cover the reader paths, so no
  `test_treesitter_coverage.rs` case was needed.
- [x] Regenerated the error table twice (after Phase 2 and after 2b). All 34
      codes kept the same number of mappings, and `test_error_corpus` passes.
      The script touches `deno.lock` as a side effect; revert that each time.

### Phase 4: consumers + docs
- [x] `document_profile.rs`: a `::: >>` Div is one comment (a leaf). Its text
      is `blocks_to_string` of the body, with author/date/kvs from the
      opener. The `quarto-edit-comment-container` Div is not a comment.
      Tests added.
- [x] CSS: I rendered a sample and screenshotted it (Playwright). Two
      problems: text sat flush against the tint, with the last child's
      margin inside it, and a block comment looked like a plain italic
      paragraph. Added `div.quarto-*` padding plus `> :last-child`
      margin reset, and a left rule on `div.quarto-edit-comment`. Test
      added in `compile_all_themes_test.rs`.
- [x] User docs: there was **no** editorial-marks section anywhere in
      `docs/`. Added "Editorial marks" (inline + block forms, attributes) to
      `docs/guides/authoring/markdown-basics.qmd`; it renders cleanly
      (checked by screenshot). It deliberately does not use the page's
      list-table layout for the rendered examples: inline marks inside a
      list-table are broken (bd-9fy9p4zl, below).
- [x] `cargo xtask verify`: all 14 steps green, including tree-sitter LF and
      **CRLF** passes (647/647 each) and workspace clippy `-D warnings`.
      The phase-5 single-doc byte-identity baseline (`styles.css` hash) was
      re-captured for the new block CSS. I verified the only delta is the
      three new rules. `preview_static_e2e::editing_the_project_config_rerenders_every_page`
      timed out once under full parallel load and passes 3/3 in isolation.
      That is a timing flake, not related to this work.

### Pre-existing bugs found along the way
- **Fixed here**, with regression tests written first:
  - `::: ^id` at EOF hung the scanner, and CRLF leaked `\r` into note ids
    (Phase 1).
  - Inline editorial marks lowered to Spans dropped their attr sidecar
    wholesale, so classes and sources were misaligned
    (`AttrAlignmentSkipped`) and the user's attribute provenance was lost.
    postprocess now keeps it and prepends `None` for the synthesized class.
    The tiling corpus had no inline mark before `030.qmd`, which is why
    this was never caught.
  - **bd-2281lkrx**: an inline editorial mark inside a list-table
    **panicked** pampa. `traverse_inline_nonterminal` had no arms for
    `Insert/Delete/Highlight/EditComment`. It now traverses them like
    `Span`.
- **Filed, not fixed** (outside this feature's scope):
  - **bd-9fy9p4zl** (P1): list-table / definition-list content is never
    postprocessed. The rewrite returns `recurse=false` before the top-down
    walk reaches the cells. So note refs, standalone attrs and editorial
    marks in cells fail (Q-3-31/32/33). Fixing it needs care around
    `with_table`'s caption heuristic.
  - **bd-i4xrcdqx**: a whitespace-only blank line before `:::` in a list
    item makes the list tight. The qmd writer emits exactly that shape,
    so list items containing a div flip Para→Plain on round-trip. The
    round-trip fixture leaves out its list-item case for this reason.

## Risks

- **LR-state churn → error-corpus remap.** This is expected and
  mechanical, but the table must be regenerated. It is easy to forget.
- **The `-` scanner path is busy** (list markers, thematic breaks,
  `-@cite`). The early sigil dispatch only runs when a sigil token is
  valid, and that happens only right after `::: `. Corpus tests for
  `- item` / `---` / `-@x` directly after a div opener line guard this.
- **Block-level `line-through`** over code blocks and tables may look
  poor. This is a styling decision, not a correctness one.
