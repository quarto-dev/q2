# Plan 1a: Return to Q1 — correct the landed 1a host/engine surface

> **STATUS (2026-06-29): RTQ code items COMPLETE on `feature/ts-engine-extensions`.** All six
> RTQ checkboxes landed + reviewed (per-task + opus whole-branch review = READY TO MERGE): ENG-2
> (`831e761b0`), Item A protocol split (`08386d678`), §2aa B3/B3b (`3a36be5bf` + `665e81d77` doc
> sweep), ENG-1 (`9c87b9b45`), FC-1 (`deb5bb0f1`), FC-2 (`38e5963f2`). Workspace 10446 passed;
> `cargo xtask verify --skip-hub-build --skip-hub-tests` clean. The ONLY unchecked box is the
> **deferred** q2 render-orchestrator consumer (FC-2, "lands with the book feature" — plan1c). All
> harness halves remain plan1b's; §2aa real bodies + global param remain Plan 2 Phase A's. Not yet
> pushed (awaiting user approval per GIT PUSH POLICY).

**Created:** 2026-06-25
**Lives on:** `feature/ts-engine-extensions` (the epic's integration line, which holds the landed
`ts_protocol.rs`/`ts_process.rs`/`ts_engine.rs`/`dependency.rs` and the developed sub-plans). The
plan's **code** items are implemented as `braid/<id>-<slug>` topic branches off the integration
line, merged `--no-ff`. See "Execution model" below.
**Surfaced by:** two review passes — the Plan 1b pre-implementation review
(`claude-notes/research/2026-06-24-plan1b-review-findings.md`) and a 2026-06-25
protocol/host/engine review batch.
**Scope:** `plan1a-protocol`, `plan1a-host`, `plan1a-engine` and their **landed code**
(`ts_protocol.rs`, `ts_process.rs`, `ts_engine.rs`, `dependency.rs`), plus the one `@quarto/api`
edit that Item A's gate-removal **forces** (the §2aa stub relabel — B3's code half). **Out of
scope, owned elsewhere (see "Plan boundary" below):** the *harness* halves of Item A / FC-1 /
FC-2 / ENG-1 — owned by **plan1b**, which already carries them as first-class work items + tests —
and the standalone `system.execProcess` knob restoration (**B1**), owned by **Plan 2**. RTQ
corrects **landed code**; it does not own work on the **unwritten** 1b harness.

## Why this plan exists — return Q1 divergences to Q1 shape + framework completeness

q2 follows the Quarto-1 engine API as closely as possible, deviating *only* with an explicit,
documented q2-forced reason. The TS-engine plans were drafted partly from an imprecise model of
Q1 (and partly under a since-superseded serial execution model), so the "frozen" 1a surface
picked up shapes that diverge from Q1 without a forcing reason. This plan is **single-mission**:

**Return Q1 divergences to the Q1 shape + carry the full engine-API framework** — where 1a
drifted from the Q1 engine API with no forcing reason (e.g. ENG-1's tier placement; the
`EngineHostContext` ambient/launch split, Item A), restore the Q1 shape; where the q2 wire can't
*carry* a Q1 capability, add the (deferred) framework seam (FC-1, the infrastructure items).

The q2-bug-fixing work that this plan once also owned — q2-introduced defects in the landed 1a
host/engine implementation with **no Q1 analogue** (HOST-1 crash-stderr attribution, HOST-2 cache
poisoning) — has been **carved out to `claude-notes/plans/2026-06-26-plan1a-host-bugs.md`** (it is
independent of return-to-Q1 and runs first). This plan no longer carries those bugs.

It is a *running* plan — add items (Q1 regressions or missing framework seams) as more are found.

**Principle — defer features, not infrastructure.** The q2 wire is the **framework between q2 and
engine features**, not a Julia-shaped subset. The original 1a protocol was scoped to what the
Julia render path needs (claim → load → launch → markdownForFile → execute → intermediateFiles)
and omitted the rest of the Q1 engine surface — **an original-planning error this plan corrects**
(the Julia scoping made an excellent skeleton, but the framework must carry the *whole* engine
API). We may defer a *feature* (no q2-side implementation yet), but the *protocol infrastructure*
for it should exist wherever Q1 exposes an engine method/field, so the full engine API can be
built against q2 **without later protocol surgery**. The **Surface coverage audit** (extracted to
`claude-notes/designs/engine-api-surface.md`) walks every Q1 engine method/field and classifies it
present / build-infra-now / defer-infra / justified-drop. Genuinely out of scope (not even infra) are surfaces q2's architecture makes
**impossible or redundant** — `populateCommand` (cliffy subcommands into a Rust CLI),
`partitionedMarkdown` (pampa parses qmd natively), `postprocess` (no post-write DOM stage; the
No-DOM-postprocessor rule) — each recorded with its reason.

## Methodology (apply to every item)

1. **Read the Q1 source directly** — not subagent-quoted line numbers. (The 1b review
   learned this the hard way: quoted lines confirm a symbol exists; they don't show how a
   value flows through the pipeline.)
2. Check the **q2 wire** type (`ts_protocol.rs`) and the **q2 core** type (`quarto-core`).
3. Decide: **forward as Q1** / **adapt with a documented q2-forced reason** / **drop with
   reason**.
4. **Tests first** (TDD): write or adjust the binding test (with a named revert hunk)
   before the production change; freeze it once green.
5. Record **which sub-plan(s) and which landed file(s)** change.

## Execution model

- **This plan and all the code it targets live on `feature/ts-engine-extensions`** — the integration
  line. All file/line references are to that line. (RTQ was authored as a plan-only `review/1b`
  branch; it now lives on the integration line, so there are no cross-branch steps left.)
- **Implementation:** a separate agent implements the **code** items as `braid/<id>-<slug>` topic
  branches off the integration line, merged `--no-ff` (worktrees convention).
- **Suggested execution order** (one logical change per commit). *The doc-only items
  (PROTO-1/2/3, HOST-3/4/5/6, the ENG-2/B1/B3 doc-halves) are folded into their target docs — see
  Documentation reconciliation — so what remains is code:*
  1. **ENG-2** — `dependency.rs` **doc-comment fix only** (both dedup arms are already test-bound;
     **no new test** — see ENG-2 and the E2 seam).
  2. **ENG-1 + Item A, coordinated** — both make **structural** edits to `ts_protocol.rs`
     (`LoadEngineResult`/`LaunchEngineResult` and the `Init`/`LaunchEngine` split); sequence them
     together so the two protocol edits land coherently, with `ts_process.rs` + the `ts_engine.rs`
     **test helpers** updated in the same change. (The harness halves are **plan1b's** — see Plan
     boundary; not part of this change.)
  3. **FC-1, FC-2** — the remaining **wire** seams (the protocol/`context.rs`/`ts_engine.rs` halves
     only; the harness halves are plan1b's — see Plan boundary). **FC-1** (carried `TsExecuteResult`
     fields) and **FC-2** (the `dependencies` verb + `engineDependencies` + the `dependencies`
     options flag) *also* edit `ts_protocol.rs`, but **additively** (`#[serde(default)]`) — so they
     land cleanly *after* the structural ENG-1 + Item A change. (Four items touch the protocol file
     in all.) **B1** is **not** here — it is a standalone `@quarto/api` correction owned by Plan 2.
- **D (`TsExecuteResult → ExecuteResult` mapping) is already implemented** on feature
  (`ts_engine.rs:489-505`: `supporting→supporting_files`, `filters` forwarded, `includes`/
  `html_dependencies` translated, `needs_postprocess` hardcoded `false`). It is **verify-only** —
  confirm it matches FC-1's field-disposition (FC-1 wire-feeds `post_process` and carries the
  deferred fields); no new code.

## Plan boundary (Option A split — 2026-06-29)

RTQ began as "corrections to the landed 1a surface" but had grown a *harness* leg on its
cross-cutting items (Item A, FC-1, FC-2, ENG-1). That leg was never a correction: **the Deno
harness (plan1b) is unwritten** — a placeholder bundle, no `src/` — so there is no harness code to
correct, only harness *design* to specify. Specifying it from RTQ created a second source of truth,
listed harness tests (T-A1/T-A2/T-A4, F2b) that cannot be mounted until 1b exists, and inverted the
dependency (a cheap corrections plan gated on building a subsystem). The split fixes this:

- **plan1b owns every harness half**, and **already carries it** as first-class work items + tests
  (it cites "RTQ Item A / FC-1 / FC-2" as provenance): the ambient `Init { global }` / no-gating
  model + the `path`/`system` build, the discovery static-field emission (`generatesFigures`/
  `canFreeze`/`quartoRequired`), the FC-1 carried-field forwarding (with `post_process` wire-fed,
  not hardcoded), and the FC-2 `dependencies` pass-through + `engineDependencies` forwarding. The
  harness-behaviour tests **T-A1 / T-A2 / T-A4** (plan1b "Engine API contract" Phase) and **F2b**
  (plan1b deferred-deps wire seam) live there, where they can actually run.
- **Plan 2 owns B1** — the standalone `system.execProcess` `mergeOutput`/`stderrFilter` restoration
  (`@quarto/api` + vendored `@quarto/types`). It touches no 1a file and is testable now against a
  fake `PlatformHost`; it belongs with Plan 2's still-open `@quarto/api` work (Phase A bodies +
  Phase B type reconciliation).
- **RTQ keeps:** the landed-Rust corrections (Item A's `ts_protocol.rs` split, ENG-1's
  `LoadEngineResult`/`LaunchEngineResult` field moves, FC-1/FC-2's additive wire fields + the
  `ts_engine.rs` mapping, ENG-2's `dependency.rs` doc-comment) **plus the one §2aa edit Item A's
  gate-removal forces** (B3's code-half stub relabel — it cannot land correctly until Item A removes
  the gate, so it stays coupled here). RTQ's remaining test seams are therefore **Rust-only**
  (E1, E2, F1, F2a) + the Rust-serde T-A3.

The dependency now reads cleanly: **RTQ (landed corrections, lands first) → plan1b (harness
implementation, now spec-complete and Q1-faithful from birth) → plan1c (wiring) → end-to-end
verification.** RTQ closes when its Rust edits land; the harness work is not gated on RTQ staying
open.

---

## Decomposition & decisions (2026-06-26)

This plan was split in the 2026-06-26 session. Decisions:

- **Split into:** (1) **this plan** — Q1-fidelity + framework completeness; its **code** items are
  the landed-Rust corrections Item A/DQ-7 (protocol split), ENG-1, ENG-2, FC-1/2 (wire halves) + the
  Item-A-forced B3 stub relabel (the harness halves are **plan1b's** and **B1** is **Plan 2's** —
  see Plan boundary; the doc-only review items PROTO-1/2/3, HOST-3/4/5/6, B2, B4 are folded into
  their target docs — see Documentation reconciliation); (2)
  **`plan1a-host-bugs.md`** — HOST-1/2 carved out (q2-introduced bugs, no Q1 relation, **independent,
  run first**); (3) **`plan5-engine-host-pooling.md`** — preview re-compute warmth (post-4 capstone).
- **The HOST-1 / HOST-2 entries have been removed from this plan** — they now live in
  `plan1a-host-bugs.md` (q2-introduced bugs, no Q1 relation, independent, run first).
- **Surface coverage audit + DQ-1…7 records → extracted** to `claude-notes/designs/
  engine-api-surface.md` (alongside the consumed-surface model from B4); the **DQs are resolved**
  (2026-06-29). This plan keeps the actionable code items + a pointer.
- **Preview-registry wiring — owned by plan1c (R5), not an open gap.** `q2 preview` runs
  built-ins-only until the preview capture path reads `ProjectContext.registry` instead of the `None`
  override (`preview.rs:216`, `capture_driver.rs`, `engine_execution.rs:110-115`). This is **plan1c's
  R5 execution item** ("Finish the registry move into the preview capture path"); landing it wires TS
  engines into preview (and is Plan 5's prerequisite). Tracked there, closes there — nothing for RTQ
  to do.
- **Decided:** B1 = restore-now, but **relocated to Plan 2** by the 2026-06-29 Option A split (see "B1 — … MOVED to Plan 2" below); B2 = AST-transform recovery reading FC-1's `preserve`
  (apply); **`can_freeze` folds into ENG-1's discovery-tier move**; DQ-7 = `project`→`LaunchEngine`
  (landed in Item A below). Plan 5 scope = preview-only / single-project, session-scoped /
  transport-abstracted (WASM-future), **measure respawn cost first**.

---

## Item A — `EngineHostContext`: ambient global config + per-instance launch context

**Status:** ready to execute — the centerpiece refactor. Sequence its `ts_protocol.rs` edit
**with ENG-1** (both change the protocol); see "Execution model" above.
**Touches (RTQ-owned):** plan1a-protocol (`ts_protocol.rs`), plan1a-host (`ts_process.rs`), Plan 2A
§2aa (`@quarto/api` path/system factories + the B3/B3b stub-relabel & comment fixes). *The harness
QuartoAPI builder / gating removal is **plan1b's** (consumer — see Plan boundary), not Item A's.*

### The Q1 shape (read directly)

- The Q1 `quarto` API is a **stateless singleton over process globals**, built once via
  `globalRegistry`. The whole path namespace (`core/api/path.ts:18-39`) is ambient:
  `runtime: quartoRuntimeDir`, `resource: (...parts) => resourcePath(parts…)`,
  `dataDir: quartoDataDir`, `absolute: normalizePath`. `resourcePath(resource?)`
  (`core/resources.ts:20`) and `quartoRuntimeDir(subdir?)` (`core/appdirs.ts:23`) take
  **no context** — they read process-level globals (`QUARTO_ROOT`, appdirs). So in Q1,
  `path.runtime`/`resource`/`dataDir` (and the pandoc path) are **never gated and never
  per-render**.
- Per-render state in Q1 is the **project context**, threaded to
  `engine.launch(context: EngineProjectContext)` (`execute/types.ts:86`) and captured in
  the **instance's closure** — *not* on the `quarto` API. API methods never read
  `projectDir`.

### The q2 regression

`EngineHostContext` **conflated two different concerns** and put both on a shared mutable
API slot:
1. **Process-global config** — `resourceDir`, `runtimeDir`, `dataDir`, `pandocPath`,
   `isInteractiveSession`, `runningInCI`, `quartoVersion`. (Q1: ambient globals on the API.)
2. **Project context** — `projectDir`, `isSingleFile`. (Q1: goes to `launch()` → instance
   closure.)

It delivers the whole bundle on **`LaunchEngine`** — this part is **landed** (`ts_protocol.rs:41-44`).
The harness design (plan1b — **specified, not yet written**) then stashes it in **one shared
`HostState.context`** and **gates the API on it** ("first `launchEngine` unblocks the gated methods
for *all* loaded engines"); the **landed** `@quarto/api` launch-context stubs
(`requiresLaunchContextError`, B3) are that gate's user-visible surface. So the regression is
**part-landed** (the wire shape + the stubs) and **part-design** (the slot + gating in the unwritten
harness) — Item A corrects both. The shared mutable slot, the retroactive "unblock-all," and the
gating have **no Q1 basis**. Two latent hazards (both
masked today by the one-subprocess-per-render invariant): (1) engine B can resolve
`path.runtime` against engine A's context once any engine launches; (2) divergent contexts
would silently first-win in release.

The root cause: everything in `EngineHostContext` is actually **subprocess-stable** (one
Deno subprocess per project render — single-doc renders use the same orchestrator), so
"deliver once, ambient" is the honest model. "Set on first launch, gate until then" was an
awkward encoding of an ambient value.

### Target design (Option A — Q1-faithful; DQ-7 resolved)

Split the two concerns the way Q1 does — **and put each on the frame whose lifetime it matches:**

1. **Process-global config → `Init`, once per subprocess.** The harness receives
   `resourceDir`/`runtimeDir`/`dataDir`/`pandocPath`/`isInteractive`/`isCI`/`quartoVersion` on a
   one-time **`Init`** frame at spawn (O-A1). The `@quarto/api` `path`/`system` factories close over
   it, so `path.runtime`/`resource`/`dataDir`/`system.pandoc` resolve **immediately** — like Q1's
   `resourcePath()`/`quartoRuntimeDir()`. **No gating, no `HostState.context`.** This config is
   **process-stable** — set once when the subprocess is born.
2. **Project context → `LaunchEngine`, per render (DQ-7).** The `EngineProjectContext`
   (`projectDir`, `isSingleFile`, + `config`/`outputDir` per DQ-5) rides **`LaunchEngine`** and is
   captured in the **instance closure** — pure Q1 (`engine.launch(EngineProjectContext)`). It is
   **render-scoped**, not process-stable. *(Earlier Item A drafts put project context on `Init` too,
   justified by "one-subprocess-per-render"; **DQ-7 moves it to `LaunchEngine`** — equally simple
   today, strictly more Q1-faithful, and the **enabler for reusing one subprocess across renders**:
   `Init` sets the process-stable globals once; each render's `launchEngine` carries that render's
   project context. See **"Subprocess reuse across renders"** below.)*

The load-bearing line: **`Init` = process-stable global config; `LaunchEngine` = render-scoped
project context.** The engine-facing API stays exactly Q1; only the **wire** changes.

### What this removes (net simplification)

- `HostState.context` shared slot — **deleted**.
- The "gated until `launchEngine`" mechanism — **deleted** (the gated column goes empty;
  `path`/`system` are ambient, available pre-launch).
- The §2aa "requires launch context" stub sequencing for `path.runtime`/`resource`/
  `dataDir`/`system.pandoc` — **gone** (they need the ambient config, injected at harness
  assembly, not a launch context).
- The gated-method error-contract and several gated-method tests — **replaced** by
  "available pre-launch" assertions.
- Both latent hazards — **gone** by construction.

### Phase 0 — tests first (each bound to a named revert)

RTQ's Item A test is the **Rust-serde** one (T-A3 below). The three **harness-behaviour** tests —
**T-A1** (path/system ambient pre-launch), **T-A2** (`LaunchEngine` carries the project context;
`launch()` receives it), **T-A4** (no shared cross-engine context slot) — are **owned by plan1b**
(its "Engine API contract" Phase, which already carries them verbatim with their named reverts).
They mount the Deno harness and can only run once 1b exists, so they are not RTQ checkboxes; see
**Plan boundary**.

- [x] **T-A3 — protocol round-trip; fields on the right frame.** `ToEngine::Init { global }` and
  `ToEngine::LaunchEngine { engine, project }` round-trip through serde with **`global` on `Init`,
  `project` on `LaunchEngine`**. *Named revert:* move `project` onto `Init` (or the whole bundle onto
  `LaunchEngine`) → the field-placement assertion RED. *(Rust `cargo nextest`.)*

### Phase 1 — changes by layer

- [x] **plan1a-protocol (`ts_protocol.rs`).** Split `EngineHostContext` into two carriers on two
  frames: **`ToEngine::Init { global: HostGlobalConfig }`** sent **once immediately after spawn**,
  before any `loadEngine` (the harness must receive it before serving `path`/`system`); and
  **`ToEngine::LaunchEngine { engine, project: EngineProjectContext }`** carried **per render**.
  (Was: the whole `EngineHostContext` on every `LaunchEngine` + gating.) Both new structs follow the
  unprefixed lifecycle-type convention (like `EngineHostContext`/`LoadEngineResult`) and stay
  **`serde_json::Value`-free** — the `config` blob uses **`TsMetadataValue`** (the same loose-JSON
  carrier `TsFormatInfo.metadata` uses), per the file's no-`Value` rule (see the `Request`
  doc-comment). Exact field lists (all `#[serde(rename_all = "camelCase")]`):

  ```rust
  /// Process-stable config, delivered once on `Init` at spawn (ambient — never gated).
  pub struct HostGlobalConfig {
      pub resource_dir: String,
      pub runtime_dir: String,
      pub data_dir: String,            // NEW — absent from today's EngineHostContext; required so
                                       // `path.dataDir` is ambient (Q1 `quartoDataDir`). Non-optional:
                                       // the producer (1c) sources it via `quarto_util::quarto_data_dir()`
                                       // (a leaf 1c must add — see 1c Phase 2); RTQ tests pass a literal.
      pub pandoc_path: Option<String>,
      pub is_interactive_session: bool,
      pub running_in_ci: bool,
      pub quarto_version: String,
  }

  /// Per-render project context, carried on each `LaunchEngine`, captured in the instance closure
  /// (pure Q1 `engine.launch(EngineProjectContext)`).
  pub struct EngineProjectContext {
      pub project_dir: Option<String>,
      pub is_single_file: bool,
      /// DQ-5: the project `engines` settings + `output-dir` config key, as values (not a callback).
      /// Loose JSON → `TsMetadataValue`, NOT `serde_json::Value`.
      pub config: Option<std::collections::HashMap<String, TsMetadataValue>>,
      /// DQ-5: Q1's `getOutputDirectory()` return, as a value (not a callback).
      pub output_dir: Option<String>,
  }
  ```

  The old `EngineHostContext` struct is **deleted** (its project fields → `EngineProjectContext`, its
  global fields → `HostGlobalConfig` + the new `data_dir`). Update the round-trip tests.
- [x] **plan1a-host (`ts_process.rs`).** Send `Init { global }` **once at `ensure_started`** (right
  after spawn), **fire-and-forget** — reuse the existing raw `WriteTransport::send` path (the same
  no-slot send `Shutdown` uses with a throwaway `id: u64::MAX`, `ts_process.rs:255-261`), **not**
  `request()`; no pending slot, no response (see O-A1). `TsEngineHost` holds the process-stable
  `global`. Change `launch_engine` to take a **`project: EngineProjectContext`**; the `TsEngine`
  instance gains a stored `Option<EngineProjectContext>` (a setter / constructor param) that
  `ensure_launched` reads and passes to `launch_engine`, captured by the existing **launched-instance
  cache** (`self.instance`, `ts_engine.rs:225-230`) at first launch. *(Why the instance must hold it
  rather than receive it per-method: `ensure_launched` is the lazy launch trigger, fired from
  `markdown_for_file` / `intermediate_files` too — **not only `execute`** — and those two carry **no
  `ExecutionContext`**, so the project cannot be threaded from a method `ctx`. It must be **set on the
  instance at engine-selection / render-setup**, before the first instance method runs. `claims_*` do
  **not** trigger launch — they use `ensure_loaded` (`ts_engine.rs:746`). RTQ ships the field + setter
  + the new `launch_engine` signature, and its tests set a literal project; the **production setter
  call site + the render-boundary cache reset** (so the next render re-captures its own project) are
  owned by **1c Phase 2**, verified Q1-consistent there.)* **Demux note:** `Init` is
  **non-engine-addressed**, so it needs arms in `engine_name_for`/`operation_name_for`
  (`ts_process.rs:951,966`) and any other exhaustive `ToEngine` match — the demux *logic* is
  unchanged, but the new variant is not engine-routed and carries no pending slot. (**Teardown
  changes under Subprocess reuse, below**.)
- **plan1b (harness) — owned by plan1b, not RTQ (see Plan boundary).** The harness half (build
  `path`/`system` ambient from `Init { global }`; no `HostState.context`, no gating; build the full
  Q1 `EngineProjectContext` for `engine.launch()` from `LaunchEngine.project` per render, incl. the
  inert harness-local `fileInformationCache` and the Plan-3E transient-notebook forward-note) is a
  first-class work item in **plan1b** ("Engine API contract" / Phase 2–3), already reworked to the
  ambient model. Nothing for RTQ to do here beyond the protocol split above.
- **Plan 2A §2aa real bodies (`@quarto/api` `path`/`system`) — owned by Plan 2 Phase A / plan1b,
  NOT RTQ (see Plan boundary).** Giving `makePathHost(host)` (`path/index.ts:127`) and
  `makeSystem(host)` (`system/index.ts:161`) an ambient `global` config param (resource/runtime/data
  dirs, pandoc path) and making `path.runtime`/`resource`/`dataDir` + `system.pandoc` **read it as
  real bodies** is **Plan 2 Phase A's body work**; the `global` is injected by the **plan1b** harness
  at assembly. **RTQ does not add the param or the bodies.** RTQ's *entire* §2aa scope is **B3 + B3b
  below** — the gate-removal stub relabel + the stale-comment fixes, i.e. "the one §2aa edit Item A's
  gate-removal forces" per the Plan boundary. Until Plan 2 lands those bodies, the four stubs **stay
  throwing** (relabelled — see B3). *(This bullet was a stale `- [ ]` RTQ checkbox that contradicted
  B3; rewritten 2026-06-29 to attribute the bodies to Plan 2 / plan1b and remove the contradiction.)*
- [x] **(B3 — gating-removal propagation, code).** The four launch-context stubs still throw
  `requiresLaunchContextError(...)` — `path.runtime`/`resource`/`dataDir`
  (`ts-packages/quarto-api/src/path/index.ts:168,172,176`) and `system.pandoc`
  (`system/index.ts:306`). That label becomes *false* once Item A removes the gate, and it is
  inconsistent with the sibling stubs (`checkRender`/`runExternalPreviewServer`,
  `system/index.ts:312,318`) that throw `notYetImplementedError(...)`. Re-label all four to
  `notYetImplementedError("Plan 2")` as part of Item A's §2aa edit. *(Verified 2026-06-26.)*
  *(The B3 **doc** half — striking Plan 2's "gated until `launchEngine`" prose and the dead
  `format.*` gate — is folded into `quarto-markdown-and-api.md`; see Documentation reconciliation.)*
- [x] **(B3b — stale `PlatformHost` comments, §2aa source; Item-A-forced like B3).** Update the
  now-overruled docstrings on `platform/index.ts` — `env.get` (`:85-87`) and `realPath` (`:107-112`)
  both still say they back the deferred `path.runtime`/`path.dataDir` bodies "to locate Quarto's
  share/data dirs / the binary's own directory." Those bodies stay **deferred** under RTQ (the stubs
  keep throwing per B3); only **Plan 2 Phase A** will make them read the injected
  `global.runtimeDir`/`dataDir`. So RTQ's edit is to neutralize the over-promising comments (do not
  assert a specific deferred body), not to wire the ambient model. **Decision deferred to
  Plan 2 Phase A:** `env`/`realPath` have **no production caller** (only the interface decl + a
  fake-host test), so they look **vestigial** post-Item-A — but removing them from `PlatformHost` is
  an interface change rippling to the 1b `denoHost` + fakes, and Plan 2's `system.pandoc` body *may*
  still want `realPath` to canonicalize the pandoc path. Plan 2 Phase A (which writes those bodies)
  makes the keep-or-remove call; RTQ only fixes the stale comments now. *(Surfaced by the Plan 2
  reviewer, 2026-06-29; verified against source.)*

