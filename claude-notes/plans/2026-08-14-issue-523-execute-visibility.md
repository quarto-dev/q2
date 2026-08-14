# GH issue #523 — `code-fold` vs `execute: echo: false`: assessment and fix plan

**Issue:** https://github.com/quarto-dev/q2/issues/523 (third-party report, 2026-08-13)
**Strand:** bd-nn2fou8h (main). Adjacent: bd-fjfizas7 (engine inference).
Related: bd-1tl09 / bd-g1prx (code-fold), bd-moef1ec4 (`eval`), bd-cymkcyaf (format-level
execute defaults).
**Reported against:** q2 0.20.0 (macOS arm64 release tarball)
**Verified against:** `main` @ `3ac596e0` (0.21.0-dev) — `git log v0.20.0..HEAD` touches neither
`engine/jupyter/text_execute.rs` nor `engine/knitr/`, so 0.20.0 and today's `main` behave
identically on this path.
**Q1 reference:** `quarto` on PATH, version `99.9.9` (dev checkout at `external-sources/quarto-cli`
@ `45caede32`).

## TL;DR

The reported *symptom* is real: `execute: echo: false` does not hide cell source in q2.
The reported *cause* is not. `code-fold: true` has **zero** effect on q2's output — with and
without it the rendered HTML is byte-identical. `code-fold` is not implemented in q2 at all
(tracked separately as bd-1tl09 / bd-g1prx), so it cannot be overriding anything.

The actual defect is broader than the report:

- **The jupyter engine ignores the entire execute-visibility family.** `echo`, `output`,
  `warning`, and `include` are unimplemented at both document scope (`execute:`) and cell
  scope (`#|`). The engine unconditionally echoes source and unconditionally emits every output.
- **The knitr engine ignores document-scope `execute:` entirely** and *actively overrides* it:
  `build_format_config` throws the document's metadata away and hardcodes `echo: true`.
  Per-cell `#| echo: false` does work there, because knitr/R handles it below q2.

So the user-visible contract "`execute: echo: false` hides the code" is broken in q2 for both
engines, in three different ways, none of which involve `code-fold`.

## Verification

All fixtures rendered with `target/debug/q2 render <f>.qmd` and, for comparison,
`quarto render <f>.qmd --to html`. Outputs inspected directly (grep + read), not inferred
from exit codes.

### 1. The reported repro — confirmed symptom

