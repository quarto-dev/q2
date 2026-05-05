# L0 — `ListingItemInfo` profile extension (sub-plan)

**Date:** 2026-05-05
**Beads:** `bd-n8a4`. Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Design rationale:**
`claude-notes/plans/2026-05-05-listings-design-discussion.md`
(see §"C5 — Named listing-item info object on the profile" and
§"Why isn't full metadata already on `DocumentProfile`?").
**Status:** Draft. Awaiting implementation.

## Goal of this phase

Lay the data substrate for every later listings phase. Specifically:

1. Add a single, named, scoped field to `DocumentProfile`:
   `listing_item: ListingItemInfo`. This is the **only** place
   listings consumers will read per-document data; non-listing
   consumers must continue using top-level profile fields.
2. Introduce the `ListingItemInfo` type with curated typed sub-fields
   plus an `extra: BTreeMap<String, ConfigValue>` bag for custom-
   listing-template fields. Outer profile shape is closed; the bag
   is a *named, scoped* hole.
3. Read the field at extraction time from `meta.listing-item`
   (frontmatter authors can pre-populate any sub-field).
4. Bump `DOCUMENT_PROFILE_VERSION` from `3` → `4`. **Note:** the
   epic plan says "2 → 3"; the version is already `3` because of
   `bd-o8pr` (`resources` field). This sub-plan corrects the bump
   target. No change to the epic decision; just the integer.
5. Update the contract doc with a new field row **and** a new
   §"Scoped feature surfaces" paragraph that explicitly forbids
   non-listing consumers from reaching into
   `listing_item.extra`. This is the user-signed-off written-down
   discipline that prevents the open-shape bag from spreading.

**No user-visible behavior change.** L0 is a pure type extension +
extraction wiring. No stage logic changes; no listing rendering.
The auto-fill machinery (description, image, word-count, reading-
time, date-modified) is L1's job. L0 just makes the field exist.

## Reference material

Read before writing code:

- Parent epic plan:
  `claude-notes/plans/2026-05-05-listings-epic.md` §"L0".
- Design rationale:
  `claude-notes/plans/2026-05-05-listings-design-discussion.md`
  §C5 + §"Why isn't full metadata already on `DocumentProfile`?".
- Current profile contract:
  `claude-notes/designs/document-profile-contract.md` (especially
  §"Mutability" and §"Serialization and versioning").
- Profile source of truth:
  `crates/quarto-core/src/document_profile.rs` (851 lines; see in
  particular the `Default for DocumentProfile`, `extract`,
  `to_json` / `from_json`, and the `extract_authors` /
  `extract_string_list` / `plain_text_field` helpers — `extract`
  for `listing_item` will reuse the same idioms).
- Recent precedent for an additive Phase-8-style field bump
  (v2 → v3 for `resources`): `bd-o8pr` commit and the existing
  `resources: Vec<String>` field in the profile struct (lines
  214–225 of `document_profile.rs`).
- `ConfigValue` source: `quarto-pandoc-types` crate.
- Contract change-log convention: see the bottom of
  `document-profile-contract.md` for the format used in earlier
  bumps.

## Resolved clarifications (2026-05-05)

User-confirmed answers to questions raised during sub-plan review;
record here so reviewers don't relitigate.

- **C1 — `reading-time` semantics: integer-minutes only.** Field is
  `reading_time_minutes: Option<u32>`. Frontmatter key is
  `reading-time-minutes` (kebab → snake). No display-string field;
  rendering decides display formatting at the listing-render stage
  (L3+). User reasoning: each document advertises a *semantic*
  value; different listings on different host pages may format the
  same value differently, so display formatting cannot live on the
  per-document profile. The doc-comment YAML examples in this
  sub-plan are corrected accordingly.
- **C2 — Naming convention: kebab in YAML, snake in Rust.**
  Quarto's general policy. Extraction does *manual* kebab-keyed
  lookup inside `extract_listing_item` (matching `extract_authors`
  and the rest of `document_profile.rs`); no `serde(rename_all =
  "kebab-case")` on the struct. On-disk profile JSON uses the
  Rust field name (`listing_item`, `reading_time_minutes`) —
  matches existing fields like `date_modified`.
