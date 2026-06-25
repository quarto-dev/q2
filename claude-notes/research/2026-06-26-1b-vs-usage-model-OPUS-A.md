# Plan 1b vs. all-five-engines usage model — review (OPUS-A lens)

**Lens:** does Plan 1b's harness spec serve the consumed/provided surface of
**all five** engines (knitr, jupyter, markdown, julia, marimo), or only the
Julia path it was scoped to? Grounded in the usage model
(`2026-06-26-engine-api-usage-model.md`), the plan
(`2026-04-16-plan1b-engine-host-deno.md`), the landed wire types
(`crates/quarto-core/src/engine/ts_protocol.rs`), and Q1 **live** source
(`~/src/quarto-cli`, `~/src/quarto-julia-engine`, `~/src/quarto-marimo`).

## Verdict

**Mostly covered, with two genuinely-new, non-Julia-shaped gaps in the
`dependencies()` fold-in — both decisive for jupyter and (transitively) any
widget-emitting engine, neither named by the exclusion list.** The `data`-cookie
carry (EQ1) is **adequate**. The harness's per-engine queue / poison / cancel
spine, the test seam set, and the `htmlDependencies` return-based wiring are
representative, not Julia-shaped. But the `dependencies()` fold (EQ2) — the
single fold the plan adds beyond `execute()` — is specified against the *wrong
argument shape*: it omits the one field Q1 actually uses to pass engine deps
(`DependenciesOptions.dependencies`), and requires an `output` field that does
not exist anywhere on the wire. Both bite jupyter/julia-with-widgets and would
make the fold a no-op or a type error if implemented as written.

---

## FINDING DEP-FOLD-1 (new) — the `dependencies()` fold omits `DependenciesOptions.dependencies`, the only channel Q1 uses to pass engine deps

**Severity: HIGH (correctness — silently drops widget deps for jupyter/julia).**

**Plan location** (step 4, lines 651–667):
> "Construct that object with all of Q1's **required** fields
> (`execute/types.ts:201-211`): `target` … `format` … `output`, `resourceDir`
> … and `tempDir` …; plus the optional `libDir` … and the minimal `projectDir`
> shim. Note `target` and `resourceDir` are mandatory and were easy to miss …"

The plan enumerates `DependenciesOptions`' required fields but **the
constructed object never carries `dependencies`** — the array of raw engine
deps. It is mentioned nowhere in step 4.

**Q1 evidence (verified):** Q1's render loop is the authority on how
`engineDependencies` reaches `dependencies()`:

- `src/command/render/render.ts:91-105` — when `executeResult.engineDependencies`
  is present, for each engine it calls
  `engineInstance.dependencies({ target, format, output, resourceDir, tempDir,
  projectDir, libDir, dependencies: executeResult.engineDependencies[engineName],
  quiet })`. The **`dependencies:` field is the payload** — `engineDependencies`
  is `Record<string, Array<unknown>>`, and the per-engine `Array<unknown>` is
  handed in as `DependenciesOptions.dependencies`.
- `src/execute/jupyter/jupyter.ts:607-624` — jupyter's `dependencies()` reads
  **exactly that field**: `options.dependencies as JupyterWidgetDependencies[]`,
  passes it to `quarto.jupyter.widgetDependencyIncludes(...)`, and returns the
  resulting `inHeader`/`afterBody` includes. With `options.dependencies` unset,
  `if (options.dependencies)` is false and `dependencies()` returns **`{ includes: {} }`** — an empty result.
- `DependenciesOptions.dependencies?: Array<unknown>` is declared at
  `execute/types.ts:209`.

**The gap in plain language:** the plan's fold says "if the result has
`engineDependencies` and the engine implements `dependencies()`, call it now"
(line 651–652) — but then constructs the `DependenciesOptions` **without the
engine-deps array**. If the harness builds the object as the plan literally
lists it, jupyter's `dependencies()` sees `options.dependencies === undefined`,
takes the false branch, and returns `{ includes: {} }`. **The widget includes
the whole fold exists to materialize are silently dropped.** This is not a
Julia problem (julia's `dependencies()` ignores its options and returns `{}`
anyway — `julia-engine.ts:147-152`), which is exactly why a Julia-only lens
would not catch it: the fold "works" against julia because julia's
`dependencies()` is a no-op. It only does real work for jupyter (and any
widget-emitting q2-native engine), and only when fed the array.

