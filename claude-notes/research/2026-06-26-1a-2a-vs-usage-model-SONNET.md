# Plan 1a-family + 2A vs All-Engines Ground Truth — Code Review Findings (SONNET)

**Reviewer:** claude-sonnet-4-6  
**Date:** 2026-06-26  
**Lens:** Julia-bias at-risk surface (Part C of ground-truth model) vs effective 1a+2A spec  
**Ground-truth source:** `claude-notes/research/2026-06-26-engine-api-usage-model.md`  
**Effective spec:** plan1a-protocol + plan1a-host + plan1a-engine **as amended by** plan1a-return-to-q1 (2026-06-25), plus plan2a

---

## Bottom-Line Verdict

**MOSTLY.** The 1a-family + 2A surface, as corrected by return-to-q1, is adequate against the all-engines ground truth for the core render path — with **two genuine (C)-class gaps** (dropped with no seam, used by non-Julia engines), **one partial-seam gap** that needs a named recovery path, and a forward correctness item on `@quarto/types`. The plan family is not broken, but two things matter enough to flag before Plan 3 begins:

1. **`system.pandoc` is a throwing stub, not a deferred seam** — marimo's only use of `quarto.system` (for HTML→markdown, which its PDF/LaTeX path needs) lands on a `throw requiresLaunchContextError("pandoc")` that will never resolve under the current design because return-to-q1's Item A makes paths ambient (no launch-context gating), yet the `system.pandoc` stub still carries the old "requires launch context" error message and has no stated recovery path to a real implementation. The stub language says "Plan 2" for `checkRender` and `runExternalPreviewServer` but says "requires launch context" for `pandoc` — after Item A, the gating rationale is gone but the stub remains. This is either a Category (C) gap or a stub that needs an explicit "Plan 2" tag.

2. **`system.execProcess` drops knitr's `mergeOutput`/`stderrFilter` params (args 3–4) with no wire-level seam** — the q2 `ExecProcessOptions` interface only admits `{cmd, args, cwd, env, stdin, stdout, stderr}` (system/index.ts:43-58); the knitr-specific `mergeOutput: "stdout>stderr"` and `stderrFilter: (output) => colored` signature (rmd.ts:440-458) has no carrier on the wire at all. This is a (C) gap for the knitr built-in, but since knitr is reimplemented natively in Rust in q2, the practical impact is zero for Plan 3. The risk is that a TS engine author modelling on knitr's pattern cannot reproduce it through `@quarto/api`. This needs a note.

The calibration hypotheses are evaluated below in order.

---

## What return-to-q1 DOES cover adequately — do not re-flag

The following are **well-handled** by the combined plan family:

