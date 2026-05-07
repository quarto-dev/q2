# Issue #161 — qmd writer doubles backslashes in attribute values

- **Reporter:** @rundel (Colin Rundel)
- **Filed:** 2026-05-06
- **GH URL:** https://github.com/quarto-dev/q2/issues/161
- **Triage branch:** `issue-161` at `.worktrees/issue-161/`
- **HEAD at triage:** rebased onto `eefff6e1` (post-#163, the #160 fix). Bug re-verified at this HEAD; the #160 fix did not touch the same code path.

## Summary

Round-tripping a div with backslash-escaped characters in an attribute
value is **not stable**: each round trip *doubles* the backslashes.

```
input:                     ::: {data-foo="\[1,2\]"}
after qmd→native:          data-foo = "\[1,2\]"          (correct: backslashes preserved)
after native→qmd:          ::: {data-foo="\\[1,2\\]"}    (writer: doubled)
after second qmd→native:   data-foo = "\\[1,2\\]"        (reader: literal two backslashes)
```

User-visible impact: any qmd file containing `tbl-colwidths="\[N,N\]"`
(used widely in `quarto-web`) accumulates an extra backslash per round
trip. Reporter linked four occurrences in the docs site.

## Reproduced at HEAD

Repro file: `repro.qmd` (35 bytes, exact bytes from the issue).
Round-trip output: `round-trip-1.qmd`. Both committed alongside this
doc.

```bash
$ cargo run --bin pampa -- claude-notes/issue-reports/161/repro.qmd
[ Div ( "" , [] , [("data-foo", "\\[1,2\\]")] ) [Para [Str "hello"]] ]

$ cargo run --bin pampa -- -t qmd claude-notes/issue-reports/161/repro.qmd
::: {data-foo="\\[1,2\\]"}
hello
:::

$ cargo run --bin pampa -- -t qmd claude-notes/issue-reports/161/repro.qmd \
    | cargo run --bin pampa --
[ Div ( "" , [] , [("data-foo", "\\\\[1,2\\\\]")] ) [Para [Str "hello"]] ]
```

Bug reproduces exactly as reported.

## Comparison against Pandoc

Pandoc treats backslash inside a double-quoted attribute value as a
generic escape: `\X` → `X` for any `X`.

```bash
$ printf -- '::: {data-foo="\\[1,2\\]"}\nhello\n:::\n' | pandoc -f markdown -t native
... ( "" , [] , [ ( "data-foo" , "[1,2]" ) ] ) ...

$ printf -- '::: {data-baz="a\\\\b"}\nhi\n:::\n' | pandoc -f markdown -t markdown
::: {data-baz="a\\b"}                       # one literal backslash → \\ on write

$ printf -- '::: {data-q="a\\"b"}\nhi\n:::\n' | pandoc -f markdown -t markdown
::: {data-q="a\"b"}                          # one literal quote → \" on write
```

So Pandoc's contract is:
- **read:** `\X` (any `X`) is a backslash escape; emit `X` only.
- **write:** the only characters that need escaping inside a `"..."`
  attribute value are `\` itself and `"`; both are escaped with a
  leading `\`. Brackets are *not* escaped.

## Where the bug is in our code

The asymmetry is on the **reader** side, not the writer.

- **Writer (correct, matches Pandoc):**
  `crates/pampa/src/writers/qmd.rs:392-394` —
  ```rust
  fn escape_quotes(s: &str) -> String {
      s.replace('\\', "\\\\").replace('"', "\\\"")
  }
  ```
  This emits `\` as `\\` and `"` as `\"`, matching Pandoc's writer.

- **Reader (incomplete, does not match Pandoc):**
  `crates/pampa/src/pandoc/treesitter_utils/text_helpers.rs:25-37` —
  `extract_quoted_text` un-escapes only `\"` (in `"..."`) or `\'` (in
  `'...'`). It does not un-escape `\\`, and it does not un-escape any
  other `\X` (e.g. `\[`, `\]`). So a written `\\` survives as two
  backslashes on the next read, and a user-typed `\[` survives as `\[`
  rather than collapsing to `[`.

`extract_quoted_text` is also used at `treesitter.rs:980` for link
titles (`"..."` after a URL), so any fix needs to reason about whether
the same escaping rules apply there. Pandoc's CommonMark reader does
treat `\X` as escapes inside link titles, so the same fix likely
applies, but link-title round-tripping is **out of scope for this
triage** — confirm before assuming.

## Scope decision

In scope: backslash handling in **div / span / code-block /
inline-attribute** values (everything that flows through
`key_value_value`). Out of scope: link titles (different surface, same
helper — needs its own check).

## Suggested fix shape (not implemented)

Update `extract_quoted_text` so that, inside the quotes, **any** `\X`
collapses to `X` (Pandoc-style generic backslash escape). The writer
does not need to change.

A regression test belongs at
`tests/roundtrip_tests/qmd-json-qmd/` (per
`crates/pampa/CLAUDE.md` § "When fixing roundtripping bugs"). Cover at
least:
1. `data-foo="\[1,2\]"` (the reported case — round-trips to
   `data-foo="[1,2]"`, then stable),
2. `data-baz="a\\b"` (literal backslash — round-trips to itself),
3. `data-q="a\"b"` (escaped quote — already works; lock it in).

Whoever picks this up should TDD: write the round-trip test, confirm
it fails, then change `extract_quoted_text`.

## Related issues

- **#160** ("qmd writer drops `=` from raw block fence") — different
  bug, neighboring area (qmd writer attribute serialization). Not a
  duplicate; should be tracked separately. The reporter's comment
  thread on #161 references "this and #161" which appears to be a
  typo for "this and #160" — the OP author is the same and the comment
  about a "deeper bit of confusion around `\` in strings" is
  consistent with cross-issue reflection.

## Outcome

Filed as **bd-tpjg** (priority 1, bug). Implementation TBD by whoever
picks it up — TDD per `crates/pampa/CLAUDE.md`.
