# Plan 1c.2 — TS Engine Extensions: loose ends

**Parent:** [2026-04-16-plan1c-extension-integration.md](2026-04-16-plan1c-extension-integration.md)
**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Status:** not started (created 2026-07-01; spec reviewed against sources + revised 2026-07-02 —
decisions dated inline; design-doc counterparts in `engine-resolution.md` §3.3/§6.1/§8/§10 and the
Plan 5 stub §Invalidation)
**Relationship to Plan 1c:** 1c shipped complete + review-clean (all 15 tasks + a downstream
regression fix; the echo-engine E2E gate passed through the real `q2 render` binary + a real Deno
subprocess; `cargo xtask verify` green). During execution + the final whole-branch review, a set of
consciously-deferred / disclosed follow-ups accumulated. They are **not** merge-blockers for 1c (the
1c plan checklist reconciled them as annotated deferrals), but they are real work items. This plan
collects them so they aren't lost (the SDD ledger that tracked them is gitignored/per-session).

## Overview

1c's engine-resolution + execution + conversion pipeline is landed and validated end-to-end. What's
left is: one **half-wired contract that Plan 4 pins** (per-render engine project context — demoted
2026-07-02 from "blocker" to wiring hygiene by the Q1 engine survey, but still sequenced first), one
**execute-path metadata gap Plan 4 needs before its first render** (`TsFormatInfo.metadata` sent
empty — P1.1b, added 2026-07-02 from the Plan 4 review), one
**real CLI bug** (build-ts-extension `--config` precedence), a **project-discovery gap** for non-QMD
engine files, and a couple of **polish** items (including the `claims-extensions` rename). This is a much smaller SDD session than 1c — but note the items are **not**
uniformly small: P1.2 and the P4 extension fix are shovel-ready, whereas **P2 is the large item** (two
crates + a `RenderableExtensions` newtype + rerouting the discovery predicate + factoring a shared
static-claim helper out of `EngineClaimsFileStage`; see its corollaries). Size the session accordingly.

## Priority tiers + checklist

> **EXECUTION ORDER (tiers ≠ order):** P1.1 → **P1.1b** → P1.2 → **P4-rename+normalization** → P2 →
> P4-contribution_order → P4-crash-E2E (optional). P1.1b immediately follows P1.1 because it
> reuses the `ConfigValue → TsMetadataValue` helper P1.1 builds (and both extend the same echo
> marker channel). P2 consumes the canonical extension form P4
> establishes (Corollaries 2/3) — doing P2 first means redoing its set construction. P1.1 and
> P1.2 are independent of each other (P1.1's fixture rebuild uses the in-clone tier-3 path,
> which P1.2's bug doesn't affect).

### P1 — correctness (Plan 4 pins these; run first)

