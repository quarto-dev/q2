# FileId/span hardening — Phase A (q2-side fixes + guardrails)

**Parent strand:** bd-nv4p0eb1 (audit + API hardening)
**Assessment:** `claude-notes/research/2026-08-09-fileid-span-integrity-audit.md`
**Approved:** 2026-08-09 (sequencing + open questions resolved in review; see
research doc §6/§7)

## Overview

Execute step 1 of the agreed sequencing: fix every verified q2-side bug from
the audit, complete the candidate lists at the correct-but-incomplete
diagnostic sites, and land the lint guardrail — all without waiting on the
upstream (quarto-source-map / quarto-yaml) API redesign. Step 2 (the
quarto-source-map memo) and step 3 (Option C plan) follow after this phase.

Working conventions for this phase:
- One strand per work item, TDD (red test first), one branch per strand
  (`braid/<id>-<slug>` off `main` unless noted).
- bd-x113wg9v (D3) and the candidate-list work (D5/D6) depend on PR #478's
  `bind_config_source` + `extension_manifest_paths` — branch those off the
  #478 branch (or main after it merges).
- Full workspace verification per CLAUDE.md before any commit is declared
  done (`cargo build --workspace`, `cargo nextest run --workspace`,
  `cargo xtask verify --skip-hub-build` minimum; full verify when
  quarto-core/pampa/quarto-pandoc-types change — that is most of this plan).

## Work items

### Bug fixes (verified in audit)

- [x] **P1 / bd-itj2mjkr (p1):** engine intermediate FileId slot desync.
      Fixture: project (`_quarto.yml`) + doc with executable cell; red test
      asserts engine-produced block FileIds resolve to the intermediate
      slot, not `_quarto.yml`. Fix: use the id actually returned by
      `add_file`/`add_file_with_info` in `engine_execution.rs` (drop the
      `filenames.len()` derivation); `debug_assert!` documenting the
      files/filenames relationship; fix the false lock-step comment.
      *Done 2026-08-09, commit 9d1c2581 on
      braid/bd-itj2mjkr-engine-slot-desync (remap made conditional, not
      additive; debug_assert judged unnecessary once the id comes from
      the add_file return). Awaiting PR/merge before strand close.*
- [x] **D3 / bd-x113wg9v (p2):** doc-level `resource_error_to_parse_error`
      mis-bind. Depends on PR #478. Fixture: `resources:` pattern declared in
      `_metadata.yml`, assert Q-5-1 snippet names the `_metadata.yml`. Fix:
      route through `bind_config_source` with candidates
      `[doc path, config_path] ++ extension_manifest_paths ++ dir-layer paths`.
      *Done 2026-08-09 via new bind_source_candidates (two-scheme pairs)
      + directory_metadata_paths_for_document; both temporary lint
      blessings removed in the same commit.*
- [x] **P2 / bd-f6h40a9r (p2):** `writers/incremental.rs:704-708` + `:747-766`
      foreign-offset fallback. Fix: `preimage_in` miss ⇒ re-serialize the
      inline (no `inline_source_span` fallback into foreign coordinates);
      audit `assemble_recursed_container` the same way.
      *Done 2026-08-09, commit 978dba6b. Scope grew to two more raw-offset
      sites found during TDD: block-level Verbatim coarsening and
      compute_separator (a foreign kept block panicked there on the red
      run). All bail to Rewrite/standard separator on preimage miss.*
- [x] **P3 / bd-t3enk8gq (p2):** `section.rs:126-139` + `pipe_table.rs:253-261`
      caption hulls. Fix: route through `hull_source_infos` (same-file
      checked, `preimage_in`-based); kill the `unwrap_or(FileId(0))`.
      *Done 2026-08-09. TDD found a third defect at both sites: raw
      Substring offsets stamped as file-absolute, reachable via
      qmd::read's public parent_source_info parameter.*
- [x] **P5 / bd-vmlhw7nx (p3):** `transforms/attribution_render.rs:176-181` gate
      on literal fid `0`. Fix: thread the blamed file's `FileId` onto
      `AttributionData` and compare against it.
      *Done 2026-08-09 (staged red: field threaded with literal-0 gate
      kept, tests red, then gate fixed).*
