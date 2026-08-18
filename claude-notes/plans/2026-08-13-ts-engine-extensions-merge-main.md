# ts-engine-extensions ← main: Merge Runbook (2026-08-13)

> **What this is:** a runbook for bringing `feature/ts-engine-extensions` up to
> date with `main` via a **merge**, plus the design changes that merge forces us
> to make. It is written to be executed by someone with **no prior context** —
> every decision below is recorded with its rationale so you do not have to
> re-derive it. Work top to bottom. **Do not push** (see Global Constraints).

**Three phases, and the split matters:**

- **Phase A — the merge.** Pure reconciliation: resolve 23 conflicts, get the
  workspace green, commit the merge. One exception noted in A5.
- **Phase B — the design changes.** Five changes the merge forces but which are
  *not* conflict resolutions. Each is its own commit, on top of the merge, so
  the merge stays reviewable and each change stays bisectable.
- **Phase C — verification and PR prep.**

## State at start

| | |
|---|---|
| Branch tip | `c667fac83` (runbook commit) — feature work ends at `89f068364` |
| `main` tip | `0dcd7e831` |
| Merge base | `65a888b0a` (2026-07-24) |
| Commits ahead / behind | 12 / **340** (10 feature + 2 runbook) |
| Branch diff | 491 files, +104,835 / −3,498 |
| — of which conflict | **23 files, +5,326 / −222** (all `crates/quarto-core` + repo files) |
| — of which clean | 468 files, +99,509 (mostly new `ts-packages/`) |
| Conflict hunks | **50** |
| Safety branch | `ts-engine-extensions-backup-premerge-20260813` |

## Why merge, not rebase (measured 2026-08-13)

A naive `git rebase main` was run in a throwaway worktree: it stops at **7 of 10
commits**, 23 file-resolutions, and the same hub files recur across stops
(`engine_execution.rs` at 3, `engine/context.rs` at 3, `project/mod.rs` at 2,
`extension/read.rs` at 2). You would resolve the same collision several times and
a wrong intermediate resolution would propagate silently into later stops. A
merge surfaces all 23 once.

## rerere is OFF, deliberately — do not turn it on

`git config rerere.enabled false` is set in this worktree. **Leave it.**

The repo's `.git/rr-cache` holds ~90 resolutions from 2026-06-05 and 2026-07-24.
When rerere was enabled for a trial merge on 2026-08-13 it replayed 17 of the 23
conflicts from that historical cache, and because those resolutions predate
main's last 340 commits they **systematically reverted main's newer work**.
Verified reversions in the replayed tree: `Extension::title` back to `String`,
`DOCUMENT_PROFILE_VERSION` back to 8, `listing_content_globs` back to
`Vec<String>`, and — worst — `project/discovery.rs` with main's entire rewrite
deleted. The replay was not uniformly wrong (it correctly kept both
`authors_structured` and `engine_resolution`), which is exactly what makes it
dangerous: right often enough that you stop checking.

rerere also measurably contributes **nothing** on a first pass (0/23 hits when
the cache was cold), so disabling it costs nothing here. Its value is on retries.
If you enable it later, `git rerere forget <path>` the stale paths first.

> **Note:** rerere auto-activates whenever `.git/rr-cache` exists, even with
> `rerere.enabled` unset. The explicit `false` is what actually disables it.

## Global Constraints

- **NEVER push without explicit user permission** (project GIT PUSH POLICY).
  This runbook ends at "green locally + commits prepared."
- **Never resolve a hub file by taking a whole side.** Every hard conflict needs
  the branch's structure **and** main's new feature. Measured cost of getting
  this wrong: a naive `--theirs` pass produced **54 compile errors across 14
  files, 10 of which merged cleanly** — and dropping main's `is_external_url`
  re-export from `quarto-util` broke `quarto-brand`, a crate that never appeared
  in the conflict list.
- **`.braid/snapshot.jsonl` is regenerated, never hand-merged**: take either
  side, then `braid export > .braid/snapshot.jsonl`.
- **Lockfiles are regenerated, never hand-merged:** `Cargo.lock`,
  `crates/wasm-quarto-hub-client/Cargo.lock`, `package-lock.json`.
- Run from `/Users/gordon/src/q2/.worktrees/ts-engine-extensions`.
- Rollback: `git merge --abort`, or
  `git reset --hard ts-engine-extensions-backup-premerge-20260813`.
- **Merge `--ours`/`--theirs` orientation:** in a merge, `--ours` = **the
  branch**, `--theirs` = **main**. This is the reverse of a rebase. The prior
  runbook (`2026-07-22-…`) documents the *rebase* orientation — do not copy it.

---

# Design decisions (settled 2026-08-14/16 with Gordon)

These are **settled**. They are recorded with rationale so you can implement them
without re-litigating. If something here looks wrong, raise it — do not silently
deviate.

## D1 — Engine-claimed extensions widen the default render pattern

**Context.** Main and the branch have opposite models of what makes a file an
input:

- **main** (bd-6d2wj4zp, 2026-08-07): an extension allow-list (`.qmd` + `.md`)
  as gate 1, plus `project.render` patterns as gate 2, defaulting to
  `DEFAULT_RENDER_PATTERN = "**/*.qmd"`. Its documented invariant: *omitting
  `project.render` is exactly equivalent to writing `render: ["**/*.qmd"]`*, and
  `.md` therefore renders only under an explicit pattern. Main's own research
  notes this is a **deliberate departure from Quarto 1**, which decides input-ness
  by "some engine claims the file."
- **the branch** (plan 1c.2): engines claim extensions; `render_patterns.is_empty()`
  means walk everything and admit by the claimed-extension set. This *is* the Q1
  model.

**Decision.** Engine-claimed extensions **widen the default pattern set**. With no
positive `project.render` pattern, the effective set becomes `**/*.qmd` plus one
`**/*.<ext>` per **statically** claimed extension. `.md` stays opt-in (main's
divergence is preserved). It is explicitly OK to override main's static-list
decision here — but note Carlos was working on this as recently as 2026-08-07, so
the change should be visible to him in review.

