# 1a-family + 2A — calibration review against the all-engines API-usage model (reconciled)

**Date:** 2026-06-26
**Scope reviewed:** the four `1a` plans — `plan1a-protocol`, `plan1a-host`, `plan1a-engine`,
and the `plan1a-return-to-q1` (RTQ) correction layer — read as a UNIT (effective spec =
originals *as amended by* RTQ), plus `plan2a-quarto-api-foundation`.
**Evaluated against:** `claude-notes/research/2026-06-26-engine-api-usage-model.md` (the new
all-five-engines ground truth) through the Julia-bias lens.
**Method:** two independent detection agents (Opus + Sonnet, identical briefs, scoped to
1a+2A so their context stayed clean) → reconciled here → **every surviving finding
re-grounded against source by the reviewer**, who additionally read the *downstream* plans
(Plan 1c, Plan 2, Plan 3) that the agents deliberately did not, to adjudicate "is this
gap closed later?" Agent reports: `…-OPUS.md`, `…-SONNET.md`.

---

## Bottom line

**MOSTLY adequate — and better than the Julia-bias thesis predicted.** The epic had
already self-corrected most of its Julia-only blind spot *before* this review: RTQ's stated
principle "**defer features, not infrastructure** … the Julia scoping made an excellent
skeleton, but the framework must carry the *whole* engine API" is exactly the right
posture, and RTQ's three big moves — **Item A** (path/system become ambient, un-stubbing
`path.runtime/resource/dataDir` + `system.pandoc`), **FC-1** (dropped `ExecuteResult` output
fields become `#[serde(default)]` wire carriers), and the **Surface coverage audit** — close
the large majority of the model's non-Julia at-risk ledger with named seams.

Both agents independently reached "mostly adequate," and converged on the **same two real
gaps**. After grounding, the net is:

- **One genuinely uncovered gap, epic-wide: `system.execProcess`'s `mergeOutput` +
  `stderrFilter` params** (knitr uses both). Low–Moderate.
- **One coordinated *feature* deferral that is seamed but unconnected: preserve-restore**
  (`postprocess` hook + `text.postProcessRestorePreservedHtml`). Low.
- **Two coordination wrinkles the RTQ correction created but didn't fully propagate**
  (a stale stub label + Plan-2's now-obsolete gating prose; a Phase-B-before-Plan-3 type
  ordering). Low.

**No drift findings.** Every load-bearing Q1 citation both agents and I tested was accurate.
The headline structural insight (below) is *why* execProcess slipped through.

---

## The structural insight (why this calibration round mattered)

RTQ's Surface coverage audit is excellent but covers exactly one half of the contract: the
engine-**PROVIDED** interface (`ExecutionEngine{Discovery,Instance}` members), the inbound
**options/context** q2 *sends* engines, and the `ExecuteResult` q2 *receives*. It has **no
systematic audit of the QuartoAPI-CONSUMED surface** — the methods engines call *back* into
`quarto.<ns>.<method>`. The new model is precisely that missing half.

Spot-checking the consumed surface against the model shows 2A independently built almost all
of it *real*, and RTQ Item A / Plan 2 Phase A cover the rest — so the blind spot did little
damage. **The lone escapee is a *parameter-level* drop inside a consumed method** that a
method-granular, provided-surface audit is structurally incapable of seeing: `execProcess`
reads as "present/real," so nothing flagged that two of its params were amputated. That is
finding F1, and it is the concrete vindication of running this lens.

---

## Findings

### F1 — `system.execProcess` drops `mergeOutput` + `stderrFilter`; uncovered across the *entire* epic (Low–Moderate)

- **Evidence (reviewer-grounded):** knitr calls `quarto.system.execProcess({…}, input,
  "stdout>stderr", (output)=>…colors.red(output))` — i.e. it passes `mergeOutput` (arg 3)
  and a `stderrFilter` closure (arg 4) — at `quarto-cli/src/execute/rmd.ts:440-458`. 2A's
  `SystemNamespace.execProcess(options, stdin?)` (`quarto-api/src/system/index.ts:97-100`)
  has only two params; its `ExecProcessOptions` (`:43-58`) is labeled a "subset … that is
  platform-neutral." **No plan restores them:** RTQ never mentions execProcess; Plan 2
  line 66 explicitly calls `system.execProcess` "already real in §2aa," treating it as done.
- **Plain language:** a real built-in engine actively uses two params q2 silently dropped,
  and nothing downstream adds them back. `respectStreams`/`timeout` (args 5-6) are used by
  *no* engine — those are safe drops. The reduction also has a wrong-rationale history (it
  is neither a "flattened options object" nor a "stubbed unused method," the only two
  simplifications the grand plan licenses — see `2026-06-26-plan2a-review-findings.md` §2a-1).
