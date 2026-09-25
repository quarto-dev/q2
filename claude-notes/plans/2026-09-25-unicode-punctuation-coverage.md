# Unicode ⟨ (U+27E8) and other non-ASCII brackets are uncoded parse errors (bd-angle-bracket-u27e8-parse-error-r6l55zmh)

**Date:** 2026-09-25
**Braid:** bd-angle-bracket-u27e8-parse-error-r6l55zmh (child of epic bd-uk8zgkha)
**Branch:** `braid/r6l55zmh-angle-bracket-parse-error` (topic branch in the main checkout, based on `main` at `ce01489c4`)
**Status:** Investigation, pending design alignment with user. **Do not start implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design.** The root cause is clear and reproducible at HEAD: the
`pandoc_str` token in `grammar.js` never admits the non-ASCII Unicode
open/close punctuation categories (`Ps`/`Pe`). A sweep of every assigned code
point shows about 200 characters that fail, in five families (below). A
single-regex fix works in tree-sitter 0.26.8 (probe below). The only real
design choice is between a minimal patch and replacing the hand-enumerated
ranges.

## Issue context

Filed 2026-09-25 by Carlos (P2 bug). `a ⟨b` and `Revert ⟨hunk⟩ then RED`
produce an uncoded `Error: Parse error` at the `⟨`. Pandoc 3.11 renders both
as literal text. The strand guessed "⟨ confused with `<`" and asked to check
⟩ U+27E9, 〈 U+2329, ‹ U+2039. The user asked this investigation to also
survey which other reasonably common Unicode characters and character classes
fail.

## Dependency graph

- **discovered-from / parent-child**: bd-uk8zgkha, the epic to render
  `claude-notes/` as a q2 website. ⟨ showed up in 6 plans (for example
  `plans/2026-08-20-provenance-2-consumers.md:424`). Its siblings are the other
  "q2 is stricter than Pandoc on prose" classes (bare `@`, flanking `* ~ ^`,
  apostrophes after spans, `$` flanking, and so on). This one is independent
  of them because it is a lexer character-class gap, not a delimiter rule.