- **C3 — Schema location: test-fixture only for L0.** Touch
  `crates/pampa/test-fixtures/schemas/definitions.yml` and file
  a follow-up bd issue for `quarto-yaml-validation` integration
  if/when project-wide schema validation gets a runtime gate. Q2
  has no production frontmatter-schema gate today; widening L0 to
  cover that is out of scope.
- **C4 — `categories` merge rule: delegate to
  `MergedConfig`.** Merge tags ride with the `ConfigValue`s
  themselves, so we don't need a special override-layer model.
  The listings consumer (L1+) computes effective categories by:

  ```rust
  // pseudo-code, lives in listings consumer code, not L0
  let merged = MergedConfig::new(vec![
      &profile.categories_as_config_value,        // lower priority
      &profile.listing_item.categories_as_cv,     // higher priority
  ]);
  let effective = merged.get_array(&[]);  // or equivalent extraction
  ```

  Behavior follows the existing tag rules: default for arrays is
  `Concat`, so by default the two layers concatenate; an author
  who writes `listing-item: { categories: !prefer [a, b] }`
  gets override semantics for free. This is exactly the design
  point of the tag-based merge system.

  **L0 implications:**
  - L0 stores `listing_item.categories` as written — `Vec<String>`
    is fine for the curated field's surface, *but* L0 must
    preserve the originating `ConfigValue` somewhere accessible
    to listings consumers, otherwise the tag information is lost
    at the profile boundary. Two options:
    1. Keep `listing_item.categories: Vec<String>` plus a
       parallel `listing_item.categories_raw: Option<ConfigValue>`
       (or similar) carrying the tagged value.
    2. Type the field as `Option<ConfigValue>` directly and
       extract the string list at consumer side.
    Sub-decision **D7** below picks option (1) for ergonomics —
    most consumers want `Vec<String>`; only the categories merge
    needs the tagged form. Confirm with user if that's wrong.
  - The same question applies to top-level `profile.categories`:
    today it's `Vec<String>`. For the merge to be tag-aware, the
    `DocumentProfile::extract` path needs to *also* expose the
    pre-flattened `ConfigValue` for `categories`. This is a
    small additive change to `DocumentProfile` (new field
    `categories_raw` or similar). Recorded as part of L0 scope;
    if it turns out to require touching too many call sites,
    pull back and resolve at L1 before L1's first listing
    consumer needs the tag-aware merge.
  - **Documentation:** the contract doc's new `listing_item` row
    notes that `categories` is merged with top-level
    `profile.categories` via `MergedConfig` at consumer time.

  D3 below is updated to record this approach. No follow-up bd
  issue needed; the merge machinery already does the work.
- **C5 — Type-mismatch diagnostics: silent drop in L0; revisit at
  L2.** Strict validation belongs in the schema layer. L0 follows
  the existing graceful-degradation pattern.
- **C6 — `extra` namespace is distinct from curated fields.**
  `listing-item: { title: "A", extra: { title: "B" } }` produces
  `listing_item.title = Some("A")` and
  `listing_item.extra["title"] = ConfigValue::String("B")`. Two
  namespaces, no collision. Test 9 is extended to cover this.

## Type design

### `ListingItemInfo` shape

Add **new module** `crates/quarto-core/src/listing_item_info.rs`
re-exported from `quarto_core::document_profile` (or kept directly
inside `document_profile.rs` if the user prefers; sub-decision —
default below):

**Default placement decision:** keep `ListingItemInfo` *inside*
`document_profile.rs`. Rationale: it's a sub-shape of
`DocumentProfile` and lives at the same architectural layer; the
file is already organized around profile shapes (`IncludeEntry`
for example). A standalone module would imply an independent
abstraction that doesn't yet exist. If the type grows enough to
warrant its own file at a later phase, the move is mechanical.

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;

use quarto_pandoc_types::ConfigValue;
use serde::{Deserialize, Serialize};

