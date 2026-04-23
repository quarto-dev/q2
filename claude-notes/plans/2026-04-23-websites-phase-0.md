# Phase 0 — Foundations: DocumentProfile, Pipeline Checkpoint, Naming

**Date:** 2026-04-23
**Beads:** `bd-f3jc` (phase); parent `bd-0tr6` (website epic).
**Parent plan:** `claude-notes/plans/2026-04-23-website-project-epic.md`
**Status:** Draft — awaiting user review on open questions at the end.

## Goal of this phase

Lay the substrate that every subsequent website-epic phase depends on:

1. A named, typed, serializable **static-document snapshot** type.
2. A named **pipeline checkpoint** between metadata merge and any AST
   mutation, where the snapshot is extracted and from which pass 2 can
   resume.
3. Round-trip serialization + clone-and-resume tests proving the
   checkpoint does what we claim.
4. A short contract document recording **what a snapshot is guaranteed
   to contain**, under what conditions, and what it is explicitly *not*
   — so future work (sidebar, incremental rebuilds, freeze) builds on
   a stable promise.

**No user-visible behavior change.** This phase ships a pure refactor +
new types. A `quarto render` on a single document must produce
byte-identical output before and after, verified by the existing test
suite and by an explicit regression integration test.

This phase does **not** implement:

- The `ProjectType` trait (Phase 1).
- Two-pass orchestration (Phase 1).
- Any sidebar / navbar / footer project behavior (Phases 2–4).
- Any cross-doc index data structure beyond what the snapshot itself
  already holds (Phase 1 introduces `ProjectIndex`).
- On-disk caching of snapshots (Phase 8).

The naming choice must accommodate those later phases (see §Naming
decisions and open questions).

## Reference material

Read before starting implementation:

- Parent plan: `claude-notes/plans/2026-04-23-website-project-epic.md`
  (especially §"Pipeline with snapshot checkpoint" and
  §"Open questions to resolve during phase 0").
- `crates/quarto-core/src/stage/data.rs` — `PipelineData` enum and the
  variant shapes we'll add to.
- `crates/quarto-core/src/stage/stages/mod.rs` — stage list.
- `crates/quarto-core/src/stage/stages/metadata_merge.rs` — the stage
  we're inserting the checkpoint right after (tentatively).
- `crates/quarto-core/src/stage/stages/pre_engine_sugaring.rs` — the
  next stage; snapshot lives at the boundary.
- `crates/quarto-core/src/pipeline.rs` — `build_html_pipeline_stages*`.
- `crates/quarto-core/src/crossref/index.rs` — reference pattern for a
  serializable per-document artifact (`CrossrefIndex`) already living
  in `quarto-core`. Follow its serde conventions.
- `crates/pampa/src/toc.rs` — `TocEntry` / `NavigationToc` (the
  outline is already computed here for `toc: auto`; reuse).
- Q1 reference: `external-sources/quarto-cli/src/project/types.ts:315`
  (`InputTargetIndex`) and `:322` (`InputTarget`) — the closest Q1
  equivalent, split across "raw" and "resolved".
- `.claude/rules/wasm.md`, `.claude/rules/cross-platform.md` — must
  apply to any new code.
- `claude-notes/instructions/testing.md` and `coding.md`.

## Naming decisions (confirmed 2026-04-23)

| Concept | Name |
|---|---|
| Snapshot type | `DocumentProfile` |
| Pipeline variant | `PipelineData::AtProfile(DocumentAtProfile)` |
| `PipelineDataKind` tag | `PipelineDataKind::AtProfile` |
| Stage | `DocumentProfileStage` |
| Module (type) | `quarto_core::document_profile` |
| Module (stage) | `quarto_core::stage::stages::document_profile` |
| Trait (Phase 1, deferred) | `ProjectType` (parent plan) — confirmation deferred to Phase 1 |

**Crate placement.** Keep `DocumentProfile` in `quarto-core` for
Phase 0; revisit `quarto-project` split at the start of Phase 1.

## Checkpoint position

