# Plan 1c: Extension Integration & End-to-End

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** plan1a-protocol, plan1a-host, plan1a-engine (Rust core: protocol, subprocess, trait, `TsEngine`)
and Plan 1b (Deno harness: `@quarto/engine-host-deno`). Phases 1-2 of this
plan could technically start from the plan1a sub-plans alone (no subprocesses spawned),
but Phase 3 (echo engine E2E test) requires both.
**Blocks:** Plan 4 (Julia Validation)
**Estimated sessions:** 1-2

## Overview

Wire the TS engine infrastructure from Plans 1a and 1b into the extension
system and the engine-resolution pipeline. Parse engine contributions from
`_extension.yml`, build TS extensions with `deno bundle`, build an
`Arc<EngineRegistry>` on `ProjectContext`, replace metadata-only engine
detection with the tiered **engine-resolution model**
([`claude-notes/designs/engine-resolution.md`](../designs/engine-resolution.md)),
and validate end-to-end with an echo engine integration test.

**Post-rebase context.** This plan was authored single-engine (April 2026);
sequential multi-engine execution (bd-5yff4), replay/capture (bd-45yw/bd-5qnj),
and the discovery cache (bd-c5u2g) have since landed on `main`. Engine
resolution is no longer "pick one engine" — it produces an ordered, distinct
**sequence** plus a per-language **ownership map** that drives each engine's
`handled_languages`. The resolution model, claim interface (`LanguageClaim`),
tiers, ownership enforcement, replay, failure model, and file-claim semantics
all live in the design contract; this plan references it for the model and
carries the wiring work items. This wiring is also the principled fix for
Carlos's multi-engine cell-ownership follow-up (bd-iq0hp).

## Phase order

Phase 1 → Phase 2 → Phase 3

## Work Items

### Phase 1: Extension discovery and build

Parse `_extension.yml` for engine contributions, build TS extensions into bundled JS, and register `TsEngine` instances.

Following Quarto 1's approach: engine extensions are **built** (bundled from TS to a single JS file) before execution. At runtime, q2 loads the bundled `.js` file — no import map or TS transpilation needed at execution time.

**Build model: explicit, never auto.** Engine extension `.js` bundles are
produced by the author running `q2 build-ts-extension` and committed to
the repo. q2 never runs `deno bundle` during render. Missing bundles fail
loudly, pointing the user to the build command. Aligns with Quarto 1.

- [ ] Add `engines` field to the `Contributes` struct in `crates/quarto-core/src/extension/types.rs`:
  ```rust
  /// Engine contributions: paths to TS engine modules or engine name
  /// strings for reordering.
  pub engines: Vec<EngineContribution>,
  ```
  And define:
  ```rust
  /// An engine contributed by an extension.
  #[derive(Debug, Clone)]
  pub enum EngineContribution {
      /// An external engine module (pre-built .js bundle).
      /// Absolute path (resolved during read_extension).
      External {
          path: PathBuf,
          /// Static hint: the engine's runtime name (e.g., "julia").
          /// `None`: not declared — q2 must `LoadEngine` to learn the
          /// name; the engine is registered under its extension id as a
          /// placeholder, and a `runtime_name → extension_id` alias map
          /// is populated on first load. Emit a warning suggesting the
          /// author add this field.
          /// `Some(name)`: declared up front; q2 registers the engine
          /// under `name` immediately (no subprocess load needed for
          /// registration or YAML lookup). At first `LoadEngine`, q2
          /// asserts `LoadEngineResult.name == name`; mismatch is a
          /// hard error pointing at the YAML.
          name: Option<String>,
          /// Static hints: languages this engine might claim (e.g., ["julia"]).
          /// `None`: not declared in _extension.yml — fall back to dynamic
          /// claim, with a warning suggesting the author add hints.
          /// `Some(empty)`: explicitly "claims no languages" — silent, no
          /// dynamic call.
          /// `Some(non-empty)`: pre-filter; only consult engine dynamically
          /// if a candidate language is in the list.
          languages: Option<Vec<String>>,
          /// Static hints: file extensions this engine might claim (e.g., [".jl"]).
          /// Same three-state semantics as `languages`.
          file_extensions: Option<Vec<String>>,
      },
      /// A bare engine name string — reordering hint that moves a
      /// previously registered engine to higher priority.
      Reorder { name: String },
  }
  ```
  This extends Quarto 1's schema: `contributes.engines` accepts both
  objects with a `path` property (creating new engines) and bare strings
  (reordering hints). The `name`, `languages`, and `file-extensions`
  fields are q2 additions — optional static hints with identical
  declarative-then-verify shape:
  - `name` lets q2 register the engine and answer YAML `engine: foo` /
    top-level-key detection without spawning the Deno subprocess at all.
  - `languages` / `file-extensions` let q2 skip dispatching to the
    subprocess for `claimsLanguage` / `claimsFile` queries when the
    input is clearly irrelevant.

  **Q1 schema compatibility:** Q1's `external-engine` schema definition
  is `closed: true` (single property `path`). A `_extension.yml` that
  declares any of `name` / `languages` / `file-extensions` will fail
  Q1's validator. Document this explicitly: q2 engine extensions are
  q2-targeted; the file is not portable backward to Q1.

- [ ] Add `engines` parsing in `parse_contributes()` in `crates/quarto-core/src/extension/read.rs`:
  - Handle array of strings (reordering hints → `EngineContribution::Reorder`)
    and objects with `path` key (resolve to absolute paths relative to ext_dir
    → `EngineContribution::External`).
  - For object entries, parse the optional `name: String` field into
    `Option<String>`. Distinguish field-absent from field-present-with-empty-list
    when reading `languages` and `file-extensions`. Map to `Option<Vec<String>>`:
    `None` if the YAML key was absent; `Some(vec![])` if present as `[]`;
    `Some(...)` if present and non-empty.
  - **Validate `path` ends in `.js` (lowercase).** Reject `.JS`, `.mjs`,
    `.ts`, etc. with the same actionable error: `Engine extension
    '{name}' has 'path: {path}'; only pre-built lowercase '.js' bundles
    are loadable. Run 'q2 build-ts-extension' to produce
    {expected_js_path} and update _extension.yml.` The runtime subprocess
    uses `deno run --allow-all <engine-host.js>` with no import map; a
    raw `.ts` (or `.mjs`/etc.) path would fail to resolve `@quarto/api`,
    `@quarto/types`, and other engine-extension imports.
  - **Emit a warning per `External` entry that has missing hints.** If
    any of `name`, `languages`, or `file_extensions` are `None`, emit a
    single `DiagnosticMessage::warning` per extension naming exactly
    which fields are missing, the extension's path, and a snippet
    showing what to add to `_extension.yml`. Suggest `[]` to silence the
    `languages` / `file-extensions` parts of the warning if the engine
    genuinely claims none. The warning fires at extension-discovery time
    (once per render). Reorder entries don't trigger the warning.
  - Include `engines` in the "at least one sub-field" validation check.
  - This supersedes Phase 8 of the extensions grand plan
    (`claude-notes/plans/2026-03-16-extensions-grand-plan.md`); the
    grand plan's Phase 8 stub should be marked as superseded in a
    follow-up edit.

