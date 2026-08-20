# Engine-API Usage Ground-Truth Model (all five engines)

**Status:** reference / ground-truth research artifact — a *new review lens*.
**Built:** 2026-06-26, by reading the Q1 source at `~/src/quarto-cli`, the two
standalone TS extensions at `~/src/quarto-julia-engine` and `~/src/quarto-marimo`,
and the landed q2 surface at
`.worktrees/ts-engine-extensions/ts-packages/{quarto-api,quarto-types}/src/`.
Every factual claim carries a `file:line` citation I verified by reading the
source. Signatures are quoted verbatim. Companion to (read first, don't restate):
`claude-notes/research/2026-06-25-q1-engine-lifecycle-model.md`.

**The lens.** The q2 TS-engine-extension epic used the **Julia engine as its
only worked example**. Julia is atypically thin on the engine-author API: it
talks TCP to its own daemon and barely touches `quarto.system`/`quarto.jupyter`.
This model establishes the ground truth by surveying *all five* known engines so
we can name precisely the surface that is **used by some non-Julia engine** and
therefore at risk of being wrongly deferred/dropped by Julia-only planning.

**PROVIDES vs CONSUMES.** Throughout: "PROVIDES" = the engine *implements* an
`ExecutionEngine{Discovery,Instance}` interface member. "CONSUMES" = the engine
*calls* a `QuartoAPI` (`quarto.<ns>.<method>`) method. These are tracked
separately (Parts A and B).

**Built-in vs standalone.** knitr (`src/execute/rmd.ts`), jupyter
(`src/execute/jupyter/jupyter.ts` + `jupyter-kernel.ts`), and markdown
(`src/execute/markdown.ts`) are Q1's **built-in** engines (in q2 they are
reimplemented natively in Rust, so they don't call the TS `@quarto/api` at
runtime — but their usage is still ground truth for "what a faithful port owes").
julia (`~/src/quarto-julia-engine/src/julia-engine.ts`) and marimo
(`~/src/quarto-marimo/src/marimo-engine.ts`) are **standalone TS extensions** —
the truest analogue of a q2 TS engine extension, so their consumption is the
**most load-bearing evidence**. Both standalone engines import only *types* from
`@quarto/types` (julia-engine.ts:13-27; marimo-engine.ts:10-24) and reach the
runtime API through the module-level `quarto` ref stored by `init` (julia-engine.ts:49,76-78;
marimo-engine.ts:28,199-201) — exactly the Q1 built-in pattern.

---

## Bottom line

The **union** engine-author surface is the full nine-namespace `QuartoAPI` plus
the full two-interface engine protocol — but the *load-bearing* non-Julia usage
clusters in four places a Julia-only plan would plausibly miss:

1. **`quarto.system.pandoc(args, stdin)`** — called by **marimo** for HTML→markdown
   conversion (marimo-engine.ts:129-132). Julia never calls it. In q2 it is a **STUB
   that throws** (`quarto-api/src/system/index.ts:124,305-307`).
2. **`quarto.system.execProcess(...)` with positional args 3–6** (`mergeOutput`,
   `stderrFilter`) — **knitr** passes `"stdout>stderr"` + an output filter
   (rmd.ts:440-458); jupyter passes none. Julia never calls execProcess at all. The q2
   `SystemNamespace.execProcess` **drops params 3–6 entirely** (`system/index.ts:97-100`).
3. **`quarto.text.postProcessRestorePreservedHtml(options)`** — **knitr & jupyter**
   call it in `postprocess` (rmd.ts:341; jupyter.ts:627). Julia's `postprocess` is a
   no-op. In q2 it is **DEFERRED / not ported** (`text/index.ts:9-11`).
4. **`quarto.path.{runtime,resource,dataDir}`** and **`quarto.console.withSpinner`**,
   **`quarto.markdownRegex.getLanguages`**, **`quarto.format.is*`** breadth, and the
   PROVIDED hooks **`filterFormat`/`run`/`postRender`/`intermediateFiles`** — all used
   by jupyter and/or knitr, **none by Julia**. `path.{runtime,resource,dataDir}` are
   q2 **STUBs that throw** (`path/index.ts:167-176`); marimo *also* needs none of these,
   so they're a jupyter/knitr-only risk.

The single biggest catch: **marimo is the only engine that calls
`quarto.system.pandoc`**, and that method is a throwing stub in q2 — a q2 marimo
port would fail at runtime on PDF/LaTeX output. The whole **`quarto.jupyter`
namespace** (consumed by jupyter *and* julia) is implemented as **types-only in
q2 with no runtime `quarto-api/src/jupyter/` directory at all** — but since Julia
*does* use it, the plan likely tracked it; the subtler jupyter-only methods
(`widgetDependencyIncludes`, `notebookFiltered`, `quartoMdToJupyter`, the
`capabilities*` family) are the at-risk slice.

---

## Part A — Engine-PROVIDED surface

Which members of `ExecutionEngineDiscovery` (`src/execute/types.ts:35-87`) and
`ExecutionEngineInstance` (`types.ts:93-136`) each engine implements. ✓ = present
with a `file:line`; ✗ = absent. Engines: **K**=knitr (`rmd.ts`), **J**=jupyter
(`jupyter.ts`), **M**=markdown (`markdown.ts`), **Ju**=julia
(`julia-engine.ts`), **Ma**=marimo (`marimo-engine.ts`).

### A.1 `ExecutionEngineDiscovery` members