Parent plan proposes the checkpoint live **after `MetadataMergeStage`
and before `PreEngineSugaringStage`**. My recommendation: stick with
that. Reasoning:

1. After merge: metadata is fully resolved (project + directory +
   document + runtime layers all flattened, format-specific keys
   unwrapped), which is exactly what downstream project features need
   to read `title`, `draft`, `categories`, `website.*`, etc.
2. Before sugar: the AST is still the raw parsed form. `outline`
   extracted here reflects the author's headings, not synthetic ones
   introduced by `TheoremSugar` / `FloatRefTargetSugar` / etc. The
   parent plan's §"Open question — Snapshot location precisely"
   identifies this as a judgment call; the cleaner cut is pre-sugar.
3. Before engine execution: the profile is static — it does not depend
   on Python, R, or Julia running. This is the contract we want for
   sidebars, incremental rebuilds, and (eventually) `freeze`.

One subtlety: `ref_type_registry` is populated by
`PreEngineSugaringStage` (from `crossref.custom` metadata and
promised-id prefixes). It is not part of the snapshot — consumers of
the profile don't need it in Phase 0 — so no conflict today.

**Forward note (user, 2026-04-23).** Eventually we should move
`ref_type_registry` construction *before* the profile checkpoint. That
would let project-level code validate custom-crossref-type
compatibility across a book or website without rendering every file —
the kind of static cross-document check DocumentProfile is meant to
enable. Not Phase 0 work, but a reminder that the checkpoint's
"pre-sugar" position is deliberately chosen to leave room for things
like this to move earlier in the pipeline. When that move happens,
the registry (or the subset of it needed for cross-file validation)
may need to be added as a profile field, with a `profile_version`
bump.

## Type sketch

```rust
// crates/quarto-core/src/document_profile.rs

use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use pampa::toc::TocEntry;

/// Version bumped when the serialized shape changes. Consumers reading
/// a cached profile from disk must check this and reject on mismatch.
pub const DOCUMENT_PROFILE_VERSION: u32 = 1;

/// Static, engine-independent snapshot of a document extracted after
/// metadata merge and before any AST mutation.
///
/// See `claude-notes/designs/document-profile-contract.md` for the
/// full contract (what is guaranteed, under what conditions).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentProfile {
    /// Version tag for serialized profiles.
    pub profile_version: u32,

    /// Source path, **always project-relative**, forward-slash
    /// separated. See §"Project root invariant" below: there is no
    /// such thing as "no project" — a bare file is treated as a
    /// single-file project rooted at the file's directory, and its
    /// `source_path` is then just the file name.
    pub source_path: PathBuf,

    /// Rendered-target href other pages should use to link to this
    /// document (e.g. `"about.html"` or `"docs/api.html"`).
    /// Forward-slash separated, relative to the project output
    /// directory.
    pub output_href: String,

    /// Which format-variant is being produced (e.g. `"html"`,
    /// `"acm-html"`). Mirrors `ctx.format.target_format` at the
    /// checkpoint.
    pub format_id: String,

    /// Document title (plain text; inline formatting flattened for
    /// cross-document use). `None` when the document has no title
    /// and no first-heading fallback could be synthesized.
    pub title: Option<String>,

    pub subtitle: Option<String>,
    pub description: Option<String>,

    /// Authors, flattened to plain-text strings. One entry per
    /// author. See open questions for the "structured authors"
    /// trade-off.
    pub authors: Vec<String>,

    pub date: Option<String>,
    pub categories: Vec<String>,
    pub keywords: Vec<String>,
    pub image: Option<String>,

    pub draft: bool,

    /// Heading outline (id, text, level, nesting). Reuses
    /// `pampa::toc::TocEntry`, which is already serde-serializable
    /// and used by `TocGenerateTransform`.
    ///
    /// Always un-numbered: `TocEntry::number` is `None` for every
    /// entry in the profile's outline. Section numbering is computed
    /// later by `CrossrefIndexTransform`, which runs *after* the
    /// profile checkpoint. Consumers that need numbered outlines must
    /// read them from the post-render AST, not from the profile.
    pub outline: Vec<TocEntry>,
}

impl DocumentProfile {
    pub const VERSION: u32 = DOCUMENT_PROFILE_VERSION;

    /// Extract a profile from a `DocumentAst` at the checkpoint.
    /// This is pure: no I/O, no runtime calls.
    pub fn extract(
        ast: &Pandoc,
        source_path: &Path,
        output_href: &str,
        format_id: &str,
    ) -> Self { /* … */ }
}
```

