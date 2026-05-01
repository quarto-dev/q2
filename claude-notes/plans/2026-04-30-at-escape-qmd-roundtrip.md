# QMD writer: missing `@` escape causes Str → Cite re-parse

**Beads:** bd-21gu
**Source:** [issue #150](https://github.com/quarto-dev/q2/issues/150), item 1

## Bug

When a `Str` node's text starts with `@` followed by an identifier character,
the qmd writer emits the `@` unescaped. On the next parse pass, the qmd reader
turns it into a `Cite`, so the round-trip is lossy.

Reporter's repro (link content):

```
$ printf 'See [\\@jjallaire](https://github.com/jjallaire/) for details.\n' \
    | cargo run --bin pampa -- -t native
[ Para [Str "See", Space,
        Link ( "" , [] , [] ) [Str "@jjallaire"]
             ("https://github.com/jjallaire/" , ""),
        Space, Str "for", Space, Str "details."] ]

$ printf 'See [\\@jjallaire](https://github.com/jjallaire/) for details.\n' \
    | cargo run --bin pampa -- -t qmd
See [@jjallaire](https://github.com/jjallaire/) for details.
```

The bug is **not** specific to link content. It also reproduces in plain
inline context, which broadens the fix scope:

```
$ printf 'See \\@jjallaire please.\n' | cargo run --bin pampa -- -t native
[ Para [Str "See", Space, Str "@jjallaire", Space, Str "please."] ]

$ printf 'See \\@jjallaire please.\n' | cargo run --bin pampa -- -t qmd
See @jjallaire please.
```

In both cases, parsing the writer's output produces a `Cite`, not the
original `Str`, so `qmd → ast → qmd → ast` is non-idempotent.

## Root cause

`crates/pampa/src/writers/qmd.rs:1226-1253` — `escape_markdown` deliberately
omits `@` from its escape set:

```rust
// Characters that don't need escaping in most contexts:
// . , - + ! ? @ = : ; / ( ) { } % & ' "
// These are only special in very specific contexts and escaping them
// everywhere would make output unnecessarily verbose.
_ => result.push(ch),
```

`@` *does* need escaping in a specific context: when followed by an
identifier character, because the qmd citation grammar treats `@<ident>` as
the start of a `Cite`. The current writer ignores this trigger condition,
so any `Str` whose body looks like a citation key is silently re-parsed.

## Fix strategy

**Always escape `@` in `Str` content** — no lookahead required.

Empirical verification of the parser's behavior (run during planning):

| Input          | Parse result                                                     |
|----------------|------------------------------------------------------------------|
| `\@foo`        | `Str "@foo"` ✓                                                   |
| `\@`           | `Str "@"` ✓                                                      |
| `foo\@bar`     | `Str "foo@bar"` ✓                                                |
| bare `@foo`    | `Cite[@foo]` (citation)                                          |
| bare `@.`      | **parse error** ("unexpected character")                         |
| bare `@ `      | **parse error**                                                  |
| bare `@<eol>`  | **parse error**                                                  |

The scanner unconditionally dispatches `@` to `parse_cite_author_in_text`
(`crates/tree-sitter-qmd/tree-sitter-markdown/src/scanner.c:2197`), which
emits a citation token; the citation grammar then either matches an
identifier (yielding a `Cite`) or fails. There is no "fallback"
production in which a bare `@` inside `Str` content is legal. Therefore:

- If a `Str`'s body contains `@` followed by alnum/`_`/`{`, the
  unescaped emit re-parses as a `Cite` (the original bug).
- If a `Str`'s body contains `@` followed by anything else, the
  unescaped emit fails to parse at all.
- Either way, the writer must escape every `@`.

Implementation: a single new arm in `escape_markdown` —

```rust
'@' => result.push_str("\\@"),
```

No lookahead, no peekable iterator, no position context. The character-
by-character loop stays as it is.

Negative-test fixtures from the original plan
(`at_no_escape_at_end.qmd`, `at_no_escape_before_space.qmd`) were
deleted: they probed for over-escape of bare `@`, but bare `@` is not
parseable input in the first place, so the round-trip test would fail to
read the original.

## Test plan (TDD — write tests first, watch them fail, then fix)

All tests live in `crates/pampa/tests/roundtrip_tests/qmd-json-qmd/`, picked
up automatically by `test_qmd_roundtrip_consistency` in
`crates/pampa/tests/test.rs:704`.

- [x] Add `at_escape_in_link_text.qmd` with the reporter's exact input:
      `See [\@jjallaire](https://github.com/jjallaire/) for details.`
- [x] Add `at_escape_plain_inline.qmd` with `See \@jjallaire please.`
- [x] Add `at_escape_in_emphasis.qmd` with `*\@user* mention.` to cover
      another container.
- [~] ~~Add `at_escape_brace_form.qmd` with `See \@{some-key} please.`~~
      Removed: bare `{...}` in inline text is itself a parse error
      independent of the `@` escape, so the brace-citation form can't
      currently be round-tripped end-to-end. Tracked separately as
      bd-tpve (discovered-from bd-21gu).
- [~] ~~Negative cases (`at_no_escape_at_end.qmd`,
      `at_no_escape_before_space.qmd`).~~ Removed: bare `@` is
      *always* a parse error in qmd `Str` context (verified — see
      "Fix strategy" table), so over-escape of `@` is not a concern.
- [x] Add `at_escape_trailing.qmd` (`End with \@ here.`) —
      verifies the simpler always-escape rule handles end-of-Str
      positions cleanly.
- [x] Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency` and
      confirm the four positive tests fail in the expected way before
      any code change. Verified: all four fixtures' writer output drops
      the `\@` escape; first three re-parse as `Cite` (wrong AST), the
      `at_escape_trailing` case re-parses as a hard parse error.

## Implementation steps

- [x] Add the failing test fixtures listed above.
- [x] Run `cargo nextest run -p pampa test_qmd_roundtrip_consistency` —
      confirm failure mode matches the bug report.
- [x] Modify `escape_markdown` in `crates/pampa/src/writers/qmd.rs` to
      always escape `@`. Single-line addition; revised comment block.
- [x] Re-run the roundtrip test; all four fixtures pass.
- [x] Run `cargo nextest run --workspace` — 7610 passed, 0 failed,
      no snapshot changes.
- [x] No snapshot diffs to report.
- [x] Run `cargo xtask verify --skip-hub-build` — passed.
- [x] Manual end-to-end: reporter's case
      `printf 'See [\@jjallaire](https://github.com/jjallaire/) for details.\n'`
      now writes `See [\@jjallaire](...)` and re-parses to the same AST
      as the original. Confirmed inspecting both writer output and the
      regenerated AST.
- [ ] Update beads: claim done; close bd-21gu after Phase 2 audit
      (single beads close so the implementation+audit ship together).
- [ ] `br sync --flush-only && git add .beads/` — at session end.

## Phase 2 — Audit other writer-escape gaps

After the `@` fix lands, do a systematic sweep for the same class of bug:
**characters that the parser consumes as escape-introducers (producing a
`Str` whose body would re-trigger syntax if written back unescaped).**

Approach:

- [ ] Enumerate every char the inline + block grammars treat as
      escape-significant. Source of truth: the inline tree-sitter grammar
      under `crates/tree-sitter-qmd/` and the parser's escape handling.
- [ ] For each candidate char `C`:
      1. Construct a minimal qmd input where `\C…` parses to a `Str`
         whose text contains `C…`.
      2. Re-parse that `Str`'s text in isolation (or in the same context)
         and check whether it round-trips. If it parses to anything other
         than the same `Str`, the writer must escape `C` in that context.
      3. Cross-check against the `escape_markdown` arm list at
         `crates/pampa/src/writers/qmd.rs:1226`. The "intentionally not
         escaped" comment block (`. , - + ! ? @ = : ; / ( ) { } % & ' "`)
         is the prime suspect set.
- [ ] Likely-suspect characters to probe explicitly:
      - `!` followed by `[` → image syntax
      - `(` after a closing `]` → link target
      - `{` followed by `.`/`#`/word → attribute span
      - `:` in line-leading position → fenced div / definition list
      - `-`, `+`, `*` in line-leading position → list markers (these
        already escape `*`; check `-` and `+`)
      - `.` after a digit at line start → ordered list marker
      - `=` and `-` on a line by themselves following text → setext header
      - `'` `"` near smart-quote boundaries (already partially handled
        via `reverse_smart_quotes`; verify completeness)
- [ ] For each genuine bug found, add a dedicated round-trip fixture under
      `tests/roundtrip_tests/qmd-json-qmd/` and either fold the fix into
      `escape_markdown` (with appropriate lookahead/position context) or
      open a follow-up beads issue if the fix is non-trivial (e.g.
      requires position-aware escaping the current char-by-char helper
      can't express).
- [ ] Summarize findings in this plan document under a "Phase 2 results"
      section, with one-line entries per char probed: char, verdict
      (clean / bug / needs-position-context), test fixture or follow-up
      beads ID.

The audit's deliverable is either (a) a clean bill of health for each
char, or (b) one beads issue per remaining bug class with reproducers.

## Phase 2 results

Method: for each candidate char `C`, generate three inputs (`a\Cb`,
`\Cword`, `\Cword`+other-positions), confirm `\C` parses to a `Str`
containing `C`, write it, and check the re-parse equals the original.

| Char | Mid-Str (`a\Cb`) | Line-start (`\Cword`) | Verdict |
|------|------------------|-----------------------|---------|
| `.`  | OK               | OK                    | clean   |
| `,`  | OK               | OK                    | clean   |
| `-`  | OK               | OK *(see note 1)*     | clean (line-start `- ` separately handled) |
| `+`  | OK               | OK                    | clean   |
| `!`  | OK               | OK                    | clean   |
| `?`  | OK               | OK                    | clean   |
| `=`  | OK               | OK                    | clean   |
| `:`  | OK               | **DIFF**              | bug — line-start fenced-div trigger |
| `;`  | OK               | OK                    | clean   |
| `/`  | OK               | OK                    | clean   |
| `(`  | OK               | OK                    | clean   |
| `)`  | OK               | OK                    | clean   |
| `{`  | **DIFF**         | **DIFF**              | bug — fix this plan |
| `}`  | **DIFF**         | **DIFF**              | bug — fix this plan |
| `%`  | OK               | OK                    | clean   |
| `&`  | OK               | OK                    | clean   |
| `'`  | OK *(note 2)*    | **DIFF** *(note 2)*   | bug — needs position context |
| `"`  | **DIFF** *(note 3)* | **DIFF** *(note 3)* | bug — needs position context |

Additional findings:

- **`Str "1."` followed by `Space` then `Str "foo"`** (e.g. from JSON
  input) writes as `1. foo`, which re-parses as an ordered list. The
  `.` in `1.` needs escape when `1.` is at line-start and followed by
  whitespace. Position-dependent (requires inline-sequence + line-start
  context). **Bug.**

Notes:
1. `-` mid-Str and as a standalone Str is fine. The `- ` line-start list
   marker case parallels `1. ` and is the same class of position-aware
   bug — verify under the same fix.
2. Mid-word apostrophe (`a'b`) round-trips cleanly because the parser
   smart-quotes it to `’` and `reverse_smart_quotes` reverses it on
   write. Unbalanced `'` at start/end of word ("'a", "a'") fails even
   on the **original** parse — so it's not a writer bug; the only way a
   `Str` body acquires a literal ASCII `'` is via JSON input, which the
   writer would currently emit unescaped and produce parse errors on
   re-parse.
3. Same shape as `'`. Balanced `"foo"` becomes a `Quoted` node
   (different writer path); literal `"` in a `Str` body could only come
   from JSON.

### Decisions for this PR

- **Always escape `{` and `}` in `escape_markdown`.** Parallel to the
  `@` fix: bare `{`/`}` is universally a parse error in `Str` context,
  and `\{` / `\}` always parse to literal `{` / `}`. Add round-trip
  fixtures.
- **Defer position-dependent escapes** (`:`, `1.`/`-`/`+` line-start
  list markers, unbalanced `'`/`"`). Open one consolidated follow-up
  beads issue: the fix mechanism for all of these is the same — the
  writer needs line-start tracking and lookahead across adjacent inline
  nodes — which is a larger refactor than `escape_markdown`.

### Phase 2 implementation steps

- [x] Audit chars; record table above.
- [x] Add roundtrip fixtures for `{` and `}`
      (`brace_escape_in_str.qmd`, `brace_escape_at_word_boundary.qmd`).
- [x] Run roundtrip test, confirm new fixtures fail.
- [x] Add `'{' => result.push_str("\\{")` and `'}' => result.push_str("\\}")`
      to `escape_markdown`.
- [x] Re-run; all roundtrip fixtures pass; full workspace
      (7610 tests) passes; xtask verify --skip-hub-build passes.
- [x] Open follow-up beads issue for position-dependent escapes:
      **bd-kk0a** (covers `:` line-start, unbalanced `'`/`"`, and the
      `1.`/`-`/`+` line-start list-marker case).

## Resolved decisions

User decisions from 2026-04-30 review:

- **Include `{` in the trigger set.** Round-tripping must work for the
  `@{...}` bracketed citation form too, so `{` is a citation-key starter.
- **ASCII-only alphanumerics.** Verified against the inline grammar's
  citation regex at
  `crates/tree-sitter-qmd/tree-sitter-markdown/grammar.js:618`:
  `[0-9A-Za-z_]+([:.#$%&+?<>~/-][0-9A-Za-z_]+)*`. The scanner
  (`scanner.c:1873 parse_cite_author_in_text`) emits the `@` token on a
  bare `@`, and the grammar's ASCII-only regex decides whether it's a
  real citation. So the lookahead test is **`is_ascii_alphanumeric()`
  || `_` || `{`** — not the Unicode-aware `is_alphanumeric`.
- **Code spans / verbatim are not affected.** `escape_markdown` is only
  invoked from `write_str` (`crates/pampa/src/writers/qmd.rs:1255`),
  which only runs on `Str` inlines. Code/verbatim go through their own
  writers and are out of scope.

Updated implementation sketch:

```rust
let mut chars = text.chars().peekable();
while let Some(ch) = chars.next() {
    match ch {
        '@' => {
            let needs_escape = matches!(
                chars.peek(),
                Some(c) if c.is_ascii_alphanumeric() || *c == '_' || *c == '{'
            );
            if needs_escape {
                result.push_str("\\@");
            } else {
                result.push('@');
            }
        }
        // ... existing arms unchanged
    }
}
```

Add a fixture covering the `@{...}` form
(`at_escape_brace_form.qmd`: `See \@{some-key}.`) on top of the
others listed in "Test plan" above.