/// Per-document information advertised for listings that include
/// this document.
///
/// **Scoped feature surface — listings only.** No other feature in
/// Quarto reads from this field. Non-listing consumers must use the
/// top-level [`DocumentProfile`] fields (`title`, `description`,
/// `image`, etc.). See the contract doc §"Scoped feature surfaces".
///
/// # Authoring surface
///
/// Authors populate this struct via a top-level `listing-item:` key
/// in YAML frontmatter:
///
/// ```yaml
/// ---
/// title: My post
/// listing-item:
///   reading-time-minutes: 15      # author override; auto-fill skipped
///   extra:
///     status: "draft"             # custom field for a custom template
///     sponsors: [Foo, Bar]
/// ---
/// ```
///
/// Frontmatter keys are kebab-case (Quarto YAML convention);
/// the corresponding Rust fields are snake_case. Extraction maps
/// between the two with explicit lookups (e.g.
/// `meta.get("reading-time-minutes")` → `reading_time_minutes`).
///
/// # Generate / render decomposition
///
/// L0 (this phase): the field exists; `DocumentProfile::extract`
/// reads it from frontmatter. Author-supplied values land here
/// directly.
///
/// L1 (next phase, `bd-izqh`): a dedicated `ListingItemInfoStage`
/// fills holes — `description`, `image`, `word_count`,
/// `reading_time_minutes`, `date_modified` — with values
/// derived from the document AST when the author has not
/// supplied them. Author values always win; the stage only
/// fills holes.
///
/// All fields are optional / collection-defaulted; an empty
/// `ListingItemInfo` is the legitimate default for documents
/// that don't participate in listings.
///
/// # `extra` and the "free-form bag" exception
///
/// `extra` is the *only* open-shape field in `DocumentProfile`.
/// Adding a key to `extra` does **not** require a profile-version
/// bump: the outer struct shape is unchanged, and consumers
/// (custom listing templates) opt in to specific keys.
///
/// Reaching into `extra` from outside the listings code path is
/// forbidden by the contract doc. If a future feature finds itself
/// wanting to read from `extra`, that is a design-review trigger,
/// not a code-completion shortcut.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ListingItemInfo {
    /// Override for the title displayed in listings. Defaults to
    /// `profile.title` when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    /// Override for the subtitle displayed in listings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,

    /// Listing description (text shown under the title).
    /// L1 fills this from the first plain-text paragraph of the
    /// post-include AST when unset; the L7 post-render upgrade
    /// may further upgrade with engine-rendered content.
    /// **Always** non-`None` after L1 runs for a non-empty
    /// document — see L1's "Safeguard contract."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Listing image src. L1 fills from the first body `Image`
    /// node when unset; L7 may upgrade. May be `None` even
    /// after L1 if the document has no images at all (the
    /// listing template falls back to its own placeholder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,

    /// Alt text for the listing image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_alt: Option<String>,

    /// Listing date (publication / display date). L1 honors an
    /// author-supplied value; auto-fill is not currently planned.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,

    /// Date the document was last modified. L1 fills from
    /// filesystem mtime when unset and mtime is available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date_modified: Option<String>,

    /// Listing categories. L1 may copy from
    /// `DocumentProfile::categories` when unset; this allows a
    /// document to advertise different categories for listings
    /// vs. the document itself. v1: copy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,

    /// Estimated reading time in minutes. L1 fills from
    /// word-count divided by a 200wpm constant when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reading_time_minutes: Option<u32>,

    /// Word count of the document body. L1 fills from a tokenized
    /// scan of the post-include AST when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub word_count: Option<u32>,

    /// Free-form fields advertised for custom listing templates.
    /// Author-declared in `listing-item.extra:` (or the equivalent
    /// per the schema decision in L2). Outer profile shape does
    /// **not** change when keys are added/removed, so no
    /// `profile_version` bump is required.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, ConfigValue>,
}

impl ListingItemInfo {
    /// True when no author-supplied or auto-filled data is present.
    /// Used by `DocumentProfile`'s `serde(skip_serializing_if = ...)`
    /// to keep on-disk profiles small for non-participating
    /// documents.
    pub fn is_empty(&self) -> bool {
        self.title.is_none()
            && self.subtitle.is_none()
            && self.description.is_none()
            && self.image.is_none()
            && self.image_alt.is_none()
            && self.date.is_none()
            && self.date_modified.is_none()
            && self.categories.is_empty()
            && self.reading_time_minutes.is_none()
            && self.word_count.is_none()
            && self.extra.is_empty()
        }
    }
```

### Adding the field to `DocumentProfile`

```rust
pub struct DocumentProfile {
    // … existing fields, unchanged …

