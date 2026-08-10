# Observed output — before and after the fix

The "Before" section below is the baseline captured during investigation; the
"After" section is the same fixture re-rendered once the fix landed.

## Before (`docs/feature-porting-process` @ `d1a8ac9f`)

Invocation, run from the repo root after `cargo xtask verify --skip-hub-build`
passed (exit 0):

```
cargo run --quiet --bin q2 -- render claude-notes/plans/callout-title-attribute-investigation/repro.qmd
```

Exit 0, no warnings emitted. The generated `repro.html` / `repro_files/` were
inspected and then deleted (build artifacts); the relevant fragments are quoted
below verbatim.

## `callout-title-container` contents, case by case

| # | Fixture case | Rendered header | Q1-correct? |
|---|---|---|---|
| 1 | `title="Off-Host Execution"` | `Note` | ❌ title dropped |
| 2 | `title="Data loss on deployment"` (warning) | `Warning` | ❌ title dropped |
| 3 | untitled control | `Note` | ✅ |
| 4 | heading title | `<span class="screen-reader-only">Note</span>Heading title` | ✅ |
| 5 | **both** attribute + heading | `<span class="screen-reader-only">Note</span>This heading should remain in the body` | ❌ twice — heading won *and* was consumed |
| 6 | markdown in attribute (`` `renv` ``) | `Tip` | ❌ title dropped |
| 7 | `title=""` + heading | `<span class="screen-reader-only">Note</span>Fallback heading` | ✅ (incidentally — the attribute is ignored either way) |

Raw fragments for the two most informative cases:

```html
<!-- case 1: the author's title survives only in the attribute -->
<div class="callout callout-style-default callout-note callout-titled" title="Off-Host Execution">
<div class="callout-header d-flex align-content-center">
<div class="callout-icon-container">
<i class="callout-icon"></i>
</div>
<div class="callout-title-container flex-fill">
Note
</div>
</div>
```

```html
<!-- case 5: heading wins over the attribute AND is removed from the body -->
<div class="callout callout-style-default callout-note callout-titled" title="Attribute wins">
...
<div class="callout-title-container flex-fill">
<span class="screen-reader-only">Note</span>This heading should remain in the body
</div>
</div>
<div class="callout-body-container callout-body">
<p>Body text.</p>
</div>
```

## What this adds beyond the strand's description

1. **The bug is confirmed at HEAD**, not merely inferred from code reading.
2. **Case 5 is a *double* divergence** the strand did not call out. Q1 gives the
   attribute precedence and leaves the heading in the body; q2 does the opposite
   on both counts — the heading supplies the title and is consumed. A fix that
   only adds an attribute branch without touching the heading-removal placement
   would fix precedence and still leave the body wrong.
3. **`title=` is preserved on the outer div** in every case, matching Q1 — so
   the fix must read the attribute without removing it.
4. **`title=""` renders as `title=""`** on the div (case 7). Worth deciding
   whether an empty attribute should be emitted at all; Q1's behavior here was
   not checked.
5. **No diagnostic is emitted** in any case.

---

# After (branch `braid/callout-title-attribute`)

Same invocation:

```
cargo run --quiet --bin q2 -- render claude-notes/plans/callout-title-attribute-investigation/repro.qmd
```

Exit 0. **One warning**, which is the intended new behavior:

```
Warning: [Q-2-43] Callout title given twice
   ╭─[ …/repro.qmd:38:1 ]
38 │╭─▶ ::: {.callout-note title="Attribute wins"}
   ┆┆
42 │├─▶ :::
   │╰──────── This callout carries both a `title=` attribute and a leading
   │          heading. The `title=` attribute is used and the heading is dropped.
```

The caret lands on the offending callout (case 5), not on a sibling.

## `callout-title-container` contents, case by case

| # | Fixture case | Rendered header | Q1-correct? |
|---|---|---|---|
| 1 | `title="Off-Host Execution"` | `<span class="screen-reader-only">Note</span>Off-Host Execution` | ✅ byte-identical to Q1 |
| 2 | `title="Data loss on deployment"` (warning) | `<span class="screen-reader-only">Warning</span>Data loss on deployment` | ✅ |
| 3 | untitled control | `Note` | ✅ unchanged, still no SR span |
| 4 | heading title | `<span class="screen-reader-only">Note</span>Heading title` | ✅ unchanged |
| 5 | **both** attribute + heading | `<span class="screen-reader-only">Note</span>Attribute wins` | ✅ attribute wins (Q1 precedence) + Q-2-43 |
| 6 | markdown in attribute | `<span class="screen-reader-only">Tip</span>Use <code>renv</code> for reproducibility` | ✅ code span parsed |
| 7 | `title=""` + heading | `<span class="screen-reader-only">Note</span>Fallback heading` | ✅ unchanged |

Case 1 now matches the markup quoted in the strand's Q1 reference exactly.

## Case 5 body — the heading is consumed

```html
<div class="callout callout-style-default callout-note callout-titled" title="Attribute wins">
...
<div class="callout-title-container flex-fill">
<span class="screen-reader-only">Note</span>Attribute wins
</div>
</div>
<div class="callout-body-container callout-body">
<p>Body text.</p>
</div>
```

`grep -c 'This heading should remain in the body' repro.html` → **0**. This is
the deliberate, warned divergence from Q1 (decision 3): Q1 would leave the
heading here, reading as a duplicate title.

## Verification note

The output above was read directly from the generated `repro.html`, not
inferred from exit status. The HTML and its `_files/` directory were deleted
afterwards as build artifacts; only this record is kept.
