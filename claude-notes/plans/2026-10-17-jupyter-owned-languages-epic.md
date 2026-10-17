---
title: 'Epic: the Jupyter engine executes the languages it owns (any kernel, several kernels per document)'
date: 2026-10-17
description: 'Outline of an epic that replaces the Jupyter engine''s hard-coded language lists with resolver-assigned ownership (Plan 4d''s owned_languages), Q1-style kernelspec lookup by language, and multiple kernels per document.'
---

**Status:** outline only. It gets further review and is then split into per-phase
plans. Not started.
**Subsumes:** [Plan 4d](2026-07-06-plan4d-owned-languages.md) (`owned_languages`).
**Trigger:** `{ggsql}` cells were never executed by `q2 render`
(see "Starting point").

## Goal

The Jupyter engine runs exactly the languages the resolver assigned to it, and
finds each language's kernel the way Q1 does. No language is named in q2 source
except where an alias table is genuinely needed. A document may use several
kernels.

## Starting point (verified 2026-10-17, main `fe52ed0fb`)

Three hard-coded, partly overlapping language lists gate Jupyter execution:

| Where | What it decides | Contents |
| --- | --- | --- |
| `engine/jupyter/text_execute.rs` `is_executable_language` | which cells execute | python/python3/py, julia/jl, r, ruby/rb, rust/rs, typescript/ts, javascript/js (+ `ggsql` after the minimal fix) |
| `engine/jupyter/text_execute.rs` `map_language_to_kernel` | language → kernel name | small alias table; unknown language name passes through as the kernel name |
| `engine/jupyter/kernelspec.rs` `is_jupyter_language` | nothing found (re-exported, no caller seen) | python/julia/r/scala/ruby/bash/sh |

Other facts the epic builds on:

- The resolver already assigns ownership correctly. Jupyter's `claims_language`
  returns `Fallback(0)` for every language, so an unclaimed language such as
  `ggsql` goes to jupyter via T4 (implicit sequence) or T2 (explicit
  `engine: jupyter`). The owned set simply never reaches the engine.
- `EngineResolution::handled_languages_for` is only the negative projection
  (`HANDLED_LANGUAGES ∪ {lang : owner != engine}`). `partition_cells` therefore
  treats "not ceded" as "mine", which cannot distinguish "owned by me" from
  "owned by nobody" (Plan 4d, Background §1–2).