    /// Per-document advertisement for listings that include this
    /// document. Scoped feature surface — listings consumers only.
    /// See [`ListingItemInfo`] and the contract doc's
    /// §"Scoped feature surfaces."
    ///
    /// L0 reads author-supplied values from `meta.listing-item` at
    /// extraction time. L1's `ListingItemInfoStage` fills holes
    /// before the checkpoint.
    ///
    /// Default empty; serializer omits empty.
    #[serde(default, skip_serializing_if = "ListingItemInfo::is_empty")]
    pub listing_item: ListingItemInfo,
}
```

Update `Default::default()` to add `listing_item: ListingItemInfo::default()`.

### Reading `listing-item:` in `extract`

Add a small extraction helper:

```rust
fn extract_listing_item(meta: &MetaMap /* … actual type of ast.meta */) -> ListingItemInfo {
    let Some(node) = meta.get("listing-item") else {
        return ListingItemInfo::default();
    };
    // Walk the ConfigValue / MetaValue map, populating fields
    // with the same tolerance the existing helpers use:
    //   - plain_text_field equivalent for scalars
    //   - extract_string_list for arrays
    //   - parse_extra_map for the extra: bag
    // Unknown keys at the top level of listing-item are dropped
    // (or collected into a diagnostic — see §"Diagnostics" below).
}
```

Call it from `DocumentProfile::extract`:

```rust
listing_item: extract_listing_item(meta),
```

The exact `MetaValue` walk follows the pattern already used by
`plain_text_field`, `extract_authors`, and `extract_string_list`
in the same file.

### `extra` map population

When the YAML has:

```yaml
listing-item:
  extra:
    status: "draft"
    sponsors: [Foo, Bar]
```

The extraction copies the `extra` sub-map's `ConfigValue` entries
verbatim into `ListingItemInfo::extra` as a `BTreeMap<String,
ConfigValue>`. No type coercion. Custom templates (L8) handle the
typed access at render time via `quarto-doctemplate`'s
`TemplateValue` conversion.

### Version bump

`DOCUMENT_PROFILE_VERSION: u32 = 3` → `4`.

The doc-comment "Version history" table inside
`document_profile.rs` gets a new entry:

```text
- `4`: bd-n8a4 — adds `listing_item: ListingItemInfo` for
  the listings-epic feature surface (curated typed fields plus
  `extra: BTreeMap<String, ConfigValue>` for custom templates).
```

### Diagnostics

For v1 of L0:

- Unknown keys at the top level of `listing-item:` produce no
  diagnostic. Quiet acceptance lets authors typo without
  blocking renders; the L2 schema work introduces strict
  validation. (If you find this distasteful, file a follow-up
  bd issue rather than tightening here — strict schema
  validation is L2's job.)
- Unknown nested keys inside `listing-item.extra:` are *valid by
  design*. Don't warn.
- Type mismatches at known keys (e.g.
  `listing-item.reading-time: [bad, type]`) follow the same
  graceful-degradation pattern other extractors use: drop the
  bad value, leave the field at its default. No crash.

## Pipeline / stage impact

**None.** L0 changes the *shape* of the profile, not the
*production* of it. `DocumentProfileStage` already calls
`DocumentProfile::extract`; that call now reads one more key.
No new stage; no pipeline-builder change; no new
`PipelineDataKind` variant.

The only ripple effect is at the cache layer: any cached
serialized profile from a prior build will fail to deserialize
under the new `DOCUMENT_PROFILE_VERSION` and be silently
regenerated. This is the intended behavior, identical to the
v2 → v3 bump for `bd-o8pr`.

## YAML schema update

The user-facing strict YAML validation lives at
`crates/pampa/test-fixtures/schemas/definitions.yml` (reference
fixtures used in tests; no production frontmatter validator
gates renders today — see L0 §"Schema status reality check"
below). For L0:

- Add a `listing-item` document-level schema definition under
  the existing definitions.yml file, mirroring the
  `ListingItemInfo` shape.
- Allow `listing-item:` as a top-level frontmatter key on
  HTML documents (matching the navbar / `image` / `categories`
  precedent in the same file).
- The full `listing:` schema (the one that drives the listings
  resolver itself) is L2's job, not L0's.

### Schema status reality check

Today Q2 does not enforce frontmatter schema validation as a
hard gate during renders. The schema fixtures exist for tests
and forward-looking validation work. L0's schema entry is
therefore primarily a documentation/test artifact — it does not
block frontmatter that omits or misuses `listing-item:`. If a
future phase wires schemas into the production pipeline, L0's
entry already exists and slot in. If you find the schema
plumbing is more invasive than this brief, scope-down to "add
the entry to the test fixture and exercise it from one new
test"; do not expand L0 to cover schema runtime wiring.

## Contract doc changes

### New field row in §"Guarantees"

| Field | Guarantee |
|---|---|
| `listing_item` | A [`ListingItemInfo`] holding per-document advertisement for listings consumers. **Scoped feature surface — listings only**; non-listing consumers must use the corresponding top-level fields (`title`, `description`, `image`, …). Author-supplied values populate during `DocumentProfile::extract`; L1's `ListingItemInfoStage` fills holes. The nested `extra: BTreeMap<String, ConfigValue>` is the only open-shape field in the profile and is forbidden to non-listing consumers — see §"Scoped feature surfaces". Default empty (`ListingItemInfo::is_empty()`). |

### New §"Scoped feature surfaces"

Add a top-level section after §"Mutability":

```markdown
## Scoped feature surfaces