**Why `dependencies: true` on the execute side does not save it.** The plan
sets `ExecuteOptions.dependencies = true` unconditionally (lines 619–631),
which routes jupyter into the `resultIncludes()` inline branch
(`jupyter.ts:556-560`) instead of the `engineDependencies` branch
(`jupyter.ts:561-569`). So for the *normal* q2 path, `engineDependencies` is
**empty** and the fold never fires — meaning the fold is currently dead code
for the very engine that motivates it. Either: (a) the fold is vestigial and
should be deleted (and the plan should say so), or (b) it is meant to handle a
real case, in which case it must carry `dependencies`. The plan can't have it
both ways: it spends a step folding `dependencies()` while simultaneously
guaranteeing (via `dependencies: true`) that the input which triggers the fold
is never produced. Resolve the contradiction.

**Recommended seam:** decide the fold's purpose explicitly. If kept: add the
per-engine raw-deps array to the constructed `DependenciesOptions.dependencies`,
sourced from `executeResult.engineDependencies[engineName]` (mirroring
`render.ts:103`) — and then `dependencies: true` must NOT be set unconditionally
for engines that should defer, or the array is always empty. If dropped: state
that q2 relies solely on the inline `resultIncludes()` path (`dependencies:
true`) and remove the fold, documenting that an engine needing the deferred
two-phase deps resolution is unsupported (this is already half-acknowledged at
lines 629–631 "an engine that depends on the deferred-deps behavior cannot be
driven that way from q2").

---

## FINDING DEP-FOLD-2 (new) — `DependenciesOptions.output` (required) has no wire source

**Severity: MEDIUM (the fold, if implemented as written, is a type error or fed a fabricated value).**

**Plan location** (step 4, line 657):
> "Construct that object with all of Q1's **required** fields … `target` …
> `format` … **`output`**, `resourceDir` ← `EngineHostContext.resource_dir`,
> and `tempDir` …"

**Q1 evidence (verified):** `DependenciesOptions.output: string` is a
**required** field (`execute/types.ts:204`, no `?`). In Q1's render loop it is
sourced from `recipe.output` (`render.ts:98`) — the actual Pandoc output path.
jupyter's `dependencies()` happens not to *read* `output`
(`jupyter.ts:607-624`), but it is a mandatory field of the type, so omitting it
is a TypeScript error at the call (the plan itself flags this risk for `target`
and `resourceDir` at line 663–664: "omitting them is a type error at the call").

**The gap:** the wire carries **no output path**. `TsExecuteOptions`
(`ts_protocol.rs:294-308`) has `input`, `source_path`, `temp_dir`, `cwd`,
`lib_dir`, … but no `output`. `EngineHostContext` (`ts_protocol.rs:191-200`)
has none either. So the harness cannot source `DependenciesOptions.output` from
anything real — it would have to fabricate a value (e.g. `""`) to satisfy the
type, or the call won't typecheck. The plan's own framing ("all of Q1's
required fields") obscures that one of those required fields is unsatisfiable
from the protocol. (`output` *is* correctly excluded from `ExecuteOptions` per
the plan's note at lines 645–648 — but that same reasoning means it's missing
for `DependenciesOptions` too, and the plan does not notice the asymmetry.)

**Recommended seam:** if the fold survives DEP-FOLD-1, either add an `output`
field to `TsExecuteOptions` (q2 knows the output path at execute time) or
document that the harness passes a placeholder because no Q1 engine's
`dependencies()` reads `output` (true today for jupyter/julia/marimo — verified)
and pin that as a named assumption that a future Q1 sync must re-check.

---

## EQ1 — the `data` cookie through `target()`: ADEQUATE (confirm-in-passing)

The harness-internal `target()` fold **does** carry the opaque `data` cookie.
Plan step 1 (lines 511–518): "Use its result (including the opaque `data` cookie
like Jupyter's kernelspec) to build the `ExecutionTarget` for `execute()`." And
the dedicated `target()` bullet (lines 739–745) repeats it. This matches Q1:
jupyter's `target()` stores `data: { transient, kernelspec }`
(`jupyter.ts:342,345,353`) and `execute()` reads it back via
`(options.target.data as JupyterTargetData).kernelspec` (`jupyter.ts:450`).
Because the harness keeps `target()` entirely Deno-side and passes the whole
returned `ExecutionTarget` into `execute()` in the same process, the cookie is
never serialized and round-trips by reference — correct, and it covers julia's
kernelspec assignment (`julia-engine.ts:212-216`) and marimo's
`target.metadata["external-env"]`/`["pyproject"]` reads
(`marimo-engine.ts:287-288`, populated by marimo's own `target()` via
`extractYaml`, `marimo-engine.ts:263-268`). **No gap.** Minor: the plan calls
out only `data`, never `metadata`; since the harness forwards the full
`ExecutionTarget` object the `metadata` field rides along regardless, so this is
cosmetic, not a defect.

---

## EQ2 — the `dependencies()` fold vs real return shapes: TWO GAPS (DEP-FOLD-1, DEP-FOLD-2 above)

The fold is specified against an incomplete/incorrect `DependenciesOptions`. On
the **re-spawn-a-subprocess** sub-question (knitr-style `dependencies()` that
shells out): not applicable to the TS-engine surface — knitr/jupyter are Rust-
native in q2 and never run through this harness, and the two *standalone* TS
engines (julia, marimo) both have no-op `dependencies()`
(`julia-engine.ts:147-152`, `marimo-engine.ts:389-393`). So there is no TS
engine today whose `dependencies()` re-spawns; the harness's `await
instance.dependencies(...)` is async and would tolerate one if it appeared. The
real risk is purely the argument-shape gap, not the subprocess shape.

---

## EQ3 — the jupyter-namespace assembly seam: ADEQUATE (deferred-with-seam, not rip-out)

1b assembles the QuartoAPI over Q1's nine namespaces with `jupyter` left
throwing "not yet implemented" pending Plan 3E (plan lines 1013–1017, 1296–1300).
This is a clean deferral, not an assembly Plan 3 must tear out: the
state-machine builder `buildQuartoAPI(state, host)` (lines 994–1006) constructs
*every* namespace closure uniformly, and `jupyter`'s body is simply a stub
swapped for the real one later — same object identity, same wiring. The Julia +
jupyter **output slice** (`toMarkdown`, `assets`, `resultIncludes`,
`resultEngineDependencies`) is precisely the slice the model (Part D.2) confirms
Q1's published types *match* live, so 3E fills a stub against a stable contract.
No structural rework implied. **No gap** — though note DEP-FOLD-1 means the
`resultIncludes`/`resultEngineDependencies` *consumers* on the q2 side are
mis-wired regardless of whether `jupyter.*` itself is stubbed or real.

---

## EQ4 — denoHost (PlatformHost impl) completeness: ADEQUATE (already widened beyond the sketch)

The plan's `denoHost` *sketch* (lines 975–992) is Julia-thin, but the plan
**explicitly** says the sketch is incomplete and lists the additional required
members the real impl must cover (lines 968–974): `fs.{ensureDir, makeTempDir,
makeTempFile, remove}`, `process.{exec (structured ExecOptions/ExecResult),
onExit, exit}`, `env.get`, `log.{info,warning,error}`. That set covers the
all-engine consumed I/O: `system.execProcess` (knitr/jupyter — Rust-native, but
the host method must exist for any q2-native engine), `system.tempContext`
(knitr — needs `makeTempDir`), `system.onCleanup` (jupyter — needs
`process.onExit`), `console.*` (julia/marimo — needs `log.*`), and
`mappedString.fromFile` (universal — needs `fs.readTextFileSync`). The known
`system.pandoc` stub and `execProcess` param-reduction are excluded-list items
(model Part D), not re-reported here. **No new gap** — the impl is scoped to the
landed `PlatformHost` interface, not to Julia's minimal I/O.

---

## EQ5 — test representativeness: ADEQUATE, with one note

The Phase-0 seam tests are representative, not Julia-shaped doubles:

- **T1** exercises the metadata partition with the `pdf-standard` cross-list
  discriminator and the nested-bin peel — a real Q1 `metadataAsFormat` parity
  test, not an echo. Verified the `pdf-standard`-in-both-lists claim is the
  right discriminator (it distinguishes render-first from pandoc-first ordering).
- **T2** exercises `MappedString.map` with `file_offset ≠ start` (so a no-op
  `.map` fails) + the `source: None` tolerance + the `closest` scan — this is the
  julia-consumed path (`julia-engine.ts:644`'s `line.map(0, true)`), the most
  load-bearing MappedString use across engines.
- **T3/T5/T6/T7** drive the **real** dispatch loop over an in-memory duplex with
  test-double engines — concurrency, per-engine serialization, cancel, and
  poison→relaunch. These are spine tests; a broken consumed surface in
  `execute()` would surface through them.
- The **return-based `htmlDependencies`** test (lines 952–957) asserts relative-
  path normalization against `lib_dir` — the one q2-native deviation.

**Note (not a finding):** no Phase-0 test exercises the `dependencies()` fold
end-to-end with a *non-empty* `engineDependencies` array. That absence is
exactly why DEP-FOLD-1/2 could ship undetected — the contract-test engine in T3
sends `execute` but the seam set never feeds an engine that returns
`engineDependencies` and implements `dependencies()`, so the fold's argument
construction is unguarded. **Recommend** adding a fold-binding test (a test
engine that returns `engineDependencies` + a `dependencies()` that asserts it
received `options.dependencies`) as the binding revert for the DEP-FOLD-1 fix.

---

## What is adequately covered (confirm-in-passing)

- `data`-cookie carry through target() (EQ1) — correct, covers jupyter/julia/marimo.
- jupyter-namespace deferral seam (EQ3) — clean, no rip-out.
- denoHost completeness (EQ4) — widened past the Julia-thin sketch in-plan.
- `supporting`/`filters` field forwarding (step 7) — correct and matches Q1's
  `supporting` figure-dir semantics (verified against `jupyter.ts:592`); this is
  an exclusion-list item, confirmed-in-passing only.
- `htmlDependencies` return-based, no shared-state accumulator — correct under
  concurrent Pass-2 (exclusion-list item).
- Phase-0 seam tests T1–T7 — representative (EQ5).

---

## What I read / how I verified

| source | what I extracted / verified |
|---|---|
| `claude-notes/research/2026-06-26-engine-api-usage-model.md` (full) | Parts A/B/C/D; the all-five consumed/provided ground truth; the Julia-bias ledger |
| `claude-notes/plans/2026-04-16-plan1b-engine-host-deno.md` (full, 1471 lines) | execute dispatch + `target()` fold (steps 1–8) + `dependencies()` fold (step 4) + Engine API contract + gating table + Phase-3 denoHost/quarto-api spec + Phase-0 seam tests + success criteria |
| `crates/quarto-core/src/engine/ts_protocol.rs` (full) | landed wire types: `TsExecuteOptions` (no `output`), `EngineHostContext` (no `output`), `TsExecuteResult`, `TsHtmlDependency`, envelope/cancel |
| `~/src/quarto-cli/src/execute/types.ts` (full) | `DependenciesOptions` (`:201-211`, `output` required `:204`, `dependencies?` `:209`); `ExecutionTarget.data`/`.metadata` (`:138-146`); `ExecuteResult` (`:166-178`) |
| `~/src/quarto-cli/src/execute/jupyter/jupyter.ts` (`target()` :274-358, `execute()` :438-601, `dependencies()` :607-624) | `data:{transient,kernelspec}` write+read; `resultIncludes` vs `resultEngineDependencies` branch on `options.dependencies`; `dependencies()` reads `options.dependencies as JupyterWidgetDependencies[]` |
| `~/src/quarto-cli/src/command/render/render.ts:80-111` | the authoritative `engineDependencies → dependencies({ …, output: recipe.output, dependencies: engineDependencies[engineName] })` wiring |
| `~/src/quarto-julia-engine/src/julia-engine.ts:147-266` | julia `dependencies()` is a no-op `{ includes:{} }`; kernelspec assignment; same resultIncludes/engineDependencies branch as jupyter |
| `~/src/quarto-marimo/src/marimo-engine.ts:253-393` | marimo `target()` populates `metadata` via `extractYaml`; `execute()` reads `target.metadata[...]`; `dependencies()` no-op |
