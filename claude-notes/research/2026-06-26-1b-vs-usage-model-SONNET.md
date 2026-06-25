# Plan 1b vs. All-Five-Engines Usage Model — Sonnet Review
**Date:** 2026-06-26
**Lens:** Does Plan 1b's Deno harness serve what ALL FIVE engines provide/consume, or only the Julia path it was scoped to?

---

## Verdict

**Mostly covered; two genuine new gaps, one confirmation-of-deferred, one precision correction.** The plan is substantively Julia-aware by design and handles the `target()` cookie, the `dependencies()` fold-in, the `intermediateFiles` dispatch, and the `claimsLanguage` numeric score. The two new gaps that are *not* on the exclusion list are:

1. **`dependencies()` in 1b relies on `quarto.jupyter.widgetDependencyIncludes` but that method is missing from q2's runtime** — a jupiter-only API that the fold-in calls at exactly the right moment (step 4) yet has no implementation to call through. The design is correct; the runtime is absent.

2. **The `EngineHostContext` gated-methods table silently includes `format.*`-is-gated language that contradicts the plan's own correction.** A stale list in the "Success Criteria" (line 1422-1426) still claims `format.*` without an explicit format is gated, though the plan body correctly retracted this (lines 831-836). The discrepancy creates an inconsistent contract a future implementer will follow incorrectly.

---

## Essential Questions

### EQ1 — The `data` cookie through `target()`

**Covered.** Plan 1b Phase 2, execute-dispatch step 1 (plan:511-517): "Call `instance.target()` if implemented … using its result (including the opaque `data` cookie like Jupyter's kernelspec) to build the `ExecutionTarget` for `execute()`." Confirmed against Q1 source: jupyter's `target()` writes `data: { transient: true, kernelspec: {} }` then reads back `nb.metadata.kernelspec` into it (jupyter.ts:337-346). The harness passes this through opaquely — no field inspection. The cookie is not on the wire; it lives inside the Deno harness as a JS object in the `ExecutionTarget` handed to `execute()`. Wire type `TsExecuteOptions` carries no `data` field (ts_protocol.rs:296-308), which is correct: the harness reassembles `ExecutionTarget` from the protocol fields, calls `target()` to get or enrich it, then calls `execute()` with the enriched target — all Deno-side. No protocol change needed.

### EQ2 — The `dependencies()` fold-in vs real return shapes

**Partially covered; one runtime gap.**

**Design coverage:** Plan 1b Phase 2, step 4 (plan:651-667) correctly specs calling `instance.dependencies(options)` with the full `DependenciesOptions` object when `engineDependencies` is present, then routing output to `includes` (step 5, plan:669-677). The `DependenciesOptions` construction names all required fields from Q1 `execute/types.ts:201-211`. The split between `includes` (from `dependencies()`) and `htmlDependencies` (from the return field) is deliberate and documented (plan:717-726 and Phase 3 discussion).

**The gap (NEW — not in exclusion list):**

**GAP-1 — `quarto.jupyter.widgetDependencyIncludes` is absent from q2's runtime.** When the harness calls `instance.dependencies()` for a jupyter engine, that method calls `quarto.jupyter.widgetDependencyIncludes(options.dependencies as JupyterWidgetDependencies[], options.tempDir)` (jupyter.ts:610-613 — verified by reading). This is the *only real work* jupyter's `dependencies()` does. q2's `jupyter` namespace is **types-only** — no `quarto-api/src/jupyter/` runtime directory exists (confirmed: model Part D.1). So at runtime: the fold-in fires at exactly the right moment (step 4), constructs `DependenciesOptions` correctly, calls `instance.dependencies()`, which calls `quarto.jupyter.widgetDependencyIncludes(…)`, which hits the absent runtime and throws (or returns undefined). This silently drops widget dependencies for jupyter. Plan 3 must ship `widgetDependencyIncludes` before the `dependencies()` fold-in is functional for jupyter.

**Evidence:** model B.3 (widgetDependencyIncludes: jupyter-only, jupyter.ts:610-613), D.1 (entire jupyter.* namespace: types-only, no runtime dir), plan Phase 2 step 4 (fold-in design is correct).

**Severity:** Medium for now (jupyter is deferred to Plan 3 anyway), but the Plan 3 scope note should explicitly name `widgetDependencyIncludes` as a dependency of the fold-in, not just of the jupyter namespace generally. If Plan 3 ships the namespace without this method the fold-in silently fails.

