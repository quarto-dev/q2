# Plan 3 (`@quarto/api/jupyter`) vs the usage model — reconciled review

**Date:** 2026-06-29
**Plan under review:** `claude-notes/plans/2026-04-16-quarto-jupyter.md` (unedited; confirmed stable).
**Lens:** the all-engines usage model (`2026-06-26-engine-api-usage-model.md`) + the lifecycle
model (`2026-06-25-q1-engine-lifecycle-model.md`) — applied to the most Julia-shaped plan in the epic.
**Method:** 3 detection agents (Opus scope, Opus mechanism, Sonnet field-by-field) varied by angle
over shared background, + 1 Opus completeness critic seeded with their output — **then every
load-bearing finding re-grounded against Q1 source by the reviewer.** Citations below marked
"VERIFIED" were read first-hand by the reconciler; a few mechanism details are marked
"agent-grounded" where the consumer side is reviewer-verified but the deep Q1 internal is taken
from a detector with a specific `file:line`.

---

## Bottom line

**Plan 3 does not "build Julia's slice and seam the rest." It re-drafts its own narrower,
partly-mistyped `JupyterToMarkdownOptions`/`Result`, mis-classifies host-dependence, and
mis-describes several Phase 3B/3C internals — and as written it breaks the epic's own validation
target (Julia) at runtime in four independent places.** The plan's repeated claim that "the API
signatures are compatible" (L402-404) is **demonstrably false** against `julia-engine.ts:272,287,256`.

Two structural roots, two structural fixes:

1. **The contract drift (Tier 1–2).** Plan 3 redrafts its own option/result types instead of
   implementing against q2's already-vendored `@quarto/types` (`quarto-types/src/jupyter.ts`) — and
   the redraft both narrows the contract and, in `cellOutputs`, mistypes it. **Fix: implement against
   the vendored types + Q1's real host I/O, not the Julia-shaped redraft.** This single change
   collapses P3-1/2/3/4/5/8.
2. **The interior errors (Tier 3).** The Phase 3B/3C mechanism descriptions — MIME priority,
   `application/json`, `text/latex`→math, `labels.ts`, `percent-script`, `preserve.ts` signature —
   are individually wrong against Q1 source. **Fix: correct each Phase 3B/3C description against the
   cited Q1 module before implementation.**

Nothing here blocks *other* plans; it is all Plan 3's to fix before it's built. Two detector
findings were **down-calibrated** on grounding (P3-5, P3-8) and one detector *correction* was itself
wrong (the ANSI/deno-dom rebuttal — P3-16). Return-based dataflow *direction*, the figure-write
mechanism, and the "no MappedString provenance" simplification are **confirmed adequate** — don't touch.

---

## Tier 1 — breaks the validation target (Julia) at runtime. Must-fix.

### P3-1 — `cellOutputs: string[]` must be `JupyterCellOutput[]` (HIGH, VERIFIED)
- **Plan** L186: `cellOutputs: string[]`.
- **Q1 / vendored** (`core/jupyter/types.ts:265,272-277`): `cellOutputs: JupyterCellOutput[]`, where
  `JupyterCellOutput = { id?, options?, metadata?, markdown: string }`.
- **Julia** (`julia-engine.ts:272`): `result.cellOutputs.map((output) => output.markdown)`; jupyter
  built-in does the same (`jupyter.ts:577`).
- **Gap:** a `string[]` runtime makes `output.markdown` `undefined` → `outputs.join("")` yields
  garbage. Breaks the worked example. Also drops the per-cell `id`/`options`/`metadata` handles used
  downstream (cell ids, crossref).
- **Action:** type `cellOutputs` as `JupyterCellOutput[]` (use the vendored type).

### P3-2 — `assets` mislabeled "pure" + camelCase field-name break (HIGH, VERIFIED consumer-side)
- **Plan** marks `assets` pure/host-free (L44, L300) and returns camelCase `{ baseDir, figDir,
  supportingDir }` (L266-271).
- **Julia** (`julia-engine.ts:287`): `supporting: [join(assets.base_dir, assets.supporting_dir)]` —
  reads **snake_case** `base_dir`/`supporting_dir`. Plan's camelCase → `undefined` → broken supporting
  path.
