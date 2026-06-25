# Plan 1b vs the all-engines usage model — reconciled review (+ held 1c findings)

**Date:** 2026-06-26
**Lens:** the all-five-engines consumed/provided ground truth
(`2026-06-26-engine-api-usage-model.md`) — the "latest lens," applied to Plan 1b.
**Inputs:** three independent agent passes (Opus-A, Opus-B, Sonnet — files
`2026-06-26-1b-vs-usage-model-{OPUS-A,OPUS-B,SONNET}.md`), **reconciled and
re-grounded against source by the reviewer**, and **netted against the current
epic state** (RTQ as of 2026-06-26 — now split, with HOST-1/2 → `plan1a-host-bugs.md`,
Plan 5 pooling stub, surface audit → `designs/engine-api-surface.md`; DQ-2; HOST-6).
**Confidence:** 1b findings below are reviewer-grounded. The 1c section is **held /
provisional** (agents interrupted mid-run because 1c is being revised — see that section).

---

## Bottom line (1b)

**Mostly covered — one genuinely-new, well-triangulated finding cluster, whose root is a
scope-reduction.** All three agents independently converged, from three angles, on **the
`dependencies()` fold-in — the one path 1b adds beyond `execute()` — being broken/incomplete
for the deferred-dependencies path.** The deeper diagnosis (1B-DEPS-2): 1b **reduced the Q1
dependencies protocol to inline-only** (hardcoded `dependencies:true` + a dead fold), dropping
the *deferred* path infrastructure. That path is part of the **Q1 engine protocol surface**,
whose first consumer is **single-file book rendering** (many chapter-`execute()`s → one output)
— a capability q2 **will** have, and where q2's speed matters most. Per the epic mandate
("**defer features, not infrastructure**; carry the whole engine API"), the deferred path must
be **built present-and-complete now**, even with zero v1 callers — the same class of finding as
the `execProcess` param drop (RTQ F1). It is *invisible on the Julia validation target* (Julia's
`dependencies()` is a no-op), which is exactly the failure mode this lens exists to catch.
Everything else 1b does for the consumed surface is adequate: the `data` cookie is carried, the
jupyter namespace is a clean deferred-with-seam, and the denoHost is explicitly widened past
Julia-thin.

Two agent findings **net out** against the current epic state (already captured
elsewhere) and are recorded as such below, not as new work.

---

## Findings (reviewer-grounded)

### 1B-DEPS-1 — the fold omits the per-engine `dependencies` array jupyter reads (HIGH)
- **Plan:** step 4 (`plan1b-engine-host-deno.md` ~L651-664) constructs
  `DependenciesOptions` with `target`/`format`/`output`/`resourceDir`/`tempDir`/`libDir`/
  `projectDir` — and **no `dependencies` field**.
- **Q1 (verified):** Q1 passes engine deps to `dependencies()` *only* via that field —
  `render.ts:103` → `dependencies: executeResult.engineDependencies[engineName]`; jupyter's
  `dependencies()` reads exactly it (`jupyter.ts:610`, `options.dependencies as
  JupyterWidgetDependencies[]`). Omitting it sends jupyter's `dependencies()` down its
  false branch → returns `{includes:{}}` → **widget includes silently dropped**.
- **Julia-blind:** julia's `dependencies()` ignores its argument entirely (no-op), so the
  omission never shows on the validation target.
