# bd-gthycd33: Jupyter engine output not spliced into preview (knitr works)

**Strand:** bd-gthycd33 (bug, P2, discovered-from bd-sfet3264)
**Branch:** `braid/bd-gthycd33-jupyter-engine-output-not` (off `main`)
**Status:** implemented + verified end-to-end (2026-07-02): Phases 1–4
complete (unit + engine-gated integration + parity suites green; full
workspace suite 10181 passed; full `cargo xtask verify` passed; CLI
`q2 render` and browser `q2 preview` e2e inspected). Staged, awaiting
commit approval; the feature-branch hub-harness cross-check happens
post-merge.

## Overview

Clicking **Run** on an `engine: jupyter` document in hub-client produces a
capture (the `CaptureRef` sidecar arrives, the status bar reads "Showing
executed output"), but the computed output is not spliced into the preview —
the `{python}` cell still renders as source. The identical flow with
`engine: knitr` splices correctly.

The bug is **independent of the hub execution-provider feature** and exists on
`main`: it affects any consumer of the capture-splice path, including `q2
preview`'s own server-side capture recording (bd-lucp). Reproduced
mechanically on `main` in this worktree (2026-07-01, see below).

## Root cause (confirmed by reproduction)

`derive_cell_outputs` (`crates/quarto-core/src/engine/capture_splice.rs`)
walks the capture pair `(A1 = parse(input_qmd), B1 = parse(result.markdown))`
and requires each engine cell in A1 to map to a **`Div` with class `cell`**
(`is_cell_wrapper`) in B1.

- **knitr** satisfies this: its vendored Quarto-1 knitr hooks
  (`crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R:403`,
  `classes <- c("cell", ...)`) wrap every executed chunk in a `::: {.cell}`
  pandoc div containing the `{.r .cell-code}` echo fence and
  `::: {.cell-output .cell-output-stdout}` output divs.
- **jupyter** does not: `execute_blocks_inner` + `format_outputs`
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs`) emit a **bare**
  echoed source fence (` ```python `) followed by bare output fences
  (` ```{.cell-output} `, ` ```{.cell-output-stdout} `, …) — **no `::: {.cell}`
  wrapper at all**.

So the splice walk, on jupyter's B1, finds a plain `CodeBlock` where it
requires a `Div.cell`, records no entry for the cell, then diverges on the
next lockstep prose comparison and stops. The cell-output map comes out
**empty**, the splice is a fail-soft no-op, and the cell renders as raw
source — exactly the browser symptom.

### Mechanical reproduction (2026-07-01, this worktree, off `main`)

