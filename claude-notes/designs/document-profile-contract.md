# `DocumentProfile` contract

**Status:** Active (Phase 0 of the website epic, `bd-0tr6` / `bd-f3jc`).
**Version tag:** `DOCUMENT_PROFILE_VERSION = 1`
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
| `outline` | `Vec<pampa::toc::TocEntry>` built from the raw block sequence at `OUTLINE_MAX_DEPTH = 6`. **Always un-numbered**: `TocEntry::number == None` for every entry and every descendant. |

## Non-guarantees (explicit)

What a profile **does not** contain:

- **Engine output.** No values produced by executing code cells
  (Jupyter, Knitr, Observable). Those require the engine stage,
  which runs after the checkpoint.
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

## Change log

- **2026-04-23 — v1.** Initial version. Fields: `profile_version`,
  `source_path`, `output_href`, `format_id`, `title`, `subtitle`,
  `description`, `authors`, `date`, `categories`, `keywords`,
  `image`, `draft`, `outline`. (`bd-f3jc`)
