---
title: 'JOL-2b: Jupyter executes the languages it owns; kernel lookup by language'
date: 2026-10-28
description: 'Replaces the Jupyter engine''s hard-coded language allowlist and alias table with the resolver-assigned owned set and Q1-style kernelspec lookup by language, with an error in multi-engine documents and a warning in single-engine documents when an owned language has no runnable kernel, and fills the kernel_for(unit) seam that EXR-2 defines and multi-kernel work later varies per unit.'
status: in-progress
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md), as JOL-2b (the rest of
Phase 2 of [the Jupyter owned-languages epic](2026-10-17-jupyter-owned-languages-epic.md)).
**Depends on:** JOL-1 (Plan 4d phases 4d-A/B, [Plan 4d](2026-07-06-plan4d-owned-languages.md):
`ctx.owned_languages`) and [JOL-2a](2026-10-28-jol-2a-jupyter-key.md) (the `jupyter:` key and
`select_kernel`).
**Blocks:** [JOL-5](2026-10-28-jol-5-docs-e2e.md) and the deferred
[JOL-3](2026-10-28-jol-3-multi-kernel.md) (if it is ever reopened). It does not block
[EXR-2](2026-10-28-exr-02-unit-scanner.md) or [EXR-3](2026-10-28-exr-03-jupyter-iee.md):
EXR-2 owns the `kernel_for(unit)` seam and ships it with today\'s first-cell behavior
as the default router; this plan supplies the real policy. The two land in either
order (see the design decision on the seam). Per the dependency table in the epic, soft prerequisite of EXR-5.
**Lane:** B (`workspace-5`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 2-3

> **For implementers:** TDD, task by task. Steps use `- [ ]` checkboxes.

## Goal

The Jupyter engine runs exactly the languages the resolver assigned to it and finds each
language\'s kernel the way Q1 does. No language is named in q2 source except in an alias
table that earns its place. `{ggsql}`, `{bash}` and an arbitrary fixture language all
execute with no q2 source change.

## Background

Verified in the tree on 2026-10-28 (workspace-1, `origin/main` at `2ee0bd966`) unless marked.

- Three overlapping language lists exist. `is_executable_language`
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs:225`) lists python, python3, py,
  julia, jl, r, ruby, rb, rust, rs, typescript, ts, javascript, js and `ggsql` (the
  minimal ggsql fix is already on main). `map_language_to_kernel` (`:96`) is the alias
  table and passes an unknown language through as the kernel name. `is_jupyter_language`
  (`engine/jupyter/kernelspec.rs`, re-exported at `engine/jupyter/mod.rs:51`) has no caller
  outside its own file in `crates/`. *Verified.*
- `partition_cells` (`text_execute.rs:177`): a language in `handled_languages` is ceded;
  else an allowlisted language executes; else with `multi_engine` it fails with
  `ExecutionError::NoHandlerForLanguage` (a clean refusal that does not poison the engine,
  `engine/error.rs:139`); else it passes through unexecuted. Because ownership is only
  available as the negative projection `handled_languages`, "not ceded" is read as
  "mine". *Verified.*
- `ExecutionContext` has `handled_languages` and `multi_engine` but no `owned_languages`
  (`engine/context.rs:96-129`; a grep for the name in `crates/` finds nothing). JOL-1 adds
  it. *Verified.*
- The kernel is the first executable cell\'s language (`text_execute.rs:76`) and
  `execute_blocks_inner` takes one `kernel_name` and starts one session
  (`text_execute.rs:262-274`). Sessions are keyed by `SessionKey(kernel_name, cwd)`
  (`daemon.rs:49,90`). *Verified.*
- `find_kernelspec_for_language` (`kernelspec.rs:96`, `#[allow(dead_code)]`) takes the first
  kernelspec whose `language` equals the language, case-insensitively, with no venv
  preference; the only error is `JupyterError::NoKernelForLanguage`
  (`engine/jupyter/error.rs:29`), whose message names only the language. *Verified.*
- `parse_code_blocks` (`text_execute.rs:124`) requires a braced fence at the start of a line. The
  executor therefore never sees cells inside blockquotes or lists. *Verified.*
- **Q1 rules** (`external-sources/quarto-cli/src/core/jupyter/kernels.ts:27` and
  `jupyter.ts:508`): the lookup compares the fence language to `kernelspec.language` with
  a case-sensitive `===` (`kernels.ts:33`; **there is no alias table in Q1**; `py` or `jl`
  fences find no kernel), against the `language` value read from `kernel.json` as written.
  q2\'s `find_kernelspec_for_language` (`kernelspec.rs:96`) compares case-insensitively,
  and it must stay that way: this machine\'s Ark kernelspec has `"language": "R"`, so a
  literal "strict equality, no alias" would break R through Jupyter (the Ark kernelspec, not an epic decision). For
  Python, a kernelspec whose path contains `VIRTUAL_ENV` is preferred (`kernels.ts:41-46`;
  venv-first is right); with no
  `jupyter:` key Q1 tries each language present in the document in order and then
  `python`, taking the first with an installed kernelspec, and rejects only if none has
  one ("No kernel found for any language checked ..."). *Verified.* This answers the JOL
  epic open question about aliases: q2\'s aliases are a q2 invention.
- Existing tests that keep or change meaning, all in `text_execute.rs`:
  `test_partition_cells_owned_unrunnable_fails_loudly` (`:946`),
  `test_partition_cells_single_engine_owned_unrunnable_passthrough` (`:982`),
  `test_sql_is_not_executable_language` (`:1010`),
  `test_partition_cells_cede_when_in_handled_languages` (`:1025`),
  `test_partition_cells_ojs_default_passthrough` (`:1064`),
  `test_partition_cells_mermaid_dot_default_passthrough` (`:1085`) and the vacuity test at
  `:1113`. *Verified.*

## Design decisions

From the JOL epic (decisions 1, 4, 5) and the records epic; not to be re-litigated.

**Decided in the 2026-10-31 review (Gordon):**

- **"Explicit kernel versus the ownership gate" (open until now).** All owned cells go to the
  one kernel, including under `jupyter: sos` (JOL-2a divergence 1). "Runnable" therefore means
  only that the cell\'s language is owned by Jupyter; it does not mean the language equals the
  document kernel\'s language. A `{python}` then `{ggsql}` document with no `jupyter:` key
  still sends ggsql to the Python kernel; that SyntaxError is accepted and the supported route
  is a multiplexing kernel (SoS) chosen with `jupyter:`. State it in JOL-5.
- **Aliases: deprecate, then delete.** `py`, `jl`, `rs`, `rb`, `ts`, `js` (and `python3` as a
  fence name) are not in Q1 and not in Jupyter; they were added by Claude Code in the
  2026-01-07 commit `2b204533e` (`ts`/`js` the same day in `701651fb5`, for the deno kernel)
  with no recorded justification. Keep them working for one release with a deprecation warning
  that names the canonical language, then remove. The `r` to `ir` kernel-name translation is
  not an alias and stays until lookup by kernelspec language replaces it.
- **Unrunnable-language warning: once per document, exempting `eval: false` cells.** One
  diagnostic listing the owned-but-unrunnable languages with the install or pin hint, not one
  per fence. Cells with `eval: false` are exempt in both the warning and the multi-engine
  error. This also turns today\'s silent drop of `{sos}` fences into a warning.
- **The Python failsafe applies only when the document has at least one owned cell.** With no
  cells `select_kernel` still returns `None` (JOL-2a; EXR-3\'s error depends on it).
- **Kernelspec source.** Use the augmented list (`runtimelib::list_kernelspecs_with_jupyter_paths`,
  as the local `list_kernelspecs()` does), not the bare `runtimelib::list_kernelspecs()` that
  `find_kernelspec_for_language` calls today; the bare one misses venv kernels, which the
  venv-first rule needs.
- **The per-language `jupyter:` map form is not built by any plan** (JOL-2a warns and falls back
  for a map without `kernel`). Do not build it here unless Gordon reopens it.
- **Citation corrected (2026-10-31):** no recorded decision covers case-insensitive lookup.
  The reason is the Ark kernelspec (`"language": "R"`).
  Note that Q1 lowercases the language in its cell regex (`jupyter.ts:321-325`) but compares
  case-sensitively in the kernel lookup (`kernels.ts:33`).
- **Q-code (approved by Gordon; settled in Phase 1, 2026-10-31): `Q-18-5`, "Owned Language Has
  No Runnable Kernel".** The error form (multi-engine) and the warning form (single-engine)
  share it. `ExecutionError` cannot carry a code: codes are attached in
  `engine/diagnostics.rs`. The existing seams are `engine_error_diagnostic` (maps a variant to
  a coded diagnostic; `NoHandlerForLanguage` currently falls through uncoded) for errors, and
  `ExecuteResult::warnings` for warnings (how `Q-18-2` and `Q-18-4` travel). Smallest
  consistent route: one builder in `diagnostics.rs` that takes the severity, used by
  `engine_error_diagnostic` for `NoHandlerForLanguage` and by `execute_qmd` for the warning.
  No new `ExecutionError` variant is needed. Register per
  `claude-notes/instructions/error-message-system.md`: `Q-18-5` in
  `crates/quarto-error-catalog/error_catalog.json` plus a corpus case.
- **Reading of "runnable" with an explicit kernel (confirmed by Gordon, 2026-10-31).** Q-18-5 fires only when the document kernel is *derived*
  (no `jupyter:` key, no `engine_config.kernel`) and an owned, executor-eligible language has
  no kernelspec. With an explicit kernel every owned cell goes to it and nothing is checked,
  which is the 2026-10-31 "all owned cells go to the one kernel" decision applied literally
  (and it keeps `jupyter: sos` with `{ggsql}` from warning). Tests are written against this
  reading; if Gordon wants the check under an explicit kernel too, only the guard changes.
- **Direct-constructed contexts.** `ExecutionContext.owned_languages` defaults to empty, so
  once the gate reads it, a context built without the stage executes nothing. That is
  correct for a resolved-to-nobody document and the stage always populates it; the only
  caller of `execute_qmd` is the engine trait method. Jupyter tests that execute must set the
  owned set explicitly. No empty-means-everything fallback.
- **Sequencing (checked 2026-10-31):** EXR-2 has landed on `feature/execution-records-epic`
  (`6a82a9738`, "plan complete"), so `router.rs` (`KernelRouter`, `SingleKernelRouter`,
  `plan_units`) exists and this plan replaces the default router's policy; `partition_cells`
  now takes an `EngineUnit` view and the line numbers in Background are stale. JOL-4 also landed.

- Jupyter\'s claim stays a universal fallback; ownership is resolved by the existing
  resolver and carried in `ctx.owned_languages`. No kernelspec check at resolution time.
- A cell executes iff its language is in `ctx.owned_languages`. Ceded cells and cells owned
  by nobody pass through untouched.
- An owned language with no runnable kernel (decided with Gordon, 2026-10-29):
  - in a **multi-engine** document it is an **error** naming the language and how to
    install or pin a kernel (today\'s `NoHandlerForLanguage` behavior stays; an explicit
    `engine:` list means the author said who runs what);
  - in a **single-engine** document it is a **warning** diagnostic with the same
    language and install or pin hint, and the cell passes through unexecuted so the
    render still succeeds. It is never silent: the original ggsql report was exactly a
    silent passthrough. This matches Q1, where only fences whose language equals the
    kernelspec language become code cells and every other fence stays display-only
    (`external-sources/quarto-cli/src/core/jupyter/jupyter.ts:321-325`, verified
    2026-10-29). Today\'s single-engine passthrough (P2-13a, "display-only") keeps its
    behavior and gains the warning; the single-engine case becomes noisy but
    successful.
- One kernel serves the document (Q1 parity; JOL-3, several kernels, is deferred): the `jupyter:` scalar
  (JOL-2a), else the kernel resolved from the primary language, which is the first
  language present, in document order, that has a kernelspec (Q1\'s walk), then Python.
- Kernel lookup is by the kernelspec `language` field, matched **case-insensitively**
  (lowercase both the fence language and the kernelspec\'s `language`; Q1 compares
  case-sensitively against `kernel.json`), with no alias, and with the venv-first
  rule for Python. Keep an alias table only where
  Q1 would also resolve the name; given Q1 has none, the default is to delete the table
  and keep at most `python3` mapping to `python`, decided in Phase 1 by test.
- **The `kernel_for(unit)` seam belongs to EXR-2** (decided with Gordon, 2026-10-29).
  [EXR-2](2026-10-28-exr-02-unit-scanner.md) Phase 5 defines the `KernelRouter` trait
  and its `kernel_for(&Unit)` method, makes the unit loop call it, and ships a default
  router with today\'s behavior (one kernel from the first owned cell). This plan
  implements the policy behind that seam, in a new module
  `engine/jupyter/kernel_select.rs` that owns `select_kernel` from JOL-2a plus the
  lookup by language: a pure function over the injected kernelspec list. If this plan
  lands first, `execute_qmd` calls the policy function directly (as JOL-2a does) and
  EXR-2 Phase 5 later wraps it in the default router; if EXR-2 lands first, this plan
  replaces the default router. Either way every unit resolves to the same document
  kernel; a reopened JOL-3 would make the router return different names per unit.
  For mixed-language documents the supported route is a multiplexing kernel such as
  SoS, chosen with the `jupyter:` key (JOL-2a).

## Out of scope

- Several kernels per document (JOL-3, deferred), the per-language `jupyter:` map form beyond what
  JOL-2a parses, the TS wire field (JOL-4), user docs (JOL-5).
- Spans of an unowned language (EXR-3 owns that check; the engine's view applies the same `owns` rule, plus the kernel-language rule for span-only documents).
- Cell-option comment syntax for SQL-family languages (`comment_syntax_for` defaults to
  `#`); noted as a follow-up in JOL-5.

## Phase 1: tests first

- [x] Rewrite the `partition_cells` tests to take an owned set. Names (old name to new
  meaning):
  - `test_partition_cells_owned_unrunnable_fails_loudly`: stays in meaning for
    multi-engine mode: an owned language with no runnable kernel yields the
    missing-kernel error (`NoHandlerForLanguage` or its successor).
  - `test_partition_cells_single_engine_owned_unrunnable_passthrough`: kept in meaning
    and extended: the cell passes through unexecuted, the call succeeds, and one warning
    diagnostic names the language and carries the install or pin hint.
  - A not-owned cell (ceded, or owned by nobody) passes through with no diagnostic.
  - `test_sql_is_not_executable_language`: becomes
    `test_sql_not_owned_by_jupyter_when_knitr_owns_it` (sql is absent from the owned
    set when knitr owns it).
  - ceded, `ojs`, `mermaid` and `dot` tests keep their meaning with `owned_languages`
    excluding them.
- [x] New `partition_cells` cases: `ggsql`, `bash` and a made-up language execute when
  owned; an owned-by-nobody language (explicit `engine: knitr` plus a `julia` cell)
  passes through.
- [x] Pure kernel-selection tests over an injected list of kernelspecs (no process
  spawn): venv-first Python (`VIRTUAL_ENV` path match), language equality that ignores
  case (a kernelspec whose `language` is `"R"` is found for an `r` fence: the Ark
  kernelspec case; a mixed-case `"Python"` likewise), no alias match, first
  language present with a kernelspec wins, Python fallback, `jupyter:` scalar wins
  (JOL-2a), missing-kernel message text (error and warning share it) names the language
  and the install hint.
- [x] Check and test how a cell with `eval: false` interacts with the single-engine
  warning and the multi-engine error: should such a cell be exempt (it never needs a
  kernel)? Not verified: no handling of the `eval` option was found in
  `partition_cells` (`text_execute.rs:177-215`), so the diagnostic may fire before
  anything looks at it. DECIDED (Gordon, 2026-10-31): exempt `eval: false` cells; test it.
- [x] Alias decision test (decided 2026-10-31: deprecate, then delete): `py`, `jl` fences still
  execute this release and emit a deprecation warning naming the canonical language; add the
  removal as a dated follow-up in this plan when the release is cut.
- [x] Resolver-level test (mock knitr plus jupyter registry, reusing Plan 4d tests):
  `owned_languages` for jupyter equals the languages the resolver assigned it.
- [x] Test the policy through the seam: with EXR-2\'s `KernelRouter` in place, this
  plan\'s router returns the document kernel for every unit (the JOL-2a scalar, else the
  walk, else Python). A stub router returning two different names for two languages is
  EXR-2\'s test, not repeated here. If this plan lands before EXR-2 Phase 5, test the
  policy function directly and leave the router wiring to EXR-2.

## Phase 2: implementation

- [x] Add `engine/jupyter/kernel_select.rs` with the lookup policy and the injected
  kernelspec source; move `select_kernel` there. Implement EXR-2\'s `KernelRouter` with
  it (or, if EXR-2 Phase 5 has not landed, expose the pure function that the default
  router will wrap).
- [x] Implement lookup by kernelspec language, case-insensitive, with the venv-first rule; reuse
  `runtimelib::list_kernelspecs` (data dirs only; no per-venv search of its own, so the
  venv rule matches on the kernelspec path as Q1 does).
- [x] Replace `is_executable_language` and the allowlist branch of `partition_cells` with
  `ctx.owned_languages.contains(...)`; remove `map_language_to_kernel` (or reduce it to
  the decided aliases); delete `is_jupyter_language` and its re-export
  (`mod.rs:51`) because nothing calls it, together with its test
  `test_is_jupyter_language` (`engine/jupyter/kernelspec.rs:233`).
- [x] New missing-kernel message carrying language and hint, in the style of the existing
  by-name message `format_kernelspec_not_found` (`engine/jupyter/error.rs:127`: searched
  paths, available kernels, `JUPYTER_PATH` hint) plus an install hint for the language
  (extend `NoKernelForLanguage` or add an `ExecutionError` variant; keep the error a
  clean refusal that does not poison the engine, like `NoHandlerForLanguage`). Multi-engine
  documents raise it as an error; single-engine documents emit the same text as a
  warning diagnostic (never silent) and pass the cell through. Resolve **all** needed
  kernels before running any cell, so an error happens before side effects.
- [x] Route the execution loop through the kernel router (EXR-2 Phase 5). If EXR-2 has
  not landed, change `execute_blocks_inner` and `execute_blocks_async` only as far as
  the policy function needs; EXR-2 owns the loop refactor.
- [x] Check the other entry points that name languages: stored `.ipynb` replay (the
  kernelspec language is lowercased into the fence, `content_processors/ipynb.rs`) and
  percent scripts; confirm neither bypasses `partition_cells`.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

## Phase 3: verification

- [x] `{ggsql}`, `{bash}` and a fixture-language document execute (manual with the ggsql
  kernel; automated with the fake kernel built in EXR-3 and extended by JOL-5, once available).
  *Done (JOL-5, 2026-11-01):* a plain `{ggsql}` document renders its plot with the real ggsql kernel (manual); the fixture language `zzz` executes with no q2 source change in `owned_e2e_tests::test_owned_language_executes_without_allowlist` (fake kernel). No `{bash}` run was made; a made-up language covers the same path.
- [x] Cross-engine parity suite (`tests/integration/engine_output_parity.rs`) still green.
- [x] Resolution is one-shot and feeds freeze cache keys (design section 6): confirm moving
  the gate from allowlist to ownership changes no key for existing documents. The
  records epic records ownership as provenance only.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

## Exit criteria

- `is_executable_language`, the alias table (or its reduced form) and `is_jupyter_language`
  are gone or justified; `ggsql`, `bash` and a fixture language execute without a q2
  source change; an owned language with no kernel fails with an install hint in a
  multi-engine document and warns with the same hint in a single-engine one.
- Clippy and `cargo nextest run -p quarto-core` green; parity suite unchanged.
- This plan\'s policy plugs into EXR-2\'s `KernelRouter` (or is ready to, if EXR-2 Phase 5
  has not landed) without further change to the execution loop.

## Risks and open questions

- **The single-engine case is now noisy but successful.** A lone unrunnable cell still
  passes through and the render succeeds (Q1 parity: Q1\'s cell regex is built from the
  kernelspec language, `jupyter.ts:321-325`, so other-language fences stay display-only),
  but the author now sees a warning. A documentation page that shows a `{bash}` cell as a
  listing will warn on every render; the `eval: false` check in Phase 1 may exempt such
  cells. Q1 does not send other-language fences to the kernel at all; it never turns them
  into cells.
- **Resolver presence scan versus executor regex.** The resolver scans the AST and also
  sees cells in blockquotes and lists that `parse_code_blocks` never sees, so an owned
  language can be "present" and never executed. **Confirmed (read 2026-10-31):** `walk_block_for_langs`
  (`resolution.rs:129`) recurses into blockquotes, divs, figures, lists, tables and note
  definitions, while `partition_cells` skips every cell where `runs_in_legacy_executor` is
  false (indent, container prefix, fence length other than 3, or an attribute tail). So
  `owned_languages` can contain a language whose only cells never run. Consequences: (1) the
  unrunnable diagnostic and the "at least one owned cell" Python failsafe key on the cells
  `partition_cells` actually considers, never on `owned_languages` membership alone; (2) a
  language present only in a blockquote must not trigger Q-18-5. Test both.
- **Three language patterns** (`\w+` here, `[A-Za-z0-9_]+` in the nested-cell mask,
  Q1\'s `[a-zA-Z][a-zA-Z0-9_.]*`) are unified by EXR-2, not here.
- **Several kernelspecs per language** (system Python versus venv, several Julias): Q1\'s
  first-match plus venv rule is the baseline; revisit only if the per-language map form
  is ever built (JOL-3, deferred).
- **SoS caveat.** A span or cell runs in whichever SoS subkernel the previous cell left
  active; nothing here changes that.

## References

- `crates/quarto-core/src/engine/jupyter/text_execute.rs`, `kernelspec.rs`, `daemon.rs`,
  `error.rs`; `engine/context.rs`; `engine/error.rs`.
- Q1: `external-sources/quarto-cli/src/core/jupyter/kernels.ts`,
  `external-sources/quarto-cli/src/core/jupyter/jupyter.ts`.
- [The JOL epic](2026-10-17-jupyter-owned-languages-epic.md), Phase 2 and Risks.

## Implementation notes (2026-10-31)

- Landed on `exr/JOL-2b`: `kernel_select.rs`, `plan_document` (replaces `partition_cells`),
  `Q-18-5` (catalog, docs page, `_quarto.yml`), alias deprecation warning (uncoded; only one
  new code was approved). The seam test is covered by `plan_document` tests: every owned cell
  runs on the one document kernel via `SingleKernelRouter`.
- Gate after rebasing onto `e926c00ab` (EXR-3 spans merged in `execute_qmd`; the Q-18-5 catalog
  entry sits beside Q-18-10): clippy clean; `nextest -p quarto-core -p quarto-error-catalog`
  6897 run, 6896 passed, 1 failed (the reveal.js vendoring baseline). The Julia j7 failure seen
  on the old base did not recur on the new pin.
- Rebase fix: `DocumentPlan.kernel` keeps an explicit kernel when no cell runs (EXR-3 evaluates
  spans on it); a derived kernel needs a runnable cell.
- Open: `cargo xtask verify`; the dated follow-up to delete the alias table when the release is cut;
  `eval: false` is ignored by the executor for runnable cells (outside this plan).
- Decided (Gordon, 2026-10-31): Q-18-5 does not fire under an explicit `jupyter:` kernel.
