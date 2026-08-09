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
- [ ] **D3 / bd-x113wg9v (p2):** doc-level `resource_error_to_parse_error`
      mis-bind. Depends on PR #478. Fixture: `resources:` pattern declared in
      `_metadata.yml`, assert Q-5-1 snippet names the `_metadata.yml`. Fix:
      route through `bind_config_source` with candidates
      `[doc path, config_path] ++ extension_manifest_paths ++ dir-layer paths`.
- [x] **P2 / bd-f6h40a9r (p2):** `writers/incremental.rs:704-708` + `:747-766`
      foreign-offset fallback. Fix: `preimage_in` miss ⇒ re-serialize the
      inline (no `inline_source_span` fallback into foreign coordinates);
      audit `assemble_recursed_container` the same way.
      *Done 2026-08-09, commit 978dba6b. Scope grew to two more raw-offset
      sites found during TDD: block-level Verbatim coarsening and
      compute_separator (a foreign kept block panicked there on the red
      run). All bail to Rewrite/standard separator on preimage miss.*
- [ ] **P3 / bd-t3enk8gq (p2):** `section.rs:126-139` + `pipe_table.rs:253-261`
      caption hulls. Fix: route through `hull_source_infos` (same-file
      checked, `preimage_in`-based); kill the `unwrap_or(FileId(0))`.
- [ ] **P5 / bd-vmlhw7nx (p3):** `transforms/attribution_render.rs:176-181` gate
      on literal fid `0`. Fix: thread the blamed file's `FileId` onto
      `AttributionData` and compare against it.
- [ ] **P6 / bd-thagcbfq (p3):** `pampa/src/lua/types.rs` `byte_range()` drops the
      fid. Fix: return the fid (third field / table entry) and make
      `quarto.attribution.lookup_range` refuse non-primary ranges.
- [ ] **D4 / bd-h5rfw3ao (p3):** harden `project_type_error`
      (`project/mod.rs:903`) — route through `bind_config_source` (or add the
      hash-equality guard) + a test pinning the invariant.

### Coverage completion (correct sites, incomplete candidates)

- [ ] **D5 / bd-r64mj1aa (p2, covers D6/D7):** `commands/render.rs` `attach_config_source` /
      `config_source_context`: extend candidates with
      `extension_manifest_paths` (+ dir-layer `_metadata.yml` paths for the
      per-page groups). Depends on PR #478.
- [ ] **D6 (in bd-r64mj1aa):** `theme_error_candidates`
      (`compile_theme_css.rs:633`): add `extension_manifest_paths` and the
      doc's dir-layer paths.
- [ ] **D7 (in bd-r64mj1aa):** register extension manifests in
      `MetadataMergeStage` so doc-scoped diagnostics anchored in
      `_extension.yml` get spans.
- [ ] **D9 / bd-fc3mf161 (p3, may defer):** `pipeline.rs:800/:834/:1006` — thread
      the document's real SourceContext into the `StageError` fallback arm
      instead of rebuilding a single-file context.

### Guardrails

- [ ] **Lint / bd-jrq4hroi (p2):** xtask lint rule — `add_file_with_id` allowed
      only in blessed modules (`config_sources.rs`,
      `stage/stages/metadata_merge.rs`, `span_assert.rs`, test code);
      everything else must use `bind_config_source`. Suppression comment
      convention consistent with `metadata-as-str`.
- [ ] **quarto-xml / bd-y5gpc8yv (p3):** replace hardcoded `FileId(0)`
      (`quarto-xml/src/parser.rs:27,43,83`) — in-tree; can ride any strand
      or the upstream version bump (UNKNOWN sentinel arrives with Option B).

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