- [ ] Define extension YAML schema for engines, extending Quarto 1's schema:
  ```yaml
  contributes:
    engines:
      - path: julia-engine.js     # required: path to bundled JS (must end in lowercase .js)
        name: julia               # optional: engine's runtime name
        languages: ["julia"]      # optional: languages this engine might claim
        file-extensions: [".jl"]  # optional: file extensions this engine might claim
      - jupyter                   # string form: reordering hint
  ```
  **Quarto 1 reference:** The extension schema (in
  `external-sources/quarto-cli/src/resources/schema/extension.yml`) defines
  engines as an array of either strings (engine names for reordering) or
  objects. Q1's `external-engine` definition
  (`external-sources/quarto-cli/src/resources/schema/definitions.yml`)
  is `closed: true` with a single `path` property. Both forms are
  allowed in both `_extension.yml` and `_quarto.yml` (identical schema).

  **q2 extensions to the schema:** `name`, `languages`, and
  `file-extensions` are new fields q2 adds alongside `path`. They are
  declarative hints that let q2 skip the Deno subprocess when an
  extension is clearly irrelevant or its name is already known:
  - `name` lets q2 register the engine and resolve YAML lookups
    (`engine: julia`, top-level `julia:` keys) without `LoadEngine`.
    When provided, q2 verifies it matches `LoadEngineResult.name` at
    first load — mismatch is a hard error.
  - `languages` / `file-extensions` are conservative supersets — they
    list what the engine *might* claim. The dynamic
    `claimsLanguage`/`claimsFile` functions in the TS engine are the
    precise check. `[]` is a valid declaration of "claims none."

  **Authors who omit any hint get a warning** at extension-discovery
  time pointing them to the YAML changes that would silence it.

  The Julia engine's `_extension.yml` uses `- path: julia-engine.js`
  pointing to the pre-built bundle. With `name: julia` declared, q2
  registers it under that name immediately. Without `name`, q2 falls
  back to a lazy `runtime_name → extension_id` alias map populated on
  first load (see Phase 2 below).

  In q2, `path` **must** point to a pre-built `.js` bundle (lowercase
  extension). q2 validates this at extension parse time and rejects
  `.ts`, `.mjs`, `.JS`, etc. with an actionable error.

  **Backward compatibility note:** Because Q1's `external-engine`
  schema is `closed: true`, a `_extension.yml` declaring any q2-only
  field will fail Q1's validator. q2 engine extensions are q2-targeted
  and not portable backward.

- [ ] Provide a template build config:
  - A template `deno.json` (`resources/extension-build/deno.json`) whose
    imports reference the **published** SDK and std lib (no q2-local import
    map for the SDK):
    - `@quarto/api` → `jsr:@quarto/api` (real code, inlined by `deno bundle`;
      each compiled `julia-engine.js` freezes the `@quarto/api` version it
      built against — managed by semver on the published package)
    - `@quarto/types` → `jsr:@quarto/types` (type-only, erased)
    - `@std/*` → `jsr:@std/*`
  - Engine authors copy/extend this `deno.json` in their extension. Within the
    q2 repo, a workspace mapping resolves `@quarto/api` / `@quarto/types` to
    `ts-packages/…` for dev builds. See the grand plan's "Distribution of the
    engine-author SDK".

- [ ] Implement a `q2 build-ts-extension` subcommand. CLI subcommands are
  defined using `clap` in `crates/quarto/src/main.rs` — add a new variant to
  the `Commands` enum, create a handler module in `crates/quarto/src/commands/`,
  and add the match arm in `main()`. Behavior:
  - Optional path argument; defaults to cwd-detected `_extension.yml`.
  - Reads the TS entry by convention (e.g., `src/<name>.ts` adjacent to
    `_extension.yml`).
  - Runs `deno bundle --config=resources/extension-build/deno.json <entry.ts>`,
    writing the output to the location referenced by `path` in `_extension.yml`.
  - This mirrors Quarto 1's `quarto call build-ts-extension`.
  - Extension authors run this after editing TS source. q2 never runs it
    during render.

  **Distribution scope.** `q2 build-ts-extension` resolves `@quarto/api` and
  `@quarto/types` from the registry (jsr/npm) via the engine's `deno.json`, so
  it works identically from an installed binary and from a q2 clone — no build
  assets are embedded in or extracted from the q2 binary. See the grand plan's
  "Distribution of the engine-author SDK".

- [ ] Scan `_extensions/` for engine contributions during project initialization.
  **Quarto 1 reference:** `resolveEngineExtensions()` in `external-sources/quarto-cli/src/project/project-context.ts` discovers extensions with `contributes.engines`, merges them into `projectConfig.engines`. Then `resolveEngines()` in `external-sources/quarto-cli/src/execute/engine.ts` imports and registers them.

- [ ] For each discovered engine:
  1. Check if the bundled `.js` referenced by `path` exists.
  2. If not, fail extension load with: `Engine extension '{name}' has no
     bundled .js file at {expected_path}. Run 'q2 build-ts-extension' in
     {ext_dir} to build it.` No auto-building.
  3. Create a `TsEngine` instance pointing to the bundled `.js`, with the
     parsed `name_hint` / `language_hints` / `file_extension_hints`
     (all `Option<...>`).
  4. **Determine the registry key.** If `name_hint` is `Some(name)`,
     register the `TsEngine` under that name immediately — no subprocess
     spawn. If `name_hint` is `None`, register under the extension id
     (e.g., the extension directory name like `julia-engine`) as a
     placeholder, and remember the engine in a separate
     `runtime_name → extension_id` alias map on the registry. The alias
     map is empty on registration; it's populated when `LoadEngine` runs
     and resolves the engine's true name.
  5. Register it in the `EngineRegistry`. **On collision** (another
     engine — built-in or extension — already registered under the
     chosen key) emit a hard error naming both contributors. q2 chooses
     a stricter behavior than Q1, which silently replaces external
     engines (Q1's `resolveEngines` uses raw `kEngines.set()` for
     externals while `registerExecutionEngine` throws for built-ins —
     an asymmetry q2 deliberately closes).
- [ ] **Support `_quarto.yml engines:` list for ordering, matching Q1.**
  Following Quarto 1's `resolveEngineExtensions` + `resolveEngines`
  pipeline (project-context.ts:739–795 and engine.ts:213–300):
  1. Extension-contributed engines are appended to
     `projectConfig.engines` after any `_quarto.yml`-declared entries.
  2. The combined list is walked: object entries (External engines)
     are registered AND their names pushed into the user-specified
     order; bare-string entries (Reorder hints) are pushed into the
     user-specified order without registering anything.
  3. **Validate every name in the user-specified order is registered.**
     If a Reorder hint names an engine that's not in the registry,
     error out at config-resolve time with the live registry listed
     (matches Q1 engine.ts:275–283: `'X' was specified in the list of
     engines... but it is not a valid engine. Available engines are
     ...`). No silent skip.
  4. **Final order:** user-specified entries first (deduplicated, in
     listed order), then the remaining built-ins in their registration
     order: `knitr → jupyter → markdown` (matching Q1 engine.ts:49–53).
  5. **Auto-promotion:** because `External` (object-form) engines push
     their name into the user-specified order during registration, an
     installed extension's engine ends up at position 0 by default,
     ahead of built-ins. This is intentional and matches Q1 — it's
     what makes `claimsLanguage` competitive without explicit user
     intervention. The static-hints mechanism (`languages`,
     `file-extensions`) keeps installed-but-unused engines silent
     during detection.
  6. **Duplicate hints in the order list** (same name listed twice)
     are silently idempotent (Map dedup, matches Q1).
  7. This ordering is the **final tiebreak** in resolution (design doc §4):
     when two engines have the same-kind claim at the same priority for a
     language, the one earlier in the order wins.
