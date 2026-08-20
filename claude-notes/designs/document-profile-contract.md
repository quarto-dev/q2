# `DocumentProfile` contract

**Status:** Active (Phase 0 of the website epic, `bd-0tr6` / `bd-f3jc`;
extended in Phase 8 sub-phase 8.0, `bd-fegm` + `bd-r82e`).
**Version tag:** `DOCUMENT_PROFILE_VERSION = 7`
**Type:** `quarto_core::document_profile::DocumentProfile`
**Stage:** `quarto_core::stage::stages::DocumentProfileStage` (name
`"document-profile"`) + `UnwrapProfileStage` (`"unwrap-profile"`),
inserted between `MetadataMergeStage` and `PreEngineSugaringStage`.
**Plans:** `claude-notes/plans/2026-04-23-website-project-epic.md`
(parent), `claude-notes/plans/2026-04-23-websites-phase-0.md` (Phase 0).

## Summary

A `DocumentProfile` is a typed, serde-serializable, **static** snapshot
of a single document. It is produced at a fixed pipeline checkpoint —
after the full metadata merge, and before any AST mutation (sugar,
engine execution, user filters, transforms). Everything a
project-scoped feature needs to know about a document *without*
running engines or filters lives here.

Phase 0 introduces the type, the checkpoint, and the resumability
contract. Phase 1 uses it to build the project-wide `ProjectIndex`.
Phases 2–9 build on that index for sidebars, navbars, cross-document
links, incremental rebuilds, and hub-client project nav. Eventually
the same checkpoint substrate will back `freeze`.

## Pipeline position

```
Parse → Merge → [Profile checkpoint] → Sugar → Engine → ThemeCSS →
UserFilters(pre) → AstTransforms → UserFilters(post) → Highlight →
RenderBody → ApplyTemplate
```

The checkpoint is **deliberately pre-sugar**: outlines reflect the
author's heading hierarchy, not synthetic sections added by theorem /
float-target / callout sugaring. This keeps the contract stable
regardless of which sugar transforms are active.

## Guarantees

Per field, what a consumer can rely on. All guarantees presume the
document successfully reached the checkpoint; an earlier stage
failure aborts the pipeline with diagnostics and no profile is
produced.

