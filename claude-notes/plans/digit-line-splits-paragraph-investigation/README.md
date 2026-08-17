# Investigation fixtures — bd-digit-line-splits-paragraph-w6tod0gh

Minimal repros captured at `fc2895b2` (v0.17.0 + docs commit). Run each with:

```bash
cargo run -q --bin pampa -- <file>.qmd -t html
```

Compare against `pandoc -f commonmark -t html <file>.qmd`, which is the
conformance target (see the plan's "Which spec are we targeting" section —
q2 already matches CommonMark, not pandoc-markdown, on list interruption).

## `digits.qmd` — the core symptom

A digit-leading continuation line ends the paragraph.

| input | q2 @ HEAD | expected |
| --- | --- | --- |
| `...leases last` / `30 minutes...` | two `<p>` | one `<p>` |
| `...greater than` / `1000, the...` | two `<p>` | one `<p>` |
| `...leases last` / `thirty minutes...` | one `<p>` ✓ | one `<p>` |

The third block is the control: a non-digit first character stays in the
paragraph. This is a line-start character class, not a mis-parsed marker.

## `linkwrap.qmd` — the fatal variant

When the wrap falls inside link text, the block ends before the closing `]`
and the render fails outright:

```
Error: [Q-2-1] Unclosed Span
  I reached the end of the block before finding a closing ']' for the span or link.
```

Reproduced at HEAD. This is the variant that makes the bug a build-breaker
rather than a cosmetic split.

## `interrupt.qmd` — the interruption matrix

Six cases pinning q2 against both Pandoc readers. `commonmark` is the target;
`markdown` (the Q1 reader) never interrupts and is *not* what q2 implements.

| continuation line | q2 @ HEAD | pandoc commonmark | pandoc markdown (Q1) |
| --- | --- | --- | --- |
| `- apples` | interrupts (`<ul>`) | interrupts | no |
| `1. apples` | interrupts (`<ol>`) | interrupts | no |
| `2. apples` | **interrupts (`<ol start="2">`)** ✗ | **no** | no |
| `1) apples` | interrupts (`<ol>`) | interrupts | no |
| `1.5 dollars` | **splits into two `<p>`** ✗ | one `<p>` | one `<p>` |
| `3986 for details` | **splits into two `<p>`** ✗ | one `<p>` | one `<p>` |

Three of six diverge from CommonMark, all in the same direction. Note that q2
already matches CommonMark (not pandoc-markdown) on rows 1, 2 and 4 — the
project has effectively already chosen CommonMark semantics for list
interruption, which is what makes the target unambiguous.

The `2. apples` row is a second-order consequence worth calling out: because
the blanket digit exclusion closes the paragraph *before*
`parse_ordered_list_marker` is ever consulted, that function's correct
`dont_interrupt` logic is dead code on this path. Fixing the gate restores it
and fixes this row for free.

## `dashplus.qmd` / `colon.qmd` — the sibling blanket exclusions

`-`, `+` and `:` are blanket-excluded from the same gates, without a peek. The
strand calls this "harmless" for `-` because a `-`-leading line genuinely can
open a list. It is not harmless:

| input | q2 @ HEAD | pandoc commonmark |
| --- | --- | --- |
| `Temperature dropped to` / `-5 degrees overnight.` | two `<p>` | one `<p>` |
| `Gain was` / `+5 percent.` | two `<p>` | one `<p>` |
| `Defined at` / `:host scope.` | one `<p>`, **colon deleted** | one `<p>`, colon intact |

`-5` and `+5` split exactly like digits — same bug, different character class.

The `:` case is different and worse. q2 keeps one paragraph but **silently
drops the colon**, confirmed at AST level:

```
[ Para [Str "Defined", Space, Str "at", SoftBreak, Str "host", Space, Str "scope."] ]
```

The character is gone, with no diagnostic. Since a single paragraph survives,
the colon is being consumed somewhere beyond the gate — most likely the fenced-
div (`:::`) or definition-list marker path. Filed separately as **bd-cxiopjw7**;
fixing the digit gate will not necessarily fix it.

## `ex284.qmd` — CommonMark Example 284, unescaped

The corpus test at `test/corpus/new-spec.txt:2039` carries a workaround:

```
Example 284 - https://github.github.com/gfm/#example-284 (qmd: start your soft breaks with \1, i'm sorry.)
```

The spec input was rewritten from `14.` to `\14.` so the test could pass. Fed
the *real* spec input, q2 does not merely split the paragraph — it invents a
list:

```html
<p>The number of windows in my house is</p>
<ol start="14" type="1">
<li>The number of doors is 6.</li>
</ol>
```

Pandoc's commonmark reader emits one paragraph. Restoring this test to the
unescaped spec input, with the apology deleted, is the acceptance criterion
for the fix.
