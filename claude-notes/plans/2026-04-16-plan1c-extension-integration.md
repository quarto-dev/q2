# Plan 1c: Extension Integration & End-to-End

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Depends on:** plan1a-protocol, plan1a-host, plan1a-engine (Rust core: protocol, subprocess, trait, `TsEngine`),
**RTQ / `plan1a-return-to-q1` (Item A — the `EngineHostContext` split into
`Init { global }` at spawn + per-render `LaunchEngine { project }`; DQ-7)**, and
Plan 1b (Deno harness: `@quarto/engine-host-deno`, which **now also depends on
RTQ**). The Phase-2 construction sequence is written against the RTQ-amended
protocol, so **1c must run after RTQ Item A** — it can no longer start from the
plan1a sub-plans alone (the earlier "Phases 1–2 from plan1a alone" note is
void). Phase 3 (echo engine E2E test) additionally requires Plan 1b fully built.
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

**RTQ-wave reconciliation (2026-06-26).** This plan was updated during the
"return to Q1" review wave (`2026-06-25-plan1a-return-to-q1.md`) to make its
intent unambiguous and consistent with what landed in plan1a. Changes:
- **D1 — authoritative static claims.** `EngineContribution::External` now
  carries the §3.3 `claims:` (kind/priority) map + `claims-files:`, replacing
  the old "languages hint." Declared claims are **authoritative for
  resolution** (zero-load); an engine declaring none is a **legacy Q1 engine**
  resolved via dynamic load (back-compat). This realizes the "complete static
  form" plan1a-engine's `TsEngine` comment already anticipated, and resolves
  the plan↔`engine-resolution.md` §3.3 contradiction.
- **D2 — `quartoRequired`:** 1c adds only the **carrier** (the missing
  engine-level wire field, via RTQ ENG-1). The **gate** on both surfaces
  (engine-level + the dormant extension-level field) and the `1.11` compat
  spoof are deferred to extension-epic **Phase 12**
  (`2026-03-16-extensions-grand-plan.md`).
- **D3 — SDK distribution:** in-repo builds bundle against the **workspace**
  (unblocking the Phase-3 echo E2E pre-publish); `@quarto/api` is **published**
  for external authors.
- **R5 — preview registry:** the preview capture path reads `project.registry`
  (today it is built-ins-only), finishing the registry-ownership move.
- **DQ-7 alignment:** `EngineHostContext` splits into a process-stable `Init`
  `global` (host-held) and a per-render `LaunchEngine.project`
  (`EngineProjectContext`); the launch cadence and pooling-ready ownership are
  stated explicitly (see the construction sequence). Forward-notes point at
  `plan5-engine-host-pooling.md`.
- **Drift fixes** (1C-1/1C-2/1C-7/1C-8): "following Q1" overclaims corrected
  to name q2's deliberate departures.

**Static-resolution + file-claim review (2026-06-28).** A follow-up review
(all-engines lens) settled three more points:
- **`whenClass` static claims** — `claims_language` is a pure function of
  `(language, first_class)`, so `first_class`-conditional claims (marimo:
  `{python .marimo}`) are **fully static** via a `whenClass:` qualifier; there
  is no must-load case for language claims. The **one** dynamic power static
  resolution genuinely can't reach is a **content-inspecting `claims_file`**
  (Julia's `# %%`) — that engine omits `claims-files` and loads. (Corrects the
  earlier "first_class → must-load" framing and design-doc §3.3.)
- **File-claim → single engine (Q1-faithful), the §8 revert.** A claimed file
  resolves to *exactly* the claiming engine; `resolve_engines` short-circuits
  the tiers. Non-kernel cells **pass through unexecuted** (verified Q1
  behavior), not a §10 loud failure; the file's own `engine:` YAML is ignored
  (verified Q1 `claimsFile` preempt). This reverts the "seed `Primary` +
  resolve" design (which needed native-language inference and left a theft
  hole) and is a net deletion in `resolution.rs`.
