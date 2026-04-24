# Merge `main` into `feature/websites`: order IncludeExpansion before DocumentProfile

**Date:** 2026-04-24
**Beads:** `bd-xfwx`
**Parent plan / epic:** `claude-notes/plans/2026-04-23-website-project-epic.md`
  (epic `bd-0tr6`). Gates the start of Phase 4 on `feature/websites`.
**Status:** Draft — awaiting user approval before any git operations.

## Goal

Merge the current state of `main` into `feature/websites` so that the
new **include-shortcode expansion** pipeline stage (landed on `main`
in `215482fb` on 2026-04-20) sits **before** the **DocumentProfile
checkpoint** (landed on `feature/websites` in `e8674612` as Phase 0 of
the website epic).

In pipeline terms, the post-merge order of the HTML pipeline must be:

```
Parse
MetadataMerge
IncludeExpansion        ← from main
DocumentProfile         ← from feature/websites (profile checkpoint)
UnwrapProfile           ← from feature/websites
PreEngineSugaring
EngineExecution
CompileThemeCss
UserFilters(pre)
AstTransforms
UserFilters(post)
CodeHighlight
RenderHtmlBody
ApplyTemplate
```

The user-visible contract this establishes: **statically-knowable
information a document declares via the `{{< include … >}}`
shortcode is visible to `DocumentProfile`**. Heading outline, code
blocks, crossref targets, and any other AST-shaped content that
arrives through an include should be reflected in the profile that
downstream project-level features (sidebars, nav, cross-doc links,
incremental rebuild cache, future `freeze`) read from.

The symmetric order choice — IncludeExpansion *after* DocumentProfile
— is explicitly rejected: profiles would then be computed from the
pre-include AST, making cross-document features inconsistent with
what the document actually renders.

## Scope

In scope:

1. Merge `main` (HEAD `349148ae` at time of writing) into
   `feature/websites` locally, no push.
2. Resolve the pipeline-ordering conflicts such that IncludeExpansion
   runs **immediately after** `MetadataMergeStage` and **immediately
   before** `DocumentProfileStage`, in all three pipeline builders
   (`build_html_pipeline_stages_with_apply_config`,
   `build_wasm_html_pipeline`, `build_analysis_pipeline`).
3. Update the stage-count / stage-name assertions in
   `crates/quarto-core/src/pipeline.rs` tests to reflect the new
   ordering.
4. Add a regression test that demonstrates the ordering contract
   end-to-end: a parent qmd with `{{< include child.qmd >}}` produces
   a `DocumentProfile` whose `outline` contains headings defined in
   `child.qmd`. **Write this test before performing the merge
   resolution** (TDD per CLAUDE.md), on a scratch branch off
   `feature/websites`, verify it fails for the "right reason" (the
   IncludeExpansion stage does not exist on `feature/websites`), then
   carry it into the merge resolution and verify it passes.
5. Run `cargo xtask verify` clean before proposing the push.

Out of scope for this session:

- Pushing the merged branch (user will approve separately).
- Shortcode resolution beyond `include` — e.g. `{{< meta … >}}` or
  user-defined shortcodes in Lua — is intentionally not moved
  relative to the checkpoint. That remains
  `ShortcodeResolveTransform`'s responsibility inside
  `AstTransformsStage`, well after the profile. If the user wants
  general shortcode resolution pre-profile later, it is a separate
  design conversation.
- Changing the `DocumentProfile` contract itself (adding fields that
  only become knowable because of includes). If the current profile
  fields cover the cases the user cares about, no contract change is
  needed. If a new field is wanted, file a follow-up beads issue.
- Merging `main` → `feature/websites` at a future, different
  snapshot of `main`; this plan targets the current
  `main` HEAD and freezes that as the merge source for reproducibility.

## Reference material

- Parent epic: `claude-notes/plans/2026-04-23-website-project-epic.md`
- Phase-0 plan (which established the checkpoint):
  `claude-notes/plans/2026-04-23-websites-phase-0.md`
- DocumentProfile contract doc:
  `claude-notes/designs/document-profile-contract.md`
- Include-expansion plan from main:
  `claude-notes/plans/2026-04-18-plan0-include-expansion-and-source-info.md`
- Commits being pulled in from `main` (the key two called out by the
  user):
  - `215482fb` — "Add include shortcode expansion pipeline stage"
  - `ca765b32` — "Restructure TS engine plans: unified @quarto/api +
    split Plan 1a" (plan-docs restructure; no functional overlap with
    the website epic)