### Pipeline data variant

```rust
// crates/quarto-core/src/stage/data.rs (additions)

pub enum PipelineDataKind {
    // … existing variants …
    AtProfile,
}

pub enum PipelineData {
    // … existing variants …
    AtProfile(DocumentAtProfile),
}

/// Pipeline state at the profile checkpoint.
///
/// Holds the extracted `DocumentProfile` plus the `DocumentAst` from
/// which subsequent stages will resume. The AST is carried through
/// unchanged — this variant exists purely to expose the profile to
/// project orchestration while keeping the per-file pipeline
/// compositional.
#[derive(Debug, Clone)]
pub struct DocumentAtProfile {
    pub profile: DocumentProfile,
    pub ast: DocumentAst,
}
```

`DocumentAst` itself needs `#[derive(Clone)]` added so the outer
`DocumentAtProfile` can be cloned. All its fields are already Clone
(`Pandoc`, `pampa::pandoc::ASTContext`, `SourceContext`,
`Vec<DiagnosticMessage>` — verified). No serde for `DocumentAst` in
this phase; serialization of the whole checkpoint is a later concern
(freeze epic).

### The stage

`DocumentProfileStage` is a pass-through on the data plus an
extraction into `ctx` (or into a new variant — see open questions).
Two candidate shapes:

**Option A: new pipeline variant (recommended).**

- Input kind: `DocumentAst`
- Output kind: `AtProfile`
- Runs between `MetadataMergeStage` and `PreEngineSugaringStage`.
- Produces `PipelineData::AtProfile(DocumentAtProfile { profile, ast })`.
- `PreEngineSugaringStage` (and everything after it) changes its
  input kind from `DocumentAst` to `AtProfile` and **unwraps**
  internally, pulling the `DocumentAst` back out before proceeding.

This keeps the checkpoint visible in the type system and makes pass 2
resumability natural: pass 2 takes a `PipelineData::AtProfile` and runs
only the tail of the pipeline.

**Option B: put the profile in `StageContext`.**

- Stage input/output both `DocumentAst`.
- Stage writes `ctx.profile = Some(DocumentProfile::extract(...))`.
- No pipeline variant change, no downstream stage signature churn.

Option B is smaller and lower-risk but does not represent the
checkpoint in the type system — it's just "metadata merge with a side
effect". Option A is more invasive but matches the parent plan's
design intent of a named checkpoint that pass 2 can resume *from*.

**Decision (2026-04-23): Option A, with the unwrap-stage
implementation strategy** (see next subsection) to keep downstream
stage signatures unchanged.

**User-flagged concern:** the Option A refactor touches every
downstream stage's input-kind declaration. The unwrap-stage approach
below keeps the diff small, but if something goes wrong during
implementation — e.g. the byte-identical regression test in
§Tests diverges for a reason we can't quickly diagnose — stop,
reassess, and consider falling back to Option B (profile in
`StageContext`, no pipeline-variant change). Option B is strictly
less informative in the type system but equivalent for Phase 0's
consumers.

### Where the profile is stored for Phase-0 consumers

In Phase 0 there are no project-orchestration consumers. The profile
is only used by:

1. A round-trip serialization test.
2. A clone-and-resume integration test (see §Tests).

For Phase 0 it's enough that the profile exists inside the pipeline
data. Phase 1 introduces `ProjectIndex` and adds it to `StageContext`.

## Project root invariant (user directive, 2026-04-23)

