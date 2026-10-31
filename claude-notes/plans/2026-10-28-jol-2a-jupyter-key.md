---
title: 'JOL-2a: the Jupyter engine honors the `jupyter:` key'
date: 2026-10-28
description: 'Wires the dead `resolve_kernel` metadata-first order into the text execution path so `jupyter: <kernel>` selects the kernel for the whole document, which makes multiplexing kernels such as SoS usable and gives span-only documents a kernel.'
status: in-progress
---

**Part of:** [the epic](2026-10-28-execution-records-epic.md), as JOL-2a, the first
independent slice of Phase 2 of
[the Jupyter owned-languages epic](2026-10-17-jupyter-owned-languages-epic.md).
**Depends on:** nothing. It does not need JOL-1.
**Blocks:** [JOL-2b](2026-10-28-jol-2b-ownership-gate.md) (builds on the kernel-selection
function introduced here) and [EXR-3](2026-10-28-exr-03-jupyter-iee.md) (a span-only
document needs its kernel resolved from `jupyter:` first). The `select_kernel` function
introduced here is what the default router behind EXR-2\'s `kernel_for(unit)` seam wraps;
this plan does not define the seam.
**Lane:** B (`workspace-5`); run it under the epic\'s "Running the plans in four lanes" section.
**Estimated sessions:** 1

> **For implementers:** TDD, task by task. Steps use `- [ ]` checkboxes.

## Progress (2026-10-31, lane B session 2)

Landed on `exr/JOL-1` (`ecb493183`, `0e4089f7f`), gates: clippy clean; `nextest -p
quarto-core` 6533 passed, 2 failed, 32 skipped. The two failures are
`vendored_reveal_assets_match_npm_package` (node_modules drift in workspace-5, unrelated) and
`julia_engine_e2e::j1_minimal_julia_render`, which passes alone in 2.6s and failed at 19.8s
while another lane ran nextest (load, not this change).

Done: `kernel_from_ts_metadata`, `select_kernel` and its call in `execute_qmd`, deletion of
`resolve_kernel`, the engine-config second source, the cell-less declared-engine resolver
(`own_file`, option 1), the Pass-1 lift, and the audit of tests that assume an empty
sequence. The stages already pass `DOCUMENT_OWN_FILE_ID`.

Open: everything still unticked below. Two have a known starting point: the kernelspec
language reading (`find_kernelspec_for_language` matches `kernelspec.language` only, and the
installed `sos` spec has `language: sos`), and the `.ipynb` provenance gap above.

### Review decisions (2026-10-31, Gordon)

A design and implementation review of the JOL epic (two review agents, claims re-checked by
the orchestrator) settled the following. Facts marked "run" were re-run; the rest were read.

- **All owned cells go to the one kernel** (divergence 1 below stands). Deliberate divergence
  from Q1, to be stated as such in JOL-5. It is what makes SoS mixed-language documents work.
- **A project-level engine may start on a cell-less page that declares a different engine.**
  Accepted quirk. Run: project `engine: [knitr]` plus document `engine: [jupyter]`, no cells,
  R missing: render fails with "Engine 'knitr' is registered but its runtime is not
  available". The merged array roots at `FileId(0)`, so the own-file rule fires and the
  sequence is `["knitr","jupyter"]`. The mirror case (project `jupyter: python3`, document
  `knitr: true`) does not run knitr, because `declares_engine_in_file` inspects only the first
  declaring value. Document both in JOL-5; no filtering of `raw_explicit`.
