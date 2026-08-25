# knitr's own inline-code pattern (upstream ground truth)

Captured from the local R installation, knitr 1.50:

```
$ Rscript -e 'cat(knitr::all_patterns$md$inline.code)'
(?<!(^``))(?<!(
``))`r[ #]([^`]+)\s*`
```

i.e. with the newline written as an escape:

```
(?<!(^``))(?<!(\n``))`r[ #]([^`]+)\s*`
```

Three defenses, none of which q2's `` `r\s+([^`]+)` `` has:

1. `(?<!(^``))` — the backtick that opens the match must not be the third
   backtick of a fence at start-of-string.
2. `(?<!(\n``))` — same, for a fence at start-of-line.
3. `[ #]` — a *single* space or hash. A newline can never open the
   expression. This is the defense that stops a 4+-backtick fence, which
   neither lookbehind catches.

Also captured, for reference — knitr's executable-chunk opener, which is
what `` ```{r} `` matches and which tolerates whitespace before the brace:

```
^[\t >]*```+\s*\{([a-zA-Z0-9_]+( *[ ,].*)?)\}\s*$
```

## Why we should NOT port knitr's pattern literally

Rust's `regex` crate has no lookbehind, so the lookbehinds have to be
re-expressed anyway. The natural re-expression is Quarto 1's idiom from
`quarto-cli/src/core/execute-inline.ts`:

```js
new RegExp("(^|[^`])`{" + language + "}[ \t]([^`]+)`", "g")
```

— capture the preceding character and re-emit it. That form is *strictly
stronger* than knitr's two lookbehinds: it rejects a backtick prefix
anywhere, not just at line start. Measured difference on the strand's
`yaml-title/` fixture (a `` ```r `` inside a front-matter scalar, mid-line):

| pattern | matches the YAML fence? |
|---|---|
| q2 today (`` `r\s+ ``) | yes — fatal |
| knitr upstream | **yes** — knitr itself has this hole |
| `(^\|[^`])` port | no |

So the `(^|[^`])` guard is not merely a port; it fixes a case knitr
upstream still gets wrong. See `regex-candidates.out` for the full run.
