# Q1 vs q2 knitr/jupyter engine baseline (for reference TS engine plans 11c/11d)

Date: 2026-07-04. Compiled from five parallel research passes over
`~/src/quarto-cli` (Q1) and this worktree (q2), in preparation for the
**reference knitr and jupyter TS engine** plans of the TS Engine Extensions
epic (`2026-04-16-ts-engine-extensions-subprocess.md`).

Framing (from Gordon):
- ts-knitr and ts-jupyter ship as **independent extensions**; shared surface
  lands in `@quarto/api`, not in a shared extension layer.
- Objective is a **faithful port of Q1 behavior**; where Q1's approach can't
  work in q2's architecture, fall back to the q2 Rust engine's behavior.
- Reference implementations: not expected to be user-facing now (most users
  should use the built-in Rust engines). Purposes: (1) prove `@quarto/api`
  suffices for any engine, including built-ins; (2) provide a basis for
  possible future WASM-driven engines (keep the execution stage abstractable;
  these plans still spawn R/Jupyter via Deno natively).
- **New engines must avoid `handledLanguages`** (see FINDING #4 trap below and
  bd-8dq4pv5s).
- **Features that don't exist in q2 get deferred unless small.**

## 1. Knitr baseline

### R side: identical programs
q2 copied Q1's `src/resources/rmd/*.R` verbatim. `rmd.R`, `execute.R`,
`patch.R`, `ojs.R` are byte-identical; `hooks.R` (missing `kotlin`/`q`
comment-char entries) and `ojs_static.R` (missing `digits = NA` in one
`toJSON`) are stale copies — copy staleness, not architecture. The JSON
protocol (one `{action,params,results,wd}` request via stdin to
`Rscript rmd/rmd.R`, reply via temp results file), `rmarkdown::render()`,
knit hooks, `.QuartoInlineRender`, df-print, and preserve-chunk extraction
are all shared.

### The whole diff is in the driver
Q1 `src/execute/rmd.ts` sends five actions; q2
`crates/quarto-core/src/engine/knitr/` sends **only `execute`**.

| Action | Q1 | q2 Rust |
|---|---|---|
| execute | yes | yes (only this) |
| spin | yes | no |
| dependencies | yes | no |
| postprocess | yes | no |
| run | yes | no |

Equivalent in both: inline `` `r expr` `` wrapping, renv-aware cwd,
`QUARTO_KNITR_RSCRIPT_ARGS`, `.rmarkdown`→basename fixup, includes `[]→{}`
quirk. q2-richer: typed error parsing (`error_parser.rs`), and multi-engine
`claims_language` (r→Primary(1); python/sql/bash/sh→Interop(0) —
reticulate, a q2-only concept).

**Missing in q2 Rust knitr** (= Q1 features the TS port restores):
- `dependencies` action (knit_meta → htmlwidgets/LaTeX deps) — deserialized
  then ignored (`types.rs:115-130` TODOs)
- `postprocess` action (downlit `code-link`; preserve restore)
- **preserved-HTML chunk restore** (htmlwidgets output breaks without it)
- `run` action (shiny)
- spin `.R` files; `.Rmd`/`.rmarkdown` file claiming (`claims_file`,
  `markdown_for_file` unimplemented)
- `checkInstallation` / capability+version gate (knitr≥1.30, rmarkdown≥2.3,
  x64-R-on-ARM-Windows detection)
- format/execute options from metadata (`build_format_config` ignores
  `ctx.engine_config` and `ctx.metadata` — always defaults)
- mapped-string "Quitting from lines X–Y" remap (q2 has a regex on-error
  approximation only)

### @quarto/api coverage for a knitr port: nearly complete
Only **two stubs** block a faithful port (everything else rmd.ts calls is
real): `text.postProcessRestorePreservedHtml` (`text/index.ts:159`) and
`system.checkRender` (`system/index.ts:277`). `system.execProcess` already
has the Q1 6-param form (mergeOutput/stderrFilter — RTQ B1). R/knitr
capability probing is engine-owned (the extension bundles it, like julia
bundles its `.jl` scripts).

