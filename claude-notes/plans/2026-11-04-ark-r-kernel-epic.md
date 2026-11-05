---
title: 'Run R via the Ark Jupyter kernel (bd-yt2o2ovt)'
date: 2026-11-04
description: 'Retest of every claim in the Ark epic against current main, and a TDD plan for the decided fixes (base64, image diagnostics, kernel resolution) so a Jupyter-engine document naming the ark kernel renders R cells and plots, using an explicit claims entry until the resolver redesign in Epic B.'
status: spikes-done  # S1-S5 done 2026-11-04, no source changed; split 2026-11-04: the resolver redesign (old D2/D2b, Phase 4) and design review moved to Epic B; no code changed yet
braid:
  strand: bd-yt2o2ovt  # "Epic: run R via the Ark Jupyter kernel in q2"
  priority: P3
---

# Overview

Goal (from the epic): a `.qmd` with `{r}` cells renders in q2 using Ark
(posit-dev/ark) as the Jupyter kernel, with figures, **without workarounds**,
and with clear diagnostics when something goes wrong.

Done when `~/docs/ark/example.qmd` renders all three chunks (including the
plot) with `jupyter: {kernel: ark}` plus **one explicit claims entry** giving
`r` to Jupyter, and no `ir` kernelspec copy. Spike S1 fixed the spelling as
`jupyter: {kernel: ark, claims: {r: 1}}` with no `engine:` key.

**Split (2026-11-04).** The original done-when (bare `engine: jupyter` plus
`jupyter: {kernel: ark}`, no `claims`) needs a resolver change: knitr\'s
`Primary(1)` claim on `r` beats Jupyter\'s `Fallback`. A design review found
that change (old D2, amended to D2b) is bigger than it looked and tangled with
open questions, so it moved to **Epic B**,
`claude-notes/plans/2026-11-04-engine-interface-review.md`, together with the
review, recommendations R1 to R7, diagnostics, and the resolution corpus
growth. This epic keeps what is decided. The claims entry is the interim
route; Epic B removes the need for it.

Branch: `braid/bd-yt2o2ovt-ark-r-kernel` in `.worktrees/workspace-7`, cut from
`origin/main` at `94e1f8afe`.

# Retest of the claims (2026-11-04)

Binary: `target/debug/q2` built in this worktree from `94e1f8afe`. Kernel: Ark
0.1.252 registered by `ark --install` (kernelspec `ark`, no `ir`). Python
environment: a scratch venv with `jupyter` (see the environment note below).
Every row was run, and the output or error was read.