- [ ] Update engine detection to recognize extension-provided engine
  names. With `name` declared in `_extension.yml`, the registry already
  has the engine keyed by name — top-level YAML key scanning
  (`registry.engine_names()`) and `engine: foo` lookups both succeed
  with zero subprocess load. Without `name`, the alias map needs to
  be populated first (see Phase 2 below).
- [ ] Support `engine: julia` in document YAML triggering the extension
  engine. Lookup probes the direct map first, then the alias map. On
  miss across both, lazy-load all hint-less unloaded TS engines to
  populate the alias map, then retry. The missing-`name` warning
  steers authors away from this slow path.
- [ ] Write test: fixture extension directory → build → engine registered and detectable
- [ ] Write test: `_quarto.yml` `engines:` list controls ordering
- [ ] Write test: `_quarto.yml engines: [foo]` where `foo` is unknown →
  hard error at config-resolve time, listing available engines (matches Q1)
- [ ] Write test: two extensions both declare `name: julia` → collision
  error at registration with both contributors named
- [ ] Write test: extension declares `name: julia`, registers cleanly,
  YAML `engine: julia` resolves with no subprocess spawn
- [ ] Write test: extension omits `name`, YAML `engine: <runtime-name>`
  triggers lazy `LoadEngine` and resolves via the alias map
- [ ] Write test: extension declares `name: julia`, `LoadEngine` returns
  `name: "jupyter"` → hard error pointing at the YAML mismatch
- [ ] Write test: extension with `path: src/engine.ts` → parse fails
  with actionable error pointing to `q2 build-ts-extension`
- [ ] Write test: extension with `path: bundle.JS` (uppercase) or
  `path: bundle.mjs` → parse fails with the same actionable error
- [ ] Write test: extension with missing `.js` bundle → registration fails
  with actionable error
- [ ] Write tests for the missing-hints warning across the field
  matrix: each of `name`, `languages`, `file-extensions` independently
  missing or present; mixed combinations. Verify `Some(vec![])` for
  `languages`/`file-extensions` is silent (a valid declaration of
  "claims none").

### Phase 2: Engine resolution + registry migration

Replace metadata-only engine detection with the tiered **engine-resolution
model** (design doc §4), build the `Arc<EngineRegistry>` on `ProjectContext`,
and restructure the pipeline entry point so that `claimsFile` runs before
`ParseDocument`. Resolution produces an `EngineResolution { sequence, ownership }`
artifact (design doc §9), not a single engine.

**Current state (post-rebase main):** `detect_engine_sequence(meta) ->
EngineSequence` already exists (multi-engine, bd-5yff4) but is **metadata-only**
— it reads the `engine:` array / top-level key and has no language-based or
claims-based resolution (`detection.rs` still carries a "Future Enhancements"
comment for language/extension detection). `EngineExecutionStage` owns the
`EngineRegistry` as a direct field plus a `spliced_engines: HashSet<String>`
(bd-sauc9iiq, preview capture-splice) and its `run()` takes `&self`, so it
cannot mutate the registry. This phase upgrades `detect_engine_sequence` into
the claims-based `resolve_engines` and moves the registry off the stage.

**The model** — claim interface (`LanguageClaim` = `Primary`/`Interop`/
`Fallback`/`None`), the four resolution tiers (Primary → explicit-Fallback →
Interop → implicit-Fallback), presence-gating, per-language ownership,
`first_class`-drives-selection, jupyter as `Fallback(0)`, and the
structural definition of "computational language" — lives in the design
contract (design doc §3–§4). **Do not restate it here.** The work items below
wire it into q2.

**Modernization (now expressed in the model):** Quarto 1's Jupyter claimed
"julia" at priority 1, conflicting with the Julia extension. In q2 Jupyter
declares `Fallback(0)` (never a positive claim), so the Julia extension's
`Primary(1)` wins `julia` cleanly, and Jupyter still catches unclaimed
computational languages via the implicit-Fallback tier — Q1's Phase-4 behavior
folded into the same scoring (design doc §4.3).

**Pre-parse engine detection:** Phase 1 (`claimsFile`) must run BEFORE
`ParseDocument`, because if an engine claims a non-QMD file (e.g., `.jl`
percent script), the engine must convert it to QMD before pampa can parse it.
The flow is:

```
Input file
  │
  ├─ claimsFile: engine claims it (non-QMD) ─→ markdownForFile ─→ QMD text
  │                                                                   │
  └─ claimsFile: no engine claims it ─────────────────────────────────┤
                                                                      ▼
                                                              ParseDocument
                                                                      │
                                                              (rest of pipeline)
```

For QMD files, no engine claims via `claimsFile` (`.qmd` is not a
percent/spin format), so parsing proceeds directly and `claimed_engine_name`
stays `None`. Engine resolution then runs in Pass 2 over the parsed AST
(`resolve_engines` — design doc §4). For a claimed non-QMD file, the claiming
engine is recorded in `claimed_engine_name` and seeds resolution as the
converted content's `Primary` owner (design doc §8).

For non-QMD files (`.jl`, `.py`, `.r`, `.ipynb`), an engine claims the
file, provides QMD text via `markdownForFile`, and that text enters the
pipeline. For TS engines, this requires the Deno subprocess to be running
— it's lazily spawned on first `claimsFile` query.