- **semver dep confirmed; publishing `@quarto/api` is *not* a plan dependency**
  (in-repo builds use the workspace; publish is future external-author-only).

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
          /// Static declaration: the engine's runtime name (e.g., "julia").
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
          /// **Authoritative static language claims**, keyed by language — the
          /// engine-resolution.md §3.3 `claims:` map. *Authoritative for
          /// resolution*, not a hint:
          /// `None`: undeclared (un-upgraded Q1 engine, **or** an engine whose
          ///   `claims_language` reads runtime/global state) — **fall back to
          ///   dynamic**: `LoadEngine` + call `claims_language` per candidate
          ///   language (warning suggests upgrading).
          /// `Some(map)`: q2 resolves from the map **without loading**. Because
          ///   `claims_language(language, first_class)` is a pure function of
          ///   its two args, `first_class`-conditional claims are fully static
          ///   via `when_class` — there is no "must-load for first_class" case.
          ///   Validated against the dynamic method if/when the engine loads to
          ///   execute — mismatch is a hard error (like `name`).
          /// `Fallback` (universal kernel) is declared via a `fallback:` entry,
          ///   not a per-language key (it cannot be a finite list); §3.3.
          claims: Option<HashMap<String, StaticLanguageClaim>>,
          /// `valid_extensions` — the file extensions this engine handles
          /// (e.g., [".jl"]). Always a *complete* static answer (it IS the
          /// list); the pre-filter before any `claims_file`. `None` undeclared
          /// (warn); `Some(vec![])` handles none (silent); `Some(...)` the set.
          file_extensions: Option<Vec<String>>,
          /// **Unconditional static `claims_file`** — extensions claimed
          /// *without* content inspection (§3.3 `claims-files:`). Authoritative
          /// when file-claim logic is extension-only; `None` (or an engine that
          /// inspects content, e.g. Julia's `# %%`) **falls back to a dynamic
          /// `claims_file` load**.
          claims_files: Option<Vec<String>>,
      },
      /// A bare engine name string — reordering hint that moves a
      /// previously registered engine to higher priority.
      Reorder { name: String },
  }

  /// One authoritative static language claim (§3.3). A pure tabulation of
  /// `claims_language(language, first_class)`.
  #[derive(Debug, Clone)]
  pub struct StaticLanguageClaim {
      pub kind: ClaimKind,             // primary | interop | fallback
      pub priority: Option<i32>,       // defaults per kind (Primary 1, Interop/Fallback 0)
      /// `None`: applies for any/no first_class. `Some(c)`: applies **only**
      /// when the cell's first class == `c` (the marimo case — `{python .marimo}`).
      pub when_class: Option<String>,
  }

  /// The kind half of `LanguageClaim`, without the priority payload, so a static
  /// claim can carry kind + priority separately. NEW type; the existing
  /// `LanguageClaim` (plan1a-engine) stays `Primary(i32)/Interop(i32)/
  /// Fallback(i32)/None`.
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum ClaimKind { Primary, Interop, Fallback }
  ```
  **Lookup / conversion to `LanguageClaim`.** `claims_language(lang, fc)` consults
  `claims[lang]`; if that entry's `when_class` is `None` **or** `Some(c) == fc`,
  it converts the static claim to a `LanguageClaim`:
  `ClaimKind::Primary` + `priority p` → `LanguageClaim::Primary(p.unwrap_or(1))`;
  `Interop` → `Interop(p.unwrap_or(0))`; `Fallback` → `Fallback(p.unwrap_or(0))`.
  Otherwise — no entry, or a `when_class` mismatch — it returns
  `LanguageClaim::None`. One rule per language key in v1; a multi-class engine
  (different kinds for different classes of one language) would make the value a
  list — deferred. The `fallback:` YAML key maps to a `StaticLanguageClaim
  { kind: Fallback, .. }` applied to every language asked. **YAML↔Rust:** the
  YAML key is `whenClass`, the field `when_class` — use `#[serde(rename)]` (or
  `rename_all = "camelCase"`).
  This extends Quarto 1's schema: `contributes.engines` accepts both
  objects with a `path` property (creating new engines) and bare strings
  (reordering hints). The `name`, `claims`, `file-extensions`, and
  `claims-files` fields are q2 additions — **authoritative static
  declarations** (engine-resolution.md §3.3), not mere hints:
  - `name` lets q2 register the engine and answer YAML `engine: foo` /
    top-level-key detection without spawning the Deno subprocess at all.
  - `claims` / `claims-files` are the **authoritative** static forms of
    `claims_language` / `claims_file`, resolved with **no subprocess load**.
    `claims_language` is a pure function of `(language, first_class)`, so even
    `first_class`-conditional claims are fully static via `whenClass` (§3.3) —
    there is no must-load case for language claims. The **one** dynamic power
    static resolution can't reach is a **content-inspecting `claims_file`**
    (Julia's `# %%`): such an engine omits `claims-files` and loads to decide,
    using `file-extensions` as its pre-filter. Authors who declare static
    claims own their accuracy — each is validated against the dynamic method on
    the first execute-time load (mismatch → hard error, like `name`).
  - `file-extensions` (`valid_extensions`) is the complete handled-extension
    set — always static, the pre-filter before any `claims_file`.

  **Dynamic fallback (un-upgraded Q1 engines).** An `_extension.yml` that
  declares **none** of `claims` / `claims-files` is treated as a legacy Q1
  engine: q2 `LoadEngine`s it and calls `claims_language` / `claims_file`
  dynamically, exactly as Q1 does (`engine.ts:177-198` / `:320-325`). The
  static path is an **upgrade, never a requirement** — a Q1 engine carrying
  only `path` (+ optional `name`) keeps working unchanged. This is the
  back-compat guarantee for the existing Q1 engine ecosystem.

  **Full-static, zero-load resolution** (the engine is spawned only to
  *execute*, once it has won ownership) is reached when an engine declares
  `name` **and** `claims`/`claims-files` covering its claiming logic. That is
  the §3.3 payoff and the precondition for the future Pass-1 resolution lift
  (engine-resolution.md §7) — when *every* engine in a project is fully
  static, resolution loads nothing.

  **Q1 schema compatibility:** Q1's `external-engine` schema definition
  is `closed: true` (single property `path`). A `_extension.yml` that
  declares any of `name` / `claims` / `file-extensions` / `claims-files`
  will fail Q1's validator. Document this explicitly: q2 engine extensions
  that use the static-claims fields are q2-targeted; the file is not portable
  backward to Q1. (A `path`-only engine remains valid in both.)

- [ ] Add `engines` parsing in `parse_contributes()` in `crates/quarto-core/src/extension/read.rs`:
  - Handle array of strings (reordering hints → `EngineContribution::Reorder`)
    and objects with `path` key (resolve to absolute paths relative to ext_dir
    → `EngineContribution::External`).
  - For object entries, parse the optional `name: String` field into
    `Option<String>`. Parse the `claims:` map into
    `Option<HashMap<String, StaticLanguageClaim>>` — each value is either the
    `{ kind: primary|interop|fallback, priority?: int, whenClass?: str }`
    object or the `boolean | number` shorthand, **normalized exactly as the
    wire harness does** (`true`→`Primary(1)`, `n`→`Primary(n)`, never `Interop`;
    engine-resolution.md §3.2), with a top-level `fallback: { priority? }`
    key mapping to the universal `Fallback`. `whenClass` (when present) is the
    `first_class` the claim is conditional on — absent means any/no first_class.
    Parse `claims-files:` and
    `file-extensions:` into `Option<Vec<String>>`. For every list/map field
    distinguish field-absent from field-present-but-empty: `None` if the YAML
    key was absent; `Some(empty)` if present as `[]`/`{}`; `Some(...)` if
    present and non-empty.
  - **Validate `path` ends in `.js` (lowercase).** Reject `.JS`, `.mjs`,
    `.ts`, etc. with the same actionable error: `Engine extension
    '{name}' has 'path: {path}'; only pre-built lowercase '.js' bundles
    are loadable. Run 'q2 build-ts-extension' to produce
    {expected_js_path} and update _extension.yml.` The runtime subprocess
    uses `deno run --allow-all <engine-host.js>` with no import map; a
    raw `.ts` (or `.mjs`/etc.) path would fail to resolve `@quarto/api`,
    `@quarto/types`, and other engine-extension imports.
  - **Emit a warning per `External` entry that lacks static claims.** If
    any of `name`, `claims`, `file-extensions`, or `claims-files` is `None`,
    emit a single `DiagnosticMessage::warning` per extension naming exactly
    which fields are missing, the extension's path, and a snippet showing what
    to add to `_extension.yml`. The message frames the cost concretely: an
    engine without `name`/`claims`/`claims-files` is treated as a legacy Q1
    engine and **`LoadEngine`d on every render to resolve** (the slow dynamic
    path), whereas declaring them enables zero-load resolution. Suggest
    `[]`/`{}` to silence a field when the engine genuinely declares none (a
    valid, authoritative "claims nothing"). The warning fires at
    extension-discovery time (once per render). Reorder entries don't trigger
    it; a deliberately-legacy `path`-only engine still works (it just pays the
    dynamic-load cost), so the warning is advisory, not an error.
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
        name: julia               # optional: engine's runtime name (zero-load registration + YAML lookup)
        claims:                   # optional: authoritative static language claims (§3.3)
          julia: { kind: primary, priority: 1 }
          # reticulate-style:        r: { kind: primary }, python: { kind: interop }
          # first_class-conditional: python: { whenClass: marimo, kind: primary }
          # universal kernel:        fallback: { priority: 0 }
        file-extensions: [".jl"]  # optional: valid_extensions (the handled-extension set)
        # julia omits `claims-files`: its claimsFile inspects content (`# %%`),
        # so it loads to decide — `file-extensions` is the pre-filter. An
        # extension-only engine (e.g. echo) WOULD declare `claims-files: [".x"]`.
      - jupyter                   # string form: reordering hint
  ```
  A `path`-only entry (optionally `+ name`) is a **legacy Q1 engine**: q2 loads
  it and queries `claims_language`/`claims_file` dynamically. Declaring
  `claims`/`claims-files` upgrades it to zero-load resolution.
  **Quarto 1 reference:** The extension schema (in
  `external-sources/quarto-cli/src/resources/schema/extension.yml`) defines
  engines as an array of either strings (engine names for reordering) or
  objects. Q1's `external-engine` definition
  (`external-sources/quarto-cli/src/resources/schema/definitions.yml`)
  is `closed: true` with a single `path` property. Both forms are
  allowed in both `_extension.yml` and `_quarto.yml` (identical schema).

  **q2 extensions to the schema:** `name`, `claims`, `file-extensions`, and
  `claims-files` are new fields q2 adds alongside `path` — **authoritative
  static declarations** (engine-resolution.md §3.3), not hints, that let q2
  resolve without spawning the Deno subprocess:
  - `name` lets q2 register the engine and resolve YAML lookups
    (`engine: julia`, top-level `julia:` keys) without `LoadEngine`.
    When provided, q2 verifies it matches `LoadEngineResult.name` at
    first load — mismatch is a hard error.
  - `claims` / `claims-files` are the authoritative static forms of the
    engine's `claims_language` / `claims_file` logic, resolved with no load.
    `first_class`-conditional language claims are fully static via `whenClass`
    (§3.3); the only must-load case is a **content-inspecting `claims_file`**
    (Julia's `# %%`), which omits `claims-files` and loads, using
    `file-extensions` as its pre-filter. q2 validates a declared claim against
    the dynamic method on the first execute-time load (mismatch → hard error).
    `{}`/`[]` is a valid, authoritative "claims none."
  - `file-extensions` (`valid_extensions`) is the engine's complete
    handled-extension set.

  **Authors who omit any static field get a warning** at extension-discovery
  time pointing them to the YAML changes that would silence it (and noting the
  per-render dynamic-load cost they pay until they do).

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

- [ ] **Teach `TsEngine` to carry and consult static claims** (modifies the
  **landed** `TsEngine` from plan1a-engine — this is the runtime half of the
  static-claims mechanism; without it the parsed `claims` do nothing). Concretely:
  - **Fields:** replace the landed `language_hints` / `file_extension_hints`
    with `claims: Option<HashMap<String, StaticLanguageClaim>>`,
    `file_extensions: Option<Vec<String>>`, `claims_files: Option<Vec<String>>`
    (copied from the `EngineContribution::External` at registration, step 5).
  - **`claims_language(lang, fc)`:** if `claims` is `Some`, answer **from the
    map without loading** (the lookup/conversion above — `when_class`-aware →
    `LanguageClaim`); cache the result. If `claims` is `None` (legacy engine),
    `LoadEngine` + call the dynamic `claimsLanguage` over the wire, cache it.
    When a static claim was used and the engine later loads to execute, validate
    it against the dynamic method (mismatch → hard error, like `name`).
  - **`claims_file(file, ext)`:** if `ext ∉ file_extensions` (when `Some`),
    short-circuit to `false` (pre-filter, no load). Else if `claims_files` is
    `Some`, answer `ext ∈ claims_files` without loading. Else `LoadEngine` + call
    the dynamic `claimsFile` (content-inspecting engines, e.g. Julia `# %%`);
    cache per canonical path (`claims_file_cache`).
  - **`valid_extensions()`:** return `file_extensions` directly when `Some`.
  consistent with "Distribution" below). Note `resources/extension-build/` does
  **not** exist yet — this item creates it.
  - **Shipped author template** (`resources/extension-build/deno.json`, the file
    `q2 build-ts-extension` falls back to) whose imports reference the
    **published** SDK + std lib:
    - `@quarto/api` → `jsr:@quarto/api` (real code, inlined by `deno bundle`;
      each compiled `julia-engine.js` freezes the `@quarto/api` version it
      built against — managed by semver on the published package)
    - `@quarto/types` → `jsr:@quarto/types` (type-only, erased)
    - `@std/*` → `jsr:@std/*`
    Engine authors copy/extend this in their extension.
  - **In-repo dev build** uses a **workspace mapping** resolving `@quarto/api` /
    `@quarto/types` to `ts-packages/…`, so q2's own `build-ts-extension` (and the
    Phase-3 echo E2E) bundle against workspace source **without the registry** —
    this is what unblocks Phase 3 before the packages are published. See the
    grand plan's "Distribution of the engine-author SDK".

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
     parsed `name` / `claims` / `file_extensions` / `claims_files`
     (all `Option<...>` — the authoritative static-claim declarations,
     `None` meaning "fall back to dynamic load" for that facet).
  4. **Determine the registry key.** If `name` is `Some(name)`,
     register the `TsEngine` under that name immediately — no subprocess
     spawn. If `name` is `None`, register under the extension id
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
     installed extension's engine ends up **at position 0 only when
     `_quarto.yml` lists no `engines:`** — otherwise it lands *after* the
     user-listed entries but still *ahead of the unlisted built-ins*
     (matches Q1: extensions append after existing entries at
     `project-context.ts:791`, then sit ahead of built-ins via
     `userSpecifiedOrder` at `engine.ts:288–297`). This is intentional —
     it's what makes a positive `claims` competitive without explicit user
     intervention. The static-claims mechanism (`claims`, `claims-files`,
     `file-extensions`) keeps installed-but-unused engines silent during
     resolution — they neither load nor win a language they don't claim.
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
  miss across both, lazy-load every unloaded TS engine that lacks a
  declared `name` to populate the alias map, then retry. The missing-`name`
  warning steers authors away from this slow path.