- **Mechanism (agent-grounded, `jupyter.ts:665-696`):** Q1 `jupyterAssets` does FS I/O —
  `ensureDirSync(figures_dir)` + `walkSync` to promote `supporting_dir` — and returns
  `{ base_dir, files_dir, figures_dir, supporting_dir }` (base absolute; the rest relative,
  forward-slashed). So `assets` is **not pure**, and the field set is larger than the plan's three.
- **Action:** make `assets` host-dependent (it needs `host.fs`); return the snake_case 4-field shape.
  This also fixes the figure-write target dir (figures land in a dir `assets` was supposed to create).

### P3-3 — `resultIncludes` mislabeled "pure"; it writes widget temp files (HIGH, VERIFIED)
- **Plan** lists `resultIncludes` as pure, no host (L44, L300; "object transformation," L65).
- **Q1 (VERIFIED, `widgets.ts:148-154`):** `resultIncludes` → `includesForJupyterWidgetDependencies`
  → `widgetTempFile`: `Deno.makeTempFileSync` + `Deno.writeTextFileSync`. It materializes widget
  includes to disk.
- **Julia (VERIFIED, `julia-engine.ts:256-259`):** calls `resultIncludes(options.tempDir,
  result.dependencies)` in the `if (options.dependencies)` branch — its **hot path** for any
  widget/plotly/HTML-library output.
- **Action:** make `resultIncludes` host-dependent (needs `host.fs` for temp writes).

### P3-4 — options drop `executeOptions` + `figPos`, both passed by Julia (HIGH, VERIFIED)
- **Plan** `JupyterToMarkdownOptions` (L170-183) omits `executeOptions` and `figPos`.
- **Julia (VERIFIED):** passes `executeOptions: options` (`julia:231`) and `figPos:
  options.format.render[kFigPos]` (`julia:245`); jupyter passes both (`jupyter.ts:532,546`).
- **`executeOptions` is load-bearing (agent-grounded, `jupyter.ts:719-733`):** it drives the
  book/single-file/minimal **fixup-profile** selection (`options.executeOptions.project`). Without it
  Plan 3 cannot reproduce the correct notebook fixups for book/project renders.
- **Action:** restore both fields (or, per the structural fix, adopt the vendored
  `JupyterToMarkdownOptions` wholesale — it has them: `quarto-types/src/jupyter.ts:248,260`).

---

## Tier 2 — contract/scope gaps (real; degrade gracefully or jupyter-forward)

### P3-5 — `notebookOutputs` missing from result (MED — down-calibrated, VERIFIED)
- **Plan** result omits `notebookOutputs`; Q1 has `notebookOutputs?: { prefix?, suffix? }`
  (`types.ts:266-269`).
- **Julia (VERIFIED, `julia:273-280`):** reads it **behind `if (result.notebookOutputs)`** — so
  absence degrades *gracefully* (lost prefix/suffix YAML round-trip on the ipynb path), it does **not**
  crash. Agent 2 rated HIGH; the guard makes **MED** the honest call.
- **Action:** add `notebookOutputs?` to the result (free if adopting the vendored type). `metadata?`
  is also dropped — harmless (no engine reads it).

### P3-6 — closed 6-method factory won't satisfy `JupyterNamespace` at Phase 3E (MED runtime / HIGH at wiring; VERIFIED roster)
- Plan builds **6 of 23** `JupyterNamespace` members (the 6 Julia calls); `createJupyter` returns a
  **closed object literal** (L295-304) with no stub seam. The 17 omitted are all jupyter-built-in-only
  — **no current q2 TS runtime consumer** (jupyter is native-Rust; marimo uses `system.pandoc`), so the
  runtime gap is MED.
- **But Phase 3E** (`return createJupyter(denoHost)`, L313-323) wires this into `quarto.jupyter`, typed
  as the full 23-method `JupyterNamespace`. A 6-method object does not satisfy a 23-method interface →
  **won't typecheck** unless q2 also narrows `JupyterNamespace` (undiscussed) or the factory stubs the
  rest.