**knitr-style subprocess-respawning `dependencies()`:** knitr's `dependencies()` in Q1 (rmd.ts:329) does return `{ includes: {} }` — it does not respawn a subprocess. Q1 knitr's subprocess spawning is in `execute()`, not `dependencies()`. The harness step 4 design handles knitr's trivial `dependencies()` correctly.

### EQ3 — The jupyter-namespace assembly seam

**Deferred with a partial seam; confirms D.2 findings already in exclusion list.** Plan 1b Phase 3 `quarto-api.ts` (plan:994-1017) says: "only `jupyter` and the launch-context-dependent method *bodies* may remain `throw 'not yet implemented'` until Plans 2/3." The gating table (plan:816-836) lists `quarto.jupyter.*` conversion logic as "available immediately after `init()`" — present as a structural slot but throwing stubs. Plan 3 (plan:13) is where the runtime is wired. The assembly has a seam: Plan 2A §2aa delivers the host infrastructure; Plan 3 plugs in the `jupyter` namespace. This is not a rip-out situation because the namespace slot exists in the type (quarto-types/src/quarto-api.ts) and the gating table reserves it. Confirmed per model D.1 and D.3.

### EQ4 — `denoHost` (PlatformHost impl) completeness for all-engine consumed set

**Covered for standalone extensions; documented gap for full-surface completeness.** Plan 1b Phase 3 `deno-host.ts` (plan:961-991) correctly notes: "**`denoHost` must implement the full landed interface** (`ts-packages/quarto-api/src/platform/index.ts`), not just the subset sketched below. The sketch omits these required (non-optional) members: `fs.{ensureDir, makeTempDir, makeTempFile, remove}`, `process.{exec …, onExit, exit}`, `env.get`, and `log.*`." The plan is self-aware about the incompleteness of its sketch. The actual `PlatformHost` contract is driven by `@quarto/api`'s interface, not by the plan sketch. For the non-Julia engines' consumed set: `system.execProcess` routes through `process.exec` (the host's structured-exec path), `system.tempContext` needs `fs.makeTempDir`, `system.onCleanup` needs `process.onExit` — all mentioned in the plan's completeness note. Verdict: the design intent is correct; the implementer must follow the completeness note, not the sketch.

**NEW precision note (GAP-2 below) on the gating table inconsistency.**

### EQ5 — Test representativeness

**Julia-shaped; partially representative.** The Phase 0 tests (T1-T7) exercise: metadata partition (T1), MappedString rehydration (T2), dispatch dispatch (T3), markdownForFile serialization (T4), concurrency (T5), cancel (T6), poison/re-launch (T7). The Engine API contract tests exercise `init()`, gating, `format.*`, shared state, idempotency, and `htmlDependencies` forwarding.

**What the contract tests do NOT exercise from the non-Julia consumed set:**
- No test engine calls `quarto.markdownRegex.breakQuartoMd` (jupyter+marimo — model B.1).
- No test engine calls `quarto.jupyter.isPercentScript` (julia+jupyter) or any `jupyter.*` method — though `jupyter` is explicitly deferred to Plan 3.
- No test engine implements `filterFormat?`, `run?`, `postRender?`, or `executeTargetSkipped?` with real cleanup — these are julia-absent members (model C.1). T3 only tests the `intermediateFiles` dispatch arm in passing.
- No test engine returns a numeric score from `claimsLanguage` (marimo-only) — the harness normalization table (plan:449-463) specifies it but T3 only covers the dispatch, not the normalization.

The test engine is thin and Julia-shaped. It does not exercise the jupyter/marimo-consumed API surface. This is consistent with Plan 3 being the jupyter scope, but means **the harness's `claimsLanguage` normalization path (number→priority) has no test seam binding it** — a marimo engine would exercise it and a regression in the normalization would go undetected until Plan 4/marimo validation.

---

## New Findings

### GAP-1 — `widgetDependencyIncludes` is the implicit dependency of the `dependencies()` fold-in

**Plan location:** Phase 2, execute-dispatch step 4 (plan:651-667). The fold-in is correctly designed. However, the step's comment "If the result has `engineDependencies` and the engine implements `dependencies()`" treats `dependencies()` as a black box. Jupyter's `dependencies()` body calls `quarto.jupyter.widgetDependencyIncludes` (jupyter.ts:610-613), which is absent from q2's runtime (model D.1). Plan 3's scope note (plan:13: "Plan 3 Phase 3E (wire jupyter into the harness)") should explicitly state that `widgetDependencyIncludes` must ship as part of Plan 3 for the fold-in to be non-vacuous for jupyter. Without this, Plan 3 can ship a `jupyter` namespace that satisfies its own type tests but silently drops widget deps at fold-in time.

**Recommendation:** Add an explicit seam in Plan 3 Phase 3E: "implement `widgetDependencyIncludes` — required for `dependencies()` fold-in to produce non-empty `includes` for jupyter widget documents."

**Severity:** Medium (jupyter deferred, so no immediate regression; but Plan 3 scope gap).

### GAP-2 — Stale "gated" language in Success Criteria contradicts plan body

**Plan location:** Success Criteria line 1422-1426 (plan near end): "`format.*` without explicit format" is listed among the gated methods. The plan body (lines 831-836) explicitly corrects this: "An earlier draft mislisted `format.*` as gated 'when called without an explicit format argument'; there is no no-arg form, and Q1's own engines always pass the format — `julia-engine.ts:236`, `core/api/types.ts:132`." The Success Criteria was not updated to remove the retracted language. The gating table (plan:816-836) correctly omits `format.*` from the gated set.

**Evidence:** Q1 live: `quarto.format.isHtmlCompatible(options.format)` — always takes an explicit `format` (julia-engine.ts:236). No no-arg form exists. q2 model D.3 confirms `format.*` is real (not gated).

**Recommendation:** Correct the Success Criteria entry: remove "format.* without explicit format" from the list of gated methods. Replace with the correct statement: "`format.*` is NOT gated — it is pure and always takes an explicit `Format`; callable both before and after `launchEngine`."

**Severity:** Low (implementers reading both sections will see the correction; only a single-criteria reader is misled). But it is inconsistent in a plan that gates implementation-contract tests on exact language.

### (Confirmed-in-passing, NOT a new finding) — `claimsLanguage` numeric score normalization is specced but untested

The normalization table (plan:449-463: `number n` → `{kind:"primary",priority:n}`) handles marimo's numeric score (marimo:225-231, model C.3). The table is correctly specced. T3 tests dispatch type per arm but does not include a named revert binding the number-normalization path. This is an observation, not a new finding — the T3 spec does name "parametrize one revert per arm" but numerical normalization is a sub-path within the `claimsLanguage` arm, not a separate arm. Whether this warrants a new bound test for the marimo path is a judgment call for the plan author; flagged without a severity rating since T3's structure would cover it if extended.

---

## What is Covered (confirmed)

- `target()` data cookie: covered (EQ1 — harness-internal, opaque, correct).
- `dependencies()` fold-in design: correct shape, correct field names, correct `DependenciesOptions` construction (EQ2).
- `intermediateFiles` dispatch arm: present in `ToEngine` enum (ts_protocol.rs:77) and in the plan's dispatch table (plan:476).
- `claimsLanguage` numeric score: normalization table present (plan:449-463), covers marimo's `2`/`1` return.
- `firstClass` 2nd arg: wired (`ToEngine::ClaimsLanguage { first_class: Option<String> }`, ts_protocol.rs:52-56).
- `markdownForFile` with `MappedString` return: covered by T4.
- Shared `HostState` / gating on launch: covered by Engine API contract tests.
- Poison→re-launch: covered by T7.
- Cancel targeting: covered by T6.
- `denoHost` completeness note: present (plan:969-974), delegates to the `PlatformHost` interface.
- `jupyter` namespace deferred to Plan 3 with a named seam (plan:13, plan:1009-1017).
- `postProcessRestorePreservedHtml` deferred: confirmed (model D.1), excluded from scope.
- `pandoc_path` on `EngineHostContext` (plan:197 in ts_protocol.rs): present, supports `system.pandoc` unblocking at launch.

---

## What I Read

| file | lines / purpose |
|---|---|
| `claude-notes/research/2026-06-26-engine-api-usage-model.md` | Full model, all five engines, Parts A-D — the lens |
| `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md` | Full plan (1471 lines) — spec under review |
| `crates/quarto-core/src/engine/ts_protocol.rs` | Full wire types (1106 lines) — ground truth for landed types |
| `~/src/quarto-cli/src/execute/types.ts` | Lines 1-220 — Q1 `ExecuteOptions`, `DependenciesOptions`, `ExecuteResult`, both interfaces |
| `~/src/quarto-cli/src/execute/jupyter/jupyter.ts` | Lines 330-360 (target/data cookie), 603-630 (dependencies/widgetDependencyIncludes, postprocess) |
