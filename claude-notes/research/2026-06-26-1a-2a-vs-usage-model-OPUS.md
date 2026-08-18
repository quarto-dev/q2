# 1a-family + 2A vs. the all-engines usage model — review through the Julia-bias lens

**Status:** review finding / calibration artifact.
**Built:** 2026-06-26 by an Opus reviewer.
**Lens:** the epic used Julia (TCP daemon; thin on the engine-author API) as its
only worked example. The ground-truth model
(`claude-notes/research/2026-06-26-engine-api-usage-model.md`, "the model") surveyed
all five engines and named the surface a **non-Julia** engine uses that a Julia-only
plan would plausibly miss. This review checks whether the **effective 1a spec** —
the original protocol/host/engine plans **AS AMENDED by** `2026-06-25-plan1a-return-to-q1.md`
("RTQ") — plus 2A's landed §2aa, adequately cover that at-risk set, or drop/stub/reduce
it with no sound seam.

**What I verified myself (not subagent-quoted):** I read the model in full; the
design contract `engine-resolution.md`; all five plans; the landed
`ts-packages/quarto-api/src/system/index.ts` and `text/index.ts`; and four Q1 / extension
source sites directly:
- knitr `src/execute/rmd.ts:440-460` — execProcess **does** pass `"stdout>stderr"` (3rd
  arg) + a stderr-filter closure (4th arg). Verbatim, confirmed.
- `postProcessRestorePreservedHtml` callsites: `rmd.ts:341`, `jupyter/jupyter.ts:627`. Confirmed.
- marimo `src/marimo-engine.ts:129-132` — the **only** `quarto.system.pandoc(["-f","html","-t","markdown"], html)` caller; falls back via `quarto.console.warning`. Confirmed.
- jupyter `jupyter.ts:360-363` — `partitionedMarkdown(file, format?)` passes `format` into `markdownFromNotebookFile`. Confirmed.

---

## Bottom line — MOSTLY adequate, two real no-seam gaps

The 1a-family-as-corrected is, on the **engine-PROVIDED / wire-carrier** axis,
genuinely good. RTQ's "defer features, not infrastructure" principle plus its
**Surface coverage audit**, **FC-1** (`#[serde(default)]` wire carriers for the dropped
`ExecuteResult` fields), and **Item A** (path/system → ambient, un-stubbing
`path.runtime/resource/dataDir` + `system.pandoc`) cover the bulk of the model's Part C
ledger with named seams. Credit that work — it is exactly the right shape and most of
the "Julia-only would miss this" risk is retired by it.

But RTQ's audit is **method-/field-granular on the surface q2 PROVIDES or carries on the
wire**, and is **systematically thinner on two things the model's Part B/C/D specifically
flag**: (1) **parameter-level** reductions inside an otherwise-present method, and (2)
the **QuartoAPI-CONSUMED** callback surface where a hook AND the API method it calls are
*both* dropped, leaving no recovery path. The hypotheses in the brief are largely
**confirmed**. The two findings that matter:

- **(C-class) `system.execProcess` param reduction** — knitr's `mergeOutput:"stdout>stderr"`
  + `stderrFilter` (rmd.ts:451-457) are **dropped from the 2A signature** (`system/index.ts:97-100`
  is `(options, stdin?)` only) and **no 1a plan or RTQ mentions it.** Param-level, so RTQ's
  method-granular audit (which marks `execProcess` "real") is structurally blind to it. **Finding A2.**
- **(C-class, coordinated no-seam) `postprocess` hook + `text.postProcessRestorePreservedHtml`
  are BOTH gone with no named recovery path.** RTQ classes the `postprocess` hook **DROP**
  ("no post-write DOM stage"); 2A §2aa **DEFERS** the API method (`text/index.ts:9-11`). The
  hook's *only* real work in knitr+jupyter (rmd.ts:341; jupyter.ts:627) is exactly that one
  API call — so both ends of the same feature are removed, and neither plan names how
  preserve-restore returns. **Finding A1.**

Everything else (`system.pandoc`, `path.*`, the jupyter namespace, `partitionedMarkdown`,
the `@quarto/types` jupyter lag) is **adequately covered or correctly out-of-scope** — see
"Confirmed adequate" below. These two are the actionable residue.

---

## Findings

Severity calibrated honestly. "(C)" = dropped/reduced-with-no-seam, used by a non-Julia
engine = the framework-completeness finding class. "(B)" = deferred-with-seam (acceptable,
noted for completeness). Drift findings separated at the end.