- Other commits on `main` not yet on `feature/websites` (all
  relevant to the merge but not to the pipeline ordering specifically):
  - `796a656a` — "Wire SourceInfo into ExecutionContext for engine
    source provenance" (prerequisite of `215482fb`; modifies
    `EngineExecutionStage::run` and `ExecutionContext`)
  - `b47dd01b` — "Add write_with_source_info to QMD writer"
  - `1ede8685` — "Add Block::source_info() / Inline::source_info()
    accessors"
  - `1cef8e8d` — "Add TS engine extensions grand plan and subplans"
    (docs-only)
  - `b1abee84` — "Add task to beads" (data file)
  - `4dace404` — "Sync beads: note PR #116 merge on bd-itj9"
  - `50274d2b` — "Add smoke-all fixtures for include shortcode
    expansion" (five `crates/quarto/tests/smoke-all/includes/*`
    fixtures and downgrades two diagnostics from ERROR to WARNING)
  - `b2a48a35` — "Refine TS engine extension plans" (docs-only)
  - `349148ae` — "Make `cargo xtask verify` unconditional in push
    checklist" (docs-only)

## Pre-merge investigation (done)

Summarized findings; full details in the commit messages and diffs.

1. **IncludeExpansion is a `DocumentAst → DocumentAst` stage.** Sits
   cleanly between `MetadataMerge` (→ `DocumentAst`) and
   `DocumentProfile` (`DocumentAst` → `AtProfile`) without any
   adapter changes.
2. **IncludeExpansion runs after MetadataMerge deliberately.** It
   needs the fully-merged metadata to resolve paths and operates on
   the parent document's AST. *Included* files have their YAML
   frontmatter stripped — Q1 parity. The profile reads the parent's
   metadata + the post-include AST; included-file frontmatter never
   reaches the profile (this is correct behavior; included files are
   bodies, not documents).
3. **IncludeExpansion registers included files in both
   `SourceContext`s on `DocumentAst`** (`ast_context.source_context`
   for offset resolution and the top-level `source_context` for
   ariadne snippets). Source info on spliced blocks is remapped to
   the included file's `FileId`. This means headings that end up in
   the profile's `outline` via an include will still carry correct
   source locations.
4. **Main also contains ordering-neutral changes** in
   `EngineExecutionStage` (source-info wiring) from `796a656a`.
   `feature/websites` did not modify `engine_execution.rs`, so that
   file merges cleanly.

## Merge conflicts expected

A dry-run of `git merge --no-commit --no-ff main` from a worktree
will surface conflicts in at least:

1. `crates/quarto-core/src/pipeline.rs`
   - Use statement (`crate::stage::{…}`) — both sides added imports.
   - Doc comment listing pipeline stages — both sides renumbered.
   - `build_html_pipeline_stages_with_apply_config` — both sides
     inserted a stage after `MetadataMergeStage`.
   - `build_wasm_html_pipeline` — same shape.
   - `build_analysis_pipeline` — `main` inserted
     `IncludeExpansionStage`; `feature/websites` did not touch this
     function. Include the stage here too (LSP outline benefits from
     expanded content; symmetric with the render pipeline).
   - Tests `test_build_html_pipeline_stages`,
     `test_build_html_pipeline`, `test_build_wasm_html_pipeline`,
     `test_build_analysis_pipeline` — stage counts and stage-name
     position assertions need updating.
2. `crates/quarto-core/src/stage/mod.rs`
   - `pub use stages::{…}` — both sides added names. Merged set:
     `ApplyTemplateStage, AstTransformsStage, CompileThemeCssStage,
     DocumentProfileStage, EngineExecutionStage,
     IncludeExpansionStage, MetadataMergeStage, ParseDocumentStage,
     PreEngineSugaringStage, RenderHtmlBodyStage, UnwrapProfileStage,
     UserFiltersStage`.
3. `crates/quarto-core/src/stage/stages/mod.rs`
   - New module declarations on each side. Merged: `document_profile`,
     `include_expansion`, `unwrap_profile` all present.
4. `.beads/issues.jsonl`
   - Both sides updated. The correct resolution is to accept a
     *union* that preserves all closed/opened issues on both
     branches. After the textual merge, run
     `br import -i .beads/issues.jsonl --resolve-collisions` to
     reconcile any conflicting timestamps.