**Why not just widen gate 1.** An earlier draft of this plan said "make
`has_renderable_extension` consult the claimed set." That is **wrong on its own**:
it widens gate 1 while gate 2 still defaults to literally `**/*.qmd`, so a
claimed `.echo` would pass the extension filter and then be silently dropped by
the pattern match. Plan 1c.2's feature would be inert in any project without an
explicit `render:` key, and no test on either side would catch it.

**Where the widening goes.** In `effective_render_patterns`, **not** in the
diagnostics path. `render_pattern_diagnostics` is called from the orchestrator
with `self.project.config.render_patterns` — the *user's* patterns — so synthetic
per-extension globs never reach it and cannot produce spurious `Q-5-13`
"pattern matched no renderable files" warnings. Preserve that.

Note the signature must change: main's `effective_render_patterns(user: &[RawGlob])`
(`discovery.rs:124`) takes no config and so cannot reach the extension set.

**Provenance.** Synthetic globs need a `SourceInfo`. Main already has the
precedent — `effective_render_patterns` builds the default with
`SourceInfo::generated(By::programmatic_config())`. Use the same.

**Which extensions widen — the fixed/claimed distinction.** Only
**engine-claimed** extensions emit a synthetic glob. `FIXED_RENDERABLE` members
must not, or `.md` would become a default pattern and undo bd-6d2wj4zp. Two
concrete consequences the implementer must handle:

- `RenderableExtensions` (`discovery.rs:48`) is a flat
  `HashSet<String>` holding `FIXED_RENDERABLE ∪ claims`, so it **cannot answer
  "which members came from an engine."** Give the newtype a second field holding
  the engine-claimed list (and an accessor), rather than subtracting
  `FIXED_RENDERABLE` at the use site — subtraction is fragile and would silently
  change meaning if the fixed set ever grows.
- The widening loop must **exclude the native set** (`""`, `qmd`, `md`,
  `markdown`) **unconditionally** — do not rely on B3 having landed. Phase A ships
  before Phase B, and in between an engine claiming `md` would otherwise make
  `**/*.md` a default pattern. Nothing breaks the build and no existing test
  catches it.

## D2 — Static claims only participate in discovery; dynamic claimers fall through silently

`ts_engine.rs` supports `claims_files: None`, meaning the engine decides by
inspecting content rather than by extension. Discovery only ever sees paths, so
such engines cannot contribute discovery wildcards.

**Decision.** Only **statically** claimed extensions contribute wildcards under
D1. A dynamic claimer is not Pass-1 compatible for discovery, but it can still
claim and convert a file that discovery found some other way — e.g. one listed
explicitly under `render:`.

**Follow the existing precedent, and make it silent.** The branch already
implements exactly this two-tier pattern for *language* claims in
`crates/quarto-core/src/engine/resolution.rs`: a tabled engine answers load-free;
an untabled engine's `try_claims_language` returns `None` ("would load") which
`?`-propagates so Pass-1 aborts and falls through to Pass-2. That fall-through is
**silent** — `resolve_engines_pass1` returns `Option`, `None` means "fell
through," and the field doc calls it *advisory, not an error*; observability is
the `engine_resolution_pass1` trace target, not a diagnostic. Mirror this. **Do
not invent a warning** for the file-claim side.

## D3 — `SourceType` loses `Ipynb` and `Rmd`; the field becomes `Option<SourceType>`

**Context.** `SourceType` dates to 2026-01-06 (`806703bde`) and has had **zero**
behavioral consumers for most of its life. Its intended consumer was specified in
that commit's design doc as future work — a `PipelinePlanner` that would push a
`ConvertNotebook` stage when `source_type == SourceType::Ipynb`. It was never
built. bd-xxul (2026-04-24) then deferred non-`.qmd` extensions with the note
"once settled, extend discovery and the pipeline's `SourceType` handling."

**Decision.** q2 processes exactly two kinds of markdown: **qmd** (with code
execution) and **md** (without). Everything else is converted to qmd *before* the
parser by the conversion stage. A static, engine-specific format list does not
make sense once engines arrive as extensions, so `Ipynb` and `Rmd` are removed.

This **completes** the January design rather than discarding it: same insertion
point (pre-parse conversion), but dynamic engine dispatch instead of static enum
dispatch. Under this model bd-19nc56ao (`.ipynb`) becomes an engine that claims
`.ipynb`, and `.Rmd` becomes knitr claiming `.Rmd`.

**Blast radius is small** — verified on both branches, `Ipynb`/`Rmd` appear
nowhere outside their own unit tests. `SourceType` is not `serde`-derived (it
reaches trace JSON only via `format!("{:?}")`), so there is no wire or snapshot
compatibility concern.

**The field becomes `Option<SourceType>`.** With only two variants, the existing
`unwrap_or(SourceType::Markdown)` in `LoadedSource::new` stops being a shrug about
unknown formats and starts asserting "plain markdown, never executes" about a file
that is about to be converted and executed.

The resulting contract, stated precisely so the pass-through case isn't left to
guesswork:

- `LoadedSource::new` stores `SourceType::from_path(&path)` **directly**, with no
  fallback. Native files get `Some(Qmd)` / `Some(Markdown)` at load; anything
  else gets `None`.
- `None` means "unknown extension, not yet converted."
- `SourceConversionStage` sets `Some(Qmd)` on the conversion branch and leaves the
  pass-through branch alone (it is already correct from load).
- **Invariant:** after the conversion stage, `source_type` is always `Some` — an
  unclaimed non-native file never gets that far, because the stage hard-errors on
  it.

## D4 — Rename the stage for what it does; keep `claims_file` as the predicate

