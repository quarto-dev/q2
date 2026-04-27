# Plan 1a (engine): TsEngine and ExecutionEngine trait extensions

**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Companion plans:** [plan1a-protocol](2026-04-16-plan1a-protocol.md) (data types), [plan1a-host](2026-04-16-plan1a-host.md) (subprocess + transport)
**Depends on:** plan1a-protocol (uses Ts* types), plan1a-host (uses `TsEngineHost` API)
**Soft-depends on:** Plan 1b (Deno harness) — a **runtime-only** contract: the
`discovery` `OnceLock` benign-race correctness relies on the harness handling
repeat `LoadEngine` idempotently (see "Race-free init"). Plan 1a is implemented
and unit-tested against `MockTransport` **without** Plan 1b; end-to-end coverage
of the composition lands in Plan 1c's echo-engine test.
**Blocks:** Plan 1c (constructs `TsEngine`, calls trait methods)
**Estimated sessions:** 1

## Overview

Extend the `ExecutionEngine` trait with discovery and file-conversion
methods, add the `LanguageClaim` enum and the `resolve_engines` resolver
(including the AST scan that enumerates the document's computational
languages), relocate `HtmlDependency` for q2-native consumers, and create the
`TsEngine` struct that bridges the (synchronous) trait to the protocol +
subprocess. Includes the two-step lazy lifecycle, hint-based pre-filter, alias
map, race-free init via harness idempotency, and the `MockTransport`-driven
test suite.

**This plan is the engine-side of the multi-engine resolution model.** Since
the April draft, sequential multi-engine execution, capture/replay, and the
discovery cache landed on `main` (bd-5yff4 / bd-45yw / bd-c5u2g). The trait's
claim surface and the resolver here feed that machinery; the cross-cutting
model — kinds/tiers, per-language ownership, `handled_languages` enforcement,
replay-from-captures — is specified once in
`claude-notes/designs/engine-resolution.md` and referenced throughout this
plan rather than re-derived.

## Drift notes (verified 2026-06-24, before execution start)

The plan text predates a few changes that landed with plan1a-host. None are
blockers; adapt as you go:

- **`ExecutionError::Timeout { engine, operation }` already exists** (with a
  `timeout(..)` constructor) — plan1a-host added it. The Phase 3 "add `Timeout`"
  item is **already done**; only `NotSupported(&'static str)` and
  `NoHandlerForLanguage { engine, language }` remain to add. (`ProcessCrashed`
  also already exists.)
- **`stage::cancellation` is already `pub mod`** (not private as the Phase 4
  prerequisite assumed). `Cancellation` is reachable as
  `crate::stage::cancellation::Cancellation`; a `pub use` re-export at the
  `stage` level is now only ergonomic, not required to compile.
- **`MockTransport` / `with_transport` shipped a richer split-half API** than
  this plan's prose describes. Reality (in `ts_process.rs`, `#[cfg(test)]`):
  `TsEngineHost::with_transport(write: Arc<dyn EngineTransport>, read: Box<dyn
  EngineReadHalf>, ctx)` fed by `MockTransport::pair()` /
  `MockTransport::pair_with_handle() -> (write, read, Arc<MockWriteHalf>)`. The
  write handle exposes `enable_auto_echo()`, `script_response(id, resp)`,
  `script_response_delayed(..)`, `signal_eof()`, and `sent_messages() ->
  Vec<ToEngine>`. All Phase-4 test capabilities the plan needs exist; use these
  names.
- **`HtmlDependency` relocation → keep-in-place + add derives** (see the amended
  Phase 3 dep item below).

## Work Items

### Phase 3: ExecutionEngine trait — discovery + file conversion

Extend the `ExecutionEngine` trait with discovery and `markdown_for_file`.
**All trait surface uses q2-native types only.**

Q1's other lifecycle hooks (`filterFormat`, `executeTargetSkipped`,
`postprocess`, `canKeepSource`, `postRender`, `dependencies`,
`partitionedMarkdown`) are intentionally **not** added to the trait.
For most of them, no q2 caller exists, and adding q2-native equivalents
without a real second implementer would calcify the design prematurely.
For `partitionedMarkdown` specifically, q2's pipeline shape replaces the
need: `DocumentProfile` (post-merge, pre-mutation checkpoint) carries
the title/heading/draft data project-scoped features read, and
filter-aware notebook conversion folds into `markdown_for_file`. See
`claude-notes/plans/2026-04-23-ipynb-filters-and-engine-partitioning.md`.

**Quarto 1 references:**
- `ExecutionEngineDiscovery` in `src/execute/types.ts` — discovery interface
- `ExecutionEngineInstance` in `src/execute/types.ts` — full lifecycle interface

- [x] **Add `ExecutionError::NotSupported(&'static str)` variant** to
  `crates/quarto-core/src/engine/error.rs`. Used by trait method defaults to
  signal "this engine doesn't implement X." The constructor `not_supported`
  follows the existing pattern.

- [x] **Add `ExecutionError::Timeout { engine, operation }` variant** to
  `crates/quarto-core/src/engine/error.rs` (constructor `timeout`, existing
  pattern). plan1a-host's `request` returns a **distinguishable forcible-abort
  error** so `TsEngine::execute` can decide whether to poison the instance: a
  user-cancel → the existing `ExecutionError::Cancelled`, a per-request
  timeout → this new `Timeout`. A normal engine failure stays
  `ExecutionFailed`. `execute` poisons **only** on `Cancelled | Timeout`
  (the forcible aborts that can leave the daemon ambiguous), never on
  `ExecutionFailed` — see the `execute` bullet.

- [x] **Add `ExecutionError::NoHandlerForLanguage { engine, language }`
  variant** to `crates/quarto-core/src/engine/error.rs` (constructor
  `no_handler_for_language`, existing pattern). The §10-case-4 loud failure: a
  resolved owner is handed a language it owns but cannot run (e.g. jupyter +
  `{sql}`). It is a **clean refusal, not a forcible abort** — so it is **not**
  in the poison match (`execute` poisons only on `Cancelled | Timeout`; this
  falls through to no-poison by exclusion, like `ExecutionFailed`). See the
  "Loud failure" item in Phase 3.5 and design doc §10.

- [x] Add the **`LanguageClaim` enum** in `engine/mod.rs` (co-located with the
  `HANDLED_LANGUAGES` constant below — both are shared, WASM-clean types
  consumed by the trait in `traits.rs`, the resolver in `resolution.rs`, and
  `ts_engine.rs`; the module root keeps them free of a `traits.rs`↔`resolution.rs`
  dependency cycle). This replaces the April `Option<i32>` design: the
  multi-engine semantics need three distinct *kinds* that don't fit a sign
  convention.
  See `claude-notes/designs/engine-resolution.md` §3.1 for the full contract.
  ```rust
  pub enum LanguageClaim {
      Primary(i32),   // I execute this. (default priority 1)
      Interop(i32),   // extend my ownership to this iff I'm already present. (default 0)
      Fallback(i32),  // universal kernel (jupyter's role; declarable by any engine). (default 0)
      None,
  }
  ```
  **Semantics (the resolver in §4 of the design doc consumes these):** `kind`
  sets the resolution tier; `priority` orders *only within* a kind (kind
  dominates priority — `Primary(-100)` beats `Fallback(100)`); `Interop` is
  presence-gated (fires only for an engine already in the sequence via a
  positive claim — "extend if I'm already here," not "claim anywhere");
  `Fallback` is the universal-kernel role, no longer hardcoded to jupyter.

- [x] Add **discovery methods** to `ExecutionEngine` trait with defaults:
  ```rust
  fn valid_extensions(&self) -> Vec<String> { Vec::new() }
  fn claims_language(&self, _language: &str, _first_class: Option<&str>) -> LanguageClaim { LanguageClaim::None }
  fn claims_file(&self, _file: &str, _ext: &str) -> bool { false }
  ```
  **All new trait methods ship with a default body, so no existing
  `ExecutionEngine` impl is forced to change and there is no compile
  cascade.** There is no pre-existing `claims_language` to "preserve" — this
  is new surface. Built-ins override only what they need: knitr/jupyter
  override `claims_language`; markdown keeps the `None` default;
  `claims_file` / `valid_extensions` / `markdown_for_file` stay on defaults
  for all three built-ins (non-QMD support is future work, Plan 1c).
  `TsEngine` overrides all four.

- [x] Add **file conversion method** with q2-native return type:
  ```rust
  /// Convert a non-QMD file to QMD text. Called only for files this
  /// engine claimed via `claims_file`. For QMD files, q2 handles
  /// parsing directly and this method is never called.
  ///
  /// Convert a non-QMD file to QMD text. Returns the converted text; the
  /// `SourceInfo` slot is reserved for faithful original-file provenance
  /// (deferred — see "Provenance" below) and is `SourceInfo::default()` in
  /// v1.
  fn markdown_for_file(
      &self,
      _file: &Path,
      _runtime: &Arc<dyn SystemRuntime>,
  ) -> Result<(String, SourceInfo), ExecutionError> {
      Err(ExecutionError::not_supported("markdown_for_file"))
  }
  ```
  The `runtime` parameter is the q2-canonical FS abstraction; engines that
  need to read the file use it. `TsEngine` ignores `runtime` (the subprocess
  reads files via Deno) and returns the harness's
  `TsMappedStringWithMap.value` as the converted text. The signature stays
  `(file, runtime)` — no `SourceContext` is threaded in (see "Provenance").

  **Provenance — v1 registers the converted text as an ephemeral intermediate
  file (decided 2026-06-24; scope = C′).** Faithful byte-mapping back to the
  *original* non-QMD file (so a diagnostic in a converted `.ipynb` cell points
  at the source cell) is **deferred** — it has no q2 consumer yet, and the two
  faithful mechanisms (see "Future work") are each a real investment. v1 does
  the honest, cheap thing instead, mirroring how engine intermediates are
  already handled (`engine_execution.rs:423/701` `add_file` ephemeral content
  → a real `FileId`):
  - The converted text is registered as an **ephemeral intermediate file**
    via `SourceContext::add_file(synthetic_name, Some(text))` on the
    document's existing context — **the qmd parser already does exactly this**
    (`qmd.rs:106`), so for the normal convert-then-parse path the `FileId` is
    invented for free and every node gets honest
    `Original { file_id, start, end }` provenance **into the converted
    buffer**. No `&mut SourceContext` on the trait, no `parent_source_info`,
    no transform pass.
  - **Synthetic identity reflects the engine that produced it.** Register
    under a name that names the converting engine, not the bare original path
    — e.g. `"<{original} (converted by {engine})>"` (matching the codebase's
    `<anonymous>`/`<unknown>` synthetic-name idiom). This is deliberate
    honesty: the offsets are positions in the *converted* buffer, not the
    original bytes, so the identity must not masquerade as the original file
    (which would point a reader at wrong line/cols in e.g. the `.ipynb` JSON).
    Naming the engine signals "this is a derived buffer."
  - **`source_map` stays on the wire, unconsumed.** `TsMappedStringWithMap`
    keeps its `source_map`/`file_name` fields (the protocol does **not**
    change), but v1 ignores them; they are the input the future A′/B′
    back-mapping will consume. The returned `SourceInfo` is `default()` in v1
    — real provenance comes from the parser's `add_file`, not from this slot.

  **Future work (commendable, not in these plans): faithful original-file
  mapping.** When a consumer needs converted-cell → source-cell positions,
  prefer **A′ — a generalized remap pass**: parse the converted text (its own
  `FileId`), register the original file, then walk the AST rewriting each
  `Original { converted_fid, s, e }` into the original-file `SourceInfo` via
  `source_map`. This *extends the proven include/engine FileId-remap idiom*
  (`include_expansion.rs:199` swaps a `FileId`; A′ generalizes "swap" to
  "apply an offset map"). Avoid **B′ — `parent_source_info` / `SourceInfo::Concat`**
  (pass the `Concat` as the parse's `parent_source_info` so nodes become
  `Substring(Concat, …)`): it rides the **dormant** `parent_source_info` path
  (no production caller passes it non-`None` today, `location.rs:215`) *and*
  `Concat` resolution requires byte-contiguous pieces (`source_info.rs:418-456`),
  which the gappy mappings real conversions produce will violate. A′ is more
  code but proven; B′ is less code but unproven + constrained.

  **Not on Rust trait** (harness-internal for TS engines):
  - `target()` — q2 constructs execution target data from its AST. TS
    engines may implement it for Quarto 1 API compat (transient notebooks,
    kernelspec). The harness builds the `ExecutionTarget` from
    `TsExecuteOptions` fields when the engine doesn't implement it.
  - `dependencies()` — Q1's deferred-deps resolution flow. The harness
    folds this into `execute` (see plan1a-protocol Phase 1 protocol notes); q2 receives a
    resolved q2-shaped `Vec<HtmlDependency>` on `ExecuteResult`, not the
    deferred map.

