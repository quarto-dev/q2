# Plan 1c.2 — TS Engine Extensions: loose ends

**Parent:** [2026-04-16-plan1c-extension-integration.md](2026-04-16-plan1c-extension-integration.md)
**Grand plan:** [2026-04-16-ts-engine-extensions-subprocess.md](2026-04-16-ts-engine-extensions-subprocess.md)
**Design docs:** `engine-resolution.md` §3.3/§6.1/§8/§10; `engine-api-surface.md` (DQ-1…DQ-7);
Plan 5 stub §Invalidation.
**Successor for content claims:** [2026-07-07-plan7a-static-content-pattern-claims.md](2026-07-07-plan7a-static-content-pattern-claims.md)
**Status:** P1 landed (2026-07-02); **P2 + P4 landed (2026-07-07)** on `feature/ts-engine-extensions`
via SDD (commits `1406074d2`→`e5a07644` + a wasm-warning fix; see `.superpowers/sdd/progress.md`).
All checklist items complete; `cargo xtask verify` green. Source refs current as of 2026-07-06.
**Reworked 2026-07-07:** the P4 "rename `claims-files → claims-extensions`" is **withdrawn**. A full
census of Q1's engine file-claims showed `claims-files` is a genuine *file-claim* surface (extension +
an optional content pattern), not a bare extension set — so the name was right all along. P4 now
**keeps `claims-files`** and instead restructures it into typed entries. The content-pattern half is
carved out into **Plan 7a** (which precedes Plan 7); 1c.2 lands the extension-only foundation it builds
on. See §"What changed in the rework" for the full delta.

> **Source-path note (added 2026-07-07 at execution start).** Throughout this plan two files are
> written with a shorthand `crates/quarto-core/src/` prefix; their real locations are:
> - `ts_engine.rs` → **`crates/quarto-core/src/engine/ts_engine.rs`**
> - `engine_claims_file.rs` → **`crates/quarto-core/src/stage/stages/engine_claims_file.rs`**
>
> All cited **line numbers within those files are accurate** (verified against HEAD `58d266eab`); only
> the directory prefix in the prose drifted. Note also `static_file_answers` lives in
> `engine/ts_engine.rs` (declared :151, pushed :723-725), not in `engine_claims_file.rs`. Other refs
> (`discovery.rs`, `project/mod.rs`, `extension/read.rs`, `extension/types.rs`, `engine/registry.rs`,
> `engine/resolution.rs`, `commands/render.rs` in the **`quarto`** crate) verified accurate, with one
> off-by-one: `build_engine_registry` is *called* at `project/mod.rs:802` (the `#[cfg]` attr is at :801).

## Overview

1c's engine-resolution + execution + conversion pipeline is landed and validated end-to-end. This
plan collects the follow-ups deferred during 1c. **P1 (correctness) is complete and in-tree.** The
remaining work is:

- **P2 — feature completeness:** fold **statically-claimed** engine extensions into project file
  discovery so a non-QMD engine file that an engine *unconditionally* claims (today: `.echo`) actually
  renders when dropped into a `_quarto.yml` project. This is the large item (touches `discovery.rs` +
  `project/mod.rs`, adds a `RenderableExtensions` newtype in `discovery.rs`, and a shared
  `claimed_file_extensions` free fn co-located with `lookup_static_claim` — see Corollary 3).

  **Scope of "engine file" here (corrected from the original draft's `.echo/.jl/.ipynb` claim).** P2
  admits only extensions an engine claims **statically and unconditionally** via `claims-files`.
  - `.echo` → echo declares `claims-files: [.echo]` → **delivered by P2.**
  - `.jl` → julia declares only `file-extensions: [.jl]` (a can-handle pre-filter), not a static
    `claims-files` claim → **not** admitted until julia declares one (a fixture change, tracked with
    Plan 7a / julia-validation).
  - `.py`/`.jl`/`.R` percent/spin scripts → claimed **content-conditionally** (`# %%` / `#' ---`);
    that is a **content pattern**, which is **Plan 7a's** static-content-claim feature, evaluated
    natively at discovery. Not P2.
  - `.ipynb` (built-in jupyter, always a document) → **website epic's** `FIXED_RENDERABLE` growth
    (Corollary 6), not this plan.
- **P4 — polish / hardening:** restructure `claims-files` into typed `{extension}` entries with
  parse-time extension normalization (**keeping the name**; content-pattern deferred to Plan 7a);
  encapsulate `EngineRegistry::contribution_order`; an optional crash-path E2E.

## Sequencing: run before Plan 4b

**This plan runs before Plan 4b, and P2/P4 should be executed now.** Two reasons:

1. **It must not languish further.** P1 landed 2026-07-02; the rest has sat unworked while Plans
   4/4b/4c/9/10 advanced. Finish it before it drifts again.
2. **Plan 4b depends on P4.** 4b's Phase-A synthetic fixtures author `claims-files` (the
   `content-claim` fixture, `plan4b` Phase 4b-A) — the **P4 restructure is a hard prerequisite** so 4b
   writes the **typed `claims-files` entry shape from the start** (no re-migration when Plan 7a adds
   `content-pattern`). 4b-C's `_quarto.yml engines:` splice builds on P4's `contribution_order` getter.
   (P1 — `set_project`/`EngineProjectContext` — already satisfied 4b's Julia prerequisite; that box can
   be ticked in 4b.)

