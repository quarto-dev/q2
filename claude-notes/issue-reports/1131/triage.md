# Issue #1131 — Code cells get none of the Quarto 1 execute variables

- **GitHub**: https://github.com/quarto-dev/q2/issues/1131
- **Reporter**: @mcanouil (Mickaël Canouil), 2026-11-09
- **Triage date**: 2026-11-10
- **Worktree**: `.worktrees/issue-1131` (branch `issue-1131`, based on `main` @ `4f7a08de2`)
- **Braid strand**: bd-8fmuyh4d
- **Scope**: the four execute variables Quarto 1 gives code cells (`QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`, `QUARTO_DOCUMENT_FILE`, `QUARTO_EXECUTE_INFO`) and the data behind them, including the resolved brand for each mode. Excludes `renderings: [light, dark]` (GH #1130), which the same dashboard needs but is a separate filter port.

## Summary

The report is accurate and reproduces exactly: q2 sets none of the four variables. The maintainer's direction (issue comment, and the brief for this triage) is to give q2 documents **the same data** as Quarto 1, without necessarily copying Quarto 1's mechanism. Two constraints shape the design: q2 must never mutate its own process environment (multithreading), and building a large JSON file in the middle of every render is a performance concern.

The investigation finds that both constraints can be met while keeping the Quarto 1 contract (variable names and the JSON file) for compatibility:

- The environment problem is real, but it is **not** about q2's own environment. q2 already passes environment to children explicitly. The hard case is that q2's Jupyter kernels are **shared by every document of a project**, and their environment is fixed when they start. Per-document values therefore need a per-document step inside the kernel, the way Quarto 1 sets `QUARTO_FIG_*`.
- The JSON file is small. Quarto 1 writes 6.5 KB for a plain document and 7.9 KB with a brand; three quarters of it is the localized `language` strings. q2 already lowers the merged metadata for every engine run, so the data costs almost nothing extra to build. The file only matters where code actually runs, and every executing engine is native-only, so WASM never sees it.

Conclusion: real feature gap, moderate scope. Filed as a feature strand.

## Reproduction

Fixtures: `repro/index.qmd` (the reporter's cell, extended to measure the execute-info file) and `repro-brand/` (the same cell next to a light/dark `_brand.yml`).

Quarto 1 (`quarto` 99.9.9 dev build), output in `repro/q1-output.txt` and `repro-brand/q1-output.txt`:

```text
QUARTO_EXECUTE_INFO = /var/folders/.../quarto-session36aa.../98c62ab6564f76dc
QUARTO_PROJECT_ROOT = .../issue-reports/1131/repro
QUARTO_DOCUMENT_PATH = .../issue-reports/1131/repro
QUARTO_DOCUMENT_FILE = index.qmd
execute-info bytes: 6513
format keys: ['execute', 'identifier', 'language', 'metadata', 'pandoc', 'render']
bytes per format bin: {'identifier': 82, 'execute': 428, 'render': 785, 'pandoc': 251, 'language': 4935, 'metadata': 202}
brand modes: None
```

With `_brand.yml` the file grows to 7882 bytes (`render` goes from 785 to 2247 bytes) and `brand modes: {'light': True, 'dark': True}`.

q2 at `main` (`cargo run --bin q2 -- render <copy>/repro/index.qmd`, run on a copy outside `claude-notes/` because that project excludes it from its render list). Output in `repro/q2-output.txt`:

```text
QUARTO_EXECUTE_INFO = None
QUARTO_PROJECT_ROOT = None
QUARTO_DOCUMENT_PATH = None
QUARTO_DOCUMENT_FILE = None
```

## What Quarto 1 provides

**Source.** `src/execute/environment.ts` (`setExecuteEnvironment`), called once per document from `src/command/render/render-files.ts:238`, just before `engine.execute`:

- `QUARTO_PROJECT_ROOT` = project dir, or the execute cwd for a lone file;
- `QUARTO_DOCUMENT_PATH` = directory of the source (or the cwd for a lone file);
- `QUARTO_DOCUMENT_FILE` = base name of the source;
- `QUARTO_EXECUTE_INFO` = path to a temp file holding `{"document-path": <abs path>, "format": <the whole Format>}`.

All four are set with `Deno.env.set` on the Quarto process itself. Children inherit them only because they are spawned afterwards. Quarto 1 knows this is fragile: `src/command/render/render-shared.ts:81` sets `QUARTO_PROJECT_DIR` the same way under the comment `FIXME we can't set environment variables like this with asyncs flying around`.

**Brand.** `src/command/render/render-contexts.ts:575` puts `{light, dark}` `Brand` objects into `format.render.brand` (`dark` only when the brand or the document enables dark mode). `JSON.stringify` of a `Brand` includes its raw `data`, which is what the reporter reads (`format.render.brand.<mode>.data`).

**Other per-execution variables.** The Jupyter setup step sets `QUARTO_FIG_WIDTH`, `QUARTO_FIG_HEIGHT`, `QUARTO_FIG_DPI` and `QUARTO_FIG_FORMAT` *inside the kernel* (`src/resources/jupyter/notebook.py:123`, `set_env_vars`). This is the per-document in-kernel mechanism the q2 design below reuses. q2 does not set these either (see Cross-references).

**Docs.** quarto-web documents the four variables in `docs/advanced/environment-vars.qmd` (table "Variables Quarto sets") and the JSON in `docs/advanced/quarto-execute-info.qmd`, which publishes the six format bins as a schema. `QUARTO_EXECUTE_INFO` is the documented recipe for **conditional execution by format** in `docs/computations/python.qmd`, `r.qmd`, `execution-options.qmd` and `authoring/_visibility-vs-execution.md`. That widens the audience well beyond brand users: it is the official answer to "skip this cell for PDF". `docs/projects/code-execution.qmd:97` also names `QUARTO_PROJECT_DIR` as the way to find the project from code.

## Localization in q2

- **q2 never mutates its environment, by design.** `crates/quarto-core/src/project/environment.rs` (module docs): `_environment` values are data, and spawn sites apply them with `Command::env`. This is the right base; nothing here needs `set_var`.
- **Engine context.** `crates/quarto-core/src/stage/stages/engine_execution.rs:630-686` builds the `ExecutionContext` for each engine run. It already has every input the variables need: `source_path`, `project_dir` (`None` for a lone file), `cwd`, the lowered merged metadata (`with_metadata`, field at `engine/context.rs:206`) and the pre-filtered `project_env` (which already carries `QUARTO_PROFILE`, bd-fu16z22k). The natural seam is to add the execute variables here, next to `QUARTO_PROFILE`.
- **knitr** spawns one R process per document with `.envs(project_env)` (`engine/knitr/subprocess.rs:386-387`). Spawn-time environment is correct there.
- **Jupyter** (`engine/jupyter/daemon.rs:73-88`): sessions are keyed by `(kernel_name, working_dir)`, and `extra_env` is a spawn-time input deliberately left out of the key. `cwd` is `ctx.project.dir` (`engine_execution.rs:632`), so **all documents of a project that use one kernel share one kernel process**. Spawn-time environment cannot carry per-document values; a second document would see the first one's `QUARTO_DOCUMENT_FILE`. Per-document values need a silent, `store_history = false` request before the document's cells (`engine/jupyter/execute.rs:28-58` already supports that request shape for inline expressions). This is Quarto 1's `set_env_vars` approach, and it changes only the kernel's environment, never q2's.
- **TS engines** (Node host, `engine/ts_engine.rs`) receive `TsExecuteOptions.format.metadata` and partition it into Quarto 1's six bins in `ts-packages/quarto-engine-host-node/src/metadata-as-format.ts`. That is already the Quarto 1 `Format` shape the JSON file needs.
- **Brand at engine time.** q2 resolves the brand per document only in later stages (`compile_theme_css.rs:614-630` via `config_paths::brand_path_resolver`; `ProjectConfig.brand` at `project/mod.rs:1231` holds the project-level one). `UnifiedBrand::split` (`crates/quarto-brand/src/lib.rs:32-41`) produces the light and dark `Brand`s. Execute info needs this resolution moved, or repeated, before engine execution.
- **Plan record.** `claude-notes/plans/2026-04-16-plan1a-protocol.md:282` treats `EXECUTE_INFO` as a Jupyter-engine detail covered by `temp_dir`. That reading was too narrow: the variable is user-facing API, documented for all engines.
- **WASM.** Jupyter, knitr and the TS engine host are all `#[cfg(not(target_arch = "wasm32"))]` (`engine/mod.rs:79-100`), so WASM never executes code and never needs the file. Only the data assembly (if it lives in a shared stage) must stay cfg-neutral.

## Open questions — resolved during triage

**Does the variable approach force q2 to mutate its own environment?** No. knitr takes spawn-time `Command::env`. The Jupyter kernel takes an in-kernel request, which mutates only the kernel process (one execution at a time). TS engines get the values in their options. The multithreading concern is real, but only for `std::env::set_var` in q2 itself, and nothing in this design calls it.

**Is spawn-time env enough for Jupyter?** No. By code reading (`daemon.rs` key plus `cwd = project.dir`), the kernel outlives the document within a project render. `QUARTO_PROJECT_ROOT` is stable for a kernel's lifetime and could go at spawn time, but `QUARTO_DOCUMENT_*` and `QUARTO_EXECUTE_INFO` must be set per document. Doing all four in the per-document request keeps one code path.

**Is the JSON file a performance problem?** Measured: 6.5 KB to 7.9 KB, mostly `language`. One write per executed document into the per-run `temp_dir` has no shared state, so it cannot contend under parallel rendering. The data is already built (`ctx.metadata`). Code execution dominates any document that has a kernel. The remaining risk is a **large website**, where the `metadata` bin may carry project-wide config (navigation and similar). The implementation should measure that case before deciding whether to trim. Documents that execute nothing (markdown engine) should not write the file at all.

**Could q2 drop the file for something more q2-native?** A file is the only channel that every kernel language reads, that reaches subprocesses a cell spawns, and that helper modules can use. The reporter's case is a helper module, so scanning the cell source for `QUARTO_EXECUTE_INFO` to decide whether to write would miss it. In-kernel variables would break every existing document and doc example. Recommendation: keep the file as the compatibility contract. A q2-native accessor (a `quarto` helper per language) can come later, on top of the same data.

## Outcome / recommended next step

Filed bd-8fmuyh4d (feature). Proposed shape:

1. **Variables.** Set `QUARTO_PROJECT_ROOT`, `QUARTO_DOCUMENT_PATH`, `QUARTO_DOCUMENT_FILE` and `QUARTO_EXECUTE_INFO`, with Quarto 1's values (lone file: root = the document's directory). Deliver them per engine: knitr by `Command::env`, Jupyter by a silent per-document setup request, TS engines through `TsExecuteOptions` (the host sets them for the child it runs). Never `set_var` in q2.
2. **Execute info.** One file per executed document in the run's `temp_dir`, built from the already-lowered metadata in Quarto 1's shape: `document-path` plus `format` with the six bins, reusing `metadata-as-format`\'s partition so the two cannot drift. `format.render.brand.{light,dark}` carries the resolved brand per mode, each with at least `data` (the field the reporter reads). Skip the write when no code executes.
3. **Docs.** A q2 page that states the variables and the schema, including where q2 differs from Quarto 1.
4. **Tests first.** One end-to-end render per engine (knitr, Jupyter, and the fake kernel for CI) that prints the four variables. A two-document project test proves Jupyter does not leak `QUARTO_DOCUMENT_FILE` across documents. A brand fixture asserts `format.render.brand.dark.data` is present.

Decisions for the maintainer before implementation:

- Keep `language` in the file (Quarto 1 shape, 75% of the bytes), or omit it?
- Is `dark` present only when dark mode is enabled (Quarto 1), or always when the brand defines dark values?
- Set `QUARTO_FIG_*` in the same step, or leave them to a separate strand?

## Verification commands used

```bash
gh issue view 1131 --repo quarto-dev/q2 --json title,body,author,createdAt,labels,comments
gh issue view 1130 --repo quarto-dev/q2 --json title,body,comments
cargo xtask create-worktree --issue 1131
cargo xtask verify --skip-hub-build
cd claude-notes/issue-reports/1131/repro && quarto render index.qmd --to md -o q1-out.md        # saved as q1-output.txt
cd ../repro-brand && quarto render index.qmd --to md -o q1-out.md                               # saved as q1-output.txt
cargo run --bin q2 -- render <scratch-copy>/repro/index.qmd                                     # grep of index.html saved as q2-output.txt
grep -rn "EXECUTE_INFO\|setExecuteEnvironment\|QUARTO_DOCUMENT_PATH\|QUARTO_PROJECT_ROOT" external-sources/quarto-cli/src
grep -rln "QUARTO_EXECUTE_INFO\|QUARTO_DOCUMENT_PATH" external-sources/quarto-web/docs
grep -rn "QUARTO_PROJECT_DIR\|QUARTO_DOCUMENT\|QUARTO_EXECUTE_INFO\|QUARTO_FIG_" crates
```

## Cross-references

- GH #1130 (`renderings: [light, dark]`), the other half of the reporter's dashboard.
- `claude-notes/plans/2026-04-16-plan1a-protocol.md:282`, the earlier scoping of `EXECUTE_INFO`.
- bd-fu16z22k, `QUARTO_PROFILE` for engine cells (the model for adding variables at `engine_execution.rs:671`).
- `crates/quarto-core/src/project/environment.rs`, the never-mutate-the-environment invariant.
