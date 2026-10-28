---
title: 'JOL-2b: Jupyter executes the languages it owns; kernel lookup by language'
date: 2026-10-28
description: 'Replaces the Jupyter engine''s hard-coded language allowlist and alias table with the resolver-assigned owned set and Q1-style kernelspec lookup by language, with an error in multi-engine documents and a warning in single-engine documents when an owned language has no runnable kernel, and fills the kernel_for(unit) seam that EXR-2 defines and multi-kernel work later varies per unit.'
status: draft
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
  literal "strict equality, no alias" would break R through Jupyter (epic decision 6). For
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

- [ ] Rewrite the `partition_cells` tests to take an owned set. Names (old name to new
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
- [ ] New `partition_cells` cases: `ggsql`, `bash` and a made-up language execute when
  owned; an owned-by-nobody language (explicit `engine: knitr` plus a `julia` cell)
  passes through.
- [ ] Pure kernel-selection tests over an injected list of kernelspecs (no process
  spawn): venv-first Python (`VIRTUAL_ENV` path match), language equality that ignores
  case (a kernelspec whose `language` is `"R"` is found for an `r` fence: the Ark
  kernelspec case; a mixed-case `"Python"` likewise), no alias match, first
  language present with a kernelspec wins, Python fallback, `jupyter:` scalar wins
  (JOL-2a), missing-kernel message text (error and warning share it) names the language
  and the install hint.
- [ ] Check and test how a cell with `eval: false` interacts with the single-engine
  warning and the multi-engine error: should such a cell be exempt (it never needs a
  kernel)? Not verified: no handling of the `eval` option was found in
  `partition_cells` (`text_execute.rs:177-215`), so the diagnostic may fire before
  anything looks at it. Record the finding here; the decision is Gordon\'s.
- [ ] Alias decision test: `py`, `jl` fences resolve to nothing (Q1 parity) unless the
  alias table is kept; record the decision in this plan.
- [ ] Resolver-level test (mock knitr plus jupyter registry, reusing Plan 4d tests):
  `owned_languages` for jupyter equals the languages the resolver assigned it.
- [ ] Test the policy through the seam: with EXR-2\'s `KernelRouter` in place, this
  plan\'s router returns the document kernel for every unit (the JOL-2a scalar, else the
  walk, else Python). A stub router returning two different names for two languages is
  EXR-2\'s test, not repeated here. If this plan lands before EXR-2 Phase 5, test the
  policy function directly and leave the router wiring to EXR-2.

## Phase 2: implementation

- [ ] Add `engine/jupyter/kernel_select.rs` with the lookup policy and the injected
  kernelspec source; move `select_kernel` there. Implement EXR-2\'s `KernelRouter` with
  it (or, if EXR-2 Phase 5 has not landed, expose the pure function that the default
  router will wrap).
- [ ] Implement lookup by kernelspec language, case-insensitive, with the venv-first rule; reuse
  `runtimelib::list_kernelspecs` (data dirs only; no per-venv search of its own, so the
  venv rule matches on the kernelspec path as Q1 does).
- [ ] Replace `is_executable_language` and the allowlist branch of `partition_cells` with
  `ctx.owned_languages.contains(...)`; remove `map_language_to_kernel` (or reduce it to
  the decided aliases); delete `is_jupyter_language` and its re-export
  (`mod.rs:51`) because nothing calls it, together with its test
  `test_is_jupyter_language` (`engine/jupyter/kernelspec.rs:233`).
- [ ] New missing-kernel message carrying language and hint, in the style of the existing
  by-name message `format_kernelspec_not_found` (`engine/jupyter/error.rs:127`: searched
  paths, available kernels, `JUPYTER_PATH` hint) plus an install hint for the language
  (extend `NoKernelForLanguage` or add an `ExecutionError` variant; keep the error a
  clean refusal that does not poison the engine, like `NoHandlerForLanguage`). Multi-engine
  documents raise it as an error; single-engine documents emit the same text as a
  warning diagnostic (never silent) and pass the cell through. Resolve **all** needed
  kernels before running any cell, so an error happens before side effects.
- [ ] Route the execution loop through the kernel router (EXR-2 Phase 5). If EXR-2 has
  not landed, change `execute_blocks_inner` and `execute_blocks_async` only as far as
  the policy function needs; EXR-2 owns the loop refactor.
- [ ] Check the other entry points that name languages: stored `.ipynb` replay (the
  kernelspec language is lowercased into the fence, `content_processors/ipynb.rs`) and
  percent scripts; confirm neither bypasses `partition_cells`.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core`.

## Phase 3: verification

- [ ] `{ggsql}`, `{bash}` and a fixture-language document execute (manual with the ggsql
  kernel; automated with the fake kernel built in EXR-3 and extended by JOL-5, once available).
- [ ] Cross-engine parity suite (`tests/integration/engine_output_parity.rs`) still green.
- [ ] Resolution is one-shot and feeds freeze cache keys (design section 6): confirm moving
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
  language can be "present" and never executed. Hypothesis from the handoff; the resolver
  scan was not read.
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