| Strand | Claim | Result | Evidence |
|---|---|---|---|
| bd-rorntup4 | `engine: jupyter` + `{r}` silently runs knitr | **Confirmed** | `engine: jupyter` and `jupyter: {kernel: ark}` printed `processing file: a.rmarkdown` (knitr). |
| bd-rorntup4 | `engine: [jupyter, knitr]` still gives `r` to knitr | **Confirmed** | Same `.rmarkdown` line. |
| bd-rorntup4 | A bare `jupyter:` key does not select Jupyter | **Refuted** (corrects the strand) | The key already selects Jupyter as an explicit engine (`explicitly_declared_engines`, `resolution.rs:409`): `jupyter: {kernel: ark, claims: {r: 1}}` with no `engine:` reached the Jupyter engine. Without `claims`, `{r}` still ran knitr, for the same T1 reason as `engine: jupyter`. |
| bd-9pvolzwl | `jupyter: {kernel: ark}` is ignored; `r` maps to `ir` | **Confirmed** | With claims forcing Jupyter: `kernelspec 'ir' not found`. |
| bd-9pvolzwl | The `engine: {jupyter: {kernel: ...}}` spelling is ignored too | **Confirmed** | Same `ir` error with `kernel: ark` inside the engine map. |
| bd-9pvolzwl | A nonexistent kernel is silently accepted | **Confirmed** | `jupyter: nosuchkernel` on a `{python}` doc rendered successfully. |
| bd-db68qtuf | Kernel name is hardcoded by language | **Confirmed in code** | `text_execute.rs:76` and `:96`; `resolve_kernel` and `find_kernelspec_for_language` are still `#[allow(dead_code)]`. |
| bd-wzz90981 | Unpadded base64 from Ark drops every plot | **Confirmed** | With Ark shimmed as `ir`: `invalid base64 ... Invalid padding`, no `<img>`, no `_files/` figure. |
| bd-pcg42lej | Fallback `[Image output]` triggers Q-2-49 | **Confirmed** | `Warning [Q-2-49]: [Image output] has no attribute block...`; the output reads `Image output`. |
| bd-pcg42lej | Decode failure is nearly invisible | **Partly** | A `tracing` WARN does appear without `RUST_LOG`, but it names no cell or source location and is not a q2 diagnostic. The strand overstates this slightly. |
| bd-4nypw3wq | `claims: {r: N}` workaround documented | **Workaround confirmed (Spikes S1, S3)** | `jupyter: {kernel: ark, claims: {r: 1}}` (no `engine:` key) and the project-level forms give `r` to Jupyter. The docs page is still unwritten (Phase 5). |
| bd-4nypw3wq | Ark renders the three chunks once `r` reaches Jupyter | **Partly (Spike S5)** | With Ark shimmed as `ir`: chunks 1 and 2 render; the plot fails only on unpadded base64 (Phase 1). |

New observations (not in any strand):

- The kernel reaches `ctx.engine_config` when `engine:` is a map, or when the
  top-level `jupyter:` shorthand is used with no `engine:` key. With
  `engine: jupyter` (bare string) or `engine: {jupyter: {claims: ...}}` plus a
  top-level `jupyter:` key, the kernel lives only in the merged metadata
  (`ctx.metadata`). Wiring must read both (confirmed by Spike S2).
- `extract_kernel_from_metadata` takes a `ConfigValue`, but the execution
  context carries `engine_config: Option<ConfigValue>` and
  `metadata: HashMap<String, TsMetadataValue>`. The dead helper needs a new
  signature, not just a call site.

Environment note: `~/docs/ark/.venv` is currently unusable. Its interpreter
(uv `cpython-3.13.7`) is killed on launch (exit 137); 3.12.12, 3.13.1 and 3.13.9
run fine. I did not touch that venv. The retest used a scratch venv on 3.13.9.
Recreating the venv (`uv venv --python 3.13.9`) fixes it.

# Design decisions

## D1. Kernel resolution order (bd-9pvolzwl, bd-db68qtuf)

Replace `map_language_to_kernel` with one async resolver, used in
`execute_qmd`, with this order (Q1 `jupyterKernelspecForLanguage` parity for
step 3):

1. Explicit kernel name: `engine: {jupyter: {kernel: X}}`
   (`ctx.engine_config`), then `jupyter: X` or `jupyter: {kernel: X}` (merged
   metadata). First hit wins. A name that is not a registered kernelspec is a
   **hard error** listing available kernels (the existing
   `KernelspecNotFound` rendering).
2. No explicit name: find a registered kernelspec whose `language` matches the
   first executable cell\'s language (case-insensitive), via
   `find_kernelspec_for_language`.
3. No match: fall back to the old name table (`python3`, `ir`, `julia`, ...),
   so behavior only widens. The existing not-found error is unchanged.

Non-obvious point: an explicit kernel applies to the whole jupyter run. In a
multi-language document the explicit kernel is used for every executable cell
jupyter owns (Q1 behavior: one kernel per document). The plan does not add
per-language kernel tables.

D2 (explicit engines win their languages) lives in Epic B. R1 and R2
(kernel-versus-ownership check; the `kernelspec` spelling) amend D1 and are
decided there; this epic implements D1 as written above.