| Member | K | J | M | Ju | Ma | Note |
|---|---|---|---|---|---|---|
| `init?` | ✓ rmd.ts:53 | ✓ jupyter.ts:78 | ✓ markdown.ts:31 | ✓ julia:76 | ✓ marimo:199 | All store `quarto = quartoAPI`. Universal. |
| `name` | ✓ rmd.ts:58 | ✓ jupyter.ts:82 | ✓ markdown.ts:35 | ✓ julia:80 | ✓ marimo:203 | string literal. Universal. |
| `defaultExt` | ✓ rmd.ts:60 | ✓ jupyter.ts:83 | ✓ markdown.ts:36 | ✓ julia:82 | ✓ marimo:205 | All `".qmd"`. Universal. |
| `defaultYaml` | ✓ rmd.ts:62 | ✓ jupyter.ts:84 | ✓ markdown.ts:37 | ✓ julia:84 | ✓ marimo:207 | `(kernel?)=>string[]`. jupyter/marimo non-empty. |
| `defaultContent` | ✓ rmd.ts:64 | ✓ jupyter.ts:87 | ✓ markdown.ts:38 | ✓ julia:86 | ✓ marimo:209 | starter cell snippet. |
| `validExtensions` | ✓ rmd.ts:70 | ✓ jupyter.ts:104 | ✓ markdown.ts:39 | ✓ julia:92 (`[]`) | ✓ marimo:217 | jupyter reads `quarto.jupyter.notebookExtensions` (jupyter.ts:105). |
| `claimsFile` | ✓ rmd.ts:72 | ✓ jupyter.ts:109 | ✓ markdown.ts:40 | ✓ julia:94 | ✓ marimo:219 (`false`) | julia/jupyter delegate to `quarto.jupyter.isPercentScript`. |
| `claimsLanguage` | ✓ rmd.ts:77 | ✓ jupyter.ts:113 | ✓ markdown.ts:43 (`false`) | ✓ julia:98 | ✓ marimo:223 | **marimo is the ONLY engine returning a numeric score** (`2`/`1`, marimo:225-231), exercising the higher-score-wins tiebreak. |
| `canFreeze` | ✓ rmd.ts:81 (true) | ✓ jupyter.ts:118 (true) | ✓ markdown.ts:46 (false) | ✓ julia:102 (true) | ✓ marimo:235 (false) | boolean property. |
| `generatesFigures` | ✓ rmd.ts:83 (true) | ✓ jupyter.ts:119 (true) | ✓ markdown.ts:47 (false) | ✓ julia:104 (true) | ✓ marimo:237 (true) | boolean property. |
| `ignoreDirs?` | ✓ rmd.ts:85 | ✓ jupyter.ts:120 | ✗ | ✓ julia:106 (`[]`) | ✗ | knitr `["renv",…]`, jupyter `["venv","env"]`. |
| `quartoRequired?` | ✗ | ✗ | ✗ | ✗ | ✗ | **No engine sets it** → safe-to-defer evidence. |
| `populateCommand?` | ✗ | ✗ | ✗ | ✓ julia:113 | ✗ | **Julia-only.** Adds `status`/`kill`/`log`/`close`/`stop` subcommands (julia:976-1011). |
| `checkInstallation?` | ✓ rmd.ts:89 | ✓ jupyter.ts:124 | ✗ | ✓ julia:119 | ✓ marimo:239 | knitr/jupyter do real `quarto.system.checkRender`; julia/marimo just `withSpinner`+`delay` (stubs). |
| `launch` | ✓ rmd.ts:225 | ✓ jupyter.ts:249 | ✓ markdown.ts:52 | ✓ julia:132 | ✓ marimo:248 | sync factory → instance literal. Universal. |

### A.2 `ExecutionEngineInstance` members

| Member | K | J | M | Ju | Ma | Note |
|---|---|---|---|---|---|---|
| `name` | ✓ rmd.ts:228 | ✓ jupyter.ts:251 | ✓ markdown.ts:54 | ✓ julia:134 | ✓ marimo:250 | mirrors discovery name. |
| `canFreeze` | ✓ rmd.ts:229 | ✓ jupyter.ts:252 | ✓ markdown.ts:55 | ✓ julia:135 | ✓ marimo:251 | mirrors discovery. |
| `markdownForFile` | ✓ rmd.ts:232 | ✓ jupyter.ts:254 | ✓ markdown.ts:57 | ✓ julia:163 | ✓ marimo:253 | `(file)=>Promise<MappedString>`. K/J/Ju branch on spin/percent/notebook; M/Ma just `fromFile`. |
| `target` | ✓ rmd.ts:240 | ✓ jupyter.ts:274 | ✓ markdown.ts:61 | ✓ julia:298 | ✓ marimo:257 | builds `ExecutionTarget`. **jupyter alone writes a transient `.quarto_ipynb` + stores `data:{transient,kernelspec}`** (jupyter.ts:337-346). |
| `partitionedMarkdown` | ✓ rmd.ts:267 | ✓ jupyter.ts:360 | ✓ markdown.ts:72 | ✓ julia:137 | ✓ marimo:272 | `(file,format?)`. jupyter is the only one to use the `format?` 2nd arg (jupyter.ts:360-363). |
| `filterFormat?` | ✗ | ✓ jupyter.ts:374 | ✗ | ✗ | ✗ | **jupyter-only.** Forces execute policy for `.ipynb` + shiny keep-hidden (jupyter.ts:374-436). |
| `execute` | ✓ rmd.ts:277 | ✓ jupyter.ts:438 | ✓ markdown.ts:78 | ✓ julia:175 | ✓ marimo:278 | the core method. Returns `ExecuteResult`. Universal. |
| `executeTargetSkipped?` | ✗ | ✓ jupyter.ts:603 | ✗ | ✓ julia:144 (`false`) | ✗ | jupyter cleans transient nb (jupyter.ts:604); julia's is a no-op stub. |
| `dependencies` | ✓ rmd.ts:329 | ✓ jupyter.ts:607 | ✓ markdown.ts:100 | ✓ julia:147 | ✓ marimo:389 | **Required** (non-optional). knitr/jupyter do real work; M/Ju/Ma return `{includes:{}}`. |
| `postprocess` | ✓ rmd.ts:339 | ✓ jupyter.ts:626 | ✓ markdown.ts:106 | ✓ julia:155 | ✓ marimo:395 | **Required.** K/J call `quarto.text.postProcessRestorePreservedHtml`; M/Ju/Ma no-op. |
| `canKeepSource?` | ✗ | ✓ jupyter.ts:631 | ✗ | ✓ julia:159 (true) | ✗ | jupyter: `!isJupyterNotebook(target.source)` (jupyter.ts:632); julia always true. |
| `intermediateFiles?` | ✗ | ✓ jupyter.ts:635 | ✗ | ✗ | ✗ | **jupyter-only.** Reports `.ipynb`/source intermediates (jupyter.ts:635-651). |
| `run?` | ✓ rmd.ts:378 | ✓ jupyter.ts:653 | ✗ | ✗ | ✗ | **knitr & jupyter only** (shiny/serve). Julia & marimo do NOT implement it. |
| `postRender?` | ✗ | ✓ jupyter.ts:726 | ✗ | ✗ | ✗ | **jupyter-only.** Amends `app.py` for `server: shiny` (jupyter.ts:726-767). |