`EngineClaimsFileStage` → **`SourceConversionStage`**, `name()` →
**`"source-conversion"`**, file `engine_claims_file.rs` → `source_conversion.rs`.
Conversion is the action; claiming is only the predicate that selects it. The
name also stays accurate if built-in (non-engine) converters appear later, and it
reads correctly immediately before `parse-document`. It follows the house
convention (`metadata-merge`, `language-resolve`, `include-expansion`).

**Do NOT rename `claims_file` / `claimsFile`.** It is not internal:

- it is a wire message pair (`ToEngine::ClaimsFile` / `FromEngine::ClaimsFileResult`)
- it is a **required export of the public engine-author API** — `engine-loader.ts`
  throws `engine module … is missing required export: claimsFile` if an extension
  omits it

Renaming it would break every third-party engine. The engine claims; the stage
converts.

## D5 — Refuse engine claims on the whole native set, with `Q-2-50`

If an engine tries to claim a file q2 owns natively, **warn and ignore the
claim**, falling through to the normal pass-through path.

**Scope: the whole native set** — `""` (extension-less, treated as qmd),
`"qmd"`, `"md"`, `"markdown"` — not just `.md`. By D3's principle both markdown
kinds are q2's own; an engine claiming `.qmd` would bypass q2's parser entirely.

**This also closes a real bug.** Without it, the claims loop is ungated, so an
engine *can* claim `.md`. In the merged tree that file would be converted (its
`source_type` set to `Qmd`) and then have execution suppressed anyway, because
main's `Q-2-40` guard reads `SourceType::from_path(&ctx.document.input)` — the
*original* path, still `.md` — and emits a spurious "engine specification
ignored" warning. Neither side's tests catch this: main has no claiming engines,
the branch has no guard.

**Consequence: main's `Q-2-40` guard needs NO change.** With `.md` unclaimable
every case lines up on main's existing code. An earlier draft of this plan
proposed rewriting the guard's predicate to consult `source.conversion` — that is
now unnecessary. Leave `engine_execution.rs:275` as main wrote it.

## D6 — `DOCUMENT_PROFILE_VERSION` → 11, and stop bumping it on the branch

**What went wrong.** The branch minted version `8` during the 2026-07-24 catch-up
(folded into `ca1994fa2`, because a rebase lands conflict resolutions in the
commit being replayed). Main then independently used `8` for something else. The
same integer now denotes two different schemas:

| Version | On main | On the branch |
|---|---|---|
| 8 | `bd-v7ixzsp5` — `listing_content_globs` becomes `GlobPattern` | "authors_structured + engine_resolution coexist" |
| 9 | `bd-mt7a6uc4` — adds `resource_globs` | — |
| 10 | current | — |

**The rule going forward.** An unmerged branch never carries a bumped version
across catch-ups. Its claim is "main's current value, plus one for our added
field," recomputed at merge time. This is not a flaw in the versioning scheme —
it is branch discipline.

**Applied now:** main is at 10, the branch adds `engine_resolution`, so **11**. If
main moves again before the PR lands, it becomes main's-new-value + 1, **not**
11 + anything.

**Changelog hygiene:** delete the branch's "Bumped 7 → 8 in the
ts-engine-extensions rebase…" narrative. That intermediate state never existed
publicly. Replace it with a single `11:` entry recording `engine_resolution`
added on top of main's 10.

## D7 — Out of scope: conversion provenance (bd-zlemoc6w)

The TS host serializes a real source map for converted files and the wire type
carries it (`TsMappedStringWithMap.source_map`), but Rust drops it at three
points. `ParseDocumentStage` compensates honestly — it registers converted text
under `<foo.ipynb (converted by jupyter)>` — so this is a **quality gap, not a
correctness bug**. Filed as **bd-zlemoc6w** (p1). Gordon's call: **before filing
the PR, not part of this merge.**

Two notes for whoever picks up bd-zlemoc6w: the Rust-side drop site is
`ts_engine.rs:1006` (it takes `result.value` and discards `result.source_map`),
and the stage-side drop site — currently `.map(|(text, _source_info)| text)` in
`engine_claims_file.rs` — **moves to `source_conversion.rs` under D4**.

This runbook ends when Phase C is green. bd-zlemoc6w is scheduled *after* it and
*before* the PR; it is deliberately not a checkbox here, because nothing in this
runbook can satisfy it.

---

# Phase A — the merge

## A0. Start

- [x] `git status --short` → clean; confirm safety branch exists
- [x] `git config --get rerere.enabled` → must print `false`
- [x] `git merge main` → expect 23 conflicts
- [x] `git diff --name-only --diff-filter=U | wc -l` → expect 23

## A1. Regenerated files

- [x] Clear `.braid/snapshot.jsonl` (either side), then
      `braid export > .braid/snapshot.jsonl`

> **Lockfiles do not conflict in this merge.** `Cargo.lock`,
> `crates/wasm-quarto-hub-client/Cargo.lock` and `package-lock.json` all
> auto-merge — verified against the merge blob. The global constraint about
> never hand-merging them still stands if that changes; there is just nothing to
> do here today. `Cargo.lock` refreshes during A8's build.

## A2. Additive unions (low risk)

Keep **both** sides at each hunk; no side is discarded.

- [x] `.gitignore` — main's `.posit/assistant/` + the branch's extension-dist entries
- [x] `crates/quarto-core/tests/integration/main.rs` and
      `crates/quarto/tests/integration/main.rs` — union the `pub mod` lists, alphabetized
- [x] `crates/quarto-util/src/lib.rs` — union. **Must include main's
      `is_external_url`**: `pub use path::{is_external_url, is_rooted, to_forward_slashes};`
      plus the branch's `data_dir` / `runtime_dir` re-exports. Dropping
      `is_external_url` breaks `quarto-brand`, which never conflicts and so gives
      you no warning.
- [x] `crates/quarto-core/src/extension/mod.rs` — union `pub use` + the branch's
      native-gated build module
- [x] `crates/quarto-core/src/engine/mod.rs` — union `pub(crate) use`
      (main's `find_rscript` + the branch's additions)