- **Action:** state the seam — stub the other 17 as `NotImplemented` throwers so the namespace type is
  satisfied and a future jupyter-port has named slots. (This is the "defer-with-seam," not drop.)

### P3-7 — `widgetDependencyIncludes` built but not exposed; deferred-fold unwired (MED, VERIFIED; cross-seam)
- Plan builds `widgetDependencyIncludes` *inside* `widgets.ts` (L143) but the factory does **not**
  export it (L295-304). So the method jupyter's `dependencies()` hook needs (`jupyter.ts:610-613`) and
  RTQ's **FC-2 deferred-deps fold** calls is unreachable through `quarto.jupyter.*`.
- **Least-cost fix:** add the one line to the factory return. Cross-link to RTQ FC-2 / 1B-DEPS-4 so the
  fold and the exposure land together. (The array-vs-singular *type* of this method is D.2 drift — Plan
  2 Phase B's; the **exposure** gap is Plan 3's.)

### P3-8 — `preserveCellMetadata` (LOW — down-calibrated per critic O2, VERIFIED)
- Plan omits it. Detectors grouped it with the Julia-breaking option drops — but **Julia comments it
  out** (`julia:246`) and passes `preserveCodeCellYaml` instead. It is **jupyter-built-in-only** (usage
  model C.3). So "dropping it breaks Julia" is **over-stated** — it's a forward/jupyter concern only.
- **Action:** include it for vendored-type fidelity, but it's LOW (no Julia impact).

---

## Tier 3 — interior mechanism errors (Phase 3B/3C). The plan's descriptions are wrong.

### P3-9 — the MIME priority order (L116) is wrong AND not a fixed order (HIGH, VERIFIED)
- **Plan** L116 asserts one fixed order: `text/html > image/svg+xml > image/png > image/jpeg >
  text/markdown > text/latex > text/plain`.
- **Q1 (VERIFIED, `display-data.ts:45-97`):** `displayDataMimeType` computes the order **dynamically
  from the target format**. Base list is `[text/markdown, image/svg, image/png, image/jpeg]` —
  **`text/markdown` is the highest base, not `text/html`** (the plan inverts the two highest). The
  html/widget cluster (`widget-state`, `widget-view`, `application/javascript`, `text/html`) is spliced
  **conditionally** on `toHtml`/`toMarkdown` — the plan **omits the three widget/javascript MIME types
  entirely**. `text/latex` is added **only** for `toLatex`. An html-table special case force-adds
  `text/html`.
- **Gap:** a from-scratch impl built to L116 mis-ranks outputs for every format and never renders
  widgets. This is the critic's headline find, in a module the boundary detectors dismissed as "~150
  lines, pure."
- **Action:** port `displayDataMimeType`'s dynamic algorithm; do not encode a fixed list.

### P3-10 — `application/json` and `text/latex`→math dispatch are wrong (MED, agent-grounded)
- `application/json → code block` (L218) is wrong: Q1 has no generic json path; `displayDataIsJson`
  matches only widget MIME types (`display-data.ts:176-179`) and emits a `<script type=…>` tag (falling
  back to a json code block only when `toIpynb`), injecting `kQuartoMimeType` first.
- `text/latex → math` (L217) skips the is-math detection: Q1 routes latex into the markdown slot only
  when `displayDataLatexIsMath` holds (`display-data.ts:108-137`), else emits a `{=tex}` raw block.
- **Action:** correct both dispatch descriptions.

### P3-11 — `labels.ts` invents one export and omits three real ones (MED, agent-grounded)
- Plan invents `cellLabelClass` (no such Q1 function). It omits the real consumer-needed exports:
  `cellLabelValidator` (duplicate-label guard, `labels.ts:47-61`), `shouldLabelCellContainer` /
  `shouldLabelOutputContainer` (crossref div wrapping, `labels.ts:63-134`). `resolveCaptions` handles
  `fig-cap`/`fig-subcap`, not `tbl-cap` (L130 is wrong — tbl-cap is a downstream lua filter). Also id
  normalization uses `asHtmlId` (`core/html.ts`), not `pandocAutoIdentifier`; and `pandocAutoIdentifier`
  is called with a **2nd boolean arg** (`jupyter.ts:1548`) the plan's 1-arg signature (L148) omits.