### Hard-to-port (knitr)
1. `run()`/shiny — live stderr-watch → in-process `onReady`; q2 has no run
   lifecycle and the protocol deliberately excludes `run`. **Defer.**
2. Preserved-HTML restoration — Q1 does it as a JS string-postprocess over
   rendered HTML + an R `postprocess` action. q2 has **no post-Pandoc DOM/
   string stage**; must be re-expressed as an AST transform consuming the
   FC-1 carried `preserve`/`post_process` fields. Shared with jupyter; the
   biggest content-correctness gap.
3. Mapped-string error line remap — must happen Rust-side (q2 owns
   SourceInfo); TS engine can't reproduce Q1's in-process MappedString remap.
4. `checkInstallation` — portable in principle but needs a q2 `check`
   integration surface that doesn't exist. **Defer** (Plan 9's
   `q2 call engine` is the adjacent precedent).

## 2. Jupyter baseline

### q2 has two jupyter stories — don't conflate
1. **Native Rust `JupyterEngine`** (`engine/jupyter/`): a real ZMQ client
   (runtimelib + jupyter_protocol; no python-side scripts), in-process daemon
   pool keyed by (kernel, dir) with 300s idle timeout. But MVP-level:
   **no `.ipynb` ingestion at all** (no claims_file/markdown_for_file),
   figures emitted as literal `[Image output]` placeholders, chunk options
   ignored, fence-options `{python echo=false}` rejected, interrupt a no-op,
   `can_freeze()=true` with no freeze machinery, user_expressions stubbed.
   The `JupyterTransform`/`output.rs` conversion path is dead (tests only);
   the live path is `text_execute.rs`. Unlike knitr, **the Rust jupyter
   engine is NOT a full behavioral fallback** — for most of Q1's jupyter
   surface, Q1 is the only complete reference.
2. **TS-engine subsystem** — the target. Julia/marimo/echo already prove it.

### Q1 jupyter surface (high points)
- `validExtensions`: `.ipynb` + percent (`.py .jl .r .q`) + `.qmd`;
  `claimsFile` for notebooks/percent; `claimsLanguage` **only "julia"**.
- `markdownForFile`: notebook→markdown (markdown/raw cells only), percent →
  markdown, else raw file.
- `target()`: for `.qmd`/percent writes a **transient `.quarto_ipynb`**
  notebook (collision-suffixed) via `quartoMdToJupyter`; kernelspec resolved
  from YAML/languages (`jupyterKernelspecFromMarkdown`).
- `execute()`: recreate transient nb if missing; `ensureYamlKernelspec` can
  rewrite an `.ipynb`'s kernelspec on disk; daemon decision (`execute.daemon`;
  default interactive&&!CI; forced off for shell-magic notebooks); oneshot vs
  keepalive; `notebookFiltered` (ipynb-filters, **real `.ipynb` sources
  only**); shinylive fixup; `jupyterAssets`; `toMarkdown`; deps inline or
  deferred; cleanup transient nb unless `keep-ipynb`.
- `filterFormat`: `.ipynb` sources **not executed by default**; shiny-python
  keep-hidden forcing.
- Plus: `executeTargetSkipped` (freeze cleanup), `dependencies` (widget
  includes), `postprocess` (restorePreservedHtml), `canKeepSource`,
  `intermediateFiles`, `run` (shiny), `postRender` (shiny static assets).
  `populateCommand` NOT implemented by jupyter. No `devServerRenderOnChange`.

### Q1 kernel layer (the part q2 lacks in TS)
`jupyter-kernel.ts` + `resources/jupyter/jupyter.py` (socket-server shell) +
`notebook.py` (actual execution via **nbclient**; papermill only for param
injection; optional **jupyter-cache**):
- TCP-only transport; daemon keyed by md5(abs input)[:20] transport file in
  runtime `jt/`; `{port,secret}` JSON; detached double-spawn (`start`→`serve`);
  default 300s keepalive self-exit; 5-consecutive-error exit.
- Wire: client→daemon `{command: execute|abort|file|start|serve, secret,
  options}` (+>1024-byte spill-to-file workaround); daemon→client
  `{type: status|error|restart, data}`.
