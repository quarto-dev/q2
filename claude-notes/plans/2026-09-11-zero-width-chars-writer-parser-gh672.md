# Zero-width / format characters: writer re-encodes as entities, parser accepts them raw (GH #672)

**Date:** 2026-09-11
**Braid:** bd-wuiu1of7 (bug, P1, labels `pampa`, `tree-sitter-qmd`, `parity`) —
discovered-from bd-named-entities-w6xbfftj (entity decode, PR #488), related
bd-96fswwce (combining marks, `\p{M}`)
**Discovered:** bd-i18zoy4n (writer does not escape `&` that forms a valid
entity reference — folded into this PR, Phase 1b); bd-5rr4lgj1 (leading BOM
not stripped — follow-up, out of scope)
**GitHub:** https://github.com/quarto-dev/q2/issues/672
**Checkout:** main @ `7ef59618` (investigated in place; no worktree yet)
**Status:** Plan reviewed 2026-09-11; all scope decisions settled (see
"Scope decisions"). Awaiting go-ahead to execute.

## Problem

Since PR #488 the reader decodes `&ZeroWidthSpace;` into a `Str` holding the
literal U+200B. The qmd writer (`escape_markdown` in
`crates/pampa/src/writers/qmd.rs`) emits that codepoint verbatim, and the
grammar's prose regexes (`PANDOC_REGEX_STR` in
`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js`) accept no
`Cf`-category character except ZWNJ/ZWJ (U+200C/U+200D, added by
bd-96fswwce). So pampa's own output does not re-parse.

Verified at `7ef59618` (`target/debug/pampa`, input `a<char>b`):

| Codepoint(s) | Category | Named entity | Prose today |
|---|---|---|---|
| U+00AD soft hyphen | Cf | `&shy;` | parse error |
| U+200B zero width space | Cf | `&ZeroWidthSpace;` (+4 `&Negative…Space;` aliases) | parse error |
| U+200C / U+200D ZWNJ / ZWJ | Cf | `&zwnj;` / `&zwj;` | **OK** (bd-96fswwce) |
| U+200E / U+200F LRM / RLM | Cf | `&lrm;` / `&rlm;` | parse error |
| U+2060 word joiner | Cf | `&NoBreak;` | parse error |
| U+2061–U+2063 (function application, invisible times, invisible separator) | Cf | `&af;` `&it;` `&ic;` (+ long aliases) | parse error |
| U+2064 invisible plus, U+FEFF ZWNBSP, U+061C ALM, U+202A–E bidi embeddings, U+2066–9 bidi isolates | Cf | *(none)* | parse error |
| U+00A0, U+2009 etc. | Zs | `&nbsp;`, `&thinsp;` … | OK (already in `PANDOC_NON_ASCII_WHITESPACE`) |

Numeric references have the same problem (`&#x200B;` → raw U+200B → parse
error). Non-prose contexts already accept the raw characters: code spans, code
blocks, link titles and attribute values all round-trip. Prose contexts that
fail include link text and YAML metadata values parsed as markdown (Q-1-20).

**The in-the-wild cascade** (quarto-web `index.qmd:26`) reproduces:

```
$ printf '# Welcome to Quarto^&ZeroWidthSpace;[®]{.trademark}^ {.mt-1}\n' > in.qmd
$ pampa -t qmd in.qmd > out.qmd     # emits raw U+200B inside the ^…^
$ pampa out.qmd
Error: [Q-2-16] Unclosed Superscript
 1 │ # Welcome to Quarto^​[®]{.trademark}^ {.mt-1}
```

**Pandoc parity:** `pandoc -t markdown` also emits the raw codepoint, and
Pandoc's reader accepts every one of these characters into `Str` verbatim.
So (2) below is pure parity; (1) is a Q2 choice for readable, robust output.

## Fix (two-sided, per the issue discussion)

1. **Writer:** `escape_markdown` re-encodes every `Cf` character as a named
   entity when one exists, else a hex numeric reference (`&#x2064;`). The
   reader already decodes both forms, so the AST round-trips.
2. **Parser:** add `\p{Cf}` to the prose content classes so raw zero-width
   characters (pasted from the web, produced by editors, or written by older
   pampa builds) parse — the same fix shape as `\p{M}` in bd-96fswwce.

