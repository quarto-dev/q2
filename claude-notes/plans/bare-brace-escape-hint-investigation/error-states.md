# Parser error-state captures for bare-brace inputs

Captured 2026-08-09 at `main` @ `ec8a35f9` via:

```
printf '<input>\n' | cargo run --bin pampa -- --_internal-report-error-state | jq '.errorStates'
```

| Input | Error state(s) `(state, sym)` | Notes |
| --- | --- | --- |
| `a {guid} b.` | `(2613, _language_specifier_token)` | Bare brace run in prose — the strand's primary case |
| `the request returns the task {guid} immediately.` | `(2613, _language_specifier_token)` | Strand's verbatim example, same state |
| `a {guid} b and {other} c.` | `(2613, _language_specifier_token)` ×2 | Two runs, same state each time |
| `empty {} braces.` | `(2613, _language_specifier_token)` | Empty braces, same state |
| `multi {two words} braces.` | `(2613, _language_specifier_token)` | Multi-word content, same state |
| `[text]{guid} attr-like.` | `(2613, _language_specifier_token)` | **Attribute-intent typo hits the same state** — hint wording must serve this reader too |
| `see [the {guid} link](https://example.com) here.` | `(2589, _language_specifier_token)` | Brace run inside link text — second state to map |
| `trailing {guid` (unclosed, EOL) | `(2613, shortcode_name)` | Different lookahead sym — separate mapping decision |
| `[text]{#id unclosed.` | `(2705, shortcode_name)` | Unclosed attribute — out of scope for this strand |

## Collision check against `_autogen-table.json`

- `state == 2613`: only claimed as `(2613, _close_block)` → `Q-2-2/simple`. **`(2613, _language_specifier_token)` is free.**
- `state == 2589`: no entries. **Free.**
- `sym == _language_specifier_token`: only claimed at `(2638, …)` → `Q-2-36/bare-label`. No overlap.

Conclusion: both target `(state, sym)` pairs are unclaimed; a pure corpus
addition (Q-2-36 "path B" mechanism) maps them without touching grammar,
scanner, or the fallback code in
`crates/quarto-parse-errors/src/error_generation.rs`.

## Observed fallback output at HEAD (`repro.qmd`)

```
Error: Parse error
   ╭─[ repro.qmd:1:31 ]
   │
 1 │ the request returns the task {guid} immediately.
   │                               ──┬─
   │                                 ╰─── unexpected character or token here
───╯
```

Highlight lands on the word *inside* the braces (`guid`), not on the brace
run — relevant to the highlight-widening design question.