- [x] `crates/quarto-core/src/project/mod.rs` **hunks 2–3 only** — both sides
      added test modules at the same spot; keep both. (Hunk 1 is A6.)
- [x] `claude-notes/designs/document-profile-contract.md` — reconcile to version 11
      (follows A4)

## A3. `Extension::title` sweep — take main's side

Main changed `Extension::title` from `String` to `Option<String>` (bd-8b0af414,
Q1-compat intake: a missing title is no longer an error). The branch still has
`String` plus a "title defaults to `id.name`" behavior that main **deliberately
reversed**. Verified: the branch's engine/extension code never reads `.title`, so
there is no hidden dependency.

- [x] `extension/types.rs` — `pub title: Option<String>`
- [x] `filter_resolve.rs`, `stage/stages/metadata_merge.rs`,
      `transforms/shortcode_resolve.rs` — `title: Some(name.to_string())`
- [x] `extension/discover.rs` hunk 1 — the `bad-ext` test fixture. The branch
      omits `contributes` with a comment saying title "defaults to the id name";
      main uses unparseable YAML. **Take main's side** (the branch's premise is
      what A3 is reversing). This file has **7** hunks, not 6.
- [x] `extension/discover.rs` hunks 2–7 — `title: Some("…".to_string())`
- [x] `extension/read.rs` hunk 1 — main's optional-metadata reads via `as_plain_text()`
- [x] `extension/read.rs` hunks 4–6 — **delete** the branch's
      `test_read_extension_missing_title_defaults_to_id_name`; keep main's
      `test_read_extension_missing_title` asserting `title == None`

## A4. `document_profile.rs` — version 11

Per **D6**.

- [x] Keep **both** field sets and both supporting types (`authors_structured`
      from main, `engine_resolution` from the branch)
- [x] Adopt main's `listing_content_globs: Vec<crate::glob::GlobPattern>` — the
      branch's `Vec<String>` is superseded, and `dependency_graph.rs` (which
      merges clean) requires `&[GlobPattern]`
- [x] `DOCUMENT_PROFILE_VERSION = 11`; single `11:` changelog entry; delete the
      `7 → 8` narrative
- [x] Rename the test `document_profile_version_is_10` → `_is_11`
- [x] `stage/stages/document_profile.rs` — **pure union**, verified: main adds
      listing/resource glob resolution, the branch stamps `engine_resolution`.
      Disjoint profile fields, no interaction.
- [x] `cargo nextest run -p quarto-core -- document_profile profile_version`

## A5. `project/discovery.rs` — RE-PORT (the real work)

**This is not a 3-way merge.** Main rewrote the module; the branch's feature must
be re-implemented against the new shape. This is the one place in Phase A where a
design change (D1) is unavoidably entangled with conflict resolution.

Main's new surface: `render_patterns: &[RawGlob]` (provenance-carrying),
`walk_sources` + `select_from_walk` (replacing `walk_qmd`), `is_renderable_source`,
`has_renderable_extension` (fixed `.qmd` + `.md`), `effective_render_patterns`,
`resolve_render_patterns`, `render_pattern_diagnostics`, `unmatched_md_files`,
`is_agent_instruction_md`.

The branch's contribution to keep: `RenderableExtensions` — the resolved
`FIXED_RENDERABLE ∪ engine-claims-file extensions` set. It exists **nowhere** on
main.