- [x] **P6 / bd-thagcbfq (p3):** `pampa/src/lua/types.rs` `byte_range()` drops the
      fid. Fix: return the fid (third field / table entry) and make
      `quarto.attribution.lookup_range` refuse non-primary ranges.
      *Done 2026-08-09. lookup_range's file check is an optional third
      arg compared Rust-side against AttributionLookup::blamed_file_id
      (fed by bd-vmlhw7nx's AttributionData.file_id); two-arg calls keep
      the historical contract.*
- [x] **D4 / bd-h5rfw3ao (p3):** harden `project_type_error`
      (`project/mod.rs:903`) — route through `bind_config_source` (or add the
      hash-equality guard) + a test pinning the invariant.
      *Done 2026-08-09: bind_config_source over config_path ++ manifest
      paths; dead content params removed; pin test in
      unknown_project_type.rs.*

### Coverage completion (correct sites, incomplete candidates)

- [x] **D5 / bd-r64mj1aa (p2, covers D6/D7):** `commands/render.rs` `attach_config_source` /
      `config_source_context`: extend candidates with
      `extension_manifest_paths` (+ dir-layer `_metadata.yml` paths for the
      per-page groups). Depends on PR #478.
      *Done 2026-08-09, all three legs; per-page layer coverage comes via
      D7's metadata_merge registration on the structured path; new shared
      register_config_source; zero lint allowances left in render.rs.*
- [x] **D6 (in bd-r64mj1aa):** `theme_error_candidates`
      (`compile_theme_css.rs:633`): add `extension_manifest_paths` and the
      doc's dir-layer paths.
- [x] **D7 (in bd-r64mj1aa):** register extension manifests in
      `MetadataMergeStage` so doc-scoped diagnostics anchored in
      `_extension.yml` get spans.
- [ ] **D9 / bd-fc3mf161 (p3, may defer):** `pipeline.rs:800/:834/:1006` — thread
      the document's real SourceContext into the `StageError` fallback arm
      instead of rebuilding a single-file context.

### Guardrails

- [x] **Lint / bd-jrq4hroi (p2):** xtask lint rule — `add_file_with_id` allowed
      only in blessed modules (`config_sources.rs`,
      `stage/stages/metadata_merge.rs`, `span_assert.rs`, test code);
      everything else must use `bind_config_source`. Suppression comment
      convention consistent with `metadata-as-str`.
      *Done 2026-08-09. render_scripts.rs + project_resources.rs
      temporarily blessed (PR #478 conflict avoidance) — remove from
      BLESSED_SUFFIXES when #478 and bd-x113wg9v land.*
- [x] **quarto-xml / bd-y5gpc8yv (p3):** replace hardcoded `FileId(0)`
      (`quarto-xml/src/parser.rs:27,43,83`) — in-tree; can ride any strand
      or the upstream version bump (UNKNOWN sentinel arrives with Option B).
      *Done 2026-08-09: exported ANONYMOUS_FILE_ID = FileId(usize::MAX)
      used by all three anonymous entry points; migrate to the upstream
      reserved id when Option B lands. Follow-up idea recorded on the
      strand: citeproc's load_csl_style knows config.csl and can use
      parse_with_file_id once CSL spans start rendering.*

### External communication

- [x] GitHub issue on posit-dev/quarto-yaml recording V1-V3
      (validation-crate context mismatch, SourceRange coordinate mixing,
      inline hash reimplementation) + Y1/Y2/Y4, noting the planned API
      redesign. Filed 2026-08-09:
      https://github.com/posit-dev/quarto-yaml/issues/17

### Phase exit

- [ ] All strands above closed (tests green, e2e-verified where user-visible)
- [ ] Write the step-2 memo for the quarto-source-map agent (Option B API,
      deprecation plan, quarto-yaml migration; declares Option C end-state)
      — separate plan doc, linked from bd-nv4p0eb1
- [ ] Update bd-nv4p0eb1 with phase-A completion evidence

## Session log

- **2026-08-09 (session 1, audit + start):** research doc + this plan;
  strands filed; posit-dev/quarto-yaml#17 filed; bd-itj2mjkr done.
- **2026-08-09 (session 1, cont.):** bd-f6h40a9r, bd-t3enk8gq,
  bd-jrq4hroi, bd-vmlhw7nx, bd-thagcbfq, bd-y5gpc8yv done — 7 of 11
  q2-side items. All merged to `feature/bd-nv4p0eb1-span-hardening`
  (`--no-ff`, one merge per strand). Full `cargo xtask verify` green at
  the P1+P2 point and re-run at branch head at session end. Discovered:
  bd-u0tldu4z (flaky quarto-hub admin_collect_lifecycle test, unrelated
  to this work — no quarto-xml dep edge, green in isolation and on
  rerun). NOT pushed — awaiting user approval.
- **Next session:** bd-fc3mf161 (D9, may defer) is the only unblocked
  code item; bd-x113wg9v / bd-h5rfw3ao / bd-r64mj1aa wait on PR #478's
  merge (then also remove the two temporary BLESSED_SUFFIXES entries in
  the lint). Then the step-2 quarto-source-map memo (phase exit).

## Details / decisions

- Pin-parse-time-content semantics confirmed (research doc §7.2); Option C
  memory cost accepted (§7.3); no canonicalization inside quarto-source-map —
  `SystemRuntime::canonicalize` is the per-setting normalizer, policy
  enforced at q2 minting sites (§7.1).
- P1 fix note: do NOT retire `ASTContext.filenames` in this phase — that is
  a larger structural change (research doc §5 "q2-internal structural
  cleanups") entangled with the JSON writer's positional pairing (P4);
  phase A only makes the engine stage stop deriving ids from it. P4 itself
  (positional serialization) is deferred to the Option B/C work where id
  tables become explicit.