- **Not in RTQ:** DQ-2 establishes the round-trip *exists* ("deferred-but-present; v1 sends
  `true`, engines resolve inline") but says nothing about *constructing* `DependenciesOptions`
  with the per-engine array. New.
- **Action (build now — infrastructure, see 1B-DEPS-2):** thread `engineDependencies[<engine>]`
  into the constructed `DependenciesOptions.dependencies`; bind it with a contract test that
  feeds a non-empty `engineDependencies` (see 1B-DEPS-TEST). This is not a documented seam to
  defer — it is the protocol path books will use; build it complete.

### 1B-DEPS-2 — the dependencies protocol is reduced to inline-only; the deferred path is dropped infrastructure, not a missing feature (MEDIUM→HIGH completeness; scope-reduction)
- **Plan:** sets `ExecuteOptions.dependencies = true` unconditionally, **and** describes the
  `engineDependencies`→`dependencies()` fold. Those are in tension: with `dependencies:true`,
  a Q1 engine resolves inline and returns `includes` directly, leaving `engineDependencies`
  **undefined** — so the fold (step 4, gated on `result.engineDependencies`) is dead code.
- **What actually drives the flag (Q1, verified).** `dependencies` is NOT an engine preference
  and NOT `embed-resources`. It is `options.resolveDependencies`, default **`true`**
  (`render-files.ts:146`); the **only** Q1 caller that sets it `false` is **single-file book
  rendering** (`book-render.ts:136`, `resolveDependencies: isMultiFileBookFormat(format)`). The
  semantic is **rendering topology**: *one output per `execute()`* → inline; *many `execute()`s
  merged into one output* (a single-file PDF/epub book) → **defer**, so all chapters' deps
  resolve **once, together, at the final combined render** where `output` is finally known
  (hence `dependencies()` takes `output: recipe.output`, `render.ts:97`).
- **Why this is a scope-reduction finding, not a design question.** The deferred path
  (`dependencies:boolean`, `engineDependencies`, `dependencies()`) is part of the **Q1 engine
  protocol surface**, not a book implementation detail — books are merely its first *consumer*.
  The epic mandate is "**defer features, not infrastructure** — carry the whole engine API so it
  builds without later protocol surgery." Hardcoding `true` + a dead fold **drops the deferred
  infrastructure**, exactly the Julia/single-doc-shaped reduction this lens exists to catch
  (cf. the `execProcess` param drop, RTQ F1). q2 **will** render books — and q2's speed is most
  valuable on 100-doc projects, where deferred resolve-once is load-bearing — so the retrofit is
  a *when*, not an *if*.
- **Also:** `engineDependencies` is a **map keyed by engine name** (`types.ts:174`), iterated in
  Q1 as `Object.keys(...)` with a possibly-*different* `executionEngine(engineName)`
  (`render.ts:91-103`); the fold must iterate by key, not assume "the engine that just executed."
- **Action — support BOTH paths (inline live, deferred present-and-complete):** make
  `dependencies` a **real wire field** (default `true`, not hardcoded) so a future book renderer
  flips it; **build the fold completely** (per-engine `dependencies` array — see 1B-DEPS-1; map
  iteration; real `output` — see 1B-DEPS-3); bind it with a `dependencies:false` contract test
  (1B-DEPS-TEST) though no production caller sends `false` yet. Only the *feature* (a q2 book
  renderer that sends `false`) is deferred; the protocol is whole. This aligns with DQ-2's
  "deferred-but-present" intent, which the current 1b step-4 fold under-implements.

### 1B-DEPS-3 — `DependenciesOptions.output` is required but has no wire source (MEDIUM)
- **Plan:** step 4 lists `output` among the required `DependenciesOptions` fields, and even
  notes (correctly) that `output` is *not* an `ExecuteOptions` member. But neither
  `TsExecuteOptions` nor `EngineHostContext` carries an output path, so the fold has nothing
  to put there — it won't typecheck, or gets a fabricated value.
- **Action (build now — infrastructure):** add an `output` carrier to the wire. This is the one
  genuinely-missing piece of deferred-path *infrastructure*; it maps to the book's final
  combined-output path (`recipe.output`) when a book renderer lights up `dependencies:false`.
  Do **not** drop the field or fake it — that re-introduces the scope reduction (1B-DEPS-2).

### 1B-DEPS-4 — jupyter's `dependencies()` calls `widgetDependencyIncludes`, absent from q2 runtime (MEDIUM, cross-plan)
- **Q1 (verified):** jupyter's `dependencies()` body calls
  `quarto.jupyter.widgetDependencyIncludes` (`jupyter.ts:610-613`).
- **q2:** the jupyter namespace is **types-only** (no `quarto-api/src/jupyter/` runtime) — Plan 3
  fills it. So even a correctly-constructed fold (1B-DEPS-1/3) has nothing to call through until
  Plan 3 lands `widgetDependencyIncludes`.
- **Action:** name `widgetDependencyIncludes` explicitly in Plan 3E's scope as a fold dependency,
  and cross-link 1b's fold ↔ Plan 3E. (Distinct from the known "jupyter is a 1b assembly stub"
  item — this is about the *specific method* the fold needs.)

### 1B-DEPS-TEST — no contract test exercises a non-empty `engineDependencies` (MEDIUM, binding)
- All three agents note it; it is *why* 1B-DEPS-1/2/3 could ship undetected. The seam tests
  (T1–T7) never feed a non-empty `engineDependencies`, and the `data`-cookie pass-through (EQ1)
  is asserted only in prose.
- **Action:** add a contract test with a fake engine that returns non-empty `engineDependencies`
  and reads `options.dependencies` — the binding revert for 1B-DEPS-1. This single test pins the
  whole cluster.

### 1B-TARGET-SIG — `target()` arity prose imprecision (LOW)
- Plan describes calling `target()` "with the reconstructed MappedString"; the real arity is
  `(file, quiet?, markdown?)` — file-first (`marimo:257`; jupyter is file-driven). Cosmetic; fix
  the prose.

---

