# Engine API surface — q2 wire vs the full Q1 engine surface

**Extracted from** `claude-notes/plans/2026-06-25-plan1a-return-to-q1.md` (RTQ) on 2026-06-26.
This is the surface-coverage record — the field-by-field / method-by-method audit of the q2
engine wire against the full Q1 engine surface, plus the framework decisions (DQ-1 … DQ-7), all
**resolved 2026-06-29**. RTQ carries the actionable protocol/code items (Item A, ENG-1, FC-1, FC-2);
this doc records the surface classification and the decisions behind it.

**Companion (consumed-surface model):** this doc audits the engine-**PROVIDED** / wire half (what
q2 *sends* and *receives*). The **CONSUMED** half — the `quarto.<ns>.<method>` calls engines make
*back* — is modeled in `claude-notes/research/2026-06-26-engine-api-usage-model.md`. The two are
two sides of the same surface.

## Governing principle — the validation target is not the scope boundary

**Carry the whole Q1 engine protocol surface. The scope boundary is the *Q1 engine API*, not what
the current *validation target* (Julia, single-doc, single-engine) happens to exercise.** "Defer
features, not infrastructure" is the operational form of this: where Q1 exposes an engine
method / field / flag, the q2 **infrastructure** to carry it is in scope **now** (classified
`build-infra` or `defer-infra` with a recorded seam below), even with zero current callers. Only
`drop` when q2's architecture makes it impossible or redundant — *with a reason*. A feature (no q2
producer/consumer yet) may be deferred; the protocol that would carry it may not be silently
narrowed.

**The failure mode this prevents.** The 1a/1b plans were drafted against the Julia render path and
repeatedly scoped the wire to *what that path needs*, dropping protocol surface no Julia render
touches. Because the drops are invisible on the Julia validation target, they pass every test and
surface only later as a coordinated retrofit. Two confirmed instances — **the same class of bug
twice**:

- **`system.execProcess` params** (RTQ F1 / `2026-06-26-plan2a-review-findings.md` §2a-1): q2
  reduced the signature to `(options, stdin?)`, dropping `mergeOutput`/`stderrFilter` because *no
  TS-extension* uses them — but **knitr does** (`rmd.ts:440-458`), and the SDK is advertised as
  Q1-consumable. *Consumed-surface half; lives in the usage model.*
- **`dependencies` flag + deferred-deps fold** (1B-DEPS-2 / `2026-06-26-1b-vs-usage-model-reconciled.md`):
  1b hardcoded `dependencies:true` + a dead fold because *no single-file book exists yet* — dropping
  the deferred path, which is Q1 **protocol** (its first consumer is single-file book rendering,
  `book-render.ts:136`). *Provided-surface half — note this doc already classes the flag
  `build-infra` (Level 1); 1b diverged from that classification.*

Both are Julia-invisible (Julia's `execProcess` unused; Julia's `dependencies()` a no-op) — exactly
why a Julia-only review misses them.

A third near-instance — **caught before landing**, recorded so the pattern stays visible:

- **Static-claim schema expressiveness** (plan1c review, 2026-06-28): the D1 static-claims schema
  was first shaped to the **echo** fixture (language-only + extension-only) and initially could not
  express marimo's `first_class`-conditional claim (`{python .marimo}`). Fixed before landing by
  adding `whenClass:` — `claims_language` is a pure function of `(language, first_class)`, so it
  tabulates fully — so the static surface is **not** narrowed. The one genuine residue is
  **content-inspecting `claims_file`** (Julia's `# %%`): it cannot be expressed as a *static* claim,
  but the **dynamic `claims_file` method remains** as the fallback, so the engine *protocol* is
  intact. That is an accepted static-vs-dynamic boundary, **not** a dropped surface — the only place
  static resolution is strictly less powerful than the dynamic method, and it is documented as such
  (engine-resolution.md §3.3).

**The author test.** For each Q1 engine method / field / flag, do **not** ask "does the Julia
validation target need this?" Ask: *does the Q1 engine API expose it as protocol, and could a
non-Julia engine or a not-yet-built render mode (books, manuscripts, serve, multi-engine) use it?*
If yes → infrastructure is in scope now (`build-infra`/`defer-infra` + seam). The validation target
proves the framework works; it does not define the framework's surface.

## Surface coverage audit — q2 wire vs the full Q1 engine surface

Both levels, **Q1 read directly** (`execute/types.ts:35-243`, `project/types.ts:164-216`, the
lifecycle model). The original 1a wire carried the Julia render path and omitted the rest of the
engine surface. Per "defer features, not infrastructure," each Q1 engine method/field is classed:
**present** (on the wire) · **build-infra** (framework should carry now; q2 consumer may stub) ·
**defer-infra** (real but premature — record the seam) · **drop** (q2 architecture makes it
impossible/redundant, reason given).

### Level 1 — inbound options & context (field-by-field)

**`ExecuteOptions` → `TsExecuteOptions`:**

| Q1 field | q2 | class | note |
|---|---|---|---|
| `target: ExecutionTarget` | flattened → `input`/`source_path`/`source_map` | **drop** | no `target()` step (DQ-3); cookie + `data` absent — `engine_state` added only if a round-trip later needs it |
| `format` | `format` | present | |
| `resourceDir` | `Init { global }` (ambient, Item A) | present | |
| `tempDir`/`cwd`/`libDir?` | `temp_dir`/`cwd`/`lib_dir` | present | |
| `dependencies: boolean` | wire flag (default `true`) | **build now** | build the round-trip — flag + `Dependencies` verb + `engineDependencies` (FC-2); orchestrator-driven, harness fold deleted (DQ-2) |
| `projectDir?` | `project_dir` | present | |
| `params?`/`quiet?` | `params`/`quiet` | present | |
| `previewServer?` | — | defer | run/serve — deferred behind a seam (DQ-2) |
| `handledLanguages` | `handled_languages` | present | |
| `project: ProjectContext` | `project_dir` + launch `EngineProjectContext` | present | `config` + output-dir carried as values (DQ-5); the two callback members dropped (DQ-1) |
| — | `source_map` | q2-native | provenance addition |

**`ExecutionTarget`** (Q1 cookie; q2 has **no `target()` step**):

| Q1 field | q2 | class | note |
|---|---|---|---|
| `source`/`input`/`markdown` | `source_path`/`input` (resolved markdown pushed) | present | |
| `metadata` | folded into `format.metadata` | verify | confirm target-vs-format metadata overlap (residual verify, not a design question) |
| `data?` (engine cookie) | — (q2 uses `engine_config` + lazy resolve in execute) | **drop** | no `target()` (DQ-3); jupyter `{transient, kernelspec}` resolved lazily in execute |
| `preEngineExecuteResults?` | — | defer-infra | cell-handler pre-results |

**`EngineProjectContext`** (passed to `launch()` after Item A; q2 carries only `{dir, isSingleFile}`):

| Q1 member | q2 | class | note |
|---|---|---|---|
| `dir` / `isSingleFile` | present | present | |
| `config?` (`engines`, `output-dir`) | carry as values on launch ctx | **build now** | cheap serializable values (DQ-5) |
| `getOutputDirectory()` | pass output dir as a value | **build now** | not a callback — a value on the launch ctx (DQ-5) |
| `fileInformationCache` | — | **drop** | host-owned live `Map`, not serializable; sole engine read is jupyter `keep-ipynb` transient tracking via `target.data` — a cookie q2 drops (DQ-3) and a file-lifecycle q2 owns host-side (DQ-1) |
| `resolveFullMarkdownForFile()` | — (q2 pushes resolved `input`) | **drop** | push model; the callback was used only by obsolete engine methods (DQ-1) |

### Level 2 — method surface

**Discovery (`ExecutionEngineDiscovery`):**

| Q1 member | q2 | class | note |
|---|---|---|---|
| `init?`/`name`/`validExtensions`/`claimsFile`/`claimsLanguage`/`launch` | present | present | |
| `generatesFigures` | → `LoadEngineResult` (discovery) | **build now** | move to discovery tier — ENG-1 (DQ-4) |
| `canFreeze` | → `LoadEngineResult` **and** instance | **build now** | add to discovery for freeze-planning; keep on instance (Q1 has both) — DQ-4 |
| `quartoRequired?` | → `LoadEngineResult` (discovery) | **build now** | load-time semver gate (grand-plan Phase 12) — DQ-4 |
| `ignoreDirs?` | — | out of scope | project file-walk — out of the render wire (DQ-6) |
| `defaultExt`/`defaultYaml`/`defaultContent` | — | out of scope | scaffolding (`quarto create`) — own command surface (DQ-6) |
| `checkInstallation?` | — | out of scope | `quarto check <engine>` — own command surface (DQ-6) |
| `populateCommand?` | — | **drop** | cliffy subcommands into a Rust CLI — impossible |

**Instance (`ExecutionEngineInstance`):**

| Q1 method | q2 | class | note |
|---|---|---|---|
| `markdownForFile`/`execute`/`intermediateFiles?` | present | present | |
| `target` | — | **drop** | per-file cookie folded into execute; no `target()` step (DQ-3) |
| `dependencies` | → wire verb | **build now** | deferred-deps round-trip built — FC-2 (DQ-2) |
| `postprocess` | — | **drop** | recover via an **AST transform** that reads FC-1's already-carried `preserve` field (the No-DOM-postprocessor rule). Connected seam: **FC-1 (carries `preserve`/`post_process`) ↔ this dropped `postprocess` hook ↔ Plan 3's `removeAndPreserveHtml` producer** (`quarto-jupyter.md:134-136`). (DQ-2) |
| `partitionedMarkdown` | — | **drop** | pampa parses qmd natively after `markdownForFile`; engine partition is redundant |
| `filterFormat?` | — | defer | engine influences `Format` pre-render — behind a seam |
| `run?`/`postRender?` | — | defer | serve/preview + after-render (server-backed engines) — documented seam (DQ-2) |
| `canKeepSource?`/`executeTargetSkipped?` | — | defer-infra | keep-md / freeze-skip hooks |

### Resolved framework decisions (DQ-1 … DQ-7)

These were the open framework questions in the 2026-06-25/26 sessions; **all are decided
(2026-06-29).** Recorded here as resolved facts — the per-surface tables above carry the same
outcomes inline. The actionable protocol/code changes live in RTQ
(`2026-06-25-plan1a-return-to-q1.md`) as **Item A, ENG-1, FC-1, FC-2**.

- **DQ-1 — engine→host callbacks → push model; callbacks dropped.** q2 pushes fully-resolved `input`
  markdown into execute, so `resolveFullMarkdownForFile` is unnecessary (Q1 engines called it only
  from obsolete methods). `fileInformationCache` drops with it: it is a host-owned live
  `Map<string, FileInformation>`, not serializable across the subprocess; its sole engine reader is
  jupyter's `keep-ipynb` transient-notebook bookkeeping, which mutates `target.data` — a cookie q2
  drops (DQ-3) and a file-lifecycle q2 owns host-side. **No `FromEngine`-initiated callback channel
  in v1** (re-entrancy/deadlock risk on the single Deno thread; no consumer).
- **DQ-2 — render lifecycle → build the `dependencies` round-trip (FC-2), orchestrator-driven; drop
  `postprocess`; defer `run`/`postRender`.** The wire gains `dependencies: bool` (default `true`) +
  `ToEngine::Dependencies`/`FromEngine::DependenciesResult` + `engineDependencies` on the result;
  **q2's render orchestrator drives the deferred resolution** (mirrors Q1 `render.ts:90-109`), **not**
  a harness-internal fold. `postprocess` is dropped (no post-write DOM stage; recover via an AST
  transform reading FC-1's `preserve`). `run`/`postRender` deferred behind a documented seam.
- **DQ-3 — per-file engine-state cookie → no `target()` step in v1.** q2's lazy-resolve +
  `engine_config` is sufficient; add an opaque `engine_state` field **only if** a future
  `dependencies` round-trip forces an engine to thread state across it.
- **DQ-4 — discovery-tier completeness → expand `LoadEngineResult` to
  `{name, valid_extensions, generates_figures, can_freeze, quarto_required}`.** All static,
  cheap-at-load, read pre-launch by resolution / freeze-planning / version-gating. ENG-1 is the first
  landed slice (`generates_figures` + `can_freeze`); `canFreeze` stays on the instance result too
  (Q1 has it at both tiers).
- **DQ-5 — `EngineProjectContext` completeness → carry `config` (`engines` + project `output-dir`)
  and the output directory as values** on the launch context. The two callback members are DQ-1
  (dropped).
- **DQ-6 — non-render surfaces → out of the render wire.** `populateCommand` is a hard drop;
  `defaultExt`/`defaultYaml`/`defaultContent` (scaffolding), `checkInstallation` (check),
  `ignoreDirs` (project walk) belong to their own command surfaces — designed when those commands
  grow engine-awareness, not bolted onto the execute protocol.
- **DQ-7 — Init/launch split → `Init { global }` once per subprocess (process-stable config);
  project context rides `LaunchEngine` per render** (Item A). This **supersedes** the earlier
  "`Init { global, project }`" recommendation: moving project context to `LaunchEngine` is strictly
  more Q1-faithful (`engine.launch(EngineProjectContext)`) and is the enabler for reusing one
  subprocess across renders (`Init` stays process-stable; each render's `launchEngine` carries its
  own project context).

### Build checklist (decision → landing item)

All boxes are **unimplemented** — RTQ is plan-only, not yet executed. Each box is the protocol/code
change a decision requires; the **owning RTQ item** is named so this stays a coverage map, not a
competing source of truth. A box marked **(no owner)** has no RTQ item yet and must get one in the
consolidation pass.

- [ ] **DQ-1** — push resolved `input`; no `FromEngine` callback channel; harness builds a
  *harness-local* `fileInformationCache` (not a wire carrier) — *RTQ Item A (plan1b harness)*
- [ ] **DQ-2** — `dependencies: bool` (default `true`) + `ToEngine::Dependencies` /
  `FromEngine::DependenciesResult` + `engineDependencies` on the result; orchestrator-driven;
  delete the harness fold — *RTQ FC-2*
- [ ] **DQ-2** — drop `postprocess`; carry `preserve`/`post_process` and recover via an AST transform
  — *RTQ FC-1 (carrier) + B2 (recovery story) + PROTO-1 (disposition)*
- [ ] **DQ-4** — move `generates_figures` → `LoadEngineResult`; add `can_freeze` there (keep on the
  instance result) — *RTQ ENG-1*
- [ ] **DQ-4** — add `quarto_required: Option<String>` to `LoadEngineResult` (discovery tier). Splits
  in two: **field → fold into ENG-1** (same `ts_protocol.rs` / harness / test-helper change as the
  DQ-4 tier completion — carry it inert, no v1 engine sets it); **load-time semver gate →
  grand-plan Phase 12** (reuse its `semver` / `VersionReq` / `cli_version()` machinery). Q1:
  `quartoRequired?` on `ExecutionEngineDiscovery` (`execute/types.ts:65`), gated by
  `checkEngineVersionRequirement` (`engine.ts:61`, **hard throw**; cf. Phase 12's extension-YAML
  `quarto-required`, which **warns** — q2 must pick one severity).
- [x] **DQ-5** — carry `config` (`engines` + project `output-dir`) and the output directory as
  *values* on `LaunchEngine.project` — *RTQ Item A* (Plan 1c.2 P1.1, 2026-07-02: wired in
  `build_engine_registry`; wire `config` carries `engines` + **flat** top-level `output-dir` —
  the host's `reconstructRichProject` bridges it into the rich `config.project.outputDir`)
- [ ] **DQ-7** — `Init { global }` once per subprocess; project context on `LaunchEngine` per render
  — *RTQ Item A (sequence its `ts_protocol.rs` edit with ENG-1)*

**Decided, no build (recorded for completeness):** DQ-2 `run`/`postRender` — deferred behind a
documented seam (name it in RTQ/grand-plan, no code now); DQ-3 — no `target()` step, add
`engine_state` only if a future round-trip needs it; DQ-6 — non-render surfaces stay off the render
wire.
