# L2 — Listing data model + schema (reference document)

**Date:** 2026-05-06
**Beads:** `bd-j60g`. Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:** L0 (`bd-n8a4`, closed) and L1 (`bd-izqh`, closed)
deliver the per-document `listing_item` substrate. L2 is the
*per-host-page* configuration substrate that L3+ consume.
**Status:** Draft. Awaiting user approval before hand-off.

## What this document is

Per the epic plan (§"L2") and the user's instruction at the start of
this session: **L2 does not ship runtime code.** Q2 has no
production-grade YAML validation gate today (see
`crates/pampa/test-fixtures/schemas/definitions.yml` reality check
in L0's plan). Until that gate exists, an L2 that ports Q1's
`Listing` / `ListingItem` types into Rust would either (a) sit
unused alongside the rest of Pass-2 (carrying maintenance cost
without payoff) or (b) be implemented as part of L3's actual
consumer code (where it belongs).

This document is therefore the **single canonical reference** that
L3, L4, L8, L9, and L10 read for:

1. The Q1 → Q2 data-model mapping (which Q1 types become which
   Q2 types, with idiom changes called out).
2. The YAML schema shape (a port of `website-listing` from Q1's
   `definitions.yml`).
3. The per-item template binding (the `TemplateValue::Map` that
   the listing render transform builds for each item).
4. Default values and validation semantics.
5. The `template:` extension convention.
6. Schema-placement reality (where it lives today; what changes
   when Q2 gains a runtime validator).

L2's bd issue (`bd-j60g`) closes once this document is approved
and committed. There is no Rust code to write under L2.

## Why a reference doc, not code

Three reasons:

1. **No runtime gate.** The schema entry under
   `crates/pampa/test-fixtures/schemas/definitions.yml` (already
   present, ported from Q1) is exercised by tests but does not
   gate frontmatter at render time. Adding Rust types now would
   not buy us validation diagnostics that users see; it would
   only duplicate the schema as Rust struct definitions.
2. **L3 is the first consumer.** The natural home for the Rust
   types is `crates/quarto-core/src/project/listing/`, defined by
   L3 as part of its implementation. Splitting L3's "define the
   types" from "consume them" into separate sessions would either
   require L2 to anticipate L3's exact field needs (premature
   commitment) or leave L2's types unused for one session
   (dead code in main).
3. **The shape is what stabilizes here.** Once this document is
   approved, L3 has a fixed target. If a future runtime YAML
   validator gets wired in, this doc is what the validator's
   schema reflects. If a colleague comes back to listings six
   months from now and needs to know "what's the schema",
   this doc answers that question without code archaeology.

## Reference material

Read first:

- Parent epic: `claude-notes/plans/2026-05-05-listings-epic.md`
  §"L2".
- Design rationale:
  `claude-notes/plans/2026-05-05-listings-design-discussion.md`
  §"What Q1 listings actually do" + §"How that maps onto Q2's
  existing machinery" + §"Custom listings via
  `quarto-doctemplate` — feasibility study".
- L0 sub-plan (delivered the per-doc substrate):
  `claude-notes/plans/2026-05-05-listings-L0-profile-extension.md`.
- L1 sub-plan (delivered the auto-fill stage):
  `claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`.