Consequence, accepted deliberately: raw U+200B in source is **not canonical**
at the text level — it re-emits as `&ZeroWidthSpace;`. The existing round-trip
harness (`test_qmd_roundtrip_consistency` in
`crates/pampa/tests/integration/test.rs`) compares ASTs (JSON minus
locations), not text, so this is already the kind of non-canonicity it
tolerates (cf. `&#34;` → `\"`, `---` → em dash).

## Design details

### Writer: which characters, which spelling

- **Set:** Unicode general category `Cf` (format), ~170 codepoints in ~20
  ranges. Rust's `std` has no general-category query, so add a small
  `is_format_char(c) -> bool` over an explicit sorted range table in a new
  `crates/pampa/src/writers/unicode_format.rs` (or a sibling module of
  `qmd.rs`). Pin the table with a unit test that cross-checks every scalar
  value against the `regex` crate's `\p{Cf}` (pampa already depends on
  `regex` with the `unicode` feature), so the table cannot drift silently.
- **Spelling:** an explicit `char → &'static str` table of *preferred* names,
  not an inversion of `html_entities.json` (which is many-to-one — five names
  map to U+200B). The names come from the WHATWG HTML named character
  references table (https://html.spec.whatwg.org/multipage/named-characters.html,
  machine-readable at https://html.spec.whatwg.org/entities.json), which is
  the same table the grammar regex and the reader's decoder are generated
  from (`crates/tree-sitter-qmd/common/html_entities.json`). The spec has no
  notion of a preferred name, so the choice among aliases is ours: the HTML 4
  legacy name where one exists, otherwise the descriptive (non-spacing-alias)
  name. Preferred names: `&shy;`, `&ZeroWidthSpace;`, `&lrm;`, `&rlm;`,
  `&NoBreak;`, `&ApplyFunction;`, `&InvisibleTimes;`, `&InvisibleComma;`
  (the long MathML names, decided 2026-09-11 — clearer in a qmd file than
  `&af;`/`&it;`/`&ic;`, and rare enough that brevity buys nothing). Put a
  comment with the spec link and this rule next to the table in code.
  Everything else in `Cf` → `&#xXXXX;` (uppercase hex, no zero padding beyond
  the codepoint's digits). Unit-test that every preferred name exists in
  `tree_sitter_qmd::HTML_ENTITIES_JSON` and decodes to exactly that char.
- **Source spelling is not preserved.** `&af;`, `&ApplyFunction;` and a raw
  U+2061 all decode to the same `Str` (adjacent `Str`s are merged), so the
  writer cannot tell them apart and emits `&ApplyFunction;` for all three.
  Same lossiness the writer already has for `&#34;`/`&quot;` → `\"` and
  `---`/em dash, and the same as Pandoc's markdown writer. Recovering the
  original bytes via `source_info` was considered and rejected: `write_str`
  does not consult source info today, and ASTs from JSON or Lua filters have
  none to consult.
- **ZWNJ / ZWJ (U+200C / U+200D): leave raw** (status quo). They already
  parse, they are legitimate content in Persian/Indic text, and ZWJ is a
  structural part of emoji sequences (`👨‍👩‍👧`), where `&zwj;` would be
  noise and would split the emoji token on re-read (still AST-equal after
  `merge_strs`, but ugly). Open question §2 below.
- **Where:** a new match arm block in `escape_markdown` *before* the `_ =>`
  fallthrough. The `suppress_dash_canonicalization` fast path in `write_str`
  is unaffected: `line_is_dash_only_hazard` only fires for dash-only lines,
  so no `Cf` char can reach that branch.
- **Not touched:** link titles, attribute values, code — those are `String`
  fields, not `Str` inlines, and already round-trip raw. Other writers
  (HTML, JSON, plaintext, ANSI) keep emitting the codepoint.

### Parser: grammar change

In `grammar.js`, generalize the existing class:

```js
// before
const PANDOC_COMBINING_MARKS = "\\p{M}\\u{200C}\\u{200D}";
// after
const PANDOC_COMBINING_MARKS = "\\p{M}\\p{Cf}";
```

(`\p{Cf}` ⊃ U+200C/U+200D, so this is a strict widening.) It is already in
both places it needs to be — the single-char alternative and the
word-continuation class — so no structural regex change. Rename to
`PANDOC_MARKS_AND_FORMAT` (or similar) and update the doc comment. Then
`tree-sitter generate; tree-sitter build; tree-sitter test` in
`crates/tree-sitter-qmd/tree-sitter-markdown/`.

EMOJI_REGEX interplay is unchanged (ZWJ was already in the class; longest
match still wins for emoji sequences). `startStrRegex` is deliberately not
widened, mirroring bd-96fswwce: a `Cf`-initial run lexes via the single-char
alternative, keeping emphasis/underscore boundary logic untouched.

## Work items

### Phase 0 — Tests first (TDD)

- [x] **Grammar corpus** `test/corpus/format_characters.txt` (new file): prose
      with raw U+200B mid-word, U+00AD mid-word, U+200E after a space,
      U+2060 at line start, U+FEFF mid-word, and U+200B inside a heading
      superscript. Verified 2026-09-11: all 7 produce `ERROR` nodes pre-fix.
- [x] **Rust parser coverage** in
      `crates/pampa/tests/integration/test_treesitter_coverage.rs`: verbatim
      `Str` for each case above (model on `test_combining_mark_*`). Verified
      2026-09-11: all 8 fail pre-fix with parse errors.
- [x] **Writer unit tests** (in the `mod tests` of `qmd.rs`):
      `escape_markdown("a\u{200B}b")` → `a&ZeroWidthSpace;b`; `&shy;`,
      `&lrm;`, `&rlm;`, `&NoBreak;`, `&ApplyFunction;`, `&InvisibleTimes;`,
      `&InvisibleComma;`; `&af;` and `&ApplyFunction;` in the same paragraph
      both come out as `&ApplyFunction;`; `\u{2064}` →
      `&#x2064;`; ZWNJ/ZWJ stay raw; emoji ZWJ sequence stays raw. Verified
      2026-09-11: the 3 encoding tests fail pre-fix; the 2 stay-raw guards
      pass before and after by design.
- [x] **Round-trip fixtures** under
      `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`:
      - extend `named_entities.qmd` with `&ZeroWidthSpace;`, `&shy;`,
        `&NoBreak;`, `&lrm;` and a numeric `&#x200B;`;
      - new `format_characters_raw.qmd` with the raw codepoints (exercises the
        parser side and the deliberate text-level non-canonicity);
      - new `zero_width_in_heading_superscript.qmd` reproducing the
        quarto-web heading (the issue's cascade).
      Verified 2026-09-11: `test_qmd_roundtrip_consistency` panics pre-fix
      at "Failed to parse original QMD" on `format_characters_raw.qmd` (the
      harness stops at the first fixture that fails to parse, so the two
      writer-only fixtures are checked once the parser change lands).
- [x] **Format-table pin tests:** the `Cf` range table matches `\p{Cf}` from
      the `regex` crate; every preferred entity name is in
      `HTML_ENTITIES_JSON` and decodes to the expected char.

### Phase 1a — Writer: format characters → entities

- [x] Add `is_format_char` + range table + preferred-name table, with a
      comment linking the WHATWG named-character-references table and
      stating the alias rule.
- [x] New arms in `escape_markdown`; doc comment explaining the choice and
      the ZWNJ/ZWJ carve-out.
- [x] Writer unit tests + round-trip fixtures that only need the writer
      (`named_entities.qmd`, heading-superscript fixture) go green (verified
      via `pampa -t qmd | pampa` AST diff before Phase 2 landed, then via
      the harness).

### Phase 1b — Writer: escape `&` that would lex as a reference (bd-i18zoy4n)

Pandoc does this (`[Str "&copy;"]` → `a\&copy;b`); we never escape `&`, so
`\&copy;` in source re-reads as `©`. Needed for soundness once 1a lands: a
literal `Str "&ZeroWidthSpace;"` and a `Str "\u{200B}"` must not produce the
same bytes.

- [ ] **Tests first:** unit tests for `escape_markdown`: `"&copy;"` → `\&copy;`,
      `"&#34;"` → `\&#34;`, `"&#x200B;"` → `\&#x200B;`, `"AT&T"` → `AT&T`
      (unchanged), `"a & b"` → unchanged, `"&AM;"` → unchanged (not a
      semicolon-terminated WHATWG name), `"&amp"` (no `;`) → unchanged.
      Round-trip fixture `ampersand_escaped_entities.qmd` with `\&copy;`,
      `\&#62;`, `\&ZeroWidthSpace;` and plain `AT&T` / `a & b`. Verify
      failures pre-fix (AST differs after regeneration).
- [ ] Implement: in `escape_markdown`, on `&` look ahead for
      `&#[0-9]{1,7};`, `&#[xX][0-9a-fA-F]{1,6};` (the grammar's
      `numeric_character_reference` regex) or `&<name>;` where `&<name>;` is a
      key of the shared WHATWG table (`entity_table()` in
      `treesitter_utils/entity_reference.rs` — make it `pub(crate)` or expose
      a `is_entity_name` helper so writer and reader share one source of
      truth). Emit `\&` in that case, `&` otherwise.
- [ ] Expect snapshot / fixture churn wherever writer output contained a
      literal `&name;`; review each change is a *correct* re-escaping.
      Prefer landing 1b as its own commit for reviewability (user's call
      2026-09-11: not strict — judge at implementation time).

### Phase 2 — Parser

- [x] `grammar.js` class change + comment; `tree-sitter generate`;
      `tree-sitter build`; `tree-sitter test` green — 625/625 (only the new
      corpus file is added; its heading case was updated with `-u` to the
      real tree shape: explicit `superscript_delimiter` nodes, no space).
- [x] Coverage tests + `format_characters_raw.qmd` round-trip go green
      (`cargo nextest run -p pampa -p tree-sitter-qmd --no-fail-fast`:
      4755 passed).
- [x] Watch `parser.c` size / generation time for state growth: generate
      took 0.26 s; `parser.c` diff is +496/−489 lines (character-class table
      rows only, no state growth).

### Phase 3 — Verification + bookkeeping

- [ ] `cargo nextest run -p pampa -p tree-sitter-qmd`.
- [ ] `cargo nextest run --workspace`.
- [ ] **Full `cargo xtask verify`** (not `--skip-hub-build`): the grammar
      change flows into the WASM parser used by hub-client and `q2 preview`.
- [ ] End-to-end (record invocation + output here): `cargo run --bin q2 --
      render` of a fixture containing the quarto-web heading and a raw-U+200B
      paragraph; inspect the HTML for the U+200B bytes inside `<sup>`; and the
      `pampa -t qmd | pampa` pipe from the issue must print the AST instead
      of a parse error.
- [ ] Commit (Rust + regenerated `parser.c` + fixtures); close bd-wuiu1of7
      with the commit hash; comment on GH #672.

## Scope decisions (settled with the user, 2026-09-11)

1. **`\p{Cf}` — the whole category**, for the same reason bd-96fswwce took all
   of `\p{M}`: Pandoc accepts everything and any narrower list is a future
   bug report (bidi isolates, tag characters, …). Fallback if
   `tree-sitter generate` rejects `\p{Cf}` (unlikely — CLI 0.26.8, and the
   grammar already uses compound `\p{M}`): an explicit range list generated
   from the same table as the writer's.
2. **ZWNJ/ZWJ stay raw in the writer.** They parse today, and Persian/Indic
   text and emoji ZWJ sequences stay readable. No emoji-awareness in the
   writer.
3. **bd-i18zoy4n is fixed in this PR** (Phase 1b), ideally as its own commit;
   test churn is acceptable.
4. **Leading BOM: follow-up strand bd-5rr4lgj1**, out of scope here. After this
   change a BOM-prefixed file parses (BOM inside the first `Str`) instead of
   erroring; Pandoc strips it, and so should we, at the reader entry point.
5. **Text-level non-canonicity is accepted.** Pandoc investigation
   (2026-09-11, pandoc 3.9.0.2): its readers resolve entities before `Str`
   construction, never split `Str` at entity boundaries, and treat
   `&af;`, `&ApplyFunction;` and raw U+2061 identically; its markdown writer
   emits raw codepoints by default and `--ascii` re-encodes with an
   unprincipled alias choice (`&COPY;`, `&af;`). A strict spelling-preserving
   round-trip would require splitting `Str` in the AST, diverging from Pandoc
   for every filter; rejected, no strand filed.

## Risks

- **Grammar regeneration** touches `parser.c` (large diff) and the WASM
  parser — full `verify` is mandatory, and the PR should be checked against
  hub-client preview once.
- **Text-level non-canonicity** is intentional (see Fix). If any consumer
  assumes writer output is byte-identical to input for entity-free sources,
  the new `format_characters_raw.qmd` fixture is where it will show up.
- **Table drift:** the `Cf` table is Unicode-version dependent (the `regex`
  crate's tables vs. tree-sitter's). The pin test catches drift on the Rust
  side; the grammar side uses the category name and needs no table.