- [ ] **Move `EngineRegistry` and `Arc<TsEngineHost>` to `ProjectContext`.**
  Currently `EngineExecutionStage` owns the registry (created with built-ins
  only in `new()`). Project rendering (post-merge with `main`) puts every
  per-file render through its own `StageContext`, so the registry — and the
  shared subprocess host inside it — must live above `StageContext` to be
  shared across passes and across files.

  Build them once at `ProjectContext` construction time. Extension discovery
  is already a `ProjectContext` concern (`ctx.extensions`), so registry
  construction is the natural pairing for that data.

  Construction sequence (executed by `ProjectContext::new` or its caller,
  after extensions are discovered and `BinaryDependencies::discover()`
  has run):

  1. **Build the static `EngineHostContext`** from project sources:
     `project_dir`, `is_single_file`, `resource_dir`, `runtime_dir`
     (via `quarto_util::quarto_runtime_dir()` — plan1a-host),
     `pandoc_path: Option<String>` (from `BinaryDependencies.pandoc`,
     stringified if `Some`), `is_interactive_session`, `running_in_ci`,
     `quarto_version` (`env!("CARGO_PKG_VERSION")`).
  2. Construct `Arc::new(TsEngineHost::new(host_context))` once,
     stashing the context. The subprocess is **not** spawned here —
     `TsEngineHost::new` is cheap; first `ensure_started()` (triggered
     by the first protocol round-trip) is what actually launches Deno.
  3. Start with `EngineRegistry::new()` (built-in engines: knitr,
     jupyter, markdown — registered in that order to match Q1's
     tie-break order in engine.ts:49–53). The registry struct shape
     (the immutable `engines` map plus `aliases` and `diagnostics`
     mutex-protected fields) is defined in plan1a-engine alongside
     the code that mutates the fields; `EngineRegistry::new()`
     constructs an instance with empty `aliases` and `diagnostics`
     vectors, ready for population in steps 5–10.
  4. Scan project extensions (`ctx.extensions`) for engine contributions
     (`contributes.engines`).
  5. For each `EngineContribution::External`, create a `TsEngine`
     instance — clone the `Arc<TsEngineHost>` into it, copy the
     `name` / `language_hints` / `file_extension_hints`. Register it
     in the registry under either `name_hint` (if declared) or the
     extension id (if not). On collision, hard error.
  6. For each `EngineContribution::Reorder { name }`, add the name to
     the user-specified ordering list (does not register anything).
  7. Apply Q1-faithful ordering (see the dedicated step further below).
  8. Validate every name in the user-specified order is registered;
     hard error if not.
  9. Store the result on `ProjectContext` as
     `pub registry: Arc<EngineRegistry>`. (The `Arc` wraps the
     registry so that per-file `StageContext` clones are cheap and
     share the same registry instance.) The `Arc<TsEngineHost>` lives
     inside the registry's `TsEngine` instances; the `EngineRegistry`
     value itself is the shared root.
  10. **Drain `registry.diagnostics`** and emit directly to the
      user-facing diagnostic sink. q2's per-document
      `StageContext.diagnostics` aggregator (`stage/context.rs:84`)
      collects execution-time diagnostics and forwards them via
      `RenderOutput.diagnostics`; init-time diagnostics from registry
      construction don't pass through that aggregator because they're
      project-scoped, not per-document. The orchestrator emits them
      directly — same destination as `RenderOutput.diagnostics` but a
      separate channel into the same sink. (Future plan: if a
      cross-document project-render output type emerges, init-time
      diagnostics could be folded in there. Out of scope.)

  **Registry `Arc` ownership (the Clone-drop already happened in plan1a-engine).**
  plan1a-engine adds `aliases` / `diagnostics` `Mutex` fields (not `Clone`), so
  **plan1a-engine already dropped `#[derive(Clone)]` and introduced
  `Arc<EngineRegistry>` at the ~25–30 mechanical clone sites** (incl.
  `HtmlRenderConfig` / `with_engine_registry` and the `quarto-preview`
  pass-through chain — see plan1a-engine's "Migration from `main`'s registry"
  note for the verified site list; mandatory-to-compile there, not optional
  cleanup). **Plan 1c does the *deeper* ownership move on top of that `Arc`:**
  hoist it to `ProjectContext`, build once, thread per-file via `StageContext`.
  Do **not** re-drop the derive or re-audit the same clone sites here — that is
  plan1a-engine's done work; 1c only relocates ownership.

  **Per-file threading.** `StageContext` gains
  `pub registry: Arc<EngineRegistry>` and is populated when the
  per-file `RenderContext` builds its `StageContext` (see
  `crates/quarto-core/src/pipeline.rs::run_pipeline`, which already
  follows this pattern for `project_index` and `resource_resolver`).
  `EngineExecutionStage` becomes stateless — its `run()` reads
  `ctx.registry`. Remove the `registry` field from
  `EngineExecutionStage`, but **preserve its `spliced_engines`**
  (bd-sauc9iiq, the preview capture-splice set) — that is per-render
  preview state, not registry state, and the stateless refactor must keep
  threading it (e.g. via `StageContext` or the stage constructor). The
  `with_registry()` test constructor is replaced by tests that build a
  `ProjectContext` with a custom registry and let it flow into
  `StageContext` naturally.

  **Resolution artifact on `StageContext`.** `EngineExecutionStage::run`
  calls `resolve_engines(meta, ast, &ctx.registry, ctx.claimed_engine_name)`
  once at the top and stashes the resulting `EngineResolution { sequence,
  ownership }` on `StageContext` (`pub engine_resolution:
  Option<EngineResolution>`), mirroring `project_index`. The execution loop
  reads `ownership` to build each engine's `handled_languages` via
  `EngineResolution::handled_languages_for`; the trace records `sequence`.
  `resolve_engines` is a pure function in `crates/quarto-core/src/engine/
  resolution.rs`, unit-testable with mock claim tables (design doc §9).

  **Replay** (design doc §6.2): with claims-based resolution, the replay path
  must **drive from the recorded `engine_captures` in order**, not re-run
  resolution (`ReplayEngine`s carry no claims, so re-resolving an implicit doc
  would produce the wrong sequence). This likely lets `with_replay_many` /
  `ReplayEngine` be replaced by a capture-driven replay path — which also
  avoids injecting engines into the now-immutable `Arc<EngineRegistry>`.

  **Why `ProjectContext`, not `ProjectPipeline`.** `ProjectContext`
  is the carrier already shared across single-doc and project
  renders; placing the registry there keeps the `Arc<TsEngineHost>`
  reachable for both flows without `ProjectPipeline` having to be
  the only constructor. Single-doc renders go through
  `DefaultProjectType` and pick up the registry the same way.

- [ ] **Wire orchestrator-driven shutdown of the engine subsystem.**
  q2's convention is **explicit shutdown methods, not Drop** (see
  `JupyterDaemon::shutdown_all` at
  `crates/quarto-core/src/engine/jupyter/daemon.rs:272-279`;
  `ProjectContext` does not have a `Drop` impl today). The
  orchestrator (the same code path that drops `ProjectContext` at
  end-of-render) explicitly calls `registry.shutdown_all()` before
  letting the context drop. `registry.shutdown_all()` iterates the
  unique `Arc<TsEngineHost>` clones held by `TsEngine` instances and
  calls `host.shutdown(&self)` on each (it is `&self` — the host is
  reached through `Arc`). `host.shutdown()` is idempotent; calling it on
  a never-spawned host is a no-op. Errors are propagated via `Result` to
  the caller (which can log at WARN and continue with teardown — failure
  to shut down cleanly is not fatal because **`TsEngineHost::Drop`
  SIGKILL-reaps the subprocess and joins the reader threads as a
  backstop**). **NB (corrected per plan1a-host's reworked teardown):**
  the host uses `std::process::Child`, which has **no `kill_on_drop`** —
  that is a `tokio::process::Command` method. The unconditional reap is
  the host's *explicit* `Drop` impl (SIGKILL + single-shot `wait()` +
  `join`), **not** `kill_on_drop`. The subprocess's stderr-reader thread
  terminates on EOF as part of process exit; the host joins it.

- [ ] **`StageContext` plumbing for `BinaryDependencies`.** The
  `EngineHostContext` built in step 1 of registry construction reads
  `BinaryDependencies.pandoc` for `pandoc_path`. Today
  `BinaryDependencies` is constructed in
  `crates/quarto-core/src/render.rs:53-60` per render and is not on any
  shared context. Move (or alias) it onto `ProjectContext` alongside
  `registry`, so both registry construction (step 1) and any future
  stage that needs binary discovery (sass, esbuild, typst) can read
  from one source. Single-doc renders constructing
  `DefaultProjectType` follow the same pattern.

- [ ] **Add `claimed_engine_name: Option<String>` to `StageContext`.**
  Set by the pre-parse stage (below) when an engine claims a file via `claimsFile`.
  Passed into `resolve_engines` as the **`Primary` seed** for the converted
  content (design doc §8): the claiming engine is seeded as a `Primary` owner
  (above extensions for the file's native language, so a generic extension
  cannot steal a converted notebook), and **resolution still runs** so other
  languages get secondary owners. This replaces the original "skip Phases 2–4"
  behavior — conversion (pre-parse) and execution (resolved) are separate axes.

