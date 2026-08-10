# Investigation findings — reference links / literal brackets

Re-verified at `05c2454e` (main, clean tree), 2026-08-10.
`cargo xtask verify --skip-hub-build` passed before any of this.

## 1. The strand's three shapes reproduce exactly at HEAD

`cargo run --bin pampa -- repro.qmd -t html` (repro.qmd is a copy of the
strand's `index.qmd`):

```html
<p>For more information, see <span>the RedHat documentation</span><span>gcc-toolset</span>.</p>
<p>You may wish to override the mount’s <span><code>noexec</code></span><span>noexec</span> option.</p>
<p>Multiple email support requires Posit Connect <span>Version TBD</span> or later.</p>
<p>Upon an initial user session <span>1</span>, the server creates a cookie
(RSC-XSRF) <span>2</span> containing a random token.</p>
<p>The default subject prefix is “<span>Posit Connect</span>”.</p>
<p><span>gcc-toolset</span>: https://example.com/gcc-toolset
<span>noexec</span>: https://example.com/noexec</p>
```

No diagnostics on stderr; exit status 0. Nothing in the parser has moved
since the strand was filed.

## 2. Detection can be AST-based, not regex-based

This is the most useful finding for whoever writes the rule.

`pampa` already emits, for every `Span`, both the attribute list and a
`SourceInfo` id resolving to a byte range in the original file — and the
range **includes the brackets**. From `mini.qmd`:

```
Text [Version TBD] and [label][ref] and [x]{.cls} and [link](u).

[ref]: https://example.com/r "T"
```

```
{"s":3,  "attr":["",[],[]],      "r":[5,18]}    # [Version TBD]
{"s":10, "attr":["",[],[]],      "r":[23,30]}   # [label]
{"s":12, "attr":["",[],[]],      "r":[30,35]}   # [ref]
{"s":17, "attr":["",["cls"],[]], "r":[…]}       # [x]{.cls}  — has classes
{"s":28, "attr":["",[],[]],      "r":[66,71]}   # [ref] on the definition line
```

So:

- **"Brackets will be eaten"** ⟺ an `Inline::Span` whose `attr` is empty
  (`["",[],[]]`). A real `[x]{.cls}` span carries classes/id/kvs and is
  trivially excluded — no lookahead for `]{`, `](`, or `][` needed.
- **`[label][ref]` adjacency** ⟺ two bare spans whose byte ranges touch
  (`23..30` then `30..35`). Exact, no regex.
- **Inline links and code spans are invisible to this pass** — `[link](u)`
  parses as a `Link`, not a `Span`, and bracket characters inside
  `` `code` `` never produce a `Span`. The AST does the "don't corrupt
  other syntax" work that §3 of the strand assigns to careful regex
  lookahead.

The one thing the AST cannot supply is the definition table: q2 never
collects `[ref]: url` lines, so the rule has to recognize them itself
(they arrive as a `Paragraph` of `Span("ref")` + `Str(":")` + URL text).
Line-shape matching for those, AST for everything else.

## 3. Escaping is idempotent and cross-engine safe — confirmed both directions

`\[escaped\]` produces **no `Span` at all** in q2 (verified in `mini2.qmd`:
nine spans, none covering the escaped text), so a second `convert` pass
cannot re-escape its own output. This matters because `qmd-syntax-helper
convert` iterates rules up to `--max-iterations` (default 10) by default.

Rendering, both engines:

| input | q2 @ 05c2454e | `quarto pandoc` (Q1) |
| --- | --- | --- |
| `\[escaped\]` | `[escaped]` | `[escaped]` |
| `[Version TBD]` | `Version TBD` (brackets gone) | `[Version TBD]` |

Q1 check: `quarto pandoc -f markdown -t html q1probe.md` →
`<p>A [escaped] B [Version TBD] C</p>`.

## 4. New shapes not in the strand description

From `mini2.qmd`:

```
A [label][] B ![alt][ref] C [multi
line] D \[escaped\] E [a][b][c]
```
```html
<p>A <span>label</span><span></span> B <img src="" alt="alt" /><span>ref</span>
   C <span>multi
line</span> D [escaped] E <span>a</span><span>b</span><span>c</span></p>
```

- **`![alt][ref]` is a fourth damage shape.** It does not produce spans for
  the alt half — it produces `<img src="" alt="alt" />`, an image with an
  **empty `src`**, plus a trailing `<span>ref</span>`. A bare `![alt]` with
  no definition does the same. This is worse than the `[…]` cases: the
  output is a broken image element, not just lost text. The strand only
  covers `[…]`; the rule needs an image arm, and it keys off
  `Inline::Image` with an empty target rather than off `Span`.
- **`[label][]` (collapsed)** yields `Span("label")` + an *empty* span for
  the `[]` (range `[9,11]`). Detectable, but the empty span is a distinct
  case from the two-bare-spans-adjacent pattern.
- **Spans cross soft line breaks** (`[multi\nline]`, range `[28,40]`), so
  any escaping edit must be offset-based, not line-based.
- **`[a][b][c]` is genuinely ambiguous** — three adjacent bare spans. Is it
  `[a][b]` + literal `[c]`, or `[a]` + `[b][c]`? Resolvable only by
  consulting which of `b` and `c` have definitions, and left-to-right when
  both do (CommonMark's rule).

## 5. Framework fit

- `qmd-syntax-helper check -r <rule>` already returns one `CheckResult` per
  violation **with a `SourceLocation`**, so the strand's "a `--check` mode
  enumerating every bracket it would escape" is the existing `check`
  subcommand, not new surface. The existing `convert --check` only reports
  a *count* (see `apostrophe_quotes.rs`), which is the weaker of the two.
- `apostrophe_quotes.rs` is the closest existing model: parse with
  `pampa::readers::qmd::read`, collect byte offsets, apply edits in
  **reverse offset order**, write back. It keys off a `Q-2-10` diagnostic;
  this rule keys off AST shape instead, but the edit-application machinery
  transfers unchanged.
- No existing rule walks the pampa AST — they use diagnostics or text. This
  rule would be the first, which is a small new pattern for the crate (not
  a new dependency: `pampa` is already a dep and `read` already returns the
  `Pandoc` value and `ASTContext`).

## Files here

- `repro.qmd` — copy of the strand's repro (the source repo is local-only).
- `mini.qmd` — minimal four-shape probe used for the byte-range table.
- `mini2.qmd` — collapsed/image/multiline/escaped/chained probe.
- `q1probe.md` — the Q1 escaped-bracket cross-check.