**There is no such thing as "no project root."** A bare `.qmd` file
with no `_quarto.yml` nearby is treated identically to a file sitting
next to an empty `_quarto.yml` in the same directory: project root is
the file's directory, `source_path` in the profile is just the file
name, and the output dir resolves relative to that same directory.

This is a deliberate inversion of Q1, where "is there a project?" was
a branch point threaded through much of the rendering code and was a
repeated source of bugs. In Q2 we never branch on "project or
not" — the project context always exists, and single-file renders are
a degenerate case of the same code path.

Concretely for Phase 0:

- `DocumentProfile::extract` takes `project_dir: &Path` and computes
  `source_path` as `input.strip_prefix(project_dir).unwrap_or(file_name)`
  — never optional, never "absolute fallback".
- `output_href` likewise is always project-output-relative. For a
  single-file render, project output dir == project dir == file's
  dir, so the href is just `"<stem>.html"`.
- Tests 1–12 below must include a single-file case and a
  multi-file-with-`_quarto.yml` case that exercise the *same* code
  path. Any branch in the implementation on `ProjectContext::is_single_file`
  is a red flag.

The existing `ProjectContext` already has `is_single_file: bool`.
Phase 0 does not try to remove that field — doing so is a bigger
project — but **no new code introduced in this phase may read it.**
Where Phase 0 code would be tempted to branch on it, resolve the
concern structurally (by making the project-relative / output-dir
math work uniformly) rather than by adding a single-file branch.

## Tests

Per CLAUDE.md §TDD: every test below gets written *before* the code
that makes it pass, and gets run to verify it fails first.

### Unit tests

Unit-level, in `crates/quarto-core/src/document_profile.rs`:

1. **`profile_extract_minimal_document`** — parse a 3-line qmd with
   just a title; run through `ParseDocumentStage` +
   `MetadataMergeStage`; call `DocumentProfile::extract`; assert
   `title`, `source_path`, `format_id` are correct, `outline` is
   empty, `draft == false`.
2. **`profile_extract_with_headings`** — document with `#`, `##`,
   `###` headings; assert `outline` is a correctly nested
   `Vec<TocEntry>`.
3. **`profile_extract_with_full_frontmatter`** — exercise every
   documented profile field (authors, categories, keywords, image,
   subtitle, description, date, draft).
4. **`profile_extract_handles_missing_title`** — document with no
   title and no H1 → `title == None`.
5. **`profile_roundtrip_json`** — build a profile, serialize with
   `serde_json::to_string`, deserialize, assert equal.
6. **`profile_version_mismatch_rejected`** — write a JSON blob with
   `profile_version: 999`, assert `serde_json::from_str` returns an
   error or our wrapper returns `VersionMismatch` (pick one; see open
   questions on strictness).

### Stage-level tests

In `crates/quarto-core/src/stage/stages/document_profile.rs`:

7. **`stage_extracts_profile_from_document_ast`** — end-to-end stage
   invocation: feed `PipelineData::DocumentAst`, assert output kind
   is `AtProfile` with expected fields.
8. **`stage_rejects_wrong_input_kind`** — feed
   `PipelineData::LoadedSource`, assert `PipelineError::UnexpectedInput`.
9. **`stage_preserves_warnings`** — input DocumentAst has parse
   warnings; output `AtProfile` preserves them on the inner
   `DocumentAst`.

### Pipeline integration tests

In `crates/quarto-core/tests/` (new file, e.g.
`document_profile_pipeline.rs`):

10. **`pipeline_at_profile_to_end_produces_expected_html`** — run the
    full HTML pipeline on a fixture, extract the HTML, and also run a
    "pause at profile, clone, resume" variant on the same fixture.
    Assert both HTML outputs are **byte-identical**. This is the
    load-bearing test for checkpoint resumability; it's what the
    parent plan's §Risks calls out.
11. **`pipeline_profile_matches_metadata`** — run the full pipeline
    with a document whose frontmatter sets title/author/categories,
    and assert the profile extracted at the checkpoint has the same
    values as the merged `ast.meta` post-merge.