`fold.qmd` (the issue's fixture verbatim) rendered by q2:

```html
<div class="cell">
<div class="sourceCode cell-code code-with-copy"><pre class="sourceCode python"><code
class="sourceCode python">…print(&quot;OUTPUT&quot;)…</code></pre></div>
<div class="cell-output cell-output-stdout">
<pre class="code-with-copy"><code>OUTPUT</code></pre>
```

Source is echoed despite `execute: echo: false`. **Confirmed.**

Q1 on the same file emits only:

```html
<div class="cell-output cell-output-stdout">
<pre><code>OUTPUT</code></pre>
```

**Q1 behaviour is as the reporter describes.**

### 2. The reported isolation — NOT reproduced

The issue states that removing `code-fold: true` makes q2 honour `echo: false`. It does not.
`nofold.qmd` (identical but `format: html`) produces output that is **byte-identical** to
`fold.html` modulo the filename:

```
$ diff <(sed 's/fold/X/g' fold.html) <(sed 's/nofold/X/g;s/fold/X/g' nofold.html)
$ echo $?
0
```

`code-fold: true` changes nothing whatsoever. There is no interaction between the two options
in q2, because one of them is a no-op. I could not reconstruct a variant of the reporter's
document where `code-fold` changes `echo` behaviour; the claim appears to be a mis-attribution
during isolation. It does not affect the validity of the underlying bug report.

### 3. `code-fold` is entirely unimplemented (both engines, both block kinds)

| Fixture | q2 | Q1 |
| --- | --- | --- |
| `code-fold: true` + executed `{python}` cell (echo on) | no `<details>` | `<details class="code-fold"><summary>Code</summary>` |
| `code-fold: true` + plain ```` ```python ```` block | no `<details>` | no `<details>` |

Note the second row: Q1 does **not** fold plain code blocks either. `quarto-post/foldcode.lua`
bails unless the block carries the `cell-code` class, so folding is an executed-cell feature in
Q1 too.

**q2 should not preserve that restriction.** Folding non-executable blocks is a long-standing Q1
feature request (quarto-cli#4693, open since 2023-03-08), and the reason Q1 cannot do it is
filter placement, not design — see also quarto-cli#8345 ("code-folding should be part of
rendering") and #4675 ("Extend the behavior of DecoratedCodeBlock", so custom writers can handle
folding). bd-1tl09's Generate/Render split is exactly the architecture those issues ask for, so
q2 should read fold attributes from any `CodeBlock`. Recorded on bd-g1prx (comment `c-zswhv43a`),
which supersedes an earlier comment of mine that said the opposite.

### 4. Scope of the echo breakage — wider than reported

| Fixture | Option | q2 result |
| --- | --- | --- |
| jupyter, `execute: echo: false` | doc scope | **ignored** — source echoed |
| jupyter, `#\| echo: false` | cell scope | **ignored** — source echoed |
| jupyter, `execute: {output: false, warning: false}` + `#\| include: false` | both | **all ignored** — source, stdout, and the `UserWarning` all emitted |
| knitr, `execute: echo: false` | doc scope | **ignored** — source echoed |
| knitr, `#\| echo: false` | cell scope | honoured (knitr/R does it) |

### 5. Adjacent observation #1 (engine inference) — confirmed, and slightly worse

A `.qmd` with `{python}` cells and no `engine:` key falls back to the markdown engine, executes
nothing, and says nothing — `-v` prints no engine-selection or skipped-execution line. Q1 infers
the jupyter engine and executes.

Beyond the silence, the fallback also leaks the brace syntax into the HTML as a literal class:

```html
<pre class="{python} code-with-copy"><code>print(&quot;NOENGINE&quot;)</code></pre>
```

That is a distinct cosmetic bug (a `{python}` CSS class is never meaningful) worth fixing even
if inference lands later.

Root cause is known and deliberate: `crates/quarto-core/src/engine/detection.rs:20-25` lists
"code block languages (`{python}` → jupyter)" under *Future Enhancements*.

### 6. Adjacent observation #2 (`embed-resources`) — confirmed, out of scope

`embed-resources: true` has no implementation in q2 (the key exists only in the schema at
`crates/pampa/test-fixtures/schemas/document-render.yml:34`; no consumer in any crate). A doc
with `embed-resources: true` still emits `<link rel="stylesheet" href="embed_files/styles.css">`.
**Per Carlos (2026-08-14), this is deliberately not a Q2 priority — no strand filed, no work
planned.** Recorded here only so the issue can be answered accurately.

## Root causes

1. **Jupyter — unconditional echo.**
   `crates/quarto-core/src/engine/jupyter/text_execute.rs:412` `render_cell()` always calls
   `echoed_source_fence()`, and `format_outputs()` always emits every output. The engine already
   has the machinery to do better: `resolve_allow_errors()` (`:324`) resolves `error:` via
   `merge_cell_over_scope(doc_scope, cell_config)`, and `document_execute_scope()` (`:278`)
   already extracts the document's `execute:` map. `echo` was simply never wired to it —
   bd-ohvl879u deliberately scoped itself to `error:` and left the rest to follow-ups
   (bd-moef1ec4 covers `eval`; visibility was never filed until now).

2. **Knitr — document metadata discarded.**
   `crates/quarto-core/src/engine/knitr/mod.rs:255`:

   ```rust
   fn build_format_config(ctx: &ExecutionContext) -> KnitrFormatConfig {
       KnitrFormatConfig::with_defaults(&ctx.format)
   }
   ```

   `ExecuteConfig::with_defaults()` (`engine/knitr/format.rs:~370`) hardcodes
   `echo: Some(Value::Bool(true))`, `warning: Some(true)`, `include: Some(true)`, … The R side
   *does* consume these (`resources/rmd/hooks.R` reads `code-fold`, `echo`, etc.), so the wire
   format is fine — nothing ever populates it from the document. Document-scope `execute:` is
   not merely ignored, it is overwritten with `true`.

3. **`ExecutionContext` carries no document metadata.**
   `crates/quarto-core/src/engine/context.rs` has `engine_config` (the engine's own sub-map) but
   not the merged `execute:` scope. Jupyter works around this by re-parsing the front matter out
   of its own serialized input string (`document_execute_scope`), which knitr cannot reuse
   because its config crosses a JSON boundary to R. The stage that builds the context
   (`stage/stages/engine_execution.rs:375`) *does* have the merged AST in hand.

## What Q1 does (the contract to match)

- `src/core/jupyter/tags.ts` — `shouldInclude(cell, options, context)` for
  `echo | output | warning | include`: **cell option wins over `options.execute[context]`**;
  absent in both, the format default applies (`echo: true` for plain HTML — verified in
  bd-cymkcyaf that plain HTML does *not* default echo off; only revealjs/beamer/pptx/dashboard do).
- `shouldHide(...)` + `keepHidden`: under `render: keep-hidden: true`, hidden content is still
  emitted but tagged with the `.hidden` class rather than dropped.
- `echo: "fenced"` is a third value (emit the cell as a fenced ```` ```` ```` block including its
  YAML options) — `echoFenced()` in the same file.
- `src/resources/filters/quarto-post/foldcode.lua` — folding applies only to blocks classed
  `cell-code`, honours a document-level `param("code-fold")` default plus per-block override,
  maps `true → "hide"`, `show → open`, and propagates a `hidden` class onto the `<details>`.

## Target semantics

Pinned empirically against Q1 (`keep-md: true`, jupyter engine, one cell), then translated into
q2 terms. **q2 is not required to reproduce Q1's post-engine markdown byte-for-byte** (Carlos,
2026-08-14) — Q1's shape is inspiration, not contract. What must match is the *observable*
behaviour, so the tests below assert on semantics, not on Q1's exact fences.

| Option (doc or cell) | Q1 post-engine result | q2 target |
| --- | --- | --- |
| `echo: false` | cell div kept, source fence dropped, outputs kept | same |
| `output: false` | source fence kept, all output divs dropped | same |
| `warning: false` | `stream`/`stderr` outputs filtered out entirely; stdout kept | same |
| `include: false` | **nothing emitted at all** — no cell div | same |
| `echo: false` + `output: false` | **nothing emitted at all** — no empty cell div | same |
| cell option vs doc scope | cell wins (`shouldInclude`) | same |

The last two rows are the subtle ones. Q1 builds the div opener but only writes it "if there is
actually content in the div" (`jupyter.ts` ~1466), so a cell whose code *and* outputs are all
suppressed leaves no wrapper behind. q2's `render_cell` currently emits the wrapper
unconditionally, so this is a real behavioural item, not a formatting detail.

### Scope — decided (Carlos, 2026-08-14)

Do the **whole visibility family** (`echo`, `output`, `warning`, `include`) for **both jupyter
and knitr** in one pass. `eval` stays with bd-moef1ec4 (it changes whether the kernel runs, not
what is shown). `keep-hidden` and `echo: "fenced"` are deferred follow-ups: neither has a
consumer in q2 today, and `keep-hidden` needs `.hidden` CSS q2 has not ported. (`echo: "fenced"`
comes nearly free on the knitr side — `execute.R` already sets `fenced.echo` — so forwarding it
is fine there; the jupyter side is what gets deferred.)

### Key finding that shapes Phase 3

q2's knitr R scripts are **already a faithful port** of Q1's: `resources/rmd/execute.R:307-335`
reads `format$execute` for `echo` (including `"fenced"`), `warning`, `message`, `include`,
`output` (mapping `false` → `results="hide"`, `fig.show="hide"`), `eval`, and `error`. Nothing on
the R side needs to change — the only defect is that nothing populates `format$execute` from the
document.

**Trap to avoid:** `execute.R` uses `isTRUE(format$execute[["warning"]])` and
`isTRUE(format$execute[["include"]])`, so an *absent* key reads as **false**, not true. The
current `ExecuteConfig::with_defaults()` exists precisely to supply those `true`s. Phase 3 must
therefore **overlay** the document's `execute:` onto the defaults, never replace them — passing
the document map through verbatim would set `include` to false for every chunk and blank the
document.

## Work items

TDD throughout: every phase writes its failing test first and confirms it fails.

### Phase 0 — Tests first

- [x] New `crates/quarto-core/tests/integration/engine_visibility.rs` (registered in
      `main.rs`), modelled on `engine_error_policy.rs`: real engines via `record_capture`,
      skipping when the engine is unavailable.
- [x] Jupyter matrix — for each of `echo`/`output`/`warning`/`include`, one doc-scope and one
      cell-scope test.
- [x] Precedence: `execute: echo: false` + `#| echo: true` ⇒ source shown (and the mirror,
      doc-true + cell-false).
- [x] No-empty-wrapper: `#| echo: false` + `#| output: false` ⇒ no `.cell` div at all.
- [x] Knitr matrix — doc-scope `echo`/`warning`/`include`/`output`; cell-scope already works, so
      one cell-scope test as a regression guard.
- [x] Regression test pinned to the issue's exact fixture (`code-fold: true` + `echo: false`)
      asserting no `.cell-code` survives.
- [x] Confirm every new test fails before touching implementation, and that each fails for the
      *expected* reason (not a harness error).

**Phase 0 result (2026-08-14):** 19 tests, **15 fail / 4 pass**. Every failure is semantic
("expected source echoed = false, got true", "warning must be filtered out", "cell must leave no
markup behind") — none is a harness error. The 4 passing are the two no-`execute:`-key default
guards, `knitr_cell_echo_false_still_hides_source` (knitr resolves `#|` itself), and
`jupyter_cell_echo_true_overrides_document_echo_false` (passes vacuously today, since echo is
always on). Knitr's doc-scope tests fail exactly like jupyter's, confirming the second root
cause independently.

Two content-marker conventions worth keeping if these tests are extended: cell bodies build
their output by concatenation (`print("O" + "UT")`) so `contains("OUT")` distinguishes output
from echoed source, and `.cell` is asserted as a *substring* so one check covers the wrapper
plus everything nested in it.

### Phase 1 — Plumb the merged `execute:` scope into `ExecutionContext`

- [x] Add `execute_scope: Option<ConfigValue>` + `with_execute_scope()` to `ExecutionContext`.
- [x] Populate it in `engine_execution.rs` from the merged metadata already held there — read
      per-iteration from `ast.meta`, so a second engine in a sequence sees the front matter of
      the input it is actually handed (exactly what the old re-parse gave it).
- [x] Retire `text_execute.rs::document_execute_scope()`'s front-matter re-parse in favour of the
      field. `front_matter_range` had no other consumer; both are deleted (−1940 bytes), along
      with the now-unused `InterpretationContext` import.

### Phase 2 — Jupyter honours the visibility family

- [x] `resolve_cell_options()` (replacing `resolve_allow_errors`) returns the merged map once;
      `resolved_flag()` reads individual keys off it. `error:` now goes through the same path.
- [x] `CellVisibility { echo, output, warning }` + `resolve()`, mirroring `shouldInclude`: cell
      option, else doc scope, else `true`.
- [x] `include: false` ⇒ emit nothing for the cell (`CellVisibility::HIDDEN` collapses the rest,
      matching Q1's early bail in `mdFromCodeCell`).
- [x] `echo: false` ⇒ no source fence; `output: false` ⇒ no output divs; `warning: false` ⇒ drop
      stderr-stream outputs (`is_warning_output`, Q1's `isWarningOutput`).
- [x] `output: false` also silences warnings — Q1 suppresses every output under it, so the
      warning channel must not leak past a cell whose outputs were switched off.
- [x] Suppress the `.cell` wrapper when neither code nor any output survives.
- [x] 9 new unit tests in `text_execute.rs` covering resolution and emission without a kernel.

### Phase 3 — Knitr receives the document's execute options

- [x] `ExecuteConfig::overlay_document_scope()` (in `format.rs`, beside the struct it maps) and
      `build_format_config()` applies it over `with_defaults()` — overlay, not replace.
- [x] Forwards exactly the keys the R scripts read. `freeze` is deliberately skipped: nothing on
      the R side consumes it (it is resolved before an engine runs).
- [x] String reads go through `as_plain_text()`, not `as_str()`: `echo: fenced` in front-matter
      context is stored as `PandocInlines`, for which `as_str()` returns `None` and the option
      would vanish (the `metadata-as-str` lint documents this trap).
- [x] `error:` now agrees across engines, and gained two tests in `engine_error_policy.rs`:
      doc-level `execute: error: true` lets a failing knitr chunk render (it could not before —
      `with_defaults` pinned `error: false`), and the mirror, that an un-annotated knitr error
      still fails the render.

### Phase 4 — Verification

- [x] `cargo nextest run --workspace` — **11954 passed**, 197 skipped, 0 failed.
- [x] Full `cargo xtask verify` (not `--skip-hub-build`, since `ExecutionContext` is in
      `wasm-quarto-hub-client`'s dependency closure) — **all 14 steps passed**.
- [x] **End-to-end through the binary** — see the evidence section below.
- [x] Spot-check `q2 preview` on a visibility-bearing document (full WASM chain via
      `cargo xtask verify`, then `cargo build --bin q2` to re-embed) — see below.
- [x] File the deferred follow-ups: **bd-tskkw5dq** (`keep-hidden` + jupyter `echo: "fenced"`)
      and **bd-42202ipf** (pre-existing capture-splice mis-pairing, found during Phase 2).

## End-to-end evidence (2026-08-14)

Rendered through the real binary, HTML inspected directly — not inferred from exit codes.

**The issue's exact fixture**, `cargo run --bin q2 -- render fold.qmd`:

```html
<main class="content" id="quarto-document-content">
…
<div class="cell">
<div class="cell-output cell-output-stdout">
<pre class="code-with-copy"><code>OUTPUT</code></pre>
</div>
</div>
</main>
```

Only the output. No `sourceCode python` block — matching Q1 on the same file, and closing the
reported bug.

Three more, same method:

| Fixture | Expectation | Observed |
| --- | --- | --- |
| knitr, `execute: echo: false` | source hidden, output kept | 0 `sourceCode` matches; `KNITROUT` present |
| jupyter, `#\| include: false`, prose either side | no cell markup at all | 0 `class="cell"`; both prose blocks intact |
| jupyter, `execute: warning: false` | stderr dropped, stdout kept | no `cell-output-stderr` div; `STDOUT_OK` present |
| jupyter, no `execute:` key | unchanged behaviour | `sourceCode python` present |

*One measurement correction worth recording*: the `warning: false` fixture wrote
`warnings.warn("WARNME")`, so `grep -c WARNME` returned 1 and briefly looked like a failure. The
single match was the **echoed source line** (echo is on in that fixture), and the HTML contains
no `cell-output-stderr` div at all. This is exactly the trap the integration tests avoid by
splitting content markers across a concatenation — the e2e fixture should have done the same.

### Phase 0 — Tests first

- `crates/quarto-core/tests/integration/` — extend the existing engine-parity suite
  (`engine_output_parity.rs`) with a visibility matrix: for each of `echo`/`output`/`warning`/
  `include`, for doc scope and cell scope, for both engines, assert on the rendered markdown.
- Assert the precedence rule directly: `execute: echo: false` + `#| echo: true` ⇒ shown.
- A regression test pinned to the issue's exact fixture (`code-fold: true` + `echo: false`)
  asserting no `.cell-code` block survives.
- Confirm all of these fail before touching implementation.

### Phase 1 — Plumb the merged `execute:` scope into `ExecutionContext`

- Add `execute_scope: Option<ConfigValue>` + `with_execute_scope()` to `ExecutionContext`.
- Populate it in `engine_execution.rs` from the merged metadata already held there.
- Retire `text_execute.rs::document_execute_scope()`'s front-matter re-parse in favour of the
  field (it exists only because the field did not). Keep `front_matter_range` if still used
  elsewhere; delete if not.

### Phase 2 — Jupyter honours the visibility family

- Add a `resolve_visibility()` alongside `resolve_allow_errors()`, mirroring Q1's
  `shouldInclude`: cell option, else doc scope, else default `true`.
- `render_cell()` takes the resolved flags: skip `echoed_source_fence()` when `echo` is false;
  skip the whole `::: {.cell}` wrapper when `include` is false; skip outputs when `output` is
  false; filter stderr/warning outputs when `warning` is false.
- Q1 parity detail to preserve: a cell with no visible content should not leave an empty
  `::: {.cell}` div behind (check against Q1's output for `include: false`).

### Phase 3 — Knitr receives the document's execute options

- `build_format_config()` reads `ctx.execute_scope` and populates `ExecuteConfig` (`echo`,
  `warning`, `error`, `include`, `output`, `fig-*`, `df-print`, …) over the current defaults.
- Verify the R side actually consumes each key we forward before claiming support for it;
  forward only what `hooks.R`/`execute.R` reads, and note the rest.
- Watch the interaction with the existing `error:` policy — jupyter resolves `error` in Rust,
  knitr in R. They must not disagree.

### Phase 4 — Verification

- `cargo nextest run --workspace`.
- `cargo xtask verify --skip-hub-build` (Rust-only unless `ExecutionContext` changes ripple into
  `wasm-quarto-hub-client`; if they do, full `cargo xtask verify`).
- **End-to-end through the binary**, per CLAUDE.md: render the issue's exact `fold.qmd` with
  `cargo run --bin q2 -- render fold.qmd`, inspect the HTML, and record the invocation + output
  snippet in this document.
- Spot-check `q2 preview` on a visibility-bearing document (needs the full WASM rebuild chain —
  `npm run build:wasm` → `cargo xtask build-q2-preview-spa` → `cargo build --bin q2`).

### Out of scope (separate strands)

- **`code-fold`** — bd-g1prx (Phase 3 of the bd-1tl09 decorations epic), already planned in
  `claude-notes/plans/2026-05-19-code-block-features.md`. Annotated there via bd-g1prx comment
  `c-zswhv43a`: q2 should fold *any* code block, not just `.cell-code`, resolving
  quarto-cli#4693/#8345/#4675. Doc-level-default scope **decided** (Carlos, 2026-08-14, comment
  `c-dyfdgmr1`): the document default stays cell-scoped; plain blocks fold only via an
  affirmative per-block attribute. q2 already parses and forwards that attribute today —
  ```` ```{.scss code-fold="true"} ```` reaches the writer as `data-code-fold="true"` — so the
  opt-in needs no parser work, only a Generate-transform reader plus attribute stripping.
- **Engine inference from cell languages** — bd-fjfizas7 (filed `discovered-from` the main
  strand), which also covers the literal `{python}` class leak.
- **`keep-hidden` / `echo: "fenced"`** — follow-ups after Phase 2.
- **`embed-resources`** — not a Q2 priority; no strand.

## Suggested reply to the reporter

Worth answering, since their isolation step was wrong and someone else may repeat it: the bug is
real and confirmed, `code-fold` is not the cause (it is unimplemented and inert), the real scope
is "the execute-visibility options are not implemented for the jupyter engine and document-scope
`execute:` is dropped by the knitr engine", observation #1 is confirmed and tracked, and
observation #2 is known but deliberately not prioritized for Quarto 2.

## Reproduction fixtures

Kept inline rather than committed; recreate under any scratch dir.

```yaml
# fold.qmd — the issue's fixture
---
title: t
format:
  html:
    code-fold: true
engine: jupyter
execute:
  echo: false
---
```
followed by a ```` ```{python} ```` cell containing `print("OUTPUT")`.

Variants used: `nofold.qmd` (`format: html`), `cellecho.qmd` (`#| echo: false`, no doc scope),
`foldecho.qmd` (`code-fold: true`, echo on), `plainfold.qmd` (`code-fold` + non-executable
block), `knitrecho.qmd` / `knitrcell.qmd` (knitr doc/cell scope), `vis.qmd`
(`output`/`warning`/`include`), `noengine.qmd` (no `engine:` key).


## Preview verification, and a pre-existing bug it exposed

`q2 preview` renders through the WASM client and splices *captured* engine output into the live
AST, so it needed its own check — the render path passing proves nothing about it (the
2026-05-20 stale-WASM incident in `CLAUDE.md`).

**The feature works in preview.** A single-file preview of a doc with `execute: echo: false`,
inspected through CDP in the rendered iframe: `div.cell` = 1, `.cell-code` = 0,
`pre.sourceCode` = 0, one `.cell-output-stdout` containing `OUTPUT_VISIBLE`. Page text is prose,
output, prose — no source anywhere.

**The check also confirmed bd-42202ipf empirically** (previously only a code trace). Two
*adjacent* cells, the first `#| include: false`:

```
div.cell count: 1
pre classes: ['sourceCode python :: print("SECOND_CELL_OUTPUT")',
              'code-with-copy :: SECOND_CELL_OUTPUT',
              '{python} code-with-copy :: print("SECOND_CELL_OUTPUT")']
```

The hidden cell's position renders the *second* cell's output; the second cell falls through to
raw source (note the literal `{python}` class). The second cell's content appears twice, and the
cell the author hid is the one showing output.

**It is pre-existing.** The same fixture under knitr — whose `include: false` path this work does
not touch — produces the identical DOM. bd-nn2fou8h only widens which documents reach it.

*Method note:* editing the `.qmd` while `q2 preview` runs did **not** re-capture in single-file
mode; the page kept showing both cells as raw source, which masks the bug. Restarting the preview
against the final content is what exposes it.