Most profile fields are typed, narrowly defined, and globally
readable: any consumer that needs `title`, `categories`,
`outline`, etc. reaches for the top-level field directly. The
contract is closed-shape, versioned, and stable.

The `listing_item` field is an **explicit exception**, scoped
to one feature (listings) by name and by convention.

**Allowed:** the listings code path (`L3 ListingResolveTransform`,
`L5 CategoriesSidebarTransform`, `L7 post-render upgrade`,
`L9 RSS feeds`) reads `profile.listing_item` to materialize
listing items.

**Forbidden:** any code outside the listings module reaches into
`profile.listing_item` (and especially into
`profile.listing_item.extra`). Sidebar generation, navbar
rendering, cross-doc link rewriting, freeze, and other features
must continue to use the typed top-level fields. If a future
feature finds itself wanting to read `listing_item`, that is a
**redesign trigger** — either widen the typed top-level field
set with a versioned bump, or define a new scoped feature
surface. Do not silently broaden listings' scope.

The discipline is enforced by code review, not the type system.
The `listing_item` field is `pub` for serde and for listings'
own use; the contract above is the boundary that matters.

This is the same discipline `bd-fegm` (Phase 8) used when it
declined to add a generic `extras: HashMap` field for filter-
introduced data and chose typed fields instead. The exception
here is granted because (a) custom listing templates genuinely
need access to author-declared free-form metadata, and (b) the
"named, scoped" framing keeps the cost of the exception
locally bounded.
```

### Change log entry

Add to the bottom of the contract doc:

```markdown
- **2026-05-05 — v4 (`bd-n8a4`).** `DOCUMENT_PROFILE_VERSION`
  bumped 3 → 4. One new field:
  - `listing_item: ListingItemInfo` — scoped per-feature
    surface for listings consumers. Curated typed sub-fields
    plus `extra: BTreeMap<String, ConfigValue>` for custom-
    listing-template fields. Default empty
    (`ListingItemInfo::is_empty()`). Outer profile shape stable;
    additions or removals of keys inside `extra` do **not**
    require a future bump. Non-listing consumers are forbidden
    from reading this field — see new §"Scoped feature surfaces".
  v3 cache entries on disk are rejected with
  `DocumentProfileError::VersionMismatch` and silently
  regenerated, identical to the v2 → v3 bump.