- **Why not blocking:** in q2, knitr/jupyter are native Rust and don't route through this TS
  API; the two standalone TS extensions (Julia, marimo) don't call `execProcess` at all
  (Julia uses TCP; marimo bypasses the seam with raw `Deno.Command`). So nothing breaks
  *today*. The exposure is future TS engines + the advertised "consumable by Q1 itself"
  portability path (Q1's knitr would break).
- **Recommended seam:** the *plan-compliant* fix is to **flatten** the knobs into
  `ExecProcessOptions` (add `mergeOutput?`/`stderrFilter?`), thread them through
  `PlatformHost.ExecOptions` → `host.process.exec`, and align the vendored
  `@quarto/types` `execProcess` signature. A candidate for the RTQ running plan or Plan 2.

### F2 — preserve-restore: producer built, data carried, consumer dropped, recovery path inconsistently named (Low)

- **Evidence (reviewer-grounded):** the *only* real work in knitr's and jupyter's
  `postprocess` is one call to `quarto.text.postProcessRestorePreservedHtml(options)`
  (`rmd.ts:341`; `jupyter.ts:627`). In q2 that API method is **deferred** (absent from
  `quarto-api/src/text/index.ts`, header note :9-11), and the `engine.postprocess` hook is
  **dropped** (RTQ Level-2: "no post-write DOM stage"). BUT: FC-1 adds `preserve` +
  `post_process` as `#[serde(default)]` **wire carriers** (RTQ:676-677), and **Plan 3 builds
  the preserve *producer*** (`removeAndPreserveHtml`, plan `quarto-jupyter.md:134-136`).
- **Plain language:** this is *not* "dropped-no-seam" (correcting the Opus agent, confirming
  Sonnet). The data seam exists (FC-1 carries `preserve`), and the producer exists (Plan 3).
  What is missing is the *consuming* restore stage, and — the real defect — **no plan
  connects the pieces**, and the two plans that mention the disposition disagree:
  `plan1a-protocol:182` lists `postprocess` as **"Deferred → will add trait + protocol
  message when a caller exists"**, while RTQ Level-2 calls it a **"Drop → recover via an AST
  transform."** Two different recovery stories for the same feature.
- **Why low:** no current engine in scope needs it (Julia/marimo `postprocess` are no-ops;
  built-in knitr/jupyter are native Rust). It is a clean feature-deferral once the recovery
  is named.
- **Recommended action:** pick one recovery story (the AST-transform reading FC-1's
  already-carried `preserve` field is the coherent one) and state it once, cross-linking
  FC-1 ↔ the dropped hook ↔ Plan 3's producer. Owner: RTQ (it owns the `postprocess` drop).

### F3 — RTQ Item A removes gating but doesn't propagate: stale `pandoc` stub label + Plan-2 gating prose (Low; an Item-A execution obligation)

- **Evidence (reviewer-grounded):** RTQ Item A *deletes* the gating model (path/system become
  ambient, `HostState.context` gone, "available pre-launch," lines 152-162). But (a) the 2A
  stub still throws `requiresLaunchContextError("pandoc")` (`system/index.ts:305-307`) — a
  label that becomes a *lie* once there is no launch-context gate, and unlike its siblings
  `checkRender`/`runExternalPreviewServer` (which throw `notYetImplementedError`) it carries
  no "Plan 2" recovery tag; and (b) **Plan 2 still extensively builds and documents the
  gated model RTQ deletes** — "exactly the methods Plan 1b gates" (`quarto-markdown-and-api.md:48`),
  the `init()` jsdoc enumerating "what's gated until launchEngine" (lines 118-120),
  including the **already-debunked** "`format.*` when called without an explicit format
  argument" gate (line 120; the 1b round-2 review established `format.*` is never gated —
  every predicate takes a `Format` arg).
- **Plain language:** the *feature* is covered (Plan 2 Phase A delivers the real
  `system.pandoc`/`path.*` bodies; the marimo-`pandoc` catch is resolved). This is purely a
  **propagation gap**: executing RTQ Item A must also (1) re-label the still-stubbed bodies
  `notYetImplementedError("Plan 2")` and (2) strike Plan 2's gating prose + the dead
  `format.*` gate. This adjudicates the two agents' one divergence — Opus called pandoc
  "adequate" (true of the feature), Sonnet flagged the stub label (true of the current
  state); both are right about different layers.
- **Recommended action:** add these two edits to RTQ Item A's checklist (it already says
  "coordinate with Plan 2's body work," line 216 — make that concrete).

### F4 — `@quarto/types` Phase B (jupyter signatures) should precede Plan 3's jupyter runtime (Low; confirm, don't assume)

- **Evidence (reviewer-grounded):** six jupyter methods drifted between Q1's *live*
  `core/api/types.ts` (what engines call) and the *published* `packages/quarto-types` that
  q2 vendored byte-for-byte (model D.2; verified across all three sources). Plan 2 Phase B
  ("was 2E") is where `@quarto/types` jupyter signatures get refined
  (`quarto-markdown-and-api.md:76-91, 239-242`); Plan 3 builds the *runtime* jupyter
  namespace. The grand-plan dependency graph lists Plan 2 and Plan 3 as **parallel** (both
  gated only on 2A §2aa, no edge between them).