- **A `jupyter:` map without `kernel`, and Q1's `kernelspec:` object, warn and fall back** to
  the next source (the first cell's language). Q1 errors on both. Replaces the "silent
  fallthrough" in divergence 2.
- **A cell-less page that declares an engine hard-errors when the runtime is missing**
  (Q1-compatible, run for knitr and for `jupyter: python3` without `jupyter` on PATH). In WASM
  and the hub the same page now carries the "not available in this build" warning
  (`engine_execution.rs:177-186`, inferred from code, not run). List as a behavior change in
  JOL-5.
- **`DOCUMENT_PROFILE_VERSION` bumps to 17 at merge time**, with a changelog line in
  `claude-notes/designs/document-profile-contract.md` (whose header says 13 and whose changelog
  lacks v14 and v16; fix that drift in the same commit). Done at merge, not earlier, to avoid
  colliding with the other lanes.
- **JOL-2a's finished core merges to the integration branch early** with `status:
  in-progress`; the unticked tail below stays open and must not hold up EXR-3. EXR-3 needs
  only the resolver rule and `select_kernel`. This needs a matching edit to the dependency
  table in the execution-records epic (Gordon's file, done in the same commit as this note).
- **`.ipynb` and percent scripts: do not synthesize a `jupyter:` value.** Run: both are
  claimed by `jupyter`, and the claimed-engine short-circuit precedes the own-file rule, so
  engine choice is unaffected. `ipynb.rs` uses `kernelspec.language` only (lowercased) and
  never reads `kernelspec.name`; selection stays with `select_kernel`. The "Known gap" below
  is therefore moot for resolution; it still applies to any consumer that applies the
  own-front-matter test to spans or YAML (decision 41, EXR-2, EXR-3, EXR-14).
- **Run results from the review**: `.md` with `jupyter: python3` still emits Q-2-40 and does
  not execute; a span-only page with `jupyter: python3` renders its span literally and starts
  no kernel (EXR-3 owns this); `jupyter: sos` with `%use` lines in `{python}` cells renders
  with the real `sos` kernelspec (new `q2`: 6 outputs, no errors; the old `q2` failed on
  `%use`). Plain `{ggsql}` fences on SoS remain untested.
- **Reading a kernelspec's language is not needed for `jupyter: <name>`**: lookup is by exact
  name (`find_kernelspec`). `find_kernelspec_for_language` is dead code and uses the bare
  `runtimelib::list_kernelspecs()`, which misses venv kernels; JOL-2b must switch it to the
  augmented list.
- **Out-of-plan finding, not part of JOL**: a `.py` percent script that opens with a `# ---`
  front-matter block followed by `# %% [markdown]` panics in `q2 render` (`line_writer.rs:125`,
  "provenance must tile contiguously"); a `# ---` block followed directly by `# %%` is not
  sniffed as a percent script ("Can't determine execution engine"). Percent front matter works
  only inside a `# %% [markdown]` cell.

### SoS recorded run (2026-10-31, lane B session 3)

Built `target/debug/q2` from this worktree (the `q2` on PATH is the 2026-10-27 nightly, which
predates this plan). Copied `~/docs/ggsql-sos/mixed.qmd` to the scratchpad and changed
`engine: jupyter` to `jupyter: sos`, so the three cells are `{python}` (`x = 21`,
`print(x * 2)`), `{ggsql}` (`%use ggsql`, a penguins histogram) and `{python}` (`%use sos`,
`print(x + 1)`). Rendered with the SoS venv (`~/docs/ggsql-sos/.venv/bin`) first on `PATH`,
since `jupyter` is not otherwise installed. The kernelspec is the real `sos` one
(`language: sos`), with no shim named `python3`.

Result: **`42`, one PNG (`cell-2-output-1.png`, 1000 x 618) and `22`**, no errors, as in the
2026-10-17 shim experiment. The ggsql subkernel's INFO log lines appear as stderr in that
cell's output (the known SoS rough edge, not q2's). So SoS covers a mixed `python`/`ggsql`
document with `%use` lines and `jupyter: sos`. Plain `{ggsql}` fences without `%use` were not
tried (see below).

### `cell_kernel` research (2026-10-31, lane B session 3)

- SoS (`sos_notebook/kernel.py:1285-1296`, `init_metadata`) reads its per-cell settings from
  **`message.metadata["sos"]` first**, then `content["sos"]`. `cell_kernel` selects the subkernel
  for the cell, and `default_kernel` is the fallback.
- q2 builds every execute request as `ExecuteRequest::new(code)` converted with `.into()`
  (`engine/jupyter/execute.rs:46`). `jupyter-protocol` 2.0.0's `ExecuteRequest` has no slot for
  extra content fields, so `content.sos` cannot be sent through the typed struct. But
  `JupyterMessage` has a `metadata: Value` and a `with_metadata` builder, so q2 can send
  `{"sos": {"cell_kernel": "<name>"}}` there without touching the protocol crate.
- Cost: about ten lines in `execute.rs`, plus one thing that is not small: a map from a fence
  language to a SoS subkernel name (`{ggsql}` to `ggsql`, `{python}` to `sos` or `python3`).
  That map is the ownership question JOL-2b owns, and is SoS-specific.
- **Recommendation: document the `%use <kernel>` line as the supported form now, and do not
  build `cell_kernel` in JOL-2a.** The recorded run shows `%use` works. Revisit after JOL-2b,
  when fence-to-kernel ownership exists, as a small follow-up and not a revived JOL-3. Not
  tested: whether SoS honors `cell_kernel` sent this way (read, not run).

### Note for JOL-5

`jupyter: <kernel>` (or `jupyter: {kernel: <kernel>}`) now selects the kernel for the whole
document; before, the key was ignored and the kernel came from the first executable cell's
language. Unlike Quarto 1, which only runs fences in the kernelspec's language and leaves the
rest display-only, q2 sends every Jupyter-owned cell to that one kernel, which is what makes a
multiplexing kernel such as SoS work (`jupyter: sos` with `%use <kernel>` lines). The language
allowlist still decides which cells run until JOL-2b, so `{sos}` and `{bash}` fences are not
executed yet. A `jupyter:` map with no `kernel`, or Quarto 1's `kernelspec:` object, warns
(`Q-18-4`) and falls back to the first cell's language instead of erroring as Quarto 1 does.
Also for JOL-5: a cell-less page whose own front matter declares an engine now runs it, and
fails hard if the runtime is missing (Quarto 1 behavior; in a WASM build the page renders
silently, unchanged); a project-level declaration alone starts nothing; and the accepted
mixed-layer quirk (project `engine: [knitr]` merged with document `engine: [jupyter]` starts
knitr on a cell-less page, while project `jupyter: python3` with document `knitr: true` does
not start knitr).

