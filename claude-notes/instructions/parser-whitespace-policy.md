# Parser whitespace policy

This is the rule for how qmd's parser and downstream
tree-sitter-AST → Pandoc-AST conversion treat whitespace bytes that
are *not* ASCII space (U+0020), tab (U+0009), or the line endings
(U+000A, U+000D).

## Rule

**Non-ASCII whitespace is content, not whitespace.**

Per Pandoc 3.9.0.2's `markdown` and `commonmark_x` readers, every
codepoint in the Unicode `White_Space=Yes` property *other than* the
ASCII whitespace characters listed above is folded into the
surrounding `Str` node. It is **not** tokenised as whitespace, does
**not** produce a `Space` node, and does **not** produce a hard or
paragraph break (this even covers U+2028 LINE SEPARATOR and U+2029
PARAGRAPH SEPARATOR — Pandoc keeps them as `Str` content).

The set of codepoints currently handled this way:

```
U+00A0 NO-BREAK SPACE
U+1680 OGHAM SPACE MARK
U+2000 EN QUAD
U+2001 EM QUAD
U+2002 EN SPACE
U+2003 EM SPACE
U+2004 THREE-PER-EM SPACE
U+2005 FOUR-PER-EM SPACE
U+2006 SIX-PER-EM SPACE
U+2007 FIGURE SPACE
U+2008 PUNCTUATION SPACE
U+2009 THIN SPACE
U+200A HAIR SPACE
U+2028 LINE SEPARATOR
U+2029 PARAGRAPH SEPARATOR
U+202F NARROW NO-BREAK SPACE
U+205F MEDIUM MATHEMATICAL SPACE
U+3000 IDEOGRAPHIC SPACE
```

U+0085 NEXT LINE is *not* in this list. Its behavior under Pandoc has
not been characterised; if you need to support it, add it to
`PANDOC_NON_ASCII_WHITESPACE` in
`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` and add a
Pandoc experiment row to the original plan doc.

## Where this is enforced

- **Grammar (root):**
  `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js` —
  `PANDOC_NON_ASCII_WHITESPACE` is included in both `startStrRegex`
  and the continuation character class of `PANDOC_REGEX_STR`. After
  any change to that file, run `tree-sitter generate` and commit the
  regenerated `parser.c` and `grammar.json` (but **not** the bundled
  `tree_sitter/array.h` unless that change is intentional — it can
  drift across tree-sitter CLI versions).

- **Block-structural scanner:**
  `crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c` —
  every whitespace check is ASCII-only (`lookahead == ' '`,
  `'\t'`, `'\n'`, `'\r'`). This is correct: Pandoc's block grammar
  is ASCII-defined (indent counting, blank lines, list/heading/fence
  markers, table cells). **Do not** change these to accept Unicode
  whitespace.

- **Downstream extractors in pampa:** sites that strip leading or
  trailing whitespace from a tree-sitter-captured token (delimiters
  for emphasis, quotes, citations, code spans, shortcodes, URI
  autolinks, note references) all use `char::is_ascii_whitespace`
  and `str::trim_ascii*` rather than the Unicode-aware variants.
  Each site has a one-line comment pointing at this doc; if you add
  a new such site, follow the same pattern. **Do not** use
  `char::is_whitespace` or `str::trim` in token-extraction code paths
  — those would silently lose non-ASCII whitespace by stripping it.

- **XML parser (quarto-xml):** XML 1.0 §2.3 defines `S` (whitespace)
  as `(#x20|#x9|#xD|#xA)+`, which is ASCII-only. The parser uses
  `is_ascii_whitespace` and `trim_ascii` accordingly.

## Why

Two reasons, in priority order:

1. **Pandoc compatibility.** qmd aims to be a near-superset of
   Pandoc's commonmark_x. Treating Unicode whitespace differently
   from Pandoc creates round-trip and interop problems.
2. **Real-world inputs.** The Claude.ai web UI emits U+202F before
   AM/PM in timestamps. Users paste Claude transcripts into qmd
   files. The first known repro of this bug (bd-rmx3) was exactly
   this scenario.

## History

- **bd-rmx3 (2026-04-30):** the original bug — pampa rejected U+202F
  in inline text with "unexpected character or token here".
- **bd-8oe4 (2026-04-30):** workspace-wide audit of hand-written
  scanners for the same class of bug; landed alongside the
  root-cause fix.
- **Plan / experiment table:**
  `claude-notes/plans/2026-04-30-unicode-whitespace-handling.md` has
  the Pandoc experiment results that justify the policy.