### Open questions (O-A) — resolved

- **O-A1 — startup-config delivery. RESOLVED: `Init` frame** (structured, reuses serde, on-protocol;
  over spawn env/argv, which is stringly-typed for `isSingleFile`/`quartoVersion`). It carries the
  **`global`** config only. **Delivery is fire-and-forget** (like `Shutdown`): `Init` rides no
  pending slot and has **no `FromEngine::Initialized` ack**. Ordering is guaranteed by the ordered,
  single-threaded stdio stream — the harness handles `Init` before the first `loadEngine` because the
  stream is ordered and the reader is single-threaded — so `ensure_started` sends `Init` and proceeds
  without blocking on a response.
- **O-A2 — per-launch / per-engine context? RESOLVED (DQ-7): yes.** Project context rides
  `LaunchEngine`, per render — captured in the instance closure (pure Q1). This is no longer a
  "reserved extension point"; it is the default, and it is the **enabler for subprocess reuse across
  renders** (the `Init` global stays process-stable; each render's `launchEngine` carries its own
  project context). See **Subprocess reuse across renders** below.

---

## Subprocess reuse across renders (q2-forward upgrade enabled by DQ-7)

**What this is.** A q2-forward upgrade beyond Q1's single-process CLI: reuse **one Deno subprocess
across many renders** instead of spawning one per render. **DQ-7 (project → `LaunchEngine`) is its
enabler**, and it makes reuse **safe by construction**: the `Init` `global` config
(resource/runtime/data dirs, pandoc, version, interactive/CI) is **process-stable** — set once for
the life of the q2 process — so a reused subprocess only takes a fresh `project` on each render's
`launchEngine`. There is **no ambient-config hazard to guard**. The full design + scope are **owned
by Plan 5** (`2026-06-26-plan5-engine-host-pooling.md`); RTQ only unblocks it via DQ-7.

