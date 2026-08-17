# Coalesce repeated per-page diagnostics in project renders

**Strand:** bd-mg3ckvp7
**Related:** bd-9hlja (closed) — built `coalesce_by_source` and wired it for
`pass2_failures` only, explicitly deferring the successful-render case.

## Overview

Rendering `external-sources/connect-docs/docs-quarto-2` emits the same
warning once per rendered page when the underlying problem lives in a
*shared* file. Measured on 2026-07-31 (`cargo run --bin q2 -- render …`,
2519 lines of stderr):

- **186×** `Warning [Q-13-2]: Navbar references missing document
  'api/index.qmd'` — one identical copy per page, all anchored at the
  same `_quarto.yml` span.
- Related shapes at smaller counts: `Q-12-7` (template/type fallback,
  15×), listing `sort:` warnings (~10×), and unknown-shortcode warnings
  re-reported per includer when the shortcode lives in a shared include
  file (`{{< include ../include/_common.qmd >}}`).
- (The bulk of the ~75 unknown-shortcode and ~88 `Q-2-9` warnings are
  *distinct* source locations — legitimately separate diagnostics, out
  of scope.)

## Diagnosis

1. **Each per-document pipeline independently re-diagnoses shared
   inputs.** The navbar render transform
   (`crates/quarto-core/src/transforms/navbar_render.rs`) runs for every
   page and calls `resolve_href_for_html`
   (`crates/quarto-core/src/transforms/navigation_href.rs:152`), which
   pushes a fresh `Q-13-2` into that page's diagnostics on every miss.
   Same story for any diagnostic derived from `_quarto.yml` config or
   from included fragments.

2. **The CLI prints successful-render page diagnostics verbatim, with no
   coalescing.** `print_render_diagnostics_text`
   (`crates/quarto/src/commands/render.rs:946-954`) loops over
   `summary.outputs` and prints each diagnostic. bd-9hlja routed only
   `pass2_failures` through `coalesce_by_source`; the comment at
   `render.rs:908-914` records the exact blocker: *"`RenderToFileResult`
   does not currently carry the input path."*

3. **The coalescing key already works for this case — verified
   empirically.** A temporary probe on a 3-page fixture (broken navbar
   href in `_quarto.yml`) showed every page's `Q-13-2` carries an
   identical location:

   ```
   location=Some(Substring { parent: Original { file_id: FileId(6053980132863075055),
     start_offset: 0, end_offset: 122 }, start_offset: 88, end_offset: 99 })
   resolve_byte_range=Some((6053980132863075055, 88, 99))
   ```

   `_quarto.yml` values get their `FileId` from
   `quarto_yaml::file_id_for_filename` (a hash of the path), so the id
   is stable across all per-document `SourceContext`s.
   `coalesce_by_source`'s `LocationKey` = `(file_id, start, end)` would
   collapse all 186 into one group.

4. **Hazard: raw `file_id` in the key is unsafe across documents.**
   Pampa's per-document contexts use *sequential* FileIds (primary file
   = `FileId(0)`). Two page-local diagnostics in *different* documents
   at the same byte offsets produce the same `(0, start, end)` key and
   would falsely merge. bd-9hlja never hit this because `pass2_failures`
   theme errors anchor at hash-based ids. Routing *all* per-page
   diagnostics through the upstream coalescer as-is would introduce
   false merges. The key must become **(resolved file path when the
   entry's own `SourceContext` can name the file, else raw file_id,
   start, end)**. Hash-based ids (config files, not registered in
   per-doc contexts) fall back to the raw id, which is path-derived and
   therefore stable and collision-safe; sequential ids resolve to
   distinct paths per document.

5. **Secondary UX gap: the warning names no source at all.** The
   `Q-13-2` text block renders without any file/line/snippet, because
   the per-doc `SourceContext` handed to `to_text` doesn't contain
   `_quarto.yml` (hash id, never registered). Even a fully coalesced
   single warning would still not tell the user *where* the bad href
   is. `theme_diagnostic.rs:69` already demonstrates the fix pattern:
   read the file, `add_file_with_id(file_id_for_filename(path), …)`.

