# A failed include fails the document

**Strand:** `bd-include-parse-failure-dropped-u4rdjxru`
**Repro:** `q2-positron-docs/llms-info/repros/include-parse-failure-dropped/`

## Overview

When an `{{< include >}}` could not be expanded, `IncludeExpansionStage`
removed the block from the AST, pushed a diagnostic, and let the render
carry on. The page landed on disk missing the content the include was
supposed to supply and counted toward `Rendered N of M`.

The clearest evidence that this was a defect rather than a policy is q2's
own behaviour on the *same* parse error placed inline instead of in an
included file:

| page          | shape                                              | before: HTML written | before: included content |
| ------------- | -------------------------------------------------- | -------------------- | ------------------------ |
| `index.qmd`   | includes `_broken.qmd`, which has a `Q-2-11`        | **yes**              | **missing**              |
| `direct.qmd`  | the identical `Q-2-11` inline on the page           | no                   | —                        |
| `good.qmd`    | includes a file that parses                         | yes                  | present                  |
| `missing.qmd` | includes a file that does not exist                 | **yes**              | **missing**              |

Same diagnostic, same severity, opposite outcome depending only on which
file it lived in. The missing-file case was the more alarming half: a
nonexistent include target was only a *warning* (`Q-17-2`), so a project
containing just that page rendered **exit 0, no errors at all** — a page
that quietly lost its main content and reported nothing wrong.

## Root cause

`crates/quarto-core/src/stage/stages/include_expansion.rs`. Every failure
arm ended the same way:

```rust
// See the circular-include arm for why the block is
// removed rather than skipped.
blocks.remove(i);
continue;
```

Removing the block is deliberate and correct — it is the `bd-qpvoamvu` /
PR #465 fix, which stopped the leftover shortcode from being misreported
downstream as an unrecognized `include`. What was missing is the other
half of that story: nothing downstream distinguished "expanded every
include" from "expanded some and deleted the rest", so the writer produced
the page as if the document were whole.

## Decisions

**D1 — the failure is fatal, not a louder warning.** The stage returns
`PipelineError::Structured(ParseError)` carrying the collected diagnostics
plus the document's own `SourceContext`. `Structured` (rather than
`StageError`) is what preserves ariadne's ability to draw the snippet
*inside the included file*: the `StageError` bridge synthesizes a
`SourceContext` from the including document's bytes only.

**D2 — all three arms abort, not only the parse failure.** The strand
flagged the circular-include arm (`Q-17-1`) as a separate judgement. What
settles it is internal consistency: leaving one arm non-fatal reintroduces
exactly the inconsistency this change exists to remove, and `Q-17-1` is
emitted from two sites (see D3), so its severity cannot be decided
independently of `Q-17-2`'s.

Quarto 1 is only weak corroboration here, and the PR should not claim more.
Measured against `~/bin/quarto` (99.9.9):

| condition | Q1 | q2 after this change |
| --- | --- | --- |
| include target missing | `Include directive failed. … could not find file …`, exit 1, no HTML — and it aborts the **whole project**, writing no `_site` at all | error on that page, exit 1, no HTML for it; sibling pages still render |
| circular include | `RangeError: Maximum call stack size exceeded`, exit 1, no HTML | `Q-17-1`, exit 1, no HTML for that page |
| included file fails to parse | **no analogue** — Q1's include is a *textual* splice, so there is no "parse the included file" step to fail; the repro's `5" gap` is ordinary text to Q1 and the page renders with its content | `Q-17-3` + the inner diagnostics, exit 1, no HTML for that page |

So the missing-target case is genuine parity in outcome (differing in
blast radius: Q1 fails the project, q2 fails the page — q2's established
per-page model, the same one `direct.qmd` already follows). The circular
case reaches exit 1 by *crashing*, not by its intended error:
`retrieveInclude` compares the resolved `path` against `retrievedFiles`
(`include-standalone.ts:41`) but pushes the raw `filename` (`:63`), so the
guard never matches and the recursion blows the stack. Its own
`Include directive found circular include` message is unreachable for a
relatively-written include. And the parse-failure case has no Q1
counterpart at all.

**D3 — the code-fence (listing) include arms move with them.** `Q-17-1`
and `Q-17-2` are each emitted from two sites: the block-level expander and
`splice_fence_text`. Raising the severity at one site only would give a
single error code two severities and two outcomes. A listing whose source
file is missing is the same silent hole as a missing block include.

**D4 — `Q-17-1` and `Q-17-2` become errors.** A page silently missing its
content is not a warning-grade outcome, and error severity is what makes
the diagnostic unsuppressable (`DiagnosticPolicy::suppresses` returns false
for errors) — a render that aborts must never abort for a reason the user
cannot see.

**D5 — expansion runs to completion before failing.** The `unresolved`
flag is set and the walk continues, so a document with several broken
includes reports all of them in one pass rather than one per re-render.

**D6 — `Q-17-4` stays a warning.** An include that is not the sole content
of its paragraph is a *placement* mistake — the expander was never asked to
serve that position — not an include that tried to supply content and
failed. Q1 leaves the shortcode text in place there rather than treating it
as an error.

**D7 — the suppression policy now applies on the error exit too.** A
failing stage can carry warnings collected before it out through its error;
`run_pipeline` applies `DiagnosticPolicy` to both exits so a suppressed
warning does not reappear just because the document later failed. Errors
are never suppressible, so the failure itself always survives.

**D8 — a failed render must not empty `q2 preview`'s watch set.**
`quarto_preview::config::resolve_single_file_deps` derives the
watch/sync set by running the real parse + include-expansion stages, and
it discarded everything when the stage returned `Err` — so one broken
include would strand the preview on the broken version, with the file
the author is editing no longer watched. The stage body is now factored
into a public `expand_document_includes(&mut DocumentAst, &mut
StageContext)`, which walks and records identically but hands the
document back regardless of the verdict. The render pipeline propagates
the `Err`; the dependency collector ignores it. Pinned by
`single_file_deps_survive_an_unresolvable_include`.

## Work items

- [x] Failing tests first: integration contracts in
      `include_expansion_diagnostics.rs` + `include_code_fence.rs`
      (7 failing on the pre-change binary, all with
      "expected the render to fail, but it produced a document")
- [x] `unresolved` flag on `IncludeExpander`, set by all five failure arms
- [x] `expand_includes_in_blocks` returns `PipelineError::Structured`
- [x] `Q-17-1` / `Q-17-2` raised from warning to error at both sites
- [x] `DiagnosticPolicy` applied on `run_pipeline`'s error exit
- [x] Unit tests reworked to assert the failure while keeping the
      block-removal (`bd-qpvoamvu`) assertions
- [x] Error-catalog message templates for `Q-17-1` / `Q-17-2` / `Q-17-3`
- [x] Docs pages `docs/errors/include/Q-17-{1,2,3}.qmd`
- [x] `expand_document_includes` seam so `q2 preview`'s dependency
      collector keeps the watch set when the render fails (D8)
- [x] smoke-all fixtures `includes/circular` + `includes/missing` moved
      from `noErrors` + WARN to `shouldError` + ERROR
- [x] End-to-end verification against the repro project
- [x] `cargo nextest run --workspace` green