Other non-conflicting but relevant changes from `main` that will
simply apply: `pampa` lua-diagnostics split, qmd writer
source-info changes, new TS-engine plan docs, five `smoke-all/includes`
fixtures, hub-client presence/visibility-gating work. None of these
interact with the pipeline ordering. They must nevertheless compile
and pass tests on the merged tree.

## Work items (in execution order)

Each checkbox is one discrete step. We pause at step 4 for a
verification gate before doing the merge, and at the end for the
push-approval gate.

### Phase A — Scaffolding before the merge

- [x] **A0. Create a worktree** `.worktrees/include-merge` off
      `feature/websites` so the merge can be aborted cleanly without
      disturbing the main working tree. Add the beads redirect per
      `.claude/rules/worktrees.md`.
      *Done:* branch `merge/include-expansion`; `br where` confirms
      the redirect resolves to the main repo's `.beads/`.
- [x] **A1. Add a TDD-style regression test** exercising
      the ordering contract we want to land. The test lives in
      `crates/quarto-core/src/pipeline.rs` (or a new test file if the
      existing module is crowded) and:
        1. Writes `parent.qmd` and `child.qmd` to a temp dir; the
           child contains a `## Section A` heading and a `## Section B`
           heading; the parent contains `{{< include child.qmd >}}`.
        2. Builds the HTML pipeline, runs it up to the profile
           checkpoint (this is already exercised by the existing
           `AtProfile` clone-and-resume test in Phase 0 — reuse its
           halting idiom; if it halts by stopping at a given stage,
           halt right after `DocumentProfileStage`).
        3. Asserts the extracted `DocumentProfile.outline` contains
           "Section A" and "Section B".
      Expected failure on `feature/websites` HEAD: the test fails
      because there is no `IncludeExpansionStage`, so the `{{< include
      >}}` shortcode is still present as an unresolved paragraph at
      profile time and the heading does not appear in `outline`.
      **Do not proceed to the merge until this test has been verified
      to fail for exactly that reason.** (This is the key TDD check
      that the merge fix actually fixes something.)
      *Done:* test `profile_sees_heading_from_included_file` added in
      `crates/quarto-core/tests/document_profile_pipeline.rs`. Landed
      with one heading (`## Child Heading`) rather than two sections —
      same contract, lighter fixture. Verified RED on
      `merge/include-expansion` (pre-merge HEAD): fails with
      `got outline titles: []` — the `{{< include >}}` shortcode is
      not expanded and the parent has no headings of its own.
- [x] **A2. Also add a stage-ordering assertion** to
      `test_build_html_pipeline_stages`: the position of
      `"include-expansion"` must be strictly less than the position
      of `"document-profile"`. This is a cheap structural guard
      against future refactors silently reordering the two stages.
      *Done:* `include_expansion_precedes_document_profile` added to
      `tests/document_profile_pipeline.rs` (kept with the other
      pipeline-shape assertions rather than in `pipeline.rs` unit
      tests). Verified RED pre-merge: panics on
      `.expect("include-expansion stage must be present ...")`.

### Phase B — Perform the merge

- [x] **B0. Fetch** latest `main` (no working-tree mutation beyond
      the fetch).
      *Done:* `git fetch origin main` → FETCH_HEAD at `349148ae`.
- [x] **B1. From the worktree**, run `git merge --no-ff --no-commit
      main` to produce a merge commit buffer with conflicts surfaced
      but nothing yet recorded.
      *Done:* two conflicts reported, in `pipeline.rs` and
      `stage/mod.rs`. `stage/stages/mod.rs` and `.beads/issues.jsonl`
      auto-merged.
- [x] **B2. Resolve conflicts** file-by-file, using the merge-target
      pipeline order from the §Goal section. Specifically:
        - `pipeline.rs`: imports union; doc comments renumbered for
          the final order; three builder functions each get both
          `IncludeExpansionStage::new()` *and* `DocumentProfileStage`
          / `UnwrapProfileStage` inserted in the target order; tests
          updated with the new stage counts. Target counts after
          merge:
            - native HTML: **14** stages (current `feature/websites`
              has 13 — inserts `IncludeExpansionStage` at position
              3, shifting `DocumentProfileStage` to 4, etc.).
            - WASM: **13** stages (current `feature/websites` has
              12 — same insertion; WASM still has no
              `EngineExecutionStage`).
            - analysis: **5** stages (current `feature/websites`
              has 4 — insertion between `MetadataMergeStage` and
              `PreEngineSugaringStage`).
        - `stage/mod.rs` and `stage/stages/mod.rs`: accept both
          sides' additions.
        - `.beads/issues.jsonl`: union of both sides; `br import
          --resolve-collisions` after the textual resolution to
          reconcile.
        - Any incidental conflicts in `CLAUDE.md`,
          `claude-notes/plans/*`, etc.: prefer the `main` version for
          files that main substantially rewrote (e.g. TS-engine
          plans) and the `feature/websites` version for website-epic
          plans.