- [ ] **Extend `LoadedSource` with conversion provenance (v1 scope = C′,
  per plan1a-engine SEAM-3 — decided 2026-06-24).** plan1a-engine scopes
  `markdown_for_file` to **C′**: it returns `(text, SourceInfo::default())`,
  and the converted text's provenance is invented downstream by registering it
  as an **ephemeral intermediate file under an engine-reflecting synthetic
  name** — honest `Original` positions *into the converted buffer*, with
  faithful original-file mapping (the remap walk, the dual-registration of the
  original bytes, real `SourceInfo::Concat`) **deferred to "A′"** (commendable,
  but not these plans). So the v1 carrier only needs the **converting engine's
  name** to build that synthetic label:
  ```rust
  pub struct LoadedSource {
      pub path: PathBuf,            // user's input path (e.g. foo.jl) — never rewritten
      pub content: Vec<u8>,         // QMD bytes after conversion; raw bytes otherwise
      pub source_type: SourceType,  // Qmd after conversion
      pub conversion: Option<ConversionProvenance>,
  }

  pub struct ConversionProvenance {
      /// Name of the engine that produced the converted QMD — used only to
      /// build the synthetic intermediate-file label
      /// `"<{original} (converted by {engine})>"`. (v1 / C′.)
      pub engine: String,
      // A′ (deferred — NOT built in these plans): faithful original-file
      // back-mapping would add `original_content: Vec<u8>` + a `source_info:
      // SourceInfo` (the converted→original map) here, consumed by a remap
      // walk. Out of scope; see "Conversion provenance" in Design Notes and
      // engine-resolution.md §13.
  }
  ```
  This is the carrier from `EngineClaimsFileStage` to `ParseDocumentStage`.

- [ ] **Create `EngineClaimsFileStage`** — a new `LoadedSource → LoadedSource`
  pipeline stage inserted before `ParseDocumentStage`. **It must be inserted in
  BOTH builders:** the full per-file pipeline (`build_html_pipeline_stages`)
  **and the Pass-1 indexing builder** (`pass1_profile_single_file_live` in
  `crates/quarto-core/src/project/orchestrator.rs`). This is load-bearing: Pass 1
  advances every file to the `DocumentProfile` checkpoint, so a non-QMD input
  (`.ipynb`, `.jl`) that isn't converted before parse yields a **garbage
  `DocumentProfile`** (and a garbage `ProjectIndex` entry). The Pass-1 builder
  currently also omits `IncludeResolveStage` / `ListingItemInfoStage` that the
  full pipeline runs before the profile checkpoint — reconcile the two stage
  lists when inserting (the profile should observe the same pre-mutation state
  in both passes). Resolution itself stays Pass-2-only (design doc §7); only the
  file-claim/convert half runs in Pass 1, and it spawns an engine only when a
  doc genuinely needs conversion.
  This stage:
  1. Gets the file extension from `LoadedSource.path`. **Normalize
     the path** to absolute + lexically normalized (no symlink
     resolution) before any engine call — matches plan1a-protocol's "Path
     conventions on the wire" appendix. For TS engines this is
     redundant (TsEngine re-normalizes at the protocol boundary), but
     for built-in engines that grow `claims_file` overrides later
     (Plan 1c "Future Work" section), and for any path-equality
     comparisons in `claims_file_cache`, normalization here keeps the
     stage's behavior consistent regardless of how the path entered
     the pipeline.
  2. For each engine in `ctx.registry` (in order), calls `claims_file(file, ext)`.
     For TS engines, the static-hint pre-filter in `TsEngine::claims_file`
     short-circuits engines whose hints don't match the extension — no
     subprocess load for those.
  3. If an engine claims the file, calls
     `markdown_for_file(file, &ctx.runtime)` to get `(qmd_text, source_info)`.
     In v1/C′ `source_info` is `SourceInfo::default()` (ignored here); the
     trait returns `(String, SourceInfo)` natively — no protocol-type stitching
     at this layer.
  4. Builds a `ConversionProvenance { engine: <claiming engine name> }` (v1/C′
     — just the engine name for the synthetic label; the original bytes +
     `source_info` are A′-deferred, see above). Replaces `source.content` with
     the QMD bytes, sets `source.source_type = Qmd`, sets
     `source.conversion = Some(provenance)`. `source.path` stays as the user's
     input path.
  5. Stores `ctx.claimed_engine_name = Some(engine.name().to_string())`.
  6. If no engine claims the file, passes through unchanged (the common case for `.qmd`).
  For TS engines that survive the static-hint filter, this lazily spawns the
  Deno subprocess + sends `LoadEngine` (then `LaunchEngine` if the engine
  claims) on first need.
  **`.qmd` cost note:** for every TS engine in the registry whose
  `file_extension_hints` is `None` (hint-less), this stage triggers a
  `LoadEngine` + `ClaimsFile` round-trip on every render even when
  the file is `.qmd`. Authors avoid this by declaring
  `file-extensions: []` (or a non-empty list). The missing-hints
  warning at extension-discovery time is what nudges them. Worth
  noting in user-facing extension-author docs once those exist.
  **WASM note:** A future plan will need to include this stage in the WASM pipeline
  (built-in engines may eventually claim `.ipynb` etc. without Deno).

- [ ] **Update `ParseDocumentStage` to consume `LoadedSource.conversion`
  (v1 / C′ — single registration, no remap).** When
  `source.conversion.is_some()`:
  1. Register the **converted** QMD content in the source_context under the
     **engine-reflecting synthetic name**
     `format!("<{} (converted by {})>", source.path.display(), conversion.engine)`
     — `add_file(synthetic_name, Some(qmd_text))` allocates the FileId; call
     it `qmd_id`. (The synthetic name names the *converting engine* so the
     buffer never masquerades as the original file's bytes — positions are in
     the converted view; plan1a-engine SEAM-3.)
  2. Pass the synthetic name as the parser filename so AST nodes get honest
     `SourceInfo::Original(qmd_id, qmd_range)` into the converted buffer.
     **The qmd parser already does this `add_file`-and-stamp** at `qmd.rs:106`,
     so for the normal convert-then-parse path the FileId is invented for free.

  That is the whole v1 path — no dual-registration of the original, no remap.
  When `source.conversion.is_none()`, `ParseDocumentStage` runs as today.

- [ ] **(A′ — DEFERRED, not these plans) byte-range AST source_info remap.**
  Faithful converted-cell → original-cell positions would: register the
  original bytes under `source.path` (a second FileId `original_id`); add a
  `remap_via_source_info` walker (extend `quarto_ast_reconcile::remap_file_ids`
  with a byte-range translator) that rewrites each `Original(qmd_id, range)` to
  `Original(original_id, mapped_range)` via the converted→original map; and feed
  it the real `source_map` the harness already serializes on the wire (consumed
  here, ignored in v1). This is the "A′" generalized-remap path plan1a-engine
  SEAM-3 prefers when a consumer appears; it is **out of scope** for Plan 1c v1
  (no consumer yet, and it depends on engines computing real provenance, which
  they don't). Listed here so the seam is known, not as a v1 deliverable.

- [ ] **Remove the `KNOWN_ENGINES` constant and `is_known_engine()` function**
  from `detection.rs`. Currently hardcoded as
  `["markdown", "knitr", "jupyter"]`. With extension engines, the set
  of known engines is dynamic — it's whatever's in the registry.
  Replace usage with a query against the registry's engine names:
  `registry.engine_names()`. `is_known_engine` had no callers outside
  detection itself; just delete it.

- [ ] Implement `resolve_engines` (upgrade of `detect_engine_sequence`).
  Signature: `resolve_engines(meta, ast, registry, claimed: Option<&str>) →
  EngineResolution { sequence, ownership }` (design doc §4, §9). It supersedes
  the metadata-only `detect_engine_sequence` on `main`; keep a thin
  `detect_engine_sequence`-compatible shim if any non-execution caller still
  needs just the sequence. The algorithm — the four tiers (Primary →
  explicit-Fallback → Interop → implicit-Fallback), presence-gating,
  per-language ownership, structural computational-language extraction from the
  AST, and `claimed` as the `Primary` seed — is specified in the design
  contract and **must not be restated here**. The wiring obligations are:
  - **File-claim seed**: when `claimed` is `Some(name)`, seed that engine as
    `Primary` for the doc and still run the tiers (design doc §8) — do **not**
    short-circuit.
  - **AST language extraction**: extract `(language, first_class)` of executable
    cells from the **parsed AST** (not regex — q2 has pampa), minus
    `HANDLED_LANGUAGES` and raw `{=fmt}` blocks (design doc §4.1).
  - **Top-level YAML key selection** stays: a top-level key matching a
    registered engine name (e.g. `julia: 1.10`) selects that engine, scanning
    `registry.engine_names()` (replacing the deleted `KNOWN_ENGINES`). Same as
    Q1 `markdownExecutionEngine` (engine.ts:161–169); document for users.
  - **Built-ins**: knitr `Primary(1)` for `r` + `Interop` for reticulate-reachable
    languages; jupyter `Fallback(0)`; markdown `None`. No built-in implements
    `claims_file` in Plans 1a/1c scope (TS extensions do — Julia for `.jl`); the
    file-claim path drives `claimed` for them.
  - **Per-engine `handled_languages`** are derived from `ownership` (design doc
    §5) and threaded into each engine's execute via the new `ExecutionContext`
    field; for non-terminal **jupyter** this requires the execute-time
    enforcement gate (plan1a-engine).
- [ ] For language extraction from AST: use pampa's existing parsing to get code block
  languages and their classes, rather than regex. Quarto 1 uses
  `languagesWithClasses()` regex on raw markdown; we should use the parsed
  tree-sitter AST instead.
- [ ] **Cache `claimsFile` results per render**, keyed on canonical path.
  Implementations may inspect file content (e.g., Julia engine reads the
  file to check for percent script `# %%` markers), but file contents
  don't change during a single render, so caching across the project
  scan is safe. See plan1a-engine for the `claims_file_cache` field
  on `TsEngine` and its `ProjectContext`-scoped lifetime. **Cache
  `claimsLanguage` results** per engine per `(language, first_class)` pair.