### A.3 Call-outs

**(a) Members NO engine implements (safe-to-defer evidence):**
- `quartoRequired?` — declared (`types.ts:65`) but **set by none** of the five.

**(b) Members ONLY Julia implements:**
- `populateCommand?` (julia:113) — the only engine adding CLI subcommands.
  (`executeTargetSkipped?` and `canKeepSource?` are also implemented by julia, but
  jupyter implements them too, so they are not Julia-exclusive.)

**(c) Members Julia does NOT implement but others DO — the AT-RISK set:**
- `filterFormat?` — jupyter (jupyter.ts:374).
- `intermediateFiles?` — jupyter (jupyter.ts:635).
- `run?` — knitr (rmd.ts:378) **and** jupyter (jupyter.ts:653).
- `postRender?` — jupyter (jupyter.ts:726).
- `executeTargetSkipped?` *with real cleanup* — jupyter (jupyter.ts:603); julia
  stubs it to `false`.
- `ignoreDirs?` *non-empty* — knitr/jupyter; julia returns `[]`.
- `checkInstallation?` *with a real render* — knitr/jupyter call
  `quarto.system.checkRender`; julia/marimo only spin+delay.

---

## Part B — Engine-CONSUMED surface (`quarto.<ns>.<method>` calls, with arguments)

Every `quarto.<ns>.<method>` call found in any engine, with the **full argument
list at each call site** quoted verbatim. "Julia?" flags whether Julia is among
the callers (the lens).