- [x] **Wire `set_project` / per-render `EngineProjectContext` (the `LaunchEngine { project }` half).**
  `TsEngine::set_project` has **zero call sites at all** (not even tests — verified 2026-07-01), so
  `ensure_launched` (the `unwrap_or_default()` at `ts_engine.rs:351`) always sends an empty
  `EngineProjectContext` (`project_dir: None`, `output_dir: None`, `config: None`,
  `is_single_file: false`).
  **Stakes recalibrated 2026-07-02:** a survey of every Q1 engine (knitr, jupyter, markdown,
  julia-engine.ts, marimo, test fixtures) found **no engine reads `dir`/`isSingleFile`/`config` from
  the launch context at all** — julia and marimo ignore the whole object. So this is **contract
  hygiene / wiring completeness (DQ-5, defined in `designs/engine-api-surface.md` §DQ-5)**, not a
  behavioral blocker. It still runs **before Plan 4** (decided 2026-07-02): Plan 4's prerequisite,
  4H assertion, and success criterion all pin the wiring.
  - **Field sources — fully settled, no converter research left (2026-07-02).** Q1's adapter
    (`engine-project-context.ts:23-57`) constructs exactly the two-key subset — the grand plan's
    wire spec (`2026-04-16-ts-engine-extensions-subprocess.md` §LaunchEngine, ~L381-387) *is* Q1:

    | wire field | source | note |
    |---|---|---|
    | `project_dir` | `ctx.dir` | stringify |
    | `is_single_file` | `ctx.is_single_file` | copy |
    | `output_dir` (top-level) | `ctx.output_dir: PathBuf` | **resolved** absolute — Q1 `getOutputDirectory()` |
    | `config.project."output-dir"` | `ProjectConfig.output_dir` | **raw** relative YAML value; typed field, no ConfigValue read. Raw-vs-resolved duplication is intentional (Q1 carries both) |
    | `config.engines` | `ProjectConfig.metadata.get("engines")` | the only ConfigValue lowering. Entries may be strings **or `{path: …}` maps** — pass through as data, uninterpreted (interpreting `engines:` is Plan 4b Task 9) |
    | `config` (whole) | `None` when no project config | single-file renders (`ProjectConfig::default()`), matching Q1's `context.config ? … : undefined` |

    **Shape:** `config` is `Option<HashMap<String, TsMetadataValue>>` — construct it as a
    **two-key map**: `{"engines": Array(…), "project": Map{"output-dir": String(…)}}` (omit a key
    when its source is absent, matching Q1's `undefined`s).
    **CORRECTION (2026-07-02, found during P1.1 implementation):** the nested `project` key above
    misdescribed the **wire** layer — the shipped host reads a FLAT wire config
    (`host.ts:183-216` doc + frozen 1c test `host.test.ts:2366+`: it pulls top-level
    `config["output-dir"]` and itself builds the nested `config.project.outputDir` the engine
    sees). Implemented flat (`{"engines": …, "output-dir": …}`); the engine-visible shape matches
    the table. Adjudicated against the authoritative host contract; T1 proves it end-to-end.
    Lowering mechanism: a **small recursive `ConfigValue → TsMetadataValue` helper (~25 lines)**
    over `Scalar`/`Sequence`/`Mapping` (string scalars via `as_plain_text()` — metadata-as-str lint
    lesson; bool/number/null direct; keys stringified; source info dropped), invoked **only** on the
    `engines` subtree. `TsMetadataValue` (`ts_protocol.rs:313-320`) is a plain JSON mirror. No Q1
    engine reads `config` (survey above), so the test is wire-faithfulness only.
  - **Call-site mechanism (settled; corrected 2026-07-02 review round 2): construction time.**
    `set_project` is an inherent `TsEngine` method behind interior mutability (`&self`, `Mutex`),
    and the registry erases the concrete type (`Arc<dyn ExecutionEngine>`) at `registry.register` —
    so the dominating point is where the concrete `TsEngine` is built: the `TsEngine::new` site at
    `project/mod.rs:557`, which lives inside **`build_engine_registry` (fn at `:441`)** — NOT
    directly in `discover_extensions_and_build_registry` (fn at `:630`), which calls it at `:653`
    under `#[cfg(not(target_arch = "wasm32"))]` (WASM builds a bare registry, no TsEngines). So the
    threading is **two hops** (`:630` → `:653` → `:441`), native-only. **The one live caller is
    `ProjectContext::discover` (`:791`)** — it handles single-file inputs itself (`is_single_file`
    computed at `:744`, `project_dir = None` passed) — with all fields resolved before the call.
    (`ProjectContext::single_file` (`:827`) has **no production callers** — grep 2026-07-02, only a
    unit test at `:1383`; update its call for consistency, but no seam binds it.) Thread the context
    (or its fields) through and set at construction — do **not** add `set_project` to the
    `ExecutionEngine` trait or downcast. First-write-wins is correct: the launch caches (Rust
    `ensure_launched` + TS host `launchedByName`) make later writes inert by design.
  - **Reword the two stale doc-comments** (`ts_engine.rs:339` "…reset owned by plan1c";
    `ts_engine.rs:122-125` field `TODO(plan1c)`). The render-boundary reset is not deferred — **a
    field-level reset is the wrong lever**: a launched instance holds its context in its closure
    behind both launch caches, unreachable by any `set_project` write. Say: *"First-write-wins per
    engine/host lifetime — launch caches make later writes inert by design. Every render builds a
    fresh registry+host, so the single write at registry build is complete. If engines ever outlive
    a ProjectContext (warm-host pooling), staleness is handled by invalidating the launched instance,
    not by resetting this field — see Plan 5 §Invalidation."* (Plan 5 stub carries the matching
    design note, added 2026-07-02.)
  - **Test:** extend echo (requires rebuilding its `dist/` via `q2 build-ts-extension` — the
    in-clone tier-3 path, unaffected by P1.2's bug) to echo back its received
    `EngineProjectContext`. **Channel (corrected 2026-07-02 during seam prevalidation):** capture
    the `context` param in `launch()`'s closure and have **`execute()`** append a
    `CONTEXT_JSON:{…}` marker line (`JSON.stringify(context)`) to its returned markdown — NOT
    `markdownForFile`: the project leg must use a `.qmd` with an `{echo}` cell (a `.echo` file in
    a project isn't discovered until P2 lands), and only `execute()` fires on that path. Two legs
    (seam detail in the Test Seam Spec, T1/T2): **both legs go through `ProjectContext::discover`**
    (the only live path — see the corrected call-site bullet) and bind different **field-source
    lines** of the same threading hunk. The project leg binds `project_dir`/`output_dir`/`config`
    + the Pass-2 launch; the single-file `.echo` leg binds the `is_single_file` source
    (`discover`'s `:744` branch) + the Pass-1 `markdown_for_file` first-launch (the echoed context
    proves the value was set before the FIRST launch, since the launched closure is what `execute`
    later reads). Assert the two config keys + raw-vs-resolved `output_dir` (not defaults) in the
    project leg, and `is_single_file: true` in the single-file leg (`config: None` there is a
    shape assertion — it equals the default; it guards the builder's absent-config branch, not the
    threading). Do **not** assert cross-render behavior in
    either direction (no second render exists per process today; freshness-on-reuse is Plan 5's).
  - Then check the reconciled 1c success-criterion "`LaunchEngine { project }` populated" (1c
    checklist L1887) as done, and tick DQ-5's build-checklist box in `engine-api-surface.md`.

- [x] **P1.1b — thread merged document metadata into `TsFormatInfo.metadata` (the execute-options
  half; added 2026-07-02 from the Plan 4 review, decision: owned here, same session as P1.1).**
  `build_execute_options` (`ts_engine.rs:~367-414`) hardcodes `metadata: HashMap::new()`, so **no
  frontmatter reaches a TS engine at execute time**. The Deno host side is already complete:
  `metadataAsFormat` (`quarto-engine-host-deno/src/metadata-as-format.ts`) partitions
  `TsFormatInfo.metadata` into Q1's six-bin `Format`; `kExecuteDefaultsKeys` includes
  `daemon`/`daemon-restart`/`fig-dpi`/`keep-hidden` (→ `format.execute`, where julia-engine.ts
  reads them); a `julia:` block falls through to `format.metadata` and reaches
  QuartoNotebookRunner via the serialized execute options. Consequences of the gap: Plan 4
  Phase 4E (daemon/exeflags/env/cell options) is untestable; `toMarkdown`'s
  `fig-format`/`fig-dpi` silently default; `execute: daemon: false` cannot disable the detached
  Julia daemon (production wires `is_interactive_session: runtime.is_interactive()`,
  `project/mod.rs:~503` — interactive dev renders daemon by default; kill-surface strand:
  bd-m1jeqhhz).
  - **Distinct wiring from P1.1** — P1.1 populates the launch-context `config` (project
    `engines` subtree, once per engine); P1.1b populates per-execute `TsFormatInfo.metadata`
    from the **merged document metadata**. They share the recursive
    `ConfigValue → TsMetadataValue` helper (P1.1 builds it on the `engines` subtree; P1.1b
    calls it over the full merged map) — hence same session, P1.1 first.
  - **Source:** the stage that builds `ExecutionContext` already has merged-metadata access
    (it resolves `execute_timeout` from `execute.timeout` today). Either add a metadata field
    to `ExecutionContext` or build the lowered map at the stage and thread it to
    `build_execute_options`; do **not** re-merge metadata anywhere.
  - **Test:** extend the echo engine's marker channel (same mechanism as P1.1's
    `CONTEXT_JSON:`) with a `FORMAT_JSON:` marker echoing `options.format.execute` plus one
    `format.metadata` key; assert that frontmatter `execute: {daemon: false}` and a custom
    top-level key round-trip through the wire and the host's `metadataAsFormat` binning.
    **Additive to the frozen Test Seam Spec** — add the seam entry (with revert hunk) before
    implementing, same prevalidation discipline as T1/T2; do not silently edit the frozen list.

- [x] **Fix `q2 build-ts-extension --config` precedence for the installed-binary path.**
  `execute()` resolves the shipped config eagerly (`build_ts_extension.rs:153-160`:
  `shipped_config_path(...)` returns `Option<PathBuf>`, rescued into a hard error by
  `.with_context()?`) **before** calling the pure `resolve_build_config` — so an installed binary
  (no workspace detected) hard-errors **even when `--config` is passed**, defeating the advertised
  precedence (Plan 1c SC L1841 "works from an installed binary"). **Precedence is 4-tier**
  (`resolve_build_config`, `build_ts_extension.rs:49`): `--config > ext-dir deno.json >
  workspace_root's deno.workspace.json > shipped`. `--workspace` is a **bool flag**, not a path.
  Zero blast radius today (no published engine extensions; the in-clone path works, P2-18 green).
  - **Fix shape (settled 2026-07-02): lazy tier 4 + EMBED the shipped config — no exe-adjacent
    probe.** The release tarball is single-member (just `q2`, `release.yml:10-11`), so probing
    `current_exe()/../resources/…` would target a path that never exists. Instead: (a) make tier-4
    resolution lazy so `--config` / ext-dir / `--workspace` short-circuit first; (b) embed
    `resources/extension-build/deno.json` via `include_str!` (it is **fully self-contained** —
    imports are `jsr:@quarto/api` / `jsr:@quarto/types` / `jsr:/@std/`, no relative paths; the
    workspace variant with `../../ts-packages/…` paths stays tier 3, in-repo only) and materialize
    it to a temp file when tier 4 is actually selected; (c) fix the overpromising docstring to
    describe the embed.
  - **JSR caveat (accepted 2026-07-02):** `@quarto/api`/`@quarto/types` are **not yet on JSR**
    (404 as of 2026-07-02) — tier 4 resolves but fails at `deno` fetch with a clear, actionable
    error until publication (planned post-demo). `--config` always works. Do not gate this fix on
    publication. When cutting a release, pin the embedded jsr specifiers to a version
    (`jsr:@quarto/api@^X.Y`) — add that line to the release runbook as part of this item.
  - **Test:** unit-test that `--config X` wins with `workspace_root = None` AND that tier 4 is
    never touched (make tier 4 a lazy provider; assert via a panic-if-called stub). The
    `--workspace` leg is separate and needs `workspace_root = Some(root)` (the flag *demands* a
    workspace — "wins with None" is incoherent); its assertion is tier-3 selected + provider still
    uncalled. `resolve_build_config` stays pure — note its `shipped_config: &Path`
    param becomes the lazy provider, so the existing P2-18/unit tests that pass it eagerly get a
    mechanical signature update. (Seams: T4/T5 — and note T4's vacuity warning: the binding seam
    is the inner fn `execute()` routes through, not the already-pure resolver.)

### P2 — feature completeness

- [ ] **Fold engine-declared extensions into project file discovery (`.echo`-in-a-project;
  admission axis = `claims-extensions`, per Corollary 3).**
  `crates/quarto-core/src/project/discovery.rs` is `.qmd`-only ("Phase-1 scope" — a pre-existing
  website-epic limitation; the `.qmd`-only freeze is a **user directive dated 2026-04-23**, recorded
  in `2026-04-23-websites-phase-1.md` §"File-list expansion", not a decision this epic owns), so a
  non-QMD engine file (`.echo`, `.jl`, `.ipynb`) dropped into a `_quarto.yml` project never enters the
  render list → 1c's `EngineClaimsFileStage` (correctly wired into both pipeline builders) never runs
  on it. Single-file `.echo` render works + is E2E-proven (P3-1b).
  - **Where the reject actually happens — one behavior edit, not two (verified 2026-07-02):** the
    explicit-file-arg rejection is `DispatchError::NotInRenderList` in the CLI dispatcher
    (`crates/quarto/src/commands/render.rs:317`), but its `project_files` comes **straight from
    `ProjectContext::discover(...).files`** (`render.rs:283-286`) — fixing discovery fixes the
    dispatcher gate automatically. The second change is test-only: **add** a new `.echo`-in-project
    admission test near `render.rs:1598` (that line is the existing `NotInRenderList`-for-excluded-qmd
    test — it stays valid as-is; don't rewrite it).

  **Clean integration design (examined 2026-07-01 — the `fixed_renderable_set ∪ registered_engine_extensions` corollary).**
  Deriving the correct shape from the current code, not the one-liner "thread the registry in":

  - **Corollary 0 — extension knowledge must move above the walk.** In `ProjectContext::discover`
    (`project/mod.rs:762–796`) the file walk (`discover_project_files`, line 776) runs **before**
    `discover_extensions_and_build_registry` (line 791) — so discovery cannot know what engines
    declare; the knowledge is built ~15 lines too late. Split extension *discovery + `contributes`
    parsing* (cheap, pure, no host, no binary-deps) above the walk; leave *registry finalization*
    (needs `binary_dependencies` + host) where it is — **and preserve P1.1's `set_project`/context
    threading in the finalization half** (`build_engine_registry`, `mod.rs:441`, where the
    `TsEngine::new` site lives; don't let the refactor orphan it). Discovery needs only the parsed
    `EngineContribution::External` static-claim extensions (`claims_extensions` after the P4
    rename — the admission axis per Corollary 3), **not** the heavy `Arc<EngineRegistry>`.
  - **Corollary 1 — discovery takes a resolved extension *set*, never the registry.**
    `DiscoveryConfig` today carries only resolved values (`project_dir`, `output_dir`,
    `render_patterns`) — never live services. Stay consistent: add one field
    `renderable_extensions: &RenderableExtensions` (a normalized-set newtype living in
    `discovery.rs`), computed by the caller as `FIXED_RENDERABLE ∪ engine claims-extensions`
    (the admission axis per Corollary 3 — NOT `file-extensions`). Discovery stays a pure path/string module
    (unit tests need no registry/host). Threading `Arc<EngineRegistry>` in is the tempting-but-wrong
    move — it couples the most dependency-light module in the project to engine internals.
  - **Corollary 2 — one predicate, one normalization (smaller than first sized).** The `"qmd"`
    literal lives in exactly **one** helper, `has_qmd_extension` (`discovery.rs:121`), with **two**
    direct callers (`is_renderable_qmd:84`, `walk_rec:329`; the pattern path funnels through
    `walk_rec` transitively); the glob machinery is already extension-agnostic. So the mechanical work is: replace the helper
    with `ext_in_set(path, set)` and thread the set — the walker filters candidates, so the set must
    reach the walk, not just the final predicate — **`walk_qmd` and `walk_rec` gain the set in
    their signatures** (today they take no `DiscoveryConfig`), which is most of the diff.
    **Normalization (simplified by doing P4 first):**
    P4 establishes canonical **undotted-lowercase at parse time** for declared extensions, and
    `path.extension()` is already undotted — so `RenderableExtensions` construction only lowercases
    candidates and *asserts* (rather than performs) the declared-side normalization. A newtype that
    owns this beats a bare `HashSet<String>`.
  - **Corollary 3 — admitted ⟹ statically claimed, as a COHERENCE property (reframed 2026-07-02;
    non-enforcement directive).** Discovery admitting `a.echo` and `EngineClaimsFileStage` claiming
    it must be driven by the **same declared data through the same match rule** — a *single*
    static-claim helper consulted by both the discovery-set builder and the claim stage, so the
    invariant holds by construction. **Where it lives:** next to the declared data (a fn on
    `EngineContribution`/in `extension/` or `engine/`), NOT in `discovery.rs` — the *caller*
    (`project/mod.rs`) uses it to build the set (Corollary 1 keeps discovery ignorant of engines),
    and `TsEngine::claims_file`'s static path uses the same fn. The admission axis is **`claims-extensions`** (static claims),
    not `file-extensions` (can-handle): admit only what some engine definitively owns.
    **Stakes, stated precisely (corrected 2026-07-02 round 2):** divergence gets NO **new** error
    surface. An admitted-but-unclaimed non-qmd file hits the claim stage's **existing §10 case-1
    loud failure** ("can't determine execution engine" — bound test
    `test_p2_11_unclaimed_non_qmd_errors`, `engine_claims_file.rs:541`) — a claim-level, Q1-parity,
    resolution-time error, squarely within the non-enforcement directive ("errors due to claiming"
    are fine; we just never verify what engines *execute* — see `engine-resolution.md` §10). An
    earlier draft said the miss path "falls through and renders as markdown" — wrong: that graceful
    pass-through is the *cell/language* axis (§8); whole unclaimed files error at case-1. The
    invariant is worth having for determinism and least-surprise, and it makes case-1 nearly
    unreachable from discovery (admission uses the same static-claim data). The error surfaces that remain are all **claim-level**
    and already exist: static-vs-dynamic claim mismatch at first load (the synthetic-file guard —
    an over-declared sniffing engine that puts a sniffed extension in `claims-extensions` hard-errors
    on its first conversion, pointing at `_extension.yml`), missing bundle at registry build,
    engine-name collision, unknown name in `engines:` (4b-C).
  - **Corollary 4 — extension axis here; content axis is Plan 7's (recharacterized 2026-07-02).**
    The earlier draft called `claims_files` "filename globs, possibly literal names like `Makefile`"
    — that was wrong: it is an **extension set by design** (match is `ext ∈ list`,
    `ts_engine.rs:606`; renamed **`claims-extensions`** in P4 below; no Q1 engine claims by filename
    anywhere in the corpus). The two static surfaces are `file-extensions` (can-handle, the
    pre-filter) and `claims-extensions` (definitively owns, the admission axis per Corollary 3).
    What stays out of discovery is the **content axis**: percent scripts (`.py`/`.jl`/`.r` with
    `# %%`) and R spin scripts — the only content-sniffed claims in the entire Q1 corpus. Q1 sniffs
    every candidate file *during the project walk* (admission IS the dynamic claim,
    `project-context.ts:932` → `fileExecutionEngine`); q2 deliberately does not. Those files can't
    render in q2 at all yet (native jupyter/knitr implement no `claims_file`) — **Plan 7 owns both
    the conversion and the sniff-at-discovery decision**; name it as the owner, not an open
    deferral of this epic. A filename-claim surface, if ever wanted, is a new field, not
    `claims-extensions`.
  - **Corollary 5 — extension overlap is a downstream claim concern, not a discovery one.** If an
    engine declares `.qmd` (or later a `.md` that `FIXED` also holds), the *set* dedups and discovery
    admits once; *which* engine owns the file is decided downstream by claim + engine resolution.
    Discovery stays purely additive to the admission set and never routes.
  - **Corollary 6 — composition with the website epic is a one-line interface agreement, not an
    ordering dependency.** The website epic's deferred work is `FIXED_RENDERABLE` growing
    `{qmd}` → `{qmd, md, ipynb, rmd, …}`; this epic adds the engine-declared members. Because it's a
    set union: (a) ship with `FIXED = {qmd}` unchanged so **every existing discovery test stays green**
    (`notebook.ipynb`/`notes.md` still excluded — nothing declares them yet), and (b) the website epic
    later just *adds members to `FIXED`*. The only coordination needed is agreeing **now** on the
    single set-valued seam (`RenderableExtensions` in `DiscoveryConfig`) so the website epic's change
    is "add members to one set," not "bolt on a second parallel gate." Either epic can land first.

  - **Test (binds Plan 1c's P2-16 project-level seam) — positive assertions only:** a `.echo` in a
    project render → its `ProjectIndex` entry has the converted doc's title/outline. **Fixture note
    (checked 2026-07-02):** echo's `markdownForFile` currently emits only fenced code blocks — no
    heading, no front matter — so the converted profile has no title. Extend the wrapper to prepend
    a heading (`# Echoed: <basename>` — basename, not full path, per T8) or front matter (rebuild
    `dist/` via `q2 build-ts-extension`),
    which also makes converted-vs-raw sharply distinguishable. This exercises the Corollary-3
    invariant end-to-end on the happy path; per the non-enforcement directive, do NOT add a test
    asserting the miss path errors. Add a companion unit test in `discovery.rs` proving an
    engine-declared extension is admitted while the existing exclusions
    (underscore/dot/README/output-dir) still apply to it. (Seams: T6/T7/T8 — T8 requires the
    fixture `.echo` body to contain no markdown heading, so raw fall-through can't fake the title.)

### P3 — removed (2026-07-02): WASM name-section strip → bd-vm53h64q

The build-size cleanup (strip the ~3.18 MB WASM name section, revert the workbox 35→40 MB
stopgap at `hub-client/vite.config.ts:124`) was cut from this plan as unrelated to the epic —
**the epic keeps the 40 MB limit**. Tracked in strand **bd-vm53h64q** (chore, P3), which carries
the diagnosis numbers and the binaryen-provisioning decision. Tier label P4 below kept as-is so
cross-references stay valid.

### P4 — polish / hardening

- [ ] **Rename `claims-files` → `claims-extensions` + parse-time extension normalization (both
  lists).** Decided 2026-07-02 (full rationale: `engine-resolution.md` §3.3 rename note). The field
  is an extension set by design (`extension/types.rs:113-114` doc says so; match is `ext ∈ list`,
  `ts_engine.rs:591/606`); the old name inherited Q1's *method* name (`claimsFile`) and misled
  (see the corrected Corollary 4). Zero blast radius: no published extensions; touchpoints are the
  YAML key (`read.rs:425`), the Rust field (`extension/types.rs`, `ts_engine.rs`, `project/mod.rs`),
  the user-facing validation strings + missing-field warning (`extension/types.rs:206-217`, tests
  `:484-492`), the echo fixture `_extension.yml`, tests, the two remaining `engine-resolution.md`
  mentions (worked example ~:132, §3.3 table row ~:146 — the rename note itself stays), plan-1c
  mentions, and one prose line in plan 4b (~L94, already reworded).
  - **Normalization:** today `parse_contributes` stores YAML verbatim and the stage lowercases only
    the candidate side, so `file-extensions: [".Echo"]` silently never matches. Fix at parse time
    for **both** lists: accept dotted or undotted input, store canonical **undotted lowercase**.
  - **The dot stays on the wire:** the JS contract is dotted (Q1 `extname()` convention; engines
    compare `ext === ".echo"`). Two Rust→TS seams re-dot: `ToEngine::ClaimsFile` construction
    (`ts_engine.rs:627-631`) and the synthetic-file load validation (`x<ext>`, `ts_engine.rs:304-308`).
    `EngineClaimsFileStage` stops re-dotting (`:115-119`) — note the single `ext` value built there
    feeds **three** consumers (the pre-filter `:591`, the static claim `:606`, and the dynamic wire
    call `:627-631`), so de-dotting it is total and ONLY the wire seams re-add the dot.
  - This dissolves the old "decide `claims_files` case-handling" question — an extension list folds
    like an extension list — and it feeds three consumers one canonical form (parse, claim stage,
    P2's `RenderableExtensions`).
  - **Test:** an uppercase-declared extension (either list, dotted or undotted in YAML) claims a
    lowercase file; the wire messages still carry dotted lowercase; the synthetic-file validation
    still catches an over-declared sniffing engine. (Seams: T9/T10/T11 — T11 asserts the captured
    wire messages directly via `MockTransport`; the existing echo e2e staying green is the
    real-JS-side guard.)
  - **Sequencing:** land before Plan 4/4b author new `_extension.yml` fixtures (they should write
    `claims-extensions`).
- [ ] **`EngineRegistry::contribution_order` encapsulation — read side now, write side in 4b-C
  (decided 2026-07-02).** Do the cheap tightening here: `pub(crate)` on the field (`registry.rs:62`)
  + a `pub fn contribution_order(&self) -> &[String]` getter. The getter is required, not optional:
  the integration test (`tests/integration/engine_registry_build.rs:120-122,330-333`) links
  quarto-core as an external crate and reads the field — **update those two read sites to call the
  getter** (they're the only external readers). In-crate unit-test pushes keep working under
  `pub(crate)`. **Defer the write API** (push/dedup/splice methods) to Plan 4b-C, where the second
  writer (`_quarto.yml engines:` splice at the Task-9 site) lands and its ordering contract
  (user-listed first, deduped, then contributions) is what shapes the method names. Mutation
  exposure is near-theoretical anyway — the registry is `Arc`-frozen after build.
- [ ] **(Optional, low priority) crash-path E2E (Plan 1c P3-4 / §1468).** A fixture whose engine
  `Deno.exit(1)`s mid-execute → assert the render fails with a `ProcessCrashed`-shaped error carrying
  captured stderr, no leaked subprocess. Exercises the reader-thread EOF→broadcast against a real
  process (only `MockTransport` covers it today).

## Design-ratification (not code — carry the 1c decision)

- [x] **T9 resolution-driven handoff loss — RATIFIED 2026-07-01 (Gordon).** The engine sequence is
  derived **once, from the original parsed AST** — an engine is in the sequence only if it owns ≥1
  language actually present in the source. This is intended behavior, not a bug. Directive on ratifying:
  *carefully describe the scenarios we are ruling out, document them, and move on.* Design sections:
  **§6.1** (sequential threading & handoff), **§4.3** (fallback gating), **§11 / bd-r8n4r**
  (nested-handoff splice → "live preview limitation"), **§8** (file-claim single-engine) — *not §4.1/§7*
  as an earlier draft mis-cited.
  - [x] **Action (done 2026-07-02):** the scenario enumeration below is copied into
    `engine-resolution.md` §6.1 (with cross-refs to §4.3/§8/§11), alongside the related
    non-enforcement statement at §10 ("Capability is judged from declarations, never from execution
    results" — ratified 2026-07-02: q2 does not verify post-hoc that engines execute what they own;
    unexecuted cells render as code blocks). *(The other half of the original action — "drop the
    'T9 pending' note from the SDD ledger" — was stale: no such note exists; the ledger records T9
    COMPLETE.)*

  **Scenarios this rules OUT (documented, accepted):**
  1. **Injected-cell handoff to an engine absent from the sequence.** Engine A, *at execution time*,
     emits a cell in language L whose only would-be owner is engine B — but B was excluded because the
     *original* source had no L cells. The sequence is fixed pre-execution, so B never runs and the
     injected L cell is not executed by B.
  2. **An explicitly-listed engine that owns nothing originally is dropped.** `engines: [knitr, customX]`
     where customX's language never appears in the source: customX contributes nothing to the sequence
     and cannot receive runtime-injected cells in its language. Note the fallback net does **not** save
     this: per §4.3, T4 only adds jupyter for *implicit* sequences, so an explicit `[knitr]` with a
     runtime-injected `{python}` does **not** auto-add jupyter either.

  **Scenarios that still WORK (unaffected):**
  - Handoff between engines that both own something in the original AST — e.g. knitr re-emits a
    `{python}` cell and jupyter executes it, *because the doc already had `{python}` cells* so jupyter is
    in the sequence.
  - knitr↔reticulate interop; jupyter-as-`Fallback(0)` catching the remainder in *implicit* docs.

  **Why acceptable / why not "just fix it":** resolving (1)/(2) would require **runtime sequence growth**
  — re-resolving the sequence mid-execution as new cells appear — which the resolution-driven + replay
  model deliberately avoids (§6.2: replay drives from recorded captures, *not* re-resolution; mid-execute
  re-resolution would break the determinism guard and the eventual freeze cache-key). It is already
  tracked as a live-preview limitation (bd-r8n4r), and the valuable common handoffs are all in the
  "still works" set above. The old test was honestly rewritten to assert the resolution-driven behavior.

## Test Seam Spec (frozen — prevalidated 2026-07-02)

One row per test. Tiers: **unit** = pure Rust, no subprocess; **msg** = Rust unit with
`MockTransport` capture (`ts_engine.rs`/`ts_process.rs` pattern); **e2e** = Deno-gated integration
(`echo_engine_e2e.rs` pattern: early-return skip when `deno` absent, real `render_to_file` path,
assert on rendered output). Once green, assertions and harness are **frozen** — never edited to go
green. **Rows are grouped by item, not execution order** — execute as
T1–T3 → T14 → T4–T5 → T9–T11 → T6–T8 → T12 → (T13), per the EXECUTION ORDER callout.
*(Revised 2026-07-02 round 2: T1/T2 re-targeted after source check — `ProjectContext::single_file`
has no production callers, so both legs bind the `discover` path; T10 re-mounted on the stage;
T14 added for P1.1b.)*

| # | item | tier | real unit mounted / seam | revert hunk → RED assertion |
|---|------|------|--------------------------|------------------------------|
| T1 | P1.1 | e2e | temp project (`_quarto.yml` with `output-dir: out` + `engines: [echo]` — use the **registered** engine name so Plan 4b's future unknown-name validation can't retro-break the fixture — + page.qmd with an `{echo}` cell) → `render_to_file`; echo's `execute` emits `CONTEXT_JSON:{…}`; assert on rendered HTML | revert the context-threading through `discover_extensions_and_build_registry` (`:630`) → `build_engine_registry` (`:441`) to the `TsEngine::new` site (`:557`), fed by the `discover` caller (`mod.rs:791`) → `project_dir` null / `output_dir` ≠ `…/out` / `config` missing its two keys → RED. **Exercised guard:** assert the `CONTEXT_JSON:` marker is present at all. **Discriminators:** `project_dir`, resolved `output_dir`, `config` keys — NOT `is_single_file:false` (equals default; asserting it is theater) |
| T2 | P1.1 | e2e | temp dir, no `_quarto.yml`, `file.echo` → single-file render; same `CONTEXT_JSON` channel. **Same threading hunk as T1** (both go through `discover` — `single_file()` has no production callers); T2 binds a different **field-source line**: the `is_single_file` value from `discover`'s `:744` branch | revert the `is_single_file` field-source line of the threading → stays `false` (default) → RED. Discriminator: `is_single_file:true` ONLY. `config:null` is a shape assertion (equals default — guards the builder's absent-config branch, a separate mini-hunk: "builder returns `Some(map)` unconditionally" → RED). Also binds first-write-before-Pass-1-launch (see P1.1 test bullet) |
| T3 | P1.1 | unit | the `ConfigValue → TsMetadataValue` helper + config-map builder; input mapping `engines: ["knitr", {path: "x.js"}]`, `output-dir: out` | revert the `Mapping` lowering arm → the `{path}` entry drops/misshapes → RED; revert `as_plain_text()` scalar arm → string entries drop → RED. Also assert: absent `engines` ⇒ key omitted; no metadata ⇒ `config: None`. *(Note: the TS-side type declares `engines?: string[]` — narrower than the `{path}` maps we pass; Q1's own adapter cast is equally loose, and passing the runtime values is the faithful choice. Inert: no engine reads it.)* |
| T4 | P1.2 | unit | the new inner selection fn that `execute()` delegates to (owns the eager/lazy boundary; tier 4 = lazy provider closure); `--config X` + everything else `None` + provider `\|\| panic!()` | re-eagerify (restore `shipped_config_path(...).with_context()?` ahead of selection) → provider fires → panic → RED. **Vacuity note:** testing only the already-pure `resolve_build_config` binds NOTHING — it already handles `None`; the bug lives in the caller, so the seam is the inner fn `execute()` actually routes through |
| T5 | P1.2 | unit | same fn, all tiers absent → tier 4 selected | revert the `include_str!` embed/materialization → tier 4 yields error/`None` → RED. Assert returned path's content == the embedded `deno.json` source and parses as JSON |
| T6 | P2 | unit | `discover_project_files` with `DiscoveryConfig{ renderable_extensions: {qmd, echo} }` over a temp tree: `a.echo`, `_draft.echo`, `.hidden.echo`, `out/b.echo`, `notebook.ipynb` | revert the `ext_in_set` threading (restore `has_qmd_extension`) → `a.echo` excluded → RED. Second hunk: admit engine extensions via a branch that bypasses the underscore/output-dir checks → `_draft.echo` admitted → RED (binds "same predicate path"). `notebook.ipynb` stays excluded (not in set) |
| T7 | P2 | unit | `RenderableExtensions` newtype: build from canonical `["echo"]`; candidates `A.ECHO`, `a.echo` | revert candidate lowercasing → `A.ECHO` rejected → RED. Construction contract (accepts P4's canonical form) asserted per newtype doc |
| T8 | P2 | e2e | temp project + `a.echo` (echo's `markdownForFile` extended to prepend `# Echoed: <basename>` — **basename**, not the full path, or the title assertion fights a temp dir); full project render; assert `ProjectIndex` entry / rendered page has title `Echoed: a.echo` | revert the discovery-set union (drop engine-declared members) → `a.echo` not in render list → no index entry → RED. **Discriminator:** the fixture `.echo` body must contain NO markdown heading line, so a non-conversion path can't produce the title. Binds Corollary 3 end-to-end, happy path only |
| T9 | P4 | unit | `parse_contributes` (`read.rs`): YAML variants `[".Echo"]`, `["echo"]`, `["ECHO"]` for BOTH lists | revert the parse-time fold (store verbatim) → stored `".Echo"` ≠ expected `"echo"` → RED. **Migration vacuity check:** existing asserts of `[".echo"]` migrate to `["echo"]`; the mixed-case + dotted inputs keep the discriminator (verbatim storage would differ on at least `.Echo`/`ECHO`) |
| T10 | P4 | unit | **`EngineClaimsFileStage` mounted via its existing `make_ctx_with_registry` harness** (`engine_claims_file.rs:431`) with an engine declaring normalized `claims_extensions: ["echo"]`; run the stage on path `X.ECHO` → claimed. *(Re-mounted 2026-07-02 round 2: a direct `claims_file` call constructs its own candidate, leaving the stage's candidate construction — the named hunk — outside the seam = vacuous.)* | revert the stage's candidate-construction alignment (`engine_claims_file.rs:115-119` keeps re-dotting while storage went undotted) → `".echo"` ≠ `"echo"` → unclaimed → RED |
| T11 | P4 | msg | dynamic path via `MockTransport`: capture `ToEngine::ClaimsFile` (no static claim declared) and the load-validation message (static claim recorded) | revert the wire re-dot seams → captured `ext == "echo"` / synthetic `file == "xecho"` instead of `".echo"`/`"x.echo"` → RED. The real-JS counterpart guard is the existing echo e2e staying green (a real dotted-comparing engine still loads + claims) |
| T12 | P4 | — | `contribution_order` getter: mechanical; no new seam — the two migrated integration-test reads keep their existing assertions binding | n/a |
| T13 | P4 (opt) | e2e | crash fixture: engine prints a marker to stderr then `Deno.exit(1)` mid-execute; assert render errors with `ProcessCrashed`-shaped error carrying the stderr marker; no orphan subprocess | revert the reader-thread EOF→broadcast (`ts_process.rs`) → no error propagates → test hangs → RED by timeout (a hang-based RED is fragile but acceptable for an optional seam; bound it with nextest's per-test timeout). Assert BOTH the error shape and the stderr content (error-without-stderr is a weaker regression) |
| T14 | P1.1b | e2e | extends T1's fixture: page.qmd frontmatter gains `execute: {daemon: false}` + one custom top-level key; echo's `execute` also emits `FORMAT_JSON:{…}` echoing `options.format.execute` + that `format.metadata` key (exercises the host's `metadataAsFormat` six-bin partition for real) | revert the `build_execute_options` metadata threading (restore `metadata: HashMap::new()`, `ts_engine.rs:~367-414`) → `FORMAT_JSON` shows empty `execute`/no custom key → RED. **Discriminators:** `daemon:false` (non-default binned value) + the custom key (cannot appear without threading). **Exercised guard:** assert the `FORMAT_JSON:` marker is present at all |

**Accepted-untested (logged, not silent):**
- **Admitted-but-unclaimed miss path — NOT untested after all (corrected 2026-07-02 round 2):**
  the claim stage already hard-errors an unclaimed non-qmd file (§10 **case-1**, "can't determine
  execution engine" — existing bound test `test_p2_11_unclaimed_non_qmd_errors`,
  `engine_claims_file.rs:541`). That's a claim-level, Q1-parity error, consistent with the
  non-enforcement directive; no new test and no new error surface needed.
- **Tier-4 JSR fetch failure UX** (P1.2): needs network + published packages; accepted until
  post-demo publication. The embed + selection are covered (T5); the deno fetch is not.
- **Cross-render context freshness**: Plan 5's by decision (see P1.1 comment rewording); no test in
  either direction.
- **`engines:` `{path}`-entry runtime semantics**: we pass values only (T3 covers shape);
  interpretation is Plan 4b Task 9's.
- **Doc-comment rewordings** (P1.1) and the release-runbook jsr-pinning line: prose, untestable.

## Notes
- Execute like 1c: SDD (implementer + reviewer per task, strict TDD, named reverts). Smaller scope than
  1c overall, but **P2 is the large item** (see Overview) — don't batch it with the shovel-ready ones.
- **P1 runs before Plan 4** (decided 2026-07-02 — Plan 4's prerequisite/4H assertion/SC pin the
  wiring, even though the engine survey demoted it from behavioral blocker to contract hygiene).
- **Ordering coordination — P4's rename+normalization runs before P2** (see the EXECUTION ORDER
  callout, which is authoritative). Both P2 (Corollary 2, discovery set) and P4
  (`EngineClaimsFileStage` match) consume the declared extension lists, and Corollary 3 requires
  them to agree; P4 establishes the parse-time canonical form (undotted lowercase) both consume.
  P4 should also land **before Plan 4/4b author new `_extension.yml` fixtures**, so they write
  `claims-extensions` from the start.
- **DQ-1…DQ-7 are defined in `claude-notes/designs/engine-api-surface.md`** (decisions §, build
  checklist below it) — cited throughout this epic without a pointer until now. DQ-5's wire shape is
  additionally spelled out in the grand plan §LaunchEngine (~L381-387).
- **P2 ↔ website epic is an interface agreement, not an ordering dependency** (P2 Corollary 6): it's a
  set union (`FIXED_RENDERABLE ∪ engine-declared`), so either epic can land first — the only thing to
  agree **now** is the single `RenderableExtensions` seam in `DiscoveryConfig` so the website epic adds
  members to one set rather than bolting on a second gate.
- **P3 (WASM name-section strip) removed 2026-07-02** — unrelated to the epic; tracked in
  **bd-vm53h64q**. The workbox 40 MB bump at `hub-client/vite.config.ts:124` stays for this epic.