- [x] **Delete** the branch's superseded hand-rolled glob code: `expand_patterns`,
      `normalize_pattern`, `glob_match`, `glob_match_path`, `segment_match`,
      `wildcard_match`, `path_to_forward_slashes`, `to_forward_slashes`, and
      **`relative_to_dir`**. Main's `crate::glob::GlobPattern` / `RawGlob` system
      replaces all of it.
      **Known out-of-file callers** (these were `pub`; the answer is already
      established, don't re-derive it): `crates/quarto-core/src/project/dependency_graph.rs:54`
      and `crates/quarto-core/src/transforms/listing_generate.rs:34` import them
      on the branch. On main both use `crate::glob` instead
      (`listing_generate.rs:40` imports `crate::glob::path_to_forward_slashes`).
      `dependency_graph.rs` is main-only-modified so main's version wins
      automatically; **`listing_generate.rs` is modified on both sides and
      auto-merges — check it explicitly after the build.**
- [x] Add `renderable_extensions: &'a RenderableExtensions` to main's `DiscoveryConfig`
- [x] **Set `FIXED_RENDERABLE = &["qmd", "md"]`.** It is `&["qmd"]` on the branch
      (`discovery.rs:42`) while main's `has_renderable_extension` is
      `matches!(ext, Some("qmd" | "md"))`. **This matters:** if
      `has_renderable_extension` is switched to consult the set while the set
      still lacks `md`, `.md` drops out of discovery entirely and silently
      reverts bd-6d2wj4zp. (An earlier draft of this plan said "the set is a
      superset" — it is not.) Equivalent alternative if you prefer to leave the
      constant alone: write the predicate as
      `matches!(ext, Some("qmd" | "md")) || ext_in_set(path, set)`. Pick one and
      say which in the commit message.
- [x] Keep `RenderableExtensions` + `ext_in_set`, adapted to main's call shape,
      plus the engine-claimed accessor D1 requires
- [x] Thread the set through `walk_sources` → `walk_rec` → `is_renderable_source`
- [x] **D1:** widen `effective_render_patterns` (signature change required) —
      when no positive user pattern exists, emit `**/*.qmd` plus one
      `**/*.<ext>` per **statically** claimed extension (D2), each with
      `SourceInfo::generated(By::programmatic_config())`, **excluding the native
      set unconditionally**. Do **not** touch the `render_pattern_diagnostics`
      path.
- [x] Update `Q-5-13`'s message text — it hard-codes "No renderable source file
      (`.qmd` or `.md`) in the project matches this pattern", which becomes wrong
      once engine-claimed extensions are renderable
- [x] Re-port the branch's T6/T6b/T7 tests against the new function names, and
      confirm they still bind: the same exclusion rules (underscore, dot,
      output-dir) must apply to a claimed `.echo` as to `.qmd`, and a non-member
      extension (`.ipynb`) must stay excluded
- [x] **Add a test for D1** — a project with a claimed `.echo` extension and **no**
      `project.render` key renders its `.echo` files. This is the regression that
      neither side's suite would otherwise catch.
- [x] **Add a test for D2** — a dynamic (`claims_files: None`) engine contributes
      no discovery wildcard, and no warning is emitted
- [x] Update the `RenderableExtensions` construction sites in `project/mod.rs`
      and `crates/quarto/src/commands/render.rs`

## A6. Signature threading

- [x] `extension/read.rs` hunks 2–3 — `parse_contributes` gained
      `runtime: &dyn SystemRuntime` on main and `extension_file: &Path` on the
      branch. They serve different purposes; **thread both**, keep both doc comments.
- [x] `project/mod.rs` hunk 1 — main rebound `config` from `Option<ProjectConfig>`
      to `ProjectConfig` (project-less profile resolution, so `--profile bad/name`
      errors and `when-profile` sees the active set). The branch's
      `build_registry(…, config.as_ref(), …)`, `RenderableExtensions::new(…)` and
      `tabled_engine_names(config.as_ref())` sit exactly there. Keep main's
      rebinding, adapt the branch's calls to the non-`Option` `config`.
      (Verified behaviorally equivalent for single-file renders: a default
      `ProjectConfig` yields no tabled engines, same as the old `None`.)
- [x] `parse_config` gained `cli_selection: Option<&[String]>` on main. The branch
      has exactly **2** call sites (`project/mod.rs:1223`, `:1233`) plus the
      definition at `:1248` — both sit *outside* the conflict hunks and arrive by
      auto-merge, so main's 3-arg definition and the branch's 2-arg calls can
      collide silently. Take main's signature and update both calls. (An earlier
      draft said 5 occurrences; that count included the definition and doc
      comments.)

## A7. Both-added adjacency in the engine/stage cluster

Keep both sides at each hunk.

- [x] `engine/context.rs` — main's `project_env` field + doc **and** the branch's
      `handled_languages` / `cancellation` defaults; both initialized in
      `Default`/constructor
- [x] `stage/stages/engine_execution.rs` h1 — main's `.md`-never-executes guard
      (`Q-2-40`) **and** the branch's engine-resolution path. Per **D5**, the
      guard itself is unchanged.
- [x] `stage/stages/engine_execution.rs` h2 — both extended the same builder
      chain: main's `.with_project_env(…)` (including the unconditional
      `QUARTO_PROFILE`) **and** the branch's `.with_handled_languages(…)`
- [x] `stage/context.rs` — main's `load_project_variables` + `active_profile_names`
      **and** the branch's additions. (Verified: `ctx.variables` feeds only
      `shortcode_resolve`, a post-execution transform — engines do not need it.)
- [x] `project/orchestrator.rs` — main's unmatched-pattern diagnostic
      (bd-mt7a6uc4 D7) **and** the branch's additions
- [x] `engine/jupyter/text_execute.rs` — main's `kernel_scope` guard (bd-hxhnnlzs)
      **and** the branch's cede/claim filtering. **Placement matters:** main puts
      the guard after its `blocks.is_empty()` early return, immediately before
      `execute_blocks_async`. The branch adds a *second* early return
      (`executable.is_empty()`, all cells ceded). The guard must sit **below both
      returns** — a naive union leaves it above the cede return and acquires a
      kernel scope on a pure passthrough.

## A8. Compile-landmine sweep

Merge-tree flags only **textual** conflicts. The branch adds ~100k lines calling
`quarto-core` APIs that main changed underneath, so expect clean-merged-but-broken
call sites.

- [x] `cargo build --workspace 2>&1 | tee /tmp/mergebuild.log | tail -40`
- [x] For **every** error ask **"did I drop one of main's fields/modules in
      A2–A7?"** before "adapt the caller." That was the measured failure mode:
      picking a side in a hub file silently deletes main's newer surface
      (`project::aliases`, `ProjectConfig::brand`, `profile_config_paths`,
      `StageContext::{variables, project_env, diagnostic_policy}`,
      `DocumentProfile::resource_globs`) and the damage lands in files the merge
      never flagged.
- [x] Known-genuine ones: `dependency_graph.rs` wants `&[GlobPattern]`;
      `quarto-brand` wants `quarto_util::is_external_url`
- [x] Repeat until `cargo build --workspace` is clean

**Silent collisions in auto-merged files** — these do not conflict, so nothing
flags them; they surface as test failures in A9 unless you look:

- [x] **`pipeline.rs` stage-count assertions.** The file auto-merges but both
      sides hard-code counts: main asserts `stages.len(), 23` / `pipeline.len(), 23`;
      the branch asserts `24` / `24`. Git silently takes one side and the test
      fails later. **Do not hand-count the tail.** Run the ordering test, read the
      sequence it prints on failure, and update each `stages[n]` expectation to
      the observed order — confirming every entry is a stage you *expect* (both
      new stages present, none accidentally dropped). Same for the
      `build_analysis_pipeline` count if it shifts.