## D3. Image-failure diagnostics (bd-pcg42lej)

**Decision:** an undecodable or unwritable image is a *warning* diagnostic and
the render continues with the escaped placeholder. (D1\'s unknown explicit
kernel is a *hard error*, also decided 2026-11-04.)

Channel (Spike S4, resolved): `ExecuteResult.warnings` already exists and
`EngineExecutionStage` drains it into the render\'s diagnostics, as for
knitr\'s Q-18-2. Reuse it; no new field. The new
diagnostic needs an error catalog entry (`error_catalog.json`) **and** its
`docs/errors/<subsystem>/<code>.qmd` page plus `docs/_quarto.yml` sidebar entry
in the same commit (lint rules `error-docs-page-missing` and
`error-docs-sidebar-unlisted`). The cell\'s source location comes from
`ctx.source_info` + `ctx.source_context` (`map_offset`).

# Checklist

All work is TDD: write the test, watch it fail for the stated reason, then fix.
Gate each phase on `cargo clippy -p quarto-core --all-targets -- -D warnings`
and `cargo nextest run -p quarto-core`.

## Phase 0: spikes (do these first; edit this plan with the results)

Each spike is a throwaway probe: record the exact invocation, the observed
output, and the consequence for the plan below. Use the worktree build
(`target/debug/q2`) and a scratch directory. Do not change source in a spike.
Prerequisites: a working venv (see the environment note above) and Ark
registered as kernelspec `ark`.

- [x] **S1: which claims spelling gives `r` to Jupyter today?** Render a
  `{r}` document with each of: `jupyter: {kernel: ark, claims: {r: 1}}` (the
  retest reached Jupyter with this, no `engine:` key);
  `engine: {jupyter: {kernel: ark, claims: {r: 2}}}`;
  `engine: jupyter` plus a top-level `jupyter: {claims: ...}`. Expect the
  `ir` not-found error (Jupyter owns `r`, kernel not yet wired) rather than a
  `.rmarkdown` knitr line. Pick the canonical spelling; check that a `claims`
  value of `1` versus `2` behaves as the tier rules predict (tie goes to the
  explicit engine; `2` beats knitr outright).
- [x] **S2: does the kernel survive next to `claims`?** The resolver strips
  `claims` from the engine config (`strip_claims_key`). Confirm, by reading
  the code and a debug log, where `kernel` ends up for each S1 spelling
  (`ctx.engine_config` versus merged metadata). This fixes the Phase 3 wiring
  target.
- [x] **S3: project-level claims.** Put the claims entry in `_quarto.yml` and
  confirm a document with only `{r}` cells inherits it, and that the corpus
  layering rules (C50 to C56) predict the result. If it works, the Phase 5
  guide can offer a one-line project setting.
- [x] **S4: knitr\'s warning channel (D3 design step).** Find how knitr
  surfaces engine warnings to the pipeline and whether `ExecuteResult` needs a
  new `diagnostics` field. Read-only; record the answer before Phase 2.
- [x] **S5: what Ark needs end to end.** With Ark shimmed or directly, run the
  document through Phase 1\'s base64 case to confirm the only remaining failure
  after S1 and kernel selection is the unpadded payload (no further Ark-specific
  display types block the three chunks).
- [x] After the spikes, update Phase 3, 5 and 6 below, the done-when line, and
  the retest table; mark each spike done here.

### Spike results (2026-11-04)

Binary `target/debug/q2` built from the corpus commit; scratch venv (Python 3.13.9 with
`jupyter-client`, `ipykernel`) under the session scratchpad; real Ark 0.1.252.
Each row is a one-cell `{r}` document rendered with `q2 render`. "Jupyter"
means the `kernelspec 'ir' not found` error (Jupyter owns `r`, kernel not yet
wired); "knitr" means a `processing file: a.rmarkdown` line.

**S1: claims spellings.**