### B.1 `markdownRegex`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `extractYaml` | K, J, M, Ju, Ma | **yes** | `quarto.markdownRegex.extractYaml(resolvedMarkdown.value)` (rmd.ts:252); `quarto.markdownRegex.extractYaml(markdown!.value)` (jupyter.ts:308); `…extractYaml(md.value)` (markdown.ts:67); `…extractYaml(markdown.value)` (julia:310); `…extractYaml(md.value)` (marimo:263). Also `extractYaml(markdown)?.jupyter` (jupyter.ts:777). |
| `partition` | K, J, M, Ju, Ma | **yes** | `quarto.markdownRegex.partition(await markdownFromKnitrSpinScript(file))` (rmd.ts:269); `quarto.markdownRegex.partition(Deno.readTextFileSync(file))` (rmd.ts:273, markdown.ts:74, julia:139, marimo:274); `partition(await quarto.jupyter.markdownFromNotebookFile(file, format))` (jupyter.ts:362); `partition(quarto.jupyter.percentScriptToMarkdown(file))` (jupyter.ts:366); `partition(Deno.readTextFileSync(file))` (jupyter.ts:370). |
| `getLanguages` | M | no | `quarto.markdownRegex.getLanguages(markdown)` (markdown.ts:84) — guards "no exec cells in .md". |
| `breakQuartoMd` | J, Ma | no | `quarto.markdownRegex.breakQuartoMd(target.markdown)` (jupyter.ts:841 — for daemon-disable shell-magic scan; marimo:321 — to map cells to outputs). **One arg each; neither passes `validate`/`lenient`/`startCodeCellRegex`.** |
| `getLanguagesWithClasses` | — | no | **Not called by any engine** (it's a host-side claim helper). |

### B.2 `mappedString`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `fromFile` | K, J, M, Ju, Ma | **yes** | `quarto.mappedString.fromFile(file)` (rmd.ts:237; jupyter.ts:270; markdown.ts:58,62; julia:171,304; marimo:254,262). |
| `fromString` | J, Ju, Ma‑adjacent | **yes** | `quarto.mappedString.fromString(quarto.jupyter.markdownFromNotebookJSON(nb))` (jupyter.ts:259); `…fromString(quarto.jupyter.percentScriptToMarkdown(file))` (jupyter.ts:265; julia:166-168). |
| `splitLines` | Ju | **yes (Julia-only)** | `quarto.mappedString.splitLines(markdown)` (julia:638) — in `buildSourceRanges`. **No other engine calls it.** |
| `indexToLineCol` | Ju | **yes (Julia-only)** | `quarto.mappedString.indexToLineCol(originalString, mapResult.index)` (julia:647). |
| `normalizeNewlines` | — | no | Not called by any engine. |

### B.3 `jupyter` (the big namespace — jupyter + julia)

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `isPercentScript` | J, Ju | **yes** | `quarto.jupyter.isPercentScript(file, [".jl"])` (julia:95,164); `quarto.jupyter.isPercentScript(file)` (jupyter.ts:111,263,305,365,443). **Julia passes the `extensions` arg `[".jl"]`; jupyter omits it.** |
| `isJupyterNotebook` | J | no | `quarto.jupyter.isJupyterNotebook(file)` (jupyter.ts:255,285,347,361,398,456,501,632,639); `…(target.source)` (jupyter.ts:442,632). |
| `notebookExtensions` (prop) | J | no | `...quarto.jupyter.notebookExtensions` (jupyter.ts:105,110). |
| `percentScriptToMarkdown` | J, Ju | **yes** | `quarto.jupyter.percentScriptToMarkdown(file)` (jupyter.ts:265,366; julia:167). |
| `markdownFromNotebookJSON` | J | no | `quarto.jupyter.markdownFromNotebookJSON(nb)` (jupyter.ts:260). |
| `markdownFromNotebookFile` | J | no | `await quarto.jupyter.markdownFromNotebookFile(file, format)` (jupyter.ts:363) — **passes `format`**. |
| `fromJSON` | J | no | `quarto.jupyter.fromJSON(nbJSON)` (jupyter.ts:257); `…fromJSON(nbContents)` (jupyter.ts:506); `…fromJSON(Deno.readTextFileSync(target.source))` (jupyter.ts:783). |
| `quartoMdToJupyter` | J | no | `await quarto.jupyter.quartoMdToJupyter(target.markdown.value, true, project)` (jupyter.ts:815) — **3 args incl. `includeIds:true` + `project`.** |
| `notebookFiltered` | J | no | `await quarto.jupyter.notebookFiltered(options.target.input, isJupyterNotebook(...) ? (options.format.execute[kIpynbFilters] as string[] \|\| []) : [])` (jupyter.ts:499-504). |
| `assets` | J, Ju | **yes** | `quarto.jupyter.assets(options.target.input, options.format.pandoc.to)` (jupyter.ts:511; julia:218-221). |
| `toMarkdown` | J, Ju | **yes** | `await quarto.jupyter.toMarkdown(nb, { executeOptions, language, assets, execute, keepHidden, toHtml, toLatex, toMarkdown, toIpynb, toPresentation, figFormat, figDpi, figPos, preserveCellMetadata, preserveCodeCellYaml })` (jupyter.ts:529-551). **Julia passes the SAME option object but omits `preserveCellMetadata`** (julia:228-250). The full option set is the consumption contract. |
| `resultIncludes` | J, Ju | **yes** | `quarto.jupyter.resultIncludes(options.tempDir, result.dependencies)` (jupyter.ts:557; julia:256-259). |
| `resultEngineDependencies` | J, Ju | **yes** | `quarto.jupyter.resultEngineDependencies(result.dependencies)` (jupyter.ts:562; julia:261-263). |
| `widgetDependencyIncludes` | J | no | `quarto.jupyter.widgetDependencyIncludes(options.dependencies as JupyterWidgetDependencies[], options.tempDir)` (jupyter.ts:610-613). **jupyter-only.** |
| `pythonExec` | J | no | `await quarto.jupyter.pythonExec(kernelspec)` (jupyter.ts:486; jupyter-kernel.ts:180); `await quarto.jupyter.pythonExec()` (jupyter.ts:687). |
| `capabilities` | J | no | `await quarto.jupyter.capabilities()` (jupyter.ts:181,187,667); `…capabilities(kernelspec)` (jupyter-kernel.ts:224). |
| `capabilitiesMessage` | J | no | `await quarto.jupyter.capabilitiesMessage(caps, kIndent)` (jupyter.ts:197); `quarto.jupyter.capabilitiesMessage(caps, "  ")` (jupyter-kernel.ts:227,234). |
| `capabilitiesJson` | J | no | `await quarto.jupyter.capabilitiesJson(caps)` (jupyter.ts:193). |
| `installationMessage` | J | no | `quarto.jupyter.installationMessage(caps, kIndent)` (jupyter.ts:221); `…(caps)` (jupyter-kernel.ts:229). |
| `unactivatedEnvMessage` | J | no | `quarto.jupyter.unactivatedEnvMessage(caps, kIndent)` (jupyter.ts:229); `…(caps)` (jupyter-kernel.ts:253). |
| `pythonInstallationMessage` | J | no | `quarto.jupyter.pythonInstallationMessage(kIndent)` (jupyter.ts:240); `…()` (jupyter-kernel.ts:236). |
| `kernelspecForLanguage` | J | no | `await quarto.jupyter.kernelspecForLanguage("python")` (jupyter.ts:201). |
| `kernelspecFromMarkdown` | J | no | `await quarto.jupyter.kernelspecFromMarkdown(markdown)` (jupyter.ts:779) — **1 arg; omits `project`.** |

### B.4 `format`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `isHtmlCompatible` | J, Ju | **yes** | `quarto.format.isHtmlCompatible(options.format)` (jupyter.ts:537; julia:236). |
| `isLatexOutput` | J, Ju | **yes** | `quarto.format.isLatexOutput(options.format.pandoc)` (jupyter.ts:538; julia:237). |
| `isMarkdownOutput` | J, Ju | **yes** | `quarto.format.isMarkdownOutput(options.format)` (jupyter.ts:539; julia:238). |
| `isIpynbOutput` | J, Ju | **yes** | `quarto.format.isIpynbOutput(options.format.pandoc)` (jupyter.ts:540; julia:239). |
| `isPresentationOutput` | J, Ju | **yes** | `quarto.format.isPresentationOutput(options.format.pandoc)` (jupyter.ts:541; julia:240). |
| `isServerShiny` | K, J | no | `quarto.format.isServerShiny(options.format)` (rmd.ts:347); `…isServerShiny(file.format)` (jupyter.ts:728). |
| `isServerShinyPython` | J | no | `quarto.format.isServerShinyPython(format, kJupyterEngine)` (jupyter.ts:382). |
| `isHtmlDashboardOutput` | J | no | `quarto.format.isHtmlDashboardOutput(options.format.identifier[kBaseFormat])` (jupyter.ts:520-522). |

### B.5 `path`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `absolute` | J, Ju | **yes** | `quarto.path.absolute(options.target.input)` (jupyter.ts:469; julia:190); `quarto.path.absolute(file)` (julia:1101); `quarto.path.absolute(target)` (jupyter-kernel.ts:322). |
| `resource` | K, J | no | `quarto.path.resource("rmd/rmd.R")` (rmd.ts:445); `quarto.path.resource("jupyter", "jupyter.py")` (jupyter-kernel.ts:186). **Julia uses none — it bundles its own scripts via `import.meta.url`** (julia:46). |
| `runtime` | J, Ju | **yes** | `quarto.path.runtime("julia")` (julia:345,453,946); `quarto.path.runtime("jt")` (jupyter-kernel.ts:308). |
| `dirAndStem` | J | no | `quarto.path.dirAndStem(file)` (jupyter.ts:313,637,729); `…(options.input)` (jupyter.ts:684); `…(file.input)` (jupyter.ts:729). |
| `isQmdFile` | J | no | `quarto.path.isQmdFile(file)` (jupyter.ts:311); `…isQmdFile(options.target.source)` (jupyter.ts:442,523). |
| `toForwardSlashes` | Ju | **yes (Julia-only)** | `quarto.path.toForwardSlashes(juliaProject)` (julia:347). |
| `inputFilesDir` | J | no | `quarto.path.inputFilesDir(file.input)` (jupyter.ts:730,739). |
| `dataDir` | J | no | `quarto.path.dataDir("logs")` (jupyter-kernel.ts:329). |

### B.6 `system`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `isInteractiveSession` | J, Ju | **yes** | `quarto.system.isInteractiveSession()` (jupyter.ts:480; julia:182). |
| `runningInCI` | J, Ju | **yes** | `quarto.system.runningInCI()` (jupyter.ts:481; julia:183). |
| `execProcess` | K, J | no | **knitr (4 positional args):** `quarto.system.execProcess({cmd: await rBinaryPath("Rscript"), args:[…], cwd, stderr: quiet?"piped":"inherit"}, input, "stdout>stderr", (output)=>{…colors.red(output)})` (rmd.ts:440-458). **jupyter (2 args):** `quarto.system.execProcess({cmd: cmd[0], args:[…], env:{MPLBACKEND…}, stdout:"piped"}, kernelCommand(command,"",options))` (jupyter-kernel.ts:181-202). **No caller uses `respectStreams` or `timeout`; only knitr uses `mergeOutput` + `stderrFilter`.** |
| `pandoc` | Ma | no | `await quarto.system.pandoc(["-f","html","-t","markdown"], html)` (marimo-engine.ts:129-132). **marimo-ONLY.** |
| `checkRender` | K, J | no | `await quarto.system.checkRender({content:`…`, language:"r", services: conf.services})` (rmd.ts:111-125); `…checkRender({content:`…`, language:"python", services: conf.services})` (jupyter.ts:146-160). |
| `runExternalPreviewServer` | J | no | `quarto.system.runExternalPreviewServer({cmd, readyPattern, cwd: dirname(options.input)})` (jupyter.ts:705-709). |
| `onCleanup` | J | no | `quarto.system.onCleanup(async () => { await server.stop(); })` (jupyter.ts:713-715). |
| `tempContext` | K | no | `quarto.system.tempContext().createDir()` (rmd.ts:589). |

> **Seam-bypass note (reviewer-added).** marimo runs its *own* subprocesses with
> **raw `new Deno.Command(...)`** in its `executePython` helper
> (marimo-engine.ts:46-66), not through `quarto.system.execProcess` — it only uses
> the API for the `pandoc` convenience (B.6). So a real extension author routes around
> the `PlatformHost` seam when the API doesn't fit. This is out of q2's control today
> (native Deno path), but it matters for the future `@quarto/engine-host-wasm`: a
> marimo-style engine calling `Deno.Command` directly cannot run under a VFS/worker
> host. Evidence that the seam is necessary but not sufficient — engines need an
> ergonomic enough `system.execProcess` (B.6 / finding 2a-1) that they don't bypass it.

### B.7 `text`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `postProcessRestorePreservedHtml` | K, J | no | `quarto.text.postProcessRestorePreservedHtml(options)` (rmd.ts:341; jupyter.ts:627). |
| `lineColToIndex` | K | no | `quarto.text.lineColToIndex(options.target.markdown.value)` (rmd.ts:301). |
| `executeInlineCodeHandler` | K | no | `quarto.text.executeInlineCodeHandler("r", (expr)=>`…`)` (rmd.ts:564-566). |
| `lines` / `trimEmptyLines` / `asYamlText` | — | no | Not called by any engine. |

### B.8 `console`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `info` | J, Ju, Ma | **yes** | `quarto.console.info(`${firstPart}${sigLine}`)` (julia:738); `…info("Starting julia control server process…")` (julia:339); many more in julia (560-565,571,948-957,1016,…); `quarto.console.info(`Subprocess stderr: ${stderr}`)` (marimo:79); `quarto.console.info(`Running: ${command} ${args.join(" ")}`)` (marimo:314). jupyter uses bare `info()` from log.ts, not `quarto.console.info`. |
| `error` | Ju, Ma | **yes** | `quarto.console.error("Execution of notebook returned undefined")` (julia:204); `…error("Could not create julia runtime directory.")` (julia:948); `quarto.console.error(`Error executing marimo: ${error}`)` (marimo:377). |
| `warning` | Ma | no | `quarto.console.warning(`Pandoc conversion failed: ${result.stderr}`)` (marimo:134); `…warning(`Marimo cell ${marimoIndex} has no corresponding output`)` (marimo:335); `…warning(`Expected ${…} marimo cells…`)` (marimo:349). |
| `withSpinner` | K, J, Ju, Ma | **yes** | knitr: `quarto.console.withSpinner({message: kMessage, doneMessage:false}, knitrCb)` (rmd.ts:153-157) + (rmd.ts:171-177); jupyter: (jupyter.ts:183-189,206-212); julia: `quarto.console.withSpinner({message:"Checking Julia installation..."}, async()=>{await delay(3000);})` (julia:120-126); marimo: (marimo:240-245). |
| `completeMessage` | K, J | no | `quarto.console.completeMessage(message)` (rmd.ts:95; jupyter.ts:130). |

> Note: `quarto.console.info(msg, {bold:true})` is used by Julia's `trace`
> (julia:972) — the `LogMessageOptions` 2nd arg *is* exercised by a non-jupyter
> engine.

### B.9 `crypto`

| method | engines | Julia? | call sites (verbatim args) |
|---|---|---|---|
| `md5Hash` | J | no | `quarto.crypto.md5Hash(targetFile).slice(0, 20)` (jupyter-kernel.ts:323). **jupyter-only.** Julia uses WebCrypto `crypto.subtle` directly (julia:814-826), NOT `quarto.crypto`. |

---

## Part C — The Julia-bias ledger (the lens, made explicit)

Every API method / interface member / notable parameter **used by a non-Julia
engine but NOT by Julia**, ranked by how load-bearing (multi-engine first, then
single-engine, then param-level).

### C.1 PROVIDED interface members (not implemented by Julia)

| rank | member | engine(s) | usage |
|---|---|---|---|
| 1 | `run?` | knitr **and** jupyter | shiny/serve loop (rmd.ts:378; jupyter.ts:653). 2 engines. |
| 2 | `filterFormat?` | jupyter | execute-policy + shiny keep-hidden override (jupyter.ts:374-436). |
| 3 | `intermediateFiles?` | jupyter | report `.ipynb`/source intermediates (jupyter.ts:635-651). |
| 4 | `postRender?` | jupyter | amend `app.py` static assets for shiny (jupyter.ts:726-767). |
| 5 | `ignoreDirs?` (non-empty) | knitr, jupyter | `["renv",…]` / `["venv","env"]` (rmd.ts:85; jupyter.ts:120). |
| 6 | `checkInstallation?` (real render) | knitr, jupyter | `quarto.system.checkRender` (rmd.ts:111; jupyter.ts:146). |

### C.2 CONSUMED API methods (not called by Julia)

| rank | method | engine(s) | why load-bearing |
|---|---|---|---|
| 1 | `text.postProcessRestorePreservedHtml` | knitr, jupyter | the `postprocess` hook's *only* real work in both built-ins (rmd.ts:341; jupyter.ts:627). 2 engines. |
| 2 | `system.execProcess` | knitr, jupyter | both built-ins spawn subprocesses (rmd.ts:440; jupyter-kernel.ts:181). 2 engines. |
| 3 | `format.isServerShiny` | knitr, jupyter | shiny gating (rmd.ts:347; jupyter.ts:728). 2 engines. |
| 4 | `path.resource` | knitr, jupyter | locate bundled `rmd.R` / `jupyter.py` (rmd.ts:445; jupyter-kernel.ts:186). 2 engines. |
| 5 | **`system.pandoc`** | **marimo** | **the only `system.pandoc` caller anywhere** — HTML→markdown for PDF/LaTeX (marimo:129). |
| 6 | `console.warning` | marimo | the only `quarto.console.warning` caller (marimo:134,335,349). |
| 7 | `markdownRegex.breakQuartoMd` | jupyter, marimo | cell-walk (jupyter.ts:841; marimo:321). 2 engines, Julia not among them. |
| 8 | `markdownRegex.getLanguages` | markdown | `.md` exec-cell guard (markdown.ts:84). |
| 9 | `crypto.md5Hash` | jupyter | transport-file hashing (jupyter-kernel.ts:323). |
| 10 | `system.checkRender` | knitr, jupyter | `quarto check` render (rmd.ts:111; jupyter.ts:146). 2 engines. |
| 11 | `system.runExternalPreviewServer` + `system.onCleanup` | jupyter | shiny serve (jupyter.ts:705,713). |
| 12 | `system.tempContext` | knitr | spin temp dir (rmd.ts:589). |
| 13 | `text.lineColToIndex`, `text.executeInlineCodeHandler` | knitr | error-line remap + inline `r` exec (rmd.ts:301,564). |
| 14 | the entire `jupyter.capabilities*` / `*Message` family, `kernelspecForLanguage`, `kernelspecFromMarkdown`, `pythonExec`, `widgetDependencyIncludes`, `notebookFiltered`, `quartoMdToJupyter`, `fromJSON`, `markdownFromNotebook*`, `isJupyterNotebook`, `notebookExtensions` | jupyter | jupyter's install-check + notebook plumbing. Julia uses only the *output* slice of `jupyter` (`toMarkdown`, `assets`, `resultIncludes`, `resultEngineDependencies`, `isPercentScript`, `percentScriptToMarkdown`). |

### C.3 Notable PARAMETERS used by a non-Julia engine but not by Julia

| param | method | engine | site |
|---|---|---|---|
| 3rd arg `mergeOutput = "stdout>stderr"` | `system.execProcess` | knitr | rmd.ts:451 |
| 4th arg `stderrFilter` (output→colored) | `system.execProcess` | knitr | rmd.ts:452-457 |
| `format?` 2nd arg | `jupyter.markdownFromNotebookFile` | jupyter | jupyter.ts:363 |
| `format?` 2nd arg | instance `partitionedMarkdown` | jupyter | jupyter.ts:360 |
| `project` 3rd arg + `includeIds:true` | `jupyter.quartoMdToJupyter` | jupyter | jupyter.ts:815 |
| `preserveCellMetadata` option | `jupyter.toMarkdown` | jupyter | jupyter.ts:547 (Julia OMITS it, julia:228-250) |
| numeric **score** return (`2`/`1`) | `claimsLanguage` | marimo | marimo:225-231 (Julia returns plain `boolean`) |
| `firstClass` 2nd arg | `claimsLanguage` | marimo | marimo:223 (Julia ignores it) |

> **NOTE — params NO engine uses** (the safe-to-defer params of otherwise-needed
> methods): `system.execProcess`'s `respectStreams` (5th) and `timeout` (6th) are
> declared (`types.ts:170-171`) but **passed by nobody**.

---

## Part D — Gap check against the landed q2 surface

Cross-referencing the union (Parts B/C) against q2's actual build. **q2 status
key:** *real* = ported with working logic; *stub* = present but throws/no-ops;
*deferred* = explicitly not ported (commented); *absent* = no runtime
implementation; *types-only* = declared in `quarto-types` but no
`quarto-api/src` runtime. No severity rating — just the mapping; the reviewer
calibrates.

### D.1 The standout gaps (used by a non-Julia engine, missing/stubbed in q2)

| union member | used by | q2 status | q2 file:line |
|---|---|---|---|
| `system.pandoc` | **marimo** | **STUB (throws "requires launch context")** | `quarto-api/src/system/index.ts:124,305-307` |
| `text.postProcessRestorePreservedHtml` | knitr, jupyter | **DEFERRED (not ported — does file I/O)** | `quarto-api/src/text/index.ts:9-11` (absent from exports) |
| `system.execProcess` params 3-6 (`mergeOutput`, `stderrFilter`, `respectStreams`, `timeout`) | knitr (3,4) | **DROPPED — q2 signature is `(options, stdin?)` only** | `quarto-api/src/system/index.ts:97-100` |
| `system.checkRender` | knitr, jupyter | **STUB (throws "not yet implemented (Plan 2)")** | `quarto-api/src/system/index.ts:130,311-313` |
| `system.runExternalPreviewServer` | jupyter | **STUB (throws "not yet implemented (Plan 2)")** | `quarto-api/src/system/index.ts:136,317-319` |
| `path.runtime` | jupyter **and julia** | **STUB (throws "requires launch context")** | `quarto-api/src/path/index.ts:167-169` |
| `path.resource` | knitr, jupyter | **STUB (throws)** | `quarto-api/src/path/index.ts:171-173` |
| `path.dataDir` | jupyter | **STUB (throws)** | `quarto-api/src/path/index.ts:175-177` |
| entire `jupyter.*` namespace | jupyter **and julia** | **types-only — NO `quarto-api/src/jupyter/` runtime directory exists** | runtime: absent (`quarto-api/src/` has no `jupyter/`); types: `quarto-types/src/quarto-api.ts:151-390` |

> `path.runtime` is the one stub that also bites **Julia** (julia:345) — so it is
> presumably tracked. But jupyter needs it too (jupyter-kernel.ts:308), and
> `path.resource`/`path.dataDir`/`checkRender`/`runExternalPreviewServer` are
> **non-Julia-only** and may be under-served.

### D.2 jupyter-types signature drift — Q1's *own* published-vs-live lag, inherited by q2

**Attribution correction (verified by the reviewer 2026-06-26).** q2 did **not**
reduce these signatures. q2's `quarto-types/src/quarto-api.ts` is a **byte-faithful
vendor of Q1's published `packages/quarto-types/src/quarto-api.ts`** (confirmed
identical for every method below). The divergence is **internal to Q1**: Q1's
*published* `@quarto/types` package lags Q1's *live* internal API
(`src/core/api/types.ts`) — the surface the engines actually call at runtime. q2
inherited that lag exactly by vendoring the published package. So the rows below are
"published (= q2) vs live", not "q2 vs Q1". This is the concrete content of the
epic-prompt warning that "the `@quarto/types` jupyter signatures have already drifted
from current Q1": *current Q1* means the live `core/api`, which is ahead of the
published types q2 copied.

