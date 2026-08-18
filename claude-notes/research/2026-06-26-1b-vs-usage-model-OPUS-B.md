# Plan 1b vs. the all-five-engines usage model — review (OPUS-B lens)

**Lens:** Does Plan 1b's Deno harness — which ASSEMBLES the QuartoAPI, PROVIDES
`denoHost`, and folds `target()`/`dependencies()` into the execute round-trip —
serve what **all five** engines provide/consume, or only the Julia path it was
scoped to? Plan reviewed: `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md`.
Wire types: `crates/quarto-core/src/engine/ts_protocol.rs` (landed).
Q1 read live at `~/src/quarto-cli`; standalone engines at
`~/src/quarto-julia-engine`, `~/src/quarto-marimo`. Harness TS is **spec-only**
(no `ts-packages/quarto-engine-host-deno/src/`; `dist/engine-host-deno.js` is a
2-line placeholder) — so this reviews the plan's spec + the landed wire shape it
must serialize to.

## Verdict

**Mostly covered, with two genuinely-new, non-Julia-shaped gaps — one of them
load-bearing.** The plan's `target()` `data`-cookie fold (EQ1) is **adequate** —
it explicitly carries the opaque cookie (good, jupyter/julia-safe). The
**`dependencies()` fold-in (EQ2) is shaped to Julia's inline-resolve and gets
the actual return shape *structurally wrong*** for the two engines that use the
deferred path (jupyter, knitr): Q1's `engineDependencies` is a **map keyed by
engine name**, iterated by `render.ts`, and **knitr's `dependencies()` re-spawns
an R subprocess** — neither the wire nor the fold-in models this. That is
finding **1B-DEPS-MAP** (HIGH) and **1B-DEPS-RESPAWN** (MED). A smaller
representativeness gap (EQ5) and the `target()` signature mismatch (EQ1b) round
it out. None of the four is named by the exclusion list (which covers the
*fold-in policy* and ExecuteResult field-routing, but not the **shape** of
`engineDependencies` itself, nor the knitr re-spawn, nor the `target()` arg
list).

---

## Findings

### 1B-DEPS-MAP — `engineDependencies` is a per-engine *map*, not a flat list; the fold-in + wire model a single inline resolve (HIGH)

**Plan location.** Phase 2 "Execute dispatch with dependencies folding" step 4
(plan line 651-652):
> "**If the result has `engineDependencies` and the engine implements
> `dependencies()`, call it now**, on the TS side, before responding."

and the `dependencies: true` rationale (plan line 619-631):
> "`dependencies: true` — set unconditionally. q2 always wants dependencies
> materialized inline: this forces Jupyter's `executeResultIncludes()` path …
> instead of the deferred `engineDependencies` map, which the harness then
> composes with Q1's `dependencies()` resolution per step 4."

**Q1 evidence (verified by me).**
- `ExecuteResult.engineDependencies?: Record<string, Array<unknown>>` —
  `~/src/quarto-cli/src/execute/types.ts:174`. It is a **map keyed by engine
  name**, each value an opaque array (the per-engine deferred-deps slice).
- Q1's render loop iterates that map and calls the **named engine's** `launch()`
  + `dependencies()` once per key, passing *that key's slice* as
  `DependenciesOptions.dependencies` —
  `~/src/quarto-cli/src/command/render/render.ts:91-103`:
  ```ts
  if (executeResult.engineDependencies) {
    for (const engineName of Object.keys(executeResult.engineDependencies)) {
      const engine = executionEngine(engineName)!;
      const engineInstance = engine.launch(engineProjectContext(context.project));
      const dependenciesResult = await engineInstance.dependencies({ …,
        dependencies: executeResult.engineDependencies[engineName], … });
  ```
- jupyter writes the map keyed by `kJupyterEngine` only when `!options.dependencies`
  — `jupyter.ts:555-567` (`engineDependencies = { [kJupyterEngine]: dependencies }`).
- `DependenciesOptions.dependencies?: Array<unknown>` — `types.ts:208` — is the
  **slice for one engine**, not the whole map.