**Key caveat (audited 2026-06-26): q2 runs NO Deno in production today.** `TsEngine` is constructed
**only in tests**; the production `EngineRegistry` registers native-Rust engines only (markdown,
knitr, jupyter — `registry.rs:74-81`), and both `q2 preview` (`preview.rs:216`) and `q2 render`
(`render.rs:652`) pass **no** custom registry. The TS-engine subprocess is **this epic's
deliverable**, not yet wired into any production path (plan1c is the unimplemented integration). So
subprocess reuse is an **integrated-future** optimization, not current behavior.

**Scope is Plan 5's, and it is narrow.** Plan 5 scopes pooling to the one case that matters —
**preview re-compute in a single project**, owning the warm host at *session* scope behind
`EngineTransport`. It **decides** there is no cross-project case (a q2 process never opens multiple
projects), so the cross-project machinery once sketched here — render-boundary signals, per-render
state reset, stashed-context invalidation, eviction/LRU, the julia transport-key audit — is **not
applicable** (Plan 5 §Scope) and has been removed from this plan. Plan 5 also gates itself on a
**measure-first** check (the kernel already survives respawn via its transport file, so pooling saves
only Deno-spawn + module-`import()`) and on the **plan1c R5** preview↔registry wiring.

**RTQ's part.** Only the **DQ-7 split** is RTQ's — it lands in **Item A** (Q1-faithful, and the
safe-by-construction enabler). The pooling implementation is **Plan 5** (post-Plan-4, low-priority).
DQ-7 keeps the door open and safe.

---

## Candidate items (B/C/D) — resolved by the 2026-06-25 review

The three original candidates were run to ground against Q1 source and the landed code in
the review batch below. Outcomes:

- **B — `claimsLanguage` return shape — NOT a regression (confirmed, closed).** Q1 is
  `boolean | number` (`execute/types.ts:56`); the `LanguageClaim` kind-tagged object
  (`primary`/`interop`/`fallback`) is a *deliberate* q2 extension (engine-resolution.md §3.2),
  reachable only via the object form, with `boolean|number` always normalizing to `Primary`
  (no sign games). The reviewer's Faithful list confirms the normalization is sign-clean and
  additive. No parity claim is over-stated. Nothing to do.
- **C — `store_html_dependencies` dedupe — reclassified: IMPLEMENTED.** No longer
  "unimplemented" — `dependency.rs::store_html_dependencies` has landed with a by-name
  first-wins guard + `DiagnosticMessage::warning`. The 1b review's 2026-06-24 "no dedup" note
  is stale. The residual is doc-scoping only → folded into **ENG-2** below.
- **D — `TsExecuteResult → ExecuteResult` mapping (Rust side) — IMPLEMENTED; verify-only.**
  The mapping landed at `ts_engine.rs:489-505` (`supporting→supporting_files`, `filters`
  forwarded, `includes`/`html_dependencies` translated, `needs_postprocess` hardcoded `false`).
  It already matches PROTO-1's field-disposition table — the residual is a one-line *verification*
  against that table (now folded into **FC-1** — see Documentation reconciliation); the
  `generates_figures` tier move is **ENG-1**.

---

## Items from the 2026-06-25 protocol/host/engine review batch

A second reviewer pass over the landed surface (`ts_protocol.rs`, `ts_process.rs`,
`ts_engine.rs`, `dependency.rs`) + the three sub-plans, evaluated against Q1 source read
directly. The **code** items from this batch — **ENG-1** and **ENG-2** — are below as execution
steps (checkboxes); the **doc-only** items (PROTO-1/2/3, HOST-3/4/5/6) were folded into their target
docs and are catalogued in *Documentation reconciliation* immediately below. Items use the
reviewer's IDs for traceability. All references target `feature/ts-engine-extensions` (see
"Execution model" above), so there are **no cross-branch follow-ups**.