- [x] **`listing_generate.rs`** — modified on both sides, auto-merges, and imports
      glob helpers A5 deletes (see A5's note)

## A9. Commit the merge

- [x] `cargo nextest run --workspace` (run directly — **never** pipe through `tail`)
- [x] Report snapshot changes: count `.snap` added/modified/removed, summarize,
      flag anything surprising (project rule)
- [x] `git commit` the merge. Do **not** push.

---

# Phase B — design changes, one commit each

Each of these is a change *on top of* the merge, not a conflict resolution. Keep
them separate so the merge stays reviewable.

## B1. `SourceType`: drop `Ipynb`/`Rmd`, field becomes `Option`

Per **D3**.

- [x] Remove the `Ipynb` and `Rmd` variants and their `from_extension` arms
- [x] Update the affected assertions in `crates/quarto-core/src/stage/data.rs`
      tests: `:467`, `:468`, `:480`, and **`:488`**
      (`assert_eq!(source.source_type, SourceType::Qmd)` becomes `Some(…)`)
- [x] `LoadedSource.source_type` → `Option<SourceType>` per the D3 contract:
      `LoadedSource::new` uses `from_path()` directly with no fallback; the
      conversion stage stamps `Some(Qmd)` only on the conversion branch
- [x] Update `crates/quarto-core/src/stage/trace.rs` `:419` and `:505` (Debug
      formatting of the field), and `:554`
      (`assert_eq!(json["source_type"], "Qmd")` — decide the rendering for `Some`
      vs `None` and make the assertion match)

> **There is no WASM guard to write.** An earlier draft of this plan claimed
> `SourceConversionStage` runs in native builders only, citing the stage's own
> doc comment. **That comment is stale and the claim was false.**
> `build_html_pipeline_stages_with_options` (`pipeline.rs:268`) has no `cfg` gate
> and inserts the stage unconditionally; the live WASM entry points reach it
> (`crates/wasm-quarto-hub-client/src/lib.rs:1520` → `render_qmd_to_html`, `:1495`
> → `render_qmd_to_preview_ast` → `build_q2_preview_pipeline_stages`, which is
> itself a filtered call to the shared builder). WASM already converts, and
> already hard-errors on unclaimed non-native extensions. See B1b for the real
> residue.

## B1b. Delete the obsolete `build_wasm_html_pipeline`

`build_wasm_html_pipeline` (`pipeline.rs:539`) is the one builder that lacks the
conversion stage. Git archaeology (2026-08-16) establishes it is **dead code, not
a seam**:

- Added by Carlos in `094c62a4a` (2026-01-28) for "a 4-stage pipeline without
  engine execution."
- **It has never had a production caller**, at origin or since — verified across
  all refs; nothing in `crates/wasm-quarto-hub-client/` or `hub-client/` has ever
  referenced it. Callers today are two integration tests, one unit test, and a
  re-export at `lib.rs:80`.
- Its own originating plan (`claude-notes/plans/2026-01-28-unify-hub-client-pipeline.md`)
  marks Phase 4 "Update WASM Entry Points" as **already done** — the browser path
  already went through `render_qmd_to_html` on the shared builder. It was born
  redundant.
- The canonical way to give the browser a pruned pipeline is now
  `build_q2_preview_pipeline_stages`, which *derives* from the shared builder
  (calls it, then filters by stage name) instead of hand-maintaining a parallel
  list.
- It has already silently drifted: `d66f4f0eb` (Task 10, 2026-06-30) added
  `EngineClaimsFileStage` to the shared builder and did not touch this one; no
  test caught it. Its unit test's rationale for keeping `math-js` also
  contradicts the real preview pipeline, which excludes `math-js` for unrelated
  architectural reasons — it encodes a superseded design.

- [x] Delete `build_wasm_html_pipeline` and its re-export at
      `crates/quarto-core/src/lib.rs:80`
- [x] Delete `test_build_wasm_html_pipeline` (`pipeline.rs:2094`)
- [x] Retarget the two integration call sites —
      `tests/integration/document_profile_pipeline.rs:91` and
      `tests/integration/language_pipeline.rs:89` — at the shared builder.
      **No unique coverage is lost:** both files already assert the identical
      invariant against `build_html_pipeline_stages` a few lines away, so the
      retargeted assertions become duplicates and may simply be dropped.
- [x] Fix the stale doc comment on the conversion stage that claims it is
      inserted "in the **native** pipeline builders only" — it is in the shared
      builder and runs on WASM too. That comment is what produced the false
      premise in the first place; leaving it would re-mislead the next reader.

> **One honest gap:** the originating work was tracked as `kyoto-5hi`, which
> predates both the beads and braid trackers and appears in neither. Whether
> removal was ever explicitly considered and declined is unknowable from the
> repo. Everything else points one way.

## B2. Rename to `SourceConversionStage`

Per **D4**.

- [x] `EngineClaimsFileStage` → `SourceConversionStage`; `name()` →
      `"source-conversion"`; `engine_claims_file.rs` → `source_conversion.rs`
- [x] Update `stage/stages/mod.rs`, `stage/mod.rs` re-exports, `pipeline.rs` import
      and insertion site
- [x] Update the pipeline-order assertions that check `stages[0].name()`
- [x] **Do not** rename `claims_file` / `claimsFile` — wire protocol + public
      engine-author API

## B3. Refuse native-set claims (`Q-2-50`)

Per **D5**.

- [x] In the claims loop, if the extension is in the native set (`""`, `qmd`,
      `md`, `markdown`) and an engine claims it: refuse the claim, `continue`,
      fall through to pass-through
- [x] **Emit `Q-2-50` once per file, not once per claiming engine.** The refusal
      sits inside `for engine in &engines`, so a naive `continue` with an inline
      warning produces N diagnostics for one file when N engines claim it.
      Collect the refused engine names and emit a single diagnostic after the
      loop, naming them all.
- [x] Add `Q-2-50` to `crates/quarto-error-catalog/error_catalog.json`
      (subsystem `markdown`; `Q-2-49` is the current highest in both the catalog
      and `docs/errors/markdown/`)
- [x] Add `docs/errors/markdown/Q-2-50.qmd` **in the same commit** — the
      `error-docs-page-missing` lint fails CI otherwise. Template: `docs/errors/README.md`
- [x] Test: an engine claiming `.md` is refused with `Q-2-50` and the file passes
      through unconverted; a claim on `.echo` still succeeds
- [x] `cargo xtask lint`

## B4. TS engine `project_env` / `QUARTO_PROFILE` parity

Per Gordon's decision: **in this merge if it is small.** Main threads
`project_env` into engine subprocesses — `knitr/subprocess.rs:367` does
`.envs(options.project_env.iter()…)` — and applies `QUARTO_PROFILE`
unconditionally (bd-fu16z22k). The branch's deno spawn (`ts_process.rs:891`) sets
no environment at all, so merged as-is TS engines see neither.

- [x] **First, size it.** Trace whether `project_env` is reachable at the deno
      spawn site. If it threads through cleanly, do it here.
      → **Sized: it is NOT reachable, and not for a threading reason.** The
      deno spawn lives in `TsEngineHost::ensure_started`, whose contract is
      that init "runs at most once across concurrent callers" — one
      long-lived host shared by every document and engine. No
      `ExecutionContext` is in scope there and structurally cannot be.
      knitr differs because it spawns a *fresh* Rscript per execution, which
      is what makes `.envs()` at its spawn site inherently per-document.
- [x] **If it is invasive, STOP and report** rather than growing the merge — file
      a strand instead. This is an explicit off-ramp, not a failure.
      → **Off-ramp taken. Filed as bd-x30l7ee5 (p1, bug).**
- [ ] ~~If done: `.envs(project_env.iter())` at the deno spawn, plus a test that a
      TS engine observes an `_environment` variable and `QUARTO_PROFILE`~~
      **NOT DONE — deliberately.** A naive `.envs()` at the deno spawn would
      bake in whichever document happened to start the host first, which is
      silently wrong whenever `_environment` or the active profile differs
      per document — arguably worse than today's honest "no env at all",
      because it would be right often enough to stop checking. The correct
      fix needs the env to travel per-execute on the wire (`TsExecuteOptions`
      has no env field today) or host keying by env-set; both are protocol/
      architecture changes that do not belong inside a merge. See
      bd-x30l7ee5.

## B5. Update the strand record

- [x] Comment on **bd-xxul** that its "extend the pipeline's `SourceType`
      handling" line is resolved in the *opposite* direction (D3), so the record
      does not mislead
- [x] Comment on **bd-19nc56ao** that `.ipynb` now lands as an engine claiming
      `.ipynb`, and that bd-zlemoc6w is its provenance prerequisite

---

# Phase C — verification and PR prep

> **Prerequisite: `deno` must be on `PATH`.** The engine e2e suites are
> deno-gated and **skip silently** when it is absent
> (`crates/quarto-core/tests/integration/echo_engine_e2e.rs:41`, and siblings),
> and `ensure_bundle` (`tests/integration/engine_fixture_build.rs:62`)
> regenerates fixture bundles at test time. Without deno, `cargo nextest run
> --workspace` can be **fully green with every test that binds D1, D2 and D5
> skipped** — i.e. green while proving nothing about this merge's actual subject.

- [x] `deno --version` succeeds → deno 2.9.0
- [x] `cargo build --workspace` clean
- [x] `cargo nextest run --workspace` green → **12,624 passed, 0 failed**
- [x] **Zero SKIP lines in the engine suites.** → 0 skips; 41 engine-suite tests ran.
      This is the check that makes the rest meaningful. Grep the nextest output for
      skipped tests in `echo_engine_e2e`, `julia_engine_e2e` and siblings; a skip
      here means the D1/D2/D5 work is unverified, not passing.
- [x] `cargo xtask lint` → all checks passed (1018 files)
- [x] **`cargo xtask verify`** — full, **not** `--skip-hub-build`: `quarto-core`
      and `wasm-quarto-hub-client` both change
- [x] **End-to-end** (project rule — tests alone are insufficient for a
      user-visible feature). At minimum:
  - `cargo run --bin q2 -- render docs/` — inspect output
  - a project with a claimed `.echo` extension and **no** `project.render` key,
    confirming the `.echo` file renders (this exercises D1 through the real
    binary — it is the single most important check in Phase C, because D1 is the
    decision with the least existing test coverage). Build the fixture from
    `crates/quarto-core/tests/fixtures/extensions/echo-engine`; note its `dist/`
    is deliberately uncommitted and is regenerated by `ensure_bundle`, so build
    the extension first (`q2 call build-ts-extension`) or copy the fixture into a
    scratch project the test harness has already bundled.
  - record the exact invocations and output snippets below
- [x] Report snapshot changes → 6 `.snap` files (3 added, 3 modified), **all
      byte-identical to main's**: purely incoming, none regenerated here.
      Added: entity-references, smart-typography-positions,
      llms_companion_rich_content. Modified: the three listing_pipeline
      categories snapshots. Nothing surprising; none touched by Phase B/C.
- [ ] **Ask Gordon for push permission.** Stop here.

```
All output below was run through the real `q2` binary and inspected — not
inferred from an exit code.

── D1: a claimed `.echo` renders with NO `project.render` key ──────────────

Fixture: `_quarto.yml` = `project: {type: default}` (no `render:` key),
`_extensions/echo-engine` (the committed fixture; `dist/` is deliberately
uncommitted, so built first), plus `a.echo`, `index.qmd`, `notes.md`.

    $ cargo run --bin q2 -- call build-ts-extension /tmp/d1-e2e/_extensions/echo-engine
    Built: .../src/echo-engine.ts → .../dist/echo-engine.js

    $ cargo run --bin q2 -- render /tmp/d1-e2e
    Rendering project: /private/tmp/d1-e2e (type: default)
    Rendered 2 of 2 files to /private/tmp/d1-e2e

Inspected:

  * `a.html` exists and contains `ECHO_EXECUTED` — the `.echo` was
    discovered with no `render:` key, converted, AND executed. This is D1;
    before the widening the file was silently dropped by a gate-2 default of
    literally `**/*.qmd`.
  * `index.html` exists — widening ADDS to the default, never replaces it.
  * `notes.html` does NOT exist ("2 of 2", not 3) — `.md` stays opt-in, so
    engine claims do not drag the native set into the default pattern set
    (bd-6d2wj4zp preserved).
  * zero `Q-2-50` warnings — a non-native claim is legitimate and silent.

── B3/D5: `Q-2-50` refuses a native-set claim ──────────────────────────────

Same fixture with the echo engine made greedy (`.md` added to both
`_extension.yml`'s `claims-files`/`file-extensions` AND the module's
`claimsFile`, so the engine's own self-enforcement check passes), and
`render: ["*.md", "*.echo"]` so the `.md` reaches the stage at all.

    $ cargo run --bin q2 -- render /tmp/b3-e2e
    Rendering project: /private/tmp/b3-e2e (type: default)
    Warning [Q-2-50]: engine claim on `/private/tmp/b3-e2e/notes.md` ignored
    The engine `echo` claimed `.md` files, but Quarto handles markdown
    natively. The claim is ignored and the file is rendered by Quarto's own
    parser.
    ℹ Quarto owns `.qmd`, `.md`, `.markdown` and extension-less inputs. An
    engine that claimed one of them would bypass Quarto's parser entirely.
    Engine extensions should claim their own file extension instead.

    Rendered 2 of 2 files to /private/tmp/b3-e2e — 1 warning

Inspected:

  * `notes.html` contains its source text verbatim and **zero**
    `ECHO_EXECUTED` — the claim was refused and the file passed through to
    q2's own parser unconverted.
  * `a.html` still contains `ECHO_EXECUTED` — the refusal is scoped to the
    native set; legitimate `.echo` claiming still works.
  * exactly **one** `Q-2-50` line for the one file (the once-per-file, not
    once-per-engine, requirement).

Incidental finding on the way: an earlier iteration edited only the YAML and
not the module, and q2 correctly hard-errored with "statically declares
claims_file true for extension 'md' but the loaded module's claimsFile
reports false" — engine self-enforcement (plan 4b) working as designed.

── Engine suites actually ran (the Phase C trap) ───────────────────────────

`grep -c 'SKIP: deno not on PATH'` over the full run = **0**, with 41
engine-suite tests passing (echo/julia/marimo/synth/behave). deno 2.9.0 was
on PATH, so the D1/D2/D5 work is genuinely verified rather than skipped —
which was the specific way this phase could have been green while proving
nothing.

── The one test failure, resolved ──────────────────────────────────────────

`quarto-preview config_reports_embedded_asset_manifest_hashes` failed
through Phase A and B. Diagnosis: this worktree's `q2-preview-spa/dist/`
dated from 2026-07-24 and so predated main's addition of
`scripts/manifest-dist.mjs` to the SPA build, leaving no `spa-manifest.json`
for the test to find. Note `cargo xtask verify` cannot self-heal this — it
runs Rust tests (step 5/14) BEFORE the hub build that regenerates the SPA.

Running `cargo xtask build-q2-preview-spa` (→ "spa manifest: 74 entries,
hash 90748d7c…") makes the test pass. Confirmed, not assumed. `dist/` is
gitignored, so nothing to commit.

── `cargo xtask verify` (full, not --skip-hub-build) ───────────────────────

    ✓ All verification steps passed!      (all 14 steps, exit 0)

Step 5 alone: **12,624 tests passed, 0 failed, 198 skipped.**

Two local tool gaps had to be closed first; neither was a code defect:

  * `wasm-opt` (binaryen) was absent, hard-failing step 7. It is a NEW
    requirement main introduced — `8024198be` added it to the hub-client
    e2e, ts-test and release workflows — and one `cargo xtask dev-setup`
    installs. Installed with `brew install binaryen`.
  * the stale `q2-preview-spa/dist/` above.

── `q2 render docs/` ───────────────────────────────────────────────────────

    $ cargo run --bin q2 -- render docs/
    Rendered 244 of 245 files to .../docs/_site — 1 error, 12 warnings

Inspected: 258 HTML pages produced; `index.html` carries its title block and
TOC; the new `docs/errors/markdown/Q-2-50.qmd` renders as
`<h1 class="title">Engine claim on a Quarto-owned file type ignored</h1>`.

The 1 error is environmental, not a regression: *"Engine 'jupyter' is
registered but its runtime is not available"* — `which jupyter` finds
nothing on this machine, so a doc page declaring `engine: jupyter` cannot
execute. The 12 warnings are pre-existing missing-image resource
references in `docs/guides/authoring/`.

**Gotcha for the next person:** `cargo xtask stage-doc-examples` (which
generates the gitignored `docs/examples/` that `docs/_quarto.yml` declares
as a resource) stages into the **main checkout**, not the worktree you run
it from — `repo_root()` resolves `git rev-parse --git-common-dir`, which in
a worktree is deliberately the main repo's `.git`. It reports success while
writing elsewhere, and because the output is gitignored on both sides
nothing contradicts it. Filed as **bd-afi4avsf**. Workaround used here: copy
`<main>/docs/examples` into the worktree.
```

## Post-merge follow-ups to spot-check

Main features that auto-merged cleanly but sit next to resolved code: project
profiles (`when-profile`, `QUARTO_PROFILE` write-back), brand resolution,
listings/feeds, `project::aliases`, title-block-parity transforms, i18n
`LanguageResolveStage`, jupyter cell-options and error policy. `cargo xtask
verify` covers most; the docs render exercises mermaid + title-block.

## Prior art

- `claude-notes/plans/2026-07-22-ts-engine-extensions-rebase-conflict-resolution.md`
  — the previous catch-up (merge-base `61e2d2276`). Conflict inventory is stale,
  but its resolution philosophy ("branch structure + main's features, never a
  whole side") and its compile-landmine task carry over. **Its `--ours`/`--theirs`
  orientation is the rebase one — do not copy it.**
- `claude-notes/plans/2026-08-07-md-render-support.md` (main) — bd-6d2wj4zp, the
  `.md` model D1 departs from.
- `claude-notes/plans/2026-04-16-plan1c-extension-integration.md:1230` — the A′
  provenance design deferred as bd-zlemoc6w.