- [ ] **Carry the engine version requirement (`quarto_required`) on the wire —
  gate deferred to Phase 12.** Q1 gates both built-in and extension engine
  registration on `quartoRequired` via `checkEngineVersionRequirement`
  (`engine.ts:62`, called at `:97`/`:255`): `satisfies(quartoVersion,
  engine.quartoRequired)`, throwing an actionable "requires Quarto X — upgrade"
  error. q2 has **no such gate today** — the engine-level requirement had no wire
  carrier, and the extension-level `_extension.yml` `quarto-required` (parsed at
  `extension/read.rs:105`) is **enforced nowhere**. Per the consolidated decision
  (**field → RTQ ENG-1; gate → grand-plan Phase 12**), 1c carries the field but
  does **not** build the gate:
  1. **Carrier (RTQ ENG-1).** `quarto_required: Option<String>` (`#[serde(default)]`)
     on `LoadEngineResult`, plus a default `fn quarto_required(&self) -> Option<&str>
     { None }` on the `ExecutionEngine` trait. ENG-1 lands these; 1c just consumes
     the carried value. Additive — today's engines send `None`.
  2. **Gate + version-spoof → Phase 12 (NOT 1c).** Both check sites (extension-level
     at registry-build; engine-level at `LoadEngine`), the shared
     `semver`/`VersionReq`/`cli_version()` machinery, and the **Q1-compat version
     spoof** are grand-plan Phase 12's work — the extension epic's
     `2026-03-16-extensions-grand-plan.md` Phase 12, an epic above this one.
     (The spoof: q2's real version is `0.x` while Q1 engines declare
     `quartoRequired: ">=1.9"`/`">=1.10"`; Phase 12 isolates a spoofed compat
     version behind a single `engine_compat_version() -> "1.11.0"` so Q1 engines'
     requirements pass — a clearly-commented stopgap, one place to revisit.
     That `1.11.0`/`engine_compat_version()` choice is recorded in Phase 12's
     notes so the gate work inherits it.)
     **Until Phase 12, q2 carries `quarto_required` inert and rejects no engine**, so
     1c's E2E loads real Q1 engines without a gate. (No 1c test asserts
     enforcement — those tests live with Phase 12's gate.)
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
- [ ] Write tests for the missing-static-fields warning across the field
  matrix: each of `name`, `claims`, `file-extensions`, `claims-files`
  independently missing or present; mixed combinations. Verify `Some(empty)`
  (`{}`/`[]`) is silent (a valid, authoritative declaration of "claims none").