12. **`wasm_pipeline_includes_profile_stage`** — verify
    `build_wasm_html_pipeline()` also includes `DocumentProfileStage`
    (hub-client needs it too; Phase 9 depends on this).

### Snapshot regression

No snapshot changes are expected in this phase — but to prove it:

13. Run `cargo nextest run --workspace` before and after. **Any
    snapshot diff is a red flag** and must be investigated per
    CLAUDE.md §"Snapshot Test Changes" before the commit.

### End-to-end verification

Per CLAUDE.md §"End-to-end verification before declaring success":
`cargo run --bin quarto -- render <fixture>.qmd` before and after the
change should produce byte-identical output on a handful of fixtures
(pick 3 from existing `crates/quarto-core/tests/fixtures/`). Record
the invocations and results in the Phase 0 completion note, per
policy.

## Crate placement

The parent plan's §Open question asks whether `DocumentProfile` and
`ProjectType` live in `quarto-core` or a new `quarto-project` crate.

**Decision (2026-04-23):** `quarto-core` for Phase 0, same crate as
`CrossrefIndex` (also a serializable per-document artifact). Revisit
the `quarto-project` split at the start of Phase 1 when we can see
what `ProjectType` actually needs to import. The
dependency analysis:

- `DocumentProfile` needs: `serde`, `pampa::toc::TocEntry` (already a
  dep), standard `PathBuf`. All already available in `quarto-core`.
- No new external deps required.
- `quarto-core` already depends on `quarto-navigation`, not the other
  way around; putting `ProjectType` (Phase 1) in `quarto-core`
  continues that direction.
- The argument *for* a new `quarto-project` crate is long-term
  hygiene: project-type logic (website, book, manuscript) will grow
  over this epic and may eventually want to depend on rendering but
  be depended-on by thin orchestrators (`quarto` binary, hub-client).
  That argument is real but Phase-0 premature — the hygiene case
  should be re-evaluated at the start of Phase 1 once we can see
  what `ProjectType` will actually need to import.

If the user prefers a new crate up front, the cost is small (new
crate, re-export, update workspace `Cargo.toml`) but the benefit is
speculative. I'd rather defer. See open questions.

## Contract doc

**Location (confirmed):** `claude-notes/designs/document-profile-contract.md`
(new file). Short, reference-style. Contents:

- What is a `DocumentProfile`? One-paragraph summary.
- **Guarantees.** Each field: what it contains, when it's
  `None`/empty, what it reflects (document YAML vs. project
  layered YAML vs. a fallback).
- **Non-guarantees.** What a profile does *not* contain:
  engine output, sugar-synthesized headings, filter-mutated AST,
  theme CSS, resolved shortcodes.
- **Versioning.** When to bump `profile_version`, what downstream
  tools must check.
- **Writing a consumer.** Short note for Phase 1+ authors:
  "profiles are read-only; mutation belongs in the user-filter
  phase, not here."

**Findability:** this Phase-0 plan and the parent epic plan must link
to the contract doc once written. When Q2 eventually documents
itself (per `bd-tr81`) we'll likely move or mirror this into the
user-facing docs site, but for now the `claude-notes/designs/`
location is fine and these plan references are how future agents
find it.

## Work items (checklist)

### Preparation
- [x] Re-read `claude-notes/instructions/testing.md` and
      `coding.md` before writing any code.
- [x] Create a worktree under `.worktrees/websites-phase-0/` per
      `.claude/rules/worktrees.md`.

### TDD phase — tests first, all failing
- [x] Add `DocumentProfile` type skeleton (empty methods) so the
      tests below compile.
- [x] Write unit tests 1–6 (see §Tests). *Result: 3 behavior tests
      failed as expected against the stub; 3 infrastructure tests
      (serde round-trip, version-mismatch, empty-doc) passed on the
      stub, which is correct — they test the serde layer, not
      extraction.*
- [x] Add `DocumentAtProfile` + `PipelineData::AtProfile` +
      `PipelineDataKind::AtProfile` skeletons so stage tests
      compile.