**The gap in plain language.** The plan's step 4 says "if the result has
`engineDependencies` … call `dependencies()`" and passes the **whole**
`engineDependencies` — but Q1's contract is: `engineDependencies` is a *map*, and
`dependencies()` is fed **`engineDependencies[thisEngineName]`** (the unwrapped
array). The plan never names this unwrap. More fundamentally: the **wire has no
`engineDependencies` field at all** — `TsExecuteResult` (ts_protocol.rs:315-321)
carries only `markdown / supporting / filters / includes / html_dependencies`.
The plan's `dependencies:true` choice (force inline `includes`, never emit the
map) makes that *correct for jupyter/julia*, since with `dependencies:true`
jupyter sets `includes` and leaves `engineDependencies` undefined
(jupyter.ts:556-560 — the `if (options.dependencies)` branch). **But step 4's
prose still describes folding the map** — it is internally inconsistent with the
`dependencies:true` decision, and if a TS engine *does* return
`engineDependencies` (the plan even adds a stderr-drop fallback at line 736-737),
the harness has no map-keyed unwrap to feed `dependencies()` correctly. This is
a Julia-blind spot: Julia's `dependencies()` is a pure inline stub returning
`{includes:{}}` (`julia-engine.ts:147-152`), so the Julia-only lens never
exercises the *map* shape that jupyter and knitr actually produce.

**Severity: HIGH.** Step 4's fold-in is the one place the plan claims Q1 parity
on a multi-engine surface and gets the data shape wrong. It is latent under
`dependencies:true` (jupyter/julia never hit it), but the prose will mislead the
implementer, and any engine that returns the deferred map gets mis-fed.

**Recommended seam.** Resolve the step-4 prose to match the `dependencies:true`
decision: either (a) drop the map-fold entirely and document that q2 always
takes the inline path (engines that return `engineDependencies` get the
existing stderr-drop), or (b) if the deferred path is kept as a fallback,
specify `dependencies()` is called per map key with `engineDependencies[key]`
unwrapped — never the whole map. Pick (a); it matches `dependencies:true`.

### 1B-DEPS-RESPAWN — `dependencies()` may re-spawn a subprocess (knitr), but the fold-in assumes an inline TS resolve (MED)

**Plan location.** Phase 2 step 4 calls `dependencies()` "on the TS side, before
responding" (plan line 652) and step 4 describes "The engine writes any required
widget files to `lib_dir` and returns `DependenciesResult`" (plan line 664-667)
— framing it as a local, inline TS resolve.

**Q1 evidence (verified by me).** knitr's `dependencies()` does **not** resolve
inline — it re-enters the R subprocess via `callR<DependenciesResult>("dependencies", …)`
(`~/src/quarto-cli/src/execute/rmd.ts:329-337`). That is a second round-trip to a
daemon, distinct from `execute()`'s. Julia's and marimo's `dependencies()` are
trivial inline stubs (`julia-engine.ts:147-152`, `marimo-engine.ts:389-393`) —
again the Julia/marimo lens hides the re-spawn case.

**The gap in plain language.** EQ2's second half: does the fold-in "handle an
engine whose `dependencies()` re-spawns a subprocess (knitr-style), not just an
inline Julia resolve?" The fold-in *calls* `engine.dependencies(options)` and
awaits it, so a re-spawning `dependencies()` would technically work — **but the
plan's cancellation/poison model is built only around `execute()`** as "the only
daemon-engaging request" (plan line 336-338: "Poison the instance when the
cancelled request was an `Execute` (the only daemon-engaging request)"). A
knitr-style `dependencies()` re-spawn is a *second* daemon engagement that runs
**inside the harness's `execute` handler** (folded in), invisible to the
per-request `AbortController`/poison machinery, and not separately cancellable.
For q2's validation target (Julia) this never bites; for a future knitr/jupyter
TS port it means a `dependencies()` subprocess hang is uncancellable and
un-poisonable.