Test: `crates/quarto-core/tests/integration/repro_gthycd33.rs` (in this
branch's working tree). It runs `record_capture` with the **real** engine
(mirroring `quarto-hub-provider`'s `pollster::block_on` calling convention —
the jupyter engine builds its own current-thread tokio runtime, so the test
must not be `#[tokio::test]`), then replays exactly what `CaptureSpliceStage`
does: parse `input_qmd` / `result.markdown` with `pampa::readers::qmd::read`
and call `derive_cell_outputs`.

```
cargo nextest run -p quarto-core -E 'binary(integration) & test(repro_gthycd33)' --no-capture
```

Observed (both engines available on this machine):

- `jupyter_capture_splice_map_is_nonempty` — **FAIL**: map has 0 entries.
  Captured `result.markdown` for `2 + 3`:

  ````markdown
  Some prose.

  ```python
  2 + 3
  ```

  ```{.cell-output}
  5
  ```
  ````

- `knitr_capture_splice_map_is_nonempty` — **PASS**: map has 1 entry.
  Captured `result.markdown` for `1 + 1`:

  ````markdown
  Some prose.

  ::: {.cell}

  ```{.r .cell-code}
  1 + 1
  ```

  ::: {.cell-output .cell-output-stdout}

  ```
  [1] 2
  ```

  :::
  :::
  ````

The same pair of tests was first run on `feature/hub-execution-provider`
(warm checkout) with identical results, matching the browser-observed
behavior recorded in
`claude-notes/plans/2026-07-01-merge-preview-status-line.md` ("Orthogonal bug
found").

### Secondary defect, same root

`resources/scss/bootstrap/_bootstrap-rules.scss:1470` styles outputs with the
selector `.cell .cell-output-stdout pre code`. Jupyter's wrapper-less emission
never matches the `.cell` ancestor, so jupyter outputs also miss intended
styling in **every** render path, not just the splice. Fixing the emission
shape fixes this for free; loosening the matcher would not.

## Proposed fix

**Fix the jupyter engine's emission to produce the Quarto-canonical cell
shape** (what knitr's hooks and Quarto 1's jupyter engine both emit), rather
than teaching `derive_cell_outputs` to tolerate wrapper-less output.

Rationale:

- The `::: {.cell}` wrapper is the established Quarto contract for executed
  cells; knitr (via the vendored Q1 hooks) and Quarto 1's jupyter engine both
  honor it. The Rust jupyter engine is the outlier.
- The splice's `(content-hash, occurrence-index)` design is sound and shared;
  special-casing wrapper-less engines there would add a second matching mode
  that has to guess where a cell's output run ends (ambiguous when outputs are
  markdown/prose). That is the hacky path.
- The wrapper also fixes the CSS-selector mismatch above and gives jupyter
  cells the same downstream affordances as knitr cells (cell-level transforms,
  code-fold, copy button targeting, etc.).

### Target emission shape

For each executed cell, `execute_blocks_inner` emits:

````markdown
::: {.cell}

```{.python .cell-code}
2 + 3
```

::: {.cell-output .cell-output-display}

```
5
```

:::
:::
````

Concretely, in `text_execute.rs`:

1. Wrap each cell's emission (echoed source + outputs) in `::: {.cell}` …
   `:::`.
2. Echoed source fence becomes ` ```{.python .cell-code} ` (attribute-syntax
   class list; the code-highlight stage resolves the language from the first
   class, same as knitr's `{.r .cell-code}` — highlighting keeps working).
3. Outputs move from bare classed fences to Q1/knitr-parity divs wrapping
   plain fences (classes per decision 2):
   - stream → `::: {.cell-output .cell-output-stdout}` (or `-stderr`) around
     a plain ` ``` ` fence;
   - execute_result / display_data `text/plain` →
     `::: {.cell-output .cell-output-display}` around a plain fence;
   - `text/html` → `::: {.cell-output .cell-output-display}` + ` ```{=html} `,
     nested inside the `.cell` wrapper;
   - error → `::: {.cell-output .cell-output-error}` around a plain fence.

Cells that produce no output still get the `.cell` wrapper (source-only cell,
like knitr's `echo`-only chunks) — the splice then correctly replaces the
live cell with the wrapper, and "no output" renders as just the echoed code.

### Scope notes

- `JupyterTransform` / `output.rs::outputs_to_blocks` (the AST-path variant)
  is **only used by tests** (`jupyter_integration.rs`); production goes
  through `ExecutionEngine::execute` → `text_execute.rs`. v1 of this fix
  touches only the text path. Whether to align or retire the AST path is
  flagged as an open question (decision 3), not silently changed.
- No `.snap` snapshot in the workspace contains `cell-output`, and no test
  fixture `.qmd` declares `engine: jupyter` — jupyter execution is only
  exercised by kernel-gated tests. Expected fallout is limited to the shape
  assertions in `text_execute.rs` unit tests and `jupyter_integration.rs`.
- `capture_splice.rs` itself needs **no change**.

## Decisions (locked 2026-07-02 with Carlos)

1. **Full knitr-parity shape** — not just the minimal `::: {.cell}` wrapper;
   the `.cell-code` echo fence and `.cell-output` divs too. Ratified
   rationale: **cross-engine congruence of post-engine markdown is a
   correctness requirement in q2**, not cosmetics — any structural divergence
   between engines' text output is precisely the class of bug bd-gthycd33 is
   (the splice, CSS selectors, and future cell-level transforms all key on
   one shared shape). Corollary: where quarto-cli's jupyter emission differs
   *structurally* from knitr's, treat that as a quarto-cli bug and do **not**
   replicate it — q2 emits one canonical structure for both engines.
   Discrepancies found during implementation are resolved toward the common
   structure and **flagged in this plan for Carlos's review when the right
   resolution is in doubt** (one such flag already recorded below: knitr
   autoprint→`-stdout` vs jupyter execute_result→`-display`).
2. **Adopt Q1's output-class scheme** (verified in
   `external-sources/quarto-cli/src/core/jupyter/jupyter.ts`,
   `outputTypeCssClass` + the output-div emitter around line 1578): every
   output div carries `.cell-output` plus a specific class —
   `.cell-output-stdout` / `.cell-output-stderr` for streams,
   `.cell-output-display` for `execute_result` and `display_data`,
   `.cell-output-error` for errors; the echoed source fence carries
   `.cell-code`; the wrapper is `::: {.cell}`. Matches knitr's hooks. Keep
   using `external-sources/quarto-cli` as the read-only shape reference
   throughout this task.
3. **`execution_count` attributes: deferred.** Nothing in q2 consumes them
   yet; add when a consumer appears (file a strand at close-out).
4. **Structural anti-regression instead of an API refactor.** The engine API
   stays text-in/text-out (future user-extensible engines will be too), so
   AST-emission uniformity cannot be enforced at the API level. Enforce it
   with tests instead: a **cross-engine output-parity suite** that runs
   equivalent minimal inputs through knitr and jupyter and asserts the parsed
   post-engine ASTs have the same structure (design below; runtime-gated on
   both engines being installed). Additionally, the test-only AST path
   (`JupyterTransform` / `outputs_to_blocks`) is a latent divergence source:
   retire it, or make it delegate to the single text-path emitter, whichever
   is the modest diff; if both turn out large, record findings and file a
   follow-up strand rather than forcing it into this fix. Full engine-API
   refactoring is explicitly out of scope.

## Cross-engine parity suite (decision 4) — design

New integration module (e.g.
`crates/quarto-core/tests/integration/engine_output_parity.rs`), runtime-
skipped unless **both** engines are available (same gating idiom as
`jupyter_integration.rs`), since it needs R+knitr and jupyter+ipykernel on
the machine. For each pair of equivalent minimal documents:

| case | knitr input | jupyter input |
|---|---|---|
| stream output | `cat("hi\n")` | `print("hi")` |
| expression result | `1 + 1` | `2 + 3` |
| error | `stop("boom")` | `raise Exception("boom")` |
| source-only (no output) | assignment `x <- 1` | assignment `x = 1` |

run `record_capture` with the real engine, parse `result.markdown`, and
assert the two block trees have equal **shape signatures**. Signature =
recursive tree of `(node kind, semantic classes)` where *semantic classes*
is the intersection with `{cell, cell-code, cell-output}` — i.e. we pin the
structure the splice/CSS/transforms rely on, while allowing the language
class (`r` vs `python`) and the output-subtype class to differ. Signature
computation is a small pure helper in the test module; content bytes are
ignored.

**Flagged for review (decision 1 corollary):** the same logical "expression
evaluates to a value" case produces `.cell-output-stdout` under knitr (R
autoprints to stdout) but `.cell-output-display` under jupyter
(execute_result). This is inherited from Q1 semantics; the block *structure*
is identical (a `.cell-output` div wrapping a plain fence) and CSS treats
both, so the parity signature above deliberately tolerates it. If we instead
want subtype-class parity too, say so and the suite tightens to full class
equality with a normalization table.

## Discrepancy log (decision 1 corollary)

Divergences between the engines (or inherited from quarto-cli) found while
working this strand. Each is either resolved toward the common structure
here, tolerated deliberately, or filed as a follow-up.

1. **knitr autoprint→`-stdout` vs jupyter execute_result→`-display`**
   (flagged pre-execution, see the parity-suite design above). Same logical
   "expression evaluates to a value"; identical block structure; only the
   output-subtype class differs. Inherited from Q1 semantics; CSS treats
   both. **Tolerated** — the parity signature compares semantic classes
   only. Carlos reviewed the recommendation 2026-07-02 (decision round 1).
2. **Error policy: jupyter embeds cell errors and continues; knitr fails
   the render** (found 2026-07-02 while writing `parity_error_output`).
   With a plain failing cell, knitr's pipeline errors out with no capture —
   Q1's default `execute.error: false` policy — while q2's jupyter
   unconditionally embeds `.cell-output-error` and reports success. This is
   a *behavioral* divergence, out of scope for this shape fix (it needs
   `#|` directive awareness in the jupyter text path). **Filed as
   bd-ohvl879u** (discovered-from bd-gthycd33). The parity suite pins the
   error-output *shape* under `#| error: true`, which both engines execute
   today.

## Work items

### Phase 1 — tests first (TDD)

- [x] Keep the two repro tests as the engine-gated regression pair —
      renamed to `capture_splice_engines.rs`
      (`jupyter_capture_splices_into_preview_ast` /
      `knitr_capture_splices_into_preview_ast`), extended to assert the
      spliced AST replaces the cell with a `Div.cell` via `splice_cells`,
      and gated with a runtime skip (not `#[ignore]` — CI runs without
      `--run-ignored`, so an ignored fence would never fire anywhere; with
      runtime gating it runs wherever engines are installed and skips
      elsewhere). ✅ jupyter FAILS / knitr PASSES (2026-07-02, this
      worktree off main).
- [x] Add **pure unit tests** (no kernel needed) in `text_execute.rs`:
      10 exact-string shape tests (echo fence with `.cell-code`, output
      divs per class, `render_cell` wrapper with/without outputs, fence
      sizing per Q1's `ticksForCode` — max(3, longest leading backtick
      run + 1); the current fixed ``` is a latent corruption bug for
      outputs containing backticks). The three old shape tests
      (`test_echoed_source_fence_strips_braces`,
      `test_format_outputs_stream`, `test_format_outputs_error`) replaced
      by exact-equality versions. ✅ red confirmed 2026-07-02: E0425 for
      the not-yet-existing `render_cell` (format_outputs assert-reds are
      masked by the compile error until it exists).
- [x] Add the **cross-engine parity suite** —
      `crates/quarto-core/tests/integration/engine_output_parity.rs`, 4
      input pairs (stream, expression value, error, source-only), recursive
      shape-parity walker (block kinds + semantic classes ∩ {cell,
      cell-code, cell-output}), runtime-skip unless both engines available.
      ✅ all 4 FAIL with the expected mismatch (knitr `[Paragraph, Div,
      Paragraph]` vs jupyter `[Paragraph, CodeBlock, CodeBlock, Paragraph]`),
      2026-07-02. The error pair uses `#| error: true` in both cells — see
      discrepancy log entry 2.
- [x] Run: new/updated unit tests fail ✅ (red = E0425 compile error for
      the not-yet-existing `render_cell`); repro jupyter test fails ✅
      (map 0 entries); parity suite fails ✅ (all 4 pairs, expected
      mismatch). knitr splice test passes ✅ (control). 2026-07-02.

### Phase 2 — implementation

- [x] Rework the emission in `text_execute.rs`: new `render_cell` wraps
      echoed source + outputs in `::: {.cell}`; `echoed_source_fence` emits
      `{.<lang> .cell-code}`; `format_outputs` emits
      `::: {.cell-output .cell-output-{stdout,stderr,display,error}}` divs
      around plain fences (`fenced_output_div`); all fences sized via
      `ticks_for_code` (Q1 rule). Also: a guard in `execute_blocks_inner`
      ensures the `::: {.cell}` opener starts its own block when the source
      lacked a blank line before the cell (fenced divs can't interrupt a
      paragraph), and `text/plain`/`text/html` mime values now go through
      `extract_text_content` (handles nbformat's array-of-lines form the
      old `as_str()` silently dropped — unit-tested).
- [x] **AST-path disposition (decision 4): retired.** `JupyterTransform` +
      `outputs_to_blocks` had **no production consumer** (grep: only
      `jupyter_integration.rs` tests) — production execution is
      `ExecutionEngine::execute` → `text_execute.rs`. Deleted
      `jupyter/transform.rs` + `jupyter/output.rs`; moved the still-used
      helpers (`strip_ansi_codes`, `extract_text_content`) + their tests
      into `text_execute.rs`; pruned the transform/inline-expr tests from
      `jupyter_integration.rs` (their durable coverage lives on: kernel
      persistence → `parity_dependent_cells` + `test_full_pipeline_multiple_cells`;
      inline `{python} expr` was a prototype only the retired path had →
      filed **bd-u996g8g2**). One emission path remains, by construction.
- [x] Added a 5th parity case, `parity_dependent_cells` (two cells, second
      reads state from the first) — pins kernel-state persistence through
      the production path *and* multi-cell shape, for both engines.
- [ ] While consulting `external-sources/quarto-cli`, log any further
      jupyter-vs-knitr structural discrepancies in this plan (decision 1
      corollary) — resolve toward the common structure, flag for review when
      in doubt. (Ongoing through Phase 3/4.)
- [x] All Phase 1 tests pass with real kernels: 19 `text_execute` unit
      tests, both `capture_splice_engines` tests, all 5 parity cases
      (2026-07-02). Also ran the `#[ignore]`d kernel suite: 25 pass; the 3
      failures are **pre-existing on main** (verified by stashing this
      work and re-running the baseline): `test_full_pipeline_*` panic on a
      nested tokio runtime in `execute_blocks_async` (filed
      **bd-2me3cslx**), `test_kernel_execute_matplotlib` needs matplotlib
      which this venv lacks (environmental).

### Phase 3 — regression sweep

- [x] `cargo build --workspace` ✅ clean (2026-07-02).
- [x] `cargo nextest run --workspace` ✅ **10181 passed, 0 failed** (1
      "leaky" warning — the knitr R child-process handle, passes; also seen
      intermittently on the standalone run).
- [x] `cargo xtask verify` (full, WASM leg included) ✅ "All verification
      steps passed!" (2026-07-02). Also, separately: `cargo clippy -p
      quarto-core --all-targets` clean, `cargo xtask lint` clean,
      `cargo fmt --check` clean.
- [x] Grep for consumers assuming jupyter's old bare-fence shape: **none**.
      All `cell-output` consumers found expect the *div* form and start
      matching better with this fix: `resources/scss/bootstrap/
      _bootstrap-rules.scss` (`.cell .cell-output-stdout pre code`),
      `quarto-core/src/project/listing/post_render_upgrade/reader.rs`
      (`div.preview-image div.cell-output-display img`), plus an inert
      fixture string in `quarto-test/src/assertions/html_elements.rs`.
      hub-client TS / ts-packages: no `cell-output` references at all.

### Phase 4 — end-to-end verification (per CLAUDE.md)

- [x] **CLI e2e through the real binary** (2026-07-02): rendered a
      two-cell jupyter doc (expression `2 + 3` + `print("streamed
      output")`) with `./target/debug/q2 render hello.qmd` (the
      workspace-built binary; scratch fixture). Inspected `hello.html`:
      two `<div class="cell">` wrappers; `<div class="cell-output
      cell-output-display"><pre...><code>5</code>`; `<div class="cell-output
      cell-output-stdout">...<code>streamed output</code>`; echoed source
      carries `class="sourceCode cell-code code-with-copy"` with highlight
      spans present. Output inspected directly, not inferred.
- [x] `q2 preview` browser e2e ✅ (2026-07-02). Chain: `cargo xtask verify`
      (rebuilt WASM + q2-preview-spa dist) → `cargo build --bin q2`
      (re-embed) → `./target/debug/q2 preview <scratch>/hello.qmd` →
      opened `http://127.0.0.1:59606` in a real Chrome tab. Inspected the
      preview iframe DOM: **2 `div.cell` wrappers; `.cell-output
      .cell-output-display` containing `5`; `.cell-output
      .cell-output-stdout` containing `streamed output`** — i.e. the
      server-recorded capture spliced into the WASM-rendered preview,
      which is exactly the surface that failed in the original report
      (the WASM markdown engine cannot compute `5` itself). Screenshot
      captured in the session transcript; the rendered page shows both
      highlighted cells with their outputs below.
- [ ] Optional cross-check on the feature branch: re-run the Option-B hub
      harness (`claude-notes/hub-execution-e2e/`, feature branch only) after
      this fix merges — `hello.qmd` Run should splice like `r-demo.qmd`.

### Phase 5 — close out

- [ ] Commit (pre-commit review checklist done; awaiting Carlos's
      approval), then `braid close bd-gthycd33`.
- [x] File deferred follow-ups as strands (all `discovered-from:
      bd-gthycd33`): **bd-ohvl879u** (error-policy divergence: jupyter
      embeds cell errors and continues where knitr fails the render),
      **bd-2me3cslx** (pre-existing broken `#[ignore]`d full-pipeline
      tests: nested tokio runtime), **bd-u996g8g2** (inline `{python}
      expr` in the text path; the retired prototype is the reference),
      **bd-5t6wvu7m** (real image outputs instead of the placeholder;
      TODO in code now carries this id), **bd-vs7sa0qx**
      (`execution_count` attributes when a consumer appears — decision 3).
      The AST path did NOT need a follow-up: it was retired in Phase 2.

## References

- Strand: bd-gthycd33 (comment c-c3qmbpa3 has the original browser repro).
- Discovery context: `claude-notes/plans/2026-07-01-merge-preview-status-line.md`
  ("End-to-end evidence" → "Orthogonal bug found"), on
  `feature/hub-execution-provider`.
- Splice design: `crates/quarto-core/src/engine/capture_splice.rs` module
  docs; `claude-notes/plans/2026-05-18-q2-preview-project-replay-engine.md`.
- knitr's wrapper emission: `crates/quarto-core/src/engine/knitr/resources/rmd/hooks.R`
  (vendored Q1 hooks).
- Quarto 1 jupyter emission (shape reference only): `external-sources/quarto-cli`.
