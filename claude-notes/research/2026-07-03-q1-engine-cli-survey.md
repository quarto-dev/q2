# Q1 engine-touching CLI surface — full inventory (2026-07-03)

## Purpose

Exhaustive inventory of **every Quarto 1 CLI command / subcommand whose
behavior touches an execution engine** (jupyter, knitr, julia, markdown, and
engine *extensions* like marimo / the bundled julia engine), so the q2 project
knows the full Q1-parity surface for engine work.

Two touchpoints are **already tracked** and out of scope here except for
cross-reference:

- `quarto call engine …` → q2 **Plan 9**, strand `bd-m1jeqhhz`
- `quarto check`'s engine section → q2 **Plan 10**, strand `bd-4qflzhwh`

This document covers **the rest**, plus the "remove julia" bug hunt.

## Sources & method

- Q1 (read-only reference): `external-sources/quarto-cli` (TypeScript).
- Command registry enumerated from `src/command/command.ts` (`commands()`),
  which registers 24 top-level commands.
- Engine interface read from `src/execute/types.ts`
  (`ExecutionEngineDiscovery` / `ExecutionEngineInstance`). The members a CLI
  command can reach into: `populateCommand` (adds engine subcommands →
  `call engine`), `checkInstallation` (→ `check <engine>`), plus the instance
  members `execute`, `run`, `dependencies`, `postprocess`, `postRender`, and
  the discovery members `claimsFile` / `claimsLanguage` / `defaultYaml` /
  `defaultContent` (engine selection & scaffolding).
- q2 status cross-checked against `target/debug/q2 --help`, `crates/quarto/src/main.rs`,
  and `crates/quarto/src/commands/*.rs`.

## Summary table

