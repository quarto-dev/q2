# Probe: what would making `sectionize_blocks` recurse into Divs actually change?

Companion to `../div-toc-probe/`. That probe established *the rule*; this one
measures *the cost and the benefit* of acting on it, and answers the question
"not recursing into divs seems to be what protects callouts and tabsets — what
would recursing buy?"

## Run

```bash
quarto render . --output-dir _site-q1     # Q1 (dev checkout, 99.9.9)
cargo run --bin q2 -- render .            # q2 @ 5b6774d1
```

(`_site/`, `_site-q1/` not committed. `callout2.qmd` is a second fixture —
a callout with both a title heading and a body heading.)

## Finding 1 — non-recursion is *not* what protects callout/tab titles

Those title Headers are **consumed** by `CalloutTransform` (pipeline.rs ~:1232)
and `PanelTabsetTransform` (:1246) long before `SectionizeTransform` (:1333)
runs. There is no Header left for sectionize to wrap, whether it recurses or
not. Q1 is the proof by construction: pandoc's `makeSections` *does* recurse
into Divs, and Q1 still gets tab and callout titles right — by the same
consume-first mechanism.

## Finding 2 — the parser already has the nesting; the reader discards it

`pandoc_div`'s grammar rule is `repeat($._block)`, and `$._block` includes
`$.section` (`tree-sitter-markdown/grammar.js:190`, `:965`). The CST for a
heading inside a div really is nested:

```
document
  section                      (## Real heading)
    atx_heading
    pandoc_div                 (.my-wrapper)
      section                  (#### Heading inside a plain div)   ← nested
        atx_heading
        pandoc_paragraph
```

`process_section` (`crates/pampa/src/pandoc/treesitter_utils/section.rs:27`)
then flattens it — `IntermediateSection(section) => blocks.extend(section)` —
yielding `[Header 2, Div(.my-wrapper)[Header 4, Para]]`. `sectionize_blocks`
rebuilds sections later from header levels, top level only.

**But preserving the parser's sections instead is not a shortcut.**
`PanelTabsetTransform` and `CalloutTransform` both scan for *flat* Headers as
direct children of their Div ("the first Header inside the Div fixes the tab
level"). Handing them pre-sectionized input breaks both. Sectionize belongs
where it is — a late Normalization transform; the gap is that it doesn't
recurse.

## Finding 3 — the current TOC agreement is an equilibrium, and it is partial

q2 has two divergences that cancel: the collector recurses into *every* Div,
sectionize into *none*. Measured across container types (TOC
`data-scroll-target` ids, Q1 vs q2 @ 5b6774d1):

| heading inside…                   | Q1  | q2  | agree? |
| --------------------------------- | --- | --- | ------ |
| plain `::: {.my-wrapper}`          | in  | in  | ✅ |
| `::: {.content-visible …}`         | in  | in  | ✅ |
| `::: {.column-margin}`             | in  | in  | ✅ |
| `::: {layout-ncol=2}`              | in  | in  | ✅ |
| **`.callout` body**                | out | **in** | ❌ leak |
| **`.panel-tabset` pane**           | out | **in** | ❌ leak |
| **`> blockquote`**                 | out | **in** | ❌ leak |

The cancellation is exact for **transparent** Divs — ones pandoc's
`makeSections` *absorbs*, merging the Div's attributes into the section it
wraps:

```html
<!-- Q1 -->  <section id="heading-in-a-layout-div" class="level3 quarto-layout-cell" …>
<!-- q2 -->  <div data-layout-ncol="2"><h3 id="heading-in-a-layout-div">
```

It fails for **opaque** Divs — ones where a filter built real chrome, so the
section ends up *nested inside* a Div rather than merged with it, and pandoc's
`sectionToListItem` (which matches only `Div(_,_,Header:rest)`) stops at the
Div:

```html
<!-- Q1, callout2.qmd: the section exists but is unreachable from the TOC walk -->
<div class="callout …">
  <div class="callout-header …">…</div>
  <section id="body-heading" class="level4 callout-body-container callout-body">
```

So `.callout` bodies leak too — this is a **third** container beyond the
tabset panes and blockquotes, which is what rules out a narrow
`.panel-tabset`-only skip.

## Finding 4 — cost of recursion: zero test failures

Spike (2026-08-18): recurse into any Div lacking the `section` class, then
`cargo nextest run --workspace` → **12306 passed, 0 failed**. The blast radius
feared in the plan (reconcile hashing, `llms.rs`, idempotence, snapshots) did
not materialize. Output changes as expected:

```html
<div class="content-visible">
  <section id="section-inside-content-visible" class="section level3">
```

## Finding 5 — the attribute merge is load-bearing, not cosmetic

Second spike, stacking the collector restriction (non-section Div terminates
the walk; `BlockQuote` arm removed) on top of recursion:

- `.callout` body → **matches Q1** ✅
- `.panel-tabset` pane (the strand's repro) → **matches Q1** ✅
- but `.content-visible`, `.column-margin`, `layout-ncol` → **entries lost** ❌

Because recursion alone yields `Div(.content-visible) > Div(section)`, and the
restricted walk stops at the outer Div. Q1 keeps those entries only because
`makeSections` **merged** the Div away — there is no wrapping Div left to stop
at.

**The three parts are coupled.** Recursion + collector restriction *without*
the merge under-collects. Any plan that takes this direction has to land all
three: recurse, merge (when the Div's id is empty and it wraps a header-led
run), restrict.