- **No fine-grained interrupt in Q1 either** — abort tears down the daemon;
  restart recursively relaunches. Restart triggers: python_cmd /
  supervisor_pid / kernelspec / input / setup-dep-hash changed, or cell
  `quarto.restart_kernel`.
- Setup/cleanup cell injection (per-language resources), figure env vars,
  inline `{lang} expr` user-expressions, cell YAML (`#|`) parsing — all in
  `notebook.py`.
- **Cache lives entirely in `notebook.py`** (jupyter-cache), invisible to TS
  except the pass-through `execute.cache` option.

### Conversion layer: largely ported already
`ts-packages/quarto-api/src/jupyter/` has real, tested modules:
`to-markdown.ts` (jupyterToMarkdown), `display-data.ts` (Q1 MIME priority),
`cell-options.ts`, `tags.ts`, `labels.ts`, `widgets.ts`, `assets.ts`,
`percent-script.ts`, `preserve.ts`, `pandoc-id.ts`, `result-helpers.ts`.
Namespace ships 7 real methods + `notebookExtensions`; **15 `NotImplemented`
throwers** deferred as "Phase 3E": isJupyterNotebook, kernelspecFromMarkdown,
kernelspecForLanguage, fromJSON, markdownFromNotebookFile/JSON,
quartoMdToJupyter, notebookFiltered, pythonExec, capabilities{,Message,Json},
installationMessage, unactivatedEnvMessage, pythonInstallationMessage.
Julia and marimo were validated **without Plan 3**, so none of the shipped
jupyter code has had a real engine consumer yet.

### Strategic read (jupyter execution layer)
The cleanest Q1-parity path: the ts-jupyter extension **bundles Q1's
`jupyter.py` + `notebook.py` as engine resources and drives them over TCP
from Deno** — exactly the detached-daemon + transport-file pattern the Julia
engine already proved under the q2 host. That inherits daemon keepalive,
restart triggers, abort, setup/cleanup cells, papermill params, inline
user-expressions, figure env plumbing, and jupyter-cache wholesale. The
alternatives (reimplement a kernel client in TS; bridge to the native Rust
ZMQ daemon via new wire verbs) both build more and port less faithfully.
The native Rust engine remains the pure-Rust/WASM-path fallback.

## 3. Cross-cutting q2 gaps both engines hit