## Goal

A document that says `jupyter: sos` (or any installed kernelspec name) runs every
executed cell on that kernel. Today the key is ignored by the text path, so the
kernel is always chosen from the language of the first executable cell.

The key is also the supported route for a document that has inline expressions and no
executable cell, as in Q1 (verified by run, 2026-10-31: `jupyter: python3`, `engine: jupyter`
and a `jupyter:` kernelspec map each make Q1 evaluate a span-only document, and a bare
`jupyter: python3` document with no spans still starts a kernel). The resolver walks code
blocks only (as q1's language scan does) and, today, returns an empty engine list as soon as
it finds no code cell, before it reads the declared engine (`resolution.rs:489-503`), so the
key alone does nothing. This plan changes that (epic decision 42, Phase 2), **only for an
engine that the document's own front matter declares** (Gordon, 2026-10-31, from the q1
evidence below). Whether to route
span-only documents automatically, without a key, is the question strand bd-my5fqvok and is
outside this epic (epic decision 11).

## Background

Facts below were read in the tree on 2026-10-28 (workspace-1, `origin/main` at
`2ee0bd966`) unless marked *not verified*.

- `execute_qmd` picks the kernel from the first executable cell only:
  `map_language_to_kernel(&executable[0].language)`
  (`crates/quarto-core/src/engine/jupyter/text_execute.rs:76`; the alias table is at
  `:96`). The document metadata is never consulted. *Verified.*
- `resolve_kernel` (`engine/jupyter/kernelspec.rs:129`, `#[allow(dead_code)]`) already
  implements the right order: explicit kernel from metadata, then the primary language,
  then Python. Its helper `extract_kernel_from_metadata` (private, same file) accepts
  both `jupyter: python3` and `jupyter: {kernel: python3}`. Both have unit tests in
  that module and no other caller. *Verified.*
- **q1 reads the engine from the document's own front matter only** (verified by run with
  q1 99.9.9, 2026-10-31, and in
  `external-sources/quarto-cli/src/execute/engine.ts:155-171`, `markdownExecutionEngine`,
  which reads the document's front matter plus command-line flags and never `_quarto.yml`).
  With no code cells: a document-level `engine: knitr` and R missing is a hard error
  ("Unable to locate an installed version of R"); a document-level `jupyter: python3` with no
  Jupyter installed starts a kernel anyway and fails hard ("Jupyter is not available"); a
  top-level `engine: knitr` in `_quarto.yml` is ignored (a plain page renders without R, and a
  page with a `{python}` cell goes to Jupyter). Not tested: `execute: engine:` or `jupyter:`
  set in `_quarto.yml`. So hard errors for a document-declared engine are q1-compatible, and
  a project-level declaration must not start an engine on a cell-less page.
- `resolve_kernel` takes a `&ConfigValue`. The execution context does **not** carry
  one: `ExecutionContext` has `engine_config` (the value under the `engine` key) and
  `metadata: HashMap<String, TsMetadataValue>`, the merged document metadata lowered for
  the TS wire, documented as ignored by built-in engines
  (`engine/context.rs:56-61` and `:140-153`). `TsMetadataValue` is an untagged enum of
  bool, number, string, array, map and null (`engine/ts_protocol.rs:321`). So the key
  is reachable as `ctx.metadata.get("jupyter")`, but in the wrong value type for
  `resolve_kernel`. *Verified.*
- The daemon takes a kernel **name**, not a resolved spec: `get_or_start_session` keys
  sessions by `SessionKey(kernel_name, working_dir)` and looks the kernelspec up again
  with `kernelspec::find_kernelspec` (`engine/jupyter/daemon.rs:84-133`). So this plan
  only has to produce the right name. *Verified.*
- Q1 for comparison (`external-sources/quarto-cli/src/core/jupyter/jupyter.ts:508-578`,
  `jupyterKernelspecFromMarkdown`): a scalar `jupyter: name` looks the kernelspec up by
  name and rejects with "Jupyter kernel 'name' not found. Known kernels: ...". A map
  accepts either a full `kernelspec:` object or a `kernel:` name; the remaining keys of
  the map are kept as notebook metadata. With no `jupyter:` key Q1 walks the languages
  present in the document (then Python as a failsafe) and takes the first language that
  has an installed kernelspec (Python is always added to the language list as the failsafe,
  `languages.add("python")` at `:522`). A `jupyter:` map without `kernel` (and without a
  `kernelspec:` object) is rejected with "Invalid Jupyter kernelspec" (`:578`). *Verified.*