- [x] **B3. Do not commit yet.** Leave the merge in-progress; move
      to Phase C to verify before committing.
      *Done:* merge left in-progress until C0-C3 passed.

### Phase C — Verification

- [x] **C0. `cargo build --workspace`** — compiles cleanly.
      *Done:* 2m 30s, exit 0.
- [x] **C1. `cargo nextest run --workspace`** — all tests pass.
      Special attention to:
        - The new regression test from A1 (must now pass — this is
          the green half of the TDD cycle).
        - The new structural assertion from A2.
        - `test_build_html_pipeline_stages`,
          `test_build_html_pipeline`, `test_build_wasm_html_pipeline`,
          `test_build_analysis_pipeline` — stage counts.
        - Phase-0 profile-checkpoint clone-and-resume tests (the
          byte-identical-resume guarantee must still hold; adding
          IncludeExpansion before the checkpoint does not change
          this because include expansion is deterministic).
        - Phase-2 sidebar / Phase-3 navbar/footer project-integration
          tests (they read the profile; adding headings via include
          must not break them).
      *Done:* 7750 tests, 0 failed, 195 skipped. Focused re-run of
      `document_profile_pipeline` confirmed:
      `profile_sees_heading_from_included_file` and
      `include_expansion_precedes_document_profile` both PASS
      (the TDD GREEN); Phase-0
      `pipeline_at_profile_to_end_produces_expected_html`
      (clone-and-resume byte-identical invariant) still PASSes.
- [x] **C2. `cargo xtask verify`** — full workspace + hub-client
      build + hub-client tests. Required because `quarto-core` is on
      the conflict path.
      *Done:* `cargo xtask verify --skip-rust-tests` (since C1
      already covered Rust). Initially failed on a pre-existing
      nightly-rustc issue (`VaList::next_arg` rename from
      `f866c65e` required a rustc newer than local `1.94.0-nightly
      2026-01-14`). After `rustup update nightly` →
      `1.97.0-nightly 2026-04-23` and a root `npm install` (fresh
      worktree had no `node_modules/`), verify reported
      **"All verification steps passed!"**
- [x] **C3. End-to-end CLI smoke** per the CLAUDE.md
      end-to-end-verification rule.
      *Done:* `cargo run --bin q2 -- render
      crates/quarto/tests/smoke-all/includes/basic/basic.qmd`
      produced `basic.html` (782 bytes). Inspected — the rendered
      HTML contains the three expected paragraphs in order:
      ```
      <p>Parent content before include.</p>
      <p>This line contains BASIC-CHILD-MARKER-XYZ from the included file.</p>
      <p>Parent content after include.</p>
      ```
      confirming `{{< include _child.qmd >}}` was resolved during
      render (and, by construction of the pipeline order,
      *before* the profile checkpoint). Build artifacts (`basic.html`,
      `basic_files/`) removed after inspection; not staged.
      (Plan note: CLI binary is `q2`, not `quarto` — the earlier
      draft of this step said `--bin quarto`, corrected at execution
      time.)
- [ ] **C4. Finalize the merge commit.** `git commit` (the merge
      buffer is still in progress). Use a descriptive message along
      the lines of:

      ```
      Merge main into feature/websites

      Threads include-shortcode expansion (main, 215482fb) through the
      DocumentProfile checkpoint (feature/websites, e8674612): the
      merged HTML pipeline runs IncludeExpansionStage immediately after
      MetadataMergeStage and immediately before DocumentProfileStage,
      so statically-knowable content declared via {{< include … >}}
      (headings, code blocks, crossref targets) is visible in the
      profile that downstream project features consume.

      See claude-notes/plans/2026-04-24-include-expansion-merge.md
      for the merge plan and rationale.
      ```

### Phase D — Follow-ups and handoff