**Why it still matters for Plan 3.** When q2 builds the *runtime* `jupyter/`
namespace, it must implement to *some* signature. If it matches what engines actually
call (the live `core/api` shape — e.g. `pythonExec(kernelspec?)`), the runtime won't
match q2's own vendored *types* (`pythonExec(python?: string)`) → type errors at
assembly. Plan 3 (and the `@quarto/types` refinement work, "Plan 2E") must decide:
re-sync the vendored types up to Q1's live `core/api`, or implement the runtime to the
stale published shape. Note the drifted methods are all **jupyter-built-in-only** —
the standalone extensions (julia, marimo) don't call them — so nothing breaks today;
this is a forward correctness item, not a live bug.

| method | Q1 LIVE `core/api/types.ts` (engines consume) | published `@quarto/types` == q2 vendored | difference |
|---|---|---|---|
| `kernelspecFromMarkdown` | `(markdown, project?) => Promise<[JupyterKernelspec, Metadata]>` (api/types.ts:73-76) | `(markdown) => JupyterKernelspec \| undefined` (quarto-api.ts:182) | **non-async, no `project`, returns single value not tuple** |
| `markdownFromNotebookFile` | `(file, format?) => Promise<string>` (api/types.ts:85) | `(file, format?) => string` (quarto-api.ts:223) | **non-async** (jupyter `await`s it, jupyter.ts:363) |
| `markdownFromNotebookJSON` | `(nb: JupyterNotebook) => string` (api/types.ts:89) | `(nbJson: string) => string` (quarto-api.ts:231) | **param type changed** (nb object → JSON string) |
| `notebookFiltered` | `(input: string, filters) => Promise<string>` (api/types.ts:96) | `(nb: JupyterNotebook, filters) => JupyterNotebook` (quarto-api.ts:264) | **input type + return type + async all changed** |
| `widgetDependencyIncludes` | `(deps: JupyterWidgetDependencies[], tempDir) => {inHeader?, afterBody?}` (api/types.ts:98-101) | `(deps: JupyterWidgetDependencies, tempDir) => PandocIncludes` (quarto-api.ts:285) | **array→single, return shape changed** |
| `pythonExec` | `(kernelspec?: JupyterKernelspec) => Promise<string[]>` (api/types.ts:109) | `(python?: string) => Promise<string[]>` (quarto-api.ts:320) | **param type changed** (kernelspec → string) |
| `capabilities` | `(kernelspec?) => Promise<JupyterCapabilities \| undefined>` (api/types.ts:110-112) | `(python?, jupyter?) => Promise<JupyterCapabilities>` (quarto-api.ts:329) | **params + nullability changed** |
| `resultIncludes` | `(tempDir, dependencies?) => PandocIncludes` (api/types.ts:102-105) | same (quarto-api.ts:297) | OK (Julia uses this) |
| `toMarkdown`, `assets`, `resultEngineDependencies`, `isPercentScript`, `percentScriptToMarkdown` | per api/types.ts | match (quarto-api.ts) | OK (Julia uses these) |