- **Q1 only turns fences of the kernelspec\'s language into code cells**
  (`external-sources/quarto-cli/src/core/jupyter/jupyter.ts:321-325`: the cell-start
  regex is built from `kernelspec.language`). Fences in any other language stay
  display-only markdown; Q1 does not send them to the kernel. *Verified 2026-10-30.*
  Q1 was run with `jupyter: sos`: a `{python}` fence stayed display-only and only a
  `{sos}` fence ran (verified by run, 2026-10-31). So "all Jupyter-owned cells go to the
  one kernel named by `jupyter:`" is a q2 design that goes beyond Q1, and a behavior
  change for Q1 documents: a document that relied on `{python}` fences staying
  display-only under `jupyter: sos` will have them executed by q2 (listed under
  "Divergences from Q1" below).
- SoS evidence (from the epic, tested 2026-10-17): `jupyter: sos` did nothing; the
  symptom was IPython rejecting `%use`. A shim kernelspec named `python3` that launches
  SoS worked. With the key honored, a document whose cells are `python`, `ggsql` (first
  line `%use ggsql`) and `python` (first line `%use sos`) should run on one SoS kernel.
  *Not re-verified here; needs the SoS install from `~/docs/ggsql-sos/`.* Caveats
  recorded 2026-10-30: the installed `sos` kernelspec has `language: sos`
  (`~/Library/Jupyter/kernels/sos/kernel.json`), so that SoS works through q2 with the
  key honored is **not verified** (JOL-2a is not built; the earlier success used a shim
  kernelspec named `python3`); with this plan alone a mixed-language document needs
  `%use <kernel>` lines inside its cells; and plain `{ggsql}`-style fences on SoS would
  need q2 to send SoS\'s `cell_kernel` in the execute request, which is an open item
  (Phase 3), not a reason to revive JOL-3.

## Design decisions

From the epic and the JOL epic (decision 4); not to be re-litigated here.

- The scalar keeps its Q1 meaning: it names the kernel for the whole document. The
  map form `jupyter: {kernel: name}` does the same. The per-language map
  (`jupyter: {python: python3, ggsql: ggsql}`) belongs to JOL-2b and, if it is ever
  reopened, JOL-3 (deferred), and is not parsed here; a map without a `kernel` key falls
  through to language lookup (a q2 divergence from Q1, below).
- The explicit name wins over language lookup; absent, behavior is unchanged.
- The allowlist (`is_executable_language`) keeps deciding *which* cells execute until
  JOL-2b replaces it. This plan only changes *where* they run. Consequence to document:
  a cell in a language outside the allowlist is still not executed even when `jupyter:`
  names a kernel that could run it.
- No new `ExecutionContext` field: read the key from `ctx.metadata`.

### Divergences from Q1

1. **Every Jupyter-owned cell runs on the kernel named by `jupyter:`.** Q1 turns only
   fences in the kernelspec\'s language into code cells and leaves the rest display-only
   (verified by run with `jupyter: sos`, above). Behavior change for Q1 documents. DECIDED
   (Gordon, 2026-10-31): keep.
2. **A `jupyter:` map without `kernel` warns and falls back to language lookup.** Q1 rejects
   it with "Invalid Jupyter kernelspec" (`jupyter.ts:578`). DECIDED (Gordon, 2026-10-31):
   warn, not silent and not an error. The same for the `kernelspec:` object (4). The
   per-language map is not built by any plan; JOL-2b must not claim it unless it reopens it.
3. **The `ctx.engine_config` source** (`engine: [{jupyter: {kernel: x}}]`, Phase 2) has no
   Q1 equivalent.
4. **The full `kernelspec:` object form** that Q1 accepts is not supported (known gap,
   below).

## Out of scope

- The per-language map form, ownership-based gating and kernelspec lookup by language
  (JOL-2b), several kernels (JOL-3, deferred).
- The full `kernelspec:` object form that Q1 accepts. Record it as a known gap.
- Sending SoS `cell_kernel` in the execute request (JOL epic, open risk): only the
  research item in Phase 3, no implementation here.
- Venv-first Python preference (JOL-2b).

## Phase 1: tests first

- [x] Unit tests for a new pure function `kernel_from_ts_metadata(&HashMap<String,
  TsMetadataValue>) -> Option<String>` next to `extract_kernel_from_metadata`:
  - `test_kernel_from_ts_metadata_scalar` (`jupyter: python3` gives `python3`)
  - `test_kernel_from_ts_metadata_kernel_key` (`jupyter: {kernel: julia-1.9}`)
  - `test_kernel_from_ts_metadata_absent` (no key gives `None`)
  - `test_kernel_from_ts_metadata_map_without_kernel_key` (`jupyter: {python: x}` gives
    `None`, reserved for JOL-2b; the Q1 divergence 2 above)
  - `test_kernel_from_ts_metadata_non_string` (a number or bool gives `None`, no panic)