## Design decisions (proposed — iterate here)

- **D1: Fix the key upstream first, then consume it here.**
  `coalesce_by_source` lives in `quarto-error-reporting`
  (`posit-dev/quarto-error-reporting`, published to crates.io; q2 pins
  `0.1.0`). We own the crate, and the path-aware key is a small,
  self-contained bug fix: `coalesce_by_source` already receives
  `Option<SourceContext>` per entry, so `LocationKey` can resolve
  `FileId → file path` through the entry's own context (falling back
  to the raw id for unregistered hash-based ids) **without any
  signature change**. Sequence: file the upstream issue → land the fix
  + collision-guard unit tests there → publish `0.1.1` → bump the
  version dep here and route the successful-render pool through the
  upstream coalescer directly. This avoids writing throwaway local
  grouping code and a later migration; the q2-side plumbing (Phases
  1–2) is independent and can proceed in parallel while the release is
  in flight. (Rejected alternative, kept for the record: local
  grouping in q2 reusing the public `CoalescedDiagnostic` renderer —
  only worth it if the upstream release were expensive, which it
  isn't for a crate we control.)
- **D2: Print-only change.** `diagnostic_counts()`, `--strict`
  promotion, and exit codes keep operating on the un-coalesced
  per-page diagnostics. Coalescing affects only the text emission.
- **D3: Sections stay separate in v1.** `pass2_failures` (already
  coalesced), `project_diagnostics`, and the new coalesced
  successful-render pool print as today's three sections, in that
  order. Merging failures + successes into one pool is a possible
  follow-up, not v1.
- **D4: `--json-errors` unchanged in v1.** Programmatic consumers get
  one `JsonDiagnostic` per page occurrence today; keeping that
  preserves per-page attribution (the hub-client overlay depends on
  per-page delivery). A future `affected_files` field is a schema
  change → separate strand if wanted.
- **D5: Snippet restoration for config-anchored groups** (fixes
  §Diagnosis 5): at print time, when a group's representative location
  doesn't resolve against its carried `SourceContext`, attempt to
  register the project config file(s) (`_quarto.yml`, and the profile
  variants in play) under `quarto_yaml::file_id_for_filename` with
  content read from disk — mirroring `theme_diagnostic.rs`. Best-effort;
  on any failure, render span-less exactly as today.

## Work items

### Phase 0 — upstream fix in posit-dev/quarto-error-reporting

- [x] File the upstream issue: `LocationKey` keys on raw `file_id`;
  sequential per-document FileIds falsely merge diagnostics from
  different files at identical offsets (see §Diagnosis 4 for the q2
  reproduction context).
  Filed: <https://github.com/posit-dev/quarto-error-reporting/issues/3>
- [x] Land the fix there (TDD in that repo): path-aware key —
  resolve `FileId → path` via the entry's own `SourceContext` when
  registered, fall back to raw `file_id` otherwise; no signature
  change to `coalesce_by_source`. (Done by another agent; shipped as
  **0.2.1**, not 0.1.1 — `FileKey::Path` / `FileKey::Raw` enum,
  `LocationKey::from(info, ctx)`.) Unit tests:
  - same hash-file-id + span across N entries → one group, N affected
    files in encounter order;
  - sequential-file-id collision (two contexts, each `FileId(0)`, same
    offsets, different registered paths) → **two** groups;
  - `location: None` → singleton pass-through;
  - mixed pool preserves encounter order.
- [x] Publish to crates.io. (Shipped as `0.2.1`.)

### Phase 1 — q2-side tests first (TDD; can start in parallel with Phase 0)

- [x] Integration test (`crates/quarto/tests/integration/` per the
  integration-test layout rule) driving the real binary or
  `print_render_diagnostics_text`'s input path: 3-page website fixture
  with a broken navbar href → exactly **one** `Q-13-2` block on
  stderr, with an `Affected files:` tail naming 3 pages.
- [x] Run new tests, verify they fail (red) before implementing.
  (Red confirmed: 5 copies / no tail; singleton test passed as expected.)

### Phase 2 — plumbing (independent of Phase 0)

- [x] Add the input path to `RenderToFileResult`
  (`crates/quarto-core/src/render_to_file.rs:127`), populated where the
  orchestrator/render_document_to_file constructs it. (This is the
  blocker bd-9hlja recorded.)

### Phase 3 — coalesced emission (needs Phase 0 published)

- [x] Bump `quarto-error-reporting` to `0.2.1` in the workspace
  `Cargo.toml` (and in `crates/wasm-quarto-hub-client/Cargo.toml`,
  which pins it independently — it is outside the workspace).
- [x] Route `summary.outputs[*].render_output.diagnostics` through
  `coalesce_by_source` in `print_render_diagnostics_text` (respecting
  `--quiet` as today).
- [x] Tests from Phase 1 go green (3/3).

### Phase 4 — config-file snippet restoration (D5)

- [x] Best-effort registration of project config sources at print time
  so the coalesced `Q-13-2` renders the `_quarto.yml` snippet with the
  offending span. (`attach_config_source` in
  `crates/quarto/src/commands/render.rs`: matches the group's FileId
  against `quarto_yaml::file_id_for_filename(project.config.config_path)`
  and registers the file's content under that id; scope is `_quarto.yml`
  only for now — open question 3 stands for `_metadata.yml`/profiles.)
- [x] Test: fixture render shows file/line for the navbar href
  (`config_anchored_warning_shows_config_snippet`; red confirmed with
  the attach call neutralized, green with it active).

### Phase 5 — verification

- [x] `cargo build --workspace`, `cargo nextest run --workspace`
  (10810 passed), full `cargo xtask verify` (exit 0, covering the
  0.2.1 bump through the WASM/hub legs), and a final
  `cargo xtask verify --skip-hub-build` (exit 0) after Phase 4.
  One unrelated flake seen once (`collect_reverification_…`,
  case-only id mismatch) — filed as bd-gypflveh; 5/5 green in
  isolation.
- [x] End-to-end on the testbed: re-rendered
  `external-sources/connect-docs/docs-quarto-2`
  (`cargo run --bin q2 -- render …`, output inspected). Q-13-2 went
  **186 → 1**; stderr 2519 → ~1050 lines; exit code unchanged (1, from
  pre-existing Q-5-3 errors). All other repeated classes verified to be
  genuinely distinct locations (Q-12-7's 15 hits = 15 distinct files).
  Observed emission:

  ```
  Warning: [Q-13-2] Navbar references missing document
      ╭─[ …/docs-quarto-2/_quarto.yml:44:15 ]
   44 │         file: api/index.qmd
      │               ──────┬──────
      │                     ╰──────── 'api/index.qmd' is not in the project index.
  ────╯
  ℹ Check the spelling, or confirm the target file is included in the render set.
  Affected files: …/admin/access-controls/index.qmd, … (and 183 others)
  ```

  Note the snippet now names `_quarto.yml:44` — before this work the
  warning carried no source pointer at all.
- [ ] File follow-up strands: optional `--json-errors` affected-files
  field; optional single-pool merge (D3).

## Open questions for review

1. The `Affected files:` tail lists the *pages* that re-reported the
   diagnostic. For a config-anchored warning like Q-13-2, "affected
   files" is arguably every page — is the tail even useful there once
   the `_quarto.yml` snippet renders (Phase 4)? Alternative: suppress
   the tail when the anchor file is a project config file, or reword to
   `Reported while rendering: …`.
2. Is `AFFECTED_FILES_CAP = 3` the right display cap (upstream const)?
   Local grouping means we could choose our own.
3. Should Phase 4 cover only `_quarto.yml`, or also `_metadata.yml` /
   profile configs / `_variables.yml`? (Same mechanism; just a list of
   candidate paths.)
4. Priority call: is `q2 preview`'s diagnostic surface in scope? (It
   consumes per-page diagnostics through a different path; coalescing
   there is a UI concern, likely fine to leave per-page.)