- **preserve/postProcess consumer**: wire carries `preserve` +
  `post_process` (FC-1, inert). No q2 consumer exists; both engines call
  `postProcessRestorePreservedHtml` in Q1. Needs (a) the `@quarto/api/text`
  stub implemented and (b) a q2-core AST-transform consumer (per the "no DOM
  postprocessor" rule). **The one genuinely shared new work item.**
- **Freeze**: no freeze/thaw machinery in q2 (`use_freeze` always false;
  bd-mx5x609r). Both Q1 engines are freeze-capable. **Defer**; carry
  `canFreeze` truthfully.
- **Deferred dependencies** (`Dependencies` verb / `engineDependencies`):
  wire + TS handler exist; **no production Rust sender** (book-feature-owned).
  v1 ports use `dependencies: true` (inline) — Q1's single-doc default.
- **`handledLanguages` trap (FINDING #4)**: the wire field is the
  **leave-alone** set (built-ins ∪ other engines' owned languages), not
  "assigned to me". marimo read it backwards → silent no-execute. Q1 knitr's
  `execute.R` uses `handledLanguages` to register pass-through knit_engines;
  Q1 jupyter's `notebook.py` cedes non-owned cells similarly. Directive:
  avoid building new TS-side logic keyed on it; see design questions.
- **`claims_file` short-circuit trap**: `EngineClaimsFileStage` whole-file
  claim bypasses the per-language `claims:` map (first-claimer-wins).
  Content-inspecting `claimsFile` without `claims-files:` declarations breaks
  multi-engine coexistence (bd-8dq4pv5s adjacent). Both ports claim files
  (.Rmd/spin; .ipynb/percent) — must declare static `claims-files`.
- **Plan 4b unexecuted**: the tier-model/multi-engine validation plan (0/54
  boxes) — knitr+jupyter contention (Interop sql/python, Fallback(0), tie
  order) is exactly its territory.
- **Daemon management** (bd-m1jeqhhz / Plan 9): `q2 call engine <e>
  status/kill/log/close/stop` — Plan 9 resolves this; both Q1 engines have
  cliffy subcommands (knitr: none actually; jupyter kernel daemon is managed
  via transport files). Reference-engine daemons should stay compatible with
  Plan 9's teardown pattern.
- **ipynb-filters divergence (deliberate)**: q2's design
  (`2026-04-23-ipynb-filters-and-engine-partitioning.md`) consolidates
  filtering at `markdown_for_file`, project-level only (drops per-doc
  `ipynb-filters:`). Port should follow q2, as a motivated deviation.
- **Percent/spin and Plan 7** (`2026-06-27-plan7-native-percent-spin-sourceinfo.md`;
  unstarted, post-4 default, pullable earlier). Corrections to the raw
  diff-table reading of "missing" q2 features:
  - Built-in jupyter `.py`/`.jl` percent claiming + conversion is **Plan 7A**;
    built-in knitr `.R` spin is **Plan 7B** (possibly via the R subprocess).
    These are planned work, not open-ended gaps. (`.Rmd` claiming for built-in
    knitr and `.ipynb` ingestion for built-in jupyter remain **outside** Plan 7.)
  - **Phase 7D (A′ faithful remap)** upgrades TS-engine `markdown_for_file`
    provenance: today the wire `source_map` gives C′ (converted-buffer)
    provenance only. The reference engines' percent/spin conversions are
    functional without Plan 7 (C′ matches the julia/marimo precedent), but
    error columns point at the original `.py`/`.jl`/`.R` only after 7D.
  - **Phase 7C** adds `SourceInfo::NotebookCell` + sidecar maps for ipynb
    provenance — the machinery a future `.ipynb` error display needs. The
    per-line wire `source_map` has no NotebookCell shape today; ts-jupyter's
    `.ipynb` conversion provenance is a real design question (defer to 7C
    alignment vs. flatten to per-line).
  - Once 7A/7B land, built-ins **content-inspect-claim** `.py`/`.jl`/`.R` —
    so extension-vs-builtin file-claim contention (candidate order: extensions
    first) is no longer hypothetical; plans 11c/11d validation must pin it, and
    the 1c-review caveat applies (content-inspecting claims must be marked
    must-load, not bare `claims-files:` extension lists).
  - Q1-faithful note: ts-knitr gets spin nearly free (`callR("spin")` — an
    R-side action against the identical scripts), independent of 7B's native
    port. ts-jupyter percent uses the already-real
    `quarto.jupyter.percentScriptToMarkdown` (Plan 7's own dependency).
- **Stale R script copies** (out-of-plan digression): q2's `hooks.R` and
  `ojs_static.R` are stale vs Q1 (kotlin/q comment chars; `digits = NA`).
  Affects the built-in Rust knitr engine regardless of these plans.

## 4. Validation methodology to reuse (from Plans 4/4c)

- Fixture under `crates/quarto-core/tests/fixtures/extensions/<engine>/`
  (copy marimo/julia layout): modified `src/` + committed `dist/*.js` bundle
  from `q2 build-ts-extension`, `_extension.yml` with `claims:` +
  `claims-files:`, runtime helper scripts, sibling `_quarto.yml`.
- Known friction: `find_entry_ts` layout mismatch → throwaway symlink
  workaround (tracked: `--entry` flag).
- Test tiers: unit-ts/unit-rs/unit-py pure; int-rs static-claim resolution
  against a real registry (no subprocess); e2e real `q2 render`; e2e-pw
  Playwright preview, env-gated (`QUARTO_*_LIVE`). Frozen seam specs with
  named revert hunks proven RED (skills: prevalidating-test-seams,
  fail-on-revert). Env-gated skips when `Rscript`/`python`/`deno` absent.
- Migration-guide template: `2026-07-02-julia-engine-migration-guide.md`,
  `2026-07-03-marimo-migration-guide.md`.
- Bugs to expect (categories that recurred): execute-defaults merging, source
  map population, supporting-dir expansion, includes path-vs-content,
  qmd-writer attr round-trips, handledLanguages polarity, claims_file
  short-circuit, preview capture-splice anchoring (`::: {.cell}` only).

## 5. Recommended plan DAG (numbers finalized 2026-07-07)

Plan-number ceiling in the epic is **10** (Plan 10 = `q2 check` /
checkInstallation, in progress on `bd-4qflzhwh-...`). New work starts at **11**.

Two corrections vs. the first-pass proposal:
- **`owned_languages` already has a plan.** The positive-ownership wire field
  that lets an engine *avoid* the ambiguous `handledLanguages` leave-alone set
  is **Plan 4d** (`2026-07-06-plan4d-owned-languages.md`, specced, unstarted;
  referenced by Plan 6). It is **not** in Plan 7A (that is percent/spin
  conversion). Plan 4d is a hard prerequisite for the reference engines.
- **checkInstallation is not deferred.** Plan 10 builds the `checkInstallation`
  wire verb + streamed `checkProgress` + `check_installation` trait method +
  Deno host dispatch. A reference TS engine that implements Q1's
  `checkInstallation` gets `q2 check` support **for free** once Plan 10 lands —
  except the test-render sub-check (`system.checkRender` stays a stub for TS
  engines, Plan 10 D-5; accepted). So the reference engines *should* implement
  `checkInstallation` (minus render probe), not stub it.

### The four new plans

Labels **11a–11d** are stable identifiers ordered by dependency layering
(a/b = shared foundations, c/d = engines); the *proceed order* to surface
design questions earliest is separate — see below.

- **Plan 11a — shared preserve/postProcess consumer (small).** Implement
  `text.postProcessRestorePreservedHtml` in `@quarto/api` (pure; Q1 source
  exists) **and** the q2-core AST-transform consumer for the already-carried
  `preserve`/`post_process` wire fields (`ts_protocol.rs:424/431`,
  "carried-and-ignored until a consumer lands"). Must be an AST transform
  (no-DOM-postprocessor rule). The one genuinely shared new work item; both
  reference engines need it for htmlwidgets / HTML-preserve parity.
- **Plan 11b — `@quarto/api/jupyter` completion.** Fill the 15 `NotImplemented`
  methods Plan 3 deliberately deferred (labelled "Phase 3E" in code): notebook
  ingestion (`fromJSON`, `markdownFromNotebook*`, `isJupyterNotebook`),
  `quartoMdToJupyter`, kernelspec resolution, `pythonExec`, `notebookFiltered`,
  capability/installation messaging. A **new** plan referencing Plan 3 as the
  origin of the stubs — Plan 3 is not reopened. Jupyter-only prerequisite.
- **Plan 11c — ts-knitr reference engine.** Port `rmd.ts` onto `@quarto/api`;
  bundle refreshed `rmd/*.R` as extension resources; restore
  spin/execute/dependencies(inline)/postprocess(downlit); static `claims-files`
  for `.rmd`/`.rmarkdown` (spin `.R` via `callR("spin")`); consume
  `owned_languages` (Plan 4d), never `handledLanguages`; implement
  `checkInstallation` (Plan 10, render sub-check stubbed). Defer run()/shiny,
  freeze. Validate per §4.
- **Plan 11d — ts-jupyter reference engine.** Bundle `jupyter.py`/`notebook.py`,
  drive the daemon over TCP from Deno (julia detached-daemon pattern),
  inheriting cache / restart / setup-cleanup cells / papermill params / figure
  extraction; transient notebooks, kernelspec resolution, percent scripts,
  widgets (inline deps), MIME/display-data via the Plan-12 TS modules; consume
  `owned_languages`; `checkInstallation` via Plan 10. Defer shiny
  run/postRender, freeze; **diverge** to q2's project-level ipynb-filters.

### Dependency graph

```
Plan 4d (owned_languages, prereq — execute first) ─┐
Plan 10 (q2 check, in progress — soft dep) ────────┤
                                                    │
Plan 11a (preserve consumer) ──┬───────────────────→ Plan 11c (ts-knitr)
                              │                     │
Plan 11b (@quarto/api jupyter)─┴──(11b also)────────→ Plan 11d (ts-jupyter)
```
- 11a and 11b have no new deps; both can start immediately, in parallel.
- 11c deps: 11a + 4d (soft: 10). 11d deps: 11a + 11b + 4d (soft: 10).
- No shared *extension* layer (shared-infra survey: Q1's engines share only the
  `QuartoAPI` contract, already ported). ts-knitr and ts-jupyter ship as
  independent extensions; all shared surface is in `@quarto/api` (Plans 11a/11b).

### Proceed order (discovery-first — surface every design question soonest)

Build/merge order follows the DAG (4d → 11a/11b → 11c → 11d). But to get every
design question on the table *before* committing to any build, author the plan
skeletons and run one spike in this order — plan-authoring is itself the
question-surfacing activity, and the two highest-unknown clusters go first:

1. **Author 11c (ts-knitr) skeleton first** + **run a throwaway 11d daemon
   spike in parallel.** 11c is the faithful-port pattern-setter: writing it
   forces the *shared* cross-cutting decisions both engines inherit —
   `owned_languages` cede contract, the 11a preserve hook, checkInstallation-
   via-Plan-10, extension-vs-builtin claim contention, repo/vendor layout — and
   knitr's low execution risk means those decisions can be made with
   confidence. The 11d spike (bundle `jupyter.py`/`notebook.py`, drive from a
   trivial Deno engine, execute one `{python}` cell) answers the single biggest
   *empirical* unknown no plan-writing can settle: daemon-over-Deno viability,
   runtime-dir/transport-file access from the extension sandbox, figure paths,
   and kernelspec-discovery-from-Deno (which also answers an 11b question).
2. **Author 11d (ts-jupyter) skeleton**, informed by the spike + 11c's ratified
   patterns. Surfaces the jupyter-specific questions: `.ipynb` ingestion +
   provenance shape, figure→`supporting`/`resourceFiles` wiring, ipynb-filters
   divergence, cache-via-notebook.py.
3. **Author 11b (@quarto/api jupyter) and 11a (preserve) skeletons.** Lowest
   drama; their questions (kernelspec-from-Deno — already answered by the spike;
   preserve-transform pipeline placement) are largely settled by now.
4. **Consolidate + answer** the full question list, then **build in DAG order.**

Rationale: the risk is concentrated in 11d (novel daemon bundling) and in the
shared contracts 11c ratifies; front-loading a 11c-author + 11d-spike pair puts
~all design questions on the table in the first step, while 11a/11b (mostly
mechanical) can be specced last without hiding surprises.

### Cross-plan coordination
- **Plan 4b** (tier model, written, 0/54 executed): run before/with 11c+11d
  validation — knitr+jupyter is its canonical ≥2-engine contention case.
- **Plan 7**: 11c/11d don't depend on it (C′ provenance is the julia/marimo
  baseline); 7D later upgrades their percent/spin error columns for free; if
  7A/7B land first, add extension-vs-builtin file-claim fixtures; ts-jupyter's
  `.ipynb` provenance stays per-line C′ for v1, deferring 7C `NotebookCell`.

### Residual decisions (plan-internal; state and proceed)
- Deferral set for "faithful v1": freeze (bd-mx5x609r epic), shiny
  run/postRender, deferred-dependencies wire (use `dependencies:true` inline),
  per-doc ipynb-filters (diverge to project-level). checkInstallation is IN
  (via Plan 10). — confirm the list.
- Claim semantics: reference engine installed ⇒ wins over builtin (extensions
  first in candidate order); ts-jupyter mirrors Q1 (`claimsLanguage` only
  `julia`, rest via `claimsFile`/fallback). — confirm.
- Repo home: new standalone extension repos + committed in-tree fixtures
  (marimo pattern). — confirm.
- R/python resource vendoring + refresh-from-Q1 policy (Plan 10 already vendors
  `knitr.R`/`jupyter.py` probes; reference engines vendor `rmd/*.R` /
  `jupyter.py`+`notebook.py`). — mechanical.

WASM-future note: keep the **execution stage** behind a seam in both engines
(conversion layers are already pure/host-injected). The natural seam is
"spawn+talk to the language daemon" — exactly the piece a WASM host would
replace; don't let engine logic call `Deno.*` directly (PlatformHost only).