- [x] Unit tests for a pure `select_kernel(metadata: &HashMap<String, TsMetadataValue>,
  engine_config: Option<&ConfigValue>, first_owned_language: Option<&str>) -> Option<String>`
  that wraps the explicit-name check and the existing first-cell fallback. With zero cells
  (`first_owned_language` is `None`) and a key it returns that kernel; with zero cells and
  no key it returns `None`, and the default-to-Python of `resolve_kernel` must not apply
  (EXR-3's "needs an executable cell or a `jupyter:` kernel" error depends on the `None`):
  - `test_select_kernel_explicit_wins_over_first_cell_language`
  - `test_select_kernel_falls_back_to_map_language_to_kernel`
  - `test_select_kernel_no_cells_with_key`
  - `test_select_kernel_no_cells_no_key_is_none`
- [ ] Integration test, `#[ignore = "requires ipykernel"]` in the style of
  `crates/quarto-core/tests/integration/jupyter_integration.rs`: a document with
  `jupyter: python3` and a first cell in a different allowlisted language still starts
  `python3` (assert on the `SessionKey` the daemon reports).
- [x] `jupyter: no-such-kernel` fails with an error that names the kernel. Done as an
  ordinary unit test, not an ignored integration test: the lookup fails before any kernel
  starts, so it needs no ipykernel (`test_missing_explicit_kernel_gives_the_rich_error`; it
  also covers the `format_kernelspec_not_found` task below).

## Phase 2: implementation

- [x] Add `kernel_from_ts_metadata` in `engine/jupyter/kernelspec.rs`. The `ctx.metadata`
  route is safe for bare YAML strings: `config_value_to_ts_metadata` lowers them with
  `as_plain_text` (`project/mod.rs:1379-1383`). The old extractor is not: see the delete
  task below.
- [x] Resolver, tests first (`resolution.rs`). The rule: a document with no computational
  languages whose **own front matter** declares a non-markdown engine (`engine:` or the
  top-level shorthand such as `jupyter:`) resolves to that engine in the sequence, with an
  empty ownership map. "Own front matter" is decided by provenance, as in EXR-2 decision 11:
  the declaring value's `SourceInfo` root file id is the document's own file (a value from
  `_quarto.yml`, `_metadata.yml`, an extension or a format default does not count). SETTLED
  (Gordon chose option 1, 2026-10-31): `resolve_engines_for_document` and
  `resolve_engines_pass1_for_document` take `own_file: Option<FileId>`; the old entry points
  pass `None`, which keeps the rule off. Both stages pass `DOCUMENT_OWN_FILE_ID` =
  `FileId(0)`, the dense id a parsed `.qmd`\'s front matter roots at (layers such as
  `_quarto.yml` root at hash-based ids). A declaration counts as the document's own when the
  declaring value (`engine:`, else the first shorthand key) has `own_file` among its root
  file ids. **Known gap (narrowed 2026-10-31):** an `.ipynb` conversion roots at virtual cell
  files (`[2,3]`) and a percent script at `[1]`, so their declarations are not recognized as
  the document's own. This does not affect engine choice, because both are claimed by
  `jupyter` and the claimed short-circuit comes first (run). Confirmed by probe against a real
  discovered project: front matter roots at `[0]`; `_quarto.yml`, `_metadata.yml`, a format
  extension default and project `format:` all root at hash ids. The probe is not committed;
  turn it into a committed integration test (task below). Tests:
  - `declared_engine_without_cells_resolves_to_the_engine` (front matter `jupyter: python3`
    and `engine: jupyter`, no cells: the engine, empty ownership map);
  - `project_level_declared_engine_without_cells_stays_empty` (`engine: knitr` only in
    the `_quarto.yml` layer: still an empty sequence, as q1 ignores it);
  - `engine_markdown_without_cells_stays_empty` (`engine: markdown` and no cells still
    resolves to `[]`, not `[markdown]`; today the P3 branch comes after the empty-scan
    return, so a naive move would change this);
  - `declared_engine_with_claims_table_without_cells_strips_the_config` (a declared engine
    with a `claims:` table and no cells resolves with the config stripped);
  - a document with neither a declaration nor cells stays markdown passthrough
    (`generated-languages` alone still does not manufacture an engine, as today).
- [x] Implement it inside `run_tiers`, not as a second early return in `prepare_resolution`.
  Keep the empty scan result but continue to the sequence build instead of returning at
  `resolution.rs:493-503`; when `languages` is empty, step 4 appends the engines of
  `raw_explicit` in declared order with the same config handling. Sequence build otherwise
  skips an engine that owns no language (`:810`, "Engine owns nothing"), and a hand-written
  early return would skip `strip_claims_key` (the `claims:` key must not leak into
  `ExecutionContext` through `with_engine_config`), the duplicate-config note and the
  unknown-engine notes. Keep the `engine: markdown` P3 behavior above.
