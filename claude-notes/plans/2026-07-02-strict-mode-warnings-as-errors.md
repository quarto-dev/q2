# Strict mode: promote warning diagnostics to errors (GH #220)

- **Braid strand:** bd-yjs54ptg
- **GitHub issue:** https://github.com/quarto-dev/q2/issues/220
- **Status:** feasibility study + implementation plan (no code yet)

## Overview

GH #220 asks for a strict mode where warnings become errors, primarily for
CI. The design goal stated for this study: find the (hopefully few, central)
locations where warnings flow, and upgrade warning diagnostics to errors
there, so the command *naturally* fails with what are now errors — and so
**future warning diagnostics automatically participate in strict mode with
zero per-call-site work**.

**Verdict: feasible, and cheap.** The architecture already has the seam we
need. The recommended design touches ~4 places, all policy-side (CLI +
summary type); the diagnostic engine stays policy-free, consistent with
Decision D1 (bd-creo) and the config-error-handling decision of 2025-12-07
("the caller decides").

## How diagnostics flow today (verified 2026-07-02)

1. **One diagnostic type.** `DiagnosticMessage` in `quarto-error-reporting`
   (source: `external-sources/quarto-error-reporting/src/diagnostic.rs:215`)
   with `kind: DiagnosticKind` (`Error | Warning | Info | Note`,
   `diagnostic.rs:9`). A warning differs from an error **only** by this
   field. Promotion is a one-field mutation. The crate has no built-in
   severity-promotion affordance — by design, policy is consumer-side.

2. **Many emitters, no single emission chokepoint.** Warnings are pushed
   into `StageContext.diagnostics` (`crates/quarto-core/src/stage/context.rs:94`),
   `RenderContext.diagnostics` (`crates/quarto-core/src/render.rs:233`),
   pampa's `DiagnosticCollector`
   (`crates/pampa/src/utils/diagnostic_collector.rs`), and ~15 transforms
   that take a bare `&mut Vec<DiagnosticMessage>`. Lua `quarto.warn()` is
   harvested into real `DiagnosticMessage`s
   (`crates/pampa/src/lua/diagnostics.rs:354`), so filter warnings ride the
   same rails. Emission-point promotion would therefore **not** be "few,
   central" — but it doesn't need to be, because:

3. **Everything converges on `ProjectRenderSummary`**
   (`crates/quarto-core/src/project/orchestrator.rs:481`), via exactly four
   fields: `pass1_failures[].diagnostics`, `pass2_failures[].diagnostics`,
   `project_diagnostics`, and per-output `outputs[].render_output.diagnostics`
   (exposed by the `OutputDiagnostics` trait, `orchestrator.rs:546`).
   Both the printing (`print_render_diagnostics`, text and `--json-errors`
   paths, `crates/quarto/src/commands/render.rs:840`) and the error/warning
   tally (`ProjectRenderSummary::diagnostic_counts`, `orchestrator.rs:583`)
   operate **post-run on the summary** — nothing structured is streamed to
   the user mid-render. This is the central seam.

4. **The exit-code boundary is one function**: `should_exit_nonzero`
   (`crates/quarto/src/commands/render.rs:830`), called from both
   `execute_single_doc` (:728) and `execute_project` (:811). Today it
   returns true iff pass1/pass2 failures exist or a *project-level*
   diagnostic has `kind == Error`.

5. **Precedent already in-tree:** `quarto-doctemplate`'s `EvalContext` has a
   working `strict_mode` flag with `warn_or_error_at` /
   `warn_or_error_with_code` (`crates/quarto-doctemplate/src/eval_context.rs:125-232`).
   That's the emission-side pattern; we deliberately do **not** generalize
   it (see "Alternatives rejected").

### Two latent facts that shaped the design

- **Per-document diagnostics on *successful* outputs never affect the exit
  code today — even `Error`-kind ones.** `should_exit_nonzero` only looks
  at failures + `project_diagnostics`. So "promote kind and the command
  naturally fails" is *almost* true: promotion must be paired with a small
  exit-gate extension, otherwise a promoted warning on a successful render
  would print as `error` and still exit 0. (This is arguably a latent
  inconsistency even without strict mode — see Open Questions.)