- [ ] When a document has an explicit `engine: julia` in YAML, skip discovery entirely
  — just look up the engine by name in the registry. This is the common case and
  requires zero subprocess calls.
- [ ] Write test: engine claims "julia" language, document with `{julia}` blocks selects it
- [ ] Write test: explicit `engine: julia` in YAML skips discovery, resolves directly
- [ ] Write test: priority scoring — higher score wins over lower score
- [ ] Write test: unclaimed computational language → implicit-Fallback to Jupyter
- [ ] Write test: no executable cells → markdown engine (empty sequence)
- [ ] Write test: extension engine registered in context, discoverable by name
- [ ] Write test: implicit `{r}`+`{python}` → `[knitr]` (knitr `Interop` python; reticulate preserved)
- [ ] Write test: explicit `engine: [knitr, jupyter]`, `{r}`+`{python}` → `[knitr, jupyter]`
  with `ownership` = {r→knitr, python→jupyter} and knitr's `handled_languages` ⊇ {python}
- [ ] Write test: pure `{python}`, no python extension → `[jupyter]` (knitr **not** dragged in)
- [ ] Write test: file-claim seed — a claimed `.echo`/`.jl` file makes the claimer
  `Primary`, and a second-language cell still resolves to its own owner (secondary)
- [ ] Write tests for the tier/presence/fallback logic against mock claim tables
  (see plan1a-engine's `resolve_engines` unit tests — Plan 1c exercises the
  end-to-end path; the pure-logic tests live with the function)

**Failure model** (design doc §10 — Q1 parity; resolution is availability-blind):
- [ ] Non-QMD file whose extension is claimed by no engine's `valid_extensions`
  → **loud** error `"Can't determine execution engine for <file>"` (Q1
  `engine.ts:317→366`); `.qmd`/`.md` always resolve. Fired in
  `EngineClaimsFileStage`. Write test.
- [ ] A resolved **owning** engine whose runtime is unavailable (`is_available()`
  false) → **loud**, actionable error naming the engine + what's missing + how
  to install (Q1 style: *"Unable to locate an installed version of R / Python 3…"*).
  Availability checked **after** resolution; **no** silent re-route to a fallback,
  **no** degradation. In a multi-engine sequence, any unavailable owner fails the
  whole render loudly, naming the engine/language. Write tests.
- [ ] Language with no claim → **graceful** jupyter/markdown fallback (not an
  error). Already covered by the fallback tests above.