## Documentation reconciliation — doc-only items folded out (not execution items)

These review items were **documentation bookkeeping** (edit a sibling plan/design doc), not code.
They have been folded into their target docs — by the 2026-06-29 consolidation pass and the folds
below — so they are **no longer execution items** in this plan. RTQ's remaining checkboxes are
code-only. The item IDs are retained here so existing `RTQ §<ID>` references (e.g. from the 1a/2a
plans' correction notes) still resolve.

| ID | What | Target doc | Status |
|---|---|---|---|
| PROTO-1 | `TsExecuteResult` field disposition — the dropped Q1 fields are now **carried** (`#[serde(default)]`); the `ts_protocol.rs` doc-comment is documented as part of **FC-1**'s code change | FC-1 (below) | folded → FC-1 |
| PROTO-2 / ENG-3 | `quarto.htmlDependency()` is a per-`Execute` value-constructor returned on `html_dependencies`, not a "registration API" | plan1a-protocol / plan1a-engine correction notes | done (consolidation) |
| PROTO-3 | `TsHtmlDependency` (3 fields) mirrors q2's own `HtmlDependency` (`pampa/src/lua/quarto_doc.rs`), not Q1 `FormatDependency` (~10). A parity pass is a q2-side `pampa::lua` `HtmlDependency` widening, **not** this epic. | (recorded here) | record-only |
| HOST-3 | cached launched-instance is stateless (cache invariant) | `engine-host-concurrency.md` | folded |
| HOST-4 | daemons must be spawned detached (plan1b harness contract; violation = silent lost warmth) | `engine-host-concurrency.md` | folded |
| HOST-5 | q2 never reads/writes/keys on engine transport files | `engine-host-concurrency.md` | folded |
| HOST-6 | poison scope = "the only daemon-engaging request **v1 issues**" | `engine-host-concurrency.md` | folded |
| ENG-2 (doc half) | content-gated dedup policy described in the plan prose | plan1a-engine correction note | done (consolidation) |
| B1 (doc half) | `execProcess` `mergeOutput`/`stderrFilter` caveat | `quarto-markdown-and-api.md` (Plan 2) | folded |
| B3 (doc half) | strike "gated until launchEngine" prose + the dead `format.*` gate | `quarto-markdown-and-api.md` (Plan 2) | folded |
| FC-2 (cross-plan) | capture/replay must cover the `dependencies` round-trip | `2026-05-03-replay-engine.md` | folded |
| B2 | one `postprocess` recovery story (AST transform reading `preserve`) | RTQ "Surface coverage audit" + plan1a-protocol | done |
| B4 | name the consumed-surface companion in the audit | `engine-api-surface.md` (already names it) | done |

The **code** items from the 2026-06-25 review batch — **ENG-1** and **ENG-2** — remain below as real
execution items.

### ENG-1 — complete the discovery tier (DQ-4): move `generates_figures`, add `can_freeze` + `quarto_required`

**Severity:** Low-Med · **Necessary?:** unforced; harmless until a consumer exists · **Touches:**
`ts_protocol.rs` (`LoadEngineResult`/`LaunchEngineResult`), `ts_engine.rs` (test helper), plan1b
harness (emit at `loaded`). Frozen-protocol edit (surfaced + agreed). **Coordinate with Item A**
(also edits the protocol).

Q1's discovery surface (`ExecutionEngineDiscovery`) carries three static, pre-launch fields q2's
discovery tier is missing or mis-placing:

- **`generatesFigures`** is a discovery property (`execute/types.ts:58`); the instance has no such
  field. q2 wrongly put `generates_figures` on `LaunchEngineResult` (the launch reply). **Move it to
  `LoadEngineResult`.**
- **`canFreeze`** lives at **both** tiers in Q1 (discovery + instance); q2 carries it only on
  `LaunchEngineResult`. **Add it to `LoadEngineResult` too** (keep it on the instance reply).
- **`quartoRequired?`** (`execute/types.ts:65`) is an optional semver-range string the engine module
  declares, read pre-launch. **Add `quarto_required: Option<String>` to `LoadEngineResult`.** Carry
  the *field* here (inert — no v1 engine sets it); the *load-time semver gate* — Q1's
  `checkEngineVersionRequirement` (`engine.ts:62`, called at engine registration, **hard throw**) —
  is **deferred to grand-plan Phase 12**, which owns the `semver` / `VersionReq` / `cli_version()`
  machinery. (Distinct from, but sharing that machinery with, the extension-YAML `quarto-required`
  gate Phase 12 also covers — see grand-plan Phase 12.)

No consumer reads any of these fields yet (the gate is deferred), so completing the tier now is free
and prevents the wrong tier from calcifying. After ENG-1: discovery `LoadEngineResult { name,
valid_extensions, generates_figures, can_freeze, quarto_required }`; instance `LaunchEngineResult {
can_freeze }`.

- [x] Move `generates_figures` from `LaunchEngineResult` → `LoadEngineResult` in `ts_protocol.rs`.
- [x] **Add `can_freeze` to `LoadEngineResult`** (keeping it on `LaunchEngineResult` — Q1 has both).
- [x] **Add `quarto_required: Option<String>` to `LoadEngineResult`** (`#[serde(default)]`, optional
      to match Q1's `quartoRequired?`). Field only — gate deferred to Phase 12.
- **plan1b (harness) — owned by plan1b, not RTQ:** the harness reads `discovery.generatesFigures`,
      `discovery.canFreeze`, **and `discovery.quartoRequired`** into the `loaded` response (static
      fields on the constructed discovery object). Already a plan1b work item (discovery emission);
      RTQ only moves the Rust wire fields.
- [x] Update the `ts_engine.rs` `launched_response()` test helper (currently constructs
      `LaunchEngineResult { can_freeze, generates_figures }`) to drop the moved `generates_figures`
      field (keeps `can_freeze`); update the `loaded`/`LoadEngineResult` test helper to include
      `generates_figures`, `can_freeze`, **and `quarto_required`**.
- [x] **Test seam:** protocol round-trip — assert `generatesFigures`, `canFreeze`, **and
      `quartoRequired`** ride `loaded` (`LoadEngineResult`), and `canFreeze` is also present on
      `launched` (`LaunchEngineResult`) while `generatesFigures` is absent there. *Named revert:* move
      `generates_figures` back onto `LaunchEngineResult` (and/or drop `can_freeze` from
      `LoadEngineResult`) → RED.

### ENG-2 — dedup content-check + warning are q2 additions, doc'd as Q1 parity

**Severity:** Low · **Necessary?:** necessary (storage-sink design) · **Touches:** `dependency.rs`
**doc-comment only** (the "q2 always warns" sentence, ~L67-69). **Behavior is already correct and
both arms are already test-bound** (see below) — so the *code* work here is the doc-comment fix
alone; no new test. The plan1a-engine doc-half is folded (see Documentation reconciliation).

Q1 dedups **by name only, content-blind, first-wins, always silent**
(`pandoc-dependencies-html.ts:228-237`). q2's `store_html_dependencies` adds a **content-equality
check** (identical → silent skip; different → drop + **warn**) — *necessary* because q2's
artifact store is a name-keyed on-disk sink (two extensions sharing a `name` would silently
clobber). These are good q2 improvements; we keep them. Only the docs over-claim parity:

- [x] **Doc-comment fix only** (`dependency.rs` ~L67-69): mark the **content check** as a q2 addition
      (Q1 never compares bytes); fix **"q2 always warns" → "warns on a name collision with *differing*
      content; identical re-registration is skipped silently."** **No code or test change** — both
      arms are *already* bound: `test_name_collision_first_wins_one_warning` (`dependency.rs:254`)
      binds the warn path, and **`test_same_content_no_warning` (`dependency.rs:328`) already binds the
      silent path** (registers `jquery`=C twice, asserts `diagnostics.is_empty()` + one stored copy).
      The E2 seam (Test Seam Spec) therefore documents an **existing** bind, not a test to write.
  *(The plan1a-engine **doc** half — describing the content-gated dedup policy — is folded into the
  plan1a-engine correction note; see Documentation reconciliation.)*

## Test Seam Spec (frozen) — RTQ code items ENG-1 / ENG-2 / FC-1 / FC-2 (wire)

Bound before dispatch per `prevalidating-test-seams`. Each row names the **exact production
hunk whose revert reddens the exact assertion**; once green, harness + assertions are frozen
(never edited to go green). **RTQ's seams are Rust-only** (E1, E2, F1, F2a) plus the Rust-serde
T-A3 (in Item A Phase 0). The **TS/vitest** seams that this plan once also listed are **owned
elsewhere** (see Plan boundary): **B1** (`execProcess` knobs) → **Plan 2**; **F2b** (FC-2 harness
no-fold) and **T-A1/T-A2/T-A4** (Item A harness behaviour) → **plan1b**, where the harness they
mount actually exists. "Mount the real unit, mock only the genuine environment dep" still governs
those, but the rows live in their owning plans, not here.

| # | Test | Tier | Real unit mounted | Seam: mount + events + assertion surface | Mock boundary | Named revert → RED |
|---|---|---|---|---|---|---|
| E1 | `generatesFigures`, `canFreeze` **and `quartoRequired`** ride `loaded`; `canFreeze` (only) also on `launched` | Rust serde unit | `FromEngine::Loaded`/`Launched` serde | `to_value(Loaded{discovery: LoadEngineResult{…, generates_figures, can_freeze, quarto_required}})` → assert `j["discovery"]["generatesFigures"]`, `j["discovery"]["canFreeze"]` **and** `j["discovery"]["quartoRequired"]` **present**; `to_value(Launched{instance: LaunchEngineResult{can_freeze}})` → assert `j["instance"].get("generatesFigures").is_none()` **and** `j["instance"]["canFreeze"]` present | none (pure serde) | move `generates_figures` back onto `LaunchEngineResult` **and/or drop `can_freeze`/`quarto_required` from `LoadEngineResult`** → the relevant tier assertions flip RED. *(Asserting the three fields on loaded AND `generatesFigures`-absent-on-launched is what binds the tier, not just a field's existence; `canFreeze` is asserted present on both tiers — Q1 has it at both.)* |
| E2 | dedup: identical re-reg is silent (binds the q2 content-split's silent arm) | Rust unit | `store_html_dependencies` (dependency.rs) | register `jquery`=C; register `jquery`=C again (identical bytes); assert `diagnostics.is_empty()` **and** one stored copy | `SystemRuntime`/`ArtifactStore` fakes already used by the sibling collision test | make identical re-reg warn (drop the content-equality "identical→skip" arm) → `diagnostics.is_empty()` RED. *(**Already implemented** as `test_same_content_no_warning`, dependency.rs:328 — this row documents the existing bind, no new test; counterpart to the different-content→1-warning test at :254. Together they bind both arms.)* |
| F1 | FC-1 carried fields round-trip **and** `post_process` is wire-fed (not hardcoded `false`) | Rust unit (serde + the `ts_engine.rs` mapping) | `TsExecuteResult` serde + `FromEngine::ExecuteResult → ExecuteResult` mapping (`ts_engine.rs:489-505`) | `to_value(TsExecuteResult{ metadata, pandoc, resource_files, preserve, post_process:true, … })` → assert `j["metadata"]`, `j["pandoc"]`, `j["resourceFiles"]`, `j["preserve"]`, `j["postProcess"]` **present**; then map that result → assert `ExecuteResult.needs_postprocess == true` | none (pure serde + mapping) | **(a)** re-hardcode `needs_postprocess: false` in the mapping → the `needs_postprocess==true` assertion RED; **(b)** `#[serde(skip)]` a carrier (e.g. `pandoc`) → its `j["pandoc"]`-present assertion RED. *(a is the behavioral bind; b binds the carriers.)* |
| F2a | FC-2 protocol: `dependencies` defaults `true`; the `Dependencies` verb + `engineDependencies` round-trip | Rust serde unit | `ToEngine`/`FromEngine` + `TsExecuteOptions`/`TsExecuteResult` serde | `from_value(TsExecuteOptions` **without** `dependencies)` → assert `.dependencies == true`; round-trip `ToEngine::Dependencies{engine, options}` / `FromEngine::DependenciesResult{includes}`; `to_value(TsExecuteResult{ engine_dependencies: Some(…) })` → assert `j["engineDependencies"]` present | none (pure serde) | change `TsExecuteOptions.dependencies` serde default `true→false` → the "absent→`true`" assertion RED; *(removing the `Dependencies`/`DependenciesResult` variant also reddens the round-trip, but via compile-fail — the default flip is the assertion-level bind)* |

**Refactor-induced expected-value change (check 2).** The existing
`test_launch_engine_result_camel_case_can_freeze_generates_figures` (ts_protocol.rs:752)
asserts **both** `canFreeze` and `generatesFigures` on `LaunchEngineResult`. ENG-1 moves the
latter, so this test **must be revised, not deleted**: assert `canFreeze` **present** +
`generatesFigures` **absent** on `LaunchEngineResult`. The new value still discriminates the
move (present→absent) and the retained `canFreeze` assertion guards against accidentally moving
`can_freeze` too. Do not migrate it to a value that reads identical before/after.

**Missing-test pass (accepted-untested, logged):**
- *ENG-1 harness emits `discovery.generatesFigures` into `loaded`* — owned by **plan1b**
  (TypeScript harness test), not this Rust seam; recorded as the cross-plan obligation.
- *ENG-1 `ts_engine.rs` `launched_response()` helper* — compile-level update (drop the moved
  field); not a behavior test.

## Surface coverage audit — q2 wire vs the full Q1 engine surface

**Extracted to `claude-notes/designs/engine-api-surface.md`** (2026-06-26). The full
field-by-field / method-by-method audit (Level 1 inbound options & context, Level 2 method surface)
and the framework decisions (DQ-1 … DQ-7, **now resolved**) now live there, alongside the
consumed-surface companion model `claude-notes/research/2026-06-26-engine-api-usage-model.md`. The
actionable items those tables feed (ENG-1, FC-1, FC-2, Item A, the infrastructure seams) remain in
this plan; consult the design doc for the per-surface classifications and the resolved DQ decisions.

## Infrastructure seams (framework completeness)

**Not drift corrections** (wrong-vs-Q1) and **not breaking** — these are **missing framework
seams**: places where the q2 wire/types can't *carry* a Q1 capability, so adding the (deferred)
feature later forces a coordinated multi-place change instead of a body-fill. All are
**additive-compatible** (`#[serde(default)]`; no consumers required; existing engines/fixtures
unaffected). Severity: **completeness / moderate** — the cost is the later coordinated change + the
framework silently advertising an incomplete surface. Verified against `ts-engine-extensions`
source + `/Users/gordon/src/quarto-cli` on 2026-06-25 (verification log in the session).

### FC-1 — `ExecuteResult` output fields dropped at the harness→Rust wire

**Q1** (`execute/types.ts:166-178`): `ExecuteResult` carries `metadata?`, `pandoc?`,
`resourceFiles?`, `preserve?`, `postProcess?`.
**q2 (verified):** wire `TsExecuteResult` (`ts_protocol.rs:315`) = `{markdown, supporting, filters,
includes, html_dependencies}`; internal `ExecuteResult` (`context.rs:198`) = `{markdown,
supporting_files, filters, includes, needs_postprocess, html_dependencies}`. So
`metadata`/`pandoc`/`resource_files`/`preserve` are absent from **both**, and `needs_postprocess`
exists internally but is **wire-disconnected** — `ts_engine.rs:503` hardcodes `needs_postprocess:
false`. The engine-facing SDK type **`@quarto/types` `ExecuteResult`** (`execution.ts:60`) **already
declares all of them** (incl. `resourceFiles?`; note `pandoc?` is `Record<string, unknown>`, **not**
a typed `FormatPandoc`) — so **the SDK needs no change; the gap is Rust/harness-only.**

**Supersedes PROTO-1's "drop" disposition** for these five fields (see Documentation reconciliation,
PROTO-1 row).

- [x] **plan1a-protocol (`ts_protocol.rs`):** add to `TsExecuteResult`, all `#[serde(default)]`:
      `metadata: Option<HashMap<String, TsMetadataValue>>`, `pandoc: Option<HashMap<String,
      TsMetadataValue>>` (**loose JSON map — the SDK's `pandoc?` is `Record<string, unknown>`; do not
      over-type as `FormatPandoc`**), `resource_files: Vec<String>`, `preserve: HashMap<String,
      String>`, `post_process: bool`.
- [x] **plan1a-engine (`context.rs` + `ts_engine.rs`):** add the matching fields to internal
      `ExecuteResult` (`#[serde(default)]`); in the `ts_engine.rs:489-505` mapping **carry
      `post_process` instead of hardcoding `false`** (wire-feed the existing `needs_postprocess`).
      Values are carried and ignored until a feature consumes them.
- **plan1b (harness) — owned by plan1b, not RTQ:** forward these from the engine's returned
      `ExecuteResult` when building the wire frame (greenfield — the execute-dispatch must forward
      **all** fields when first written). Already a plan1b work item (the carried-but-inert FC-1
      fields, with `post_process` wire-fed, are enumerated in plan1b's execute-dispatch step).
- [x] **Test seam (F1, frozen):** carried fields round-trip **and** `post_process` is wire-fed (not
      hardcoded). *Named revert:* re-hardcode `needs_postprocess: false` in the `ts_engine.rs` mapping
      → the `needs_postprocess==true` assertion RED. See the Test Seam Spec.
- [x] **Additive-compatibility:** `#[serde(default)]` throughout (precedent: `html_dependencies`,
      `context.rs:248`) — existing engines / stored capture fixtures omitting these still deserialize.

### FC-2 — the `dependencies()` round-trip has no wire verb (deferred-deps path)

**DQ-2 resolved: build the infrastructure now, the Q1-orchestrated way** (decision 2026-06-27).

**Q1 (engine-protocol surface, read directly):** `ExecutionEngineInstance.dependencies(options:
DependenciesOptions)` (`execute/types.ts:123`) is the **deferred** companion to inline materialization, and
**the render orchestrator drives it — not the engine harness.** `ExecuteOptions.dependencies: boolean` (Q1
`resolveDependencies`, default `true` — `render-files.ts:146,224`) selects the path: `true` → `execute()`
resolves inline into `includes` (jupyter `resultIncludes`, `jupyter.ts:557`), `engineDependencies` stays
**undefined**; `false` → `execute()` **returns** the deferred `engineDependencies?: Record<string,
Array<unknown>>` map (engine-name-keyed, `:174`), and **later the orchestrator** (`render.ts:90-109` — the
comment is literally *"run the dependencies step if we didn't do it during execute"*) iterates that map by
engine name and calls `engine.dependencies({…, output: recipe.output, dependencies:
engineDependencies[<engine>], …})` → `DependenciesResult { includes }`, merging into `format.pandoc`.
`DependenciesOptions` (`:201-211`) requires `target, format, output, resourceDir, tempDir` (+ optional
`libDir`/`projectDir`) **and** the `dependencies?: Array<unknown>` payload. The deferred path's *point* is
resolving deps **once at a merged output** (book/project rendering) — which only the orchestrator, seeing
all documents, can do.

**q2 (verified, `ts_protocol.rs:296`):** there is **no `dependencies` verb** on the wire, `TsExecuteResult`
carries **no `engineDependencies`**, and `TsExecuteOptions` has **no `dependencies` flag**. The old 1b plan
papered over this with a **harness-internal fold** (harness calls `dependencies()` during `execute`) — which
is **un-Q1 and useless**: folding immediately equals inline, and the harness (one document at a time) can
never resolve at a merged output. So the deferred feature was unbuildable without protocol surgery.

**Decision — build the Q1-orchestrated infra now (deferred *feature*, not deferred *infra*).** Add the wire
surface so q2 orchestrates exactly as Q1 does; the book/project renderer becomes a body-fill. No v1/Julia
caller sends `false`; the path is present-but-unexercised, inert for real engines until Plan 3E lands
`quarto.jupyter.widgetDependencyIncludes`.

- [x] **`ts_protocol.rs` (landed code — RTQ-tracked, *not* a `plan1a-protocol.md` edit):** all additive
      (`#[serde(default)]`):
      - `TsExecuteOptions.dependencies: bool` (**default `true`** — match Q1's `resolveDependencies`).
      - `TsExecuteResult.engineDependencies: Option<HashMap<String, Vec<TsMetadataValue>>>` — the deferred
        map, **forwarded** to q2 (*not* resolved in the harness).
      - a new verb **`ToEngine::Dependencies { engine, options: TsDependenciesOptions }`** + result
        **`FromEngine::DependenciesResult { includes: TsPandocIncludes }`** — the symmetric sibling of
        `IntermediateFiles`/`IntermediateFilesResult`. **`output` lives on this options struct**
        (supplied at the call = the final/merged output), *not* on `TsExecuteOptions`. Two q2-shape
        reconciliations vs Q1's `DependenciesOptions` (`:201-211`): **(DQ-3)** there is no
        `ExecutionTarget` cookie — carry the **same flattened target fields** as `TsExecuteOptions`
        (`input`/`source_path`/`source_map`), not a Q1 `target`; and **(Item A)** `resource_dir` is
        **omitted** — it is ambient via `Init.global` (`path.resource`), exactly as `TsExecuteOptions`
        omits it. Resulting struct:

        ```rust
        #[serde(rename_all = "camelCase")]
        pub struct TsDependenciesOptions {
            // flattened target (DQ-3 — no ExecutionTarget cookie), mirroring TsExecuteOptions
            pub input: String,
            pub source_path: String,
            pub source_map: Vec<TsSourceMapEntry>,
            pub format: TsFormatInfo,
            pub output: String,                     // final/merged output — supplied at the call
            pub temp_dir: String,                   // per-render scratch (NOT resource_dir — ambient)
            pub lib_dir: Option<String>,
            pub project_dir: Option<String>,
            pub dependencies: Vec<TsMetadataValue>, // the deferred deps for this engine
                                                    // (= engineDependencies[<engine>])
            pub quiet: bool,
        }
        ```
- **plan1b (harness) — owned by plan1b, not RTQ:** handle the `dependencies` verb as a **thin
      pass-through** to `instance.dependencies(opts)` (reply `dependenciesResult { includes }`);
      **forward** `engineDependencies` on the execute reply; **no harness-internal fold**. Already
      done in plan1b's reworked execute-dispatch flow + the new `dependencies` message arm.
- [ ] **q2 render orchestrator (plan1a-engine / plan1c render path — deferred consumer):** when an `execute`
      reply carries `engineDependencies`, iterate by engine name and send a `dependencies` message per key
      (`output` = the render recipe's output), merging each `DependenciesResult.includes`. Mirrors
      `render.ts:90-109`. Lands with the book feature — the verb existing now means **no protocol surgery
      then**.
- **Cross-plan dependency (note, not an execution item) → Plan 3E:** the jupyter `dependencies()` body
      calls `quarto.jupyter.widgetDependencyIncludes` (`jupyter.ts:609-611`), provided only at 3E. Until
      then the verb is plumbed-but-inert for real engines.
- **Cross-plan dependency (note, not an execution item) → replay/capture:** the `dependencies` round-trip
      must be captured/replayed alongside `execute` so a frozen render reproduces deps deterministically.
      Already flagged in `claude-notes/plans/2026-05-03-replay-engine.md` (future, with the book feature).
- [x] **Test seam (frozen, RTQ):** **F2a** (Rust serde) — `dependencies` defaults `true`, the
      `Dependencies` verb + `engineDependencies` round-trip; *named revert:* flip the `dependencies`
      default `true→false` → "absent→true" RED. See the Test Seam Spec.
- **Test seam (owned by plan1b):** **F2b** (TS/vitest, 1b harness, **fake** engine) — `execute` with
      `false` forwards `engineDependencies` verbatim and the harness does **not** call
      `dependencies()`; `true` → empty/absent; a `dependencies` message → `instance.dependencies(opts)`
      → `dependenciesResult{includes}`. Lives in plan1b (deferred-deps wire seam); not an RTQ
      checkbox.
- [x] **Additive-compatibility:** `#[serde(default)]` throughout — existing fixtures/engines deserialize.

## Calibration review (2026-06-26) — items B1–B5

A whole-epic calibration pass against the **consumed**-API ground truth
(`claude-notes/research/2026-06-26-engine-api-usage-model.md`) + reconciled findings
(`…-1a-2a-calibration-reconciled.md`). Each item was **re-verified against source** (Q1
`/Users/gordon/src/quarto-cli` + landed q2) — the cited `file:line` confirmed, corrections noted
inline. B3's code-half lives on Item A's checklist (above); **B1 moved to Plan 2** (Option A split —
see Plan boundary); B6/B7 are recommendations, not queued items.

### B1 — `system.execProcess` drops `mergeOutput` + `stderrFilter` — **MOVED to Plan 2**

**Relocated to Plan 2 (`quarto-markdown-and-api.md`) by the Option A split (2026-06-29).** B1 is a
standalone return-to-Q1 correction of **landed §2aa `@quarto/api` code** — it touches no 1a file
(`system/index.ts`, `platform/index.ts`, vendored `@quarto/types/quarto-api.ts`), is testable now
against a fake `PlatformHost`, and belongs with Plan 2's still-open `@quarto/api` work (Phase A
bodies + Phase B `@quarto/types` reconciliation). The full fix (flatten `mergeOutput`/`stderrFilter`
into `ExecProcessOptions`, thread through `PlatformHost.ExecOptions`, reconcile the vendored 6-param
signature), the **T-B1** vitest seam, and the 2026-06-26 verification/decision now live in Plan 2.
The ID is retained here so existing `RTQ B1` references resolve. *(B3's code-half stub relabel stays
on Item A's checklist — it is forced by Item A's gate-removal, unlike B1.)*

### B2 — preserve-restore: one coherent recovery story (doc)

**Severity:** Low · **Necessary?:** doc-coherence · **Touches:** RTQ (Level-2 `postprocess` row),
plan1a-protocol (`:186`). Doc-only.

**Verified (2026-06-26):** the *only* real work in knitr/jupyter `postprocess` is one call to
`quarto.text.postProcessRestorePreservedHtml(options)` (`rmd.ts:341`, `jupyter.ts:627`); in q2 that
consumer is **deferred** (`@quarto/api` `text/index.ts:9-11`: "DEFERRED — file I/O"). The three
real pieces exist but are unconnected — **FC-1 carries** `preserve` + `post_process` on the wire
(this plan); **Plan 3 builds the producer** `removeAndPreserveHtml` (`quarto-jupyter.md:134-136`,
returns `{output, preserved}`); the **consumer is dropped.** And the disposition is told two ways:
**plan1a-protocol:186** classes `postprocess` *Deferred → trait + protocol message*, while **RTQ
Level-2** classes it *Drop → AST transform*. *(Cite correction: the `postprocess` "Deferred" row is
plan1a-protocol **:186**, not :182 — :182 is the `partitionedMarkdown` row.)*

*(Done, doc-only — folded out: the single `postprocess` recovery story (an AST transform reading
FC-1's carried `preserve` field, seam-linked to Plan 3's `removeAndPreserveHtml` producer) is stated
in the Surface coverage audit, and `plan1a-protocol:186` was reconciled to match. See Documentation
reconciliation.)*

### B3 — Item A gating-removal propagation

Two parts. The **code** half — re-label the four `requiresLaunchContextError` stubs →
`notYetImplementedError("Plan 2")` — stays on **Item A's Phase-1 checklist** (above). The **doc**
half — striking Plan 2's "gated until `launchEngine`" prose and the dead `format.*` gate — is
**folded into `quarto-markdown-and-api.md`** (see Documentation reconciliation). Verified 2026-06-26
(stubs `path/index.ts:168,172,176` + `system/index.ts:306`).

### B4 — record the consumed-surface audit gap (doc, structural)

**Severity:** Low (honesty/structural) · **Touches:** RTQ ("Surface coverage audit"). Doc-only.

RTQ's Surface coverage audit covers only the engine-**PROVIDED**/wire half (what q2 *sends* and
*receives*). It has **no systematic audit of the QuartoAPI-CONSUMED surface** — the
`quarto.<ns>.<method>` calls engines make *back*. That blind spot is exactly what let B1 (a
param-level drop *inside* a method that reads as "present") slip a provided-surface audit.

*(Done: `engine-api-surface.md` already names `…/2026-06-26-engine-api-usage-model.md` as the
consumed-surface companion and states the audit is two-sided. See Documentation reconciliation.)*

### B5 — Phase-B-before-Plan-3 type ordering — **resolved (drift re-grounded)**

**Severity:** Low · **Touches:** none in 1a/2A — owned by Plan 2 Phase B. Doc-only.

Plan 2 **Phase B** owns the `@quarto/types` jupyter signature reconciliation; Plan 3 builds the
*runtime* jupyter namespace; the grand-plan dep graph lists Plan 2 ∥ Plan 3 (no edge). If Plan 3
implemented jupyter against the stale vendored signatures, the two could disagree.

**Resolved.** The drift is re-grounded in `claude-notes/research/2026-06-28-plan2-phaseB-vs-usage-model-reconciled.md`
and Plan 2's Phase B "Type drift reconciliation" section. It is **seven** lagging signatures (not the
earlier "six" — that count netted one out): `kernelspecFromMarkdown`, `markdownFromNotebookFile`,
`markdownFromNotebookJSON`, `notebookFiltered`, `widgetDependencyIncludes`, `pythonExec`,
`capabilities` — exactly the drifted rows in usage-model D.2. **Phase B's decision: reconcile the
runtime up to Q1-live** (sync→async + param/return reshapes), keeping the vendored `@quarto/types`
authoritative. The only residual is a soft ordering confirm — if Plan 3 self-types its jupyter
namespace (as 2A's `system` carries its own `ProcessResult`), strict Phase-B-before-Plan-3 ordering
isn't required. *(All jupyter-built-in-only; the standalone extensions don't call these, so nothing
breaks today — a forward-correctness item, not a live bug.)*

### B6 / B7 — 2A cleanup (DONE, not deferred)

Done directly on `feature/ts-engine-extensions` (pure 2A hygiene, independent of return-to-Q1):
- **B6 (config parity test):** switched `config.test.ts` from `Set`-equality to **sorted-array
  compare** — catches a removed/added key, a value mutation, **and a lost duplicate** (the language
  keys' deliberate duplicates), while ignoring functionally-irrelevant order. **217 pass / 1 skip**
  confirmed.
- **B7 / 2a-3 (stale status):** flipped plan2a's "§2aa not yet built" banners (lines 6-7, 28, 140)
  → "landed".
- **B7 / 2a-4 (stale bullet):** struck `postProcessRestorePreservedHtml` from the `text/` work-item
  bullet (plan2a:194-196) — deferred per resolved-decision #3; the landed code already omits it.
- **B7 / 2a-5 (cosmetic):** **no action** (per the 2A review — `fs.remove`/`kLanguageDefaultsKeys`
  are expected pre-build-sketch drift; the landed `platform/index.ts`/`config/index.ts` are
  authoritative).

**Still coupled to Item A (NOT done here):** B3's stub re-label (`requiresLaunchContextError` →
`notYetImplementedError`) and the Plan-2 gating-prose strike are only correct *after* Item A removes
the gate — they live on Item A's checklist.

## References

- 1b review findings: `claude-notes/research/2026-06-24-plan1b-review-findings.md`
- pampa pandoc-option support (PROTO-1 evidence):
  `claude-notes/research/2026-06-25-pampa-pandoc-option-support.md`
- Concurrency model: `claude-notes/designs/engine-host-concurrency.md`
- Q1: `core/api/path.ts`, `core/resources.ts`, `core/appdirs.ts`, `core/api/types.ts`
  (`PathNamespace:148`, `QuartoAPI:236`), `execute/types.ts` (`launch:86`,
  `ExecuteResult:166-178`)
- q2: `crates/quarto-core/src/engine/ts_protocol.rs` (`LaunchEngine:41`),
  `crates/quarto-core/src/engine/context.rs` (`ExecuteResult:198`)