- [x] **Fix the `intermediate_files` doc-comment on the trait.** The
  existing `intermediate_files` doc-comment in
  `crates/quarto-core/src/engine/traits.rs` currently says the returned
  files "may need to be cleaned up after rendering completes" — wrong
  framing. `intermediate_files` is a *pure prediction of intermediate
  file paths derived from the input path* (NOT post-execution
  introspection, NOT a cleanup list): the argument is the original
  source path; the return lists paths the engine will produce alongside
  the primary output (e.g. a generated `.ipynb`, `.html.md` backups);
  the result is used to **exclude those paths from the project's
  input-file set** so they are not treated as separate render targets.
  Rewrite the doc-comment to match these semantics when this plan is
  implemented.

- [x] Implement on built-in engines (claim tables per
  `claude-notes/designs/engine-resolution.md` — jupyter's `Fallback(0)` and
  the T4 gate are §4.3; the knitr/markdown rows are the §4.4 worked cases and
  the §3 model):
  - **JupyterEngine**: `claims_language(..) → LanguageClaim::Fallback(0)` for
    every language it is asked about — jupyter is the default universal
    fallback (asked only about the doc's actual executable, non-handler
    languages; it never enumerates). **Deliberate q2 design choice (new
    surface, not a Q1 port):** jupyter does not claim "julia" at priority 1
    the way Quarto 1 did (`claimsLanguage` jupyter.ts:113–117). Under the enum this
    falls out for free — jupyter's `Fallback(0)` *loses* to the Julia
    extension's `Primary(1)` (kind dominates priority), so the Julia
    extension wins cleanly when installed, and `{julia}` without it still
    reaches jupyter via the `Fallback` tier (T4). **`claims_file`,
    `valid_extensions`, and `markdown_for_file` use the trait defaults for
    now** — jupyter does not claim `.ipynb` or percent scripts in the scope
    of these plans. Built-in non-QMD support is documented as future work in
    Plan 1c (its "Future Work: Built-in engine percent/spin script support"
    section); doing it well requires a Rust port of Q1's
    `markdownFromJupyterPercentScript` plus an `.ipynb` parser, neither of
    which has a current q2 consumer. The trait machinery is shipped here so
    the future implementation is a drop-in. ipynb-filter handling, when
    implemented, lives inside jupyter's `markdown_for_file` override (see
    ipynb-filters research plan).
  - **KnitrEngine**: `claims_language("r", _) → Primary(1)`; **`Interop` for
    `["python", "sql", "bash", "sh"]`** so knitr *keeps* them when it's
    already running R but *cedes* them to a dedicated engine when one is
    present (its `Primary` out-ranks knitr's `Interop`). This set is the
    knitr `knit_engines` capability — the languages knitr actually executes
    in-session — not a guess: `python` via `eng_python`/reticulate, `sql` via
    `eng_sql`/DBI, `bash`/`sh` via the shell engines. `sql` is **pinned, not
    deferred**: Q1 ships dedicated support for knitr-executed SQL —
    `knitr-fixup.lua:4-12` repairs the `knitsql-table` div `eng_sql` emits and
    `_quarto-rules.scss:385` styles `.knitsql-table` — so `{sql}`-in-knitr is a
    supported path, not a maybe. (Optional future extension if `Interop` means
    raw `knit_engines` capability rather than verified-output handling:
    `awk`/`ruby`/`perl`/`stan`. `python` remains the load-bearing case.)
    **Deliberate q2 design choice (not a Q1 port):** Q1 knitr's
    `claimsLanguage` claims *only* `"r"`
    (`external-sources/quarto-cli/src/execute/rmd.ts:77-79`) — the other
    languages are a knitr-package *execution-time* capability (`knit_engines`),
    never a claim-layer claim. q2 does **not** call reticulate itself (zero
    references in `quarto-cli/src/`); it lifts knitr's implicit in-session
    capability into an explicit `Interop` claim so the multi-engine resolver
    (§4) can reason about it and hand e.g. `{python}` to a dedicated engine
    when one is present. (Same shape as the jupyter/julia change above — q2
    makes an implicit Q1 behavior explicit at the claim layer.) Note the
    distinct axis: knitr's `handled_languages` (`ojs`/`mermaid`/`dot`) are
    *pass-through cell handlers* knitr re-emits, **not** languages it
    executes — the opposite of `Interop`. **No `claims_file` /
    `valid_extensions` overrides** (same scope decision: spin-script support
    is future work). Trait default for `markdown_for_file` for now.
  - **MarkdownEngine**: returns `LanguageClaim::None` (claims nothing).

- [x] **Promote knitr's hardcoded `["ojs", "mermaid", "dot"]` to a shared
  constant.** Add `pub const HANDLED_LANGUAGES: &[&str] = &["ojs", "mermaid",
  "dot"]` in `crates/quarto-core/src/engine/mod.rs`. The literal currently
  appears at **three sites** that must all read from the constant:
  `crates/quarto-core/src/engine/knitr/mod.rs:187`,
  `crates/quarto-core/src/engine/knitr/types.rs:250`, and the test at
  `crates/quarto-core/src/engine/knitr/subprocess.rs:903`. `TsEngine::execute`
  reads from the same constant when populating
  `TsExecuteOptions.handled_languages`.

  **Semantics: instruction, not documentation.** This list tells the engine
  which language blocks to **leave alone** in its output — q2 will handle
  them downstream via cell handlers (today: ojs, mermaid, dot — none of
  these are real cell handlers in q2 yet, but the protocol contract is
  established now so it doesn't change later). Engines take the whole
  document and return the whole document, so they need to know which
  blocks not to execute. Knitr's R subprocess already follows this
  contract; TS engines must follow the same. When q2 grows real cell
  handlers, this constant migrates to a registry — single source of
  truth in the meantime.

- [x] **Add `Serialize`/`Deserialize` to `HtmlDependency` and friends *in
  place* in `pampa`.** *(Amended 2026-06-24: the original "relocate to
  `quarto-core::dependency`" instruction was **impossible** — it would create a
  dependency cycle. `quarto-core` depends on `pampa`, never the reverse, and
  `pampa` itself **constructs and uses** these types: `quarto_doc.rs` builds
  them in `extract_html_dependencies`/`extract_text_includes`, and
  `unified_filter.rs`, `lua/shortcode.rs`, `lua/filter.rs` all hold
  `Vec<HtmlDependency>`/`Vec<TextInclude>`. Moving the definitions out of
  `pampa` would force `pampa → quarto-core`. The relocation's stated motivation
  — "don't force `quarto-core` to depend on `pampa::lua`" — is already moot:
  `quarto-core/src/dependency.rs:16` already imports `pampa::lua::{HtmlDependency,
  IncludeLocation, TextInclude}`, and **no crate outside `pampa`/`quarto-core`
  names these types**. So we keep them where they are and reference them via the
  existing re-export.)*
  The types live in `crates/pampa/src/lua/quarto_doc.rs` and are re-exported
  (un-gated, WASM included) from `pampa/src/lua/mod.rs:36-38`. **Leave them
  there.** `ExecuteResult.html_dependencies: Vec<HtmlDependency>` references the
  type through the existing `pampa::lua` re-export (the import
  `quarto-core/src/dependency.rs` already uses; `engine/context.rs` imports the
  same).
  **`HtmlDependency`, `TextInclude`, *and* `IncludeLocation` must gain
  `Serialize` / `Deserialize` derives (added in `quarto_doc.rs`).** (`TextInclude`
  contains `IncludeLocation`, so the enum needs the derives too or
  `TextInclude`'s won't compile; today all three derive only `Debug, Clone` —
  `IncludeLocation` also `PartialEq, Eq`.) `pampa` already has `serde` with the
  `derive` feature (`Cargo.toml:58`) and `serde_json` (`:59`), so the derives are
  trivially available. `ExecuteResult` is already `Serialize`/`Deserialize` on
  `main` (captured as a `serde_json::Value` inside `EngineCapture` for the
  trace/replay path, bd-45yw); a new `html_dependencies` field that doesn't
  round-trip would break capture serialization. Add the derives in the same
  commit that adds the `ExecuteResult` field.

- [x] **Add `html_dependencies: Vec<HtmlDependency>` to `ExecuteResult`** in
  `crates/quarto-core/src/engine/context.rs`. `EngineExecutionStage` calls
  `crate::dependency::store_html_dependencies` on this field after each
  execute, in addition to extending `ctx.includes` from `result.includes`.
  **Note on the current `ExecuteResult` shape (`main`):** the struct already
  derives `Serialize`/`Deserialize`/`Default`, the supporting-files field is
  named `supporting_files` (not Q1's `supporting`) and carries
  project-resource semantics (bd-o8pr: drained from `StageContext.resource_report`
  and copied into the output dir), and there is no `metadata` field. The new
  `html_dependencies` accumulation must run **inside the multi-engine loop**
  (one `store_html_dependencies` call per engine), alongside the existing
  per-engine `includes` / `supporting_files` accumulation.

  **The two channels are disjoint** (see plan1a-protocol's "Two disjoint dep
  channels" note). `ExecuteResult.includes` (`PandocIncludes`) carries
  pre-rendered HTML/text fragments from Q1-shaped engines (the harness
  routes `engine.dependencies(...)` results here); `ExecuteResult.html_dependencies`
  carries structured `{ name, stylesheets, scripts }` manifests from
  engines that opt into a Q2-native registration API (Plan 1b's
  `quarto.htmlDependency()` helper). Engines populate one or both;
  q2 routes each to its own sink without dedup logic at the boundary.

- [x] **Dedup `HtmlDependency` by `name` in `store_html_dependencies`.**
  This is a **different** dedup from the one q2 already does, and the two
  must not be conflated. `store_html_dependencies` stores under
  `ArtifactScope::Project`, which dedupes the **same** artifact shared across
  pages (cross-page sharing). It does **not** guard against two engines
  registering **different** content under the **same** `name` — those both
  write to `libs/{name}/…` and the second clobbers the first. Add a
  name-collision guard: key on `name` only, **first-wins**, drop the later
  registration entirely (matching Q1's unit-of-dedup at
  `external-sources/quarto-cli/src/command/render/pandoc-dependencies-html.ts:228-237`,
  which `continue`s past a later dependency whose `name` already appears).
  **Improve on Q1:** Q1's drop is *silent*; q2 pushes a
  `DiagnosticMessage::warning` naming both registrants. The dedup happens at
  storage time (q2's artifact-store-as-canonical-sink), unlike Q1 which
  dedupes at injection time. Document the two dedups (project-scope vs.
  name-collision) in the function's doc-comment, and cover the name collision
  with a regression test (two engines emit `{ name: "jquery" }` with different
  content → first wins, one warning).

  **Deferred q2-native fields:** `preserve` (HTML preservation /
  postprocess) and `pandoc` (format-affecting options) are NOT added to
  `ExecuteResult` in this plan. They have no q2 consumer (no postprocess
  stage; format mutation is upstream of execute). When q2 grows the
  consumers, the harness will translate from Q1's deferred shape into
  q2-native fields. See `claude-notes/plans/2026-04-18-html-js-deps-design.md`
  for the broader JS-deps story.

- [x] Write tests for built-in engine claiming: knitr's
  `claims_language("r", _)` returns `Primary(1)` and `claims_language(L, _)`
  returns `Interop(_)` for each of `L ∈ {"python", "sql", "bash", "sh"}`
  (and `None` for an unclaimed language like `"julia"`); jupyter's
  `claims_language` returns `Fallback(0)` for
  all inputs (including "julia" — verifying it loses to a `Primary(1)` julia
  claim but wins over `None`); markdown returns `None` for everything. No
  `claims_file` tests for built-ins — they use the trait default (returns
  `false`), which is checked once via the trait-default test below.
- [x] Write tests for default `markdown_for_file` returning `NotSupported`.

### Phase 3.5: Engine resolution (`resolve_engines`) + ownership enforcement

The pure resolver that turns claims into an ordered sequence + a per-language
ownership map, and the execute-time enforcement that makes engines cede cells
they don't own. Full model: `claude-notes/designs/engine-resolution.md`
(§4 tiers, §5 enforcement, §9 artifact).

- [x] **Create `crates/quarto-core/src/engine/resolution.rs`** with the pure
  resolver and its artifact:
  ```rust
  pub struct EngineResolution {
      pub sequence:  Vec<DetectedEngine>,        // ordered, distinct owners
      pub ownership: HashMap<String, String>,    // language -> owning engine name
  }
  impl EngineResolution {
      /// HANDLED_LANGUAGES ∪ { lang : ownership[lang] != engine }
      pub fn handled_languages_for(&self, engine: &str) -> Vec<String>;
  }
  pub fn resolve_engines(
      meta: &ConfigValue, ast: &Pandoc, registry: &EngineRegistry, claimed: Option<&str>,
  ) -> EngineResolution;
  ```
  The four tiers (Primary → explicit-Fallback → Interop → implicit-Fallback),
  presence-gating, kind-dominates-priority, the implicit-only gate on T4, and
  the per-language ownership rule all live here. `claimed` is the file-claim
  `Primary`-seed (§8 of `engine-resolution.md`). The result is a pure
  function of `(meta, ast, registry, claimed)` — no I/O — so the future
  Pass-1 lift (stamp it on `DocumentProfile`) is a zero-cost move.

  **`DetectedEngine.config` provenance (the resolver does more than name
  engines).** `sequence` is `Vec<DetectedEngine>` and `DetectedEngine` is
  `{ name, config: Option<ConfigValue> }` (`detection.rs:38-47`); the stage
  threads `detected.config` into each engine's `ExecutionContext` via
  `with_engine_config`. The tiers resolve *names*, so `resolve_engines` must
  also attach config: it reads the explicit `engine:` block out of `meta` and
  **attaches each listed engine's config to the matching resolved owner**;
  **claim-derived owners** (e.g. jupyter reached via T4, or an `Interop`
  extension) get `config: None`. State this so an implementer doesn't ship a
  resolver that returns name-only entries and silently drops user
  `engine:`-supplied config.

- [x] **Enumerate the document's computational languages from the AST** —
  the `languages` input the tiers consume (design doc §4.1/§4.2). **This scan
  does not exist today:** `detect_engine_sequence` is metadata-only, and
  `engine/detection.rs` explicitly lists "Code block languages (`{python}` →
  jupyter)" as a *Future Enhancement*. Add it now, in `resolution.rs`, as a
  pure helper feeding `resolve_engines`:
  ```rust
  /// Ordered, de-duplicated computational languages of the document, each
  /// paired with the cell's first non-language class (`first_class`, §4.2).
  /// Mirrors Q1's `languagesWithClasses(markdown)` (engine.ts:174) — the
  /// first occurrence of a language wins its `first_class`.
  fn computational_languages(ast: &Pandoc) -> Vec<(String, Option<String>)>;
  ```
  Rules (design doc §4.1, "What counts as a computational language"):
  - **Executable cells only** — a braced `{lang}` fence. Reuse the existing
    per-block primitive
    `engine::capture_splice::engine_cell_lang(&Block) -> Option<&str>`
    (it matches the brace-wrapped class pampa preserves; plain ` ```r `
    highlight fences have no braces and are skipped).
  - **Recurse** into container blocks (Divs, `BlockQuote`, list items, etc.) —
    cells can be nested, so the per-block primitive must be driven by a full
    block walk. **No shared block-walker exists to reuse** — the tree has only
    ad-hoc local walkers (e.g. `engine_execution.rs:1003`'s
    `fn walk_block(b, out)` collecting `FileId`s, the closest structural
    precedent). Hand-roll a small private recursion in `resolution.rs`
    mirroring that idiom; do not build a general visitor.
  - **Exclude `HANDLED_LANGUAGES`** (`ojs`/`mermaid`/`dot` — cell handlers,
    not engines). Raw-attribute fences (`` ```{=html} ``) need no handling
    here: pampa parses them as `RawBlock` (`fenced_code_block.rs:74-87` routes
    a raw format to `Block::RawBlock`), and `engine_cell_lang` matches only
    `CodeBlock`, so it never returns them. `HANDLED_LANGUAGES` is the only
    thing the scan filters. (Do **not** add a "tokens starting with `=`"
    filter — there are no such tokens at this point; an earlier draft assumed
    `engine_cell_lang` returns `=fmt`, which is false.)
  - **`first_class`** is the cell's first class *after* the language token
    (e.g. `{python .marimo}` → language `python`, first_class `marimo`),
    read from the `CodeBlock` attr class list. It sharpens *selection* but
    not ownership (§4.2), and is passed straight to `claims_language`.
  - **Empty set → no engine → markdown passthrough** (§4.1).
  `resolve_engines` calls this internally; `EngineExecutionStage` passes the
  AST it already holds. Update `detection.rs`'s "Future Enhancements" comment
  (the future has arrived) — though the explicit `engine:`-key path in
  `detection.rs` stays metadata-only; this is the *language* axis, not the
  declared-engine axis.
- [x] **`EngineExecutionStage` calls `resolve_engines` once** at the top of
  `run`, stashes `EngineResolution` on `StageContext` (mirroring
  `project_index` in `run_pipeline`), reads `ownership` to build each
  engine's `handled_languages` via `handled_languages_for`, and the trace
  records `sequence`. This is a function + `StageContext` artifact, **not** a
  new pipeline stage (it transforms no `PipelineData`).
- [x] **jupyter execute-time `handled_languages` enforcement.** jupyter's
  *claiming* is already correct (`Fallback(0)`), but it has **no**
  `handled_languages` consumption today and runs every cell it's handed. Add
  an execute-time gate: jupyter skips / re-emits verbatim any cell whose
  language is in its leave-alone set. Required when jupyter is **non-terminal**
  in a sequence (e.g. explicit `[jupyter, knitr]`); as the terminal/fallback
  engine it owns the remainder and never cedes. knitr already enforces via
  `knit_engines` (the population just changes from the static
  `[ojs,mermaid,dot]` constant to the ownership projection); TS engines honor
  the contract via `TsExecuteOptions.handled_languages`. See design doc §5.
- [x] **Loud failure when an owner can't execute a language it owns** (design
  doc §10 case 4; scope expansion blessed 2026-06-24 — "adapting the existing
  engines to the TsEngine/Quarto-API contract is part of the work"). The
  four-tier model can hand an engine a language it has no handler for: e.g.
  `engine: [knitr, jupyter]` with `{sql}` routes `sql` to jupyter via
  explicit-`Fallback` (T2 > knitr's `Interop`, §4.4), but jupyter has no SQL
  kernel — whereas knitr's `eng_sql` does. The owning engine MUST fail with a
  clear `ExecutionError` naming **engine + language** ("engine `jupyter` has no
  kernel for `sql`"), **not** silently skip the cell or emit it unexecuted.
  - **This is an execute-time failure by design, NOT a pre-execute capability
    probe (decided 2026-06-24).** Resolution stays capability-blind so engine
    *selection* is a deterministic, environment-independent pure function —
    which is what lets it lift to Pass-1 / `DocumentProfile`. An eager "can
    jupyter run sql?" check would make which-engine-is-chosen depend on the
    installed kernels. So we accept that `[knitr, jupyter]`+`{sql}` runs knitr's
    `{r}` cells first and *then* halts at the `{sql}` cell (partial work before
    the loud halt) — the trade for deterministic selection. Design doc §10.
  - For **jupyter**: when it owns a language (not in its leave-alone set) for
    which its (single, per-doc) kernel can't run, error rather than no-op.
    Reuse/extend the existing kernel-not-found path (§10 case 3); a distinct
    message that names the *language* (not just the kernel) is the improvement.
    Add a new `ExecutionError::NoHandlerForLanguage { engine, language }`
    variant (constructor `no_handler_for_language`, existing pattern).
  - **`NoHandlerForLanguage` does NOT poison the instance.** It is a clean
    refusal — the engine never started computing — so it behaves like
    `ExecutionFailed`, not a forcible abort. `execute` poisons **only** on
    `Cancelled | Timeout` (the existing match), so this new variant correctly
    falls through to no-poison by exclusion; do **not** add it to the poison
    match.
  - This closes the only silent-failure hazard the broadened knitr `Interop`
    set (`[python, sql, bash, sh]`) introduces; the common knitr-only
    `{r}`+`{sql}` case still resolves `sql → knitr` (Interop wins with no
    explicit fallback present).
- [x] **Unit-test the resolver in isolation** with a `MockEngine` registry of
  hand-written claim tables (no subprocess, no AST execution): the worked
  cases from design doc §4.4 — implicit `{r}`+`{python}` → `[knitr]`
  (reticulate); implicit `{r}`+`{sql}` → `[knitr]` (`sql → knitr` via Interop);
  explicit `[knitr, jupyter]` → r→knitr, python→jupyter; **explicit
  `[knitr, jupyter]` with `{sql}` → `sql → jupyter`** (T2 explicit-`Fallback`
  preempts knitr's `Interop` — the routing that triggers the §10-case-4 loud
  failure at execute); pure `{python}` → `[jupyter]`; `{julia}` ± extension;
  `Fallback` priority ordering beating registration order; `Primary(-100)`
  beating jupyter's `Fallback(0)` (kind dominates — the §4.4 table row); T4
  implicit-only (explicit `[knitr]` + `{julia}` does **not** add jupyter —
  stated in §4.3 prose). These tests pin the tier logic without any of the
  TsEngine subprocess machinery.

### Phase 4: TsEngine struct

The Rust struct that implements `ExecutionEngine` by delegating to the shared subprocess.

- [x] Create `crates/quarto-core/src/engine/ts_engine.rs`:
  ```rust
  pub struct TsEngine {
      /// The registry key under which this TsEngine was inserted —
      /// either the `name` declared in `_extension.yml` (declared
      /// path) or the extension id (lazy-alias path). Used for log
      /// messages and as the value returned by `name()` until/unless
      /// the lazy-alias resolution updates it.
      name: String,
      /// Whether `name` was declared up-front in `_extension.yml`.
      /// When `true`, the first `LoadEngine` validates that
      /// `LoadEngineResult.name == self.name` and errors on mismatch.
      /// When `false`, the first `LoadEngine` records the runtime
      /// name in the registry's alias map.
      name_declared: bool,
      host: Arc<TsEngineHost>,          // Shared subprocess (from EngineRegistry).
                                        // Bundle is embedded in the q2 binary
                                        // via include_str! (plan1a-host's
                                        // "Bundle embedding" design note);
                                        // TsEngine doesn't carry a bundle path.
      // Two-step init state machine.
      // None: not yet loaded. Some: module loaded, discovery available.
      discovery: OnceLock<LoadEngineResult>,
      // None: not yet launched. Some: instance running, execute/etc. available.
      // `Mutex<Option<…>>`, NOT `OnceLock` (which is set-once and can't be
      // cleared): an `Execute` cancel/timeout *poisons* the instance
      // (plan1a-host's Execute-scoped poison policy). `poison_instance` clears
      // this to `None` so the next instance request re-runs `LaunchEngine`
      // (~0) and reconnects/restarts the detached daemon.
      instance: Mutex<Option<LaunchEngineResult>>,
      // Cache of claims_language(language, first_class) results.
      // Sound iff the engine's claimsLanguage is a pure function of its
      // inputs — see "Cache determinism contract" in design notes.
      claims_language_cache: Mutex<HashMap<(String, Option<String>), LanguageClaim>>,
      // Cache of claims_file(path, ext) results, scoped to one
      // project render (the `Arc<EngineRegistry>` lifetime owned by
      // `ProjectContext`; see Plan 1c Phase 2). Engines may inspect
      // file content (Julia checks for `# %%` percent-script markers);
      // without this cache, project scans re-read the same files for
      // every (file, engine) pair.
      // Cache key is the canonical path; the ext argument is derived
      // from the path so it isn't part of the key. Lifetime is one
      // project render (q2's pipeline is stateless across renders).
      claims_file_cache: Mutex<HashMap<PathBuf, bool>>,
      // Static hints from _extension.yml (see Plan 1c).
      // None: not declared by the extension author.
      // Some(empty): explicit "claims none" — silent, no dynamic call.
      // Some(non-empty): pre-filter; only consult subprocess if input matches.
      // These are the *pre-filter* form of the static-claim story (design
      // doc §3.3): a hint is a conservative superset ("might I claim this?")
      // that avoids a load when the language clearly doesn't match, but still
      // loads to get the precise claim when it does. The *complete* static
      // form is a full `claims:` declaration in _extension.yml (kind +
      // priority / fallback) that resolution reads without loading at all —
      // dynamic `claims_language` is then the back-compat escape hatch. A
      // fully-static engine (declared `name` + `file-extensions` + `claims`)
      // is loaded only to *execute*, never to resolve.
      language_hints: Option<Vec<String>>,
      file_extension_hints: Option<Vec<String>>,
  }
  ```
  `TsEngine` does NOT own the subprocess — it shares `TsEngineHost` with other
  TS engines via `Arc`. The transport `Mutex` is inside `TsEngineHost`, not
  `TsEngine`.

  **`Send + Sync` is satisfied at the type level** because `Arc<TsEngineHost>`,
  `OnceLock<LoadEngineResult>` (discovery), and `Mutex<…>` (the instance slot
  and the caches) are all `Send + Sync`. Required by the existing
  `Arc<dyn ExecutionEngine>` registry contract (`engine/registry.rs`).

  **Concurrent correctness** under the now-live rayon-per-worker parallelism of
  Pass-2 is achieved two ways, slot by slot: the `discovery` `OnceLock` leans on
  Plan 1b's idempotent harness lifecycle (a benign double-`LoadEngine` race
  resolves to one cached result), and the `instance` `Mutex<Option<…>>`
  serializes its own init under a short-held lock (cheap — `LaunchEngine` starts
  no daemon). See "Race-free init" below. Neither uses Rust-side double-checked
  locking.

- [x] **Two-step lazy lifecycle.** Four internal helpers (not on the trait —
  called from inside trait method impls):
  - `ensure_loaded(&self, c: &Cancellation) -> Result<&LoadEngineResult>` —
    ensures the shared subprocess is running (`host.ensure_started()`), then
    calls `host.load_engine(path, c)` if `discovery` is empty (plan1a-host's
    higher-level helper over the demux `request` — not a raw `send`/`recv`
    pair). Cheap (~10–50ms total). Required before any discovery method.
  - `ensure_launched(&self, c: &Cancellation) -> Result<LaunchEngineResult>` —
    calls `ensure_loaded` first, then locks `instance`; if `None`, calls
    `host.launch_engine(name, c)` **under the lock** and stores `Some(...)`.
    Returns the result **by value** (a `Copy`-cheap `{ can_freeze,
    generates_figures }` pair) — a `Mutex<Option<…>>` can't lend a `&` past its
    guard. Holding the lock across `launch_engine` is fine because it is
    **~0** (`LaunchEngine` only constructs the `ExecutionEngineInstance` object
    on the Deno side; it starts no daemon — the expensive Julia/Jupyter startup,
    5+s, happens lazily inside `execute()` on the first call). The short lock
    makes init *exclusive* (no double-launch) while keeping the slot clearable
    for `poison_instance`. Required before any instance method.
  - `poison_instance(&self)` — locks `instance` and `.take()`s it back to
    `None`. Called by `execute` when an `Execute` `request` resolves with a
    **forcible-abort error** (`Cancelled | Timeout` — *not* a plain
    `ExecutionFailed`; plan1a-host's Execute-scoped poison policy): the detached
    daemon may be mid-computation, so the next instance request must
    re-`LaunchEngine` and reconnect/restart it. `discovery` is never poisoned —
    `LoadEngine` engages no daemon, nothing to invalidate.
  - `name()` and `is_available()` are local-only — never touch the
    subprocess.
  Shutdown is on `TsEngineHost`, not per-engine, and is **explicit** —
  matching q2's existing convention (e.g., `JupyterDaemon::shutdown_all`
  at `crates/quarto-core/src/engine/jupyter/daemon.rs:272-279` is an
  explicit method, not a `Drop`). The orchestrator calls
  `registry.shutdown_all()` at end-of-render (Plan 1c owns this site)
  before `ProjectContext` drops. `registry.shutdown_all()` iterates the
  unique `Arc<TsEngineHost>` clones held by `TsEngine` instances and
  calls `host.shutdown()?` on each; errors are surfaced through the
  caller's `Result`. As a backstop against panic/unexpected drop, the
  child process spawned by `StdioTransport` is reaped by an explicit
  `Drop` impl (`std::process::Child` has no `kill_on_drop` — that is a
  `tokio::process::Command` method; see plan1a-host) so a forgotten explicit
  shutdown still kills the subprocess.

- [x] **Race-free init.** The engine→subprocess path is **fully synchronous**:
  the `ExecutionEngine` trait is a sync trait (`fn execute(&self, …) -> Result<…>`),
  `TsEngine` calls plan1a-host's **synchronous** `EngineTransport` (blocking
  stdio I/O through the host demux — `StdioTransport` over the child's
  stdin/stdout in v1; loopback TCP is the deferred Phase 1.6), and there is
  **no `async`/`await`
  or `block_on`** between the trait method and the wire. (The `PipelineStage`
  layer *above* is `?Send` async, but by the time control reaches
  `engine.execute` it is a plain blocking call.) **Pass-2 is now parallel**
  (rayon-per-worker), so concurrent callers on the same `TsEngine` are live, not
  hypothetical. The two init slots handle that differently:
  - **`discovery` (`OnceLock`)** — naive (`get()` → `host.load_engine` →
    `set()`), no `Mutex<()>` double-checked locking. Two racers both pass
    `is_none()`, both issue concurrent `LoadEngine` `request`s (distinct ids,
    both in flight), both land at `set()`; the late `set()` fails silently and
    both read the same value via `discovery.get().unwrap()`. **Benign because
    Plan 1b's harness is idempotent** for repeat `LoadEngine` (cache hit, no
    re-`import()`). Cost: one extra round-trip per *racer* — and under parallel
    Pass-2 the cold-start racers are the N rayon workers all first needing the
    same engine, so a cold `discovery` can fan out to **up to N concurrent
    `LoadEngine`s** (not merely 1–2), each a cheap harness cache-hit after the
    first. `LoadEngine` engages no daemon, so the fan-out leaks nothing; it
    settles to a single cached `LoadEngineResult`.
  - **`instance` (`Mutex<Option<…>>`)** — init is **exclusive**: lock, check
    `None`, `host.launch_engine` *under the lock*, store `Some`. The second
    racer blocks on the lock and finds `Some`, so `LaunchEngine` is issued
    **exactly once**. Holding the lock across `launch_engine` is acceptable
    precisely because it is ~0 (no daemon start) — and the slot must be a
    clearable `Mutex<Option<…>>` anyway for `poison_instance`, so exclusivity is
    free. (Harness `LaunchEngine` idempotency still holds as a backstop, but the
    lock means we don't lean on it here.)

  Why not `Mutex<()> + OnceLock<T>` double-checked locking for `discovery`? The
  closest analog in the q2 tree (`JupyterDaemon` —
  `crates/quarto-core/src/engine/jupyter/daemon.rs`) uses a naive
  `OnceLock` for the process-global daemon handle plus a per-key check in
  the daemon's session map (itself a `tokio::sync::RwLock<HashMap>`) without
  init-mutex serialization across the check-then-insert gap. Adopting
  double-checked locking here would
  introduce a pattern that doesn't exist anywhere else in the codebase.
  Idempotent lifecycle on the harness side is the right place to put
  the obligation: the harness already maintains a
  `Map<engineName, { discovery, instance? }>`, so the work is a few-line
  contract addition rather than a new Rust idiom.

  **Test (Plan 1a, Rust-side invariant only).** Race two
  `ensure_launched` calls on the same `TsEngine` (two threads,
  `Barrier` synchronizing the start) against a `MockTransport` (see
  the testing items below). Assert: the `instance` slot ends up `Some`
  with a single `LaunchEngineResult`, no panic, and — because instance
  init is exclusive under the `Mutex` — the `LaunchEngine` message count
  observed by the mock is **exactly 1**. A companion test races two
  `ensure_loaded` calls and asserts the `LoadEngine` count is **1 or 2**
  (the `discovery` `OnceLock`'s benign double-issue window — never 0,
  never > thread count). The end-to-end "engine.launch() invoked exactly
  once across the real harness" assertion is **Plan 1b's contract**, tested
  against the real harness; Rust+harness composition lives in Plan 1c's
  echo-engine integration test.

- [x] **Add `cancellation: Cancellation` AND `execute_timeout: Option<Duration>`
  to `ExecutionContext`** (the cross-plan dependency plan1a-host's "Cancellation
  wiring" and "Per-request timeouts" are blocked on). `cancellation` is the only
  way the token reaches the engine: `request`'s timeout/cancel loop polls
  `is_cancelled()`, but `execute` receives only `&ExecutionContext`, which today
  carries no token. `execute_timeout` is the resolved `Execute` window —
  **`EngineExecutionStage` reads `execute.timeout` from `doc_ast.ast.meta`**
  (via `get_path(&["execute","timeout"])`, tri-state per plan1a-host) since
  `TsEngine` cannot reach the top-level `execute:` block itself; add a
  `with_execute_timeout(Option<Duration>)` builder next to `with_cancellation`.
  `TsEngine::execute` passes `ctx.execute_timeout` as the `window`. Other engines
  (markdown/knitr/jupyter) ignore both fields — though wiring `execute_timeout`
  here means a future jupyter could honor it instead of its hardcoded
  `DEFAULT_EXECUTE_TIMEOUT` (`engine/jupyter/execute.rs:24`), out of scope now.
  - Populate it in `EngineExecutionStage` from `ctx.cancellation` where the
    `ExecutionContext` is built (`crates/quarto-core/src/stage/stages/engine_execution.rs:~310`).
  - `TsEngine::execute` passes `&ctx.cancellation` to
    `host.request(msg, window, cancellation)`. Discovery methods
    (`claims_language`/`claims_file`), which run without a full
    `ExecutionContext`, pass a default (non-cancellable) `Cancellation` — the
    10s discovery `window` still bounds them; threading a real token into
    discovery is out of scope.
  - **Constructor churn:** `ExecutionContext` is built in several places
    (`engine_execution`, `fixture`, `replay`, `preview_record`, tests). Add both
    fields with defaults (`cancellation: Cancellation::new()`,
    `execute_timeout: Some(DEFAULT_EXECUTE_TIMEOUT)`) — exposed via
    `with_cancellation` / `with_execute_timeout` builders — so this is **not** a
    breaking cascade. `Cancellation` is already cfg-portable (native + WASM —
    `stage/cancellation.rs`), so the WASM build is unaffected.
  - **Prerequisite: re-export `Cancellation` from `stage`.** Today
    `stage/mod.rs:84` declares `mod cancellation;` (**private**), and the type
    is reached only via `super::cancellation::Cancellation` *inside* `stage`
    (`stage/context.rs:27`). `engine::context` is a sibling module under
    `crate`, so naming the type there needs a `pub use cancellation::Cancellation;`
    (or `pub mod cancellation;`) added to `stage/mod.rs` — without it, adding
    the field is an unresolved-import compile error. One line, but a real
    prerequisite of this work item.
  - **Prerequisite: promote `DEFAULT_EXECUTE_TIMEOUT` to a shared home** (the
    `execute_timeout` default surfaced this when the host plan was rebased in).
    It is currently a **private** `const DEFAULT_EXECUTE_TIMEOUT` in
    `engine/jupyter/execute.rs:24` (300s), so the `ExecutionContext` default
    `Some(DEFAULT_EXECUTE_TIMEOUT)` can't reach it. Move the const to a shared
    location (e.g. `engine/mod.rs`, alongside `HANDLED_LANGUAGES`) and have
    jupyter re-reference it — making it the single source of truth the host
    plan already assumes ("a future jupyter could honor `execute_timeout`
    instead of its hardcoded `DEFAULT_EXECUTE_TIMEOUT`"). Same class of
    one-line-visibility prerequisite as the `Cancellation` re-export above.
  - **Bound test (cross-plan obligation from plan1a-host's Test Seam Spec).**
    `EngineExecutionStage` resolves `execute.timeout` from `doc_ast.ast.meta` via
    `get_path(&["execute","timeout"])` (tri-state). Add a test on the resolver:
    `{execute:{timeout:5}}` → `Some(5s)`; `{execute:{timeout:false}}` → `None`;
    absent → `Some(300s)`. **Named revert:** delete the `get_path`/`as_bool`/`as_int`
    branch (hardcode `Some(300s)`) → the `5s` and `None` cases go RED. (Vacuity:
    the three cases must map to three distinct windows, not all to the default.)

- [x] Implement `ExecutionEngine` trait — all methods that touch the
  subprocess go through `ensure_loaded` or `ensure_launched`:

  **Existing trait methods:**
  - `name()` → `self.name` (no subprocess call)
  - `is_available()` → check Deno in PATH (no subprocess call). The
    bundle is embedded via `include_str!` and always present at runtime
    — no file-existence check needed.
  - `can_freeze()` → if launched (`self.instance.lock()` holds `Some`), read its
    `can_freeze`; if `None` (never launched, or poisoned back to `None`),
    conservative `false` (no subprocess call to find out)
  - `execute(input, ctx)` → `ensure_launched`, build `TsExecuteOptions` from
    `ctx`. Translation: `doc.ast.meta` (`ConfigValue`) → flat
    `HashMap<String, TsMetadataValue>` per "ConfigValue → TsMetadataValue"
    in plan1a-protocol's appendix; q2's `Format` → `TsFormatIdentifier` (the four identifier
    fields the protocol forwards; q2's other `Format` fields stay on
    the q2 side); both packed into a single `TsFormatInfo`. `SourceInfo` →
    `Vec<TsSourceMapEntry>` per the source-map flattening rules in plan1a-protocol's appendix.
    `HANDLED_LANGUAGES` constant → `handled_languages`. Issue the `Execute`
    `request` (the long-window call), translate the response back to q2-native
    `ExecuteResult` (`html_dependencies` from `TsHtmlDependency[]`,
    `includes` from `TsPandocIncludes`, etc.). **Match the `request` result: on
    a forcible-abort error (`ExecutionError::Cancelled | ExecutionError::Timeout`)
    call `poison_instance()` before returning it; on a plain `ExecutionFailed`
    (a normal engine error) do NOT poison** — the instance is still healthy.
    plan1a-host's `request` returns these as *distinguishable* errors precisely
    so `execute` can make this call. `Execute` is the only request that engages
    the daemon, so it is the only one that poisons; `intermediate_files` starts
    no daemon and never poisons.
  - `intermediate_files(input_path)` → `ensure_launched`, send
    `IntermediateFiles`, recv result, translate `Vec<String>` → `Vec<PathBuf>`.
    **Semantics:** this is a *pure prediction of intermediate file paths
    derived from the input path* — NOT post-execution introspection of
    what `execute()` produced. The argument is the original source path;
    the return lists paths the engine will produce alongside the primary
    output (e.g. a generated `.ipynb`, `.html.md` backups). The result is
    used to **exclude those paths from the project's input-file set** so
    they are not treated as separate render targets. It stays on the
    instance tier (needs `LaunchEngine`), faithful to Q1's
    `ExecutionEngineInstance.intermediateFiles`; because `LaunchEngine`
    is cheap (it starts no daemon), this costs nothing during a project
    crawl.

  **Discovery methods (defined in Phase 3):**
  - `valid_extensions()` → **hints are the source of truth pre-load.**
    Q1's `validExtensions()` is consulted in two dispatch sites that
    Plan 1c's dispatcher needs to match: a per-file pre-gate inside
    `fileExecutionEngine` (`external-sources/quarto-cli/src/execute/engine.ts:312-318`)
    that rejects files whose extension no engine declares, and a
    project-wide aggregate (`engine.ts:140-144`) used for project file
    discovery. In Q1 these calls are sync, in-process, free; in q2
    they would force a `LoadEngine` round-trip per TS engine just to
    answer "do you handle `.qmd`?" The rule is therefore:
    - `file_extension_hints == Some(...)` → return the hints directly,
      no load. Hints are the authoritative answer pre-load.
    - `file_extension_hints == None` → fall back to
      `ensure_loaded` and return `discovery.valid_extensions`. Engines
      that want fast project-level discovery should declare hints.
    Hint-validation at load time (below) catches mismatches between
    declared hints and runtime `valid_extensions`.
  - `claims_language(language, first_class) -> LanguageClaim` → **static
    `claims:` declarations (if present) answer with no load; otherwise hints
    pre-filter the dynamic path**:
    - A full static `claims:` entry for the language → return it directly
      (no load). This is the zero-load path (design doc §3.3).
    - `language_hints == Some(empty)` → return `None` (no load — explicit
      "claims none").
    - `language_hints == Some(non-empty)` and language not in list →
      return `None` (no load — pre-filter rejection).
    - `language_hints == Some(non-empty)` and language IS in list →
      check `claims_language_cache`; on miss, `ensure_loaded`, send
      `ClaimsLanguage`, recv `ClaimsLanguageResult`, cache, return.
    - `language_hints == None` (no hints declared) → check
      `claims_language_cache`; on miss, `ensure_loaded`, send
      `ClaimsLanguage`, recv `ClaimsLanguageResult`, cache, return.
      Engines that want to avoid loading on first dispatch should declare
      hints; engines that want to avoid loading *entirely* during resolution
      should declare full static `claims:`.
    The harness normalizes the engine's JS return into the wire claim — **no
    sign games**: `false`/`null` → `None`, `true` → `Primary(1)`, `number n`
    → `Primary(n)` (negative = low-priority primary, never interop), and the
    object form maps to `Primary`/`Interop`/`Fallback` directly (design doc
    §3.2). `Interop` and `Fallback` are reachable only via the object.

    **Wire → resolution conversion (mind the shape gap).** The protocol
    `TsLanguageClaim` (`ts_protocol.rs:138-144`) has only **three** variants
    in struct form — `Primary{priority} | Interop{priority} | Fallback{priority}`
    — and **no `None`**: "no claim" is modeled by the *absence* of a claim. So
    `ClaimsLanguageResult` carries an **`Option<TsLanguageClaim>`** (`None` ⇒
    no claim), and `TsEngine` maps it to the resolution-layer `LanguageClaim`
    (tuple form, with a `None` variant) via a `From<Option<TsLanguageClaim>>
    for LanguageClaim` conversion living in `ts_engine.rs` at the
    protocol→native boundary (next to the `Format`/`SourceInfo` translations):
    `None ⇒ LanguageClaim::None`, `Some(Primary{p}) ⇒ Primary(p)`, etc. This
    is the only seam where the two near-identically-named types meet; keep the
    protocol DTO inside `ts_engine.rs` and hand resolution the native enum.
  - `claims_file(file, ext)` → **hints are the source of truth pre-load**:
    - `file_extension_hints == Some(empty)` → return `false` (no load —
      explicit "claims none").
    - `file_extension_hints == Some(non-empty)` and ext not in list →
      return `false` (no load — pre-filter rejection).
    - `file_extension_hints == Some(non-empty)` and ext IS in list →
      check `claims_file_cache` keyed on the canonical path; on hit,
      return cached. On miss: `ensure_loaded`, send `ClaimsFile`,
      recv `ClaimsFileResult`, cache, return.
    - `file_extension_hints == None` (no hints declared) → check
      `claims_file_cache`; on miss: `ensure_loaded`, send `ClaimsFile`,
      recv `ClaimsFileResult`, cache, return. Engines that want to
      avoid loading on first dispatch should declare hints.

    The cache is scoped to one project render — same lifetime as
    the `Arc<EngineRegistry>` owned by `ProjectContext` (Plan 1c
    Phase 2) and the `Arc<TsEngineHost>` clones it owns. A project
    scan that consults N engines for M files would otherwise pay
    N×M file reads (engines like Julia inspect content for
    percent-script markers); the cache reduces that to N×M results
    but ≤M file reads per engine. The cache assumes the file's
    content does not change *during* the render — a reasonable
    invariant since q2 reads inputs once at pipeline entry. q2's
    render pipeline is currently stateless across renders (each
    render builds a fresh `ProjectContext`), so cross-render cache
    staleness is not a concern; if a future architecture reuses
    `ProjectContext` across renders (e.g., a long-running preview
    server), that plan revisits cache invalidation. Engine-content-aware
    caching across renders is otherwise out of scope.

  **Concurrent claims caching (parallel Pass-2).** Resolution runs per-document
  in Pass 2, and Pass 2 is parallel, so multiple rayon workers call
  `claims_language` / `claims_file` on the *same* shared `TsEngine`
  concurrently. The `Mutex<HashMap>` caches keep this memory-safe, and the
  failure mode mirrors the `discovery` race: two workers can both miss the same
  key and both issue the dynamic `ClaimsLanguage` / `ClaimsFile` query (the
  cache does **not** dedup *in-flight* misses) — benign, because the determinism
  contract below makes both answers identical and the second `set` is an
  idempotent overwrite. The lock is held only for the map read/write, never
  across the round-trip. Worst case on a cold key under N-way contention: up to
  N redundant (idempotent) queries, settling to one cached value.

  **Hint validation at load time:** when `LoadEngine` returns
  `discovery.valid_extensions`, validate that any non-empty
  `file_extension_hints` superset-contains it. If the engine claims
  extensions outside the declared hints, emit a `DiagnosticMessage::warning`
  naming the extensions and the engine — the static-hint pre-filter
  would silently miss those claims. Same channel for the missing-hints
  cost note (TS engines without `file_extension_hints` trigger
  `LoadEngine` on every `.qmd` render). All Plan-1a-time engine
  warnings/errors flow through `DiagnosticMessage` (q2's standard
  user-facing diagnostic channel — `DiagnosticMessage::warning` at
  `crates/quarto-error-reporting/src/diagnostic.rs:247`),
  not `tracing::warn!`. The registry holds
  `pub diagnostics: Mutex<Vec<DiagnosticMessage>>` (see registry-state
  block below); `ProjectContext` drains it at end of init and forwards
  to the pipeline observer. `tracing::warn!` is reserved for operator
  logging. **Push diagnostics idempotently:** hint-validation runs inside
  `LoadEngine`, so the benign `discovery` double-issue (parallel Pass-2) can run
  it twice for one engine and push the *same* warning twice. Either guard the
  push so it fires only on the `OnceLock`-winning load, or de-dup the drained
  vec by `(engine, message)` before forwarding — otherwise users see duplicated
  warnings under load.

  **`EngineRegistry` struct definition.** plan1a-engine owns the
  registry's struct shape because plan1a-engine is what mutates the
  fields (alias insertion under `LoadEngine`, diagnostic pushes during
  hint validation). Plan 1c constructs an instance with
  `EngineRegistry::new()` and populates it; this is the canonical
  definition.
  ```rust
  pub struct EngineRegistry {
      engines:     HashMap<String, Arc<dyn ExecutionEngine>>,        // immutable post-construction
      aliases:     Mutex<HashMap<String, ExtensionId>>,              // runtime_name → extension_id, lazily populated
      diagnostics: Mutex<Vec<DiagnosticMessage>>,                    // hint-validation, missing-hints, name-collision warnings
  }
  ```
  Both mutexes are independent of `TsEngineHost`'s transport mutex.
  Following the `JupyterDaemon` pattern (`crates/quarto-core/src/engine/jupyter/daemon.rs`)
  — separate locks for separate concerns; no cross-locking.

  **Migration from `main`'s registry (`#[derive(Clone)]` blocker — the
  Clone-drop is NOT self-contained; decided 2026-06-24).** On `main`,
  `EngineRegistry` is `{ engines: HashMap<String, Arc<dyn ExecutionEngine>> }`
  and derives `Clone`. `Mutex` is **not** `Clone`, so adding the `aliases` /
  `diagnostics` fields **requires dropping `#[derive(Clone)]`** — and dropping
  the derive **breaks the build at 8 real sites across 4 files** that clone an
  `Option<EngineRegistry>`:
  - **6 explicit `Option<EngineRegistry>::clone()` calls:**
    `quarto-core/src/pipeline.rs:847`, `quarto-preview/src/lib.rs:200`, `:206`,
    `:244`, `quarto-preview/src/capture_driver.rs:109`, and
    `quarto-core/src/render_to_file.rs:328`.
  - **2 transitive `#[derive(Clone)]` structs holding `Option<EngineRegistry>`
    by value:** `PreviewConfig` (`quarto-preview/src/lib.rs:65`) and
    `RenderToFileOptions` (`quarto-core/src/render_to_file.rs:83`).

  (The previously-named sites — `EngineExecutionStage::with_registry`
  (`engine_execution.rs:113` — a *stage* method, not on the registry), the
  per-document construction in `EngineExecutionStage::new`, and
  `with_replay_many` — do **not** break *from the `Clone`-drop*: they *move* the
  registry in or *build it fresh*, so they never relied on `Clone`. Note
  `with_registry`'s *signature* still changes as part of the `Arc`-type
  propagation below — "doesn't break from `Clone`" and "signature changes for
  `Arc`" are both true and not in tension.)

  **Decision: pull the minimal `Arc`-wrap into Plan 1a** so the registry
  change stays independently compilable (the project's green-build-per-plan
  discipline). Plan 1a reroutes those clone sites to `Option<Arc<EngineRegistry>>`
  (a cheap `Arc` clone replaces the deep clone) as part of this work item —
  this is mandatory-to-compile, not optional cleanup. Plan 1c then does the
  deeper `ProjectContext`-owned ownership (built once, shared across Pass 1 +
  Pass 2), building on the `Arc` Plan 1a introduces.

  **Scope of the type change — ~25–30 mechanical sites, zero semantic change
  (verified 2026-06-24).** The 8 clone sites are where the build *first*
  breaks, but the `Option<EngineRegistry>` → `Option<Arc<EngineRegistry>>`
  type then propagates transitively through a closed, mechanical set:
  - **`EngineExecutionStage`** — field `registry: EngineRegistry` →
    `Arc<EngineRegistry>`, plus `with_registry(...)` signature and the
    `new()` body (`Arc::new(EngineRegistry::new())`). All reads are through
    `&self` methods (`get`/`default_engine`), and `Arc` derefs transparently,
    so **no consume site changes behavior**.
  - **A third config struct the clone-site list omitted: `HtmlRenderConfig`**
    (`pipeline.rs:118`) and its `with_engine_registry` builder
    (`pipeline.rs:131`). The `render_to_file.rs:328` clone feeds
    `config.engine_registry`, so the `Arc` reaches into `quarto-core`'s
    `HtmlRenderConfig` — **name it explicitly so the count isn't a surprise.**
  - **The `quarto-preview` pass-through chain** (~9 fn signatures in
    `re_execute.rs` / `capture_driver.rs` / `cache.rs` that carry the
    registry as an opaque `Option<…>` param) — pure signature type swaps, no
    body logic.
  - **One real construction site**: `render_to_file.rs:331`
    (`Arc::new(EngineRegistry::with_replay_many(...))`); plus test fixtures
    that build a registry and pass it in (`Arc::new(...)` at the test
    boundary).

  **Nothing mutates the registry post-construction** (no `&mut`/`.register()`
  after it enters a config or stage — confirmed), so `Arc`-wrapping is sound
  and matches the new "engines immutable post-construction" design. Note too
  that **in production the override is `None` everywhere** — the real default
  registry is built inside `EngineExecutionStage::new()`; the only `Some` that
  flows through config in production is the replay path. So the wrap is mostly
  test-fixture + replay churn, all trivial type substitution.

  **`with_replay_many` stays in Plan 1a, untouched.** It builds a fresh
  registry and does not depend on `Clone`, so the Clone-drop does not affect
  it. Its removal is purely the §6.2 capture-driven-replay rework — replay
  drives from recorded `engine_captures` instead of injecting `ReplayEngine`s
  into the (now immutable) registry — which is **Plan 1c's** architectural
  change, not a consequence of Plan 1a's registry edit. See
  `claude-notes/designs/engine-resolution.md` §6.2.

  **Name validation at load time:** when `name_declared` is true,
  assert `LoadEngineResult.name == self.name`. Mismatch is a hard
  error pointing at the YAML: `Engine extension declares 'name: {self.name}'
  in _extension.yml but the loaded module reports 'name: {actual}'.
  Update _extension.yml or the engine module's name property.`
  When `name_declared` is false, the registry's `aliases` map is
  updated with `LoadEngineResult.name → self.name` so subsequent
  lookups by runtime name resolve to this engine. **Insertion is
  transactional**: a single `aliases.lock()` covers both the
  collision check and the insert, so two concurrent `LoadEngine`
  round-trips for *different* extensions returning the same runtime
  name produce a deterministic hard error on whichever ran second.
  **The collision check must be identity-aware**, because the benign
  `discovery` double-issue (parallel Pass-2, above) can run *this same
  engine's* alias insert twice concurrently: check `aliases.get(name)` and
  treat `Some(existing)` as a hard collision **only if `existing` is a
  *different* extension id** — a re-insert of the same `runtime_name → same
  ExtensionId` is an idempotent no-op, not a self-collision. Keying on the
  stable `ExtensionId` (`Eq + Hash`) rather than mere name-presence is what
  makes the same-engine race a no-op while a genuine two-extension clash is
  still a hard error.

  **Name-collision policy: hard error.** Any of the following is a
  hard error that fails the render with a clear message naming both
  conflicting engines:

  1. Two extensions declare the same `name` in their `_extension.yml`.
     The error fires at registry construction time (Plan 1c Phase 2),
     before Pass 1 begins.
  2. Two lazy-loaded engines (no declared name) self-report the same
     runtime `name` from `LoadEngine`. The error fires under the
     `aliases.lock()` when the second engine's `LoadEngineResult.name`
     would overwrite the first entry.
  3. A lazy-loaded engine self-reports a name that collides with a
     built-in (`markdown`, `knitr`, `jupyter`) or another already-known
     declared engine. Same error.

  The relaxed-collision case (e.g., last-writer-wins, or namespacing by
  extension id) is deferred — we can revisit if a real use case
  surfaces. The hard-error stance keeps the registry deterministic and
  the YAML-vs-runtime contract simple.

  **Cache determinism contract:** the `claims_language` cache assumes the
  engine's `claimsLanguage` is a pure function of `(language, first_class)`.
  q2 doesn't enforce this; engine authors who introduce non-determinism
  (reading mutable state, side effects) will see stale cache hits. The
  contract is documented in
  [Plan 1c](2026-04-16-plan1c-extension-integration.md) — the
  extension-author-facing surface lives there alongside the
  `_extension.yml` schema, hint declarations, and engine-API docs.
  Same rule applies to `claims_file`'s content-inspection (cache key is
  the canonical path; if the engine reads mutable file metadata the
  cache will go stale within a single render, which is expected to be
  rare in practice).

  **Cache writes only on success.** When the engine throws during
  `claimsLanguage` (subprocess sends `FromEngine::Error`), the Rust side
  propagates `ExecutionError` to the caller without touching the cache.
  Per plan1a-host's "Error categories" item 4, discovery errors are
  terminal — the render fails and the host is torn down — so no second
  query ever happens against the same cache slot. The cache value type
  stays `LanguageClaim` (success states only: `None` for "no claim", or
  `Primary`/`Interop`/`Fallback` for a real claim); there is no need for a
  `Result<_, _>` slot to encode "engine errored." Same rule applies
  trivially to `claims_file_cache`: errors propagate, render fails, no
  cache write.

  **File conversion (defined in Phase 3):**
  - `markdown_for_file(file, runtime)` → `ensure_launched`, send
    `MarkdownForFile`, recv `MarkdownForFileResult`. Return
    `(result.value, SourceInfo::default())` — the converted text plus the
    reserved (v1-default) provenance slot. (`runtime` is unused — the
    subprocess reads files via Deno; `result.source_map` is carried on the
    wire but **not consumed** in v1.) Provenance for the converted text is
    invented downstream when the convert-then-parse path registers it as an
    ephemeral intermediate file under an engine-reflecting synthetic name —
    see the **Provenance** note in Phase 3 (scope C′; A′/B′ deferred). Called
    only for non-QMD files claimed via `claims_file`.

  Note: `run()` is excluded from the protocol — it's fundamentally different
  (long-running interactive mode, not request/response). Deferred to a future
  plan. `partitioned_markdown` is excluded too (see Phase 3 rationale and
  the ipynb-filters research plan).

- [x] **Drop `#[derive(Clone)]` on `EngineRegistry` and reroute
    `Option<EngineRegistry>` → `Option<Arc<EngineRegistry>>`** (mandatory-to-compile
    once the `aliases` / `diagnostics` `Mutex` fields land — see the migration
    note for the full rationale and verified site list). The build first breaks
    at the 8 clone sites; the type then propagates through **~25–30 mechanical
    sites total, all trivial type substitutions with zero semantic change**:
    the `EngineExecutionStage` field + `with_registry` + `new()`; the three
    config structs `PreviewConfig` / `RenderToFileOptions` / **`HtmlRenderConfig`**
    (`pipeline.rs:118`) and `with_engine_registry` (`pipeline.rs:131`); the
    `quarto-preview` pass-through signature chain; `Arc::new(...)` at the one
    real construction site (`render_to_file.rs:331`) + test fixtures. No
    post-construction mutation exists, so the wrap is sound. This keeps Plan
    1a independently compilable; Plan 1c does the deeper `ProjectContext`
    ownership. `with_replay_many` is untouched.

- [x] Wire into engine module (`engine/mod.rs`): add the `ts_engine` module
    (native-gated, same gate as knitr/jupyter) and the **un-gated**
    `resolution` module (see Phase 3.5 — it must compile for WASM). Re-export
    `TsEngine` from `engine/mod.rs`. `ts_protocol` is already wired
    (plan1a-protocol, done); `ts_process` and the `TsEngineHost` re-export are
    added by **plan1a-host**.
    **The new *shared* types stay un-gated and must be WASM-clean:**
    `LanguageClaim`, `EngineResolution` / `resolve_engines`, the new
    `ExecutionContext` leave-alone field, and `ExecuteResult.html_dependencies`
    all live in `quarto-core` and feed `wasm-quarto-hub-client`. They are pure
    data + a pure function, so they compile for WASM; in a WASM build the
    registry is markdown-only (knitr/jupyter native-gated) and execution is
    bypassed by `CaptureSpliceStage`, so resolution is inert there but must
    still compile and degrade gracefully (no engine → markdown passthrough, no
    panic). Gate the rebase commits with full `cargo xtask verify` (not
    `--skip-hub-build`) — see design doc §13.
- [x] Transport access is **multiplexed by the `TsEngineHost` demux**, not
    serialized by a single transport `Mutex`. Under parallel Pass-2 many
    blocking rayon workers call the host concurrently; each `host.request`
    allocates an `id`, registers a pending slot, and blocks on *its own* slot
    while one reader thread routes `Response`s by `id` (plan1a-host). The
    transport's write half is briefly mutexed (one framed write); **no lock is
    held across the round-trip**, so cross-engine requests run concurrently (the
    Deno event loop interleaves them) and same-engine requests are serialized on
    the *harness* side (per-instance queue). The `claims_language_cache` /
    `claims_file_cache` mutexes are separate from the transport; under parallel
    Pass-2 they *can* be briefly contended (workers resolve concurrently — see
    "Concurrent claims caching"), but the contention is short and benign. **The
    Rust transport is synchronous**
    (plan1a-host's `EngineTransport` is a blocking duplex — `StdioTransport`
    over the child's stdin/stdout in v1, newline-framed JSON; **stdout is the
    protocol channel**, **stderr** is diagnostic-only, drained on a separate
    host-side reader thread. The deferred Phase 1.6 `TcpTransport` (loopback TCP)
    moves the protocol off stdout to delete the `console.log` footgun).
    `TsEngine` calls it from the sync `ExecutionEngine` methods — no runtime, no
    `block_on`, no async bridge. The concurrency is real but lives on the Deno
    event loop, surfaced to blocking workers via the demux —
    `claude-notes/designs/engine-host-concurrency.md`. (The earlier "single
    transport `Mutex` / lockstep request-response / async buys no concurrency"
    framing predated parallel Pass-2 and is **retired**.)
- [x] **Transport seam ownership (host vs. engine).** The transport
    abstraction is **plan1a-host's**: it defines the `EngineTransport` trait,
    `StdioTransport` (the v1 impl; loopback `TcpTransport` is the deferred
    Phase 1.6), the `TsEngineHost` demux (all in `ts_process.rs`), and owns
    the `StdioTransport` `deno`-gated smoke test. **`MockTransport` is also
    a plan1a-host deliverable** (reassigned during the 2026-06-24 review — host's
    own timeout/cancel/crash tests need it, and it lives in host's
    `ts_process.rs`). plan1a-engine *consumes* both. The test seam:
    - **plan1a-host** adds the test-only constructor
      `TsEngineHost::with_transport(Box<dyn EngineTransport>, EngineHostContext)`
      — which **starts the real reader thread** so tests run the production demux
      path (not a synchronous shortcut).
    - **plan1a-host** owns `MockTransport` — a test-only impl of the
      `EngineTransport` trait (under `#[cfg(test)]` in `ts_process.rs`). It is
      **id-keyed, delay-capable, and BLOCKS in `recv()`** until a paired/scripted
      response is available (a passive `VecDeque` would read empty-as-EOF and
      false-trigger the crash path); `shutdown()` signals EOF. It captures sent
      messages (`sent_messages() -> &[ToEngine]`) and echoes each `Request`'s
      `id`. See plan1a-host's Design Note "MockTransport & the test demux" for
      the full shape and rationale.
    All Phase 4 unit tests construct a
    `TsEngineHost::with_transport(Box::new(MockTransport::…), …)`; no Deno is
    required to run plan1a-engine's test suite.

    End-to-end coverage of the real Plan 1b bundle lives in
    Plan 1c's echo-engine integration test (Plan 1c Phase 3); the
    harness-side idempotency contract is tested in Plan 1b directly
    against Plan 1b's harness. Plan 1a does not own subprocess-level
    fidelity tests.
- [x] Write `MockTransport` round-trip test: build a `TsEngineHost`
    via `with_transport`; send a `LoadEngine` followed by a
    `ClaimsLanguage`; assert the response round-trips and that the
    captured `sent_messages()` contains the expected sequence.
- [x] Write state-machine test: a `TsEngine` backed by a
    `MockTransport` answers discovery without triggering
    `LaunchEngine`; calling `execute` triggers `LaunchEngine` exactly
    once on the wire (one `ToEngine::LaunchEngine` in the captured
    log).
- [x] Write race-free-init tests (two, one per slot):
    - **instance (exclusive):** two threads concurrently call
      `ensure_launched()` on the same `TsEngine` (synchronized via
      `std::sync::Barrier`) against a `MockTransport` pre-seeded with two
      `Launched` results. Assert: the `instance` slot converges to a single
      value, no panic, captured `LaunchEngine` count is **exactly 1** (the
      `Mutex<Option<…>>` serializes init).
    - **discovery (benign double-issue):** two threads concurrently call
      `ensure_loaded()` against a `MockTransport` seeded with **two distinct**
      `LoadEngineResult`s; the **binding** assertion is that **both threads
      observe the same cached value** (the `OnceLock` converges) — see Test
      Seam Spec row 7. The `LoadEngine` count `1 ≤ n ≤ 2` is kept only as a
      shape bound, **not** the discriminator (an over-locked impl also passes
      "1 or 2", so count alone is vacuous).
    These test the Rust-side invariants Plan 1a owns; the "engine.launch()
    invoked exactly once across the real harness" assertion lives in Plan 1b.
- [x] Write poison test: a `TsEngine` whose `MockTransport` returns a
    cancel/timeout for an `Execute` request; assert `execute` calls
    `poison_instance` (the `instance` slot is `None` afterward) and that a
    subsequent `execute` re-issues `LaunchEngine` on the wire.

## Test Seam Spec (frozen — prevalidated 2026-06-24)

**Freeze this before writing any test.** Each row names the **one production
hunk** whose revert turns the **named assertion** RED; once a test goes green
its assertions + harness are frozen (never edited to go green). All rows are
`cargo nextest`, native, **no Deno** (subprocess fidelity is Plan 1b; the real
harness composition is Plan 1c's echo E2E). The bulleted test items above are
the prose; this table binds them. Tiers: **claim** (pure trait method),
**resolver** (pure `resolve_engines`), **engine** (`TsEngine` over a
`MockTransport`-backed `TsEngineHost`), **registry**, **dep**.

| # | Test | Tier | Real unit (not mocked) | Mock boundary | Named revert → RED assertion (+ vacuity guard) |
|---|------|------|------------------------|---------------|------------------------------------------------|
| 1 | Built-in claim tables | claim | `{Knitr,Jupyter,Markdown}Engine::claims_language` | — (pure) | Revert jupyter's blanket `Fallback(0)` to Q1's `"julia"→Primary(1)` → assert `jupyter.claims_language("julia",_) == Fallback(0)` RED. Revert knitr's `Interop` arm for `sql` → assert `knitr.claims_language("sql",_) == Interop(_)` RED. **Vacuity:** assert the exact *kind+payload* (`Fallback(0)`/`Interop(_)`/`Primary(1)`/`None`), never just "non-`None`" — kind dominates, so "some claim" hides the regression. |
| 2 | Trait default `markdown_for_file` | claim | the trait default body | — | Revert the default from `Err(not_supported("markdown_for_file"))` to `Ok((String::new(), default))` → assert `matches!(MarkdownEngine.markdown_for_file(…), Err(ExecutionError::NotSupported("markdown_for_file")))` RED. **Vacuity:** match the `NotSupported` variant **and** its `&'static str` payload, not "is `Err`" (an `Io` err also matches `Err`). |
| 3 | Resolver tiers (§4.4) | resolver | `resolve_engines` + the four tiers | `MockEngine` claim tables (no AST exec) | One revert per rule, each reddening a distinct case: (a) revert **kind-dominates** (sort by priority ignoring kind) → `Primary(-100)` vs `Fallback(0)` case: assert owner==weak-engine RED. (b) revert **Interop presence-gating** (fire Interop unconditionally) → pure `{python}`: assert `sequence==[jupyter]` (not `[knitr]`) RED. (c) revert **T2>T3** (Interop above explicit-Fallback) → `[knitr,jupyter]`+`{sql}`: assert `ownership["sql"]=="jupyter"` RED. (d) revert **T4 implicit-only gate** → explicit `[knitr]`+`{julia}`: assert jupyter **not** added RED. **Vacuity:** assert the `ownership`/`sequence`, and keep the `{r}+{sql}→knitr` vs `[knitr,jupyter]+{sql}→jupyter` pair — they must resolve to *different* owners or presence-gating is untested. |
| 4 | §10 case-4 loud failure | engine | the owner's "owns language, no handler" guard | kernelspec lookup → none | Revert the guard (let the owned-but-unrunnable cell run/skip) → assert jupyter handed `{sql}` it owns returns `Err(NoHandlerForLanguage{engine:"jupyter",language:"sql"})` RED. **Vacuity + path-exercised:** resolution must actually give `sql` to jupyter (assert `ownership["sql"]=="jupyter"` in setup) else it passes vacuously; assert the error **names the language**, and assert **no** unexecuted-cell output (silent no-op would otherwise pass). |
| 5 | Two-step lifecycle | engine | `ensure_loaded` vs `ensure_launched` split | `MockTransport` `sent_messages()` | Revert `claims_language` to call `ensure_launched` (not `ensure_loaded`) → after a discovery-only sequence assert `sent_messages()` has **zero** `LaunchEngine` RED; after one `execute`, **≥1** `LaunchEngine`. **Vacuity:** the discovery call must be one that *could* have launched (a real `ClaimsLanguage` that hits the wire), and assert the **count**, not presence of `LoadEngine`. |
| 6 | Race-free **instance** (exclusive) | engine | `ensure_launched` `Mutex<Option<…>>` init | `MockTransport` + `std::sync::Barrier`, 2 threads | Revert the `Mutex<Option>`-under-lock init to naive get/launch/set → assert captured `LaunchEngine` count **== exactly 1** RED. **Vacuity:** `Barrier` aligns both threads into the race window; assert **==1**, never "≤2". |
| 7 | Race-free **discovery** (convergence) | engine | `discovery` `OnceLock` | `MockTransport` seeded with **two distinct** `LoadEngineResult`s, `Barrier`, 2 threads | Revert the `OnceLock` caching (re-load per call) → assert **both threads observe the *same* `LoadEngineResult`** (convergence) RED. **Vacuity (fixes the weak "1 or 2" count):** the count assertion `1≤n≤2` is *non-discriminating* (an over-locked impl also passes it) — keep it only as a bound; the **binding** assertion is value-convergence, which requires the two seeded responses to **differ**. |
| 8 | Poison policy (only forcible aborts) | engine | `execute` poison-on-`{Cancelled,Timeout}` + `poison_instance` | `MockTransport` returns `Timeout` for one Execute, `ExecutionFailed` for another | Revert the match to `poison_instance()` on **any** error (or drop it on the abort branch) → assert: after a `Timeout` execute, `instance==None` **and** next execute re-issues `LaunchEngine`; **and** after an `ExecutionFailed` execute, `instance` stays `Some` (no relaunch) RED. **Vacuity:** the `ExecutionFailed`-doesn't-poison half is the discriminator — without it, "poison on any error" passes. *(Same non-poison expectation holds for `NoHandlerForLanguage` — a clean refusal; if test 4 produces it through `execute`, assert `instance` stays `Some`.)* |
| 9 | Registry name-collision (identity-aware) | registry | `aliases.lock()` transactional + identity-aware check | two `LoadEngineResult`s, same runtime name | Revert identity-awareness (key on name presence, not `ExtensionId`) → assert: two **different** `ExtensionId`s reporting name `"foo"` → hard `Err` (collision); **same** `ExtensionId` re-inserting `"foo"` → `Ok` (idempotent no-op) RED. **Vacuity:** both cases asserted; same-name-different-id vs same-name-same-id is what identity-awareness discriminates. |
| 10 | Hint-validation warning | registry | load-time `file_extension_hints ⊇ valid_extensions` check | `TsEngine` whose hints omit an ext `LoadEngine` reports | Revert the validation+push → assert `registry.diagnostics` contains a `warning` naming **the engine and the missing extension** RED. **Vacuity:** assert the diagnostic *content* (both names), not "diagnostics non-empty." |
| 11 | Hint pre-filter (no load) | engine | `claims_language` `language_hints` pre-filter | `MockTransport` load-counter; `language_hints=Some(["python"])` | Revert the pre-filter short-circuit → assert `claims_language("ruby",_)==None` **and** `sent_messages()` has **zero** `LoadEngine` RED. **Vacuity + path-exercised:** the no-load (zero `LoadEngine`) is the binding assertion (the trait default also returns `None` but *would* load); `"ruby"` must be outside the hint list so the pre-filter actually fires. |
| 12 | Claims cache (success cached) | engine | `claims_language_cache` write-on-success | `MockTransport` answers one `ClaimsLanguage` | Revert the cache write → assert two `claims_language(same key)` calls issue **exactly one** `ClaimsLanguage` on the wire RED. **Vacuity:** assert the **wire count** (1 query / 2 calls), not the returned claim (which is equal either way). |
| 13 | `store_html_dependencies` name dedup | dep | the first-wins name-collision guard + warning | — (two `HtmlDependency`, same `name`, different content) | Revert the name guard → assert the **first** content survives (second dropped) **and** exactly one `DiagnosticMessage::warning` naming **both** registrants RED. **Vacuity:** same `name` + *different* content; assert **which** content won (first), not "one survived." |
| 14 | `HtmlDependency` serde round-trip | dep | the new `Serialize`/`Deserialize` derives | — | Revert the derives (or remove `ExecuteResult.html_dependencies`) → assert an `ExecuteResult` carrying a non-empty `html_dependencies` round-trips through `serde_json` (the `EngineCapture` path) with deps intact RED. **Vacuity:** the input `html_dependencies` must be **non-empty** and asserted equal post-round-trip, not just "`ExecuteResult` serializes." |

(The `execute.timeout` tri-state bound test is already frozen in the
"Constructor churn" item above — `{timeout:5}`→`Some(5s)`, `{timeout:false}`→`None`,
absent→`Some(300s)`; revert the `get_path`/`as_bool`/`as_int` branch → RED.)

**Missing-test pass (reasoned across the change, not just the listed items):**
- **WASM no-engine → markdown passthrough** — *accepted-untested* with rationale:
  `MockTransport` is native-only (`#[cfg(not(wasm))]`), so the WASM degrade path
  isn't exercised; mitigated by a native `resolve_engines` test against a
  markdown-only registry (empty `sequence` → passthrough), identical behavior
  (see the WASM coverage caveat in Success Criteria). The cross-target
  *compilation* is covered by `cargo xtask verify`.
- **`MockTransport` round-trip smoke** (the bulleted item above) is a *plumbing*
  check that `with_transport` + the demux carry a `LoadEngine`→`ClaimsLanguage`
  sequence; its discriminating reverts live in plan1a-host's Test Seam Spec
  (rows 3/16) — kept here only as a wiring smoke, not double-counted as an
  engine-behavior guard.
- **`name()` / `is_available()` locality** (never touch the subprocess):
  *accepted-untested* — trivial getters; a revert (making them call
  `ensure_loaded`) is caught by test 5's zero-`LaunchEngine`/zero-`LoadEngine`
  discovery assertions if those methods were on the discovery path, but they
  aren't, so no dedicated row.

## Success Criteria

- [x] `MockTransport`-based round-trip works: `TsEngineHost::with_transport`
  test constructor lets unit tests exercise `LoadEngine`/`LaunchEngine`/
  discovery/execute/intermediateFiles without spawning Deno
- [ ] End-to-end against the real Plan 1b bundle is exercised in
  Plan 1c's echo-engine integration test, not here
- [x] Two-step lazy lifecycle: `LoadEngine` runs without `LaunchEngine`;
  discovery methods don't trigger launch
- [x] `TsEngine` implements `ExecutionEngine` (discovery, file conversion,
  execute, intermediate_files) using only q2-native types on the trait
  surface; protocol types stay inside `ts_engine.rs`
- [x] `LanguageClaim` enum (`Primary`/`Interop`/`Fallback`/`None`) added;
  `claims_language` returns it (not `Option<i32>`); kind sets tier, priority
  orders within a kind, `Interop` is presence-gated
- [x] Built-in claim tables per design doc §4.3: knitr `Primary(1)` for "r" +
  `Interop` for `["python", "sql", "bash", "sh"]` (knitr `knit_engines`
  capability); jupyter `Fallback(0)` for everything
  (losing to a `Primary(1)` julia claim); markdown `None`. No
  `partitioned_markdown` method exists on the trait or in the protocol
- [x] `resolve_engines` / `EngineResolution` in `engine/resolution.rs` is a
  pure function, unit-tested in isolation with mock claim tables (the §4.4
  worked cases); it enumerates the document's computational languages from the
  AST (executable `{lang}` cells minus `HANDLED_LANGUAGES` and raw `{=fmt}`,
  with `first_class` — §4.1/§4.2) via a helper built on `engine_cell_lang`;
  `EngineExecutionStage` stashes the result on `StageContext`
- [x] jupyter honors `handled_languages` at execute time (cedes cells it
  doesn't own) when non-terminal in a sequence
- [x] An owner that is handed a language it cannot execute fails **loudly**
  (clear `ExecutionError` naming engine + language), never silently — design
  doc §10 case 4; verified for the `[knitr, jupyter]` + `{sql}` routing
- [x] `ExecuteResult.html_dependencies: Vec<HtmlDependency>` is populated
  by `TsEngine::execute` from harness-emitted structured deps; the
  Q1-shaped `dependencies()` resolution path populates `ExecuteResult.includes`
  (the channels are disjoint per plan1a-protocol's "Two disjoint dep channels")
- [x] `store_html_dependencies` dedupes by `name` (first-wins) and emits a
  `DiagnosticMessage::warning` on duplicate registration — the name-collision
  guard, distinct from and additional to the existing `ArtifactScope::Project`
  cross-page dedup (both documented in the function's doc-comment)
- [x] `HtmlDependency`/`TextInclude`/`IncludeLocation` **kept in `pampa::lua`**
  (the original "relocate to `quarto-core`" was impossible — it would cycle
  `pampa`→`quarto-core`; amended 2026-06-24) and given
  `Serialize`/`Deserialize`/`PartialEq`/`Eq` derives **in place** (so
  `html_dependencies` survives the `EngineCapture` round-trip); referenced from
  `quarto-core` via the existing `pampa::lua` re-export
- [x] `HANDLED_LANGUAGES` constant introduced as the cell-handler contribution
  to each engine's leave-alone set; each engine's `handled_languages` is the
  ownership projection `HANDLED_LANGUAGES ∪ {other engines' owned languages}`,
  not the bare constant; knitr's hardcoded list and `TsExecuteOptions.handled_languages`
  both read the projection
- [x] `ExecutionError::NotSupported(&'static str)` and
  `ExecutionError::Timeout { engine, operation }` variants added; `execute`
  poisons the instance only on `Cancelled | Timeout`, never on `ExecutionFailed`
- [x] `EngineRegistry` carries `aliases: Arc<Mutex<HashMap<String, ExtensionId>>>`
  and `diagnostics: Arc<Mutex<Vec<DiagnosticMessage>>>`; alias insertion is
  transactional **and identity-aware** under a single `lock()`. The
  `Arc<Mutex<…>>` field type enables leaf-Arc sharing (TsEngine holds Arc clones
  of the sinks, avoiding a `registry`→`engine`→`registry` cycle).
  `#[derive(Clone)]` **was dropped** (Mutex isn't Clone) and
  `Option<EngineRegistry>` → `Option<Arc<EngineRegistry>>` **was rerouted across
  ~28 sites in Plan 1a** (F1); the deeper `ProjectContext`-owned ownership stays
  Plan 1c. `with_replay_many` is left intact (Plan 1c removes it).
- [x] New shared types (`LanguageClaim`, `EngineResolution`, the
  `ExecutionContext` leave-alone field, `html_dependencies`) compile for
  WASM; full `cargo xtask verify` is green. **Coverage caveat:** all Phase-4
  unit tests are native-only by construction (`MockTransport` lives under
  `#[cfg(not(wasm))]` in `ts_process.rs`), so `verify` proves the un-gated
  resolver *compiles* for WASM but does **not** test the WASM "no engine →
  markdown passthrough" degrade path. If that path needs a guard, it wants a
  native test of `resolve_engines` against a markdown-only registry (asserting
  empty sequence → passthrough), since the behavior is identical and the gap
  is only test-location. (Done 2026-06-24: full `cargo xtask verify` passed on this branch — all 14 steps incl. WASM + hub-client build/tests green.)
- [x] All existing tests pass (no regressions)
- [ ] (Protocol message types are plan1a-protocol's success criterion;
  subprocess plumbing is plan1a-host's; harness dispatching, idempotency,
  and bundle-build are Plan 1b's.)