- **Action:** correct the `labels.ts`/`pandoc-id.ts` Phase-3B descriptions against Q1.

### P3-12 — `tags.ts` omits `echoFenced` and `includeWarnings` (LOW-MED, agent-grounded)
- Plan's hide*/include* list (L120-124) misses `echoFenced` (drives `echo: fenced`, `tags.ts:68-75`)
  and `includeWarnings`, plus the "global false + local true" warning logic (`tags.ts:39-44,93-101`).

### P3-13 — `preserve.ts` signature shape is wrong (MED, agent-grounded)
- Plan: `removeAndPreserveHtml(output) => { output, preserved }` (L135-138). Q1
  (`preserve.ts:12-42`): `removeAndPreserveHtml(nb: JupyterNotebook) => Record<string,string> |
  undefined`, **mutating cell output bundles in place** (swaps `text/html` for a markdown placeholder).
  Not a per-output pure-string transform. Compounds with P3-15.

### P3-14 — percent-script is more than "regex" and mis-described, on Julia's hot path (MED-HIGH, VERIFIED)
- **Q1 (VERIFIED, `percent.ts:32-45`):** `isJupyterPercentScript` requires
  `^\s*${cms}\s*%%+\s+\[(markdown|raw)\]` — a **language-specific** comment char (`kLangCommentChars`)
  plus a `[markdown]` or `[raw]` marker. **A `.jl` with only `# %%` code markers is NOT detected.** Plan
  L62 ("check for `# %%` markers") is inaccurate, and detection feeds Julia's `claimsFile` →
  `isPercentScript(file, [".jl"])` and `markdownForFile` → `percentScriptToMarkdown` (`julia:95,164,167`).
- Also (agent-grounded, `percent.ts:12`) `markdownFromJupyterPercentScript` imports
  `mdRawOutput`/`mdFormatOutput` from `jupyter.ts` — so percent-script **couples to to-markdown**,
  contradicting the plan's "self-contained ~80-line module" framing (L62, L247-261).
- **Action:** correct the percent-script description (marker requirement, language comment chars,
  to-markdown dependency); ground the content branch (resolves the inherited 1c-GAP-A).

---

## Tier 4 — fidelity, down-calibrated / resolved contradictions

### P3-15 — preserve producer with no consumer; Q1's `isPreservedHtml` is inert today (MED, VERIFIED)
- **VERIFIED (`preserve.ts:58-60`):** `isPreservedHtml(_html) { return false; }` — Q1's producer
  preserves nothing today, so `htmlPreserve` is always empty and `postProcess` always false (Julia's
  `preserve`/`postProcess` at `julia:292-294` are inert end-to-end). A *faithful* port is a harmless
  no-op → **MED, not HIGH.**
- **The real hazard is the plan's prose:** it describes a *live* "protect HTML / restore for
  post-processing" mechanism Q1 doesn't currently run, and names **no consumer** (the restorer
  `postProcessRestorePreservedHtml` is in `quarto.text`, deferred — RTQ F2/B2; the `postprocess` hook is
  dropped under the No-DOM rule). If an implementer makes `isPreservedHtml` return `true` without
  building the (AST-transform) restorer, output ships literal `preserve<uuid>` tokens.
- **Action:** state explicitly — port `isPreservedHtml` as the constant-`false` no-op it is, **or**
  build the AST-transform restorer reading the `preserve` map (No-DOM rule). The plan does neither.

### P3-16 — ANSI "always strip": contradiction resolved at source (MED, VERIFIED)
- **VERIFIED (`src/core/ansi-colors.ts:7,8,16,21-26`):** Q1 uses `ansi_up` (primary color→span) **plus**
  deno-dom `parseHtml` for an `ansi-bold` class post-step. So Agent 2's "Q1 uses ansi_up, **not**
  deno-dom" is **factually wrong** (deno-dom is used); the critic's "the plan's deno-dom label is
  accurate" is closer but generous (deno-dom's role is a narrow, regex-replaceable bold swap).