- [ ] **A resolved owner is available but owns a language it cannot execute**
  (design doc §10 case 4 — added 2026-06-24). The tiers can route a language to
  an engine that owns it but has no handler/kernel: `engine: [knitr, jupyter]`
  with `{sql}` routes `sql → jupyter` via explicit-`Fallback`, but jupyter has
  no SQL kernel (knitr's `eng_sql` does). The owner MUST fail **loudly** — a
  clear `ExecutionError::NoHandlerForLanguage { engine, language }` ("engine
  `jupyter` has no kernel for `sql`") — and MUST NOT silently skip or emit the
  cell unexecuted. **This is an execute-time failure by design** (not a
  pre-execute capability probe): resolution stays capability-blind so engine
  *selection* is deterministic and environment-independent — the property that
  lets it lift to Pass-1 (design doc §10). So the render runs knitr's `{r}`
  cells, then halts at `{sql}`. **Applies to TS engines too:** an extension
  engine handed (via `TsExecuteOptions.handled_languages`) a cell in a language
  it owns but can't run must return a protocol `error`, surfaced loudly — never
  a silent pass-through. `NoHandlerForLanguage` is a clean refusal, so it does
  **not** poison the instance. Write a test (the `[knitr, jupyter]` + `{sql}`
  route, plus a TS-engine owned-but-unrunnable case once the echo engine
  supports it).

### Phase 3: Echo engine integration test

End-to-end test with a minimal TypeScript engine that exercises **both**
discovery paths: language claiming (the resolution tiers) and file
claiming (the pre-parse flow). Without the file-claiming half, the
`EngineClaimsFileStage` + `markdown_for_file` pipeline gets no E2E
coverage in Plan 1c — Plan 4 (Julia + `.jl`) would be its first
end-to-end exercise.

**Dependency note:** The echo engine imports types from `@quarto/types`. If Plan 2
Phase 2E hasn't defined these yet, create a minimal type stub inline in the echo
engine file (just the interfaces it needs: `ExecutionEngineDiscovery`,
`ExecutionEngineInstance`, `QuartoAPI`). These can be replaced with proper imports later.

- [ ] Create test fixture `tests/fixtures/extensions/echo-engine/`:
  ```
  _extension.yml
  src/echo-engine.ts
  fixtures/lang.qmd       # { echo } code block fixture
  fixtures/file.echo      # whole-file echo fixture
  ```
  `_extension.yml` declares `name: echo`, `languages: ["echo"]`,
  `file-extensions: [".echo"]` so the extension exercises the
  zero-subprocess-on-mismatch fast path as well.
- [ ] `echo-engine.ts` — claims `"echo"` language AND `.echo` files:
  ```typescript
  const echoEngine: ExecutionEngineDiscovery = {
      name: "echo",
      claimsLanguage: (lang) => lang === "echo",
      claimsFile: (_file, ext) => ext === ".echo",
      launch: (ctx) => ({
          name: "echo",
          canFreeze: false,
          // For .echo files: wrap whole file as a single {echo} code block
          // so the resulting QMD then runs through the language path.
          markdownForFile: async (file) => {
              const text = await Deno.readTextFile(file);
              return {
                  value: "```{echo}\n" + text + "\n```\n",
                  fileName: file,
                  sourceMap: [],   // no provenance tracking — matches Q1
              };
          },
          execute: async (opts) => ({
              engine: "echo",
              markdown: opts.target.markdown.value.replace(
                  /```\{echo\}[\s\S]*?```/g,
                  "**ECHO_EXECUTED**"
              ),
              supporting: [],
              filters: [],
          }),
          // ... minimal stubs for other methods
      }),
  };
  export default echoEngine;
  ```
- [ ] Write Rust integration test covering **both fixtures**:
  1. Set up project with echo engine extension.
  2. Render `lang.qmd` (a `.qmd` with `{echo}` blocks). Verify output
     contains `ECHO_EXECUTED`. This exercises:
     registry → `resolve_engines` (language claim → ownership) → execute.
  3. Render `file.echo` (a non-QMD file claimed by extension). Verify
     output contains `ECHO_EXECUTED`. This exercises:
     registry → `EngineClaimsFileStage` → `markdown_for_file` →
     `LoadedSource.conversion` populated → `ParseDocumentStage` registers the
     converted text under the engine-reflecting synthetic name (C′ — single
     registration, AST nodes get `Original(qmd_id)` into the converted buffer) →
     `claimed_engine_name` propagated → execute.
  4. Use either `cargo run --bin q2 -- render <file>` (the `quarto`
     crate is the main CLI binary) or a Rust test that programmatically
     drives the render pipeline through `render_document_to_file` —
     check existing tests in `crates/quarto/tests/` for patterns.
- [ ] This pair of tests validates the full pipeline for both discovery
  paths: discovery → subprocess spawn → `LoadEngine` → discovery query
  (claimsLanguage / claimsFile) → `LaunchEngine` → markdownForFile (for
  the `.echo` case) → execute → result.
- [ ] **Verify teardown end-to-end (this is the home plan1a-host defers to).**
  plan1a-host unit-tests teardown via `MockTransport`/the spike but explicitly
  defers the *real* shutdown-on-render-end verification to Plan 1c ("Lifecycle
  caller is Plan 1c"). So after the render completes, assert the Deno subprocess
  **exited cleanly**: capture the child pid (or expose `host.is_alive()`), and
  after `ProjectContext` teardown assert the process is gone (no zombie). This
  exercises the orchestrator's explicit `registry.shutdown_all()` → `host.shutdown()`
  → close-stdin → child-exit → reader-thread-join path that no other test covers.
- [ ] **(Optional, lower priority) crash-path E2E.** A third fixture whose echo
  engine `Deno.exit(1)`s (or is killed) mid-`execute` → assert the render fails
  with a `ProcessCrashed`-shaped error carrying the captured stderr, and that no
  subprocess is leaked. Exercises the reader-thread EOF→broadcast path against a
  real process (the `MockTransport` crash test only covers the broadcast logic).

## Design Notes

### Extension build model

Following Quarto 1's two-step approach:
1. **Build time:** `deno bundle --config=<deno.json> <entry.ts>` bundles the TS engine extension into a single `.js` file. The engine's `deno.json` resolves `@quarto/api` and `@quarto/types` from the registry (`jsr:@quarto/api` / `jsr:@quarto/types`; the latter erased as type-only) plus Deno std lib imports. All dependencies are inlined. `deno bundle` is a stable Deno feature (reintroduced in Deno 2.4, permanently supported; uses esbuild under the hood).
2. **Runtime:** The Deno subprocess loads the bundled `.js` file via dynamic `import()`. No import map or TS transpilation needed — everything is already resolved and bundled.

Note: The **engine-host harness** is built with esbuild (matching existing q2 patterns), while **engine extensions** are built with `deno bundle` (matching Quarto 1's extension build model and handling Deno-specific imports like `jsr:` specifiers). These are different build steps for different artifacts.

This means the Deno subprocess invocation is simple:
```bash
deno run --allow-all <engine-host-deno.js>
```

No `--import-map` flag needed at runtime.

### EngineHostContext field sources

plan1a-protocol defines `EngineHostContext` (sent on every `LaunchEngine`) with
the fields below. Several aren't naturally available in q2 today; this
table fixes their q2-side sources so `TsEngine::launch` is implementable.

| Field | q2 source | Notes |
|---|---|---|
| `project_dir` | `ctx.project.dir` | trivial |
| `is_single_file` | `ctx.project.is_single_file` | trivial |
| `quarto_version` | `env!("CARGO_PKG_VERSION")` from the `quarto` crate, exposed via a `quarto_core::version()` const | one-liner |
| `resource_dir` | `crate::extension::BUILTIN_EXTENSIONS.path()` (existing `ResourceBundle.path()`) | narrower than Q1's "all bundled resources"; document the scope |
| `runtime_dir` | `{project_dir}/.quarto/cache/engines/`, created on demand | reuses the existing `.quarto/cache/` convention; no new persistent-state infra needed for Plan 1c |
| `pandoc_path` | `ctx.runtime.find_binary("pandoc", "QUARTO_PANDOC")` (already exists in render.rs:51 via `BinaryDependencies::discover`) | `Option<String>` — `None` is fine; engines that need pandoc fail with a clear error only if they actually call it. q2 itself does not invoke pandoc on the main render path (pampa replaces it). |
| `is_interactive_session` | new `SystemRuntime::is_interactive(&self) -> bool` (NativeRuntime checks `IsTerminal` on stdin; WasmRuntime returns `false`) | small new method; ~10 lines |
| `running_in_ci` | new `SystemRuntime::running_in_ci(&self) -> bool` (reads `CI` env var via existing `env_get`) | small new method; ~5 lines |

The two new `SystemRuntime` methods are the only genuinely new
infrastructure required for the launch context. Everything else is
existing machinery being pointed at.

### Conversion provenance: C′ (converted-buffer) now, A′ deferred

**v1 (C′, per plan1a-engine SEAM-3).** `EngineClaimsFileStage` records the
converting engine's name on `LoadedSource.conversion`; `ParseDocumentStage`
registers the converted QMD under the engine-reflecting synthetic name
`"<{original} (converted by {engine})>"` and parses it, so AST nodes get honest
`SourceInfo::Original(qmd_id, …)` **into the converted buffer**. Diagnostics
resolve to a real position in *the converted view of the file*, labelled by the
engine that produced it — better than Q1's "origin unknown," and with no
panic, no dormant code path, no transform. `markdown_for_file` returns
`(String, SourceInfo::default())`; the protocol `source_map`/`file_name` ride
the wire **unconsumed**.

**A′ (deferred — commendable, not these plans).** Faithful converted-cell →
original-cell positions are out of scope here (no consumer yet, and computing
accurate conversion provenance is engine-specific — percent-script, spin,
ipynb cell boundaries). When a consumer appears, the preferred path is the
generalized FileId-remap (register the original bytes, walk the AST rewriting
`Original(qmd_id, …)` → original ranges via the `source_map` the harness
already serializes) — *not* the dormant `parent_source_info`/`Concat` path.
The trait return shape `(String, SourceInfo)` is the forward-compatible seam:
when engines compute real provenance, they fill the `SourceInfo` and the remap
walker (built then) composes the rest. See plan1a-engine SEAM-3 and
engine-resolution.md §13. Who consumes the resolved positions is Plan 0
("Error remapping responsibility"), also deferred.

### No `partitioned_markdown` on the q2 trait

Q1's `ExecutionEngineInstance.partitionedMarkdown` is **not** ported to
the q2 trait or protocol. `DocumentProfile` (the post-merge,
pre-mutation pipeline checkpoint introduced by the website epic on
`main`) carries the title, heading, and draft data Q1 read via
`partitionedMarkdown.yaml/headingText/draft`, and Q1's pre-execute
filter-YAML harvest folds into the natural `MetadataMergeStage`
cascade once filters run inside `markdown_for_file`. See
`claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md`.

### Distribution of build-ts-extension assets (resolved: published SDK)

`@quarto/api` and `@quarto/types` are published to a registry (jsr/npm), so
`q2 build-ts-extension` bundles against them from the registry — no q2 source
clone, and no build assets embedded in or extracted from the q2 binary. See the
grand plan's "Distribution of the engine-author SDK". The one ongoing surface
is API stability: `deno bundle` freezes the `@quarto/api` version into each
author's `.js`, managed by semver on the published package.

## Future Work: Built-in engine percent/spin script support

The pre-parse `claimsFile` → `markdownForFile` flow (Phase 2 above) is
designed for TS engine extensions but also applies to built-in engines.
Currently, q2's built-in engines don't implement `claims_file` or
`markdown_for_file` — they only handle `.qmd` input.

Adding non-QMD file support to built-in engines requires implementing
the trait methods on each engine:

- **Jupyter**: `claims_file(".py") → true`, `claims_file(".jl") → true`,
  `markdown_for_file` with a Rust percent-script converter (port of
  Quarto 1's `markdownFromJupyterPercentScript`)
- **Knitr**: `claims_file(".r") → true` (for spin scripts),
  `markdown_for_file` invoking R's `knitr::spin()` via the R subprocess

No pipeline changes needed — the architecture from Phase 2 supports it.
This is out of scope for this plan (validation target is `.qmd` files)
but is a natural follow-on.

## Success Criteria

- [ ] Extension discovery finds engine extensions in `_extensions/`
- [ ] Both string (reordering) and object (new engine) forms parsed from `contributes.engines`
- [ ] `path` validation: lowercase `.js` accepted; `.ts`, `.JS`, `.mjs`, etc. rejected with actionable error
- [ ] `EngineContribution::External` carries `name: Option<String>`,
  `languages: Option<Vec<String>>`, `file_extensions: Option<Vec<String>>`;
  YAML field-absent vs explicit `[]` distinguished for the list fields
- [ ] Missing-hints warning fires once per extension at discovery time, naming the missing field(s) (any of `name` / `languages` / `file-extensions`) and showing the YAML snippet to add
- [ ] When `name` is declared, registry lookup by name succeeds with zero subprocess load; mismatch with `LoadEngineResult.name` at first load is a hard error
- [ ] When `name` is undeclared, the registry's `runtime_name → extension_id` alias map is populated lazily on first `LoadEngine`, and YAML `engine: foo` lookups succeed via that map
- [ ] No auto-build during render; missing `.js` bundle fails with actionable error pointing to `q2 build-ts-extension`
- [ ] `q2 build-ts-extension` subcommand exists and produces a working bundle against the published `@quarto/api` / `@quarto/types` (works from a clone or an installed binary — no embedded build assets)
- [ ] `_quarto.yml engines:` list ordering matches Q1: user-specified
  entries first (External engines auto-promoted), then built-ins in
  registration order (knitr → jupyter → markdown). Unknown name in the
  list → hard error listing available engines (matches Q1 engine.ts:275–283).
  Two contributors registering the same name → hard error (q2 strengthens
  Q1's silent-replace asymmetry).
- [ ] `EngineRegistry` (with alias map) lives on `ProjectContext` as
  `Arc<EngineRegistry>` and is populated with extension engines at
  `ProjectContext::new` time (or its caller); per-file `StageContext`
  receives a clone via `RenderContext` threading
- [ ] `Arc<TsEngineHost>` is constructed once at `ProjectContext`
  build time, lives inside the registry's `TsEngine` instances, and
  is shared across every per-file `StageContext` in both Pass 1 and
  Pass 2; subprocess not spawned until first protocol round-trip
- [ ] Single-doc renders use `DefaultProjectType` and pick up the
  same registry/host plumbing; no special case
- [ ] `KNOWN_ENGINES` constant and `is_known_engine()` function removed; detection uses registry dynamically
- [ ] `LoadedSource` extended with `conversion: Option<ConversionProvenance>` (v1/C′: carries the converting engine's name for the synthetic label; faithful original-file mapping deferred to A′)
- [ ] `EngineClaimsFileStage` runs before `ParseDocumentStage`; built-in engines decline file claims (deferred to future work); TS engines claim via `claimsFile`/`markdownForFile` end-to-end (covered by the echo `.echo` fixture)
- [ ] `ParseDocumentStage` registers the converted content under the engine-reflecting synthetic name (C′ — single registration; AST nodes get honest `Original(qmd_id)` into the converted buffer); the dual-registration + source_info remap walker are A′-deferred (not v1)
- [ ] `claimed_engine_name` propagates from the pre-parse stage and seeds
  `resolve_engines` as the converted content's `Primary` owner (resolution still runs)
- [ ] `resolve_engines` produces `EngineResolution { sequence, ownership }` per the
  tiered model (design doc §4); `EngineExecutionStage` reads it off `StageContext`,
  derives each engine's `handled_languages` from `ownership`, and drives the
  multi-engine loop; the trace records the resolved `sequence`
- [ ] Failure model matches Q1 (design doc §10): loud on can't-find-engine-for-extension,
  runtime-unavailable-owner, **and owner-can't-execute-an-owned-language (case 4 —
  e.g. `[knitr, jupyter]`+`{sql}`→jupyter with no SQL kernel → loud, naming
  engine+language; applies to TS engines too)**; graceful jupyter/markdown fallback on
  the language axis; no silent degradation, no silent no-op
- [ ] Replay drives from recorded captures (not re-resolution); single-engine and
  multi-engine record→replay are byte-clean
- [ ] `EngineRegistry` (already `Clone`-dropped + `Arc`-wrapped by plan1a-engine) is
  **hoisted to `ProjectContext` as `Arc<EngineRegistry>`** here — 1c relocates ownership,
  does not re-drop `Clone` or re-audit clone sites; `spliced_engines` preserved through
  the stateless-stage refactor
- [ ] (A′-deferred) Conversion-provenance × multi-engine FileId-remap compose — the
  `ParseDocumentStage` source_info remap is **not built in v1** (C′), so this composition
  test lands with A′, not Plan 1c (design doc §13; relates to bd-8h3sn)
- [ ] `EngineHostContext` populated from documented field-source table (incl. two new `SystemRuntime` methods: `is_interactive` and `running_in_ci`)
- [ ] Echo engine integration test exercises **both** discovery paths: language claim (`{echo}` blocks in `.qmd`) and file claim (`.echo` whole-file)
- [ ] Tests requiring Deno are skipped if Deno is absent (runtime `has_deno()`
  check with early return, matching the pandoc test pattern)
- [ ] All existing tests pass (no regressions)