```

## Tests

TDD: each test below is written first, run, and watched fail
(except the infrastructure ones that test serde mechanics —
those pass on a stub). Implementation lands after failures are
confirmed.

### Unit tests in `document_profile.rs` test module

1. **`listing_item_info_default_is_empty`** — `ListingItemInfo::default().is_empty() == true`. Sanity.
2. **`listing_item_info_partial_not_empty`** — setting *any* one field flips `is_empty()` to false. Cover each field; this is the load-bearing fence on the serialization skip.
3. **`listing_item_info_serde_roundtrip_empty`** — serialize an empty `ListingItemInfo`, deserialize, assert equality. Round-trip via JSON.
4. **`listing_item_info_serde_omits_empty_fields`** — serialize a partial value, assert the JSON omits unset fields and the empty `extra` map.
5. **`listing_item_info_extra_roundtrip`** — populate `extra` with a string, an array, and a nested map; serde round-trip; assert structural equality of the `ConfigValue` entries.
6. **`profile_default_listing_item_is_empty`** — `DocumentProfile::default().listing_item.is_empty()` is true; serializing the default produces JSON with no `listing-item` key (or with an empty one — verify against `skip_serializing_if`).
7. **`profile_extract_no_listing_item_key`** — frontmatter without a `listing-item:` key produces an empty `ListingItemInfo`.
8. **`profile_extract_listing_item_curated_fields`** — frontmatter:
    ```yaml
    listing-item:
      title: "Listing title"
      description: "Listing desc"
      reading-time-minutes: 15
      categories: [a, b]
    ```
    populates the corresponding fields.
9. **`profile_extract_listing_item_extra_passthrough`** — frontmatter:
    ```yaml
    listing-item:
      extra:
        status: draft
        sponsors: [Foo, Bar]
    ```
    populates `extra` with the right `ConfigValue` shapes.
9b. **`profile_extract_listing_item_extra_namespace_distinct`** —
    per C6, frontmatter:
    ```yaml
    listing-item:
      title: "Curated"
      extra:
        title: "Custom"
    ```
    produces `listing_item.title == Some("Curated")` *and*
    `listing_item.extra["title"] == ConfigValue::String("Custom")`.
    Confirms the two namespaces don't collide.
10. **`profile_extract_listing_item_unknown_top_key_dropped`** — `listing-item: { not-a-known-field: 42 }` does not panic and produces an empty `ListingItemInfo` (the unknown key drops). Confirms graceful degradation per §Diagnostics.
11. **`profile_extract_listing_item_type_mismatch_dropped`** — `listing-item: { reading-time-minutes: [bad, type] }` does not panic; `reading_time_minutes` stays `None`. (Per C5, no diagnostic in L0; that's L2's job.)
12. **`document_profile_version_is_4`** — `DOCUMENT_PROFILE_VERSION == 4`. Catches accidental version-bump regressions.
13. **`profile_v3_json_rejected_with_version_mismatch`** — synthesize a JSON string with `"profile_version": 3`; assert `DocumentProfile::from_json` returns `DocumentProfileError::VersionMismatch { expected: 4, found: 3 }`.

### Integration test

14. **`pipeline_extracts_listing_item_from_frontmatter`** —
    end-to-end: parse a fixture `.qmd` with `listing-item:` in
    frontmatter, run the pipeline up to the profile checkpoint,
    inspect the extracted profile, assert `listing_item.title`
    matches the author value. This is the cross-stage smoke
    test that proves the wiring works under realistic
    `MetadataMergeStage` output.

### Snapshot tests

15. None for L0. Snapshots arrive at L3 when listings actually
    render. Run the workspace test suite per CLAUDE.md before
    and after L0; **any snapshot diff is a red flag** and must
    be investigated. None are expected — the new field defaults
    empty for every existing fixture.

### End-to-end CLI verification

Per CLAUDE.md §"End-to-end verification before declaring success":

- Run `cargo run --bin q2 -- render <fixture>.qmd` on three
  existing fixtures from
  `crates/quarto-core/tests/fixtures/`. Output should be
  byte-identical before and after L0 (the new field doesn't
  affect rendering — listings come in L3+).
- Render one new fixture under
  `crates/quarto-core/tests/fixtures/listings-l0/` that uses
  `listing-item: { title: "X" }` in frontmatter; verify it
  *also* renders byte-identically to the same fixture without
  the listing-item key (the field is only consumed by listings
  code that doesn't exist yet). Record MD5 hashes in the L0
  completion note.

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement, watch
pass.

### Preparation

- [ ] Re-read
      `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [ ] Create a worktree under `.worktrees/listings-l0/` per
      `.claude/rules/worktrees.md` (branch `beads/bd-n8a4-listing-item-info-profile`).
- [ ] `npm install` in the worktree (Rust+hub bootstrap; see
      `.claude/rules/worktrees.md`).
- [ ] `cargo xtask verify --skip-hub-build` to baseline.

### TDD phase — tests first, all failing (except serde mechanics)

- [ ] Add `ListingItemInfo` skeleton (struct fields, `Default`
      derive, empty `is_empty()` returning `false` so tests fail
      meaningfully) so the tests below compile.
- [ ] Write unit tests 1–13 in `document_profile.rs`'s test
      module. **Run, observe failures, record which pass on the
      stub** (the serde-mechanics tests will pass; behavior tests
      will fail — same pattern as Phase 0).
- [ ] Write integration test 14 in
      `crates/quarto-core/tests/`. *Result expected:* fails
      because `listing_item` extraction is not yet wired into
      `extract`.

