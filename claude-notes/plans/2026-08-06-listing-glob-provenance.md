# Listing `contents:` globs — provenance-based base-directory resolution

**GitHub issue:** https://github.com/quarto-dev/q2/issues/456
**Braid strand:** bd-v7ixzsp5 (bug, P1)
**Status:** plan draft — awaiting review/iteration with Carlos before execution.

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
- [ ] Commit the five fixture projects under the integration-test convention
      (`tests/integration/` layout; follow `.claude/rules/integration-tests.md`).
- [ ] Failing integration tests, one per fixture, asserting: exact listing item
      set in rendered HTML, zero `Q-13-4` warnings, resolved `.html` hrefs
      (no `.qmd` leakage).
- [ ] Unit tests for the resolver (provenance → base dir, incl. fallback) and
      the single-view matcher (incl. `..` normalization; escaping the project
      root matches nothing AND emits the new diagnostic code).
- [ ] Unit tests for negation partition semantics.
- [ ] Dep-graph tests updated: edges under new semantics (existing tests #14–#22
      in `dependency_graph.rs` will need review — some encode the dual-view
      behavior being removed).

### Phase 2 — resolver + data model
- [ ] Provenance-resolver helper (shared, pure) in `project/listing/` or
      `project/mod.rs`.
- [ ] `ListingContents::Glob { pattern, base_dir }`; `parse_listings` threads
      the per-entry `ConfigValue` source info into it.
- [ ] `DocumentProfile` field change + `profile_version` bump.

### Phase 3 — matcher swap
- [ ] Shared single-view matcher; `ListingGenerateTransform` and
      `ProjectDependencyGraph::build` both use it; delete the dual-view logic
      and the now-unused `relative_to_dir` host-view path if nothing else uses it.
- [ ] Negation applied in both sites.

### Phase 4 — verification
- [ ] `cargo nextest run --workspace`.
- [ ] End-to-end: `cargo run --bin q2 -- render` on each fixture; inspect HTML;
      record invocation + output snippets here per the E2E policy.
- [ ] Render `docs/` with q2 and diff listing pages against main.
- [ ] `cargo xtask verify` (full — quarto-core changes affect the WASM leg).

### Phase 5 — `listing.contents` interpretation (defects #5 + #6)
- [ ] Failing test first: `p6-glob-corruption` fixture — `p*osts*.qmd` must
      match `pXosts_extra.qmd` (glob survives with asterisks intact).
- [ ] Annotation-source table in the front-matter interpretation path
      (`listing.contents[*]` → `Interpretation::Glob`), designed for later
      replacement by schema-derived annotations.
- [ ] Remove the `PandocInlines` recovery chain in `parse_listings`.
- [ ] Regression test: clean renders of `p1-basic`/`p4-root-host` emit zero
      `Q-1-20` warnings.

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