- [x] Add `DocumentProfileStage` skeleton.
- [x] Write stage tests 7–9. *Result: 1 behavior test failed as
      expected; 2 invariant tests (wrong-input rejection, warnings
      preserved) passed because their logic is in the stage, not
      the stub extractor.*
- [x] Write pipeline integration tests 10–12. *Result: 4 of 5
      tests failed because the stages weren't in the pipeline
      builders yet — correct intermediate state. 1 stage-name
      sanity test passed.*

### Implementation
- [x] Implement `DocumentProfile::extract(ast, source_path, output_href, format_id)`.
      Pull each field from `ast.meta` via `ConfigValue::get` /
      `as_plain_text()`. Outline from `pampa::toc::generate_toc`
      using max depth (6), un-numbered post-scrub.
- [x] Implement `DocumentProfileStage::run`: extract profile, wrap
      into `DocumentAtProfile`, return `PipelineData::AtProfile(...)`.
      Project-relative paths and output href computed in the stage,
      then handed to the pure extractor. **No branch on
      `is_single_file`** — the math works uniformly.
- [x] Add `#[derive(Clone)]` to `DocumentAst` (all inner fields
      were already Clone; `cargo check` confirmed).
- [x] **Decision captured in code**: went with the *unwrap-stage*
      approach. `UnwrapProfileStage` sits immediately after
      `DocumentProfileStage` and hands the inner `DocumentAst` back,
      so every downstream stage keeps its `DocumentAst` input kind
      unchanged. Phase 1's orchestrator will replace this with a
      real consumer that reads the profile and drives two passes.
- [x] Wire `DocumentProfileStage` + `UnwrapProfileStage` into
      `build_html_pipeline_stages_with_apply_config` and
      `build_wasm_html_pipeline`. Analysis pipeline left alone — no
      consumer there yet; adding stages without a consumer is dead
      weight.
- [x] Implement `profile_version` check at deserialize time
      (`DocumentProfile::from_json` returns
      `DocumentProfileError::VersionMismatch`).
- [x] Run unit + stage tests; all 13 pass.
- [x] Run integration tests; all 5 pass. Test 10 (clone + resume
      byte-identical HTML) is the acceptance criterion and passes.

### Documentation
- [x] Write `claude-notes/designs/document-profile-contract.md`
      per §Contract doc.
- [x] Add doc comment at `DocumentProfile` pointing at the contract.
- [x] Add a note to `CLAUDE.md` under "Architecture Notes".

### Verification and close-out
- [x] `cargo build --workspace` clean.
- [x] `cargo nextest run --workspace` — 7654/7654 tests pass, 195
      skipped; no snapshot diffs. 3 hard-coded stage-count
      assertions in `crates/quarto-core/src/pipeline.rs` tests
      updated as intended (11 → 13 for HTML, 10 → 12 for WASM).
- [x] `cargo xtask lint` passes (609 files checked).
- [x] `cargo xtask verify` passes end-to-end: Rust build, Rust
      tests, hub-client build, hub-client tests, trace-viewer
      build, trace-viewer tests all green. (First attempt failed
      on missing node_modules in the worktree; resolved by running
      `npm install` once from the worktree root per hub-client
      conventions.)
- [x] End-to-end: `cargo run --bin q2 -- render <fixture>.qmd`
      on 3 synthetic fixtures (title-only, headings+author+categories,
      full frontmatter with code block). MD5 of each rendered HTML
      matches between `feature/websites` (pre-change) and
      `feature/websites-phase-0` (post-change):
      - `65d0bf7fa6978659d2bde67acfcbf5cb` (test-basic: title+subtitle+author+python)
      - `d95941f74c232939bf75f5f533ce1b69` (fix2: minimal title)
      - `88f0a5d8af4d23d4052a1eea9511890c` (fix3: categories, multi-author)

      **The Phase-0 change is behaviorally invisible at the CLI.**
- [ ] `br update bd-f3jc --status closed` with reason.
- [ ] `br sync --flush-only && git add .beads/ && git commit`.
- [ ] Stop and request permission before pushing to remote, per
      CLAUDE.md §GIT PUSH POLICY.