- [ ] **D0. `br sync --flush-only`** in the main repo (not the
      worktree); `git add .beads/ && git commit -m "sync beads"`.
- [ ] **D1. Update the epic plan file**
      (`claude-notes/plans/2026-04-23-website-project-epic.md`) §Work
      items with a note that this merge landed and the checkpoint
      contract now includes post-expansion content. Reference this
      plan.
- [ ] **D2. File a follow-up beads issue** if the regression tests
      surface that the profile contract should gain a field (e.g.
      "is this document an include target of any other document in
      the project?" — a question that becomes meaningful once
      project orchestration inspects profiles). Only file if
      actually needed; don't pre-file speculatively.
- [ ] **D3. Propose push to the user.** Do not push without explicit
      approval. `git push origin feature/websites` only after the
      user says yes.
- [ ] **D4. Delete the worktree** once the merge is on
      `feature/websites` proper:
      `git worktree remove .worktrees/include-merge`.

## Test strategy (recap)

- **Structural:** stage-ordering assertion in `pipeline.rs` tests
  (A2); stage-count assertions updated for all three builders.
- **Contract:** A1 — profile-sees-included-heading regression test.
  Failing-then-passing is the TDD proof that the merge does what
  this plan claims.
- **Regression:** full workspace nextest + `cargo xtask verify`.
- **End-to-end:** CLI smoke render of a smoke-all/includes fixture
  per CLAUDE.md §End-to-end verification.

## Risks and mitigations

- **Risk:** Phase-0 clone-and-resume byte-identical-output guarantee
  breaks because IncludeExpansion runs before the clone point.
  *Mitigation:* that test clones *at* the profile checkpoint, which
  under the new ordering is already past IncludeExpansion; both
  branches of the clone then resume with the same post-include AST.
  No change needed to the guarantee.
- **Risk:** Phase 2/3 tests hard-code the parent-document-only view
  of the AST and break if an included heading shows up in the
  outline. *Mitigation:* Phase 2/3 tests use in-memory fixtures
  without `{{< include >}}` shortcodes — they won't trigger the new
  behavior. Verify in C1.
- **Risk:** `build_analysis_pipeline` (LSP) gains IncludeExpansion,
  and some LSP operation that previously saw the unresolved
  `{{< include >}}` paragraph now sees the spliced content,
  surprising an LSP test. *Mitigation:* LSP outline wants the
  spliced content (that's what a user expects to see in the
  outline), so this is the right behavior. If tests catch a
  regression in some other LSP feature, the fix is to update that
  feature, not to skip include expansion in the analysis pipeline.
- **Risk:** `.beads/issues.jsonl` merge produces a malformed file.
  *Mitigation:* always run `br import --resolve-collisions` after the
  textual merge and verify `br ready` works before committing.
- **Risk:** Some hub-client test depends on the exact pipeline stage
  list (unlikely but possible). *Mitigation:* `cargo xtask verify`
  catches this before push.

## Open questions to resolve during execution

None blocking. The following are "decide in-session if they come up":

1. **Should `build_analysis_pipeline` include `IncludeExpansionStage`?**
   Recommendation: yes. LSP outline should see included content.
   Confirm by reading any LSP tests that hit this pipeline before
   flipping it; if any test specifically asserts the unresolved
   shortcode is visible, that test has to change (and the bd issue
   should note it).
2. **Should we add a test that exercises `include` declaring
   a heading that then contributes to a `website.sidebar: auto`
   listing?** This is a higher-level test than what's in scope
   for the merge, but it's the best end-to-end proof of the user's
   stated goal ("documents declare statically-knowable information
   through shortcodes"). Recommendation: not in this plan; open a
   bd issue against Phase 2 / `auto:` sidebar listings to cover it
   once Phase 4+ work lands.
3. **Does the profile contract need a `includes: Vec<PathBuf>`
   field** tracking which files were spliced in? Might be useful
   for incremental rebuilds (a change to any included file should
   invalidate the parent's cached profile). Not required for this
   merge; file as Phase-8-adjacent follow-up if/when incremental
   cache keying needs it.

## Non-goals for this merge

- No new `DocumentProfile` fields.
- No changes to `IncludeExpansionStage` semantics.
- No changes to what Phase 0's `PipelineData::AtProfile`
  contains beyond the profile seeing the post-include AST.
- No push to `origin` without user approval.
- No merge into `main`; this is `main → feature/websites` only.
