# Listing `contents:` globs — provenance-based base-directory resolution

**GitHub issue:** https://github.com/quarto-dev/q2/issues/456
**Braid strand:** bd-v7ixzsp5 (bug, P1)
**Status:** in execution on branch `braid/bd-v7ixzsp5-listing-contents-globs-resolve`.

## Overview

A listing host in a subdirectory with `contents: ["*.qmd"]` picks up project-root
documents. Root cause: `matches_any_glob` in
`crates/quarto-core/src/transforms/listing_generate.rs` (and its mirror in
`crates/quarto-core/src/project/dependency_graph.rs`) matches every candidate
against **both** the host-relative and the project-relative path view, OR-ed
together. The project-relative fallback lets `*.qmd` from `sub/index.qmd` match
root-level `about.qmd` and `index.qmd`.

Agreed direction (Carlos, session 2026-08-06): do **not** chase Q1-exact
behavior. Instead, resolve each glob relative to the directory of the file where
the `contents:` entry was **written**, using the `ConfigValue` provenance
(`SourceInfo`) infrastructure. Front-matter globs resolve against the host's
directory; `_metadata.yml` globs against that file's directory; `_quarto.yml`
globs against the project root. This naturally extends to future metadata
sources.

## Observed defects (assessment matrix)

Five fixture projects were built (currently in session scratchpad under
`issue456/`; to be committed as test fixtures in Phase 1). Rendered with
`cargo run --bin q2 -- render <dir>` at 3ce0c095:

| Fixture | Mechanism | Declared glob(s) | Expected (new semantics) | Actual today |
|---|---|---|---|---|
| `p1-basic` | front matter, host `sub/index.qmd` | `*.qmd`, `!index.qmd` | only `sub/p1.qmd` | Home + About + P1 listed; 2× spurious `Q-13-4`; `href="about.qmd"` broken link; `href="index.html"` silently points at host's own page; negation ignored |
| `p2-dirmeta` | `blog/_metadata.yml`, host `blog/deep/index.qmd` | `deep/*.qmd` | `blog/deep/p1.qmd` | **empty listing** (glob resolved against host dir and project root, never against `blog/`) |
| `p3-projmeta` | `_quarto.yml` top-level `listing:` | `sub/*.qmd` | `sub/p1.qmd` on every host, project-root base | correct on root host (accidentally — host-relative == project-relative there) |
| `p4-root-host` | front matter, host at root | `posts/*.qmd` | posts a, b | correct (must not regress) |
| `p5-parent-glob` | front matter, host `sub/index.qmd` | `../rootpost.qmd` | `rootpost.qmd` | **empty listing** (`relative_to_dir` can't go up; project-relative literal `../…` never matches) |

Distinct bugs:

1. **Dual-view OR matching** (the reported bug). Wrong items, broken/wrong
   links, spurious `Q-13-4` (emitted twice per phantom item — thumbnail link +
   title link — via the dep-graph/link-resolution path).
2. **No provenance base for `_metadata.yml` globs** — p2 renders an empty
   listing. (Precedent: `adjust_paths_to_document_dir` in
   `project/mod.rs` already rebases `!path` values per layer at merge time but
   deliberately skips `Glob`.)
3. **`../` traversal unsupported** — p5 renders an empty listing.
4. **Negation globs (`!pattern`) silently ignored** — no negation support
   anywhere in the listing matcher; `!index.qmd` is matched literally (`!` is
   only special inside `[...]`), so it matches nothing and drops out.
5. **Spurious `Q-1-20`** ("Failed to parse metadata value as markdown") on any
   glob string containing `*` in front-matter `contents:` — DocumentMetadata
   context tries to parse the string as markdown before the listing parser's
   `as_plain_text` fallback recovers it.
6. **Silent glob corruption when the markdown parse *succeeds*** (fixture
   `p6-glob-corruption`, verified 2026-08-06): `contents: ["p*osts*.qmd"]`
   parses as emphasis, `as_plain_text` reconstructs `posts.qmd` (asterisks
   lost), the listing renders empty, and **no diagnostic fires at all**. Same
   root cause as #5; strictly worse failure mode.

## Design

### Provenance → base directory

At listing-parse time, each glob's `ConfigValue.source_info` is resolved to a
**base directory** (project-relative, forward slashes):

- `root_file_id()` on the value's `SourceInfo`:
  - Front matter is `Substring` → resolves to pampa's primary `FileId(0)` →
    base = host document's directory.
  - `_metadata.yml` / `_quarto.yml` layers are parsed by
    `quarto_yaml::parse_file`, whose FileIds are **hash-derived from the
    filename** (`quarto_yaml::file_id_for_filename`) → build a candidate
    inventory (document path, `_quarto.yml` path, each `_metadata.yml` on the
    doc's directory chain) and match. Worked precedent:
    `compile_theme_css.rs:379-384` does exactly this for theme diagnostics.
  - Unresolvable (programmatic/runtime metadata, `Generated` sentinels) → fall
    back to the host directory.
- `ListingContents::Glob(String)` becomes a struct variant carrying
  `pattern` + resolved `base_dir` (resolved eagerly at parse time so downstream
  consumers never need the file table).

### Single-view matching (seed of a shared glob-expansion API)

Replace the dual-view OR with: join `base_dir` + pattern, lexically normalize
(`..` segments collapse), then match the normalized project-relative pattern
against each candidate's project-relative path with the existing
`glob_match_path_or_dir` (bare-directory rule preserved). A pattern that
normalizes **outside the project root** matches nothing and emits a **new
diagnostic code** (register in `quarto-error-catalog`, next free code in the
listing/`Q-12-*` or glob-appropriate subsystem), so warnings-as-errors mode
fails loudly. One shared helper used by **both** `ListingGenerateTransform` and
`ProjectDependencyGraph::build` so the two sites cannot drift.

Decision (Carlos, 2026-08-06): design this helper as the seed of an **internal
glob-expansion API** for all of q2 — base-dir-anchored resolution, negation,
consistent defaults — with the intent that other glob consumers
(`project.render`, resources, etc.) migrate onto it over time, squashing the
Q1-inherited inconsistency where some globs default to `*.qmd` and others to
`**/*.qmd`. Migration of other consumers is out of scope here; file a
follow-up strand (`discovered-from:bd-v7ixzsp5`) when the API shape settles.

Consequence (intentional behavior change, shipping **silently** — Carlos,
2026-08-06: q2 is 0.*, no stability promises): a subdirectory host using a
project-relative glob (`posts/*.qmd` meaning root-level `posts/` from
`sub/index.qmd`) stops matching. This aligns with both Q1 and the new
provenance semantics. No transition diagnostic.

### DocumentProfile / dep graph

`DocumentProfile.listing_content_globs: Vec<String>` must carry base dirs so
graph edges agree with render-time resolution. Per the profile contract
(profiles are read-only; producers move earlier, fields get added):

- new field shape (e.g. `Vec<ListingContentGlob { pattern, base_dir }>`),
- `profile_version` bump,
- profile cache invalidates via the version bump (verify in
  `project/profile_cache.rs`).

Provenance resolution therefore happens **at profile-extraction time** (where
the metadata + file inventory is in scope) and at `parse_listings` time in the
render transform; both must use the same resolver helper.

### Negation

Partition a listing's `contents` into positive and negative (`!`-prefixed)
patterns. An item is included iff it matches ≥1 positive pattern (after
base-dir resolution) and 0 negative patterns (same base-dir resolution, minus
the `!`). If `contents` has only negative patterns, the positive set defaults
to `*.qmd` (mirroring the existing "absent contents" default — approved by
Carlos 2026-08-06). Order within the list does not matter. (Q1 uses
order-independent exclusion too; exact parity not required per issue
direction. Q1's defaults are known-inconsistent — `*.qmd` vs `**/*.qmd`
depending on code path — which is exactly what the shared glob API above is
meant to eventually fix.)

Dep-graph note: negative patterns only ever shrink the item set, so the graph
builder must apply them as well — otherwise excluded items still force
host re-renders.

### Interpretation of `listing.contents` (fixes defects #5 and #6)

The glob strings in `contents:` are semantically globs/paths, not markdown.
The markdown round-trip both spams `Q-1-20` (parse failure) and silently
corrupts globs whose asterisks parse as emphasis (parse success, defect #6).

Design (settled with Carlos 2026-08-06, addressing his schema concern): teach
`yaml_to_config_value` an **annotation source** — a small declarative table
mapping key paths to `Interpretation` (here: `listing.contents[*]` →
`Interpretation::Glob`) — NOT an inline `if key == …` special case. When full
YAML validation lands, schemas become the annotation source and the
hand-written table is deleted/generated; the consumption contract is
unchanged. Rationale for why the scoped hint is safe despite user-schema
shadowing concerns: q2 already owns `listing:` in document metadata
(`parse_listings` consumes it and emits `Q-12-*` on shape mismatch), and the
failure asymmetry favors the hint — a user who truly wanted markdown gets a
diagnosable no-match warning, whereas today's defect #6 is silent corruption.
This also removes the `PandocInlines` recovery chain in `parse_listings`.

## WASM note

`ListingGenerateTransform` runs in hub-client preview. The resolver must not
touch the filesystem: the candidate inventory is built from paths already known
to the stage/render context. Keep the helper synchronous and pure.

## Work items

### Phase 0 — investigation residue (small, before tests)
- [x] Confirm how pampa assigns the document's own FileId (assumed `FileId(0)`)
      and that `_quarto.yml`'s metadata layer retains `parse_file` provenance
      through `resolve_format_config` + merging. **Confirmed 2026-08-06:**
      front matter = `Substring` into the RawBlock's source (doc `FileId(0)`,
      `pampa/src/pandoc/meta.rs:355-361`); `_quarto.yml` parsed via
      `parse_file(<full path>)` → hash FileId, `config_path` retained
      (`project/mod.rs:621-699`); `_metadata.yml` same scheme
      (`directory_metadata_for_document`), chain re-derivable by walking
      root→doc dir. Merge preserves leaf `source_info` (theme diagnostics
      pointing into `_quarto.yml` post-merge are the existing proof;
      `compile_theme_css.rs:368-388`).
- [x] Check `docs/` site and existing test corpus. **Confirmed 2026-08-06:**
      docs/ has one listing host (`errors/index.qmd`, glob `*/Q-*.qmd`) which
      matches via the host-relative view — unaffected. All existing
      project-relative tests (dep-graph #15/#15b, transform
      `project_relative_glob_matches_files_in_subdir`) use hosts at the
      project root, where host-relative == project-relative — they survive
      unchanged (doc comments need updating).
- [x] Confirm `Q-13-4` origin. **Confirmed 2026-08-06:** emitted by
      `resolve_doc_relative_href` (`transforms/navigation_href.rs`) once per
      phantom-item link (thumbnail + title = the observed pair). Fixing bug 1
      removes them; no separate change needed.

### Phase 1 — tests first (TDD)
- [x] Fixture projects as **inline-written temp-dir fixtures** (the repo's
      established convention in `listing_pipeline.rs` — no committed fixture
      dirs needed): `tests/integration/listing_glob_resolution.rs`, registered
      in `main.rs`.
- [x] Failing integration tests (8, all verified failing for the right
      semantic reasons at 59500cf1): host-dir resolution, dual-view removal,
      `_metadata.yml` base dir, `_quarto.yml` base dir, `../` traversal,
      root-escape `Q-12-17`, negation, negation-only default. Bonus finding
      pinned by `projmeta` test: cross-directory items currently get
      non-relativized hrefs (`href="posts/a.qmd"` verbatim) — the fix must
      produce page-relative hrefs for items outside the host's directory.
- [x] Unit tests for the resolver (provenance → base dir, incl. fallback) and
      the single-view matcher (incl. `..` normalization; escaping the project
      root matches nothing AND emits the new diagnostic code) — in
      `glob_resolve.rs`.
- [x] Unit tests for negation partition semantics (`glob_resolve.rs` +
      dep-graph test #22b).
- [x] Dep-graph tests updated: #15/#19 rewritten for resolved patterns, #22b
      added for negation; the rest were root-host tests that survive
      unchanged.

### Phase 2 — resolver + data model
- [x] Provenance-resolver helper: `project/listing/glob_resolve.rs`
      (`resolve_content_globs`, `item_matches`; pure, WASM-safe).
      `MetadataMergeStage` registers `_quarto.yml`/`_metadata.yml` in BOTH
      document SourceContexts (hash FileIds; symmetric append preserves
      `IncludeExpansionStage`'s FileId-parity invariant — a full-suite run
      caught the one-context version).
- [x] `ListingContents::Glob { pattern, source }` — carries `SourceInfo`
      rather than a pre-resolved base dir (resolution happens at the two
      consumption points, which own the context).
- [x] `DocumentProfile` v7 → v8: `listing_content_globs:
      Vec<ListingContentGlob>` (resolved pattern + negated), populated by
      `DocumentProfileStage` (extract stays pure).

### Phase 3 — matcher swap
- [x] Shared single-view matcher in both consumers; dual-view logic and
      `relative_to_dir` deleted. Bonus fix surfaced by the tests:
      `host_relative_qmd` in `binding.rs` now emits `../`-style hrefs for
      items outside the host's directory (page-relative links were broken
      for cross-directory items).
- [x] Negation applied in both sites (render + dep graph). `Q-12-17`
      registered in the catalog + docs stub page; audit script clean.

### Phase 4 — verification
- [x] `cargo nextest run --workspace` — 10890 passed at 359ad0c2 (includes
      the 8 new integration tests, all green).
- [x] End-to-end: `./target/debug/q2 render <fixture>` on all six scratchpad
      fixtures at 359ad0c2; HTML inspected. Observed listing items
      (href/title extracted from the rendered pages):
      - `p1-basic` sub/index.html → `p1.html P1` only; **zero Q-13-4** (was:
        Home + About phantoms + 2 warnings). Q-1-20 still fires (Phase 5).
      - `p2-dirmeta` blog/deep/index.html → `p1.html Deep P1` (was empty).
      - `p3-projmeta` index.html → `sub/p1.html Sub P1`.
      - `p4-root-host` index.html → posts a+b unchanged (no regression).
      - `p5-parent-glob` sub/index.html → `../rootpost.html Root Post`
        (was empty; href correctly page-relative).
      - `p6-glob-corruption` → still empty, fixed by Phase 5.
- [x] Render `docs/` with q2 and diff against main (clean caches, both
      sides). The errors-reference listing (`docs/errors/index.qmd`, glob
      `*/Q-*.qmd`) is **more correct on the branch**: main's render dropped
      `crossref/Q-15-1`, `project/Q-5-6`, `project/Q-5-7` from the listing
      data; the branch lists all 147 on-disk `Q-*.qmd` pages exactly
      (verified by `comm` against the file tree; includes the new
      `Q-12-17`). No entries were lost. The legacy drop mechanism wasn't
      chased further — the code path that caused it is deleted.
- [ ] `cargo xtask verify` (full — quarto-core changes affect the WASM leg).

### Phase 5 — `listing.contents` interpretation (defects #5 + #6)
- [x] Failing tests first (pampa unit tests, verified failing with the
      emphasis-corrupted value visible in the assert output; +
      `glob_with_markdown_parseable_asterisks_survives` integration test).
- [x] Annotation-source table: `crates/pampa/src/pandoc/meta_annotations.rs`
      (`listing.contents` + `format.*.listing.contents` → `Glob`;
      exact-length matching, single-segment wildcard, arrays transparent,
      maps extend the path; explicit tags always win; module doc marks it
      delete-on-schema-arrival). `yaml_to_config_value` threads a key path
      internally; public signature unchanged.
- [x] `parse_listings` recovery chain: **kept as defensive fallback** rather
      than removed — PandocInlines/string-shaped values can still arrive from
      non-YAML sources (programmatic construction, runtime metadata), and the
      fallback is harmless; comment updated to reflect the new primary path.
- [x] Regression tests: zero `Q-1-20` asserted in the integration suite;
      E2E: `p1-basic`, `p4-root-host`, `p6-glob-corruption` all render with
      **zero warnings** at c7b475cb and p6's listing contains `Should Match`.
      Full workspace suite: 10911 passed.

### Phase 6 — bookkeeping
- [ ] `braid close bd-v7ixzsp5`; comment on GH #456 with the fix summary.
- [ ] docs/ website: document listing `contents` resolution semantics
      (user-facing, no internals).

## Decisions (Carlos, 2026-08-06)

1. **Behavior change ships silently.** q2 is 0.*; no stability promises. Fix
   it properly, no transition diagnostic.
2. **Escaping the project root warns** via a new diagnostic code registered in
   `quarto-error-catalog`, so warnings-as-errors mode fails loudly.
3. **Negation-only `contents` defaults the positive set to `*.qmd`.** Design
   the matcher as the seed of an internal glob-expansion API to be adopted by
   other q2 glob consumers over time (follow-up strand when the shape settles).
4. **Q-1-20 / interpretation is in scope (Phase 5)**, implemented as a
   schema-replaceable annotation-source table, not an inline key special-case.
   Elevated in priority by the discovery of defect #6 (silent glob corruption
   on successful emphasis parse) — doing nothing is not neutral. See
   "Interpretation of `listing.contents`" section for the full rationale
   against Carlos's user-schema-shadowing concern.
5. **Single strand.** bd-v7ixzsp5 stays a single strand; no epic conversion.