### A1 — `postprocess` + `postProcessRestorePreservedHtml`: coordinated no-seam drop — **(C), completeness, Low-Med**

**Plan locations:**
- RTQ Surface audit, Level 2 (line 608): `| `postprocess` | — | **drop** | q2 has no post-write
  DOM stage (AST transforms; No-DOM-postprocessor rule). Preserved-region restore, if ever
  needed → AST transform |`
- RTQ DQ-2 (line 628-629): "**drop `postprocess`** (justified — no post-write DOM stage)".
- 2A §2aa (`text/index.ts:9-11`, landed): "`postProcessRestorePreservedHtml` is DEFERRED — it
  does file I/O … See task-2aa-2 plan decision #3." Plan2A line 301-302 same.

**Model / Q1 evidence (verified):** the `postprocess` instance hook is **Required** (model
A.2, types.ts) and implemented by all five engines, but its *only real work* in knitr+jupyter
is the single call `quarto.text.postProcessRestorePreservedHtml(options)` (rmd.ts:341,
jupyter.ts:627 — I read both). markdown/julia/marimo no-op it. So this is a **two-engine**
(knitr, jupyter), non-Julia capability.

**The gap in plain language:** the hook is classed DROP *and* the API method it calls is
classed DEFER, by two different plans, with **no cross-reference between them** and **no
single named recovery path**. RTQ's drop rationale ("→ AST transform") is plausible and
consistent with the repo's No-DOM-postprocessor rule — preserve/restore of HTML-preserved
regions could in principle be an AST transform. But: (a) preserve-restore is **real Q1
functionality both built-ins rely on**, not a DOM-cosmetic like auto-stretch; (b) nobody has
checked that q2's writer even produces the preserved-region markers the restore would
re-expand, so "→ AST transform" is an unverified assertion, not a seam; (c) the API-method
defer and the hook drop are not connected, so an implementer reading either plan alone sees a
clean local decision and misses that the *whole feature* has no home.

**Why not higher severity:** q2 reimplements knitr/jupyter **natively in Rust** (model's
built-in note) — these two engines never call the TS `@quarto/api` at runtime, so no *TS
extension* is broken today by the method being deferred. The exposure is (i) a future TS
engine that legitimately needs preserve-restore has no API to call, and (ii) the native
knitr/jupyter ports owe the equivalent behavior and no plan tracks where it lives. Real, but
not a live break.

**Recommended action:** add one cross-plan note (RTQ owns it, since RTQ owns the drop
classification): preserve-restore is a single coherent feature whose **hook (DROP) and API
method (DEFER) are coupled**; name the concrete recovery as "an AST transform in the
Finalization phase **that depends on the QMD/HTML writer emitting preserved-region markers** —
verify that prerequisite before assuming the transform path exists." Promote the `text`
method from silent DEFER to **defer-infra-with-named-consumer** so it isn't invisibly dropped.

### A2 — `system.execProcess` param reduction (`mergeOutput` + `stderrFilter` dropped) — **(C), completeness, Low-Med**