- [ ] Write test: an engine declaring an authoritative `claims:` map resolves
  its language **with no `LoadEngine`** (zero-load resolution); a `path`-only
  legacy engine resolves the same language via a dynamic `LoadEngine` +
  `claims_language` call (dynamic fallback). Both reach the same ownership.
- [ ] Write test: a declared `claims` entry that **disagrees** with the
  engine's dynamic `claims_language` at first execute-time load → hard error
  naming the engine + language (the §3.3 authoritative-claim validation).

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
engine is recorded in `claimed_engine_name`, and `resolve_engines`
**short-circuits the tiers**: the claimed file resolves to that **single
engine**, Q1-faithful (design doc §8). It executes the cells it recognizes and
**passes the rest through unexecuted** (not a loud failure).

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

  1. **Build the process-stable `global` config** (DQ-7 / RTQ Item A — the
     old single `EngineHostContext` is now **split across two frames**; the
     host holds only the *global*). Fields: `resource_dir`, `runtime_dir`
     (via `quarto_util::quarto_runtime_dir()` — plan1a-host), `data_dir`
     (via `quarto_util::quarto_data_dir()` — **prerequisite: this leaf does not
     exist yet; add it to `quarto-util` mirroring `runtime_dir.rs`, honoring a
     `QUARTO_DATA_DIR` override for Q1 faithfulness**), `pandoc_path: Option<String>`
     (from `BinaryDependencies.pandoc`,
     stringified if `Some`), `is_interactive_session`, `running_in_ci`,
     `quarto_version` (`env!("CARGO_PKG_VERSION")`). This is process-stable
     — identical for every render and engine — and is delivered once on the
     `Init` frame at spawn. See RTQ Item A for the authoritative `global`
     field set. **`project_dir` / `is_single_file` are NOT in the global** —
     they move to the per-render `EngineProjectContext` (step 2a).
  2. Construct `Arc::new(TsEngineHost::new(global))` once, stashing the
     global. The subprocess is **not** spawned here — `TsEngineHost::new`
     is cheap; first `ensure_started()` (triggered by the first protocol
     round-trip) sends `Init { global }` and launches Deno.
  2a. **Build the per-render `EngineProjectContext` at render time, not
     here** (DQ-7). It carries `project_dir`, `is_single_file`, and `config`
     (the `engines` list + `output-dir`), sourced from `ProjectContext` /
     `project.config`. It is **set on the selected `TsEngine` instance at
     engine-selection (render-setup), *before* its first instance method runs**
     (RTQ provides the instance setter); `ensure_launched` then reads it and
     passes it to `launch_engine(engine, project, …)` at first launch. (It cannot
     be threaded from a method `ctx`: launch is triggered lazily from
     `markdown_for_file` / `intermediate_files` too — neither carries an
     `ExecutionContext` — not only from `execute`.) Captured in the instance closure (pure Q1
     `engine.launch(EngineProjectContext)`; the harness reconstitutes the
     full Q1 object — `fileInformationCache` as a harness-local `Map`,
     `resolveFullMarkdownForFile` returning the pushed resolved markdown —
     per DQ-1/DQ-5). **`LaunchEngine` cadence: once per engine per *project
     render*.** The call site (`EngineExecutionStage`) runs per file, but the
     host's launched-instance cache makes `LaunchEngine` idempotent within a
     render: the first file to need the engine launches it (capturing that
     render's project context), the rest reuse it. The invariant is keyed to
     the **project render, not the subprocess** — under Plan 5 pooling
     (`plan5-engine-host-pooling.md`) one subprocess outlives the render, the
     cache resets at the render boundary, and the next render re-launches
     with its own project context. That is *why* DQ-7 moves `project` onto
     `LaunchEngine`.
     **Verified consistent with Q1 (agent check 2026-06-27):** Q1's `launch()`
     is a cheap, synchronous, stateless object-literal construction called
     *liberally* (multiple times, even per file — `engine.ts:323`, `rmd.ts:246`,
     `render.ts:94`); the instance closes over only the **project-scoped**
     `EngineProjectContext` (`project/types.ts:164-216`, file identity always a
     method arg), and Q1's `fileInformationCache` is **designed to persist
     across a project's files** (`engine.ts:358-379`). So no per-file value ever
     feeds `launch()` — once-per-engine-per-project-render with instance caching
     is the *faithful* Q1 form (per-file launch would wrongly reset that cache),
     not merely a permitted one.
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
     `name` / `claims` / `file_extensions` / `claims_files`. Register it
     in the registry under either `name` (if declared) or the
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

  **Pooling-ready ownership (forward-note — do NOT build here, R3).** Write this
  move so the host/registry is a **borrowable shared root**. A future
  `plan5-engine-host-pooling.md` lifts the host *above* a single
  `ProjectContext` — into a process- or preview-server-scoped pool that each
  `ProjectContext` borrows — for preview re-compute warmth across renders
  (enabled by DQ-7's per-render `LaunchEngine.project`). Keeping the
  `Arc<TsEngineHost>` reachable *through the registry* (not buried in per-file
  `StageContext` state) makes that lift a relocation, not a rewrite. 1c lands the
  per-`ProjectContext` ownership; Plan 5 moves it up. Do not do the pooling work
  here.

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

- [ ] **Finish the registry move into the preview capture path (R5).** The
  registry-ownership move above wires TS engines into the **render** path, but
  `q2 preview` will still run **built-ins only** unless the preview capture path
  also reads the project registry. **Verified 2026-06-26 — the override is never
  populated in production, not merely `None` at the call site:** every
  `engine_registry: Some(...)` in the whole tree is **test-only** (the `tests/`
  integration dirs *plus* the inline `#[cfg(test)]` probe/replay sites at
  `pipeline.rs:2271`/`:2314`; the `mod tests` boundary is `pipeline.rs:1438`).
  The **sole production assignment anywhere** is `engine_registry: None` at
  `crates/quarto/src/commands/preview.rs:216`, and `ProjectContext` has **no
  `registry` field today** (this plan adds it) — so nothing upstream *can* feed
  a `Some`. The capture driver forwards that `None`
  (`crates/quarto-preview/src/capture_driver.rs` — its doc-comment confirms the
  `Some` form is a test substitution seam) through
  `preview_record.rs::build_capture_pipeline_stages` into
  `build_html_pipeline_stages_with_options(None, …)`, and when it is `None`
  `EngineExecutionStage::new()` builds a **built-ins-only** `EngineRegistry`.
  So even after this plan adds `ProjectContext.registry`, preview never reads
  it. **Fix — repoint *all three* native preview call sites to `project.registry`
  (the post-1c registry that includes discovered TS extension engines), not just
  the eager driver.** All three thread an `engine_registry:
  Option<Arc<EngineRegistry>>` override into the **common funnel**
  `cache.rs::record_capture_cached` (`cache.rs:150`) → `record_capture`
  (`cache.rs:181`); each currently passes the production default **`None`
  (built-ins only)**. The funnel is one chokepoint, but the override is *chosen*
  at three distinct source sites — fix it at each:
  1. **`record_eager_captures`** (startup capture) — `capture_driver.rs:57`,
     from `lib.rs:214`.
  2. **`recompute_staleness`** (on-edit; when `EnginePolicy::Auto && is_stale`)
     → `trigger_auto_re_execute` (`capture_driver.rs:311`), from `lib.rs:260`.
  3. **`re_execute.rs`** (live re-execution / the `/api/preview/re-execute`
     handler) — `record_capture_cached` call at `re_execute.rs:309`; production
     passes `None`.
  Each should source `project.registry` from the discovered `ProjectContext`
  when no explicit (test) override is given; keep the `Some(...)` override as the
  test substitution seam. **If only the eager site is repointed, a TS-engine doc
  captures once at startup but every live re-execution silently falls back to
  built-ins-only — the engine *vanishes the moment the user edits a code
  cell*.** R5 owns only pointing the registry correctly so re-compute is
  *correct*; **on-edit re-execution latency** (keeping the Deno host warm so a
  cell edit doesn't respawn Deno + re-`import()`) is **Plan 5's** concern, not
  R5's — Plan 5 already notes these same three sites (commit `6abe3b1c4`). Do not
  solve warmth here.
  **Why this is load-bearing:** a TS/Julia engine is a native kernel/daemon and
  **can never run in the browser/WASM**, so `q2 preview` must execute it
  server-side via exactly this capture path. It is the *only* mechanism that
  makes TS-engine live preview work — hence getting all three sites right.
  - **Phase-0 seam (named revert):** an extension-registered TS engine runs in a
    `q2 preview` capture (asserted via the engine's effect in the captured
    output). Revert — drop the `project.registry` read at any of the three sites
    so it falls back to `None` → built-ins only → the extension engine is absent
    → RED.
  - **Preview-path regression test (distinct from the CLI-render E2E):** drive
    the echo engine through the preview **capture → splice** pipeline (not `q2
    render`) and assert `ECHO_EXECUTED` appears — **and that it survives an
    on-edit re-execution** (so sites 2/3 are exercised, not only the eager
    capture). Without this, *nothing* exercises TS engines in the preview
    pipeline.
  - This is the preview half of the same ownership move; it is **not** the
    cross-render pooling work (that is Plan 5). Per-render preview state and
    teardown stay as-is here.

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

  **Teardown scope under DQ-7 vs. Plan 5 (forward-note, R4 — keep distinct).**
  Under DQ-7 alone, teardown is **unchanged**: per-`ProjectContext`, the
  orchestrator calls `registry.shutdown_all()` → `host.shutdown()` (which kills
  the child, `ts_process.rs:716`) at end-of-render. Under Plan 5 pooling the
  subprocess outlives a single render, so per-render `shutdown()` becomes
  **drain-only** (no kill) or moves to `Drop`, and the actual reap happens at
  q2-process exit / pool eviction. **1c keeps the per-`ProjectContext` teardown
  above**; do not pre-emptively make it drain-only — that is Plan 5's change.

- [ ] **`StageContext` plumbing for `BinaryDependencies`.** The
  the `global` config built in step 1 of registry construction reads
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
  Passed into `resolve_engines` as `claimed`; when `Some(name)`,
  `resolve_engines` **short-circuits the tiers and returns that single engine**
  as the whole sequence (design doc §8, Q1-faithful — `fileExecutionEngine`
  `return`s the claiming engine and consults nothing else). The claimed file's
  own front-matter `engine:` is **ignored** (Q1's `claimsFile` preempts it). The
  engine executes the cells it recognizes and passes the rest through
  unexecuted. *(This reverts the earlier "seed `Primary` + resolve" design,
  which required native-language inference the resolver can't do and left a
  theft hole — see design doc §8 "Why this replaced…".)*

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
  **Cache the conversion across passes.** Because the stage runs in *both*
  builders, a non-QMD file would otherwise be converted twice (a second
  `markdown_for_file` subprocess round-trip per TS-engine file). Cache the
  `markdown_for_file` output **per render, keyed on canonical path** (same
  lifetime/keying as `claims_file_cache` — file content is fixed within a
  render, so the two passes get byte-identical QMD). Pass 1 populates it; Pass 2
  hits the cache. (One conversion per file per render, not two.)
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
     **The first engine that claims wins — stop iterating** (Q1's
     `fileExecutionEngine` `return`s on the first match, `engine.ts:320-325`; a
     file has exactly one claiming engine). For TS engines, the static file-claim
     pre-filter in `TsEngine::claims_file` (from `file_extensions` /
     `claims_files`) short-circuits engines whose declared extensions don't
     match — no subprocess load for those; an engine with an authoritative
     `claims_files` answers entirely without loading.
  3. Once an engine claims the file, calls
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
  For TS engines that survive the static file-claim filter (or declare no
  static file facet, so must load), this lazily spawns the
  Deno subprocess + sends `LoadEngine` (then `LaunchEngine` if the engine
  claims) on first need.
  **`.qmd` cost note:** for every TS engine in the registry whose
  `file_extensions` / `claims_files` are both `None` (no static file facet),
  this stage triggers a
  `LoadEngine` + `ClaimsFile` round-trip on every render even when
  the file is `.qmd`. Authors avoid this by declaring
  `file-extensions: []` (or a non-empty list). The missing-static-fields
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

- [ ] **Wire `resolve_engines` into the stage** (§2B — *the function already
  landed in plan1a-engine*: `crates/quarto-core/src/engine/resolution.rs`
  defines `EngineResolution`, `handled_languages_for`, **and** `resolve_engines`
  with the full tier logic + AST language scan; verified present on the branch).
  So this item is **wiring, not implementation**: call the existing
  `resolve_engines(meta, ast, registry, claimed: Option<&str>) →
  EngineResolution { sequence, ownership }` (design doc §4, §9) from
  `EngineExecutionStage`, retire the metadata-only `detect_engine_sequence` call
  sites (keep a thin `detect_engine_sequence`-compatible shim only if a
  non-execution caller still needs just the sequence). **What 1c still owes the
  resolver:** the static-claims path (D1) — a `TsEngine` with an authoritative
  `claims`/`claims_files` answers `claims_language`/`claims_file` *without*
  loading, so resolution stays zero-load; the resolver itself is unchanged
  (it just calls the trait methods). The algorithm — the four tiers (Primary →
  explicit-Fallback → Interop → implicit-Fallback), presence-gating,
  per-language ownership, structural computational-language extraction from the
  AST, and the `claimed` short-circuit — is specified in the design
  contract and **must not be restated here**. The wiring obligations are:
  - **File-claim short-circuit**: when `claimed` is `Some(name)`,
    `resolve_engines` returns that single engine as the whole sequence and does
    **not** run the tiers (design doc §8, Q1-faithful). *(This replaces the
    landed `resolution.rs` seed handling — `:354-411`, which marks the seed
    "present" + disables T4 but never short-circuits, leaving a theft hole; the
    revert is a net deletion of the `explicit_with_seed`/seed-present logic.)*
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
- [ ] Write test: claimed file → **single engine** — a claimed `.echo`/`.jl`
  file resolves to exactly the claiming engine (`sequence == [claimer]`); a
  second-language cell in the converted content is **passed through unexecuted**
  (not stolen by another engine, **not** a loud failure); the file's own
  `engine:` YAML, if present, is ignored (Q1's `claimsFile` preempts it)
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
  cells, then halts at `{sql}`. **Applies to TS engines too — but only in a
  multi-engine sequence:** a TS engine that is a *non-sole* participant, handed
  (via `TsExecuteOptions.handled_languages`) a cell in a language it owns but
  can't run, must return a protocol `error`, surfaced loudly — never a silent
  pass-through. `NoHandlerForLanguage` is a clean refusal, so it does **not**
  poison the instance.
  **Scope — case 4 is gated on `|sequence| > 1` (multi-engine only).** It fires
  only when the tiers / `engine:` list routed a language to one of *several*
  engines (silent-skip would betray the user's explicit composition). A
  **single-engine sequence** — a claimed file (§8) *or* a `.qmd` resolving to
  one engine — is handed the whole document and **self-selects**: it runs what
  it can and **passes the rest through unexecuted** (Q1's `quartoMdToJupyter`
  makes a non-kernel `{bash}` cell display-only — verified 2026-06-28), never a
  loud failure. Full Q1 parity (Q1 is always single-engine); case 4 is the
  deliberate q2 *multi-engine* divergence.
  **Landed-code change (1c owns it, alongside the `resolution.rs` revert):**
  `engine/jupyter/text_execute.rs`'s `partition_cells` currently raises
  `NoHandlerForLanguage` for *any* owned-but-unrunnable cell regardless of
  sequence length — gate the loud branch on `|sequence| > 1`, ceding
  (passing through) in the single-engine case; reconcile plan1a-engine's
  case-4 / jupyter-enforcement prose to match.
  Write tests: the `[knitr, jupyter]` + `{sql}` route (multi-engine → loud);
  **and** a single-engine `{python}`-in-a-claimed-`.echo` case (→ pass-through,
  no error) — the echo fixture's appended `{python}` cell exercises exactly this.

### Phase 3: Echo engine integration test

End-to-end test with a minimal TypeScript engine that exercises **both**
discovery paths: language claiming (the resolution tiers) and file
claiming (the pre-parse flow). Without the file-claiming half, the
`EngineClaimsFileStage` + `markdown_for_file` pipeline gets no E2E
coverage in Plan 1c — Plan 4 (Julia + `.jl`) would be its first
end-to-end exercise.

**Dependency note:** The echo engine imports types from `@quarto/types`. With the
recommended order (**Plan 2 before Plan 1c** — see the grand plan's dependency
graph), Plan 2 Phase B has already refined these interfaces, so import them
directly. If 1c runs before Plan 2 instead, create a minimal type stub inline in
the echo engine file (just the interfaces it needs: `ExecutionEngineDiscovery`,
`ExecutionEngineInstance`, `QuartoAPI`) and swap it for the real imports once
Plan 2 Phase B lands.

- [ ] Create test fixture `tests/fixtures/extensions/echo-engine/`:
  ```
  _extension.yml
  src/echo-engine.ts
  fixtures/lang.qmd       # { echo } code block fixture
  fixtures/file.echo      # whole-file echo fixture
  ```
  `_extension.yml` declares `name: echo`,
  `claims: { echo: { kind: primary, priority: 1 } }`,
  `file-extensions: [".echo"]`, and `claims-files: [".echo"]` so the
  extension is **fully static** — it exercises the zero-load resolution path
  (no `LoadEngine` to resolve `{echo}` / `.echo`) as well as the
  zero-subprocess-on-mismatch fast path.
- [ ] **Second, *legacy* fixture engine** to cover the dynamic-fallback path in
  the same E2E run: a minimal extension `tests/fixtures/extensions/echo-legacy/`
  whose `_extension.yml` declares **only** `path: src/echo-legacy.js` (no
  `name`, no `claims`, no `claims-files`, no `file-extensions`). Its
  `echo-legacy.ts` claims a distinct language (e.g. `claimsLanguage: (lang) =>
  lang === "echolegacy"`) and executes it. The Rust test then asserts a
  `{echolegacy}` doc resolves this engine via a **dynamic `LoadEngine` +
  `claimsLanguage`** round-trip (the legacy path), and registers under its
  extension id with the `runtime_name → extension_id` alias populated on first
  load — the mirror of the static echo engine's zero-load path.
- [ ] `echo-engine.ts` — claims `"echo"` language AND `.echo` files:
  ```typescript
  const echoEngine: ExecutionEngineDiscovery = {
      name: "echo",
      claimsLanguage: (lang) => lang === "echo",
      claimsFile: (_file, ext) => ext === ".echo",
      launch: (ctx) => ({
          name: "echo",
          canFreeze: false,
          // For .echo files: wrap the file as an {echo} block, PLUS append a
          // second-language {python} cell that echo does NOT run — so the
          // single-engine claimed-file path is exercised end-to-end, including
          // the Q1-faithful **pass-through** of a language the claiming engine
          // can't execute (§8).
          markdownForFile: async (file) => {
              const text = await Deno.readTextFile(file);
              return {
                  value: "```{echo}\n" + text + "\n```\n\n" +
                         "```{python}\nprint('not run by echo')\n```\n",
                  fileName: file,
                  sourceMap: [],   // q2 protocol's flattened source-map, empty here (1C-2).
                                   // NOT Q1's shape — Q1's markdownForFile returns a
                                   // MappedString { value, fileName?, map() }, no sourceMap field.
              };
          },
          execute: async (opts) => ({
              engine: "echo",
              // Transform only {echo} cells; leave every other language cell
              // (the {python} above) untouched — i.e. pass it through
              // unexecuted, exactly as a single claimed engine does in Q1.
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
     output contains `ECHO_EXECUTED`, **and that the appended `{python}` cell
     is passed through unexecuted** — its source `print('not run by echo')`
     appears as a code listing with **no execution output** (the Q1-faithful
     single-engine pass-through, §8). Also assert resolution produced a
     **single-engine** sequence `[echo]` — no second engine was pulled in for
     the `{python}` cell, and the converted file's own `engine:` (if any) is
     ignored. This exercises:
     registry → `EngineClaimsFileStage` → `markdown_for_file` →
     `LoadedSource.conversion` populated → `ParseDocumentStage` registers the
     converted text under the engine-reflecting synthetic name (C′ — single
     registration, AST nodes get `Original(qmd_id)` into the converted buffer) →
     `claimed_engine_name` propagated → `resolve_engines` **single-engine
     short-circuit** → execute.
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

The **two-step shape** — bundle one TS entry to a single `.js` at build time,
then `import()` that `.js` at runtime — matches Quarto 1's extension build
model. **What q2 imports differs (1C-1, verified against Q1 source):** Q1's
build resolves only the **type-only** `@quarto/types` (via a *local* relative
`.d.ts` path in `src/resources/extension-build/import-map.json`, **not** jsr),
and has **no runtime SDK import at all** — Q1's `quarto` API is **ambient**,
injected into the engine via `init(quarto)`. q2 adds a published runtime
`@quarto/api` SDK because its engine runs in a **subprocess** and cannot receive
an in-process `init(getQuartoAPI())`. So the published-SDK model is a **forced
q2 departure**, not a Q1 port — do not describe it as "following Q1."

1. **Build time:** `deno bundle --config=<deno.json> <entry.ts>` bundles the TS engine extension into a single `.js` file. The engine's `deno.json` resolves `@quarto/api` (real code, inlined) and `@quarto/types` (type-only, erased) plus Deno std lib imports — **from the workspace in-repo, and from the published registry (`jsr:@quarto/api` / `jsr:@quarto/types`) for external authors** (D3; see "Distribution" below). `deno bundle` is a stable Deno feature (reintroduced in Deno 2.4, permanently supported; uses esbuild under the hood).
2. **Runtime:** The Deno subprocess loads the bundled `.js` file via dynamic `import()`. No import map or TS transpilation needed — everything is already resolved and bundled.

Note: The **engine-host harness** is built with esbuild (matching existing q2 patterns), while **engine extensions** are built with `deno bundle` (handling Deno-specific `jsr:` specifiers). These are different build steps for different artifacts.

This means the Deno subprocess invocation is simple:
```bash
deno run --allow-all <engine-host-deno.js>
```

No `--import-map` flag needed at runtime.

### Init `global` + LaunchEngine `project` field sources

Per RTQ Item A / DQ-7 the bootstrap context is **split across two frames**: a
process-stable `Init { global: HostGlobalConfig }` sent **once at spawn**, and a
per-render `LaunchEngine { project: EngineProjectContext }`. Several fields aren't
naturally available in q2 today; this table fixes their q2-side sources so the host
send and `TsEngine::launch` are implementable.

**`Init { global }`** (process-stable, sent once at spawn):

| Field | q2 source | Notes |
|---|---|---|
| `quarto_version` | `env!("CARGO_PKG_VERSION")` from the `quarto` crate, exposed via a `quarto_core::version()` const | one-liner |
| `resource_dir` | `crate::extension::BUILTIN_EXTENSIONS.path()` (existing `ResourceBundle.path()`) | narrower than Q1's "all bundled resources"; document the scope |
| `runtime_dir` | `{project_dir}/.quarto/cache/engines/`, created on demand | reuses the existing `.quarto/cache/` convention; no new persistent-state infra needed for Plan 1c |
| `data_dir` | q2 data-dir convention (alongside `runtime_dir`) | mirror `runtime_dir`'s resolution |
| `pandoc_path` | `ctx.runtime.find_binary("pandoc", "QUARTO_PANDOC")` (already exists in render.rs:51 via `BinaryDependencies::discover`) | `Option<String>` — `None` is fine; engines that need pandoc fail with a clear error only if they actually call it. q2 itself does not invoke pandoc on the main render path (pampa replaces it). |
| `is_interactive_session` | new `SystemRuntime::is_interactive(&self) -> bool` (NativeRuntime checks `IsTerminal` on stdin; WasmRuntime returns `false`) | small new method; ~10 lines |
| `running_in_ci` | new `SystemRuntime::running_in_ci(&self) -> bool` (reads `CI` env var via existing `env_get`) | small new method; ~5 lines |

**`LaunchEngine { project }`** (per render):

| Field | q2 source | Notes |
|---|---|---|
| `project_dir` | `ctx.project.dir` | trivial |
| `is_single_file` | `ctx.project.is_single_file` | trivial |
| `config` (`engines`, `output-dir`) + `output_dir` | `ctx.project.config` / output-dir resolution | values, not callbacks (DQ-5) |

The two new `SystemRuntime` methods (`is_interactive`, `running_in_ci`) are the only
genuinely new infrastructure required; everything else is existing machinery being
pointed at.

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
`main`) carries the title and heading data Q1 read via
`partitionedMarkdown.yaml`/`headingText` — **plus `draft`, which Q1 does
not store on `PartitionedMarkdown` at all** (its `core/pandoc/types.ts:16`
shape has no `draft` field): Q1 derives `draft` from the front-matter YAML
into the project index (`project-index.ts:284`), and q2 reads the same
front-matter via `DocumentProfile` (1C-8). Q1's pre-execute filter-YAML
harvest folds into the natural `MetadataMergeStage` cascade once filters run
inside `markdown_for_file`. See
`claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md`.

### Distribution of build-ts-extension assets (resolved: workspace build + publish for clients, D3)

Two build contexts, one specifier (`jsr:@quarto/api` / `jsr:@quarto/types`):

- **In-repo (q2 dev, incl. the echo E2E):** `q2 build-ts-extension` bundles
  against the **workspace** `ts-packages/quarto-api` + `quarto-types` via a
  workspace mapping in the repo's `resources/extension-build/deno.json` (which
  this plan creates — it does **not** exist yet). This is what lets **Phase 3's
  echo E2E build and pass without the packages being published** — the in-repo
  build never reaches the registry. (Unlike Q1, whose import map points at a
  local `.d.ts`; q2's points at the live workspace source.)
- **External authors:** the **published** `@quarto/api` / `@quarto/types` on the
  registry (jsr/npm). q2 **publishes `@quarto/api` out of the workspace** for
  clients (D3). `q2 build-ts-extension` then bundles against the registry — no
  q2 source clone, and no build assets embedded in or extracted from the q2
  binary. See the grand plan's "Distribution of the engine-author SDK".

q2 is **forced to publish** where Q1 got away with an ambient API + a local
`.d.ts` (1C-1) — the subprocess model requires a real, versioned SDK. **But
nothing in these plans depends on that publish.** `@quarto/api` is brand-new,
created *by* this epic; every in-repo build (registration, the Phase-3 echo
E2E, all of 1c) resolves it from the **workspace**, never the registry.
Publishing to jsr/npm is a **future, external-author-only** enablement (so
third parties can `jsr:@quarto/api`) and is **out of scope** for this epic — it
is *not* a readiness gate for anything here. The one ongoing surface, once
published, is API stability: `deno bundle` freezes the `@quarto/api` version
into each author's `.js`, managed by semver on the published package.

## Future Work: deferred-deps round-trip orchestration (book/project rendering)

The q2-side consumer of the deferred dependencies path (RTQ FC-2) lands here, with the
book/project renderer — **deferred, not built in 1c v1.** When an `execute` reply carries a
non-empty `engineDependencies` (only under `dependencies: false`), q2's render orchestrator must
iterate that map by engine name and send a `dependencies` message per key (`output` = the render
recipe's final/merged output), merging each `DependenciesResult.includes` — a direct port of Q1's
`render.ts:90-109` (*"run the dependencies step if we didn't do it during execute"*). The wire
surface (the `dependencies` verb, `engineDependencies` on `TsExecuteResult`, the `dependencies`
flag) is built up-front by **RTQ FC-2**, so this is a body-fill with no protocol surgery; v1 sends
`dependencies: true` (inline) and never exercises it. See RTQ FC-2 and Plan 1b's `dependencies`
message arm. (Capture/replay must also record this round-trip — see `2026-05-03-replay-engine.md`.)

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
This is out of scope for this plan and is a natural follow-on. The deferral
rests on a **finalized** design: the byte/column-precise `SourceInfo`
conversion needed to map percent/spin-script positions back to the original
file is worked out in
`claude-notes/plans/2025-12-15-source-info-for-structured-formats.md` (status
"Design finalized"). So this is a scoped-out-with-a-plan deferral, not an
open question — it lands when a built-in engine grows `claims_file`.

## Success Criteria

- [ ] Extension discovery finds engine extensions in `_extensions/`
- [ ] Both string (reordering) and object (new engine) forms parsed from `contributes.engines`
- [ ] `path` validation: lowercase `.js` accepted; `.ts`, `.JS`, `.mjs`, etc. rejected with actionable error
- [ ] `EngineContribution::External` carries `name: Option<String>`,
  `claims: Option<HashMap<String, StaticLanguageClaim>>` (kind/priority +
  `when_class` for `first_class`-conditional claims),
  `file_extensions: Option<Vec<String>>`, `claims_files: Option<Vec<String>>`
  (the authoritative static-claim fields, engine-resolution.md §3.3); YAML
  field-absent vs explicit empty (`{}`/`[]`) distinguished
- [ ] **Static claims are authoritative for resolution:** a declared
  `claims`/`claims-files` resolves with no `LoadEngine`; an engine declaring
  none (legacy Q1 `path`-only) falls back to dynamic `LoadEngine` +
  `claims_language`/`claims_file`. A declared claim that disagrees with the
  dynamic method at first execute-time load is a hard error
- [ ] Missing-static-fields warning fires once per extension at discovery time, naming the missing field(s) (any of `name` / `claims` / `file-extensions` / `claims-files`) and showing the YAML snippet to add (and the per-render dynamic-load cost incurred until added)
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
- [ ] `claimed_engine_name` propagates from the pre-parse stage and makes
  `resolve_engines` **short-circuit to that single engine** (Q1-faithful, §8 —
  no tiers, no seed, claimed file's own `engine:` ignored, non-kernel cells
  pass through unexecuted)
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
- [ ] `Init { global }` + `LaunchEngine { project }` populated from documented field-source table (incl. two new `SystemRuntime` methods: `is_interactive` and `running_in_ci`)
- [ ] Echo engine integration test exercises **both** discovery paths: language claim (`{echo}` blocks in `.qmd`) and file claim (`.echo` whole-file)
- [ ] Tests requiring Deno are skipped if Deno is absent (runtime `has_deno()`
  check with early return, matching the pandoc test pattern)
- [ ] All existing tests pass (no regressions)
