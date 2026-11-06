---
title: 'Angle-bracket link destinations (`[x](<a b.png>)`)'
date: 2026-11-06
description: 'Support CommonMark pointy-bracket link destinations so targets may contain spaces, matching Pandoc''s AST.'
status: done  # implemented; PR open
braid:
  strand: bd-angle-link-dest-qkqkegqo
  priority: P2
  labels: [parser, grammar]
---

Related: bd-m20x2zrp (Q-2-31 misfires on braces in link/image URLs);
bd-z4lqspcd (Q-2-33 inline-container coverage, discovered here).

## Goal

Accept the CommonMark/Pandoc *pointy-bracket link destination* form:

```markdown
![](<a path with spaces.png>)
[link](<foo bar.html> "title")
```

and produce the same AST Pandoc produces.

## Reproduction (on `main` @ 52fcb5148)

| Input | pampa today | pandoc 3.12.1 |
|---|---|---|
| `![](<a path with spaces.png>)` | **Q-2-33** "Spaces in link targets" | `Image … ("a%20path%20with%20spaces.png", "")` |
| `[link](<foo bar.html> "title")` | **Q-2-33** | `Link … ("foo%20bar.html", "title")` |
| `![](<nospace.png>)` | parses, but URL is `"<nospace.png>"` (**brackets leak**) | `"nospace.png"` |
| `[x](<>)` | URL `"<>"` | URL `""` |
| `[x](<a\>b.html>)` | URL `"<a\\>b.html>"` | `"a%3Eb.html"` |

So there are two bugs, not one: the space case is a parse error, and the
space-free case is a silent wrong answer. The silent one is arguably worse.

## Diagnosis

`crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:445`:

```js
target: $ => seq(
    /[ \t]*[\]][(]/,
    optional($._inline_whitespace),
    alias(repeat(choice(/[^ {\t)]|(\\.)+/, $.shortcode)), $.url),
    optional(seq($._inline_whitespace, alias($._commonmark_double_quote_string, $.title))),
    ')'
),
```

The URL is a repeat of single non-space characters, and `<` has no special
meaning in it. The parser trace (`pampa -v`) for `![](<a path.png>)`
shows `<`, `a` lexed as `target_token2` (the URL char regex). The URL ends at
the first space, and the parser then expects a `"title"` but finds `path`. The
resulting ERROR state maps to Q-2-33 in the error table. The external
scanner's `<` handling (bd-j9cf, `html_element` / `_pandoc_literal_str`)
does not get involved here, because those symbols are not valid in the
`target` state. The trace shows `lex_external` declining.

On the Rust side, `treesitter.rs:1409` copies the `url` node text verbatim, and
`span_link_helpers.rs::process_target` passes it through. That is why
`<nospace.png>` keeps its brackets.

## Pandoc semantics to match

Measured against pandoc 3.12.1:

- Contents between `<` and `>` become the URL, with backslash escapes
  processed.