- [x] `resolve_engines_pass1` (`:878`), test first `pass1_lifts_declared_engine_without_cells`:
  the declared-engine return consults no claim, so Pass 1 returns `Some` with that sequence
  and an empty ownership map. Returning `None` would count the document as `fell_through`
  and trigger the project-wide "engine extension declares no static claims" warning
  (`project/orchestrator.rs:258-263`). `pass1_lifts_no_languages` (`resolution.rs:3347`) stays
  unchanged (its meta declares only `generated-languages`). Check what a `.md` input does:
  the `Q-2-40` guard reads the declared engines (`explicitly_declared_engines`), not the
  resolved ones, and returns before any engine runs (`engine_execution.rs:332`), so only the
  stash and the Pass-1 stamp change; add a test. Audit done 2026-10-31 (grep of `quarto-core`, `quarto`,
  `quarto-preview`, `quarto-hub-provider`): no test combines a document-declared engine with
  no cells and expects an empty sequence. The empty-sequence tests in `resolution.rs`
  (`test_markdown_only_empty_sequence`, `generated_language_alone_is_noop`,
  `pass1_lifts_no_languages`, the doubled-brace display-fence test) all use empty metadata or
  `generated-languages` only, so they stay valid. The only cell-less declared-engine tests
  are the `.md` Q-2-40 ones (`engine_execution.rs` `md_with_engine_*`, and
  `render_cli_e2e.rs` `md_with_engine_spec_renders_with_q_2_40_warning`); the guard reads
  `explicitly_declared_engines` and returns before engines run, so they should be unaffected
  (confirm by running them after the resolver change).
- [x] Behavior of a declared engine on a cell-less page, tests first (q1 evidence above:
  hard errors for a document-level declaration are q1-compatible). Cover three cases:
  (1) R missing with `engine: knitr` in the document's front matter and no cells: a hard
  error, as in q1 (`get_engine_with_fallback` returns `Err` for a registered engine whose
  runtime is unavailable, `engine_execution.rs:152-176`); (2) `jupyter: python3` in WASM
  where Jupyter is not registered: the "not available in this build" warning
  (`engine_execution.rs:177-186`), the render succeeds; (3) a plain page in a project whose
  `_quarto.yml` declares an engine: no engine runs and no capture is emitted. Also record
  what the Jupyter engine does for zero cells with a kernel (passthrough,
  `text_execute.rs:55-58`) and that the page still goes through serialize, reparse and
  reconcile with an engine capture emitted (`engine_execution.rs:~590-620`). Review run
  results are in Progress; no test covered any of cases (1)-(3) before. Add a fourth case for
  the accepted mixed-layer quirk (project `engine: [knitr]` plus document `engine:
  [jupyter]`), asserting the current behavior so a later change is deliberate.
  DONE (2026-10-31, session 3), in `engine_execution.rs` tests `cell_less_*`: case 1 hard
  error, case 3 project declaration runs none, case 4 the merged array fails on knitr, and a
  fifth test that a declared engine with its runtime runs once with no diagnostics.
  **Case 2 did not behave as planned.** The "not available in this build" warning does not
  fire: `explicitly_declared_engines` drops engines the registry lacks, so in WASM a
  cell-less `jupyter: python3` page renders silently, as it did before this plan. The
  earlier claim (inferred from code, not run) was wrong; the test pins the silent behavior
  (`cell_less_declared_engine_not_in_build_is_silent`). Whether the hub should warn is open
  for Gordon; JOL-5 must not list a hub warning as a behavior change. The case-4 test models
  the merged array with front matter `engine: [knitr, jupyter]` (one array rooted at the
  document's file), which is the shape the pipeline hands the stage.
- [x] Commit the own-file probe as an integration test under
  `crates/quarto-core/tests/integration/` (not a top-level `tests/<name>.rs`): a discovered
  project with `_quarto.yml`, `_metadata.yml`, a format extension and a format default;
  assert which layers root at `FileId(0)`. The throwaway probe is saved at
  `/private/tmp/claude-502/-Users-gordon-src-q2/a76cf139-d7af-4cab-bfbc-1e4abd581f95/scratchpad/probe_own_file.rs.txt`
  (scratch, may be gone; the cases are listed in Progress and the commit for `0e4089f7f`).
  DONE: `tests/integration/own_file_provenance.rs`, five tests (front matter roots at
  `FileId(0)` and resolves to `jupyter`; `_quarto.yml`, `_metadata.yml`, a format extension
  default and a project `format:` default each do not, and a plain page gets no engine).
- [x] Warn and fall back for a `jupyter:` map without `kernel` and for a `kernelspec:`
  object (divergences 2 and 4), tests first. The warning gets a Q-code only if the project's
  diagnostic conventions require one; decide when writing it. DONE: new `Q-18-4` "Jupyter Kernel
  Not Named" (catalog entry and `docs/errors/engine/Q-18-4.qmd`; the catalog is the
  convention for coded warnings). `unusable_jupyter_key` (kernelspec.rs) classifies the two
  forms, `jupyter_key_warning` (text_execute.rs) builds the warning, and `execute_qmd`
  attaches it to `ExecuteResult::warnings` after a successful run, which the stage drains.
  Unit-tested for both forms and for no warning on valid keys. Not run end to end: the
  warning only attaches on a successful kernel run, which needs ipykernel.