| Field | Guarantee |
|---|---|
| `profile_version` | Always equals [`DOCUMENT_PROFILE_VERSION`](../../crates/quarto-core/src/document_profile.rs). A deserialized profile with a different version value is rejected with `DocumentProfileError::VersionMismatch`. |
| `source_path` | Project-relative, forward-slash separated. For a single-file project, this is just the file name — see §"Project root invariant" in the Phase-0 plan. Never absolute. |
| `output_href` | Project-output-relative, forward-slash separated. Usable directly as an HTML `href`. |
| `format_id` | Mirrors `ctx.format.target_format` at the checkpoint (e.g. `"html"`, `"acm-html"`). |
| `title` | Plain-text title from merged metadata. `None` only when the frontmatter genuinely has no title and no fallback was applied by a pre-checkpoint stage. |
| `subtitle`, `description`, `date`, `image` | Plain-text extraction of the corresponding merged-metadata key, or `None`. |
| `authors` | Flat list of plain-text names, extracted from either the `author` or `authors` key. Supports scalar, array-of-strings, array-of-maps with a `name` key, and a single map with a `name` key. Structured author metadata (affiliation, email, ORCID, …) is deliberately dropped — a dedicated author-model design is a separate epic. |
| `categories`, `keywords` | Arrays of plain-text strings. A single scalar value is lifted into a one-element list. |
| `draft` | Boolean. Defaults to `false` when the key is missing or non-boolean. |
| `order` | `Option<i32>` sort key from `order:` frontmatter. `None` when the key is absent or non-integer. Consumed by Phase-2's auto-sidebar sort (`claude-notes/plans/2026-04-24-websites-phase-2.md`). Added v1-additive (no version bump). |
| `outline` | `Vec<pampa::toc::TocEntry>` built from the raw block sequence at `OUTLINE_MAX_DEPTH = 6`. **Always un-numbered**: `TocEntry::number == None` for every entry and every descendant. Since v11, `TocEntry::title` is `Inlines`, not `String` — the outline carries the heading's inline markup verbatim. Consumers that want text project it themselves (`pampa::writers::plaintext::inlines_to_string`). |
| `includes` | `Vec<IncludeEntry { path, content_hash }>` recording every file whose contents were spliced into the parent AST via `{{< include child.qmd >}}`. Populated by `IncludeExpansionStage` via a side-channel on `DocumentAst.recorded_includes`, drained into the profile by `DocumentProfileStage`. Direct + transitive children appear; cycles are pre-truncated. **Phase-8 cache invalidation depends on this field** (`bd-r82e`). Default empty. |
| `nav_dependencies` | `Vec<PathBuf>` of project-relative `.qmd` paths the user explicitly declares as cross-doc dependencies via `meta.project.nav-dependencies`. The Phase-8 dependency graph adds an edge to each declared target. The escape hatch for Lua filters that walk siblings without using sidebar / link / prev-next channels. Default empty. |
| `always_render` | `bool` from `meta.project.always-render`. When `true`, Mode B (subset render) pulls this page into the render set if any of its dependents is among the user-named targets. Mode A re-renders every page anyway, so this flag has no Mode-A effect. Default `false`. |
| `body_link_targets` | `Vec<PathBuf>` of project-relative `.qmd` paths this page links to from its body content. Populated by `LinkResolutionStage` (Pass-1) using the same `resolve_doc_relative_target` helper Phase 6's `LinkRewriteTransform` calls in Pass-2 — equivalence test asserts the two produce the same set. The Phase-8 dependency graph turns each target into an edge. See `body-link-resolution-contract.md`. Default empty. |
| `resources` | `Vec<String>` of document-level `resources:` patterns from the merged frontmatter (`bd-o8pr`). Raw patterns; expansion happens at the post-render collector. The snapshot of what the author declared at frontmatter-freeze time — engines and Lua filters that run later contribute through a separate channel (`DocumentResourceReport`) and cannot retroactively shrink this list. Default empty. |
| `categories_raw` | `Option<ConfigValue>` carrying the originating tagged value of the top-level `categories:` key (`bd-n8a4`). Mirrors `categories` but preserves `!prefer` / `!concat` merge tags so listings consumers can feed it (alongside `listing_item.categories_raw`) into `quarto_config::MergedConfig` for tag-aware merging. Most consumers should keep reading the flattened `categories`; only listings reach for the raw form. Default `None`. |
| `listing_content_globs` | `Vec<String>` of unresolved glob strings from the host page's `listing.*.contents:` declarations (`bd-xbnf`, listings L6). Flattened across all listings on the page. The dependency-graph builder expands these against `ProjectIndex` at graph-build time (host-relative first, project-relative fallback — matches L3's render-time rule) to add forward edges from each listing host to its content files; hosts with non-empty entries are also added to the graph's `force_render` set so Mode B (`quarto render posts/foo.qmd`) pulls in listing hosts when any of their content files is targeted. Resolution is **not** cached on the profile (the per-doc cache cannot represent dependency on the full project source set safely). Default empty. |
| `listing_item` | `ListingItemInfo` advertising per-document data for listings consumers (`bd-n8a4`). **Scoped feature surface — listings only**; non-listing consumers must use the corresponding top-level fields (`title`, `description`, `image`, …). Author-supplied values populate during `DocumentProfile::extract`; `ListingItemInfoStage` (`bd-izqh`, L1, landed) auto-fills holes pre-checkpoint for `description` (full first paragraph), `image` (first inline image's URL), `word_count` (Q1-parity tokenization, footnote text excluded), `reading_time_minutes` (`ceil(word_count / 200)`), and `date_modified` (filesystem mtime via `SystemRuntime::path_metadata` formatted as `YYYY-MM-DD` UTC). Author values always win — the stage strictly fills holes. The nested `extra: BTreeMap<String, ConfigValue>` is the **only** open-shape field in the profile and is forbidden to non-listing consumers — see §"Scoped feature surfaces". Default empty (`ListingItemInfo::is_empty()`). |
| `engine_resolution` | `Option<ProfileEngineResolution>` (`engine-resolution.md` §9.1). `Some` only when the document's engine resolution is provably load-free at Pass-1 — the needs-no-load predicate in `engine-resolution.md` §3.3 (P1–P4) — and is then **complete**: `sequence` is the resolved engine names in run order, `ownership` is the language→engine map in insertion order. `None` means resolution fell through to Pass-2's existing (non-profiled) resolution — **not an error**; most documents may show `None` until every engine a project uses is static or tabled (`engine-resolution.md` §3.3, §12). Names only, no `ConfigValue` blobs. Default `None`. |

## Non-guarantees (explicit)

What a profile **does not** contain:

- **Engine execution output.** No values produced by executing code
  cells (Jupyter, Knitr, Observable). Those require the engine
  stage, which runs after the checkpoint. **Engine *resolution* is
  the exception to this line, not a contradiction of it:** deciding
  which engine(s) will run and which owns which language is a pure,
  pre-load computation (`engine-resolution.md` §9) and *is*
  profile-eligible — see the `engine_resolution` field above. The
  boundary is between *deciding* an owner (resolution, may be on the
  profile) and *running* that owner to get a value (execution, never
  on the profile).
- **Sugar-synthesized structure.** No callout custom nodes, no
  theorem/float-target/equation-label canonicalization, no
  crossref numbering (`TocEntry::number`), no appendix structure,
  no sectionize-added div classes. Any consumer that needs these
  must read them from the post-render AST.
- **User-filter mutations.** Nothing that `pre` or `post` user
  filters would introduce (Lua, JSON, citeproc).
- **Theme CSS, code highlighting, rendered HTML body, applied
  template.** All of those are downstream of the checkpoint.
- **Resolved shortcodes.** `{{< meta … >}}` and friends are resolved
  during `AstTransformsStage`, after the checkpoint.
- **Cross-document information.** A `DocumentProfile` describes a
  single file. Merged across siblings by Phase 1's `ProjectIndex`
  (future work).
- **Absolute filesystem paths.** Everything path-shaped is
  project-relative by construction.

## Scoped feature surfaces

Most profile fields are typed, narrowly defined, and globally
readable: any consumer that needs `title`, `categories`,
`outline`, etc. reaches for the top-level field directly. The
contract is closed-shape, versioned, and stable.

The `listing_item` field is an **explicit exception**, scoped to
one feature (listings) by name and by convention.

**Allowed:** the listings code path (planned
`L3 ListingResolveTransform`, `L5 CategoriesSidebarTransform`,
`L7 post-render upgrade`, `L9 RSS feeds`) reads
`profile.listing_item` to materialize listing items.

**Forbidden:** any code outside the listings module reaches into
`profile.listing_item` (and especially into
`profile.listing_item.extra`). Sidebar generation, navbar
rendering, cross-doc link rewriting, freeze, and other features
must continue to use the typed top-level fields. If a future
feature finds itself wanting to read `listing_item`, that is a
**redesign trigger** — either widen the typed top-level field set
with a versioned bump, or define a new scoped feature surface. Do
not silently broaden listings' scope.

The discipline is enforced by code review, not the type system.
The `listing_item` field is `pub` for serde and for listings' own
use; the contract above is the boundary that matters.

This is the same discipline `bd-fegm` (Phase 8) used when it
declined to add a generic `extras: HashMap` field for filter-
introduced data and chose typed fields instead. The exception
here is granted because (a) custom listing templates genuinely
need access to author-declared free-form metadata, and (b) the
"named, scoped" framing keeps the cost of the exception locally
bounded.

The companion field `categories_raw: Option<ConfigValue>` and its
sibling `listing_item.categories_raw` are likewise listings-only
surfaces: their purpose is to preserve `!prefer` / `!concat`
merge tags so listings consumers can apply tag-aware merging via
`quarto_config::MergedConfig`. Non-listing consumers continue to
read the flattened `categories: Vec<String>`. See
`claude-notes/plans/2026-05-05-listings-L0-profile-extension.md`
§"D7" for the design rationale.

## Mutability

**Profiles are read-only.** A Phase 1+ user filter that wants to
observe cross-document state reads `&[DocumentProfile]` through
`ProjectIndex`, but has no API for mutating individual profiles. Any
mutation would undermine caching and cross-document invariants.

If a profile "should" reflect some piece of state that can only be
computed after the checkpoint today, the fix is to move the producing
logic earlier in the pipeline — not to back-patch the profile. One
tracked case is the `ref_type_registry`: Phase 0 populates it in
`PreEngineSugaringStage` (i.e. after the checkpoint), but eventually
it should move before the checkpoint so cross-document validation of
custom crossref types is possible without full renders. When that
happens, the registry (or its static subset) may become a profile
field, guarded by a `profile_version` bump.

## Serialization and versioning

`DocumentProfile` derives `Serialize + Deserialize` and uses
`serde_json` via `to_json` / `from_json`. The latter checks
`profile_version` before returning the value.

Bump `DOCUMENT_PROFILE_VERSION` whenever the serialized shape changes
in a way that a v1 consumer would misread:

- adding a new **required** field,
- removing or renaming a field,
- changing the semantics or units of an existing field.

Additive, backward-compatible changes (new `Option<_>` field with a
`None` default, new `Vec<_>` field with an empty default) **do not**
require a bump — but the contract doc must be updated.

Consumers reading a profile off disk (Phase 8 incremental rebuild,
future freeze) must handle `DocumentProfileError::VersionMismatch`
gracefully by invalidating the cached profile and re-running the
head pipeline.

## Checkpoint resumability

The pipeline data at the checkpoint is `PipelineData::AtProfile`,
wrapping `DocumentAtProfile { profile, ast }`. `DocumentAtProfile`
derives `Clone`, so a Phase 1+ orchestrator can:

1. Run the head pipeline once per file, collecting every
   `DocumentAtProfile`.
2. Build the `ProjectIndex`.
3. For each file, clone the bundle and run the tail pipeline to
   completion, passing the shared index via `StageContext`.

The load-bearing integration test
`pipeline_at_profile_to_end_produces_expected_html` in
`crates/quarto-core/tests/document_profile_pipeline.rs` asserts
byte-identical HTML between an end-to-end render and a render that
pauses, clones, and resumes at the checkpoint.

For Phase 0, an `UnwrapProfileStage` runs immediately after the
checkpoint and discards the profile, re-emitting the inner
`DocumentAst` so downstream stages keep their existing input kind.
Phase 1 replaces that with a real consumer.

## Writing a consumer (for future-phase authors)

1. Obtain a `&DocumentProfile` (or `&[DocumentProfile]` from the
   future `ProjectIndex`) — never clone profiles just to mutate them.
2. Treat `None` / empty-vec fields as "the author did not specify
   this"; do not synthesize defaults in the consumer unless your
   feature's user-facing semantics require it.
3. If you need post-merge state that is not in the profile today,
   either read it from the post-transform AST at your own stage
   position, or file a bd issue to move its producer pre-checkpoint
   and add it as a profile field (with a version bump).
4. Never add a branch on `ProjectContext::is_single_file` — see
   §"Project root invariant" in the Phase-0 plan.

## Failure surface and the strict-vs-lenient consumer contract

### Engine policy

A document that fails to reach the checkpoint (parse error,
metadata error, missing required `_metadata.yml`, …) is dropped
from the project's `ProjectIndex` and surfaced as a
`FileFailure` on the orchestrator's `ProjectRenderSummary`:

```rust
pub struct FileFailure {
    pub input: PathBuf,
    pub error: String,                 // pre-rendered ariadne text for parse errors
    pub diagnostics: Vec<DiagnosticMessage>,        // structured form
    pub source_context: Option<SourceContext>,      // for offset → line/column mapping
}

pub struct ProjectRenderSummary<O> {
    pub outputs: Vec<O>,
    pub pass1_failures: Vec<FileFailure>,           // profile-pass dropouts
    pub pass2_failures: Vec<FileFailure>,           // renderer errors
    pub project_diagnostics: Vec<DiagnosticMessage>,
}
```

The orchestrator is **policy-free**: it does not decide whether
a `pass1_failures` entry should abort the run, change the exit
code, or be displayed inline. Pass-2 simply runs over whatever
files succeeded Pass-1; any references to the dropped file from
sibling navigation get the project-scoped warning
`"<tag> references missing document information for '<path>'"`
(emitted in `quarto_core::transforms::navigation_href`).

### Consumer contract

Two consumers exist today:

| Consumer                                    | Policy   | What it does                                                                                                                                                                                                  |
|---------------------------------------------|----------|---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| `quarto render` (CLI / CI)                  | Strict   | Any non-empty `pass1_failures` *or* `pass2_failures` causes a non-zero exit. Headless renders must not silently drop pages.                                                                                  |
| `quarto preview` / hub-client (interactive) | Lenient  | Surfaces failures in the preview overlay with source-file attribution; keeps rendering everything that did succeed. Partial progress lets a user fix one error at a time without losing live preview elsewhere. |

The `RenderResponse` wire format the WASM hub-client returns
mirrors the orchestrator surface:

```ts
interface RenderResponse {
  success: boolean;             // active page render
  error?: string;               // active-page failure message
  html?: string;
  diagnostics?: Diagnostic[];   // active-page errors
  warnings?: Diagnostic[];      // page-local + project-scoped warnings
  pass1_failures?: Pass1Failure[];  // sibling-page Pass-1 failures
}

interface Pass1Failure {
  source_file: string;
  error: string;                // ariadne snippet for parse errors
  diagnostics: Diagnostic[];    // structured form for Monaco markers
}
```

`Diagnostic` carries an optional `source_file` attribution for
project-scoped warnings whose origin isn't pinned by their
location (e.g. nav warnings).

### Adding a new consumer

A planned third consumer is a `quarto preview` binary that wraps
hub-client infrastructure for local previews. New consumers must:

1. **Not modify the engine.** Pass-1 failures, project
   diagnostics, and per-page diagnostics all flow through
   `ProjectRenderSummary` (native) or `RenderResponse` (WASM)
   unchanged.
2. **Choose a policy explicitly.** Strict is appropriate for
   one-shot / CI-style consumers; lenient is appropriate for
   live / iterative consumers.
3. **Preserve attribution.** When showing a Pass-1 failure to
   the user, name the failing file. The hub-client overlay's
   "Sibling page 'X' failed to parse" pattern is the reference
   implementation.

Tracking: `bd-creo` (CLI strictness), `bd-mwtf` /
`bd-rqba` (hub-client leniency + attribution),
`bd-0tr6` (websites epic). Plan:
`claude-notes/plans/2026-05-01-hub-client-website-render-ux.md`.

## Change log

- **2026-04-23 — v1.** Initial version. Fields: `profile_version`,
  `source_path`, `output_href`, `format_id`, `title`, `subtitle`,
  `description`, `authors`, `date`, `categories`, `keywords`,
  `image`, `draft`, `outline`. (`bd-f3jc`)
- **2026-04-24 — v1-additive.** Added `order: Option<i32>` field
  from `order:` frontmatter, consumed by Phase-2 auto-sidebar sort.
  `#[serde(default)]` so v1-serialized profiles still deserialize.
  No `profile_version` bump. (`bd-9svl`)
- **2026-04-27 — v2 (`bd-fegm` / `bd-r82e`).** `DOCUMENT_PROFILE_VERSION`
  bumped 1 → 2. Four new fields, all collection-shaped with
  `#[serde(default, skip_serializing_if = ...)]` so default profiles
  serialize without bloat:
  - `includes: Vec<IncludeEntry>` — closes `bd-r82e`. Required for
    Phase-8 cache invalidation when transitive `{{< include … >}}`
    children change.
  - `nav_dependencies: Vec<PathBuf>` — user-declared cross-doc
    dependency channel for filter-introduced edges.
  - `always_render: bool` — per-page Mode-B opt-in for non-
    deterministic / non-modelable filters.
  - `body_link_targets: Vec<PathBuf>` — populated by the new
    `LinkResolutionStage` for Phase-8 dependency-graph edges.
  v1 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently
  regenerated.
  See companion contracts:
  `claude-notes/designs/body-link-resolution-contract.md`,
  `claude-notes/designs/sidebar-auto-expansion-contract.md`.
- **2026-04-29 — v3 (`bd-o8pr`).** `DOCUMENT_PROFILE_VERSION`
  bumped 2 → 3. One new field:
  - `resources: Vec<String>` — document-level `resources:`
    patterns from frontmatter, snapshot at frontmatter-freeze
    time. The post-render collector expands the patterns and
    augments them with engine/filter contributions through a
    separate channel (`DocumentResourceReport`); the profile
    field is read-only and immutable downstream of the
    checkpoint. Default empty.
  v2 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently
  regenerated.
- **2026-05-05 — v4 (`bd-n8a4`, listings epic L0).**
  `DOCUMENT_PROFILE_VERSION` bumped 3 → 4. Two new fields, both
  additive at the on-disk layer (`skip_serializing_if` keeps
  default profiles compact):
  - `listing_item: ListingItemInfo` — scoped per-feature
    surface for listings consumers. Curated typed sub-fields
    plus `extra: BTreeMap<String, ConfigValue>` for custom
    listing-template fields. Default empty
    (`ListingItemInfo::is_empty()`). Outer profile shape
    stable; additions or removals of keys inside `extra` do
    **not** require a future bump. Non-listing consumers are
    forbidden from reading this field — see new §"Scoped
    feature surfaces".
  - `categories_raw: Option<ConfigValue>` — tagged form of the
    top-level `categories:` value, preserving `!prefer` /
    `!concat` merge tags for listings consumers' tag-aware
    merging via `quarto_config::MergedConfig`. Most consumers
    keep reading the flattened `categories: Vec<String>`;
    only listings reach for the raw form. Default `None`.
  v3 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently
  regenerated, identical to the v2 → v3 cascade.
  Plan: `claude-notes/plans/2026-05-05-listings-L0-profile-extension.md`.
  Parent epic: `bd-61cd`
  (`claude-notes/plans/2026-05-05-listings-epic.md`).
- **2026-05-06 — `ListingItemInfoStage` lands (`bd-izqh`, listings
  epic L1).** No version bump; the field shape is unchanged. New
  pre-checkpoint stage between `IncludeExpansionStage` and
  `DocumentProfileStage` auto-fills `meta.listing-item.{description,
  image, word-count, reading-time-minutes, date-modified}` from the
  post-include AST (and filesystem mtime via
  `SystemRuntime::path_metadata`) when the author hasn't supplied
  them. `DocumentProfileStage` then extracts the enriched
  `ListingItemInfo` via the same path it used for purely
  author-supplied values in v4. `categories` is **not** auto-filled
  by L1 (D8 — listings consumers do their own L0-`categories_raw`-aware
  merge). The hub-client/WASM pipeline runs the same stage, but
  `date_modified` stays `None` until `bd-a3we` teaches the Automerge
  VFS to surface change-history time. Plan:
  `claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`.
- **2026-05-07 — v5 (`bd-xbnf`, listings epic L6).**
  `DOCUMENT_PROFILE_VERSION` bumped 4 → 5. One new field, additive
  at the on-disk layer (`skip_serializing_if` keeps default
  profiles compact):
  - `listing_content_globs: Vec<String>` — flattened glob strings
    from the host page's `listing.*.contents:` declarations.
    *Unresolved* globs only; resolution happens at graph-build
    time inside
    `crate::project::dependency_graph::ProjectDependencyGraph::build`,
    which expands each glob against the full project source set
    (host-relative first, project-relative fallback — same rule
    `ListingGenerateTransform` uses at render time) to add forward
    edges from each listing host to its content files. Hosts with
    non-empty entries are also added to the graph's
    `force_render` set so Mode B (`quarto render posts/foo.qmd`)
    automatically pulls in listing hosts when any of their
    content files is in the user-named target set. Resolution is
    **not** cached on the profile because it depends on the full
    project source set, which a per-doc profile can't represent
    safely (a new sibling `.qmd` would not invalidate the host's
    profile cache, leaving the resolution stale). Default empty.
  v4 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently
  regenerated, identical to every prior bump.
  Plan: `claude-notes/plans/2026-05-07-listings-L6-dep-graph.md`.
  Parent epic: `bd-61cd`
  (`claude-notes/plans/2026-05-05-listings-epic.md`).
- **2026-06-?? — v6 (`bd-c1et2`).** (Entry added retroactively —
  the bump was documented only in the code doc-comment at the
  time.) `DOCUMENT_PROFILE_VERSION` bumped 5 → 6. `resources`
  changed from `Vec<String>` to `Vec<RawResourcePattern>` so each
  pattern carries its YAML `SourceInfo` for Ariadne-span
  diagnostics.
- **2026-07 — v8 (merge of two concurrent v7 bumps).** Two branches
  each bumped `DOCUMENT_PROFILE_VERSION` 6 → 7 for a different new
  field; the ts-engine-extensions rebase merged them, so both fields
  coexist under **v8** (there is no single-field v7 on the merged
  line). v6/v7 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently regenerated,
  identical to every prior bump. Both new fields:
  - `authors_structured: Vec<ProfileAuthor>` (`bd-ez0hiowa`,
    title-block parity epic P2) — the structured author model (name
    literal + given/family components, ORCID, email, url, degrees,
    attribute flags, denormalized affiliations as
    `ProfileAffiliation { name, department, url }`). Produced by the
    shared normalization in `crates/quarto-core/src/metadata/authors.rs`
    (`parse_authors_model`) — the same pass `AuthorsNormalizeTransform`
    uses to derive the `by-author`/`by-affiliation` metadata the HTML
    title block renders. The flat `authors: Vec<String>` field keeps
    its type and now derives its literals from the same model, so the
    two fields always agree. Fields the profile does not carry yet
    (roles, notes, funding) join later with another bump.
  v6 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently
  regenerated, identical to every prior bump.
  Plan: `claude-notes/plans/2026-07-15-html-title-block-parity.md`.
- **v8 (`bd-v7ixzsp5`, GH #456)** and **v9 (`bd-mt7a6uc4`).**
  (Entries added retroactively in 2026-08 — like v6, these bumps
  were documented only in the `DOCUMENT_PROFILE_VERSION`
  doc-comment at the time. That comment remains the fuller
  account.) v8 changed `listing_content_globs` from
  `Vec<String>` to `Vec<GlobPattern>`, resolving patterns to
  project-relative form at extraction time against the directory
  of the file each was written in, and carrying a `negated` flag.
  v9 added `resource_globs: Vec<GlobPattern>` plus the
  index-aligned `resource_glob_sources` and `rejected_resources`,
  applying the same host-directory resolution to `resources:`.
- **2026-08-12 — v10 (`bd-aliases-redirects-missing-sch7cd1g`).**
  `DOCUMENT_PROFILE_VERSION` bumped 9 → 10. Two new fields, both
  `#[serde(default, skip_serializing_if = "Vec::is_empty")]`:
  - `aliases: Vec<String>` — the document's `aliases:`
    front-matter entries (old URLs that should redirect to this
    page), kept **raw**.
  - `alias_sources: Vec<SourceInfo>` — index-aligned provenance,
    one entry per alias.

  Note the deliberate contrast with v9's `resource_globs`:
  `resources:` patterns are *resolved* at extraction time because
  resolution depends on the declaring file's directory, which only
  the stage knows. An alias instead resolves against the page's own
  `output_href` — already on the profile — so extraction stays pure
  and resolution moves to the consumer. Validation has no choice in
  the matter: whether an alias collides is a property of the whole
  project, so it can only be decided once every profile is in hand.

  Both therefore happen in `project::website_post_render`, which is
  also the only place a diagnostic survives profile caching. This is
  the same lesson `rejected_resources` records: a diagnostic emitted
  at extraction time appears on the render that populated the cache
  and never again.

  v9 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently regenerated,
  identical to every prior bump.
  Plan: `claude-notes/plans/2026-08-12-aliases-redirect-stubs.md`.

- **2026-08-13 — v11 (`bd-toc-smart-quotes-6nro57ed`).** Changes
  `outline`'s entry titles from `String` to `Inlines`
  (`pampa::toc::TocEntry::title`).

  The flattened title was lossy in a way that produced a visible
  defect: a heading `## Using a "raw" volume` rendered with curly
  quotes but its TOC entry rendered `Using a raw volume`, because the
  flattener recursed into `Inline::Quoted` without emitting the
  delimiters. Inline code, emphasis, and math spans were dropped the
  same way — Quarto 1 renders all of them inside TOC entries.

  The fix is to stop flattening at all. `TocEntry::title` now carries
  the heading's inlines, and each consumer decides what to do with
  them: the HTML TOC renders them through the inline writer (stripping
  links and notes, which an `<a>` cannot nest); consumers wanting
  plain text call `pampa::writers::plaintext::inlines_to_string`.

  This is the profile-side consequence of that decision, and it is
  deliberate: the outline is meant to be a *faithful semantic
  outline*, so encoding one renderer's structural constraint (or one
  consumer's plain-text preference) into the stored shape would be
  exactly the kind of lossy back-patching the "profiles are read-only"
  rule exists to prevent.

  **Serialized shape changes**: a title that was `"Top"` is now an
  array of inline nodes. v10 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently regenerated,
  identical to every prior bump — `DOCUMENT_PROFILE_VERSION` is in the
  cache-key hash domain, so stale entries are never even looked up.
  Plan: `claude-notes/plans/2026-08-13-toc-smart-quotes.md`.

- **2026-08-16 — v12 (plan6, Pass-1 engine resolution).** Adds
  `engine_resolution: Option<ProfileEngineResolution>` — the
  per-document engine resolution, additive at the on-disk layer
  (`skip_serializing_if` keeps default profiles compact), stamped only
  when Pass-1 can resolve it without loading an engine
  (`engine-resolution.md`'s needs-no-load predicate, §3.3/§7/§9.1).
  `None` means the doc falls through to Pass-2's existing resolution —
  not an error. Names only, no `ConfigValue` blobs: `sequence` is the
  resolved engine names in run order, `ownership` is the
  language→engine map in insertion order. Feeds the LSP today; future
  freeze and kernel-pooling consumers later (`engine-resolution.md`
  §12).

  v11 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently regenerated,
  identical to every prior bump.
  Plan: `claude-notes/plans/2026-06-29-plan6-pass1-engine-resolution.md`.