- Then Pandoc's `escapeURI` runs: whitespace and characters such as `"`, `<`, `>`, `{`,
  `}`, `|`, `^` and `` ` `` become `%XX`. Non-ASCII stays as it is (`<é ü.png>` → `é%20ü.png`).
  Parentheses stay as they are (`<a(b) c.png>` → `a(b)%20c.png`).
- A title may follow the closing `>` after whitespace: `(<a b> "t")`.
- Pandoc is more lenient than CommonMark. It accepts a newline inside the brackets
  (`<a⏎b>` → `a%20b`) and an unescaped `<`. CommonMark forbids both.
- `<>` gives an empty URL.

Downstream already handles percent-encoded paths:
`quarto-core/src/pandoc_request/resources.rs::percent_decode` and
`transforms/link_resource_discovery.rs` decode `%20` before resolving files. So
`a%20path.png` resolves to `a path.png` on disk without any further changes.

## Proposed design

### Grammar (tree-sitter-qmd)

Add a second form for the URL inside `target`. Use a **single lexical
token**, not `seq('<', …, '>')`:

```js
target: $ => seq(
    /[ \t]*[\]][(]/,
    optional($._inline_whitespace),
    choice(
        alias($._angle_url, $.url),                       // new
        alias(repeat(choice(/[^ {\t)]|(\\.)+/, $.shortcode)), $.url),
    ),
    optional(seq($._inline_whitespace, alias($._commonmark_double_quote_string, $.title))),
    ')'
),
_angle_url: $ => token(prec(1, seq('<', /([^<>\\\n\r]|\\.)*/, '>'))),
```

Why a single token: tree-sitter's lexer takes the longest match. For `<a b.png>`
the whole token is longer than the 1-char URL regex, so it wins. For an
*unclosed* `<foo)`, the token cannot match, so lexing falls back to today's
char-by-char URL. That keeps current behaviour for every input that doesn't use
the new form. A `seq('<', …)` at the grammar level would instead commit to
`'<'` as a literal (string literals beat regexes on ties) and turn
`[x](<foo)` into an error.

Both forms keep the node name `url`, so `queries/highlights.scm` (lines 58, 64)
and any `(target (url))` consumers keep working without changes.

### Rust (pampa)

In `treesitter.rs` `"url"` arm (or in `process_target`):

1. If the text starts with `<` and ends with `>`, strip them and narrow
   `url_range` by one byte on each side. The source span must then point at the
   content, matching how `title` already records content provenance.
2. Process backslash escapes inside the brackets.
3. Apply a Pandoc-compatible `escape_uri` to the result of the angle form
   (whitespace and the set above → `%XX`; leave non-ASCII and parens alone).
   Put it in a small helper with unit tests that mirror the table above.

### Diagnostics

- Update the Q-2-33 message/notes (`error_catalog.json`,
  `resources/error-corpus/Q-2-33.json`) to suggest the angle form as well:
  "…replace spaces with `%20`, or wrap the target in `<…>`."
- Regenerate the error table (`crates/pampa/scripts/build_error_table.ts`),
  because grammar changes renumber parse states. Then confirm that the existing Q-2-33 cases
  (`![](image file.png)` etc.) still report Q-2-33, and that the `Q-2-31` / brace
  hint tests still pass (see `crates/pampa/AGENTS.md` on state-keyed errors).

### Writer round-trip

The qmd writer emits targets verbatim. With option A below, URLs never contain raw
spaces, so `<…>` round-trips to `%20` form, which is valid and stable, and the writer needs
no changes. Add a round-trip test to lock this in.

## Decisions (resolved 2026-11-06)

- **D1 → A.** Pandoc-compatible percent-encoded URL in the AST. Matching
  Pandoc is the general principle.
- **D2 → CommonMark-strict.** No newline or unescaped `<` inside `<…>`.
  Where CommonMark is stricter than Pandoc, follow CommonMark.
- **D3 → angle form only.** `escape_uri` applies only to `<…>` destinations.
- **D4 → no shortcodes** inside `<…>`.

The other bugs in "Out of scope" are for a separate session.

### Original options considered

**D1. What URL string goes in the AST?**
- **A (recommended): Pandoc-compatible.** `a%20path.png`. This gives AST parity with
  Pandoc, which our comparison tests rely on. Downstream decoding already exists, and the writer
  needs no changes.
- B: Store the decoded `a path.png`. This is friendlier for Lua filters, but diverges
  from Pandoc, forces the qmd writer to emit `<…>` whenever a URL has a space, and
  risks double-decoding in `percent_decode`.

**D2. Strict CommonMark or lenient Pandoc inside the brackets?**
- **Recommended: CommonMark-strict.** No newline, no unescaped `<`. A newline inside an
  inline token would have to cross block-continuation prefixes (`> `, list
  indent), which is where this grammar has been bitten before (bd-ilv8p).
  Rejected inputs fall back to today's parse, so they produce a diagnostic, not a silent
  wrong answer.

**D3. Should `escape_uri` also apply to bare (non-angle) URLs?**
Pandoc applies it to every destination. Today we pass bare URLs through
verbatim, and bare URLs can't contain spaces anyway. **Recommended: angle form
only for now.** Track parity for the bare form separately, together with the next item.

**D4. Shortcodes inside angle URLs (`<{{{< meta img >}}} x.png>`)?**
**Recommended: not supported.** The angle token is opaque. Note this in docs.

## Out of scope (separate strands if wanted)

- Backslash escapes in **bare** URLs are not decoded: `[x](a\)b)` gives
  `"a\\)b"`, while Pandoc gives `"a)b"`. This divergence predates this work.
- bd-m20x2zrp: braces in bare URLs mis-diagnosed as Q-2-31.
- `qmd-syntax-helper` Q-2-33 converter: optionally offer `<…>` instead of
  `%20`. Leave `%20` as the default.

## Work items

- [x] Grammar: add `_angle_url` token + `choice` in `target`. Then `tree-sitter generate`.
- [x] Tree-sitter corpus tests (`tree-sitter-markdown/test/corpus/`): angle URL
      with/without spaces, with title, with attrs `(<a b>){.c}`, empty `<>`,
      escaped `\>`, unclosed `<foo)` (falls back), newline inside (rejected).
- [x] pampa: strip brackets + narrow source range, process escapes, add `escape_uri`
      helper + unit tests.
- [x] pampa integration tests (`tests/integration/`): native output equals
      Pandoc's for the reproduction table; check the source-location span of the URL;
      qmd round-trip.
- [x] Q-2-33 message/notes update; rebuild error table; run the error-corpus tests.
- [x] End-to-end check: render a doc with `![](<img with space.png>)`. The image
      is found/copied (HTML) and resolves for typst output.
- [x] Docs: mention the syntax wherever link-target spaces are documented
      (Q-2-33 docs page).
- [x] `cargo xtask verify`.

## Implementation notes (2026-11-06)

What differed from the plan, and what the plan missed:

- **Telling the two `url` forms apart.** The plan assumed the Rust side could
  check `node.grammar_name()`. It can't: tree-sitter *renames* a token that is
  only ever used under an alias, so the generated symbol is just `"url"`. The
  grammar now tags the angle form with a field,
  `field('angle_bracket_url', alias($._angle_bracket_url, $.url))`, and
  `treesitter.rs` checks `parent.child_by_field_name("angle_bracket_url")`.
  Corpus tests show the field (`angle_bracket_url: (url)`).
- **New diagnostic Q-2-59** ("Text after angle-bracket link destination").
  `[x](<a>b>)`, `![](<my file>.png)` and `[x](<a b> c)` used to parse as a
  (wrong) bare-URL link on `main`. With the angle token they became an uncoded
  "Parse error". CommonMark and Pandoc say these aren't links at all. Q-2-59 has
  error-corpus cases for **every inline container** (both shapes: text glued
  to `>`, and an unquoted title after a space), following the coverage-trap
  guidance in `crates/pampa/AGENTS.md`. The reconciliation test is
  `tests/integration/test_angle_bracket_link_destination.rs`. Superscript plus
  a space is exempt: it is correctly Q-2-9.
- **A newline inside `<...>`** is reported by the existing Q-2-37 ("Line
  break in link destination"). No new case was needed.
- **The resource copy collector did not percent-decode image URLs.** This was
  found in the end-to-end check, and it already happens on `main` with a
  hand-written `![](a%20b.png)`. `ResourceCollectorTransform::collect_resource`
  joined the raw URL onto the doc dir, so `figs/sales%20chart.png` produced a
  false Q-5-6 "referenced file does not exist" warning, and in a website the
  image was **not copied to `_site`**. The plan's assumption that "downstream
  already decodes" held only for `link_resource_discovery` and
  `pandoc_request/resources.rs`. The fix decodes in `collect_resource`, which also fixes
  the `q2 preview` single-file asset sync (`collect_referenced_asset_urls`).
  The Typst output was already fine (Pandoc's Typst writer unescapes image paths).
- **Discovered, filed separately:** bd-z4lqspcd — Q-2-33 has the same
  inline-container coverage gap (uncoded inside emphasis, quotes, table cells,
  etc.). This already happens on `main`.
- Docs: `docs/errors/markdown/Q-2-33.qmd` (angle form as a fix),
  new `Q-2-59.qmd` (+ sidebar entry), and the `%20` paragraph in
  `docs/guides/projects/paths.qmd`.