| Front matter | Result |
|---|---|
| `jupyter: {kernel: ark, claims: {r: 1}}` (no `engine:`) | Jupyter |
| `jupyter: {claims: {r: 1}}` and `{r: 2}` (no `engine:`) | Jupyter |
| `engine: {jupyter: {kernel: ark, claims: {r: 2}}}` | Jupyter |
| `engine: {jupyter: {kernel: ark, claims: {r: 1}}}` | Jupyter |
| `engine: {jupyter: {claims: {r: 1}}}` + top-level `jupyter: {kernel: ark}` (the `example.qmd` spelling) | Jupyter |
| `engine: {jupyter: {claims: {r: 2}}}` | Jupyter |
| `engine: {jupyter: {claims: {r: 1}}, knitr: default}` | Jupyter |
| `engine: jupyter` + top-level `jupyter: {kernel: ark, claims: {r: 1}}` | **knitr** |
| `engine: jupyter` + top-level `jupyter: {claims: {r: 2}}` | **knitr** |
| `engine: [jupyter, knitr]` + top-level `jupyter: {claims: {r: 1}}` | **knitr** |

Findings:

- Claims are honored only when they sit in the engine\'s own config: either an
  `engine:` map entry, or the top-level `jupyter:` shorthand **when there is no
  `engine:` key**. With a bare-string `engine: jupyter` the top-level
  `jupyter:` map is ignored for ownership (no config reaches
  `raw_explicit`), so its `claims` are silently dead and knitr wins. This is
  the known "an `engine:` key disables the top-level shorthand" defect
  (Epic B, finding 6); the spike adds one concrete instance, which Epic B can
  turn into a corpus case (none covers a claims map in that position today).
- `claims: {r: 1}` and `{r: 2}` behave the same in the documents above:
  knitr is never an explicit engine there, so no tie arises. The tie was
  tested separately, with knitr listed too (real Ark; Epic B corpus cases C45
  to C48): at `r: 1` the declared order decides (`[knitr, jupyter]` keeps
  knitr, `[jupyter, knitr]` gives jupyter); at `r: 2` jupyter wins in either
  order. For this epic, `1` is right because knitr is not listed. Document
  `2` as the spelling that is safe when `engine:` also lists knitr.
- **Canonical spelling for this epic:**
  `jupyter: {kernel: ark, claims: {r: 1}}` with no `engine:` key. One map,
  and the kernel and the claim sit together. The `example.qmd` spelling also
  works but splits them across two keys.

**S2: where `kernel` ends up** (code: `explicitly_declared_engines`,
`resolution.rs:409`; `strip_claims_key`, `:983`; `:813-822`). The resolver
strips only the `claims` key, and `kernel` survives:

- `jupyter:` shorthand, no `engine:` key: `ctx.engine_config` is
  `{kernel: ark}`, and the merged metadata also holds `jupyter:`.
- `engine: {jupyter: {kernel: X, claims: ...}}`: `ctx.engine_config` is
  `{kernel: X}`.
- `engine: {jupyter: {claims: ...}}` + top-level `jupyter: {kernel: X}`:
  `engine_config` has no kernel (empty map after stripping); the kernel is
  only in merged metadata.