**Plan location:** 2A landed `system/index.ts:97-100`:
```ts
execProcess(options: ExecProcessOptions, stdin?: string): Promise<ProcessResult>;
```
plus the mapping at `:178-202` which marshals only `{cwd, env, stdin}` and treats `stdout`/`stderr`
mode as "advisory." **No 3rd/4th positional param.** RTQ never mentions execProcess; its Level-2
audit marks `execProcess` present/real (it appears in D.3's "ported (real)" list as the "2-arg
form"). The reduction is therefore **invisible to RTQ's method-granular audit** — confirming
brief hypothesis 2.

**Model / Q1 evidence (verified by me, rmd.ts:440-460):** knitr passes **four** positional args:
```ts
quarto.system.execProcess(
  { cmd: await rBinaryPath("Rscript"), args: [...], cwd, stderr: quiet?"piped":"inherit" },
  input,                  // 2nd: stdin
  "stdout>stderr",        // 3rd: mergeOutput
  (output) => { if (outputFilter) output = outputFilter(output); return colors.red(output); }  // 4th: stderrFilter
);
```
`mergeOutput:"stdout>stderr"` folds R's stderr into the captured stdout stream;
`stderrFilter` colorizes/filters R's progress output. jupyter passes neither (2-arg). The
model (B.6 note + the explicit safe-to-defer note at C.3) is precise: **`respectStreams` (5th)
and `timeout` (6th) are used by no engine** — dropping those is safe and should be stated as
such; **`mergeOutput` (3rd) and `stderrFilter` (4th) are used by knitr** and are the at-risk slice.

**Gap in plain language:** a future TS knitr-shaped engine (or anyone wanting merged/filtered
subprocess output) cannot express it through `quarto.system.execProcess` — the params don't
exist on the signature, and **no plan records that they were dropped or why.** This is exactly
the "param-level drop invisible to a method-granular audit" failure mode. The model's
seam-bypass note (B.6) sharpens the stakes: marimo *already* routes around the API with raw
`Deno.Command` when the API doesn't fit — an ergonomically-incomplete execProcess invites
exactly that bypass, which then can't run under a future VFS/worker host.

**Why not higher:** the only Q1 user is knitr, which q2 reimplements natively in Rust (doesn't
hit this TS path). No live break. But it is a genuine framework-completeness hole with **zero
documentation**, unlike every other deferral RTQ tracks.

**Recommended action:** smallest correct fix is a **documentation seam** in RTQ's coverage audit:
add an execProcess param-disposition row — `mergeOutput`/`stderrFilter` = **defer-infra**
(knitr uses them; recovery = widen `ExecProcessOptions` with `mergeOutput?` + a `stderrFilter?`
callback, or a structured equivalent), and `respectStreams`/`timeout` = **safe-drop (no engine
uses)**. Optionally land the two `?`-optional fields now (additive, no consumer) so the SDK
type doesn't calcify a reduced shape. Either way: **name it**, because today it is silently gone.

---

## Confirmed ADEQUATE (do not touch — credit where due)

- **`system.pandoc` (marimo-only, the model's single biggest catch) — RESOLVED by RTQ Item A.**
  The model flags it as a throwing stub (`system/index.ts:124,305-307`). RTQ Item A makes
  `path`/`system` **ambient** (delivered once at startup via the `Init` frame), explicitly
  un-stubbing `system.pandoc` ("`path.runtime`/`resource`/`dataDir` and `system.pandoc` become
  real as soon as the harness is assembled with the startup config", RTQ:215-216; the §2aa
  "requires launch context" stub is "**gone**", RTQ:158). This is precisely the recovery the
  model asks for. **Covered.** (Caveat: the *body* still depends on Plan 2 wiring the pandoc path
  through; RTQ correctly says "coordinate with Plan 2's body work." That's a body-fill, not a
  protocol gap — the seam exists.)

- **`path.runtime/resource/dataDir` (jupyter/knitr; runtime also julia) — RESOLVED by Item A.**
  Same mechanism; the model's D.1 stubs-that-throw become ambient. Covered.

- **The whole jupyter namespace + the jupyter-only methods (`capabilities*`, `widgetDependencyIncludes`,
  `notebookFiltered`, `quartoMdToJupyter`, …) — correctly OUT OF SCOPE for 1a/2A.** It is types-only
  by design; runtime bodies are explicitly **Plan 3** (2A:235-236, 263). Not a 1a/2A gap.

- **`@quarto/types` jupyter signature lag (model D.2) — NOT mis-claimed by any 1a/2A plan.**
  2A vendors Q1's *published* `packages/quarto-types` byte-faithfully (2A:118-124) and **explicitly
  defers** QuartoAPI signature refinement to "Plan 2E" (2A:29, 126, 270-272). No 1a plan or 2A
  asserts Q1-*live*-parity for `pythonExec`/`capabilities`/`notebookFiltered`/etc. The drifted
  methods are jupyter-built-in-only (julia/marimo don't call them), so nothing breaks today, and
  reconciliation is correctly downstream. Brief hypothesis 5 **confirmed** — this is a forward
  correctness item, not a current mis-claim. Adequate.

- **`partitionedMarkdown` DROP — sound superset-replacement, NOT a silent capability loss.**
  Brief hypothesis 4. The model (A.2 line 109) notes all five implement it and jupyter uses the
  `format?` 2nd arg (verified, jupyter.ts:360-363). RTQ classes it DROP ("pampa parses qmd natively;
  DocumentProfile subsumes"). plan1a-protocol:182 does the homework the model asks for: it enumerates
  Q1's **5 caller sites**, identifies the **2** that pass a real `format` (project-index, render-contexts),
  and argues both are subsumed by q2's `DocumentProfile` checkpoint + `MetadataMergeStage`; the
  filter-aware notebook slice folds into `markdown_for_file` (with the open items honestly flagged in
  the ipynb-filters research plan). This is a reasoned superset-replacement with a named home, **not**
  a silent drop. The `format?` arg's purpose (filter-aware YAML harvest for jupyter notebooks) is the
  one genuinely-engine-specific bit, and it is explicitly tracked as future work, not lost. Adequate.
  (Minor: the claim "DocumentProfile genuinely covers the consumers" rests on the ipynb-filters fold-in
  actually landing; that's tracked, so it's a defer-with-seam, not a no-seam gap.)

- **`claimsLanguage` numeric-score / kind-tagged claim (marimo numeric, model C.3) — covered & correct.**
  marimo is the only engine returning a numeric score (model A.1 line 92). The design contract
  (`engine-resolution.md` §3.2) + RTQ candidate-B closure handle the `boolean|number|object`
  normalization with **no sign games** (false→None, true→Primary(1), number n→Primary(n), object→tagged).
  Mechanism-fidelity (false→skip, true→1, number→score, strictly-greater-wins) is faithfully
  preserved. RTQ explicitly re-verified B as "NOT a regression." Adequate.

- **FC-1 `ExecuteResult` output-field carriers — exactly right.** `metadata`/`pandoc`/`resourceFiles`/
  `preserve`/`postProcess` added as `#[serde(default)]` wire carriers (RTQ FC-1), superseding PROTO-1's
  "drop." This is the model's "carry the framework, defer the feature" applied correctly. Adequate.

---

## Drift vs. completeness ledger

- **A1, A2 are completeness** (missing/reduced seam), **not drift** — the plans don't *cite a wrong
  Q1 shape*; they *omit* a real one. No false Q1 claim is made about either.
- **No drift findings.** I checked the load-bearing Q1 citations the plans rely on (execProcess
  arity, postProcess callsites, marimo pandoc, partitionedMarkdown format arg) and each plan's
  Q1 characterization that I could test was accurate. RTQ's self-corrections (candidate B closure,
  D.2 attribution-correction crediting the published-vs-live lag to Q1 not q2) are themselves
  drift-corrections done correctly.
- **Self-consistency:** the one incoherence is internal to the *plan set*, not a single plan: A1's
  hook-drop (RTQ) and method-defer (2A) are individually coherent but jointly leave a feature
  homeless because no plan owns the join.

---

## What I read / how I verified

| source | used for |
|---|---|
| `claude-notes/research/2026-06-26-engine-api-usage-model.md` (full) | the checklist; Parts B/C/D at-risk ledger |
| `claude-notes/designs/engine-resolution.md` (full) | claim scoring, tiers, mechanism-fidelity |
| `2026-06-25-plan1a-return-to-q1.md` (full) | the correction layer — Item A, FC-1, Surface audit, DQ-1..7, PROTO/HOST/ENG items |
| `2026-04-16-plan1a-protocol.md` (full) | wire shapes; `partitionedMarkdown` 5-callsite analysis; deferred-method table |
| `2026-04-16-plan1a-engine.md` (postprocess/partitioned/run sections) | trait-surface deferrals + rationale |
| `2026-04-16-plan2a-quarto-api-foundation.md` (full) | §2aa namespace dispositions; `text` defer; execProcess scope |
| landed `ts-packages/quarto-api/src/system/index.ts` (full) | **execProcess `(options, stdin?)` — params 3-4 gone**; pandoc/checkRender/runExternalPreviewServer stubs |
| landed `ts-packages/quarto-api/src/text/index.ts:1-40` | `postProcessRestorePreservedHtml` DEFERRED comment |
| Q1 `src/execute/rmd.ts:440-460` (read directly) | execProcess 4-arg call: `"stdout>stderr"` + stderrFilter closure |
| Q1 `rmd.ts:341`, `jupyter/jupyter.ts:627` (grep) | postProcessRestorePreservedHtml callsites |
| Q1 `jupyter/jupyter.ts:358-372` (read directly) | partitionedMarkdown `format?` arg flows to markdownFromNotebookFile |
| `quarto-marimo/src/marimo-engine.ts:127-135` (read directly) | sole `system.pandoc(["-f","html","-t","markdown"], html)` caller + console.warning fallback |

**Coverage note:** I did not re-read the host/engine plans' HOST-1..6/ENG-1..2 code-bug items —
they are q2-native bug fixes with no Q1 analogue and out of this review's lens (the model is about
the *engine-author API surface*, not the host's crash/cache internals). RTQ owns those and they
don't bear on the non-Julia at-risk ledger.