- [x] `DOCUMENT_PROFILE_VERSION` (`document_profile.rs:160`, was 16; bumped to 17 at merge): DECIDED (Gordon,
  2026-10-31): bump to 17 at merge time, with a changelog line in the contract and the header
  drift fixed (see Progress). The stamped `DocumentProfile.engine_resolution` changes meaning for document-declared
  engines on cell-less pages. Read `claude-notes/designs/document-profile-contract.md` (not
  read here) for the bump rule, whose history shows bumps for semantic changes, then bump or
  record why not.
- [x] Update stale text that says a cell-less document resolves to an empty sequence (done): the
  doc comment of `explicitly_declared_engines` (`resolution.rs:~402-408`), the comment at
  `engine_execution.rs:~315-325`, and the P2 definition in
  `claude-notes/designs/engine-resolution.md:321` (P2 becomes "the language scan is empty and
  the document's own front matter declares no engine").
- [x] Add `select_kernel` and call it in `execute_qmd` in place of the direct
  `map_language_to_kernel(&executable[0].language)` call (`text_execute.rs:76`). This plan is
  written against the tree before EXR-2 Phase 5, where `execute_qmd:76` and `CodeBlock`
  exist. Either order works, as in JOL-2b's design decision on the seam: if EXR-2 Phase 5 (the `KernelRouter`
  seam) has landed, make the default router wrap `select_kernel` and replace the default
  router; if not, call `select_kernel` directly.
- [x] Delete `resolve_kernel` and `extract_kernel_from_metadata` (`kernelspec.rs:129`, and
  the helper below it) with their metadata-only tests. `extract_kernel_from_metadata` reads
  with `ConfigValue::as_str()`, which returns `None` for `PandocInlines`
  (`quarto-pandoc-types/src/config_value.rs:736-747`), and a bare YAML string such as
  `jupyter: python3` arrives as `PandocInlines` in merged metadata. The `engine_config` read
  in `select_kernel` must therefore use `as_plain_text`, not `as_str`; test it with merged
  metadata, not a hand-built `Scalar`.
- [x] Test that the missing-kernel error for an explicit name is the existing rich one:
  `format_kernelspec_not_found` (`engine/jupyter/error.rs:127`) already lists the
  searched paths, the available kernels and a `JUPYTER_PATH` hint. No new message is
  needed; assert the document-level error contains the kernel name and the list.
- [x] Honor the documented engine-config form as a second source. The user guide shows
  `engine: [{jupyter: {kernel: python3}}]` with "the map\'s value is passed to the
  engine" (`docs/guides/authoring/engines.qmd:35-36`), and `ctx.engine_config` carries
  that map (`engine/context.rs:56-61`), but `execute_qmd` reads neither. Precedence
  (decided, Gordon, 2026-10-30): the `jupyter:` document key first (Q1 meaning), then
  `engine_config.kernel`, then language lookup; a conflict between the two is not an
  error, because merged metadata cannot tell the project layer from the document
  layer. Add
  `test_select_kernel_engine_config_kernel` and
  `test_select_kernel_jupyter_key_beats_engine_config`.
- [x] Check the other entry points that name kernels: the percent-script processor
  (`content_processors/percent.rs`) and the `.ipynb` converter
  (`content_processors/ipynb.rs`). ANSWERED (2026-10-31, review): leave selection to
  `select_kernel`; neither synthesizes a `jupyter:` value (see Progress). Residual, outside
  this plan: a notebook whose kernelspec is `julia-1.11` maps by language to a kernel named
  `julia`, which may not exist. Not a JOL-2a task.

## Phase 3: end to end

- [x] SoS manual check, recorded in this plan with the outcome (see "SoS recorded run" below): install `sos` and
  `sos-notebook` plus the ggsql kernelspec per the JOL epic research notes; render
  `~/docs/ggsql-sos/` documents with `jupyter: sos`; expect `42`, a PNG and `22` as in
  the 2026-10-17 shim experiment, without the shim kernelspec. Use the real `sos`
  kernelspec (`language: sos`), put `%use ggsql` and `%use sos` lines in the cells, and
  record the result; this is the verification that SoS covers mixed-language documents
  (the reason JOL-3 is deferred). Review note 2026-10-31: a review agent already rendered a
  `%use sos` / `%capture` / `%use ggsql` document with the real `sos` kernelspec (6 outputs,
  no errors); the recorded manual run with the `42`/PNG/`22` expectations is still to do.