- Q1 type definitions (the source we're porting from):
  `external-sources/quarto-cli/src/project/types/website/listing/website-listing-shared.ts`.
- Q1 schema (the source we're porting from):
  `external-sources/quarto-cli/src/resources/schema/definitions.yml`
  lines 1502–1765 (`website-listing`,
  `website-listing-contents-object`).
- Q1 hydration logic (the source we're porting from for default
  field sets, type-specific defaults):
  `external-sources/quarto-cli/src/project/types/website/listing/website-listing-read.ts`.
- Q1 templates (reference shapes for L3's built-ins):
  `external-sources/quarto-cli/src/resources/projects/website/listing/`
  (`item-default.ejs.md`, `listing-default.ejs.md`,
  `item-grid.ejs.md`, `listing-grid.ejs.md`,
  `listing-table.ejs.md`).
- Existing schema fixture entry (already imported from Q1; the
  starting point for the L2 schema port):
  `crates/pampa/test-fixtures/schemas/definitions.yml`
  lines 1439–1711 (`website-listing` definition) + 1717–1754
  (`listing-item` per-doc shape, added by L0).
- Existing Q2 generate/render precedent (the architectural
  pattern L3 will follow):
  `crates/quarto-core/src/transforms/navbar_generate.rs` +
  `crates/quarto-core/src/transforms/navbar_render.rs`.

## Settled inputs (from the epic)

These are not open questions for L2; they are inputs:

- **Schema placement: top-level `listing:` frontmatter key**
  (epic decision 5). On the host page's frontmatter; not under
  `website:`.
- **Custom-template extension: `.template`** (epic decision 1).
  `.ejs.md` accepted with a deprecation diagnostic.
- **Templating engine: `quarto-doctemplate`** (epic settled
  decision 3 + the discussion doc's feasibility study).
- **Built-ins ship as embedded `MemoryResolver` partials.**
  Authors can read them as the canonical reference.
- **`contents:` accepts globs in v1.** Inline-metadata records
  validate against the schema (Q1-compat at the YAML level) but
  L3 emits a "not yet supported" diagnostic until a follow-up bd
  issue lands them. Decided 2026-05-06.

## Q1 → Q2 type mapping

This section describes the *shape* L3 will define in
`crates/quarto-core/src/project/listing/`. L3 owns the actual
struct definitions; L2 documents the target.

### Top-level: `Listing` (per-host-page, configures one listing)

Q1 (`website-listing-shared.ts:163`):

```ts
export interface Listing extends ListingDehydrated {
  fields: string[];
  [kFieldDisplayNames]: Record<string, string>;
  [kFieldTypes]: Record<string, ColumnType>;
  [kFieldLinks]: string[];
  [kFieldSort]: string[];
  [kFieldFilter]: string[];
  [kFieldRequired]: string[];
  [kPageSize]: number;
  [kMaxItems]?: number;
  [kFilterUi]: boolean;
  [kSortUi]: boolean;
  [kImagePlaceholder]?: string;
  sort?: ListingSort[];
  template?: string;
  [kGridColumns]?: number;
}
```

Q2 (target shape; L3 implements):

```rust
/// One listing declared on a host page. Authors put one or more of
/// these under the top-level `listing:` frontmatter key.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    pub id: String,
    pub r#type: ListingType,
    pub contents: Vec<ListingContents>,    // globs + (deferred) inline records
    pub fields: Vec<String>,                // user override; defaults per type
    pub field_display_names: BTreeMap<String, String>,
    pub field_types: BTreeMap<String, ColumnType>,
    pub field_links: Vec<String>,
    pub field_sort: Vec<String>,
    pub field_filter: Vec<String>,
    pub field_required: Vec<String>,
    pub page_size: u32,                     // default 30 (Q1)
    pub max_items: Option<u32>,              // None = unlimited
    pub filter_ui: bool,                    // default true for table; false otherwise
    pub sort_ui: bool,                      // default true for table; false otherwise
    pub image_placeholder: Option<String>,
    pub sort: Option<Vec<ListingSort>>,
    pub template: Option<PathBuf>,          // custom-template path
    pub template_params: BTreeMap<String, ConfigValue>, // exposed in binding
    // Type-specific knobs:
    pub grid_columns: Option<u32>,          // default 3 for grid
    pub grid_item_border: Option<bool>,     // default true for grid
    pub grid_item_align: Option<GridItemAlign>,
    pub table_striped: Option<bool>,
    pub table_hover: Option<bool>,
    pub image_align: Option<ImageAlign>,    // default for default
    pub image_height: Option<String>,       // CSS height
    pub image_lazy_loading: Option<bool>,   // default true
    pub date_format: Option<String>,
    pub max_description_length: u32,        // default 175 (Q1)
    pub include: Vec<ListingFilter>,
    pub exclude: Vec<ListingFilter>,
    pub categories: ListingCategoriesMode,  // disabled / numbered / unnumbered / cloud
    pub feed: Option<ListingFeedOptions>,
}
```

Idiom changes from Q1:

- **`Option<T>` over sentinel values.** Q1 uses `0` /
  `undefined` interchangeably; Q2 picks `None` for "unset".
- **Enums over string flags.** `r#type`, `image_align`,
  `grid_item_align`, `categories` are typed enums instead of
  strings.
- **`PathBuf` for filesystem paths.**
- **Boolean defaults are explicit on the struct.** Q1 reads
  defaults at hydration time; Q2 sets them in `Default` so the
  hydrated `Listing` is self-contained.
- **`contents` is `Vec<ListingContents>`,** an enum that
  represents either a glob string or (deferred) an inline
  record. See §"contents" below.

### Supporting types

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListingType {
    #[default]
    Default,
    Grid,
    Table,
    Custom,    // requires `template:` to be set
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingContents {
    /// One of the matched-file globs (default: every `.qmd`
    /// next to the host page minus the host itself).
    Glob(String),
    /// Inline metadata record. Deferred — schema accepts; L3
    /// emits a "not yet supported" diagnostic. The shape is
    /// preserved so the deferred-implementation bd issue can
    /// pick up where the schema parsing leaves off.
    Inline(BTreeMap<String, ConfigValue>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingSort {
    pub field: String,             // "title", "date", "author", or any field name
    pub direction: SortDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnType {
    Date,
    String,
    Number,
    Minutes,    // for reading-time-style fields
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListingCategoriesMode {
    #[default]
    Disabled,
    Default,        // Q1's `category-default`
    Unnumbered,
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingFilter {
    /// `include: [{ author: "Foo" }]` matches items with
    /// `author == "Foo"`. Multiple keys = AND. Multiple
    /// `include` records = OR.
    pub fields: BTreeMap<String, ConfigValue>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListingFeedOptions {
    pub items: Option<u32>,                // default 20 in Q1
    pub r#type: FeedType,                  // partial / full / metadata
    pub title: Option<String>,
    pub description: Option<String>,
    pub categories: Vec<String>,           // per-category sub-feeds
    pub image: Option<String>,
    pub language: Option<String>,
    pub xml_stylesheet: Option<PathBuf>,   // L9; out of scope for v1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FeedType {
    #[default]
    Full,
    Partial,
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageAlign {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridItemAlign {
    Left,
    Right,
    Center,
}
```

### Per-item: `ListingItem`

A *resolved* item that L3's render transform iterates over to fill
the template. Hydrated from a `DocumentProfile` (specifically
`profile.listing_item` plus the curated top-level fields) at
render time. **Not stored on disk anywhere** — built inside L3's
generate transform per host-page render.

```rust
/// One resolved listing item. Built from a `DocumentProfile`
/// (or, deferred, from an inline metadata record) at render time.
#[derive(Debug, Clone)]
pub struct ListingItem {
    /// Display title. Falls back through:
    ///   listing_item.title → profile.title → filename stem.
    pub title: String,
    pub subtitle: Option<String>,
    pub description: Option<String>,        // L1 fallback or L7-upgraded
    pub author: Vec<String>,                // joined display string built at template time
    pub date: Option<String>,               // parsed at consumer side per `date-format`
    pub date_modified: Option<String>,
    pub categories: Vec<String>,
    pub image: Option<String>,
    pub image_alt: Option<String>,
    pub image_lazy_loading: Option<bool>,
    pub reading_time_minutes: Option<u32>,
    pub word_count: Option<u32>,

    /// Project-relative source path of the item's input file.
    /// Used to populate `item.path` for templates.
    pub source_path: PathBuf,

    /// Output href. The template's link target.
    pub output_href: String,

    /// Author-declared free-form fields (from
    /// `profile.listing_item.extra`). Templates read these via
    /// `$item.extra.<key>$`. Always present in the binding,
    /// possibly empty.
    pub extra: BTreeMap<String, ConfigValue>,
}
```

Field hydration rules:

- `title`: `listing_item.title` ?? `profile.title` ??
  `source_path.file_stem()`.
- `subtitle`: `listing_item.subtitle` ?? `profile.subtitle`.
- `description`: `listing_item.description` (already auto-filled
  by L1) ?? `profile.description`.
- `image`: `listing_item.image` ?? `profile.image`.
- `categories`: tag-aware merge of
  `profile.categories_raw` + `profile.listing_item.categories_raw`
  via `MergedConfig`, falling back to the flattened lists when
  no merge tags are present (per L0 §C4).
- `date_modified`: `listing_item.date_modified` (auto-filled by
  L1) ?? `profile.date_modified`.
- `reading_time_minutes`, `word_count`: from `listing_item` —
  L1 always populates these.
- `extra`: copied verbatim from `listing_item.extra`.

## YAML schema

The schema is a port of Q1's `website-listing` definition,
already partially present in
`crates/pampa/test-fixtures/schemas/definitions.yml` lines
1439–1711.

### Top-level shape

The `listing:` key on a host page's frontmatter accepts:

- **A single `Listing` object** — most common case, one
  listing on the page.
- **An array of `Listing` objects** — multiple listings on one
  page (rare but supported in Q1).
- **A boolean `true`** — shorthand for "default listing of all
  sibling `.qmd` files," equivalent to
  `{ id: "listing", type: default, contents: ["*.qmd"] }`.

### Required vs optional

- `id` — optional. L3 synthesizes one (`listing-1`,
  `listing-2`, …) at hydration time when absent.
- `type` — optional. Defaults to `default`.
- All other fields optional.

### Type-specific defaults

Per Q1's `hydrateListing` (`website-listing-read.ts`), the
default `fields` set varies by `type`:

| Type      | Default `fields`                                                                                       |
|-----------|---------------------------------------------------------------------------------------------------------|
| `default` | `[date, title, author, subtitle, description, image, image-alt, categories, filename, file-modified, reading-time]` |
| `grid`    | `[title, subtitle, author, date, image, image-alt, description, categories, filename, file-modified, reading-time]` |
| `table`   | `[date, title, author]`                                                                                |
| `custom`  | `[]` — author template decides                                                                        |

Other type-specific defaults:

| Field               | `default` | `grid` | `table` | `custom` |
|---------------------|-----------|--------|---------|----------|
| `image-align`       | `right`   | —      | —       | —        |
| `image-height`      | `120px`   | —      | —       | —        |
| `grid-columns`      | —         | `3`    | —       | —        |
| `grid-item-border`  | —         | `true` | —       | —        |
| `grid-item-align`   | —         | `left` | —       | —        |
| `sort-ui`           | `false`   | `false`| `true`  | `false`  |
| `filter-ui`         | `false`   | `false`| `true`  | `false`  |
| `page-size`         | `25`      | `25`   | `30`    | `25`     |
| `max-description-length` | `175` | `175`  | `175`   | `175`    |
| `image-lazy-loading`| `true`    | `true` | `true`  | `true`   |
| `image-placeholder` | (none)    | (none) | (none)  | (none)   |

### `contents:` field

Q1 accepts `Array<string | Metadata>`. L2 schema reflects this.
L3 implementation:

- **String entries** → `ListingContents::Glob(s)`. Resolved via
  the existing project glob expander
  (`crates/quarto-core/src/project/discovery.rs`'s
  `expand_patterns` style). L3's sub-plan documents whether to
  reuse it directly or spin a project-relative variant. The
  pattern grammar is the one already documented for
  `_quarto.yml`'s `project.render`: literal paths, `*`, `?`,
  `**`.
- **Object entries** → `ListingContents::Inline(map)`. Schema
  accepts (Q1 docs and existing migrations rely on this); L3
  emits a `Q-listing-1` "inline contents not yet supported"
  diagnostic per object entry and continues processing the
  glob entries. A follow-up bd issue (file under L11 close-out)
  picks up inline-record support.

Default `contents:` when omitted is `["*.qmd"]` relative to the
host page's directory, minus the host itself. This matches Q1's
default (see `readContents` in `website-listing-read.ts`).

### Sort key parsing

`sort:` accepts:

- `false` or empty array → preserve insertion order.
- A string like `"date desc"` → one `ListingSort { field:
  "date", direction: Desc }`.
- An array of strings → multi-key sort, applied in declared
  order.

If the direction suffix is omitted (e.g. just `"date"`),
direction defaults to `Asc`. Whitespace separates field from
direction; multiple spaces tolerated.

### Schema placement reality

Today, `crates/pampa/test-fixtures/schemas/definitions.yml`
holds the schema entries (already ported from Q1). It does
**not** gate render-time frontmatter. L3 reads the listing
config from `ast.meta` directly, applying the defaults above,
and emits diagnostics for shape mismatches it encounters.

When Q2 gains a runtime YAML validator (no bd issue filed for
this; tracked informally in `quarto-yaml-validation`'s
roadmap), the schema entries become the source of truth that
the validator enforces. L2's schema is therefore "schema as
documentation" today, "schema as enforcement" later, with no
shape change between the two.

### bd-n9dr reconciliation

The still-open question of whether navbar / sidebar /
similar configs should be **top-level** keys or **namespaced
under `website:`** (`bd-n9dr`) bears on listings indirectly.
The decision for L2: follow Q1 and put `listing:` at the
**top level of the host page's frontmatter**. Rationale:

- Listing config is per-host-page, not per-website. Putting
  it under `website:` would be a shape mismatch (the same
  page can have one listing, two listings, or none).
- `bd-n9dr` is about *site-level* config keys; listings are
  page-level.

If `bd-n9dr` ultimately decides on a namespaced placement
for site-level config, listings remain at top level. The two
decisions are independent.

## Per-item template binding (load-bearing for L3 / L4 / L8)

This is the contract every listing template (built-in or
custom) consumes. L3 builds it server-side; L4's
`ConfigValue → TemplateValue` bridge powers the `extra` map's
typed access; L8's custom templates read the same shape.

```text
TemplateValue::Map({
  // Per-listing context:
  "listing": Map({
    "id":              String("my-listing"),
    "type":            String("default"),  // or "grid", "table", "custom"
    "fields":          List([String("title"), String("date"), …]),
    "show":            Map({ "title": Bool(true), "date": Bool(true), … }),
    "page-size":       Number(25),
    "image-align":     String("right"),
    "image-lazy-loading": Bool(true),
    "image-height":    String("120px"),
    "image-placeholder": String(<resolved-url-or-empty>),
    "grid-columns":    Number(3),
    "grid-item-border": Bool(true),
    "grid-item-align":  String("left"),
    "max-description-length": Number(175),
    "filter-ui":       Bool(false),
    "sort-ui":         Bool(false),
    "categories":      String("default"),   // or "" / "unnumbered" / "cloud"
    "template-params": Map(<author-supplied params>),
  }),

  // The item set:
  "items": List([
    Map({
      // Curated typed fields (from `profile.listing_item` +
      // `profile.<top-level>` fallbacks; see hydration rules):
      "title":          String("My post"),
      "subtitle":       String("…"),
      "description":    String("First paragraph or L7-upgraded."),
      "date":           String("2026-04-01"),       // formatted per listing.date-format
      "date-modified":  String("2026-04-15"),
      "author":         String("Jane Doe, John Roe"), // joined display
      "authors":        List([String("Jane Doe"), String("John Roe")]),
      "categories":     List([String("rust"), String("design")]),
      "image":          String("img.png"),
      "image-alt":      String("Alt text"),
      "path":           String("posts/foo.qmd"),     // project-relative source path
      "outputHref":     String("posts/foo.html"),
      "reading-time":   String("15 min"),            // formatted per L3 helper
      "word-count":     Number(2873),

      // L3-pre-rendered helper strings (replace Q1 utilities;
      // built server-side in the render transform — templates
      // never call functions):
      "image-html":      String("<img src=\"img.png\" class=\"thumbnail-image\" alt=\"…\" loading=\"lazy\">"),
      "metadata-attrs":  String("data-index=\"0\" data-categories=\"rust,design\""),
      "category-html":   String("<div class=\"listing-categories\">…</div>"),

      // Free-form author fields (from `profile.listing_item.extra`):
      "extra": Map({
        "status":    String("draft"),
        "sponsors":  List([String("Foo"), String("Bar")]),
        // anything declared in `listing-item.extra` on the source doc
      }),

      // Per-item display flags computed from listing.fields:
      "show": Map({
        "title": Bool(true),
        "date":  Bool(true),
        // … one boolean per field that built-ins consult
      }),
    }),
    // … one map per resolved item, in sort order
  ]),

  // Project context (where useful):
  "project": Map({
    "site-url":  String("https://example.com"),  // or empty
    "title":     String("My Site"),
  }),
})
```

**Discipline note.** This binding is a **public contract for
custom listing templates** (L8). Adding a key is non-breaking;
removing or renaming a key is breaking and must be called out
in commits + the L11 close-out report. Pre-rendered helper
strings (`image-html`, `metadata-attrs`, `category-html`) are
the listings substitute for Q1's `listing.utilities.*`
function calls; they exist *because* doctemplate has no
function-call surface.

## `template:` field

Canonical extension: `.template`.

- `template: my-listing.template` → resolved relative to the
  host page's directory via `FileSystemResolver`. References to
  built-in partials (`item-default`, `item-grid`, etc.) chain
  through `MemoryResolver`. L3 emits a diagnostic when the
  resolved file is missing, with a source span on the YAML key.
- `template: my-listing.ejs.md` → accepted with a
  **deprecation diagnostic** pointing at the L10 migration
  documentation. The schema declares this an `anyOf` of
  `path` to keep YAML-level validation permissive; the
  diagnostic is emitted by L3 at consumer time, not by the
  schema layer.
- Other extensions → accepted (the schema is `path`); L3
  treats them as doctemplate input.

When `template:` is set, `type:` must be `custom`. If
`type:` is not specified and `template:` is, L3 infers
`type: custom`. If `type` is something other than `custom`
*and* `template:` is set, L3 emits a "template field requires
type: custom" diagnostic and falls back to the type's
built-in template.

## Where the Rust types live

L3 owns the implementation. Recommended layout (L3 may
adjust if the file sizes warrant):

```
crates/quarto-core/src/project/listing/
  mod.rs       — re-exports + module-level docs
  config.rs    — Listing, ListingContents, ListingSort, …
                 plus the ConfigValue → Listing parser
  item.rs      — ListingItem, hydration from DocumentProfile
  filter.rs    — include/exclude application
  sort.rs      — multi-key sort logic
  templates/   — embedded built-in templates (.template files)
    listing-default.template
    listing-grid.template
    listing-table.template
    item-default.template
    item-grid.template
    _filter.template       (optional)
    _pagination.template   (optional)

crates/quarto-core/src/transforms/
  listing_generate.rs  — Pass-2 generate transform (L3)
  listing_render.rs    — Pass-2 render transform (L3)
```

L3's sub-plan is the place to confirm or adjust this layout.

## Resolved decisions for L3+ (user-confirmed 2026-05-06)

These were drafted as open questions during L2's authoring and
confirmed by the user before hand-off. Recording inline so L3's
hand-off agent doesn't relitigate them.

### R1 — Resolved listing data lives at `meta.listings.<id>`

Mirrors `meta.navigation.navbar` (where the navbar generate
transform writes its resolved data). The alternative — a new
field on `RenderContext` or a side-channel on `DocumentAst`
similar to `recorded_includes` — is rejected for v1 because the
`meta` precedent is closest and Lua filters running between
generate and render need a documented hook surface (the meta
map already serves that role).

L3 may switch to a side-channel only if storing the full item
set in `ConfigValue` form proves measurably expensive or if
mid-pipeline `meta` mutation becomes a contract problem. If so,
the L3 author files a follow-up bd issue and confers with the
user before changing the storage location.

### R2 — Sort-key field-name normalization: kebab end-to-end

Q1 supports `reading-time` as a sort key (kebab); `readingTime`
would be a typo. Q2 normalizes during sort-key parse: kebab in
YAML, kebab in the field-name lookup. Unknown sort keys produce
a diagnostic with a source span on the `sort:` value.

### R3 — `include` / `exclude` predicate semantics: literal-equality v1

For v1: literal-equality on scalar fields, any-element-match
on list fields. I.e.:

- `include: [{author: "Foo"}]` matches an item whose `author`
  field equals `"Foo"` exactly.
- `include: [{categories: rust}]` matches an item whose
  `categories` list contains `"rust"`.
- Multiple keys inside one record = AND.
- Multiple records inside `include`/`exclude` = OR.

Q1's wildcard / regex predicates are deferred to a follow-up
bd issue if real-world templates need them.

### R4 — `listing: false` is rejected

Listings are page-local; there is no parent-default mechanism
to disable. `listing: false` is a typo, not an override. L3
emits a diagnostic with a source span on the `listing:` value
and renders the page without a listing (no fallback synthesis).

(Contrast with `navbar: false`, which *does* affirmatively
disable a website-level default — but website-level navbar
defaults exist and listings have no analogous parent.)

### R5 — `field-display-names`: typed as `Record<String, String>`

Q1's schema is `object` (anything goes), but only string values
have a sensible interpretation. L3 declares the field as
`BTreeMap<String, String>` and emits a diagnostic with a source
span on non-string values, dropping them from the resolved map.

These decisions are inputs to L3, not its discretion. The L3
hand-off agent should treat changes here as redesign triggers
that need user sign-off.

## Out-of-scope for v1 (deferred to follow-ups or later phases)

- **Inline-metadata `contents:` records.** Schema accepts;
  L3 emits a `Q-listing-1` diagnostic. Follow-up bd issue
  filed at L11 close-out.
- **`xml-stylesheet` for RSS feeds.** Schema accepts;
  L9 ignores. Q1 has the same shape.
- **`field-required` blocking validation.** Q1 throws on
  missing required fields; v1 emits a diagnostic and drops
  the item.
- **Pagination knobs (`page-size` runtime behavior).** The
  field is parsed and bound into the template as today; the
  *interactive* paginator is `list.min.js` territory, which
  L3 wires up but doesn't extend.
- **`field-types: { foo: minutes }` cascading from
  `reading-time-minutes`.** Q1 does this implicitly; L3
  documents the supported `ColumnType` set above and ports
  the same defaults.

## Decisions log

Recording here so reviewers don't relitigate.

- **D1 (placement of YAML schema entries):**
  `crates/pampa/test-fixtures/schemas/definitions.yml`. The
  `website-listing` and `website-listing-contents-object`
  definitions are already imported (lines 1439–1711). L2
  reviews them for completeness against this doc; minor
  drift gets fixed in L2's commit. No move to
  `quarto-yaml-validation/` until that crate gains a runtime
  consumer (out of scope for the listings epic).
- **D2 (Rust types live in `quarto-core`):** under
  `crates/quarto-core/src/project/listing/`. Not its own
  crate. Rationale: the types are read by `quarto-core`'s
  Pass-2 transforms and write nothing engine- or
  format-specific; an extra crate boundary has no payoff.
- **D3 (top-level `listing:` key):** confirmed against
  `bd-n9dr`. Listings are page-level; `bd-n9dr` is
  site-level; the two decisions are independent.
- **D4 (`.template` extension):** epic decision 1.
  `.ejs.md` accepted with a deprecation diagnostic.
- **D5 (`contents:` v1 = globs only):** inline records
  validate against the schema (Q1-compat) but L3 emits
  a "not yet supported" diagnostic per inline-object entry.
- **D6 (binding contract):** the per-item shape in §"Per-item
  template binding" is the public contract for L3's
  built-ins and L8's custom templates. Additions
  non-breaking; renames or removals breaking.
- **D7 (no Rust code lands under L2):** L2 ships only this
  document. The Rust types arrive with L3.

## Filing reminder

This sub-plan corresponds to `bd-j60g`. Once approved:

1. Update `bd-j60g`'s description with a one-line link to
   this file.
2. After hand-off / approval, close `bd-j60g` with a reason
   that says "Closed at draft approval; no Rust code; L3
   inherits."
3. `br sync --flush-only && git add .beads/ && git commit`
   from the main repo (per `.claude/rules/worktrees.md`).
4. Mark the row in the epic plan's table with **Closed**
   plus the commit hash that landed this doc.

Because L2 ships no code, the close-out is exactly the
plan-doc commit + bd close. There is no `cargo xtask verify`
gate (no Rust changes) and no test run.