| Q1 command | Engine touchpoint | User-visible behavior | q2 status | Recommended home |
| --- | --- | --- | --- | --- |
| `render` | Engine **selection** + `--execute` / `--no-execute`, `--execute-daemon`, `--execute-daemon-restart`, `--execute-debug`, `--execute-dir`, `-P/--execute-param(s)` | Runs the file's engine (jupyter/knitr/julia/markdown); jupyter daemon flags keep a kernel alive | flags **exist**, engine exec **stub** (no real jupyter/knitr/julia) | q2 render pipeline epic (not Plan 9/10) — big |
| `preview` | Same engine selection as render + `isServerShinyPython(engine)` branch, `--no-watch-inputs` | Live re-render through the engine; Shiny-python preview path | exists (real preview), engine exec stub | q2 render pipeline epic — big |
| `serve` | Runs a Shiny interactive doc via the engine instance's `run()` | `quarto serve app.qmd` renders then serves a Shiny app | **stub** (`NotImplemented`) | own strand (Shiny runtime) |
| `run` | Script handlers for R / Python / Lua / deno / shell (`src/core/run/`) — engine *runtimes*, not the `ExecutionEngine` interface | `quarto run script.py` runs a utility script in the matching language | **stub** (`NotImplemented`) | own strand (script runner) |
| `convert` | ipynb ⇄ qmd via jupyter notebook format (`isJupyterNotebook`, `jupyterNotebookToMarkdown`) | `quarto convert nb.ipynb` → `.qmd` and back | **stub** (`NotImplemented`) | own strand (jupyter notebook I/O) |
| `create-project` | `--engine <engine:string>` with `engine:kernel` syntax; scaffolds via engine `defaultYaml`/`defaultContent` | `quarto create-project --engine jupyter:julia-1.9` | **absent** (folded into `create`) | fits q2 `create` work |
| `create` (extension) | `create extension` **type `engine`** (engine-extension template, with cell language); `initializeProjectContextAndEngines()` loads bundled engines | scaffold a new engine extension | exists (`create`), engine-template stub | own strand (engine-extension authoring) |
| `use binder` | Reads `projEnv.engines` (knitr→R, jupyter→python) to emit binder/Dockerfile/`environment.yml` | `quarto use binder` builds reproducible-env config aware of the project's engines | exists (`use`), binder **stub** | own strand (binder) |
| `inspect` | Returns a project's **engines** and a file's resolved **engine** | `quarto inspect` reports config + engines | **absent** (q2 has `get-config`/`trace` instead) | own strand (parity later) |
| `capabilities` (hidden) | Reports `python` = `jupyterCapabilities()` + kernel list via `jupyterKernelspecs()` | hidden JSON dump of formats/themes/**python+kernels** | **absent** | fits Plan 10 (check/capabilities share jupyter probing) |
| `remove` | Enumerates & deletes **extensions**, incl. bundled engine extensions (julia) — **built-in guard escaped, see bug section** | `quarto remove julia-engine` can delete the bundled julia engine | **stub** (`NotImplemented`) | own strand + carry the guard fix |
| `list` | Lists extensions incl. engine extensions; `788d1d3e8` added the same `builtIn` filter | `quarto list extensions` shows engine extensions | **stub** (`NotImplemented`) | own strand |
| `install` / `uninstall` / `tools` | Manage **tools** = TinyTeX + Chromium **only** (NOT engines — see `installableToolNames()`) | `quarto install tinytex` etc. | **stub** (`NotImplemented`) | own strand (not engine parity) |
| `add` / `update` | Extension install/update (engine extensions included as extensions) | `quarto add <ext>` / `quarto update <ext>` | **stub** (`NotImplemented`) | own strand |
| `dev-call pull-git-subtree` | Pulls the **julia-engine** git subtree from `PumasAI/quarto-julia-engine` | dev-only: refresh bundled julia engine source | **absent** (dev tooling) | out of scope (dev) |
| `call engine` | Engine `populateCommand` hook | *(tracked — Plan 9 / bd-m1jeqhhz)* | `call` exists but only dispatches `test`; **engine absent** | **Plan 9** |
| `check [engine]` | Engine `checkInstallation` hook | *(tracked — Plan 10 / bd-4qflzhwh)* | **stub** (`NotImplemented`) | **Plan 10** |

## Per-command detail (file:line evidence)

### render — engine selection + execute flags
`src/command/render/cmd.ts:51-79` registers the execute family:
`--execute` / `--no-execute` (`:51`), `-P/--execute-param` (`:55`),
`--execute-params` (`:59`), `--execute-dir` (`:63`), and the **jupyter-daemon**
flags: `--execute-daemon` ("Keep Jupyter kernel alive (defaults to 300
seconds)", `:67-69`), `--execute-daemon-restart` ("Restart keepalive Jupyter
kernel before render", `:71-73`), `--execute-debug` (`:75`). Engine choice is
made by the discovery phase (`claimsFile`/`claimsLanguage`, `src/execute/types.ts:45-57`)
and the file is run through the engine's `execute()`.

**q2 status:** the flags exist on `q2 render --help` (verified), but
`--execute-debug` has been **repurposed** in q2 to "Replay engine output from a
recorded trace file" rather than debug output — a semantic divergence worth
noting. There is no real jupyter/knitr/julia execution in q2 yet.

### preview
`src/command/preview/cmd.ts:304` branches on
`isServerShinyPython(renderFormat, engine?.name)` — engine-aware Shiny preview.
Otherwise it re-renders through the same engine as `render`. `--no-watch-inputs`
(`:90`) governs the re-render loop, not the engine directly. q2 has a real
(and more advanced) preview, but engine execution underneath is stubbed.

### serve — Shiny
`src/command/serve/cmd.ts` — "Serve a Shiny interactive document." Renders
(unless `--no-render`) then serves; the interactive runtime is driven by the
engine instance's optional `run()` member (`src/execute/types.ts:127`).
q2 `serve` is an 8-line `NotImplemented` stub
(`crates/quarto/src/commands/serve.rs:7`).

### run — script runtimes
`src/command/run/run.ts` + `src/core/run/run.ts`: a registry of `RunHandler`s
(`src/core/run/{r,python,lua,deno,shell}.ts`) selected by `canHandle(script)`.
This runs **utility scripts** in R/Python/Lua/TS/shell — it exercises the
language *runtimes* engines depend on, but **not** the `ExecutionEngine`
interface. q2 `run` is `NotImplemented` (`crates/quarto/src/commands/run.rs:7`).

### convert — jupyter notebook I/O
`src/command/convert/cmd.ts` + `convert/jupyter.ts`: `isJupyterNotebook(input)`
picks direction, then `jupyterNotebookToMarkdown` / `markdownToJupyterNotebook`.
Pure ipynb⇄qmd; no kernel execution, but it is jupyter-format-specific. q2
`convert` is `NotImplemented` (`crates/quarto/src/commands/convert.rs:7`).

### create-project / create — engine scaffolding
- `src/command/create-project/cmd.ts:50-55` — `--engine <engine:string>`
  ("Use execution engine (jupyter, knitr, markdown, ...)") with `engine:kernel`
  parsing (`:55`); `:11` imports `executionEngine`/`executionEngines`.
- `src/command/create/artifacts/extension.ts:46,88,186-188` — extension type
  **`engine`** with a cell-language prompt (this is how a marimo/julia-style
  engine extension is scaffolded).
- Both call `initializeProjectContextAndEngines()`
  (`create/cmd.ts:56-57`, `create-project/cmd.ts:12`), which loads **bundled
  engines** via a zero-file project context (`src/command/command-utils.ts:23-66`).
q2 folds this into `create`; the `--engine` project flag and the `engine`
extension template are absent/stub.

### use binder — engine-aware reproducible env
`src/command/use/commands/binder/binder.ts:120-155,513` reads
`projEnv.engines` and special-cases `knitr` (needs R, `:513`) and the
markdown-only case (`:150-155`) when generating binder config. q2 `use` exists
as a command but binder is stub.

### inspect
`src/command/inspect/cmd.ts:20-21` — "Inspecting a project returns its config
and **engines**… an input path returns its formats, **engine**, and dependent
resources." No q2 equivalent (q2 has `get-config` + `trace`).

### capabilities (hidden)
`src/command/capabilities/capabilities.ts:22-37` — `python:
await jupyterCapabilities()`, and when jupyter_core is present,
`kernels = jupyterKernelspecs()`. This is the same jupyter probing `check`
does, so it is a natural companion to **Plan 10**. No q2 equivalent.

### install / uninstall / tools — TOOLS only, not engines
`src/command/tools/cmd.ts`, `install/cmd.ts`, `uninstall/cmd.ts` all route
through `installableToolNames()` / `installableTools()`
(`src/tools/tools.ts`). A grep of `src/tools/` for `julia|jupyter|knitr`
returns **nothing** — the tool system manages **TinyTeX and Chromium /
chrome-headless-shell only**. These are *not* an engine-parity concern; listed
here so their absence from engine scope is documented, not skipped. All are
`NotImplemented` stubs in q2.

## The "remove julia" bug — FOUND (present in current Q1)

**Verdict: real bug, still present.** `quarto remove julia-engine` (or picking
it from the interactive list) will **delete the bundled julia execution
engine** from the Quarto installation, because the built-in-extension guard
only protects extensions whose organization is `"quarto"`, and the bundled
julia engine loads with **`organization: undefined`**.

### Mechanism (file:line)

1. The julia engine ships **inside Quarto** as a git-subtree extension at
   `src/resources/extension-subtrees/julia-engine/_extensions/julia-engine`
   (subtree pulled by `dev-call pull-git-subtree`,
   `src/command/dev-call/pull-git-subtree/cmd.ts:22-25`). Its `_extension.yml`
   declares `contributes.engines` and has **no `organization` field**.

2. When extensions are enumerated, subtree extensions are read by
   `readSubtreeExtensions` → `readExtensions(subtreeExtensionsPath)` **with no
   `organization` argument** (`src/extension/extension.ts:305-306`). In
   `readExtensions`, an extension folder found directly under `_extensions`
   gets `extensionId = { name, organization }` where `organization` is
   `undefined` (`src/extension/extension.ts:487`). So the julia engine's id is
   `{ name: "julia-engine", organization: undefined }`.

3. The subtree path is **always** part of the scanned dirs:
   `inputExtensionDirs()` unconditionally includes `builtinSubtreeExtensions()`
   (`src/extension/extension.ts:544,569`). So `remove`/`list` see the julia
   engine.

4. **Both guards check `organization === kBuiltInExtOrg` where
   `kBuiltInExtOrg = "quarto"`** (`src/extension/constants.ts:7`):
   - The list/find filter: `options?.builtIn !== false || ext.id.organization
     !== kBuiltInExtOrg` (`src/extension/extension.ts:104`). For julia,
     `undefined !== "quarto"` is **true**, so `{ builtIn: false }` does **not**
     filter it out — it appears in the removable set.
   - The hard block in `removeExtension`: `if (extension.id.organization ===
     kBuiltInExtOrg) throw …"can't be removed since it is a built in
     extension"` (`src/extension/remove.ts:13-16`). For julia,
     `undefined === "quarto"` is **false**, so **no throw** — execution reaches
     `Deno.remove(extension.path, { recursive: true })`
     (`src/extension/remove.ts:20`), deleting the engine from the install tree.

5. `quarto remove julia-engine` resolves the name (not a tool, since
   `installableTools()` is just tinytex/chromium — `src/command/remove/cmd.ts`
   `resolveCompatibleArgs`), then `extensionContext.find("julia-engine", …,
   { builtIn: false })` matches it (org-less glob `*/julia-engine/` and bare
   `julia-engine/`, `src/extension/extension.ts:604-611`) and passes it to the
   unguarded delete.

### Why the guard misses it

The guard commit `788d1d3e8 "Filter out built in extensions"` (Charles Teague,
2022-09-27) predates engine-as-extension entirely and hard-codes
`organization === "quarto"`. The julia engine arrived later as a **subtree**
extension read *without* an org, so it is "built-in" in location but not in
`organization`, and slips through every guard. There is **no** engine-specific
removal guard anywhere: a grep of `src/extension/` and the remove/list commands
for `contributes.engines` in a protective context, or any "is an engine /
cannot remove engine" message, finds nothing (`remove.ts:15` is the only
"can't be removed" string, and it is org-gated).

### Parity implication for q2

q2's `remove`/`list` are `NotImplemented` stubs today, so q2 is **not yet
vulnerable** — but when q2 implements extension removal it must **not** copy
the org-only guard. The correct guard is either (a) protect any extension whose
resolved path is under the install/subtree resource dir, or (b) refuse to
remove any extension that `contributes.engines` unless explicitly forced.
Recommend capturing this as its own strand attached to the future
`remove`/`list` work (out of Plan 9/10 scope).

## Commands deliberately ruled OUT (checked, no engine touch)

- `pandoc` — runs the embedded Pandoc binary; no engine. (grep clean)
- `typst` — runs the embedded Typst binary; no engine. (grep clean)
- `publish` — renders (engine runs only via the render path it delegates to)
  then uploads; the publish command itself has no engine code. (grep clean)
- `editor-support` — LSP / YAML-intelligence surface; no engine. (grep clean)
- `add` — extension install; no engine-specific code (engine extensions are
  just extensions). (grep clean for engine/jupyter)
- `dev-call` (other than `pull-git-subtree`) — developer utilities.
- q2-only commands with no Q1 engine analogue: `lsp`, `get-config`, `trace`,
  `mcp`, `hub`, `build-ts-extension` (the last builds a TS engine extension
  bundle — authoring tooling, not engine execution).

## Recommended parity homes (roll-up)

- **Plan 9** (`call engine`): already scoped.
- **Plan 10** (`check <engine>`): already scoped; **fold `capabilities` in**
  since it shares the jupyter capability/kernel probing.
- **Own strands** (each outside Plan 9/10): `serve` (Shiny), `run` (script
  runner), `convert` (ipynb I/O), `create`/`create-project` engine scaffolding
  + `engine` extension template, `use binder`, `inspect`, and the
  extension-management family (`remove`/`list`/`add`/`update`) — the last
  **must carry the engine-removal-guard fix** so q2 never reintroduces the
  "remove julia" bug.
- **Not engine parity:** `install`/`uninstall`/`tools` (TinyTeX + Chromium only).
- **Big / render-pipeline epic:** the `render`/`preview` execute flags and
  actual engine execution.