### Implementation

- [ ] Implement `ListingItemInfo::is_empty()` correctly.
- [ ] Add `listing_item: ListingItemInfo` to `DocumentProfile`
      with `#[serde(default, skip_serializing_if = "ListingItemInfo::is_empty")]`.
- [ ] Update `DocumentProfile::Default::default()` to include
      `listing_item: ListingItemInfo::default()`.
- [ ] Implement `extract_listing_item(meta)` walking
      `meta.get("listing-item")`'s `ConfigValue` map.
- [ ] Wire `extract_listing_item` into
      `DocumentProfile::extract`.
- [ ] Bump `DOCUMENT_PROFILE_VERSION` 3 → 4. Update the version-
      history doc-comment block.
- [ ] Run unit + integration tests; all 14 must pass.

### Documentation

- [ ] Update
      `claude-notes/designs/document-profile-contract.md`:
      add the `listing_item` row, the new §"Scoped feature
      surfaces", and the v4 change-log entry.
- [ ] Add a doc comment on `DocumentProfile.listing_item`
      pointing at the contract doc and at this sub-plan.
- [ ] Add the YAML schema entry under
      `crates/pampa/test-fixtures/schemas/definitions.yml`. If
      this turns out to require more than ~30 lines of fixture
      changes, *stop*, file a follow-up bd issue, and finish L0
      without runtime schema wiring — see §"Schema status
      reality check" above.

### Verification and close-out

- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace` — entire workspace passes,
      no snapshot diffs (any diff is a red flag — investigate
      per CLAUDE.md §"Snapshot Test Changes").
- [ ] `cargo xtask lint` passes.
- [ ] `cargo xtask verify` (full, including hub-client) passes.
      L0 touches a `quarto-core` type; hub-client builds against
      it. This step *must* run.
- [ ] End-to-end CLI verification on three existing fixtures +
      the new listings-l0 fixture; MD5 hashes recorded.
- [ ] Stop and request user permission before any push (per
      CLAUDE.md §"GIT PUSH POLICY").
- [ ] `br update bd-n8a4 --status closed` with a reason after
      user approval.
- [ ] `br sync --flush-only && git add .beads/ && git commit`
      from the **main repo** (per
      `.claude/rules/worktrees.md` §"Committing beads changes").

## Risks and mitigations

- **Risk: contract doc § "Scoped feature surfaces" is the
  load-bearing piece, and it's just text.** That's the entire
  point — the discipline is enforced by code review, not the
  type system. *Mitigation:* the section is explicit, names the
  forbidden access patterns, and identifies redesign triggers.
  Reviewers of L1, L3, L5, L7, L9 must check that no consumer
  outside listings reads `listing_item`. The fact that the rule
  is written down in *one place* and referenced from the
  `listing_item` doc comment is the mitigation.
- **Risk: existing tests break because of the new field.**
  *Mitigation:* `serde(skip_serializing_if = ...)` keeps the
  on-disk JSON shape unchanged for documents without
  `listing-item:` frontmatter. The `Default` impl populates
  the field with an empty struct. Existing snapshots
  shouldn't move; if they do, that's a real regression to
  investigate.
- **Risk: WASM build picks up `BTreeMap<String, ConfigValue>`
  in a way that breaks hub-client.** Both types are already
  WASM-compatible (used elsewhere in the profile). *Mitigation:*
  `cargo xtask verify` (full) is mandatory.
- **Risk: schema-fixtures update spirals.** *Mitigation:*
  §"Schema status reality check" above explicitly bounds the
  schema work; a single fixture entry is sufficient.
- **Risk: cache invalidation cascade.** v3 → v4 bump
  invalidates every Phase-8 cached profile. *Mitigation:* this
  is the *correct* behavior; `VersionMismatch` triggers
  silent regeneration. Document in the change-log entry.
  Identical to the v2 → v3 cascade for `bd-o8pr`.
- **Risk: `extra` extraction copies `ConfigValue` shapes the
  rest of the codebase doesn't expect to round-trip through
  serde.** *Mitigation:* test 5 covers exactly this. If it
  fails on a particular `ConfigValue` variant, *stop and ask
  the user* — do not work around by stripping that variant.
  The extra-bag's value is that it preserves whatever the
  author wrote.

## Explicit non-goals for this phase

- **No `ListingItemInfoStage`.** Auto-fill is L1. L0 only reads
  what the author wrote.
- **No reading-time / word-count / first-paragraph extraction.**
  All L1.
- **No `listing:` schema or transform.** L2 / L3.
- **No production schema-validation gating.**
  §"Schema status reality check".
- **No removal of overlapping top-level fields.** Top-level
  `title`, `description`, `image`, `categories`, etc. continue
  to exist and serve their existing consumers. `listing_item`
  *overrides* them for listings, but only listings.
- **No structured author model.** Same defer as elsewhere; if
  a custom listing wants structured author info, the author
  puts it in `listing-item.extra.authors` and the custom
  template reads it.

## Decisions log

Recording the decisions made writing this sub-plan; if the user
disagrees with any, push back before implementation starts.

- **D1 (placement):** keep `ListingItemInfo` inside
  `document_profile.rs`. Rationale in §"Type design".
- **D2 (version bump target):** `3 → 4`, not `2 → 3` as the
  epic plan stated. The epic was authored before recognizing
  `bd-o8pr` had already taken `3`. No change to the epic
  decision; just the integer.
- **D3 (categories handling):** Listings consumers (L1+) compute
  effective categories by feeding `profile.categories` and
  `profile.listing_item.categories` into `MergedConfig` and
  letting the tag rules decide. Default array behavior is
  concat; authors opt into override with `!prefer`. L0 stores
  both fields as written — see C4 above and D7 below for the
  tagged-value preservation requirement.
- **D7 (preserving ConfigValue tags through the profile):** L0
  preserves the originating `ConfigValue` for both
  `profile.categories` and `profile.listing_item.categories`
  alongside the flattened `Vec<String>` form, so the tag-aware
  merge in D3 has tagged values to merge. Concretely: add a
  field `categories_raw: Option<ConfigValue>` on
  `DocumentProfile` *and* on `ListingItemInfo` (or, if the
  existing extraction path conveniently keeps the raw value,
  surface it without renaming). The flattened `Vec<String>`
  remains the primary consumer surface; the raw form is a
  secondary, listings-aware surface. Naming TBD during impl
  (e.g. `categories_source`, `categories_raw`). If the change
  requires touching many existing consumers of
  `profile.categories`, *stop* and confer with the user — D7
  may need to be lifted out of L0.
- **D4 (extra field encoding):** `BTreeMap<String, ConfigValue>`,
  not a generic `serde_json::Value`. The profile's existing
  serde stack uses `ConfigValue`; consistency with the rest of
  the type wins. `BTreeMap` over `HashMap` for deterministic
  serialization (CLAUDE.md §"HashMap and Determinism").
- **D5 (diagnostics for unknown keys):** drop silently.
  Strict validation belongs at L2's schema layer or in a future
  schema-runtime pass.
- **D6 (stage-name reservation):** L0 does not introduce
  `ListingItemInfoStage`. The name is reserved for L1 (`bd-izqh`).

## Open sub-questions (defer; do not block L0)

- **Naming for the tag-preserving raw fields (D7).**
  `categories_raw`? `categories_source`? `raw_categories`? Pick
  during impl based on what reads naturally next to the
  flattened `categories: Vec<String>` field. Not a
  decision-blocker.
- **Should other curated `ListingItemInfo` fields also preserve
  raw `ConfigValue` for tag-aware merging?** D7 only addresses
  `categories` because that's the field with a clear
  multi-source merge pattern (top-level + listing-item). Other
  fields (`title`, `description`, etc.) are scalars and the
  override semantics are unambiguous (when both are set, listings
  consumers pick `listing_item.<field>`). If a future need
  arises (e.g. structured author lists merging), revisit then.
- Does the contract's §"Scoped feature surfaces" need a
  cross-link from each typed top-level field's doc comment
  ("see §Scoped feature surfaces for what listings get
  instead")? Probably overkill; one link from
  `listing_item`'s doc comment back to the section is
  sufficient.

## Filing reminder

This sub-plan corresponds to `bd-n8a4`. After implementation:

1. Update the issue with status `closed` plus a one-line
   reason linking back to this plan.
2. `br sync --flush-only && git add .beads/ && git commit`
   from the main repo (per `.claude/rules/worktrees.md`).
3. Add bd ids and resolved follow-ups (if any) to this
   sub-plan's "Decisions log" or a new "Follow-ups" section
   before closing.