- **~552 `eprintln!` and ~63 `tracing::warn!` call sites bypass the
  structured system entirely.** Strict mode structurally cannot see these.
  That is acceptable (they are logging, not user-facing diagnostics), but
  it makes the convention "user-visible warnings must be
  `DiagnosticMessage`s" load-bearing. A follow-up xtask lint could police
  new `eprintln!("warning: ...")`-style output in quarto-core.

## Recommended design: promote at the summary boundary

Add a `--strict` flag to `quarto render`. When set, after `pipeline.run()`
returns and **before** printing:

```rust
if args.strict {
    summary.promote_warnings_to_errors();
}
```

where `ProjectRenderSummary::promote_warnings_to_errors()` (new, in
`orchestrator.rs`) walks the four diagnostic sources and flips
`DiagnosticKind::Warning` → `DiagnosticKind::Error`. `Info`/`Note` are left
alone. Then extend the exit gate:

```rust
fn should_exit_nonzero(summary: &ProjectRenderSummary, strict: bool) -> bool {
    if !summary.pass1_failures.is_empty() || !summary.pass2_failures.is_empty() {
        return true;
    }
    if strict && summary.diagnostic_counts().errors > 0 {
        return true; // post-promotion, this is exactly "any former warning or real error anywhere"
    }
    summary.project_diagnostics.iter().any(|d| d.kind == DiagnosticKind::Error)
}
```

### Why this satisfies the sustainability requirement

Every structured diagnostic — from pipeline stages, transforms, pampa,
Lua filters, project post-render — already drains into the summary; both
existing consumers of severity (printer, counter) read the summary
post-promotion. A **new warning added anywhere in the codebase tomorrow
automatically**: prints as `error` under `--strict` (text and
`--json-errors` alike), counts as an error in the summary line, and fails
the command. No per-call-site opt-in, no new emission API to remember.

### What the user sees

- Without `--strict`: unchanged.
- With `--strict`: diagnostics render with error severity (ariadne styling,
  JSON `"kind": "error"`), the counts clause reports them as errors, and
  the process exits 1. Consistent labeling everywhere because promotion
  happens *before* any output is produced.

### Touched locations (the "few, central" list)

1. `crates/quarto/src/commands/render.rs` — `RenderArgs.strict` (clap flag,
   modeled on `fail_fast`, :89), the two `promote` call sites (or one, if
   the shared tail of `execute_single_doc`/`execute_project` is factored),
   and the `should_exit_nonzero` extension.
2. `crates/quarto-core/src/project/orchestrator.rs` —
   `promote_warnings_to_errors()`; add `diagnostics_mut()` to the
   `OutputDiagnostics` trait (with the same `#[cfg(target_arch = "wasm32")]`
   empty-impl shape as the existing `diagnostics()`), or implement the
   method directly on `ProjectRenderSummary<RenderToFileResult>`.

Nothing in `quarto-error-reporting` (external crate) changes.

### Scope boundaries

- **`q2 preview` / hub-client / WASM: intentionally out.** Decision D1
  makes preview/hub deliberately lenient (partial progress over strictness);
  the WASM response even carries `warnings` as a separate field
  (`crates/wasm-quarto-hub-client/src/lib.rs:616-621`). Strict mode is a
  CLI-render policy. If browser strictness is ever wanted, the promotion
  point would be where the WASM response assembles its
  `warnings`/`diagnostics` fields.
- **`quarto-doctemplate`'s internal `strict_mode`: untouched.** Its
  warnings surface upward as ordinary `DiagnosticMessage` warnings and get
  promoted at the boundary like everything else.
- **`eprintln!`/`tracing` output: out of scope** (not structurally
  reachable; see above).
- **Render control flow: unchanged.** Strict mode does not abort rendering
  mid-flight or change what gets rendered — it renders everything, then
  reports and fails. (`--fail-fast` keeps its current meaning: stop at
  first *failure*; it does not learn to stop at first warning in v1.)

## Alternatives rejected

- **Exit-gate-only** (`strict && warnings > 0` → exit 1, no kind
  promotion): smallest diff, but the command fails while the output still
  says "warning" — confusing in CI logs, and the issue/user intent is
  explicitly "what are now errors".