- [x] Research item, answer recorded here (see "cell_kernel research" below): can q2 add `sos: {cell_kernel: <name>}` to
  the execute request content (read `runtimelib` 2.0 and the execute-request code
  under `engine/jupyter/`; exact location not checked)? Plain `{ggsql}` fences on SoS need it; not verified.
  Decide from the manual check above whether to build it as a small follow-up (not a
  revived JOL-3) or to document the `%use` lines as the supported form.
- [x] Add the user-facing note (one paragraph, for JOL-5 to expand; text under "Note for JOL-5" below) that `jupyter:`
  now selects the kernel and that the language allowlist still applies until JOL-2b, and
  that, unlike Q1, cells in other Jupyter-owned languages are sent to that kernel rather
  than left display-only. Also for JOL-5: the cell-less hard error and hub warning, the
  mixed-layer quirk, and that `{sos}` fences are not executed until JOL-2b.
- [x] At merge: bump `DOCUMENT_PROFILE_VERSION` to 17, add the contract changelog line, and
  amend the lane/dependency table in the execution-records epic as described in Progress.
- Gate: `cargo clippy -p quarto-core --all-targets -- -D warnings` plus
  `cargo nextest run -p quarto-core` (the Phase 3 manual checks are not part of the gate).

## Exit criteria

- All Phase 1 tests pass; `cargo clippy -p quarto-core --all-targets -- -D warnings` and
  `cargo nextest run -p quarto-core` are green.
- A document with `jupyter: <name>` starts that kernel regardless of the first cell
  language; one without the key behaves exactly as before (existing
  `partition_cells` and `execute_qmd` tests untouched).
- `resolve_kernel` is either wired in or gone.

## Risks and open questions

- **Metadata type mismatch.** Lowering uses `as_plain_text` for string-like values
  (`project/mod.rs:1379-1383`), so a bare YAML string reaches `kernel_from_ts_metadata` as a
  `String`; a number or bool gives `None` (covered by the non-string test). The remaining
  risk is the `engine_config` read, which is a `ConfigValue`: use `as_plain_text` there too.
- **Cell-less pages with a declared engine.** For a document-declared engine, a page with no
  cells now reaches the engine stage: a missing runtime is a hard error (q1-compatible, see
  the Phase 2 task), the stamped profile changes meaning, and an engine capture is emitted.
  A declaration that appears only in the project layer does not start an engine (a plain page
  in a project starts none), but a project engine merged with a document-level one does start
  (accepted quirk, Gordon 2026-10-31, see Progress); this differs from q2's existing behavior for pages that *do* have
  cells, where the merged-metadata declaration applies and q1's does not (not changed here).
- **Allowlist versus explicit kernel.** Until JOL-2b, a SoS document with a `bash`
  cell does not execute that cell. Q1 does not "send everything" either: it makes code
  cells only of fences in the kernelspec\'s language (`jupyter.ts:321-325`) and leaves
  other fences display-only (verified by run with `jupyter: sos`: `{python}` stayed
  display-only, only `{sos}` ran), so sending every Jupyter-owned cell to the one kernel is
  a q2 behavior change for Q1 documents (divergence 1). This is
  the JOL epic open risk "explicit `jupyter:` kernel versus the ownership gate"; JOL-2b
  decides it.
- **Mixed-language regression.** The ggsql allowlist fix already on main means a
  document with `python` then `ggsql` cells runs both on the first kernel and the
  second fails with a SyntaxError (verified 2026-10-30, with or without `engine:
  jupyter`). `jupyter: sos` is the supported route for mixed-language documents; JOL-3
  (several kernels) is deferred.
- **Engine-config path.** The user guide documents `engine: [{jupyter: {kernel: x}}]` but
  the engine ignores it today (verified: `execute_qmd` reads only the first cell
  language). It has no Q1 equivalent. This plan honors it as a second source (Phase 2),
  below the `jupyter:` key (decided 2026-10-30).

## References

- `crates/quarto-core/src/engine/jupyter/text_execute.rs` (`execute_qmd`,
  `map_language_to_kernel`), `kernelspec.rs` (`resolve_kernel`), `daemon.rs`
  (`get_or_start_session`), `engine/context.rs`.
- Q1: `src/core/jupyter/jupyter.ts` (`jupyterKernelspecFromMarkdown`),
  `src/core/jupyter/kernels.ts`.
- [The JOL epic](2026-10-17-jupyter-owned-languages-epic.md), "Research notes: SoS".