## Risks and mitigations

- **Risk: every downstream stage changes its input kind from
  `DocumentAst` to `AtProfile`.** That is mechanically large and
  invasive. *Mitigation:* the alternative (unwrap stage right after
  the checkpoint) keeps all other signatures unchanged. I'd start
  with the unwrap-stage approach to minimize diff surface, then
  revisit in Phase 1 when pass-2 resumability actually needs to
  enter the pipeline mid-way.
- **Risk: test 10 (byte-identical clone + resume) reveals the
  pipeline has hidden shared mutable state I missed.** *Mitigation:*
  this is exactly why test 10 exists. If it fails, it is correct
  to fail; the fix is to identify the shared state (likely in
  `StageContext` — artifacts, registries, diagnostics) and either
  clone it at the checkpoint or document that it's additive-only
  and resuming from a clone is well-defined.
- **Risk: `DocumentAst` not Clone breaks something non-obvious.**
  *Mitigation:* `cargo check` on the whole workspace after adding
  the derive is the canary. Existing code that takes `DocumentAst`
  by value should be unaffected.
- **Risk: outline computed pre-sugar differs from what a user
  writing `toc: true` sees in the rendered document.** This is
  intentional (the profile's outline is the *author's* hierarchy,
  not the sugared one) but could surprise consumers. *Mitigation:*
  call this out explicitly in the contract doc. Phase 2 (sidebar)
  consumers will be designed around the pre-sugar outline.
- **Risk: changing the pipeline breaks hub-client.** *Mitigation:*
  `cargo xtask verify` catches this. Also — `DocumentProfileStage`
  is added symmetrically to both `build_html_pipeline_stages` and
  `build_wasm_html_pipeline`.
- **Risk: `PipelineDataKind::AtProfile` adds a variant that every
  `match` on the kind must handle.** *Mitigation:* `cargo check`
  will flag non-exhaustive matches. Address each.

## Explicit non-goals for this phase

- No project orchestration, no two-pass driver.
- No disk cache for profiles.
- No `ProjectIndex` type.
- No `ProjectType` trait.
- No sidebar / navbar / footer / cross-doc features.
- No `DocumentAst` serialization (freeze concerns).
- No changes to user-filter positions. (The "profile-reading
  filter position" mentioned in the parent plan is Phase 1+.)

## Decisions log (user confirmed 2026-04-23)

All open questions from the initial draft are resolved. Recording
them here for the audit trail.

**Naming** (all confirmed as proposed):
- Type: `DocumentProfile`
- Stage: `DocumentProfileStage`
- Pipeline variant: `PipelineData::AtProfile(DocumentAtProfile)`
- Tag: `PipelineDataKind::AtProfile`
- Modules: `quarto_core::document_profile`,
  `quarto_core::stage::stages::document_profile`

**Shape**: Option A (new pipeline variant) + unwrap-stage strategy
to keep downstream signatures unchanged. User flagged that if the
large downstream refactor runs into trouble we should reconsider —
captured in §Shape and §Risks.

**Fields**:
- `source_path`: project-relative, forward-slash. See §"Project
  root invariant" — "no project root" is not a case; a bare file
  is a single-file project rooted at its directory.
- `output_href`: project-output-relative, forward-slash.
- `authors`: flat `Vec<String>` for now; structured-author design
  is a separate future pass.
- `outline`: un-numbered; consumers needing numbers read them from
  the post-render AST.

**Crate placement**: `quarto-core` for Phase 0; defer
`quarto-project` split to Phase 1.

**Checkpoint position**: after `MetadataMergeStage`, before
`PreEngineSugaringStage`. Confirmed.

**Docs**: contract at
`claude-notes/designs/document-profile-contract.md`; both this
Phase-0 plan and the parent epic plan must link to it. When Q2 is
documenting itself (see `bd-tr81`), this may move / be mirrored.

**Out-of-scope sanity check**: defer `ProjectType` trait naming
confirmation to Phase 1 — not needed for Phase 0.