- `engine: jupyter` + `jupyter: X`: no `engine_config`; kernel only in
  metadata (and ownership is not Jupyter\'s, per S1).

Consequence: Phase 3 wiring must read `engine_config` first, then the lowered
`jupyter` metadata key (string form or map form). Both are needed, as already
noted above.

**S3: project level.** Each of these in `_quarto.yml`, with a document holding
only `{r}` cells and no `engine:` of its own, reached Jupyter:
`engine: {jupyter: {kernel: ark, claims: {r: 1}}}`;
`jupyter: {kernel: ark, claims: {r: 1}}`;
`engines: [{jupyter: {claims: {r: 1}}}]` + `jupyter: {kernel: ark}`;
`engine: {jupyter: {claims: {r: 2}}}`. The Phase 5 guide can offer the
one-line project setting `jupyter: {kernel: ark, claims: {r: 1}}`. (I did not
cross-check against the C50 to C56 corpus cases; the observed behavior
matched a straightforward reading of them.)

**S4: warning channel.** The plan\'s premise was wrong: `ExecuteResult`
already has `warnings: Vec<DiagnosticMessage>` (`context.rs:382`, added for
knitr\'s Q-18-2 by bd-gy2ozix3). `EngineExecutionStage` drains it into
`ctx.diagnostics` (`engine_execution.rs:693`), so the diagnostics are counted,
printed, and promoted by `--strict`. The builder pattern to copy is
`unreadable_include_diagnostic` in `engine/diagnostics.rs` (a
`DiagnosticMessageBuilder::warning(...).with_code(...)`). **No new field is
needed.** Phase 2 only has to collect diagnostics inside the Jupyter
`format_outputs` path and put them in `ExecuteResult.warnings`. The next free
code in the existing `Q-18-*` family is `Q-18-4` (catalog has `Q-18-1` to
`Q-18-3`); confirm the subsystem fits when adding the entry.

**S5: Ark end to end.** With Ark shimmed as `ir` (a copy of Ark\'s
`kernel.json` under the scratch venv\'s `share/jupyter/kernels/ir`, nothing
touched in `~/Library/Jupyter`), `~/docs/ark/example.qmd` rendered chunks 1 and
2 correctly (`[1] 3` and the `summary()` table are in the HTML). The plot chunk
failed with exactly the Phase 1 error: `jupyter figure: invalid base64;
skipping mime="image/png" error=Invalid padding`, then
`Warning [Q-2-49]` and `<p><span>Image output</span></p>` in the page. A direct
`jupyter_client` probe of `plot(mtcars$wt, mtcars$mpg)` shows Ark sends a
single `display_data` with only `image/png` (25282 characters, `len % 4 == 2`,
no newlines, no other mime types); adding `==` makes it decode to a valid PNG
header. So the unpadded payload is the only blocker beyond kernel selection; no
other Ark display type is involved in the three chunks.

## Phase 1: unpadded base64 (bd-wzz90981)

- [x] Test: `write_image` given a valid PNG payload with the trailing `=`
  removed decodes and writes the file (fails today with `Invalid padding`).
- [x] Test: payload containing `\r\n`, spaces or tabs between lines decodes.
- [x] Fix: `GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent)`
  and strip all ASCII whitespace, not just `\n`.
- [x] Check `preview_record.rs:521` and `capture_files.rs:41` for the same strict
  decoder on kernel-derived data; fix only if they decode kernel payloads.
  Checked: both only round-trip q2\'s own padded encoding (and a test decode), so
  no change.
- [ ] (skipped: not cheap, Ark is not a CI dependency) Parity case in `engine_output_parity.rs` is skipped when no R kernel is
  present; add only a Python-kernel case if one is cheap (Ark is not a CI
  dependency).

## Phase 2: image-failure quality (bd-pcg42lej)

- [ ] Test: a cell whose image cannot be decoded renders with **no** Q-2-49
  warning from the placeholder.
- [ ] Test: the same render yields a diagnostic naming the cell location and
  the decode error.
- [ ] Fix 1: escape the placeholder as `\[Image output\]` and update the test at
  `text_execute.rs:~1813` that pins the old string.
- [x] Diagnostics channel decided (S4: reuse `ExecuteResult.warnings`).
- [ ] Add the catalog entry (next free code in the Q-18 family, `Q-18-4`), docs page and
  sidebar entry in one commit.
- [ ] Fix 2: emit the diagnostic from `write_image` failures (decode and I/O),
  thread it through `format_outputs` to the stage.

## Phase 3: kernel resolution (bd-9pvolzwl, bd-db68qtuf)

- [ ] Tests for D1 in `kernelspec.rs`/`text_execute.rs` using a fake
  kernelspec directory (no real kernel needed): explicit name wins over
  language; `jupyter: X`, `jupyter: {kernel: X}` and
  `engine: {jupyter: {kernel: X}}` all select X; unknown explicit name errors
  and lists available kernels; language lookup finds a kernel registered under
  a non-default name (the `ark` case); no match falls back to the old name table.
- [ ] Integration test (`tests/integration/`): `jupyter: nosuchkernel` on a
  `{python}` document fails the render (skips when no Python kernel).
- [ ] Rewrite `extract_kernel_from_metadata` to read `ctx.engine_config`
  first, then the lowered `jupyter` metadata key (string or map form), per S2; remove the `#[allow(dead_code)]` markers as the helpers get
  callers.
- [ ] Wire the resolver into `execute_qmd`, replacing `map_language_to_kernel`
  (keep the table only as the step-3 fallback).
- [ ] Cross-check the Windows build:
  `cargo clippy -p quarto-core --all-targets --target x86_64-pc-windows-msvc -- -D warnings`.

R1 (error when an owned cell\'s language does not match the explicit kernel)
and R2 (the `kernelspec` spelling) are **not** part of this phase; they are in
Epic B, phase B2. Consequence to document in Phase 5: with an explicit kernel,
every cell Jupyter owns runs in that kernel, so an Ark document should contain
`{r}` cells only (or narrow the claims to `r`).

## Phase 5: docs (bd-4nypw3wq)

- [ ] After Phase 3 lands, write a short guide section: running `{r}` cells on
  Ark or IRkernel with `jupyter: {kernel: ...}` and the claims entry chosen in
  Spike S1 (`jupyter: {kernel: ark, claims: {r: 1}}`, no `engine:` key; S3
  confirmed the same line works in `_quarto.yml`). Warn that
  `engine: jupyter` plus a top-level `jupyter:` claims map does not work. Use the integer
  form of `claims` only; the object form is silently dropped (Epic B, R5).
  Say plainly that this is the current spelling and that a later release may
  make the claims line unnecessary. Document in user terms, not resolver
  internals.

## Phase 6: end-to-end verification (epic done-when)

- [ ] Recreate a working venv (the `~/docs/ark/.venv` interpreter is broken, see
  above) and render `~/docs/ark/example.qmd` reduced to the S1 spelling
  (`jupyter: {kernel: ark}` plus the claims entry) through
  `target/debug/q2 render`.
- [ ] Inspect the HTML: three cell outputs, an `<img>` for the plot, the figure
  file under `example_files/figure-html/`, no warnings, no `.rmarkdown` in the
  log. Record the invocation and output snippet here.
- [ ] Negative runs: nonexistent kernel errors; corrupt image payload gives the
  new diagnostic and no Q-2-49.
- [ ] Confirm the bare spellings (`engine: jupyter` + `jupyter: ark`,
  `jupyter: ark` alone) still render `r` with knitr, unchanged: they are
  Epic B\'s corpus cases C10, C30 and C31 and must remain expected-fail
  there.
- [ ] Not verified by the epic and left out of scope: other Ark display types,
  errors, stdin, interrupts.
- [ ] Reconcile this checklist with what landed, then close the child strands
  and the epic with `braid close`.

# Ordering and risk

Phase 0 spikes first: they fix the claims spelling, the kernel-wiring target,
and the diagnostics channel. Phases 1 and 2 are independent of 3 and are the
safest first commits. Phase 3 must precede Phase 6 for end-to-end verification;
the code is independent. No phase here changes resolver behavior, so no
document that works today changes meaning. `quarto-core` has downstream
dependents (`wasm-quarto-hub-client`), so before pushing I will recommend
`cargo xtask verify --skip-hub-build` at minimum, plus the WASM leg since
engine code is in `quarto-core`.