The pattern is telling, and it explains *why* the lag went unnoticed: the
**Julia-used slice of `jupyter`** (`toMarkdown`, `assets`, `resultIncludes`,
`resultEngineDependencies`, `isPercentScript`, `percentScriptToMarkdown`) is the
slice where Q1's published types *match* its live API — so a Julia-only validation
would never surface the lag. The **jupyter-built-in-only slice** is exactly where
Q1's published `@quarto/types` (and therefore q2's faithful vendor) fell behind the
live `core/api` — async dropped, params reshaped, return types changed. Julia
exercises none of it, so the epic's Julia-only lens was blind to the divergence: this
is the Julia-bias thesis confirmed at the type level.

### D.3 What q2 DID port (real) that non-Julia engines use

For completeness — these union members are present and working in q2, so they are
*not* gaps:
- `markdownRegex.{extractYaml, partition, getLanguages, getLanguagesWithClasses, breakQuartoMd}` — all real (`quarto-api/src/markdownRegex/index.ts:114,341,404,374,764`).
- `mappedString.{fromString, fromFile, splitLines, indexToLineCol, normalizeNewlines}` — all real (`quarto-api/src/mappedString/index.ts:268,388,328,350,302`).
- `format.{isHtmlCompatible,isIpynbOutput,isLatexOutput,isMarkdownOutput,isPresentationOutput,isHtmlDashboardOutput,isServerShiny,isServerShinyPython}` — all real (`quarto-api/src/format/index.ts:63-151`).
- `path.{absolute,toForwardSlashes,dirAndStem,isQmdFile,inputFilesDir}` — real (`quarto-api/src/path/index.ts:130,42,52,71,85`).
- `system.{execProcess (2-arg form), isInteractiveSession, runningInCI, tempContext, onCleanup}` — real (`quarto-api/src/system/index.ts:178,205,210,225,297`).
- `text.{lines,trimEmptyLines,lineColToIndex,executeInlineCodeHandler,asYamlText}` — real (`quarto-api/src/text/index.ts:25,36,83,100,133`).
- `console.{info,warning,error,withSpinner,completeMessage}` — real (`quarto-api/src/console/index.ts:110`).
- `crypto.md5Hash` — present (`quarto-api/src/crypto/index.ts`).
- **All PROVIDED interface members** are present as *types* in
  `quarto-types/src/execution-engine.ts:53-247` (init…postRender all declared).

