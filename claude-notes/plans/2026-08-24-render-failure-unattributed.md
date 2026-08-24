# Render failures with no attribution (bd-render-failure-unattributed-yxe0v7th)

## Overview

A page can fail during a project render and the output never names the file.
The strand reports two shapes; investigation confirmed both and found a third,
worse face of shape 1.

### Shape 1 — engine-output parse errors are bound to the wrong buffer

`EngineExecutionStage` parses the engine's executed markdown
(`result.markdown`, e.g. knitr's `.knit.md`) at
`crates/quarto-core/src/stage/stages/engine_execution.rs:611-621`. On the
**Ok** path it carefully registers the intermediate as a new `SourceContext`
slot and remaps `FileId(0)` into it. On the **Err** path it throws that
context away:

```rust
.map_err(|diagnostics| {
    PipelineError::stage_error_with_diagnostics(self.name(), diagnostics)
})?;
```

`pampa::readers::qmd::read` returns only `Vec<DiagnosticMessage>` on failure,
so the `SourceContext` that gave those spans meaning is dropped. The generic
`StageError -> QuartoError::Parse` conversion at
`crates/quarto-core/src/pipeline.rs:725-741` then **fabricates** a
`SourceContext` from the *original document's* bytes and binds the
engine-output spans to it.

Measured, `q2 render` on a 106-byte `knit.qmd` whose R chunk emits a bad
shortcode:

```
DBG conv: source_name=.../knit.qmd content_len=106
          diags=[("Unquoted shortcode parameter starting with digit",
                  Some((4716, "Original { file_id: FileId(0),
                               start_offset: 4716, end_offset: 4717 }")))]
```

Offset 4716 indexes the `.knit.md`; the bound buffer is 106 bytes. Two
outcomes follow from where the offset lands:

- **Past EOF** (the common case — engine output is much larger than the
  source): `map_offset` fails, so ariadne renders nothing *and*
  `to_text_with_renderer`'s `at <file>:<row>:<col>` fallback is skipped too.
  Output is a bare title + problem with no file, line, or frame. This is the
  shape the strand reports.
- **Inside the source file** (a long `.qmd`): a fully-formed, confident
  ariadne frame is printed against an **arbitrary wrong location**. Measured
  on a 14576-byte `big.qmd`: the error was reported at `big.qmd:68:100`,
  caret on the letter `e` of a filler paragraph containing no shortcode at
  all. **Not in the strand** — silently wrong attribution is worse than
  missing attribution, because nothing signals it is wrong.

Same defect class as the `add-file-with-id` lint rule's rationale
(bd-m6wmztln): binding an *assumed* file to a diagnostic's resolved id
renders byte offsets against the wrong text.

### Shape 2 — the render summary drops `FileFailure.input`

`print_render_diagnostics_text` in `crates/quarto/src/commands/render.rs:1330-1355`
prints `error: {input}: {error}` only for `legacy_failures` (failures whose
`diagnostics` vec is empty). Any failure that *does* carry structured
diagnostics is routed through `coalesce_by_source` and rendered by
`CoalescedDiagnostic::to_text()`, which relies entirely on the diagnostic's
own span to name the file — and deliberately omits the `Affected files:`
tail for singleton groups. So when the span is absent or unresolvable, the
known-good `failure.input` is discarded and nothing names the file.

Confirmed on the committed repro
(`q2-positron-docs/.../silent-failure/repro`): a chunk calling `stop()` in a
project render prints `Error: Execution failed in knitr: R process failed`
with the `.qmd` name absent from the whole transcript. Same for a
location-less engine-availability failure (`location: None` verified by
instrumentation).

Note the `--json-errors` path already emits `"source_file": ".../knit.qmd"`
correctly — it reads `failure.input` structurally. Only the text path loses it.

## Scope

Decided with Gordon 2026-08-24:

- **In scope**: shape 1's root cause, and a structural attribution guarantee
  so every per-file failure names its `.qmd`.
- **In scope**: the attribution line is added **only where today's output goes
  silent about which page failed**, so existing well-attributed output and its
  snapshots are untouched. (Sketched as "only when the diagnostic is
  unlocatable"; Fix A changed what that covers — see Outcome.)
- **Out of scope**: remapping knitr's `failing.rmarkdown:133-135` traceback
  line numbers back to `.qmd` lines. That needs a line map from the
  serialized intermediate plus per-engine stderr parsing. To be filed as a
  separate strand.

## Phase 1 — Tests (TDD, must fail first)

- [x] `engine_execution.rs`: fake `ExecutionEngine` returning markdown with a
      parse error; assert the stage returns `PipelineError::Structured` whose
      `SourceContext` resolves the diagnostic span against the **engine
      output**, under the intermediate filename.
- [x] `engine_execution.rs`: assert the diagnostic's offset is in range for
      the registered content (guards the wrong-frame face directly).
- [x] `render.rs`: unit tests for the attribution helper — (a) location-less
      diagnostic gains the line, (b) resolvable location is left untouched,
      (c) location that does not resolve in its context gains the line,
      (d) a span resolving to an engine intermediate gains the line,
      (e) multi-file groups are left to their `Affected files:` tail,
      (f) matching survives mixed path separators (Windows).
- [x] knitr-gated e2e (following `marimo_engine_e2e.rs`'s
      `rscript_available()` / `knitr_r_package_available()` pattern): project
      render whose R chunk emits a bad shortcode names the `.qmd`.

## Phase 2 — Fix A: engine-output parse errors keep their own context

- [x] Build a `SourceContext` holding the engine output under the
      intermediate name and return `PipelineError::Structured(ParseError)`.
- [x] Audit the sibling site `serialize_ast_to_qmd`
      (`engine_execution.rs:734`) for the same defect.

## Phase 3 — Fix B: structural attribution in the render summary

- [x] Add an "is this diagnostic locatable in its context" predicate.
      (Landed as `failure_attribution_line`; see Outcome for the refinement.)
- [x] Prefix `error: while rendering <path>` for unlocatable pass1/pass2
      failures; leave locatable ones byte-identical.

## Phase 4 — Verification

- [x] `cargo clippy` + per-crate `nextest` for touched crates.
- [x] Workspace `cargo nextest run --workspace`; report delta vs live baseline.
- [x] End-to-end via the real binary on the knitr repro and the Positron
      shape-2 repro; record invocation + observed output here.
- [x] Reconcile this checklist against reality before handoff.

## Outcome

### Predicate refinement (deviation from the pre-implementation sketch)

The agreed rule was "add the attribution line only when the diagnostic is
unlocatable". Implementing Fix A changed what "unlocatable" covers: engine
output parse errors now resolve *perfectly well* — against
`<stem>.<engine>.rmarkdown`. A pure unlocatable test would therefore have gone
quiet again on exactly the case the strand was filed for.

`failure_attribution_line` uses the intended rule instead: **add the line when
the rendered output does not name the page that failed.** That is unlocatable
spans *plus* spans resolving to a file other than `FileFailure.input`. The
no-churn property Gordon asked for is preserved — an ordinary parse error in
the `.qmd` resolves to the `.qmd`, so nothing is added.

### Audit result: `serialize_ast_to_qmd`

No change needed. Its `Err` carries either the location-less `Q-3-1` IO
diagnostic (now covered by Fix B) or `ctx.errors`, whose spans index the
original AST's source — so the generic conversion binds them correctly.

The same `stage_error_with_diagnostics` shape also appears at
`crates/quarto-core/src/engine/preview_record.rs:241`
(`preview-record/compute-input-qmd`). That feeds the q2-preview frontend, not
the CLI render summary, so it is untouched here and unverified.

### Verification

`cargo clippy -p quarto-core -p quarto --all-targets -- -D warnings` — clean.

Workspace, measured live on this branch:

| run | tests | result |
| --- | --- | --- |
| baseline (changes stashed) | 13130 | 1 failed: `quarto-core engine::ts_engine::tests::test_race_free_instance_exclusive` |
| with changes | 13140 | 13140 passed, 199 skipped |

Delta **+10**, fully accounted for: 2 (`engine_execution.rs`) + 6
(`render.rs` unit) + 2 (`render_cli_e2e.rs`). Skipped count unchanged at 199.

The baseline failure is a pre-existing flake, not a regression: it timed out at
15.152s under full-workspace contention, passes in 0.295s in isolation, and
passed in the with-changes workspace run.

### End-to-end, through the real binary

`q2 render` in a project whose R chunk emits `{{< fa envelope size=1x >}}`.

Before — the page dies unnamed:

```
Error [Q-2-34]: Unquoted shortcode parameter starting with digit
Shortcode parameter values starting with digits must be quoted.
```

After — output inspected, both the page and the offending line are named:

```
error: while rendering /…/t2/knit.qmd
Error: [Q-2-34] Unquoted shortcode parameter starting with digit
     ╭─[ /…/t2/knit.knitr.rmarkdown:138:23 ]
     │
 138 │ {{< fa envelope size=1x >}}
     │                       ┬
     │                       ╰── Shortcode parameter values starting with digits must be quoted.
─────╯
```

Shape 2, on the committed Positron repro
(`q2-positron-docs/.../silent-failure/repro`, `q2 render failing.qmd`):

```
error: while rendering /…/repro/failing.qmd
Error: Execution failed in knitr: R process failed
```

The scoped-out half remains: the traceback above it still cites
`failing.rmarkdown:133-135`.

Regression check — an ordinary parse error in a `.qmd` is byte-identical to
before, with no `while rendering` line added.

## Follow-up

- [ ] File a strand for remapping engine-intermediate line numbers
      (`<stem>.<engine>.rmarkdown:NNN`) back to `.qmd` lines, covering both
      knitr's stderr traceback and the ariadne frame's file/line.