- **Emission-point promotion** (generalize doctemplate's
  `warn_or_error_*` everywhere): no central emission chokepoint exists
  (~15 bare-`Vec` pushes, several sink types, plus Lua harvesting); every
  future warning author would need to remember the strict-aware API —
  exactly the unsustainable shape we're avoiding. It would also let strict
  mode alter mid-render control flow (`has_errors()` checks, fail-fast
  aborts), changing *what renders* rather than just the outcome.
- **Promotion inside `quarto-error-reporting`**: violates the
  engine-stays-policy-free design (D1; 2025-12-07 config plan) and would
  require a release cycle of the external crate for no benefit.

## Work items (TDD order)

### Phase 1 — tests first

- [x] Pick/build a fixture with a stable, successful-render warning:
      an unresolved crossref (`@fig-nonexistent`) — confirmed empirically
      to render successfully with one warning, exit 0.
- [x] CLI integration tests:
      `crates/quarto/tests/integration/strict_mode.rs` (7 tests).
      Verified failing first: 6 failed with clap's
      `unexpected argument '--strict'`, baseline test passed.
- [x] Unit tests for `promote_warnings_to_errors` covering all four
      summary sources; `Info`/`Note` untouched (orchestrator.rs tests).
- [x] Unit tests for the `should_exit_nonzero` strict arm (render.rs
      tests: warning-only, promoted, clean, info-only).
- [x] `--json-errors --strict` test: emitted JSON `kind` is `"error"`.
- [x] Project-render variant (multi-file, warning in one file).

### Phase 2 — implementation

- [x] `RenderArgs.strict` + clap wiring (`--strict`, global to the render
      subcommand, modeled on `--fail-fast`) + `--help` text.
- [x] `OutputDiagnostics::diagnostics_mut` (native + wasm impls) +
      `ProjectRenderSummary::promote_warnings_to_errors`.
- [x] Promotion call + `should_exit_nonzero(summary, strict)` in both
      execute paths. The strict arm defensively checks `warnings > 0` too,
      so the gate stays correct even if a caller forgets to promote.
- [x] `cargo build --workspace` clean; `cargo nextest run --workspace`:
      9884 passed. Full `cargo xtask verify` (WASM leg) run as well.

### Phase 3 — end-to-end + docs

- [x] End-to-end verification (2026-07-02, real binary, output inspected):

      ```
      $ q2 render warn.qmd --strict        # warn.qmd contains @fig-nonexistent
      Error: unresolved crossref `@fig-nonexistent`: no target with this identifier was found.
      1 error
      EXIT: 1                              # warn.html still written (905 bytes)

      $ q2 render warn.qmd                 # without --strict: unchanged
      Warning: unresolved crossref `@fig-nonexistent`: ...
      1 warning
      EXIT: 0

      $ q2 render warn.qmd --strict --json-errors
      {"$schema":".../json-diagnostic.json","kind":"error","title":"unresolved crossref ..."}
      ```

- [x] User-facing docs: "Rendering in CI" section in
      `docs/guides/publishing/index.qmd`; page verified to render via
      `q2 render docs/guides/publishing/index.qmd`.
- [x] Close the loop on GH #220: PR
      https://github.com/quarto-dev/q2/pull/362 closes it on merge and
      links this plan + strand bd-yjs54ptg.

## Decisions (Carlos, 2026-07-02)

1. **Latent inconsistency** (Error-kind diagnostic on a successful output
   exits 0): **separate strand**, likely handled right after this one.
   Filed as a discovered-from strand of bd-yjs54ptg.
2. **Config surface**: **CLI-flag-only** for v1; no `_quarto.yml` key.
3. **Flag name**: **`--strict`**, with help text spelling out the
   warnings-become-errors semantics.
4. **`--strict --fail-fast` interplay**: **keep orthogonal**. Under
   `--strict --fail-fast`, a promoted warning does not stop the render
   (fail-fast still means "stop at first *failure*"). Accepted as
   not-ideal; not worth reconsidering the parallelism / diagnostic-merge
   design until there is strong evidence of shortcomings.