- No blocks edges. The epic's clean render is the only thing waiting on it.
- **Precedent, same bug family** (not linked in braid, but the same fix
  shape):
  - bd-6kewx (`3451f64f7`): non-ASCII `Po`/`Pc` accepted as Str. Corpus
    `test/corpus/po-as-str.txt`.
  - bd-96fswwce (`fa7dcbc39`): combining marks. Plan
    `2026-08-10-combining-marks-parse.md`.
  - bd-wuiu1of7 (GH #672): `Cf` format characters.
  - bd-rmx3 / bd-8oe4: non-ASCII whitespace
    (`2026-04-30-unicode-whitespace-handling.md`).

  Each of these patched one Unicode category after a user hit it. This strand
  is the fourth in the series, which argues for closing the whole family at
  once (see Q1).

## What the code looks like today

`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:1-190` builds
`PANDOC_REGEX_STR` from:

| constant | covers | how |
|---|---|---|
| `PANDOC_ALPHA_NUM` | L, N | `\p{L}\p{N}` |
| `PANDOC_PUNCTUATION` | Pd, a few ASCII, `…` | `\p{Pd}` plus literals |
| `PANDOC_VALID_OTHER_PUNCTUATION` | non-ASCII Po, Pc | **enumerated ranges** (`scripts/unicode-ranges.py`) |
| `PANDOC_VALID_SYMBOLS` | Sm, Sk, Sc (minus ASCII), So | Sm/Sk/Sc **enumerated**, So is `\p{So}` |
| `PANDOC_SMART_QUOTES` | 12 Pi/Pf quotes | literals |
| `PANDOC_COMBINING_MARKS` | M, Cf | `\p{M}\p{Cf}` |

**Ps and Pe are missing.** ASCII `( )` sit in `PANDOC_PUNCTUATION`, and
`[ ] { }` are markup, but no non-ASCII opening or closing punctuation is
accepted. The strand's guess (confusion with `<`) is wrong. ⟨ is simply not
in any token, so the lexer errors. ‹ U+2039 works because it is in
`PANDOC_SMART_QUOTES`.

### Sweep results (all 149,718 assigned non-ASCII code points, Unicode 15.1)

Scripts and raw output are in
`claude-notes/plans/2026-09-25-unicode-punctuation-coverage-investigation/`:

- `static_sweep.mjs`: tests the compiled `pandoc_str` pattern from
  `grammar.json` against every code point in Node (Node's Unicode tables are
  newer than Python's).
- `dynamic_sweep.py <pampa>`: parses `a Xb and aX b` with the real pampa
  binary, bisecting failing batches (about 7 s). This is the authoritative
  list: **196 failures**.

| family | count | examples | how common |
|---|---|---|---|
| **Ps/Pe brackets** | 74 + 74 | ⟨⟩ ⟦⟧ ⟪⟫ ⌈⌉ ⌊⌋ 〈〉 ⦃⦄ ⁽⁾ ₍₎ ❨❩; **CJK** 「」『』【】〈〉《》〔〕（）［］｛｝｢｣ 〝〞〟 | **High.** Math notation in prose, plus *every* CJK document that uses corner brackets or full-width parentheses. |
| Pi/Pf editorial brackets | 6 + 6 | ⸂⸃ ⸄⸅ ⸉⸊ ⸌⸍ ⸜⸝ ⸠⸡ | Low (text-critical editions) |
| Po drift | 35 | ⹓ ⹔ (medieval ?/!), Kawi, Old Uyghur, Balinese | Very low. Characters added in Unicode 14/15 after the enumerated table was generated. |
| Sc drift | 1 | ₰ U+20B0 GERMAN PENNY SIGN | Very low. It is just a hole in the hand-typed currency list (U+20AF, then U+20B1). |
| Sm drift (static only) | 12 | U+10D8E, U+1F8D0-D8 | Nil. These are Unicode 16/17 characters. |

Everything else passed: all letters, digits, marks, Cf, So, Sk, the Sm list,
non-ASCII whitespace, and emoji. So the Ps/Pe family is the one that matters.
The rest is enumeration drift, which will keep coming back as Unicode grows
unless the enumerations go away.

Pandoc 3.11 folds all of them into `Str` (checked: `a ⟨b⟩ 「c」 ⸂d⸃ （e） ₰ ⌈f⌉`).

### Probe: class set operations work in tree-sitter 0.26.8

A throwaway grammar (scratchpad, not committed) with
`new RegExp("[[\\p{P}\\p{S}]&&[^\\x00-\\x7F]]", "v")` generates and lexes
⟨ 「 ₰ ⸂ ⌈ as one token, and still rejects ASCII `<`. Notes:

- The JS `v` flag is needed for Node to accept `&&`; the `u` flag throws.
- tree-sitter reads `.source` into `grammar.json` verbatim and compiles it
  with its own Unicode tables.

So "every non-ASCII punctuation or symbol character" can be a single class,
with no enumeration and no generator script.

## Proposed phases (draft)

- **Phase 0: failing tests first.**
  - Tree-sitter corpus file (for example `test/corpus/brackets-as-str.txt`,
    modeled on `po-as-str.txt`) with ⟨⟩, ⌈⌉, 「」, （）, and the strand's two
    repros.
  - pampa integration test (`crates/pampa/tests/integration/`) checking the
    AST is `Str`s. Include a round trip through the qmd writer so it does not
    escape or mangle these characters.
  - Keep `dynamic_sweep.py` as a manual regression tool. Possibly also a
    cheap in-repo test over a curated list of code points (see Q3).
- **Phase 1: grammar change.** Either the minimal patch or the class rewrite
  (Q1). Then regenerate `parser.c`, run the corpus tests, and rerun the sweep
  and expect 0 failures.
- **Phase 2: downstream checks.**
  - `cargo xtask verify`: the wasm parser is built from the same `parser.c`.
  - Compare `parser.c` size and lex state count before and after.
  - Re-render `claude-notes` and confirm the ⟨ diagnostics in the 6 plans
    are gone.
- **Phase 3: docs.**
  - Update the grammar.js comments.
  - Either retire `scripts/unicode-ranges.py` or document it as legacy.
  - A note in `2026-05-18-bare-lt-as-str.md` or the grammar header about
    the policy: non-ASCII P/S is Str unless it is explicitly markup.

## Open design questions for the user

1. **Minimal patch or subsume the family?**
   - (a) Minimal: add `\p{Ps}\p{Pe}` (non-ASCII) plus the missing Pi/Pf and
     ₰ as another alternative.
   - (b) Replace `PANDOC_VALID_OTHER_PUNCTUATION`, the enumerated
     Sm/Sk/Sc lists, `\p{So}`, `\p{Pd}`, and the non-ASCII part of the smart
     quotes with one class, `[[\p{P}\p{S}]&&[^\x00-\x7F]]`. ASCII handling
     stays exactly as it is.

   My recommendation is (b). It closes the Po, Sc and Sm drift for good. It
   deletes about 40 lines of generated ranges. It matches Pandoc's actual
   rule: anything that is not markup is Str. The cost is that every
   start-of-token `P`/`S` character now shares one alternative, so it needs a
   careful check of interactions with smart quotes (see Risks).
2. **Any non-ASCII punctuation that should *not* be plain Str?** Are there
   characters we deliberately keep out? For example, is anything planned as
   q2-specific syntax, the way `<` and `@` are reserved? I found none in the
   grammar, but you may have plans.
3. **Regression coverage.** Is a curated corpus test enough (one or two
   characters per family plus the CJK set)? Or do you want a full-sweep test
   in CI? The sweep takes about 7 s with a debug pampa, which is probably too
   slow for every run but fine as an `#[ignore]` test or an xtask.
4. **Scope of "common".** With (a), which of the rare families (editorial
   Pi/Pf, Unicode 14+ Po, ₰) do you want included? With (b) they come for
   free.

## Risks / tradeoffs (draft)

- **Smart quotes.** ‘ ’ “ ” are in `startStrRegex` and in the
  continuation. That is what makes `don’t` a single token, and it must
  stay the longest match. A broad P/S alternative only matches one
  character, so longest-match should still prefer the word token. This needs
  confirming with the existing smart-quote and contraction corpus tests.
- **ASCII must not leak in.** The `&&[^\x00-\x7F]` guard handles ASCII
  markup (`< > | ~ ^ $ * _ @ [ ] { }` and so on). The only other question is a
  non-ASCII character that the grammar treats as markup somewhere else. I
  grepped `scanner.c` and found no non-ASCII code point literals, so it
  looks like there are none.
- **Unicode table version** is tree-sitter's, not ours. Unicode 16/17
  characters are accepted only once tree-sitter updates its tables. That is
  harmless.
- **Generated-file churn.** `parser.c` and `grammar.json` will diff heavily.
  Lexer size may shrink with (b), since fewer ranges means fewer states.