- **Net:** the substantive finding survives — Plan 3's "always strip" loses HTML color + the
  `.ansi-escaped-output` CSS hook — **but only on HTML output**; for latex/markdown/ipynb targets it
  matches Q1's strip exactly. Severity **MED, HTML-output fidelity**. A portable port needs `ansi_up` +
  a regex (or DOM) bold-class swap.

### P3-17 — `widgetDependencies` producer drops the in-place nb mutation (LOW, agent-grounded)
- Q1's `extractJupyterWidgetDependencies(nb)` mutates `cell.outputs` in place to strip hoisted HTML
  libraries before the cell-walk (`widgets.ts:47-62`); the plan's `widgetDependencies(outputs)` (L142)
  takes outputs and omits the strip → plotly/HTML-library `<script>` could double-emit. LOW (downstream
  of P3-3/P3-7 already needing rework).

---

## Plan-internal nits
- **"API signatures are compatible" (L402-404) is false** — the load-bearing meta-claim, disproven by
  P3-1/2/4 against `julia:272,287,231,245`. Strike or qualify it.
- **"7 methods" (L15) vs "6" (L408)** — counting error; it's 6.

## Confirmed adequate — do NOT touch
- **Return-based dataflow *direction*** — Plan 3 returns a result object; Phase 3E forwards
  `createJupyter(host)`; no accumulator/registration. Matches lifecycle §3. (The *shape* is the
  problem — Tier 1 — not the direction.)
- **Figure-write mechanism** — base64-decode → `host.fs.writeFileSync` via the `createJupyter(host)`
  closure is correct (L221-229); the defect is upstream in `assets` (P3-2), not the write.
- **"No MappedString provenance" simplification (LOW)** — `toMarkdown` returns plain markdown strings;
  Julia builds source ranges *separately* via `mappedString.splitLines`/`indexToLineCol`
  (`julia:638-647`), not inside `toMarkdown`. Dropping provenance here breaks no consumer read. (The
  per-cell `id`/`options`/`metadata` loss is P3-1, a different issue.)
- **No-tree-sitter / no-YAML-schema-validation for cell options** — acceptable; no consumer needs the
  CST at this layer.
- **percent-script host-binding** — correctly host-bound, not over-deferred (the *detection logic* is
  P3-14, but the host-dependence classification is right).

## Netted out — cross-plan, don't re-track here
- The **D.2 jupyter TYPE drift** (7 methods vendored-vs-live) → **Plan 2 Phase B**. Plan 3 builds the
  runtime for only the 6 Julia methods, which sit outside the 7 drifted — so the *type* reconciliation
  is Phase B's, the *runtime/scope/mechanism* is Plan 3's.
- The **`postProcessRestorePreservedHtml` consumer** (the restore half of P3-15) is deferred in
  `quarto.text` → **RTQ F2/B2** (one-recovery-story: AST transform reading FC-1's `preserve`).

---

## What I read / how I verified
- Re-grounded first-hand: `julia-engine.ts:252-296` (cellOutputs `.markdown`, notebookOutputs guard,
  resultIncludes call, assets.base_dir/supporting_dir, preserve/postProcess), `core/jupyter/types.ts:260-277`
  (result + `JupyterCellOutput`), `preserve.ts:50-78` (`isPreservedHtml`=false + restore I/O),
  `display-data.ts:45-106` (dynamic MIME priority), `percent.ts:32-55` (marker requirement, lang comment
  chars), `widgets.ts:143-160` (`makeTempFileSync` write), `src/core/ansi-colors.ts` (`ansi_up` +
  deno-dom). Agent-grounded (consumer side reviewer-verified): `jupyter.ts:665-696` (assets FS I/O),
  `:719-733` (fixup selection), `labels.ts`/`tags.ts` exports, `display-data.ts:108-179` (json/latex).
- Inputs: 3 detection agents (scope / mechanism / field-by-field) + 1 completeness critic; the critic
  correctly surfaced the Phase 3B/3C interior errors (P3-9..14) and pushed back on P3-5/P3-8/P3-16.