**Execution order:** **P4 restructure + normalization → P2 → P4 `contribution_order` → P4 crash-E2E
(optional).** P4's restructure runs before P2 because both consume the declared claim lists and
Corollary 3 requires them to agree on a single canonical form (undotted lowercase), which the P4
parse-time fold establishes.

## Checklist (remaining work)

> **Start here → §P4 restructure.** The checklist is in execution order, but the *prose sections* below
> read §P2-then-§P4 for narrative flow. The first thing to build is **P4 restructure + normalization**
> (§P4, further down); §P2 is implemented *after* it (it consumes `FileClaim` + the parse-time
> canonical form). See **Execution order** above.

- [x] **P4 restructure + normalization** — keep `claims-files`; introduce typed `FileClaim { extension }`
  entries (bare-string `.echo` shorthand accepted via untagged serde); parse-time undotted-lowercase
  fold for both `file-extensions` and `claims-files`; single `to_wire_ext` adapter (T9/T10/T11).
  **Content-pattern is out of scope — Plan 7a.**
- [x] **P2 discovery fold** — statically-claimed extensions enter project discovery (both the walk and
  the `render:`-pattern paths) via a `RenderableExtensions` set; shared `claimed_file_extensions` free
  fn co-located with `lookup_static_claim` (T6/T6b/T7/T7b/T8/T8b).
- [x] **P4 `contribution_order` encapsulation (read side)** — `pub(crate)` + getter; migrate the
  integration reads (T12).
- [x] **P4 crash-path E2E (optional)** — `ProcessCrashed` on mid-execute `Deno.exit(1)` (T13).

P1.1 / P1.1b / P1.2 landed 2026-07-02. P3 was an unrelated WASM-name-section cleanup, removed to
strand **bd-vm53h64q**; the **P4 label is kept** because plan-4b cross-references "1c.2 P4."

---

## What changed in the rework (2026-07-07)

The content-pattern census (Q1: every engine file-claim is `extension-gate → read-file → one regex`,
never a must-load operation; the knitr `spin`→Rscript work is the *conversion*, post-claim) overturned
the assumption that content-inspecting claims are dynamic-only. Consequences for this plan:

1. **P4 rename withdrawn.** `claims-files` stays `claims-files` (it *is* a file-claim surface). P4 now
   restructures the value from `Vec<String>` to typed `Vec<FileClaim>` (extension-only for now).
2. **P2 Overview corrected.** Only *statically, unconditionally* claimed extensions are P2's deliverable
   (`.echo`). The `.jl`/`.ipynb` examples were removed — `.jl`/`.py`/`.R` content claims are Plan 7a;
   `.ipynb` is the website epic.
3. **Corollary 4 resolved.** Its deferred "sniff-at-discovery decision" is **answered by Plan 7a** (yes,
   statically, via a content pattern evaluated natively at Pass-1). Plan 7 keeps the *conversion*.
4. **Terminology.** P2's corollaries and tests now say `claims-files` throughout (the aborted
   `claims-extensions` name is gone).
5. **Review nits folded in.** T12 read-site count corrected (4 reads / 2 assertions); the
   `project/mod.rs` `claims_files` back-reference corrected to line 1500.

---

## P1 — correctness (LANDED 2026-07-02)

Complete and verified in-tree; recorded here for the P2 refactor, which must preserve the `set_project`
threading (Corollary 0).

- **P1.1 — per-render `EngineProjectContext` wiring.** `TsEngine::set_project` is called at
  `project/mod.rs:713`, inside `build_engine_registry` (`:565`) at the `TsEngine::new` site (`:697`),
  once per engine at construction (first-write-wins; the launch caches make later writes inert). It
  carries `project_dir` / `is_single_file` / `output_dir` (raw + resolved) / `config`, lowering the
  project `engines` subtree via a recursive `ConfigValue → TsMetadataValue` helper. The wire config is
  **flat**; the Deno host (`quarto-engine-host-deno/src/host.ts`, `reconstructRichProject`) rebuilds the
  nested `config.project.outputDir` the engine sees. `ensure_launched` reads `self.project`
  (`ts_engine.rs:364`). Bound by echo E2E (`echo_engine_e2e::p1_1_*`) + unit tests. Reconciles plan-1c
  SC "LaunchEngine { project } populated" and DQ-5.
- **P1.1b — merged document metadata into `TsFormatInfo.metadata`.** `build_execute_options` threads
  `metadata: ctx.metadata.clone()` (`ts_engine.rs:394`) from the merged document metadata (no
  re-merge). The Deno host's `metadataAsFormat` partitions it into Q1's `Format` (five bins:
  identifier/render/execute/pandoc/metadata; `kExecuteDefaultsKeys` in `quarto-api/src/config/index.ts`
  routes `daemon`/`daemon-restart`/`fig-dpi` etc. to `format.execute`). Bound by T14.
- **P1.2 — `build-ts-extension --config` precedence.** `resolve_build_config` (`build_ts_extension.rs:61`)
  is 4-tier — `--config > ext-dir deno.json > workspace deno.workspace.json > shipped` — with the shipped
  tier as a **lazy provider closure** (`shipped_config: F`), so `--config` short-circuits before tier 4.
  The shipped `deno.json` is embedded via `include_str!` (`SHIPPED_DENO_JSON`, `:127`) and materialized
  to a temp file only when tier 4 is selected. `--workspace` is a bool flag. Bound by T4/T5. See § JSR /
  offline test policy for the installed-binary consequence.