- Kernel choice is `executable[0].language` only; every block runs on that one
  kernel (`text_execute.rs`, "Determine the kernel from the first executable
  cell"). Sessions are already keyed by `SessionKey { kernel_name, working_dir }`
  in `engine/jupyter/daemon.rs`, so several concurrent sessions are structurally
  possible.
- `runtimelib` expands only `{connection_file}` in kernelspec `argv`. The minimal
  fix (commit `0e443883d`) expands `{resource_dir}` in `daemon.rs`.

### Q1 behavior to match

- `src/execute/engine.ts:173-210`: any braced language that no engine claims
  and that is not a handler language (`ojs`, mermaid, dot, ...) goes to Jupyter.
  Jupyter's own `claimsLanguage` claims only `julia`.
- `src/core/jupyter/kernels.ts:27` `jupyterKernelspecForLanguage`: first installed
  kernelspec whose `language` equals the block language; python prefers a kernel
  inside `VIRTUAL_ENV`. The YAML `jupyter:` key overrides.
- One kernel per document (the kernelspec's `language` is passed to
  `toMarkdown`). Several kernels per document is a q2 extension beyond Q1.

## Decisions (from Gordon, 2026-10-17)

1. **Jupyter's claim stays a universal `Fallback(0)`** (Q1 parity). No
   kernelspec check at resolution time.
2. **The epic subsumes Plan 4d.** Its Rust phases come first; the TS-wire phases
   ride along.
3. **Multiple kernels per document are in scope.**
4. **`jupyter:` key:** the scalar keeps its Q1 meaning (the kernel for the primary
   language, i.e. the first executable one). A new map form
   `jupyter: {python: python3, ggsql: ggsql}` overrides per language.
5. **Owned language with no matching kernelspec:** a loud error naming the
   language and saying how to install or pin a kernel. Not a warning, not a
   passthrough (design §10).
6. **Kernels are isolated sessions, with an ordering guarantee:** cells run
   strictly in document order across kernels (so a file written by one cell can
   be read by the next); nothing else is shared between kernels.
7. **Delivery:** probably one PR, many sessions and plans of work. Do not plan
   around PR boundaries.

## Phases

### Phase 1: the Rust carrier (Plan 4d-A/B)

- `EngineResolution::owned_languages_for(engine)`: `{lang : ownership[lang] ==
  engine}`, sorted, present-in-render only.
- `ExecutionContext.owned_languages`, populated by `EngineExecutionStage` for
  every engine next to `handled_languages` (`engine_execution.rs:552`).
- Exit: resolver tests for owned set, including the owned-by-nobody case
  (explicit `engine: knitr` + `{julia}`).

### Phase 2: jupyter consumes ownership; kernel lookup by language

- `partition_cells` executes `ctx.owned_languages.contains(lang)`; delete
  `is_executable_language`. Owned-by-nobody and ceded cells pass through
  untouched.
- Replace `map_language_to_kernel` with kernelspec lookup by `language` field
  (Q1 rule, venv-first for python). Keep a small alias table only where the
  language name in the document differs from the kernelspec language (`py`,
  `jl`, `ts`, ...) — decide which aliases Q1 actually has.
- Missing kernelspec → the loud error from decision 5; this replaces the
  single-engine passthrough branch of `partition_cells`.
- Delete `is_jupyter_language` if it still has no caller.
- Check the other entry points that name languages: stored `.ipynb` replay
  (kernelspec language is lowercased into the fence) and percent scripts.
- Exit: `{ggsql}`, `{bash}`, an arbitrary fixture language all execute with no
  q2 source change; existing `partition_cells` tests keep their meaning
  (`test_sql_is_not_executable_language` becomes "sql is not owned by jupyter
  in these configurations").

### Phase 3: several kernels per document

- Resolve a kernel per distinct owned language (decision 4's `jupyter:` forms
  first, then kernelspec lookup); start sessions lazily, key by `SessionKey`,
  shut all down with the render's kernel scope.
- Execute blocks in document order, routing each to its language's session
  (decision 6). `execute_blocks_inner` currently takes one `kernel_name`.
- Things to check: figure file naming (`FigureWriter::begin_cell` is keyed to
  document position, so it should be unaffected); error messages should name
  the kernel; record/replay and preview capture assume one session; kernel
  startup cost for documents that never needed a second kernel must stay zero.
- Exit: a fixture document with two languages runs on two kernels, in order,
  with a cross-kernel file handoff.

### Phase 4: TS-engine wire (Plan 4d-C/D/E)

- `TsExecuteOptions.owned_languages`, TS types, host passthrough, wire-parity
  tests, the positive end-to-end assertion, and the design-doc reconciliation,
  exactly as Plan 4d specifies. Independent of Phases 2–3; order is free.

### Phase 5: docs and end-to-end

- User docs: using non-Python Jupyter kernels, with ggsql as the worked example
  (`uv venv`, `uv pip install jupyter ggsql-jupyter`, `ggsql-jupyter --install`,
  `engine: jupyter`), the `jupyter:` map form, the missing-kernel error.
- A fake-kernel integration test (no external install) covering Phases 2–3.
- Update `claude-notes/designs/engine-resolution.md` and Plan 4d's
  "native engines are out of scope" claims.

## Risks and open questions

- **Several kernelspecs for one language** (system python3 vs venv kernels,
  several Julia versions): Q1's first-match-plus-venv rule is the baseline. Is
  that enough once the map form exists?
- **Language aliases:** which aliases does Q1 really apply (`py`, `jl`, `ts`,
  ...)? The current q2 alias table is not known to be a copy of Q1's.
- **Freeze/cache keys:** resolution is one-shot and feeds freeze cache keys
  (design §6). Confirm that moving the gate from "allowlist" to "ownership"
  does not change any key for existing documents.
- **Mixed-language `.ipynb` input:** a stored notebook carries one
  notebook-level `metadata.kernelspec`, and q2 fences every code cell with that
  one language (see "Research notes", `.ipynb`). Does Phase 3 apply to notebooks
  at all, or only to `.qmd`? What should an `.ipynb` whose kernelspec has no
  `language` do?
- **Resource-dir edge cases** noted in review of the minimal fix (non-UTF-8
  kernelspec paths) are deliberately not handled.
- **Cell-option comment syntax for ggsql:** see "Research notes". Unlisted
  languages default to `#`, which is wrong for SQL-family languages.
- **Resolution of explicit `jupyter:` map keys:** the keys are language names.
  Are they matched case-insensitively and through the same alias table as
  fences?
- **Does `engine: jupyter` need to be written?** See "Research notes".

## Research notes (everything learned so far that the outline does not carry)

### How we got here (session of 2026-10-17)

- Goal of the session: set up a working directory with `uv`, install the ggsql
  Jupyter kernel, and see whether `.qmd` files with `{ggsql}` blocks render in
  `q2`. They did not.
- Two scratch directories exist, both with `01-basic.qmd` (bar chart),
  `02-scatter.qmd` (point plot with labels), `03-session.qmd` (CREATE TABLE in
  one block, plain SELECT in a second, plot in a third), `04-mixed-python.qmd`
  (`{python}` then `{ggsql}`):
  - `~/docs/ggsql` — venv plus the **hand-built** kernel
    (`cargo build --release -p ggsql-jupyter` in `~/src/ggsql`, then
    `~/src/ggsql/target/release/ggsql-jupyter --install`).
  - `~/docs/ggsql-user` — venv plus the **PyPI** kernel, following the end-user
    instructions from ggsql's own docs (`doc/get_started/tooling/jupyter-quarto.qmd`):
    `uv venv`, `uv pip install jupyter ggsql-jupyter`, `ggsql-jupyter --install`.
    It also holds a scratch `t.qmd` (python, `{foo}` and `{ggsql}` blocks) used to
    show that `{foo}` and `{ggsql}` behave identically.
- Both kernels are version 0.5.2. Both installs write the kernelspec with
  `--user` to `~/Library/Jupyter/kernels/ggsql`, so the second install
  **overwrote** the first (same name). Whichever was installed last is the one
  q2 uses. The hand-built one was installed last.
- Driving the hand-built kernel directly with `jupyter_client` (no q2) returned
  `image/png` and `text/plain` for a bar chart, so the kernel itself was never the
  problem.

### What was verified and what was not

- **Baseline on latest main (0.34.0, workspace-7 binary):** `{ggsql}` blocks
  render as literal text (`<pre class="{ggsql} code-with-copy">`), no kernel
  starts, no diagnostics. The 0.33 nightly on PATH behaved the same. `{foo}`
  behaves the same as `{ggsql}`; `{python}` runs.
- **With the allowlist fix only:** q2 then tried to spawn the kernel and failed
  with `failed to spawn kernel process for 'ggsql': No such file or directory`
  because `argv[0]` was the literal `{resource_dir}/ggsql-jupyter`.
- **With both fixes (commit `0e443883d`):** `01`, `02` and `03` render. `03` gives
  a SQL result table followed by a plot, so the DuckDB session persists across
  blocks. The `02` PNG was inspected and is a correct plot. Output goes to
  `<stem>_files/figure-html/cell-N-output-1.png`.
- **Not verified:**
  - `04-mixed-python.qmd` with the fix. The reviewer's claim (the python kernel
    gets the ggsql text, or the reverse) is expected, not observed.
  - The PyPI kernel with the fixed binary. The PyPI install writes the same
    `{resource_dir}` form, so it should work.
  - Whether `engine: jupyter` and `jupyter: ggsql` in the front matter are
    actually needed. The test files had both added while debugging, and they were
    never removed to test without them. The end-user instructions we want to give
    depend on this. A plain `{ggsql}` document with no front matter is the case to
    run.
  - An unexplained observation: the very first renders (before any `engine:` key
    was added) showed no python output in `04-mixed-python.html` either. It is not
    known whether a `{python}` block with no `engine:` key executes. Check before
    writing user docs.

### ggsql specifics

- The kernelspec is `{"argv": ["{resource_dir}/ggsql-jupyter", "-f",
  "{connection_file}"], "language": "ggsql", "env": {"RUST_LOG": "error"}}`;
  `--install` copies the binary into the kernelspec directory. Kernel name and
  language are both `ggsql`, which is why the pass-through in
  `map_language_to_kernel` happened to work.
- The kernel renders plots itself (image bundles), pure SQL as an HTML table, and
  keeps one in-memory DuckDB session (`-- @connect:` switches connections,
  `-- @uncache` clears the cache layer). Source: `~/src/ggsql/ggsql-jupyter/CLAUDE.md`.
  Output is PNG; whether HTML-targeted documents could use vector output (SVG or
  Vega-Lite) is unexplored.
- ggsql's docs describe `{ggsql}` blocks in Quarto and say "Each block in the
  document shares the same session" — a one-kernel-per-document assumption that
  matches Q1.
- **Cell options:** `comment_syntax_for` (`cell_options/mod.rs`, ported from Q1's
  `kLangCommentChars`) defaults unknown languages to `#`, but ggsql is SQL-family
  (`--` comments), and ggsql also uses `-- @...` magic comments. Whether `#|`-style
  options (`echo`, `error`, `fig-*`) work in a ggsql cell, and whether `#|` lines are
  valid ggsql, is untested. Q1's table lists `sql` as `--`; whether Q1 has a ggsql
  entry is unchecked.

### The resolver and execution path (code facts, main `fe52ed0fb`)

- `LanguageClaim` is `Primary(i32) | Interop(i32) | Fallback(i32) | None`
  (`engine/mod.rs:133`). Kind dominates priority. Tiers: T1 Primary, T2 explicit
  Fallback, T3 Interop (presence-gated), T4 implicit Fallback (only when the engine
  sequence is implicit).
- Jupyter: `claims_language` returns `Fallback(0)` for every language;
  `try_claims_language` answers statically (`engine/jupyter/mod.rs:266-276`).
- `HANDLED_LANGUAGES = ["ojs", "mermaid", "dot"]` (`engine/mod.rs:151`); its
  comment says it migrates to a registry when q2 grows real cell-handler support.
- `EngineResolution { sequence, ownership: LinkedHashMap<lang, engine>, notes }`.
  `ownership` has entries only for languages present in the render; a language can
  be absent (owned by nobody), e.g. explicit `engine: knitr` plus `{julia}`.
- `EngineExecutionStage` (`stage/stages/engine_execution.rs`): `multi_engine =
  to_run.len() > 1` (line 502), `handled_languages_for` (552), `with_multi_engine`
  (587); resolution runs once and is cached (Plan 4d cites line 230).
- `partition_cells` today: ceded (in `handled_languages`) → skip; else allowlisted
  → execute; else `multi_engine` → `NoHandlerForLanguage` error; else passthrough.
  `NoHandlerForLanguage` is a clean refusal that does not poison the engine
  instance.
- `execute_blocks_inner` takes one `kernel_name`, gets or starts one session, and
  runs all blocks on it. `JupyterDaemon` keeps sessions by `SessionKey`; a
  `kernel_scope()` guard shuts kernels down when the outermost scope drops.
  Cell options (`#|`) are partitioned per block before execution.
- Kernel discovery: `kernelspec.rs` wraps `runtimelib::list_kernelspecs`
  (data dirs only, no per-venv search of its own) and `find_kernelspec(name)`;
  `resolve_kernel` also reads the `jupyter:` metadata key (scalar string).
  `runtimelib` 2.0.0 `KernelspecDir::command` substitutes only `{connection_file}`.
- `.ipynb` input: `content_processors/ipynb.rs:212` reads the one notebook-level
  `metadata.kernelspec.language` and fences every code cell with it (`cell_wrap`,
  line 259); a kernelspec without `language` yields an empty fence language.
  Percent scripts have their own content processor
  (`content_processors/percent.rs`) and were not examined.
- `runtimelib` and `jupyter-protocol` are 2.0.0 in the cargo registry.

### Plan 4d facts that this epic changes

- Plan 4d states "q2 core does not read `owned_languages`" and "native engines
  (knitr/jupyter) ... leave `subprocess.rs`/`hooks.R` untouched". This epic reverses
  the first for jupyter and must amend the plan's Design decisions when it absorbs
  it. Plan 4d is 0/39 tasks done; its Phase 4d-A tests (`owned_languages_for` with a
  mock knitr+jupyter registry) can be reused as-is.
- Plan 4d describes `owned_languages` as informational and `handled_languages` as
  the execute-time gate. For jupyter this epic makes ownership the gate; whether
  `handled_languages` is then still needed by jupyter (the `ojs`/`mermaid`/`dot`
  leave-alone set) is open.
- Plan 4d notes a pre-existing wire doc comment that overstates `handled_languages`
  and `owned_languages` as "coincide"; Phase 4d-D corrects it.

### Minimal-fix review findings that were deliberately left

From the review of commit `0e443883d`:
- **Mixed kernels (real regression in behavior):** `{python}` plus `{ggsql}` used to
  leave the ggsql cells unexecuted; they now run on the first cell's kernel and fail.
  Phase 3 fixes this.
- `to_string_lossy` on a non-UTF-8 kernelspec path (rewrites invalid bytes); an
  `OsString`-based argv would avoid it. Jupyter itself does a plain string replace.
- `{resource_dir}` is expanded only at the one `.command(` call site. If a second spawn
  path appears, move the expansion into a shared wrapper around `command`.
- No test covers the spawn path actually using the expanded spec; that needs the
  fake-kernel fixture from Phase 5.
- Fixed in that commit: the path is absolutized (a relative `JUPYTER_PATH` would break
  once the kernel runs from another cwd); the weak `map_language_to_kernel("ggsql")`
  assertion was removed.

### Q1 reference points

- `quarto-cli` checkout at `~/src/quarto-cli` (HEAD `c57d2a483` when read):
  `src/execute/engine.ts` (engine claim loop and Jupyter fallback),
  `src/core/jupyter/kernels.ts` (`jupyterKernelspecForLanguage`),
  `src/execute/jupyter/jupyter.ts:113` (`claimsLanguage`), `:508`
  (`jupyterKernelspecFromMarkdown`), `src/core/pandoc/pandoc-partition.ts:144`
  (`languagesWithClasses`: the `{lang ...}` fence regex that decides which languages
  are present).
- The Q1 markdown language scan accepts `[a-zA-Z][a-zA-Z0-9_.]*` and records the first
  class (`{python.marimo}`); q2's equivalent scan was not compared.

### Local state at the end of the session

- Worktree `~/src/q2/.worktrees/workspace-7`, branch
  `feature/ggsql-executable-language` (from `origin/main` `fe52ed0fb`, previously
  `parking/workspace-7`). Debug binary: `target/debug/q2` there (0.34.0, includes the
  fix).
- That worktree's gitignored `CLAUDE.local.md` still describes an unrelated earlier
  use (pandoc-wasm lane H: "never push", "retire to `parking/workspace-7`"); it was
  not rewritten.
- Nothing has been pushed.
