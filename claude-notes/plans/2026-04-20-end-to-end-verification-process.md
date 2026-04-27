# Process improvement: end-to-end verification before declaring "done"

- **Date**: 2026-04-20
- **Author**: Claude, at user request
- **Beads**: bd-469o (discovered-from bd-n7x2)

## Motivation

On 2026-04-20 a session reported Phase 2 of the syntax-highlighting
plan (`claude-notes/plans/2026-04-19-syntax-highlighting-design.md`)
complete. Every test passed, lints were clean, hub-client + SCSS +
trace-viewer builds were green. The user then rendered a real `.qmd`
file via `quarto render` and observed **no syntax highlighting**.

Root cause (now fixed): `render_qmd_to_html` in
`crates/quarto-core/src/pipeline.rs` had two branches for the HTML
stage list — one used when `css_paths` was empty, one used when it was
non-empty. The tests supplied the default (empty-`css_paths`) config,
which used `build_html_pipeline_stages()` (includes `CodeHighlightStage`).
The CLI always supplies a non-empty `css_paths` (it writes
`styles.css`), which used an inlined stage list that omitted
`CodeHighlightStage`. The two code paths had drifted, tests exercised
one and not the other, and "tests pass" was mistaken for "feature
works."

This is not a one-off. It's an instance of a recurring failure mode:
**declaring a feature complete based on the test suite without
exercising the end-to-end code path a real user would hit.**

## Goals of this plan

1. Amend `CLAUDE.md` so future sessions are instructed to exercise
   features end-to-end before declaring them complete, and to report
   the invocation + observed output in the session transcript or plan
   document.
2. Establish a convention: feature plans include an explicit
   end-to-end-verification acceptance criterion per phase (the
   syntax-highlighting plan has been amended with this pattern as a
   reference).
3. Encourage test helpers that exercise the same code paths as the
   real binary (`quarto render`) rather than in-process calls that
   bypass config branches.

## Work items

- [x] Fix the underlying highlighting bug (unify the two branches of
      `render_qmd_to_html`).
- [x] Add a regression test that exercises the non-empty-`css_paths`
      code path and asserts on `hl-*` spans.
- [x] Amend `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`
      with the post-mortem and per-phase end-to-end acceptance
      criterion.
- [ ] Amend `CLAUDE.md` with explicit end-to-end verification
      requirements. Proposed section text is below.
- [ ] Create a beads issue linking to this plan, covering the
      `CLAUDE.md` amendment and any follow-up tooling (test helpers
      that drive the binary).
- [ ] (Follow-up; not in this session) Audit existing `quarto-core`
      pipeline tests for similar config-branch coverage gaps. Are
      there other stages that live in one branch but not the other?
      Are there other pipeline builders (`build_wasm_html_pipeline`,
      etc.) whose tests run against a different code path than the
      real caller?
- [ ] (Follow-up) Explore adding a test helper crate or module that
      wraps `render_document_to_file` so every feature touching the
      HTML pipeline can assert against the *file the user would see*,
      not just an in-memory `RenderOutput`. A handful of `quarto
      render`-equivalent integration tests should replace ad-hoc
      `render_qmd_to_html` tests wherever possible.

## Proposed `CLAUDE.md` addition

A new section under "Testing instructions" (or a new top-level
section; placement to be decided during the edit):

> ## End-to-end verification before declaring success
>
> Tests passing is **necessary but not sufficient** to declare a
> feature complete. Before reporting a feature done, you MUST:
>
> 1. **Exercise the feature end-to-end through the binary a real user
>    would run.** For CLI features, that means `cargo run --bin q2 --
>    render <fixture>.qmd` (or the equivalent). For hub-client
>    features, that means a real browser session against a running
>    hub. In-process tests that call library functions directly do NOT
>    count as end-to-end verification — they may bypass config
>    branches, CLI argument parsing, file I/O, or pipeline builders
>    that the real binary uses.
>
> 2. **Inspect the actual output.** Read the generated file, view the
>    rendered HTML in a browser if UI is involved, grep for the
>    expected markup. Do not infer success from the absence of errors.
>
> 3. **Record the end-to-end example in your communications.** Either
>    in the session transcript (when reporting completion) or in the
>    plan document for the feature, include:
>    - the exact invocation used,
>    - a snippet of the observed output demonstrating the feature,
>    - an explicit note that the output was inspected.
>
> 4. **Prefer test helpers that drive the binary.** When adding tests
>    for a CLI-visible feature, route through `render_document_to_file`
>    (or the equivalent end-to-end entry point) with realistic config
>    — not `render_qmd_to_html` with `HtmlRenderConfig::default()`. If
>    the feature activates only under a specific config branch, make
>    sure at least one regression test hits that branch.
>
> If you cannot test a feature end-to-end (e.g. no access to a browser
> for a hub-client change), **say so explicitly** rather than claiming
> success based on unit tests alone. "Tests pass, I did not verify the
> real render path" is a valid and honest status update.
>
> **Why this matters:** tests verify the contract the test author had
> in mind. Real invocations verify the contract the user is relying
> on. These are not the same thing. Past incidents where they
> diverged:
> - 2026-04-20: `CodeHighlightStage` never ran under `quarto render`
>   because the CLI path used a different pipeline builder than the
>   tests. See
>   `claude-notes/plans/2026-04-19-syntax-highlighting-design.md`,
>   "Phase 2 post-mortem".

## Open questions

- Should we add a CLI smoke test helper that literally shells out to
  the `q2` binary? (The existing `smoke_all` test runs `q2` via
  `Command::new`; we may want more.) Track as follow-up if this plan
  generates enough demand.
- Should plans living in `claude-notes/plans/` be required to include
  a phase-end verification section, not just a checklist? Worth
  discussing with the user before making it a hard rule.