---

## P2 — feature completeness

**Fold statically-claimed engine extensions into project file discovery (`.echo`-in-a-project).**

`crates/quarto-core/src/project/discovery.rs` is `.qmd`-only (the `.qmd`-only freeze is a website-epic
user directive, `2026-04-23-websites-phase-1.md` §"File-list expansion", not this epic's to lift). So a
non-QMD engine file dropped into a `_quarto.yml` project never enters the render list, and 1c's
`EngineClaimsFileStage` never runs on it. Single-file `.echo` render already works (P3-1b).

**Where the reject happens — one behavior edit.** The explicit-file-arg rejection is
`DispatchError::NotInRenderList` (`crates/quarto/src/commands/render.rs:318`), but its `project_files`
comes straight from `ProjectContext::discover(...).files` (`render.rs:284`/`:286`) — fixing discovery
fixes the dispatcher gate. The only other change is test-only: add a `.echo`-in-project admission test
near the existing excluded-qmd test (`classify_qmd_excluded_by_render_list_errors`, `render.rs:1592`,
which stays valid). (Note: this `render.rs` is in the **`quarto`** crate, not `quarto-core`.)

**Design: `fixed_renderable_set ∪ statically_claimed_extensions`.**

- **Corollary 0 — extension knowledge must move above the walk.** In `ProjectContext::discover`
  (`project/mod.rs:867`) the walk (`discover_project_files`, `:935`) runs before
  `discover_extensions_and_build_registry` (`:950`) — so discovery cannot yet know what engines declare.
  Split extension *discovery + `contributes` parsing* (cheap, pure, no host) above the walk; leave
  *registry finalization* (needs `binary_dependencies` + host) where it is — **and preserve P1.1's
  `set_project` threading** in the finalization half (`build_engine_registry`, `:565`, `set_project` at
  `:713`). Discovery needs only the parsed `EngineContribution::External` `claims_files`, not the
  `Arc<EngineRegistry>`.
  **Concrete cut:** `discover_extensions_and_build_registry` (`:776-822`) is one fn with **two**
  separate `#[cfg(target_arch = "wasm32")]` pairs — `builtin_dir` (`:786-792`) and registry
  construction (`:801-819`). The clean split line is `:799`/`:801`: `discover_extensions` (the cheap,
  host-free parse) produces the extensions at `:799`; only `build_engine_registry` (`:801+`) consumes
  `binary_dependencies`/`output_dir`/`config`. Extract the parse half (**carrying the `builtin_dir`
  cfg pair with it**) so it runs before the multi-file walk (`:935`), compute `RenderableExtensions`
  from its result, thread that set into `DiscoveryConfig`, then call `build_engine_registry` after the
  walk as today. **Both callers take the split signature** — `discover` (`:950`) and single-file
  (`:991`) — but only the project `discover` path feeds `RenderableExtensions` into a walk
  (`discover_project_files`); single-file render has no walk (it already admits its one `.echo`,
  P3-1b), so it threads the split through without computing a discovery set. The set computation must
  be target-agnostic (no
  new WASM-cfg branch) — WASM builds an empty registry but still discovers `.qmd` (`FIXED` alone).
- **Corollary 1 — discovery takes a resolved extension *set*, never the registry.** `DiscoveryConfig`
  (`discovery.rs:41`) carries only resolved values (`project_dir`, `output_dir`, `render_patterns`).
  Add one field `renderable_extensions: &RenderableExtensions` — a normalized-set newtype living in
  `discovery.rs`, computed by the caller as `FIXED_RENDERABLE ∪ engine claims-files extensions`.
  Discovery stays a pure path/string module. Do **not** thread `Arc<EngineRegistry>` in.
  **Both `RenderableExtensions` and the `FIXED_RENDERABLE` const (= `{"qmd"}`) are introduced here** —
  neither exists yet; `FIXED_RENDERABLE` replaces the current hardcoded `"qmd"` in `has_qmd_extension`.
  > **Forward note (Plan 7a):** 7a extends this seam to (a) union **built-in** engines' static claim
  > declarations (read as *data*, still without launching them — the rule sharpens from "never the
  > registry" to "never *launch*; do read static declarations"), and (b) admit **content-pattern**
  > claims by reading candidate files and evaluating a native regex. 1c.2 lands the extension-only,
  > External-only tier; 7a is purely additive over the same `RenderableExtensions` seam.
- **Corollary 2 — one predicate, one normalization.** The `"qmd"` literal lives in exactly one helper,
  `has_qmd_extension` (`discovery.rs:121`), with two callers (`is_renderable_qmd:84`, `walk_rec:329`;
  the glob machinery is already extension-agnostic). Replace it with `ext_in_set(path, set)` and thread
  the set through `walk_qmd` (`:298`) and `walk_rec` (`:305`) — they take no `DiscoveryConfig` today, so
  gaining the set is most of the diff. `path.extension()` is undotted and P4 stores declared extensions
  undotted-lowercase, so `RenderableExtensions` construction only lowercases candidates and *asserts*
  the declared-side normalization.
  **Both discovery paths are covered by this single threading.** `discover_project_files` (`:57`) has
  two branches: empty `render:` → `walk_qmd` (`:65`); explicit `render:` → `expand_patterns` (`:67`).
  But `expand_patterns` (`:148`) *re-seeds from* `walk_qmd` (`:160`) and then glob-matches — so once
  `walk_qmd` takes the set, the compiler forces `expand_patterns` to forward it, and both paths admit
  engine extensions through the same seed. Both branches also re-filter through `is_renderable_qmd`
  (`:71` → `has_qmd_extension:84`), so the set must reach `is_renderable_qmd` too (it already holds
  `&DiscoveryConfig`). **Intent (state in P2 so it isn't later filed as a bug):** an engine file is
  admitted by the *same* `render:` machinery as `.qmd` — it renders iff it is walked-in (default
  project, no `render:`) **or** matched by a `render:` glob. A qmd-only render list (e.g.
  `render: ["**/*.qmd"]`) will **not** surface an `.echo`; the user must include it (`"*.echo"`,
  `"**/*"`, or the explicit path). This is correct, not a gap — but it is unobvious, so say it.
- **Corollary 3 — admitted ⟹ statically claimed (coherence by construction).** Discovery admitting
  `a.echo` and `EngineClaimsFileStage` claiming it must be driven by the same declared data through the
  same match rule. **Decision: a single free `pub fn` helper, co-located with `lookup_static_claim`**
  (`extension/types.rs`, next to the declared data and the existing claim helpers) that takes an
  `&EngineContribution` and returns the normalized claimed extensions (each `FileClaim`'s `extension`).
  A **free fn, not a method** — every claim helper in that file is a free `pub fn`
  (`lookup_static_claim`, `static_claim_to_language_claim`, `combine_claims`,
  `engine_contribution_missing_fields_warning`); match the style. **Name it off the `static_claim`
  prefix:** that family is the `claims:` **language**-claim axis, a *different* thing from
  `claims-files` **file** claims, and reusing the prefix would confuse the two. Use e.g.
  `claimed_file_extensions`. It contributes **nothing** for `EngineContribution::Reorder` and for
  `External { claims_files: None }` (the "fall back to dynamic" case) — such engine files are simply not
  discovered, which is correct under non-enforcement (do **not** invent a discovery-time fallback-load
  path). The caller (`project/mod.rs`) uses it to build the discovery set (keeping `discovery.rs`
  engine-ignorant); `TsEngine::claims_file`'s static path (`ts_engine.rs:718`) consults the same
  normalized `claims_files`. The admission axis is **`claims-files`** (definitively owns), not
  `file-extensions` (can-handle). Divergence needs no new error surface: an admitted-but-unclaimed
  non-qmd file hits the existing §10 case-1 loud failure ("can't determine execution engine" —
  `test_p2_11_unclaimed_non_qmd_errors`, `engine_claims_file.rs:541`). Per the non-enforcement directive
  we never verify what engines execute.
  > **1c.2 vs 7a:** here every `claims-files` entry is *unconditional* (extension-only), so
  > "admitted ⟹ claimed" is trivially true. 7a preserves the invariant when an entry gains a
  > `content-pattern`: admission then evaluates the *same* predicate the claim stage does (extension +
  > pattern), so admit ⟺ claim still holds by construction.
- **Corollary 4 — extension axis here; content axis is Plan 7a's (resolved).** `claims-files` (in 1c.2)
  is an unconditional extension set (match `ext ∈ list`, `ts_engine.rs:718`). The content axis (percent
  scripts `# %%`, R spin) is a **static content pattern** — its "sniff-at-discovery decision," deferred
  in the original draft, is **answered by Plan 7a**: yes, statically, via a native regex evaluated at
  Pass-1 with zero engine load. **Plan 7 owns the *conversion*** (`markdown_for_file`); **Plan 7a owns
  the *claim*** (the pattern). 1c.2 lands the unconditional-extension tier the pattern tier extends.
- **Corollary 5 — extension overlap is a downstream claim concern.** If an engine declares `.qmd` (or a
  `.md` that `FIXED` also holds), the set dedups and discovery admits once; *which* engine owns the file
  is decided downstream by claim + engine resolution. Discovery is purely additive to the admission set.
- **Corollary 6 — composition with the website epic is a one-line interface agreement.** The website
  epic grows `FIXED_RENDERABLE` `{qmd} → {qmd, md, ipynb, …}`; this epic adds the engine-declared
  members. It is a set union: ship with `FIXED = {qmd}` unchanged (every existing discovery test stays
  green), agree now on the single `RenderableExtensions` seam in `DiscoveryConfig`, and either epic can
  land first. (This is the path by which **`.ipynb`** — a built-in-jupyter, always-a-document extension —
  becomes project-discoverable; it is *not* an engine `claims-files` member.)

**Test (binds plan-1c P2-16 project-level seam) — positive assertions only:** a `.echo` in a project
render → its `ProjectIndex` entry (via `ProjectIndex::lookup_by_source`; the render→index flow is
exercised by the integration tests `project_pipeline.rs` / `render_page_in_project.rs`, whose production
source is `pass2_renderer.rs` / `orchestrator.rs`) has the converted doc's title/outline. Extend echo's
`markdownForFile` to prepend a heading (`# Echoed: <basename>`) so converted-vs-raw is distinguishable
and the fixture body has no heading of its own. **After editing `src/echo-engine.ts`, rebuild the
committed `dist/echo-engine.js` via `q2 build-ts-extension` (tier-3 source map) and commit it — the E2E
tier runs the committed bundle (§ JSR / offline test policy), so an unrebuilt bundle makes T8
green-but-vacuous.** Add a companion `discovery.rs` unit test proving an engine-declared extension is
admitted while the existing exclusions (underscore/dot/README/output-dir) still apply. Do **not** test
the miss path (non-enforcement). Seams: T6 (walk path) / T6b (`render:`-pattern path) / T7 (newtype) /
T7b (`claimed_file_extensions` axis contract) / T8 (index e2e) / T8b (dispatcher gate).

---

## P4 — polish / hardening

### Restructure `claims-files` into typed entries + parse-time normalization

`claims-files` is a **file-claim** surface: the match is `ext ∈ list` (`ts_engine.rs:718`), and a census
of every Q1 engine's `claimsFile` (knitr, jupyter, markdown, julia) confirmed the claim is
`extension + (optional) content-pattern` — never a filename/glob, never a must-load operation. The name
`claims-files` is therefore correct (the earlier plan to rename it `claims-extensions` is **withdrawn**).
What P4 changes is the **value shape**: from a flat `Vec<String>` to typed entries, so Plan 7a can add a
`content-pattern` field **additively** with no second migration.

**The typed entry.** `claims_files: Option<Vec<FileClaim>>` where `FileClaim { extension: String }`
(undotted lowercase). A **bare-string shorthand** is accepted (`- .echo` ≡ `- {extension: .echo}`) via
`#[serde(untagged)]`, so existing fixtures that write `claims-files: [.echo]` / `claims-files: []` stay
valid unchanged. (Plan 7a grows `FileClaim` to `{ extension, content_pattern: Option<CompiledPattern> }`.)

**No published extensions**, so there is no external blast radius. Internal touchpoints (line refs as of
2026-07-06):

- **Code:** the YAML parse (`read.rs:425`, currently `parse_string_list` — replace with the typed
  parser), the Rust field + doc (`extension/types.rs:119-121`), `ts_engine.rs` (147, 172, 189, 304, 982,
  997 — the `claims_files` field/param/init/consumer sites now carry `FileClaim`s; `:718` matches
  `claim.extension`), `project/mod.rs` (667, 704, 1500, 1517), `engine_claims_file.rs` (candidate/match),
  the validation strings + missing-field warning (`extension/types.rs:249-294`), and
  `engine-resolution.md §3.3`.
- **Tests + fixtures (mostly unchanged — the name and bare-string form survive):** the echo fixture
  `_extension.yml:14`; `engine_registry_build.rs` (67,152,213,231,321,392); `echo_engine_e2e.rs:254`;
  `marimo_engine_e2e.rs` (incl. `+ "\n      claims-files: []\n"` at :196 / assertion :206); the
  **TypeScript** spec `q2-preview-spa/e2e/engine-capture-splice-marimo.spec.ts` (incl.
  `out += '      claims-files: []\n';` at :197). These keep the `claims-files` key; only their Rust-side
  parse target changes type. Fixtures may migrate to the explicit `{extension: …}` form or keep the
  bare-string shorthand — either parses.
- **Warning test:** `warning_names_missing_claims_files_field` (`extension/types.rs:608`) — the field
  name is unchanged, so this stays green.

**Normalization.** Today `parse_contributes` (`read.rs`, from `:162`) stores YAML verbatim — there is
**no case-folding in `read.rs` at all**; only the *candidate* (per-file) side is lowercased downstream
(in the claim/resolution path, not here), so a declared `file-extensions: [".Echo"]` silently never
matches. Fix at parse time
for **both** `file-extensions` and the `claims-files` entries' `extension`: accept dotted or undotted
input, store canonical **undotted lowercase**.

**Dots are a wire-only detail — one adapter.** The canonical Rust-side form is undotted (it agrees with
`path.extension()`, and P2's `RenderableExtensions` compares candidates with no per-file dot transform).
The JS/wire contract stays dotted (Q1 `extname()`; engines compare `ext === ".echo"`). **Today the only
re-dotter is `EngineClaimsFileStage` (`engine_claims_file.rs:118`, `format!(".{ext}")`); the `ts_engine`
wire adds no dot of its own — it *forwards whatever it is given* (today an already-dotted `ext`, since
`:118` dotted it upstream and it is stored dotted in `static_file_answers`, `:724-725`). "Undotted"
here describes the wire's own behavior, not the value flowing through it today.** Land as: **remove the
`format!(".{ext}")` re-dot at `engine_claims_file.rs:118`** so the stage passes the ext undotted; the
stage and all Rust-side matching then compare **undotted** ext against undotted stored extensions. A
single **free `fn to_wire_ext(ext: &str) -> String`** (preferred — one call site, matches the "one
adapter" framing; a serde newtype on the wire message is an acceptable alternative) re-adds the dot at,
and only at, the two Rust→TS seams — the `ToEngine::ClaimsFile` construction (`ts_engine.rs:739-743`)
and the synthetic-file load guard (`:313`, which becomes `format!("x{}", to_wire_ext(ext))` → `x.echo`,
not `xecho`). This collapses dot-handling to one call site. The
invariant states in one sentence: **extensions are undotted everywhere; the wire adapter adds the dot.**

**Test:** an uppercase-declared extension (dotted or undotted in YAML, bare-string or object entry)
claims a lowercase file; the captured wire messages still carry dotted lowercase; the synthetic-file
validation still catches an over-declared sniffing engine. Seams: T9/T10/T11 (T11 asserts wire messages
via `MockTransport`; the echo E2E staying green is the real-JS guard).

### `EngineRegistry::contribution_order` encapsulation — read side

The field is currently **`pub`** (fully public, `registry.rs:62`); this **narrows** it to `pub(crate)`
+ adds `pub fn contribution_order(&self) -> &[String]`. The **only out-of-crate reader** is the
integration test, at **four sites forming two assertions** (`engine_registry_build.rs`: `:120` + `:122`
are one `assert!`; `:330` + `:333` are the other) — migrate **all four** reads to the getter (the two
assertion conditions *and* the two format-arg reads). Confirmed via `grep -rn '\.contribution_order'`:
no other external reader exists. In-crate touchpoints keep **direct field access** under `pub(crate)`
and need no migration — production reads (`project/mod.rs:752`, `engine/resolution.rs:88`), the
production write (`project/mod.rs:745`, `registry.contribution_order = order`), and the unit-test
pushes (`engine_claims_file.rs` ×5, `resolution.rs:1306`). So this is **read-side** encapsulation only
(production code still writes the field directly). **Defer the write API** (push/dedup/splice) to Plan
4b-C, where the `_quarto.yml engines:` splice lands and its ordering contract shapes the method names.
Seam: T12 (the two assertions keep their assertions after all four reads are migrated).

### (Optional) crash-path E2E

A fixture whose engine `Deno.exit(1)`s mid-execute → assert the render fails with a `ProcessCrashed`-shaped
error carrying captured stderr, no leaked subprocess. Exercises the reader-thread EOF→broadcast against a
real process (only `MockTransport` covers it today). Seam: T13.

---

## Design ratification — resolution-driven handoff (carried from 1c)

The engine sequence is derived **once, from the original parsed AST**: an engine is in the sequence only
if it owns ≥1 language present in the source. This is intended, documented in `engine-resolution.md`
§6.1 (with §4.3 fallback gating, §8 file-claim single-engine, §11/bd-r8n4r nested-handoff), alongside
the §10 non-enforcement statement ("capability is judged from declarations, never from execution
results").

**Ruled out (accepted):**
1. **Injected-cell handoff to an engine absent from the sequence.** Engine A emits a runtime cell in
   language L whose only owner is engine B, but B was excluded because the original source had no L
   cells. The sequence is fixed pre-execution, so B never runs.
2. **An explicitly-listed engine that owns nothing originally is dropped.** `engines: [knitr, customX]`
   where customX's language never appears: customX contributes nothing and cannot receive
   runtime-injected cells. The fallback net does not save this — per §4.3, T4 adds jupyter only for
   *implicit* sequences.

**Still works:** handoff between engines that both own something in the original AST (knitr re-emits a
`{python}` cell and jupyter runs it, because the doc already had `{python}`); knitr↔reticulate interop;
jupyter-as-`Fallback(0)` in implicit docs.

**Why accepted:** resolving (1)/(2) needs runtime sequence growth (re-resolving mid-execution), which the
resolution-driven + replay model deliberately avoids (§6.2: replay drives from recorded captures, not
re-resolution — mid-execute re-resolution would break the determinism guard and the eventual freeze
cache-key). Tracked as a live-preview limitation (bd-r8n4r); the valuable common handoffs all work.

---

## JSR / offline test policy

`@quarto/api` and `@quarto/types` are **not** published on jsr.io, and we do not test against jsr. Every
engine build in tests routes through the **tier-3 workspace source map**
(`resources/extension-build/deno.workspace.json`, which remaps `@quarto/*` → local `../../ts-packages/…`
source) and commits a **pre-resolved** `dist/*.js` bundle (no `jsr:`/`@quarto` specifiers survive
bundling). E2E tests run the committed bundle; the one deno-bundling test
(`build_ts_extension_e2e.rs:59`) selects tier-3 by construction (it plants the temp copy under
`workspace_root/target/` so `find_workspace_root` resolves to local `ts-packages/`); T4/T5 are pure
config-resolution. **New engine tests MUST follow this — never rely on the shipped tier-4/jsr path at
test time.**

**Installed-binary tier-4 — accepted deferral.** The shipped `deno.json` embeds `jsr:@quarto/*`
specifiers that 404 until publication, so an installed binary builds extensions via `--config` (tier-1);
the zero-config tier-4 path awaits jsr publication. Plan-1c SC "works from an installed binary" is met
for `--config`; zero-config is deferred, not a blocker. The tier-4 `deno bundle` is permanently
untested-in-CI. When jsr publication happens, pin the embedded specifiers (`jsr:@quarto/api@^X.Y`) — a
release-runbook line, not a code change.

---

## Test Seam Spec

Tiers: **unit** = pure Rust, no subprocess; **msg** = Rust unit with `MockTransport` capture; **e2e** =
Deno-gated integration (skip when `deno` absent; real `render_to_file`; committed bundle). Seams are
frozen once green — never edited to go green. T1–T5 and T14 are **landed with P1**; T6–T13 are the work
to execute (order T9–T11 → T6/T6b/T7/T7b/T8/T8b → T12 → optional T13, per the execution order above).

| # | item | tier | status | seam / revert-hunk → RED |
|---|------|------|--------|--------------------------|
| T1 | P1.1 | e2e | ✅ landed | temp project (`_quarto.yml` `output-dir: out` + `engines: [echo]` + `{echo}` cell) → `render_to_file`; echo's `execute` emits `CONTEXT_JSON_START{…}CONTEXT_JSON_END`. Revert the context threading (`build_engine_registry:565` → `set_project:713`) → `project_dir` null / `output_dir` wrong / `config` missing keys → RED |
| T2 | P1.1 | e2e | ✅ landed | temp dir, no `_quarto.yml`, `file.echo` single-file render; same channel; binds the `is_single_file` field-source (`discover:903`). Revert → stays `false` → RED |
| T3 | P1.1 | unit | ✅ landed | `ConfigValue → TsMetadataValue` helper + config-map builder; input `engines: ["knitr", {path: "x.js"}]`, `output-dir: out`. Revert the `Mapping`/`as_plain_text` arms → entries drop/misshape → RED |
| T4 | P1.2 | unit | ✅ landed | `config_wins_without_workspace_and_never_touches_shipped_provider` (`build_ts_extension.rs:519`); `--config X` + rest `None` + provider `\|\| panic!()`. Re-eagerify tier 4 → provider fires → RED |
| T5 | P1.2 | unit | ✅ landed | `tier4_materializes_embedded_shipped_config` (`:549`); all tiers absent → tier 4 selected. Revert the `include_str!` embed → tier 4 yields error → RED. Asserts content == embedded `deno.json` + parses as JSON (no `deno bundle`) |
| T14 | P1.1b | e2e | ✅ landed | extends T1: frontmatter `execute: {daemon: false}` + a custom top-level key; echo's `execute` emits `FORMAT_JSON_START{…}FORMAT_JSON_END` echoing `options.format.execute` + the `format.metadata` key. Revert `metadata` threading (`ts_engine.rs:394`) → empty execute / no custom key → RED |
| T6 | P2 | unit | ▶ to do | `discover_project_files` with `renderable_extensions: {qmd, echo}` **and empty `render_patterns`** (walk path) over `a.echo`, `_draft.echo`, `.hidden.echo`, `out/b.echo`, `notebook.ipynb`. Revert `ext_in_set` threading (restore `has_qmd_extension`) → `a.echo` excluded → RED. Positive invariant (same run): `_draft.echo` / `.hidden.echo` / `out/b.echo` stay EXCLUDED, `notebook.ipynb` stays excluded (not in set) — binds that engine extensions flow through the *same* exclusion predicate, not a bypass branch |
| T6b | P2 | unit | ▶ to do | **pattern path:** same as T6 but `render_patterns: ["*.echo"]` (or `["a.echo"]`) → `a.echo` admitted via `expand_patterns`. **Shares T6's revert hunk** — reverting the `ext_in_set` threading reddens *both* paths (each seeds via `walk_qmd`→`walk_rec:329` and post-filters via `is_renderable_qmd:84`); there is no independent `expand_patterns` hunk today (the compiler forces the forward when `walk_qmd` gains the set param). T6b's distinct role is **path coverage + a forward-guard**: a *future* extension-filter added inside `expand_patterns`'s glob layer would redden here while T6 stayed green |
| T7 | P2 | unit | ▶ to do | `RenderableExtensions` newtype: build from canonical `["echo"]`; candidates `A.ECHO`, `a.echo`. Revert candidate lowercasing → `A.ECHO` rejected → RED |
| T7b | P2 | unit | ▶ to do | **`claimed_file_extensions` coherence — the axis contract (Corollary 3).** Three synthetic `EngineContribution`s: (a) `External { file_extensions: Some([".jl"]), claims_files: None }` → `[]`; (b) `External { file_extensions: Some([".py"]), claims_files: Some([{extension:"echo"}]) }` → `["echo"]` (reads `claims-files`, ignores the *disagreeing* `file-extensions`); (c) `Reorder { name }` → `[]`. Revert the helper to read `file_extensions` (or union both axes) → (a) returns `["jl"]` and (b) returns `["py","echo"]` → RED. **The one seam that catches a wrong-field helper:** echo's fixture declares the *same* value (`.echo`) on both axes, so T6/T7/T8/T8b cannot distinguish `claims-files` from `file-extensions` — only these disagreeing synthetic inputs bind that the admission axis is `claims-files` |
| T8 | P2 | e2e | ▶ to do | temp project + `a.echo` (echo's `markdownForFile` prepends `# Echoed: <basename>`); full render; assert `ProjectIndex` entry has title `Echoed: a.echo`. **Two named reverts:** (1) revert the discovery-set union → no index entry → RED (not *discovered*); (2) revert echo's `markdownForFile` heading → entry exists but title ≠ `Echoed: a.echo` → RED (discovered but not *converted*). Fixture body has NO heading, so raw fall-through can't fake the title — title discriminates conversion, entry-existence discriminates discovery |
| T8b | P2 | unit | ▶ to do | **dispatcher gate (`quarto` crate):** `classify_inputs` given an explicit `a.echo` arg in a project whose discovery admits `echo` → **not** rejected (no `DispatchError::NotInRenderList`). Place near `classify_qmd_excluded_by_render_list_errors` (`render.rs:1592`). **Shares T8's revert hunk** (the discovery-set union): reverting it drops `a.echo` from `project.files` → `NotInRenderList` at `render.rs:318` → RED. Distinct role: binds the **explicit-file-arg entry** (`render.rs:318`) that T8's whole-project render never reaches |
| T9 | P4 | unit | ▶ to do | typed `parse_contributes` (`read.rs`): `claims-files` as bare-string `[".Echo"]` AND object `[{extension: ".Echo"}]`; `file-extensions` `[".ECHO"]`. All normalize to undotted-lowercase `echo`. Revert the parse-time fold → stored `".Echo"` ≠ `"echo"` → RED; revert the untagged accept → object form fails to parse → RED |
| T10 | P4 | unit | ▶ to do | `EngineClaimsFileStage` via `make_ctx_with_registry` (`engine_claims_file.rs:431`) with `claims_files: [{extension: "echo"}]`; run on `X.ECHO` → claimed. Revert the stage's candidate alignment (`:118` still re-dotting while storage went undotted) → unclaimed → RED |
| T11 | P4 | msg | ▶ to do | `MockTransport`: capture `ToEngine::ClaimsFile` (no static claim) + the load-validation message (static claim). Revert `to_wire_ext` → captured `ext == "echo"` / synthetic `xecho` instead of `".echo"`/`"x.echo"` → RED. Real-JS guard = echo E2E staying green |
| T12 | P4 | — | ▶ to do | `contribution_order` getter: mechanical; all **four** migrated integration reads (`engine_registry_build.rs:120`/`:122`/`:330`/`:333`) keep their **two** assertions |
| T13 | P4 (opt) | e2e | ▶ to do | crash fixture: marker to stderr then `Deno.exit(1)` mid-execute; assert `ProcessCrashed`-shaped error carrying the stderr marker, no orphan. Revert the reader-thread EOF→broadcast (`ts_process.rs`) → hang → RED by nextest timeout. Assert BOTH error shape and stderr content |

**Accepted-untested (logged):**
- **Admitted-but-unclaimed miss path** — the claim stage already hard-errors it (§10 case-1,
  `test_p2_11_unclaimed_non_qmd_errors`, `engine_claims_file.rs:541`); no new test, per non-enforcement.
- **Tier-4 JSR `deno bundle`** — permanently untested-in-CI (see § JSR / offline test policy); the embed
  + selection are covered by T4/T5.
- **Cross-render context freshness** — Plan 5's; no test in either direction.
- **`engines:` `{path}`-entry runtime semantics** — Plan 4b Task 9's; we pass values only (T3 shape).
- **Content-pattern claims** — Plan 7a's; 1c.2 lands only unconditional extension claims.
- **`file-extensions` can-handle *consumption*** — T9 binds the parse-time normalization (storage side);
  that a normalized `.ECHO`-declared engine then actually pre-filters `a.echo` at resolution relies on
  existing resolution tests (lowercase fixtures). `file-extensions` is the can-handle pre-filter, not
  P2's admission axis (that's `claims-files`, bound by T7b), so this is low-stakes — logged, not seamed.
- **Empty-extension `to_wire_ext`** — an extensionless file: `to_wire_ext("")` → `""` (preserves the
  pre-existing "empty stays empty" at `engine_claims_file.rs:116`), and discovery excludes extensionless
  files (`ext_in_set` is false for a `None` extension). No seam.
- **`RenderableExtensions` set-dedup (Corollary 5)** — an engine declaring `.qmd` (or a `.md` already in
  `FIXED`) dedups structurally via the set union; testing it is testing the set type. Logged.

---

## Notes

- Execute like 1c: SDD (implementer + reviewer per task, strict TDD, named reverts). **P2 is the large
  item** — don't batch it with the shovel-ready ones.
- **P4 restructure runs before P2** (execution order): both consume the declared extension lists,
  Corollary 3 requires them to agree, and P4 establishes the parse-time canonical form (undotted
  lowercase).
- **P4 restructure runs before Plan 4b Phase-A** so 4b's fixtures author the typed `claims-files` entry
  from the start (no re-migration when Plan 7a adds `content-pattern`).
- **P2 ↔ website epic is an interface agreement, not an ordering dependency** (Corollary 6): a set union
  (`FIXED_RENDERABLE ∪ engine-declared`) over the single `RenderableExtensions` seam; either epic can
  land first. `.ipynb` arrives via the website epic's `FIXED` growth, not via `claims-files`.
- **Content claims are Plan 7a** (`2026-07-07-plan7a-static-content-pattern-claims.md`): it adds a
  `content-pattern` field to `FileClaim`, a native regex evaluated at both the claim stage and Pass-1
  discovery, and built-in-engine static declarations — extending, additively, the seam 1c.2 lands.
- **DQ-1…DQ-7 are defined in `engine-api-surface.md`** (decisions § + build checklist); DQ-5's wire
  shape is also in the grand plan §LaunchEngine.