- **Plain language:** if Plan 3 implements the runtime jupyter methods against the stale
  vendored signatures (or against Q1-live without Phase B reconciling the published types),
  the two can disagree. Sonnet flagged this as a hard ordering dependency; I calibrate it
  **softer** — 2A's namespaces carry their *own* types (e.g. `system` has its own
  `ProcessResult`), so Plan 3's jupyter runtime may likewise be self-typed with
  reconciliation deferred, in which case strict ordering isn't required. **Confirm** how
  Plan 3 structures its types before treating this as a blocker.
- **Recommended action:** when reviewing Plan 3, verify it targets Q1's **live** `core/api`
  jupyter shapes (what engines actually call) and note the Phase-B reconciliation point. No
  1a/2A change.

---

## Confirmed adequate — do NOT re-flag (credit to RTQ + 2A)

- **`system.pandoc` (the model's biggest catch — marimo-only):** feature covered by **Plan 2
  Phase A** + RTQ Item A ambient config. (Only the stub label is stale — F3.)
- **`path.runtime/resource/dataDir`:** un-stubbed by RTQ Item A; real bodies in Plan 2 Phase A.
- **Dropped `ExecuteResult` output fields** (`metadata`/`pandoc`/`resourceFiles`/`preserve`/
  `post_process`): carried by **FC-1** as `#[serde(default)]` wire fields. The model's FC-1
  concern is resolved.
- **`partitionedMarkdown` DROP:** reasoned superset-replacement — `plan1a-protocol:182` does
  the **5-callsite homework** (only 2 of 5 pass a real `format`; both subsumed by
  `DocumentProfile` + the `MetadataMergeStage` cascade), cross-linked to the ipynb-filters
  research plan. Not a silent loss.
- **`run?`/`filterFormat?`/`postRender?`/`executeTargetSkipped?`/`ignoreDirs?`/
  `checkInstallation?`:** all classed **defer-infra with a named DQ seam** in RTQ Level-2.
- **`intermediateFiles?`:** present on the wire.
- **`checkRender`/`runExternalPreviewServer`:** 2A stubs with correct "Plan 2" labels; real
  bodies in Plan 2 Phase A. Used only by native-Rust built-ins today.
- **The `@quarto/types` jupyter signature lag:** not mis-claimed as Q1-parity by any 1a/2A
  plan; reconciliation correctly routed to Plan 2 Phase B (F4 is just the ordering note).
- **Claim mechanism:** `LanguageClaim` kind-tagged object is a *documented* q2 extension
  (RTQ Candidate B, design §3.2); `boolean|number → Primary` normalization is sign-clean;
  marimo's numeric score + `firstClass` are handled. No mechanism-fidelity finding.
- **No drift:** every Q1 name/signature/behavior the plans cite that was tested holds.

---

## Calibration notes (for the standing review going forward)

- **The two agents converged** on F1 + F2 independently — high confidence those are real.
  Their **one divergence** (pandoc: "adequate" vs "(C) gap") dissolved on grounding into a
  feature-vs-current-state layering (F3) — neither was wrong; the reconciler's job was to
  see both layers.
- **The reconciler's whole-epic read was load-bearing twice:** (1) it *confirmed* F1 is
  uncovered epic-wide (Plan 2 treats execProcess as done), and (2) it *surfaced* F3's
  Plan-2-stale-gating, which no agent scoped to 1a+2A could see. This validates the division
  of labor: agents stay context-clean on the plan-under-review; the reconciler holds the
  epic and adjudicates the seams *between* plans.
- **Julia-bias thesis: confirmed but largely pre-mitigated.** The epic's own RTQ correction
  ("defer features, not infrastructure") had already retired most Julia-only blind spots.
  The model's value was pinning the *residue* — one param-level escapee (F1) and one
  unconnected feature seam (F2) — not a systemic hole.

## What I read / how I verified
- Both agent reports (recaps + spot-checks of their citations).
- Grounded myself: `rmd.ts:440-458` (execProcess args), `system/index.ts:97-100,300-320`
  (2A signature + stub labels), `plan1a-protocol:175-200` (partitionedMarkdown homework +
  the postprocess "Deferred" disposition), `quarto-markdown-and-api.md` (full — Phase A/B,
  the stale gating prose, execProcess "already real"), `quarto-jupyter.md:134-136` (preserve
  producer), RTQ Item A + FC-1 + Level-1/2 audit (full), model Part B/C/D.
- Prior grounding this session: the engine-API-usage model review (julia/marimo call sites,
  the published-vs-live jupyter drift across three files), the 2A review (config, vendored
  types, namespace ports).