---

## Method to reproduce / what I read

| file read | extracted |
|---|---|
| `~/src/quarto-cli/src/execute/types.ts` (full) | both interface definitions + `Execute/Dependencies/PostProcess/Run` option shapes |
| `~/src/quarto-cli/src/core/api/types.ts` (full) | the nine `QuartoAPI` namespace signatures (the CONSUMED contract) |
| `~/src/q2/.worktrees/ts-engine-extensions/claude-notes/research/2026-06-25-q1-engine-lifecycle-model.md` (full) | lifecycle/ownership framing (referenced, not restated) |
| `~/src/quarto-cli/src/execute/rmd.ts` (full) | knitr PROVIDES + every `quarto.*` call w/ args (execProcess params 3-4, checkRender, withSpinner, path.resource, text.*) |
| `~/src/quarto-cli/src/execute/jupyter/jupyter.ts` (full) | jupyter PROVIDES (incl. filterFormat/intermediateFiles/run/postRender) + the full `jupyter.toMarkdown` option object + jupyter.* call sites |
| `~/src/quarto-cli/src/execute/jupyter/jupyter-kernel.ts` (full) | jupyter's `execProcess`, `pythonExec`, `crypto.md5Hash`, `path.{runtime,dataDir,absolute}`, capabilities* calls |
| `~/src/quarto-cli/src/execute/markdown.ts` (full) | markdown PROVIDES (minimal) + `getLanguages`/`extractYaml`/`fromFile`/`partition` |
| `~/src/quarto-julia-engine/src/julia-engine.ts` (full) + `constants.ts` | julia PROVIDES (incl. populateCommand) + the *exact* Julia-consumed API slice; confirmed `@quarto/types` types-only import + `init` ref |
| `~/src/quarto-marimo/src/marimo-engine.ts` (full) + `lib/cell-execution-regex.ts` | marimo PROVIDES (numeric `claimsLanguage` score) + **`system.pandoc`** + `console.warning` + `breakQuartoMd` |
| grep across both extension repos for `@quarto` | confirmed both import only from `@quarto/types` (types), reach runtime via `init`-stored `quarto` |
| `~/src/q2/.worktrees/ts-engine-extensions/ts-packages/quarto-api/src/{index,system,path,text,format,console,markdownRegex,mappedString}/index.ts` | q2 real-vs-stub-vs-deferred status; confirmed NO `jupyter/` runtime dir; `system.pandoc`/`checkRender`/`runExternalPreviewServer` + `path.{runtime,resource,dataDir}` throw; execProcess reduced to `(options, stdin?)`; postProcessRestorePreservedHtml deferred |
| `~/src/q2/.worktrees/ts-engine-extensions/ts-packages/quarto-types/src/{quarto-api,execution-engine,external-engine}.ts` | q2 declared types incl. full jupyter namespace (with the D.2 signature drift); all PROVIDED members declared |

**Coverage note:** the jupyter `src/execute/jupyter/` directory contains exactly
two files (`jupyter.ts`, `jupyter-kernel.ts`); both read in full. The marimo
`src`/`lib` dirs contain exactly the two files read. The julia `src` dir's only
TS engine sources are `julia-engine.ts` + `constants.ts`; both read in full. No
other engine source files exist in those trees.