## Adequately covered (do NOT touch)
- **The `data` cookie through `target()` (EQ1):** the harness builds/returns the full
  `ExecutionTarget` by reference, so jupyter's `{transient, kernelspec}` and marimo's metadata
  ride through to `execute()`. Correct, and correctly off-wire (Deno-side object). *(Worth one
  contract test per 1B-DEPS-TEST, but the design is right.)*
- **jupyter-namespace assembly seam (EQ3):** a clean deferred-with-seam (Plan 3 wires it), not a
  rip-out. All three agents agree.
- **denoHost / PlatformHost completeness (EQ4):** the plan's self-aware completeness note
  (~L969-974) requires the *full* PlatformHost, not Julia's minimal I/O. Adequate at the spec level.

## Netted out — already captured elsewhere (NOT new 1b work)
- **knitr's re-spawning `dependencies()` vs the execute-only daemon/poison model** (Opus-B
  1B-DEPS-RESPAWN) → **already RTQ HOST-6** ("Q1's `dependencies()`/`run()` also spawn subprocesses…
  revisit poison scope if/when added"). Confirmed, not new.
- **1b success-criteria still lists "`format.*` without explicit format" as gated** (Sonnet GAP-2)
  → folded into the **RTQ gating-removal propagation** (Item A / my B3 stub-relabel+prose-strike).
  A 1b-doc instance of an already-queued cleanup; will resolve with B3. Note, don't re-track.

---

## Held: 1c findings (PROVISIONAL — rerun pending)

The three 1c agents were interrupted because 1c is **being revised** (the `review-1c-extension-integration`
worktree carries a 1134-line revision vs the 1000-line copy the agents read). These are **agent-level,
not reviewer-grounded**, captured here so they survive the rerun and feed the final tally:

- **1c resolution is solid (Sonnet, complete + my own grep baseline):** all four tiers in
  `resolution.rs`, `first_class`/marimo path *tested* (`test_first_class_passed_to_claim`),
  knitr+jupyter ownership + reticulate Interop tested, `handled_languages_for` wired in
  `engine_execution.rs`. Killed-Opus-A had further confirmed **jupyter execute-time
  `handled_languages` enforcement IS landed** (`text_execute.rs`) — refuting the pre-dispatch
  "likeliest gap" hypothesis.
- **1c-GAP-A — content-inspecting `claims_file` deferred/untested (Sonnet GAP-1; MED).** jupyter's
  `isPercentScript(file)` (content branch) + julia's `isPercentScript(file,[".jl"])` are in the
  revised plan's "Future Work: Built-in engine percent/spin script support" (review-1c L1115) — still
  deferred; echo test engine is extension-only. **Persists in the revised plan** — verify on rerun.
- **1c-GAP-B — `KNOWN_ENGINES` still hardcoded in `detection.rs` (Sonnet GAP-3; MED, landed).** Top-level
  YAML key scan only checks knitr/jupyter, so `julia: 1.10` without `engine: julia` won't select julia
  (Q1 scans all registered). Plan Phase-2 item still `- [ ]`; one-line fix (`registry.engine_names()`).
- **1c-GAP-C — test representativeness (killed-Opus-B trend; LOW-MED).** Numeric/multi-engine/`first_class`
  claims are tested against **Rust mock registries**, never through the **TS-engine wire**; the only TS
  E2E (echo) is boolean-Primary. The non-boolean claim path is unexercised end-to-end.
- **Adjacent, already captured:** the **preview-registry gap** (preview runs `engine_registry: None`,
  `preview.rs:216`) is **RTQ R5** (a plan1c registry-ownership finish), and Plan 5 (pooling) builds on it.

**Rerun guidance:** re-review 1c against the **revised 1134-line** plan on `review-1c-extension-integration`,
finish the two interrupted Opus passes, and reviewer-ground GAP-A/B/C. GAP-B is the most concrete (landed
limitation); GAP-A persists in the revision; GAP-C is the Julia-shaped-test-double signature.

---

## What I read / how I verified
- The three 1b agent recaps + grounded the deps-fold cluster against `plan1b-engine-host-deno.md`
  step 4 (~L640-664), Q1 `execute/types.ts` (`DependenciesOptions:201-211`, `ExecuteResult.engineDependencies:174`),
  `jupyter.ts:610`, `render.ts:91-103`.
- Current epic state: `plan1a-host-bugs.md`, `plan5-engine-host-pooling.md`, RTQ (DQ-2, HOST-6, R5,
  the surface-audit extraction to `designs/engine-api-surface.md`), review-1c structure.
- Prior baseline: `resolution.rs` grep (tiers, `first_classes` map, `handled_languages`), the engine
  call-site surveys (julia/marimo/knitr/jupyter), the usage model.