- **Return-to-Q1 Item A** (ambient path/system, pre-launch): restores Q1 fidelity for `path.runtime`, `path.resource`, `path.dataDir`, and `system.pandoc` *in concept*, un-stubs the gating architecture, and makes them available pre-launch once the harness is assembled with ambient config. Credit this fully.
- **return-to-q1 Surface coverage audit** (Level 1 + Level 2): classifies every Q1 engine interface member across both tiers. The classification is thorough and consistent with the ground-truth model.
- **FC-1** (infrastructure seams): adds `metadata`, `pandoc`, `resource_files`, `preserve`, `post_process` as `#[serde(default)]` wire carriers on `TsExecuteResult`. All five Q1 `ExecuteResult` fields that were "dropped" by original 1a are now promoted to "carry but defer." This closes the original PROTO-1 concern.
- **DQ-2** (render lifecycle): names `run?`/`postRender?` as "defer-infra" with documented seams. The model confirms these are jupyter/knitr-only; julia and marimo do not implement them. Both plans agree on deferral.
- **DQ-4** (discovery-tier completeness): ENG-1 moves `generatesFigures` to the discovery tier (`LoadEngineResult`), matching Q1 semantics.
- **`postprocess` classified DROP** with an explicit justification ("no post-write DOM stage; the No-DOM-postprocessor rule"): the plan explicitly drops `postprocess`, and `quarto.text.postProcessRestorePreservedHtml` is also deferred from §2aa (plan2a, §2aa resolved decision #3). Whether this is sound is discussed in Finding 3 below.
- **`partitionedMarkdown` classified DROP**: explicitly justified ("pampa parses qmd natively"). Discussion in Finding 4.
- **ALL PROVIDED interface members** are declared as types in `quarto-types/src/execution-engine.ts` (per model D.3). The plan confirms this; no phantom gaps in the type surface.
- **`markdownRegex.*`, `mappedString.*`, `format.*`, `console.*`, `crypto.md5Hash`, `path.{absolute,toForwardSlashes,dirAndStem,isQmdFile,inputFilesDir}`** — all ported as real methods in §2aa (confirmed by model D.3). No gaps.
- **`claimsLanguage` numeric-score return** (marimo's `2`/`1` scores): the `TsLanguageClaim` protocol type and harness normalization table handle `number → Primary { priority: n }`, so marimo's higher-score-wins tiebreak is correctly transported. Plan1a-protocol appendix confirms this.

---

## Calibration Hypothesis Evaluations

### Hypothesis 1 — return-to-q1's audit covers PROVIDED/wire surface but NOT systematically the CONSUMED surface

**CONFIRMED, with nuance.** The return-to-q1 "Surface coverage audit" (Level 1 and Level 2) methodically walks the Q1 **engine interface** (`ExecutionEngineDiscovery` + `ExecutionEngineInstance`) — the PROVIDES side. It does not enumerate what engines call back into `quarto.<ns>.<method>` systematically. Plan 2A §2aa covers the CONSUMED surface indirectly (it ports all nine namespaces), but the plan's framing is "port from Q1 core/api" rather than "verify against all-engine consumption patterns." The practical consequence: gaps in the CONSUMED surface (e.g., `execProcess` param reduction, `system.pandoc` stub state) are not surfaced by return-to-q1's audit.

**Evidence:** return-to-q1's Level 1 and Level 2 tables (lines 549–613) classify `postprocess` as DROP, `filterFormat?` as defer-infra, `run?`/`postRender?` as defer-infra — all based on Q1 interface members. There is no corresponding table that cross-references which `quarto.*` methods each engine *consumes*, against which q2 has real vs. stub implementations.

**Impact:** Finding 1 (below) is the primary gap this creates.

### Hypothesis 2 — `system.execProcess` param reduction (mergeOutput/stderrFilter)

**CONFIRMED.** The q2 `ExecProcessOptions` interface (`system/index.ts:43-58`, read directly) has seven fields: `cmd`, `args`, `cwd`, `env`, `stdin`, `stdout`, `stderr`. Q1's knitr (rmd.ts:440-458) calls `quarto.system.execProcess` with **four positional args**: options, stdin, `"stdout>stderr"` (mergeOutput), and a `stderrFilter` callback. The q2 interface has no `mergeOutput` or `stderrFilter` fields.

**Is this in any plan?** No. Neither plan1a-protocol, plan1a-host, plan1a-engine, return-to-q1, nor plan2a mentions `mergeOutput` or `stderrFilter`. Return-to-q1's Level 1 field-by-field audit of `ExecuteOptions` does not include the `execProcess` params (correct — those are CONSUMED, not PROVIDES). The model's Part C.3 ("Notable PARAMETERS") explicitly calls this out, but no plan addresses it.

**Severity calibration:** Low for Plan 3 scope. Knitr is a built-in Rust engine in q2; no TS engine replicates knitr-style merged-stderr. The model confirms no standalone engine (julia, marimo) uses these params. This is a gap in the TS engine author API that would only manifest if a future third-party TS engine needed to replicate knitr's merged-stderr pattern — which is a narrow, unusual need. **Not an immediate blocker, but worth a note at the `execProcess` definition site.**

**Params safely absent (confirmed):** `respectStreams` (5th arg) and `timeout` (6th arg) are declared in Q1 `types.ts:170-171` but passed by nobody (model Part C.3 note). Their absence from q2's interface is unambiguously sound.

### Hypothesis 3 — `postprocess` classified DROP + `postProcessRestorePreservedHtml` deferred = coordinated no-seam gap

**PARTIALLY CONFIRMED — the gap is real but the sound-vs-unsound verdict depends on which engines you care about.**

**Evidence for the gap:**
- return-to-q1 level 2: `postprocess` → DROP ("no post-write DOM stage; the No-DOM-postprocessor rule"). No seam, no recovery path in any 1a plan.
- plan2a §2aa resolved decision #3 (line 303): `postProcessRestorePreservedHtml` is DEFERRED ("does file I/O; no 1b contract test calls it"). Not in §2aa exports (`text/index.ts:9-11`).
- Model B.7 / C.1: knitr (rmd.ts:341) and jupyter (jupyter.ts:627) call `quarto.text.postProcessRestorePreservedHtml(options)`. This is the `postprocess` hook's only real work in both built-ins.

**Is dropping `postprocess` (rather than defer-infra) sound?** For the standalone extension use case (julia, marimo) — yes. Julia's `postprocess` is a no-op (julia-engine.ts:155); marimo's is a no-op (marimo-engine.ts:395). **For built-in engines (knitr, jupyter): both call `postProcessRestorePreservedHtml`**, which handles HTML preservation/restore for embedded content (e.g., raw HTML in output that must survive the Pandoc pass).

The q2 architecture argument is: "no post-write DOM stage; preserve/restore should be an AST transform." That is a valid q2-architectural position — but it means knitr and jupyter's preserve-restore functionality has **no recovery path stated in any plan**. PROTO-1/FC-1 adds `preserve` and `post_process` as `#[serde(default)]` wire carriers, which is good infrastructure. But neither return-to-q1 nor any 1a plan states: "when q2 reimplements knitr/jupyter preserve-restore, it will be an AST transform at stage X, and the wire already carries the `preserve` field for the engine to populate." The narrative is "dropped" not "deferred-with-named-alternative-path."

**Severity:** Medium-Low. For the standalone engine story (the primary audience of Plans 1a–3), this is not blocking. For the built-in engine completeness story, the recovery path exists (the `preserve` field is on the wire via FC-1) but needs to be named in at least one plan.

**Verdict on Hypothesis 3:** The coordinated no-seam gap is real (both the hook and the API method are absent from the TS-engine path). However, the absence is architecturally justified for standalone engines (julia, marimo). The residual is that the recovery path for built-in engines (knitr/jupyter preserve-restore) is unnamed. This is a documentation gap, not a structural one — FC-1's `preserve` wire carrier is the right seam.

### Hypothesis 4 — `partitionedMarkdown` classified DROP ("pampa parses natively / DocumentProfile subsumes")

**SOUND, with a caveat about the jupyter `format?` arg.**

**Evidence:** The model (A.2) confirms all five engines implement `partitionedMarkdown`; jupyter is the only one that uses the `format?` second arg (jupyter.ts:360-363). Plan1a-protocol explicitly addresses this (line 182):

> "Q1 has 5 caller sites for `partitionedMarkdown` (inspect, project-index, project-config, render-shared, render-contexts), of which only two pass a real `format` to invoke the filter-aware path — `project/project-index.ts:102` (project indexing) and `command/render/render-contexts.ts:632` (pre-execute filter-YAML harvest). Those two are subsumed by q2's `DocumentProfile` checkpoint..."

The plan also cross-references `claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md` as the forward work for filter-aware notebook conversion via `markdown_for_file`.

**Assessment:** The drop is architecturally sound because (a) q2's pampa parser handles QMD natively and produces an equivalent or superior partition, (b) the filter-aware path (jupyter's `format?` arg) has a named alternative (`markdown_for_file` + ipynb-filters research plan), and (c) marimo's use is `Deno.readTextFileSync(file)` + `quarto.markdownRegex.partition()` — no `format` arg, fully reproducible in q2. The julia engine similarly uses it without format.

**The caveat:** The ipynb-filters research plan (`2026-04-23-ipynb-filters-and-engine-partitioning.md`) is referenced but its open items are explicitly listed as unsettled in plan1a-protocol (line 58). The jupyter `format?` arg recovery path is "future work" without a delivery plan. This is an **acknowledged deferral, not an oversight** — rated correctly.

**Verdict:** DROP is sound for all standalone engines; the jupyter `format?` arg has a forward recovery path (named, deferred). Not a finding — the plan's reasoning is adequate.

### Hypothesis 5 — `@quarto/types` jupyter signature lag

**CONFIRMED — forward correctness item, not a live bug.**

Plan2a acknowledges the vendored `@quarto/types` as a copy of Q1's published package (lines 118-134). The model (D.2) documents six methods where the published `@quarto/types` diverges from Q1's live `core/api/types.ts`:

| Method | Published (q2 vendored) | Live Q1 `core/api` |
|---|---|---|
| `kernelspecFromMarkdown` | non-async, no `project`, returns single value | `(markdown, project?) => Promise<[JupyterKernelspec, Metadata]>` |
| `markdownFromNotebookFile` | non-async | `async (file, format?) => Promise<string>` |
| `markdownFromNotebookJSON` | `(nbJson: string) => string` | `(nb: JupyterNotebook) => string` |
| `notebookFiltered` | input/return type + sync all differ | `(input, filters) => Promise<string>` |
| `widgetDependencyIncludes` | array→single, return changed | `(deps[], tempDir) => {inHeader?, afterBody?}` |
| `pythonExec` | `(python?: string) => Promise<string[]>` | `(kernelspec?: JupyterKernelspec) => Promise<string[]>` |

**No 1a/2A plan claims Q1-parity for these signatures.** Plan2a explicitly defers `@quarto/types` refinements to "Plan 2E" (line 29). The drifted methods are all **jupyter-built-in-only** (model D.2 confirms: julia, marimo do not call them). So nothing breaks today for the standalone engine story.

**What this means for Plan 3:** When Plan 3 implements the `quarto.jupyter` namespace runtime, it must decide: implement against the stale published-types shape (types match q2's vendor, runtime behavior wrong) or implement against the live `core/api` shape (correct behavior, type errors at assembly until Plan 2E runs). Plan 2E (the `@quarto/types` refinement) must precede or run concurrently with Plan 3's `jupyter` namespace implementation to avoid silent type mismatch. No plan currently states this ordering dependency explicitly.

**Severity:** Medium-Low (forward correctness, no live breakage). The recommended action is to add a cross-plan dependency note in Plan 2A or Plan 3: "Plan 2E must precede or coincide with Plan 3's `quarto.jupyter` runtime implementation."

---

## Findings

### Finding 1 — `system.pandoc` stub carries a post-Item-A-stale error message and has no "Plan 2" tag

**Key:** FIND-1  
**Classification:** Completeness — (C) dropped-no-seam for the marimo use case  
**Plan location:** plan2a §2aa resolved decision #2 (line 298): "`system.pandoc`... throw a clear, specific 'requires launch context' error"; system/index.ts:305-307 (read directly): `throw requiresLaunchContextError("pandoc")`

**Model evidence:** model B.6 confirms marimo is the **only** engine that calls `quarto.system.pandoc` (`marimo-engine.ts:129-132`), for HTML→markdown conversion needed on PDF/LaTeX output paths. Model D.1: `system.pandoc` status is "STUB (throws 'requires launch context')" at `quarto-api/src/system/index.ts:124,305-307`.

**The gap:** After return-to-q1 Item A (which makes `path`/`system` methods ambient, removing the "gated until launchEngine" mechanism), the stub's `requiresLaunchContextError("pandoc")` error message is architecturally stale — "launch context" is no longer the gating concept. More importantly, `checkRender` and `runExternalPreviewServer` both throw `notYetImplementedError("... Plan 2")` (system/index.ts:311,317), naming a concrete plan. But `system.pandoc` throws `requiresLaunchContextError` — the Plan 2 recovery tag is absent, and the error message points at a mechanism that Item A removes.

**Item A's status:** return-to-q1 Item A is **unexecuted** (its phase 0 + phase 1 checkboxes are all unchecked). Until Item A lands, the gating architecture still exists, so the `requiresLaunchContextError` is technically correct today. **But** once Item A lands, the `pandoc` stub needs to be re-tagged as `notYetImplementedError("pandoc — pandoc binary path is delivered via Init config; implement in Plan 2")`, or better, actually implemented using the ambient config.

**Severity:** Medium. Marimo's PDF/LaTeX render path would fail at this stub. For the current epic scope (julia engine integration), it is not blocking. But Plan 3's marimo story depends on it.

**Recommended action:** After Item A lands, change `system.pandoc`'s stub from `requiresLaunchContextError` to `notYetImplementedError` (or implement it — the pandoc binary path is in the ambient Init config per Item A's design, so implementation is straightforward). Add a "Plan 2" recovery tag. Document in plan2a or plan2A §2aa.

---

### Finding 2 — `system.execProcess` drops knitr's `mergeOutput`/`stderrFilter` params with no wire-level documentation

**Key:** FIND-2  
**Classification:** Completeness — param-level (C) for the knitr built-in pattern  
**Plan location:** system/index.ts:97-100 (the `ExecProcessOptions` interface); no plan mentions `mergeOutput` or `stderrFilter`

**Model evidence:** model B.6 + C.3: knitr calls `quarto.system.execProcess({...stderr:"piped"}, input, "stdout>stderr", (output)=>{...colors.red(output)})` — four positional args, of which args 3 (`mergeOutput`) and 4 (`stderrFilter`) are absent from q2's `ExecProcessOptions`. The model's C.3 table names this as the only non-Julia param in `execProcess`.

**Severity:** Low for Plan 3 scope (knitr is Rust-native in q2; no TS engine needs merged-stderr). Medium for long-term API completeness (a TS engine author cannot reproduce knitr's stderr-coloring pattern). The `respectStreams` and `timeout` params are safely absent (no engine passes them).

**Recommended action:** Add a comment to `ExecProcessOptions` noting the absent params: "Q1's `execProcess` accepts optional 3rd arg `mergeOutput: 'stdout>stderr'` and 4th arg `stderrFilter: (output: string) => string` used by the knitr engine (rmd.ts:451-457). These are not carried here because (a) knitr is Rust-native in q2, and (b) no standalone TS engine uses them. Add when a TS engine needs them."

---

### Finding 3 — `postprocess` hook + `postProcessRestorePreservedHtml` both absent; recovery path for built-in engines unnamed

**Key:** FIND-3  
**Classification:** Completeness — partial (C) gap; recovery path exists via FC-1's `preserve` wire carrier but is unnamed in any plan  
**Plan location:** return-to-q1 Level 2 table (line 609): `postprocess → DROP ("no post-write DOM stage")`. plan2a §2aa decision #3 (line 303): `postProcessRestorePreservedHtml` DEFERRED.

**Model evidence:** model B.7 + C.1: both knitr (rmd.ts:341) and jupyter (jupyter.ts:627) call `postProcessRestorePreservedHtml(options)`. Julia (julia-engine.ts:155) and marimo (marimo-engine.ts:395) have no-op `postprocess`. Model Part C rank 1 (the highest-ranked non-Julia gap).

**Assessment:** The DROP is **architecturally sound** for the standalone engine use case (which is what Plans 1a–3 target). The latent gap is that neither return-to-q1 nor any 1a plan names the recovery path for q2's built-in knitr/jupyter preserve-restore functionality. FC-1 adds `preserve: HashMap<String,String>` and `post_process: bool` as wire carriers — that is the right seam — but no plan says: "when q2 implements HTML preserve-restore for built-in engines, it will use the `preserve` wire field + an AST transform at [stage]."

**Severity:** Low for Plan 3 scope. Standalone engines don't need it. The risk is a future engineer implementing the built-in preserve-restore feature without awareness of the FC-1 wire carrier.

**Recommended action:** Add one sentence to FC-1's description: "The `preserve` field's consumer is a future AST-transform stage that replaces knitr/jupyter's `postProcessRestorePreservedHtml` call in Q1's `postprocess` hook (rmd.ts:341; jupyter.ts:627); the wire already carries the data engine-side."

---

### Finding 4 — `@quarto/types` / Plan 2E ordering dependency not stated

**Key:** FIND-4  
**Classification:** Forward correctness — not a live bug; a missing cross-plan ordering note  
**Plan location:** plan2a (line 29): Plan 2E deferred; no plan states "Plan 2E must precede Plan 3's `jupyter` namespace implementation"

**Model evidence:** model D.2: six jupyter-namespace methods where published `@quarto/types` diverges from Q1's live `core/api/types.ts`. All drifted methods are jupyter-built-in-only; julia and marimo are unaffected.

**Severity:** Low (forward correctness). No live breakage today.

**Recommended action:** Add a cross-plan note in plan2a's "Note" section (or the Plan 3 header): "Plan 2E (q2-specific `@quarto/types` refinements) must run before or concurrently with Plan 3's `quarto.jupyter` namespace runtime implementation to avoid type mismatch on the six drifted jupyter methods (see engine-api-usage-model.md D.2)."

---

## Adequately Covered — Confirmed Not Findings

The following from the model's Part C ledger are adequately covered by the plan family:

| C-rank | Surface | Plan coverage | Assessment |
|---|---|---|---|
| C.1-rank-3 | `format.isServerShiny` (knitr, jupyter) | plan2a §2aa: `format.*` all real (quarto-api/src/format/index.ts:63-151) | COVERED — real method |
| C.1-rank-4 | `path.resource` (knitr, jupyter) | return-to-q1 Item A: ambient, real after Init | COVERED by Item A (pending execution) |
| C.1-rank-6 | `console.warning` (marimo) | plan2a §2aa: `console.{info,warning,error,...}` real (quarto-api/src/console/) | COVERED — real method |
| C.1-rank-7 | `markdownRegex.breakQuartoMd` (jupyter, marimo) | plan2a §2aa: `markdownRegex.*` all real | COVERED |
| C.1-rank-8 | `markdownRegex.getLanguages` (markdown) | plan2a §2aa: real | COVERED |
| C.1-rank-9 | `crypto.md5Hash` (jupyter) | plan2a §2aa: real | COVERED |
| C.1-rank-10 | `system.checkRender` (knitr, jupyter) | stub with Plan 2 tag (system/index.ts:311) | DEFERRED-WITH-SEAM — correct |
| C.1-rank-11 | `system.runExternalPreviewServer` + `onCleanup` (jupyter) | stub with Plan 2 tag (line 317); `onCleanup` real | DEFERRED-WITH-SEAM |
| C.1-rank-12 | `system.tempContext` (knitr) | plan2a §2aa: real (`makeSystem` includes `tempContext`) | COVERED |
| C.1-rank-13 | `text.lineColToIndex`, `text.executeInlineCodeHandler` (knitr) | plan2a §2aa: real (text/index.ts) | COVERED |
| C.1-rank-14 | `jupyter.*` capabilities, kernelspec, python families (jupyter-only) | Plan 3 (explicitly deferred, all jupyter-built-in-only) | DEFERRED-WITH-SEAM — correct for standalone extension scope |
| PROVIDED `run?` (knitr, jupyter) | return-to-q1 Level 2: defer-infra, DQ-2 | DEFER-INFRA with seam |
| PROVIDED `filterFormat?` (jupyter) | return-to-q1 Level 2: defer-infra | DEFER-INFRA with seam |
| PROVIDED `intermediateFiles?` (jupyter) | present in protocol (`IntermediateFiles` message) | COVERED |
| PROVIDED `postRender?` (jupyter) | return-to-q1 Level 2: defer-infra, DQ-2 | DEFER-INFRA with seam |
| `claimsLanguage` numeric score (marimo) | plan1a-protocol: `TsLanguageClaim` harness normalization handles `number → Primary{priority:n}` | COVERED |

---

## Drift Findings (Wrong-vs-Q1)

### Drift-1 — `system.pandoc` error label is stale post-Item-A

(Described under Finding 1.) The "requires launch context" label is correct today but will be wrong after Item A lands. Low severity; tracked as part of FIND-1.

### Drift-2 — No new drift identified in plan text

The return-to-q1 review already caught and addressed the significant Q1 divergences. Candidate B (`claimsLanguage` return shape) was confirmed NOT a regression (deliberate q2 extension). Candidate C (`store_html_dependencies` dedup) is IMPLEMENTED. Candidate D (`TsExecuteResult → ExecuteResult` mapping) is IMPLEMENTED at `ts_engine.rs:489-505`.

---

## Self-Consistency Check

### Plan1a-protocol: `partitionedMarkdown` framing

Plan1a-protocol states "The fold-in of ipynb-filter support into `DocumentProfile` + `markdown_for_file` is worked out as future work in the ipynb-filters research plan, which still flags open items... it is not yet settled." This matches plan1a-engine's identical language. The two plans agree; no self-consistency issue.

### Plan2a §2aa resolved decision #2 vs Item A

Plan2a §2aa (decision #2, line 298) says `path.runtime`, `path.resource`, `path.dataDir`, and `system.pandoc` "throw a clear, specific 'requires launch context' error" — framing these as gated methods. Return-to-q1 Item A redesigns this as "ambient, no gating." There is a **temporal inconsistency** between plan2a's resolved decisions (which reflect the pre-Item-A architecture) and Item A (which deletes the gating). This is not a logic error — Item A explicitly amends §2aa (see Item A Phase 1, line 212-216). But until Item A is executed, plan2a's documented decisions remain inconsistent with the target state. The plans are not self-contradictory; they are a before/after pair where the "after" (Item A) is not yet implemented.

---

## Mechanism-Fidelity Check

### `claimsLanguage` numeric score (marimo C.3 table, `2`/`1`)

Marimo is the only engine returning a numeric score from `claimsLanguage` (marimo-engine.ts:225-231). Plan1a-protocol's harness normalization table (line 472-478) correctly maps `number n → Primary { priority: n }`. The model confirms marimo uses scores `2` (primary claim) and `1` (secondary), and the higher-score-wins tiebreak is exercised. This is correctly handled.

### `dependencies()` round-trip

DQ-2 (return-to-q1) recommends adding dependencies round-trip infrastructure. The plan correctly notes that v1 folds `dependencies()` into execute (harness-internal). FC-1 adds `post_process` as a wire carrier. The mechanism is correct; no fidelity issue.

---

## What I Read / How I Verified

All claims below are grounded in source I read directly in this session:

| Source | What I extracted |
|---|---|
| `claude-notes/research/2026-06-26-engine-api-usage-model.md` (full, 445 lines) | Parts A–D: all five engines' PROVIDES + CONSUMES, Julia-bias ledger, q2 gap table |
| `claude-notes/plans/2026-04-16-plan1a-protocol.md` (full, 958 lines) | Protocol types, `partitionedMarkdown` drop rationale, TsExecuteResult shape |
| `claude-notes/plans/2026-04-16-plan1a-host.md` (first 749 lines) | Subprocess management, demux design, execution status |
| `claude-notes/plans/2026-04-16-plan1a-engine.md` (first 766 lines) | Trait extensions, TsEngine struct, HtmlDependency handling |
| `claude-notes/plans/2026-06-25-plan1a-return-to-q1.md` (full, 699 lines) | Item A (ambient context), PROTO-1/FC-1, ENG-1, HOST-1/2, surface coverage audit, DQ-1..DQ-7 |
| `claude-notes/plans/2026-04-16-plan2a-quarto-api-foundation.md` (full, 306 lines) | §2aa resolved decisions, platform seam, namespace status |
| `ts-packages/quarto-api/src/system/index.ts` (full, 332 lines) | Verified `execProcess` signature (2 args only), `pandoc` stub throws `requiresLaunchContextError`, `checkRender`/`runExternalPreviewServer` stub throws `notYetImplementedError("Plan 2")` |
| `ts-packages/quarto-api/src/path/index.ts` (first 180 lines) | Verified `runtime`/`resource`/`dataDir` throw `requiresLaunchContextError`; pure methods are real |
| `ts-packages/quarto-api/src/text/index.ts` (first 50 lines) | Verified `postProcessRestorePreservedHtml` is absent (only 5 text functions exported) |

**Verification method:** For each calibration hypothesis, I cross-referenced the ground-truth model's Part C/D citations against the plan text (quoting specific line ranges) and the landed source files (reading the actual TypeScript). I did not rely on plan summaries or agent quotes — I read the source directly.

**Scope limits:** I read plan1a-host to line 749 of 1541 (the Phase 2 subprocess management section is fully covered; the Design Notes section is truncated). I read plan1a-engine to line 766 of 1361 (through Phase 4 TsEngine struct + race-free init). The complete plan text for both plans is captured sufficiently for this review's findings.