**Severity: MED.** No current consumer (knitr/jupyter are native Rust in q2,
Julia's is inline), so it cannot bite the validation target — but it is a real
all-engine surface the daemon/cancel model silently doesn't cover, and the plan
asserts the fold-in is Q1-faithful without noting the re-spawn class.

**Recommended seam.** Add one sentence to the step-4 fold-in noting that a
re-spawning `dependencies()` (knitr-class) runs under the *same* request's
`AbortController` as its `execute()` — so cancel/poison covers it transitively —
or, if it must be independently cancellable, that it needs its own request id
(deferred until a re-spawning TS engine appears). Naming it closes the lens gap.

### 1B-TARGET-SIG — harness `target()` call passes only a MappedString; real signature is `(file, quiet?, markdown?)` (LOW)

**Plan location.** Phase 2 step 1 (plan line 651-655) and the `target()`
harness-internal bullet (plan line 739-745):
> "it calls it with the reconstructed MappedString, and uses the returned
> `ExecutionTarget`"

**Q1/engine evidence (verified by me).** The real `target()` signature is
`(file: string, quiet?: boolean, markdown?: MappedString)` — marimo
(`marimo-engine.ts:257-261`) and julia both take **`file` first**, with the
MappedString as the optional **third** arg. jupyter's `target` reads `file` and
calls `quarto.path.dirAndStem(file)`, `isQmdFile(file)`, writes the transient
notebook keyed off `file` (`jupyter.ts:311-345`) — it is **file-driven**, not
markdown-driven. A harness that "calls it with the reconstructed MappedString"
(and not `file`) would break every real `target()`.

**The gap in plain language.** The plan's prose describes the call as passing the
MappedString, omitting the `file` (source path) and `quiet` args that the actual
engines read first. This is almost certainly a prose imprecision (the harness has
`source_path` on `TsExecuteOptions` and would pass it), but as written it does not
match the all-engine `target()` arity — a Julia-shaped sketch (Julia's `target` at
`julia-engine.ts:298` is also file-first; the plan just under-describes it).

**Severity: LOW.** Spec imprecision, not a wire/shape defect; easily corrected
when `src/host.ts` is written. Flagged because the lens is "Julia-shaped vs
representative," and this call-site sketch is shaped to "MappedString in, target
out" rather than the real file-first arity.

**Recommended seam.** Fix step-1/`target()` prose to: call
`instance.target(source_path, quiet, reconstructedMappedString)`.

### 1B-TEST-ECHO-THIN — the contract-test engine consumes a Julia-shaped slice; no jupyter/`data`-cookie/re-spawn coverage (LOW)

**Plan location.** Phase 0 tests T1-T7 + the Engine-API-contract tests. The
execute-path test double (T3/T5/T6/T7) is "a minimal test engine" whose `execute`
"blocks on a controllable deferred" (plan line 166-179); the return-based
`htmlDependencies` test (plan line 952-957) returns one entry.

**Evidence.** Across T1-T7 and the contract tests, no test engine: (a)
implements `target()` returning a `data` cookie and asserts the cookie reaches
`execute()` (EQ1's contract is *described* in the plan but **not bound by any
test**); (b) returns `engineDependencies` (the map) to exercise the step-4
fold-in unwrap; (c) re-spawns from `dependencies()`. The pure/format/path
namespaces are touched (contract tests), but the *execute round-trip's*
non-Julia-shaped surfaces (data cookie, deferred-deps map) are asserted by
nothing — a broken `data`-cookie fold or a wrong map-unwrap would pass the whole
suite.

**Severity: LOW** (it is a test-coverage gap on surfaces that are themselves
findings 1B-DEPS-MAP / EQ1; once those resolve, add the binding test).

**Recommended seam.** Add one execute-path test where the test engine implements
`target()` → `{…, data:{kernelspec:"foo"}}` and `execute()` asserts
`options.target.data.kernelspec === "foo"` (binds EQ1's cookie pass-through,
currently prose-only). This is the single highest-value missing bind.

---

## What is adequately covered (confirm-in-passing)

- **EQ1 — `data` cookie through `target()`: COVERED.** Plan step 1 (line 651-655)
  and the `target()` bullet (line 739-745) **explicitly** carry "the opaque
  `data` cookie like Jupyter's kernelspec" into the `ExecutionTarget` built for
  `execute()`. Verified Q1: jupyter's `target` stores `data:{transient, kernelspec}`
  (`jupyter.ts:337-346`, confirmed verbatim), `ExecutionTarget.data?: unknown`
  (`types.ts:144`). The fold does the right thing — the cookie is opaque and
  forwarded, not dropped. (Only gap: it isn't test-bound — see 1B-TEST-ECHO-THIN.)
- **EQ3 — jupyter-namespace assembly seam: ADEQUATE (deferred-with-seam).** The
  plan defers the `jupyter` namespace bodies to Plan 3 but keeps the
  Julia+jupyter *output slice* (`toMarkdown/assets/resultIncludes/
  resultEngineDependencies`) real via §2aa, and the assembly wiring (gating on
  `state.context`) is 1b's deliverable (plan line 1007-1017, 1287-1300). This is a
  seam Plan 3 fills, not one it rips out. The D.2 signature-drift is already on
  the exclusion list (model Part D) — confirmed not re-reported here.
- **EQ4 — denoHost completeness: ADEQUATE.** Plan line 968-974 **explicitly**
  requires `denoHost` implement the *full* landed `PlatformHost` interface
  (`fs.{ensureDir,makeTempDir,makeTempFile,remove}`, structured
  `process.exec`/`onExit`/`exit`, `env.get`, `log.*`) and flags the sketch as
  incomplete — i.e. it is *not* shaped to Julia's minimal I/O. The known
  execProcess-param reduction is on the RTQ exclusion list; the host *impl*
  breadth is called out adequately.
- **ExecuteResult field routing** (supporting/filters/includes/metadata/
  resourceFiles/preserve/postProcess) — already on the exclusion list (RTQ FC-1);
  the landed `TsExecuteResult` (ts_protocol.rs:315-321) matches the plan's
  deliberate subset. Confirmed, not re-reported.

---

## What I read / how I verified

| source | what I extracted / verified |
|---|---|
| `claude-notes/research/2026-06-26-engine-api-usage-model.md` (full) | Parts A/B.3/B.6/C/D — the all-engine consumed/provided ledger |
| `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md` (full, both pages) | Phase 0-4, execute-dispatch flow, fold-in steps 1-8, QuartoAPI contract, denoHost sketch |
| `crates/quarto-core/src/engine/ts_protocol.rs` (full) | landed wire: `TsExecuteResult` has NO `engineDependencies` field; `EngineHostContext`; `TsExecuteOptions` |
| `~/src/quarto-cli/src/execute/types.ts:143-211` | `ExecutionTarget.data?:unknown` (:144); `ExecuteResult.engineDependencies?: Record<string,Array>` (:174); `DependenciesOptions.dependencies?: Array<unknown>` (:208) |
| `~/src/quarto-cli/src/execute/jupyter/jupyter.ts:300-346,550-596` | target() stores `data:{transient,kernelspec}` verbatim; `engineDependencies={[kJupyterEngine]:…}` only in the `!options.dependencies` branch |
| `~/src/quarto-cli/src/command/render/render.ts:80-115` | render iterates `engineDependencies` map by key, calls named engine's `dependencies()` with `engineDependencies[engineName]` slice |
| `~/src/quarto-cli/src/execute/rmd.ts:329-337` | knitr `dependencies()` **re-spawns R** via `callR<DependenciesResult>("dependencies",…)` |
| `~/src/quarto-julia-engine/src/julia-engine.ts:147-263` | julia `dependencies()` inline `{includes:{}}` stub; `execute()` output slice |
| `~/src/quarto-marimo/src/marimo-engine.ts:257-420` | `target(file, quiet?, markdown?)` signature; inline `dependencies()` stub |
| `ts-packages/quarto-engine-host-deno/` | harness is **spec-only**: no `src/`; `dist/engine-host-deno.js` = placeholder |
