# L3 — Listing resolve transforms (sub-plan)

**Date:** 2026-05-06
**Beads:** `bd-ml8z` (this phase) and `bd-b5jm` (L4 — bundled
into the same hand-off; see §"Bundled L4 scope" below). Parent
epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:**
- L0 (`bd-n8a4`, closed) — `DocumentProfile.listing_item` and
  `categories_raw` substrate.
- L1 (`bd-izqh`, closed) — `ListingItemInfoStage` auto-fill.
- L2 (`bd-j60g`, draft) — listing data-model + schema
  reference doc:
  `claude-notes/plans/2026-05-06-listings-L2-data-model.md`.
**Status:** Draft. Awaiting user approval before hand-off.

## Goal of this phase

Land Q1-feature-parity listings on the three built-in types
(`default`, `grid`, `table`) for HTML output. Specifically:

1. Define the listing Rust types as documented in L2 (under
   `crates/quarto-core/src/project/listing/`).
2. Implement two AST transforms — `ListingGenerateTransform`
   (resolve item set) and `ListingRenderTransform` (apply
   built-in template, splice into host AST) — wired into the
   Pass-2 transform pipeline. Custom templates (`type:
   custom`) are L8's job; L3 short-circuits them with a
   diagnostic for now (see §"Custom-template handling" below).
3. Ship the L4 enhancements `quarto-doctemplate` needs to be
   the rendering engine: pipe evaluator, `ConfigValue →
   TemplateValue` bridge, project-scoped resolver flavor.
4. Embed the three built-in templates as `MemoryResolver`
   partials, ported from Q1's `item-default.ejs.md` /
   `listing-default.ejs.md` / etc. into doctemplate's
   `$var$` syntax.
5. Bundle `list.min.js` for client-side sort/filter
   interactivity, registered as a `Project`-scoped artifact
   via Phase-5's artifact store (epic decision 3).
6. Emit description-preview and preview-image placeholder
   comments in the same regex format Q1 uses, so L7's
   post-render upgrade can substitute them later. Listing
   output must be **correct without L7** — every item
   carries the L1 fallback inline; placeholders are an
   *upgrade marker*, not a *required substitution*.
7. Pass `cargo xtask verify` (full, including hub-client
   build).

L4 is bundled into this hand-off because L3 cannot render
without L4. Per the user's 2026-05-06 decision, the L3+L4
session ships them together rather than splitting into two
sessions. See §"Bundled L4 scope" for the exact L4 surface
this session must ship.

**Out of scope for L3 (deferred to later phases):**
- Categories sidebar markup → **L5** (`bd-5vsr`).
- Engine-rendered description / preview-image substitution
  → **L7** (`bd-qf7r`). L3 emits the placeholders; L7 reads
  them.
- Custom-template `template:` resolution → **L8**
  (`bd-rqgx`). L3 emits a "custom templates not yet
  supported" diagnostic when `type: custom` and
  `template:` are set together.
- RSS feeds → **L9** (`bd-o90m`).
- Inline-metadata `contents:` records (`{ title:..., path:...
  }`) → **deferred follow-up**. L3 emits a diagnostic per
  inline entry and processes the glob entries.
- Dependency-graph integration (`listing_content_targets`)
  → **L6** (`bd-xbnf`). L3's generate transform expands
  globs at render time; L6 expands at dep-graph build time.
  These are independent computations that share the glob
  expander.

## Reference material

Read first:

- Parent epic: `claude-notes/plans/2026-05-05-listings-epic.md`
  §"L3" + §"L4" + §"L7" (for the placeholder contract) +
  §"Architecture summary".
- L2 reference doc (the data model L3 implements):
  `claude-notes/plans/2026-05-06-listings-L2-data-model.md`.
- Design rationale:
  `claude-notes/plans/2026-05-05-listings-design-discussion.md`
  §"Custom listings via `quarto-doctemplate` — feasibility
  study" (the doctemplate analysis L3 inherits) + §"How that
  maps onto Q2's existing machinery" (item discovery /
  ProjectIndex usage).
- L1 sub-plan (predecessor; explains why
  `profile.listing_item` is always populated by the time L3
  reads it):
  `claude-notes/plans/2026-05-05-listings-L1-autofill-stage.md`.
- Existing Q2 generate / render precedent (the pattern L3
  follows):
  - `crates/quarto-core/src/transforms/navbar_generate.rs`
    (line 64's `transform` body is the closest analog —
    reads `ast.meta.navbar`, resolves with
    `ProjectIndex`, writes back to `ast.meta`).
  - `crates/quarto-core/src/transforms/navbar_render.rs`
    (the consumer of the generated data).
  - `crates/quarto-core/src/transforms/sidebar_generate.rs`
    + `sidebar_render.rs` (similar shape, more complex
    because it has cross-doc co-membership).
- doctemplate internals:
  - `crates/quarto-doctemplate/src/evaluator.rs` lines 209
    + 337 — the two `// TODO: Apply pipes` sites L4 must
    fill.
  - `crates/quarto-doctemplate/src/context.rs` —
    `TemplateValue` enum that L4's bridge produces.
  - `crates/quarto-doctemplate/src/resolver.rs` —
    `PartialResolver`, `MemoryResolver`,
    `FileSystemResolver`, `ChainedResolver`. L4 needs a
    project-scoped wiring; the chain primitives are
    already there.
- Project glob expander (the existing one to reuse or
  parallel):
  `crates/quarto-core/src/project/discovery.rs` lines
  148–253 (`expand_patterns`, `glob_match`,
  `wildcard_match`, `segment_match`).
- ProjectIndex (where item profiles come from):
  `crates/quarto-core/src/project/index.rs`. The
  `lookup_by_source(path)` method is L3's primary entry
  point.
- Phase-5 artifact store (where `list.min.js` registers):
  `crates/quarto-core/src/project/` — the artifact-emission
  hooks established in Phase 5. L3's sub-plan author should
  grep for `flush_site_libs` or `register_artifact` to find
  the exact API surface; the surface has stabilized by
  Phase 9.
- Q1 EJS templates (the rendering shape L3's built-ins
  reproduce in `$var$` syntax):
  `external-sources/quarto-cli/src/resources/projects/website/listing/`
  — start with `item-default.ejs.md` (124 lines) and
  `listing-default.ejs.md` (7 lines).
- Q1 `templateMarkdownHandler` (the rendering pipeline
  L3 ports the *shape* of, but with doctemplate as the
  engine):
  `external-sources/quarto-cli/src/project/types/website/listing/website-listing-template.ts`
  starting at line 58.

## Settled inputs

These are decisions, not open questions:

- **Generate / render decomposition** — two transforms
  (user-confirmed 2026-05-06; matches navbar / sidebar
  precedent).
- **Built-ins ship as embedded `MemoryResolver` partials** —
  authors can read them as the canonical reference.
  Embedded via `include_str!` of files under
  `crates/quarto-core/src/project/listing/templates/`.
- **`list.min.js` + `quarto-listing.js` + `quarto-listing.scss`
  bundled in v1** (user-confirmed 2026-05-06). All three
  Q1 client-side assets route through Phase-5's artifact
  store as `Project`-scoped artifacts. The SCSS is the
  riskier of the three — Q1's lives inside a Sass include
  chain that reaches into Bootstrap variables. **Spike
  ordering during impl:** ship the JS pair first (clean
  static-vendor case), then attempt the SCSS. If the SCSS
  needs Bootstrap-variable wiring that forces
  cross-cutting churn, defer just the SCSS to a follow-up
  bd issue and ship L3 with unstyled-but-functional
  listings; markup and JS still match Q1, the user just
  gets default browser styling until the SCSS lands.
  See §"Vendored client-side assets" below.
- **Custom templates (L8) deferred** — L3 emits a
  diagnostic when `type: custom` is set and falls back to
  the `default` built-in. The diagnostic links to
  `bd-rqgx`.
- **Per-item template binding contract** — L2 §"Per-item
  template binding". L3 builds exactly that
  `TemplateValue::Map`.
- **Placeholder format** — Q1's regex format, verbatim:
  - Description: `<!-- desc(5A0113B34292)[max=<n>]:<rel-path> -->`
  - Image: `<!-- img(9CEB782EFEE6)[<attrs>]:<id>:<rel-path> -->`
  Where `<rel-path>` is the **rendered** sibling output
  href (e.g. `posts/foo.html`), so L7 can read the file
  directly.
- **`contents:` glob expansion via `ProjectIndex.profiles()`**
  (user-confirmed 2026-05-06). L3 does **not** walk the
  filesystem with a host-relative glob expander. Instead,
  the resolved item set is computed by filtering
  `ctx.project_index.profiles()` against the listing's
  glob patterns (relative to the host page's directory).
  See §"Generate transform: item discovery" below for the
  exact rule. Side-benefits: works identically on native
  and WASM (no second filesystem walk in the WASM
  Automerge VFS), naturally excludes non-`.qmd` files
  Q2 doesn't know about, and naturally excludes the host
  page itself.
- **Re-parse diagnostics surface as a single host-page
  warning** (user-confirmed 2026-05-06). The L3 render
  transform parses the doctemplate output via
  `pampa::readers::qmd::read(...)` with a fresh
  `SourceContext`. Diagnostics from that re-parse are
  *not* threaded back to the host page's `SourceContext`
  in v1; instead, if the re-parse produces any
  diagnostics, L3 emits one warning on the host page's
  diagnostic stream naming the listing id and the count
  + first message. Full source-info threading is a
  follow-up (filed alongside L3's hand-off; see
  §"Filing reminder"). A user who needs precise
  diagnostics for a built-in template's output should
  open the embedded `.template` file under
  `crates/quarto-core/src/project/listing/templates/`.
- **Diagnostic codes use the new `Q-12-N` major
  ("listing" subsystem)** (user-confirmed 2026-05-06).
  Catalog entries land in
  `crates/quarto-error-reporting/error_catalog.json` as
  part of L3. v1 codes:
  - `Q-12-1` — custom listing template not yet supported
    (fallback to `default`; mentions `bd-rqgx`).
  - `Q-12-2` — inline-record `contents:` entry not yet
    supported (skip and process glob entries).
  - `Q-12-3` — unknown sort field on `sort:`.
  - `Q-12-4` — listing-id collision (first-wins).
  - `Q-12-5` — `field-display-names` non-string value
    (drop).
  - `Q-12-6` — `listing: false` rejected.
  - `Q-12-7` — `template:` set with non-`custom` `type:`.
  - `Q-12-8` — `template:` references missing file.
  - `Q-12-9` — `.ejs.md` template deprecated; pointer to
    L10 migration doc.
  - `Q-12-10` — re-parsed listing produced N diagnostics
    (the umbrella warning above).
- **Filter (`include`/`exclude`) lookup falls through
  curated → `extra`** (user-confirmed 2026-05-06). For a
  filter key K on an item I:
  1. Look up I's curated `ListingItem` field named K
     (e.g. `author`, `categories`).
  2. If absent, look up `I.extra.K` (Q1's flat shape).
  3. If absent, the predicate fails (no match).
  This preserves Q1 parity for blogs that filter on
  custom fields like `status: published` and gives L8
  custom templates clean ergonomics. The L2 binding
  contract already exposes `extra` keys at the item-map
  root; this rule mirrors that surface in the filter
  matcher.

## Architecture

### Where the transforms run

```
Pass 2 (per file, with ProjectIndex):
  PreEngineSugaring → Engine → ThemeCSS → UserFilters(pre) →
  AstTransformsStage:
    ┌─ existing transforms (callout-resolve, theorem,
    │  float-ref-target, navbar-generate, sidebar-generate,
    │  …)
    ├─ ListingGenerateTransform   ← new
    ├─ ListingRenderTransform     ← new
    └─ existing render-side transforms (navbar-render,
       sidebar-render, page-nav-render, footer-render, …)
  → UserFilters(post) → CodeHighlight → RenderBody → ApplyTemplate
```

Order rationale:

- **Generate before Render** — same as navbar / sidebar.
- **After other generate transforms** — so a Lua filter
  (running between generate and render) sees the generated
  navbar / sidebar / listings together if it wants to.
- **Before render-side transforms** — so render-side
  consumers (`navbar-render`, etc.) don't try to render
  listing data into their own slots. Listings have their
  own slot; the order is "all generate, then all render."

The exact slot to add the two transforms in
`build_transform_pipeline` is between `sidebar_generate` and
`navbar_render` (or whatever the next `*_render` is). The L3
session author confirms the existing order in `pipeline.rs`
and inserts accordingly.

### What each transform does

The split is the same as navbar / sidebar: **generate
collects semantic data, render emits output**. The
template engine (doctemplate) only ever runs inside the
render transform — the generate transform never sees a
template.

**ListingGenerateTransform** (no template invocation).

1. Read the host page's `listing:` config from `ast.meta`.
   Skip with no-op when:
   - `listing:` is absent, or
   - `listing: false` (per L2 §"Open question 4" — recommend
     reject; sub-plan can revisit), or
   - `meta.listings` is already populated (Lua-filter
     override hook).
2. Parse the `ConfigValue` shape into one or more `Listing`
   structs (using the L2-documented hydration rules).
   Schema-shape diagnostics surface here.
3. For each `Listing`:
   - Resolve `contents:` globs against the project's input
     paths via the shared glob expander (see §"Glob
     expansion" below). Inline-record entries get a
     "not yet supported" diagnostic.
   - For each matched path, look up the `DocumentProfile`
     via `ctx.project_index.lookup_by_source(path)`. Skip
     paths with no profile (with a debug-level note;
     this generally means a non-`.qmd` matched a glob).
   - Build a `ListingItem` per the L2 hydration rules.
   - Apply `include` / `exclude` filters.
   - Apply `sort` (multi-key, stable sort).
   - Truncate to `max-items`.
   - Hydrate type-specific defaults (`fields`, `page-size`,
     etc.).
4. Store the resolved listing under
   `meta.listings.<listing.id>` as a `ConfigValue::Mapping`.
   The shape is the per-listing context from L2 §"Per-item
   template binding" — the *generate* transform produces
   exactly the data the render transform will hand to
   doctemplate. Lua filters running between generate and
   render see this map and can mutate it.

**ListingRenderTransform** (this is where doctemplate runs).

1. Walk `meta.listings.<id>` for each listing. Skip if
   already rendered (the rendered marker is an attribute on
   the slot Div; see step 3).
2. Convert the `ConfigValue::Mapping` to a
   `TemplateValue::Map` via L4's bridge.
3. Pre-render helper strings server-side (`image-html`,
   `metadata-attrs`, `category-html`) and add them to each
   item's map. These are the substitutes for Q1's
   `listing.utilities.*` calls (which doctemplate cannot
   make because it has no function-call surface).
4. Resolve the listing's template:
   - `type: default` → `MemoryResolver` partial
     `listing-default`.
   - `type: grid` → `listing-grid`.
   - `type: table` → `listing-table`.
   - `type: custom` → diagnostic + fall back to `default`
     (L8 territory).
5. Apply doctemplate via `Template::compile_with_resolver`
   then `template.render(&ctx)`. Get back a markdown
   string.
6. Re-parse the markdown via the markdown re-parse pipeline
   the host page already has access to (the same
   pampa-internal reparse Q1 uses for its
   `templateMarkdownHandler` shape — see §"Markdown
   re-parse strategy" for the exact API call to use). The
   result is a `Vec<Block>`.
7. Splice the resulting blocks into the host page's AST:
   - **If the host has an explicit slot** — a `Div` with
     `id == "<listing.id>"` somewhere in `ast.blocks` —
     replace the slot's contents with the rendered blocks.
     Mark the Div with a `data-listing-rendered="1"`
     attribute so re-runs are idempotent.
   - **Otherwise** — append a new `Div` with that id and
     class `quarto-listing` to the end of `ast.blocks`,
     containing the rendered blocks.
8. Emit `<!-- desc(...) -->` and `<!-- img(...) -->`
   placeholders inside the rendered blocks per the
   L1-fallback-and-placeholder rule (see §"Placeholder
   emission" below).

### Module layout

Per L2 §"Where the Rust types live", with L3-specific
additions:

```
crates/quarto-core/src/project/listing/
  mod.rs                — re-exports + module-level docs
  config.rs             — Listing, ListingContents, ListingSort, …
                          plus the ConfigValue → Listing parser
  item.rs               — ListingItem, hydration from DocumentProfile
  filter.rs             — include/exclude application
  sort.rs               — multi-key sort logic
  binding.rs            — build the per-listing TemplateValue::Map
  helpers.rs            — pre-rendered helper strings
                          (image_html, metadata_attrs, category_html)
  templates/            — embedded doctemplate sources
    listing-default.template
    listing-grid.template
    listing-table.template
    item-default.template
    item-grid.template
    _filter.template      (optional v1; revisit during impl)
    _pagination.template  (optional v1; revisit during impl)

crates/quarto-core/src/transforms/
  listing_generate.rs   — Pass-2 generate transform
  listing_render.rs     — Pass-2 render transform

crates/quarto-doctemplate/src/
  evaluator.rs          — L4: implement pipes
  pipes.rs              — L4: new module with
                          escape / escape_xml / date_format /
                          first / rest implementations
  context.rs            — L4: TemplateValue helpers if needed
                          for richer ConfigValue conversion

crates/quarto-core/src/template_bridge.rs (or similar; L3 names)
                        — L4: ConfigValue → TemplateValue bridge.
                          Lives in quarto-core because
                          quarto-doctemplate doesn't depend on
                          quarto-pandoc-types.
```

L3 may rename or coalesce these as the implementation
progresses — the test suite is the authority.

## Bundled L4 scope

L3 must ship the following L4 deliverables in the same
session. None of them are listings-specific; they are pure
templating-engine improvements that L3 happens to be the
first consumer of.

### L4.1 — Pipe evaluator

**Revised 2026-05-06 after impl-time discovery.** The
tree-sitter grammar
(`crates/tree-sitter-doctemplate/grammar/grammar.js` lines
56–73) only accepts a fixed enumerated set of pipe names:
`pairs`, `first`, `last`, `rest`, `allbutlast`,
`uppercase`, `lowercase`, `length`, `reverse`, `chomp`,
`nowrap`, `alpha`, `roman`, plus `left`/`center`/`right`
(with width + border args). Adding new names like
`escape` / `escape_xml` / `date_format` would require
grammar surgery + tree-sitter rebuild.

**Decision:** L4.1 wires the *existing grammar pipe set*
into the evaluator. No grammar changes. The built-in
listing templates do not need `escape` / `escape_xml` /
`date_format` because:

- **Dates** are pre-formatted server-side in
  `binding.rs` per `listing.date-format` (matches the
  existing `image_html` / `metadata_attrs` pattern). The
  template just splices the formatted string.
- **HTML escaping** is handled implicitly: doctemplate
  output is markdown, and the markdown re-parse +
  HTML writer chain escape special characters in title /
  description text per the writer's normal contract.
- **XML escaping for RSS** is L9's concern. L9 will
  either pre-render strings server-side using the same
  pattern, or file a follow-up for grammar additions.

Two TODOs to wire:

- `evaluator.rs:209` (`// TODO: Apply pipes`) —
  variable-pipe path.
- `evaluator.rs:337` (`// TODO: Apply pipes to partial
  output`) — partial-pipe path.

**Pipe semantics.** Each pipe is a function
`fn(value: TemplateValue, args: &[PipeArg]) ->
TemplateValue`. Invariants:

- Apply pipes left-to-right (so
  `$x/uppercase/length$` first uppercases then takes
  the length).
- Unknown pipe name emits `Q-10-6` diagnostic
  (already in catalog) and the value passes through
  unchanged.
- Wrong arg count / type emits `Q-10-7` (already in
  catalog) and the value passes through unchanged.

**Pipe table** (existing grammar names; semantics ported
from Pandoc's doctemplates library):

| Pipe         | Args        | Behavior                                                      |
|--------------|-------------|---------------------------------------------------------------|
| `pairs`      | —           | Map → list of `[key, value]` pairs.                            |
| `first`      | —           | List → first element; String → first char; other → unchanged. |
| `last`       | —           | List → last element; String → last char; other → unchanged.   |
| `rest`       | —           | List → all but first; String → all but first char.            |
| `allbutlast` | —           | List → all but last; String → all but last char.              |
| `length`     | —           | List → element count; String → char count; Map → entry count. |
| `uppercase`  | —           | String → uppercased.                                           |
| `lowercase`  | —           | String → lowercased.                                           |
| `reverse`    | —           | List → reversed; String → reversed.                            |
| `chomp`      | —           | String → trailing newline removed.                            |
| `nowrap`     | —           | Returns the value unchanged in v1. Pandoc's nowrap controls fill-mode in plain text output; for our use case (markdown output) it's a no-op. |
| `alpha`      | —           | Integer-string → letter form (1→a, 2→b, …, 26→z, 27→aa).       |
| `roman`      | —           | Integer-string → lowercase Roman numeral.                      |
| `left`       | width [pad] | Pad-right string to width chars (no-op if longer).             |
| `center`     | width [pad] | Center string within width.                                    |
| `right`      | width [pad] | Pad-left string to width chars.                                |

Tests:
- Each pipe in isolation (input → output table).
- Pipe + variable composition (`$xs/length$`).
- Pipe chaining (`$xs/first/uppercase$`).
- Pipe + partial composition.
- `left 20` argument parsing.
- Unknown pipe → `Q-10-6` diagnostic.

**Follow-up bd issue (file during impl if needed):** if
L9 / future custom templates demand `escape` /
`escape_xml` / `date_format` as pipes, file a bd for
grammar + evaluator extension. Today they're not blocking.

### L4.2 — `ConfigValue → TemplateValue` bridge

**Revised 2026-05-06 after impl-time discovery.** Pampa
already has this bridge at
`crates/pampa/src/template/config_merge.rs:64`:
`config_to_template_value(config: &ConfigValue, ctx:
&mut ConfigConversionContext) -> TemplateValue`. It
handles all `ConfigValueKind` variants including
`PandocInlines` / `PandocBlocks` (rendered through a
`MetaWriter`) and is more sophisticated than the bridge
L4.2 was originally going to add.

**Decision:** L3's render transform calls
`pampa::template::config_merge::config_to_template_value`
directly, with `ConfigConversionContext::new(MetaWriter::Html)`.
No new bridge code in `quarto-core`. The L4.2 task
collapses to "verify the existing bridge handles the
listing-binding shape" via the integration test on
the per-item map.

Tests added (in `crates/quarto-core/src/project/listing/`):
- Listing-binding fixture: build a representative
  per-item map (curated fields + nested `extra` map +
  pre-rendered helper strings), pass it through
  `config_to_template_value`, assert the resulting
  `TemplateValue::Map` has the expected keys at the
  expected nesting.

### L4.3 — Project-scoped resolver

Add a small constructor or helper in
`quarto-doctemplate/src/resolver.rs` (or in
`quarto-core` if the helper needs project-paths it doesn't
have):

```rust
/// Resolver chain for project-scoped template lookup:
///   1. FileSystemResolver rooted at the host-page directory
///      (for custom templates, L8).
///   2. MemoryResolver carrying the listings built-ins.
///   3. NullResolver fallthrough.
pub fn project_listing_resolver(
    host_page_dir: &Path,
    builtins: MemoryResolver,
) -> impl PartialResolver;
```

L3 uses this in the render transform to compile and apply
templates. Tests:
- Built-in lookup with no host-page directory falls back
  to `MemoryResolver`.
- Custom-template path (L8 territory) shadows a built-in
  with the same name when present.
- Missing partial → `TemplateError::PartialNotFound` with
  source span.

## Generate transform: item discovery (step 3a)

A sub-topic of `ListingGenerateTransform` step 3 — resolving
each `Listing.contents` glob into a list of project paths
that get profile-looked-up and turned into `ListingItem`s.

**Rule (decided 2026-05-06):** L3 does **not** walk the
filesystem. Instead, item discovery operates entirely on
`ctx.project_index.profiles()` — the already-enumerated set
of project documents. This works identically on native and
WASM (no second walk through the Automerge VFS) and
naturally restricts matches to files Q2 actually parses.

For each `Listing`:

1. Take `ctx.project_index.profiles()` as the candidate set
   (a slice of `&DocumentProfile`).
2. Compute each candidate's path relative to the host
   page's directory. Profiles outside the host's directory
   subtree are **not** automatically excluded — Q1's
   default `*.qmd` is host-dir-relative, but explicit globs
   like `posts/**/*.qmd` are project-relative. The match
   tries both forms (see step 3).
3. For each `ListingContents::Glob(pattern)` on the
   listing:
   - If the pattern is host-dir-relative (default `*.qmd`,
     or any pattern that matches a candidate when computed
     against `host_dir`), include candidates whose
     `host_dir`-relative path matches.
   - Else if the pattern is project-relative, include
     candidates whose project-relative path matches.
   The matcher reuses `glob_match` /
   `wildcard_match` / `segment_match` from
   `crates/quarto-core/src/project/discovery.rs`. **No new
   walker, no new wrapper function.** Refactor those
   helpers from `pub(crate)` to `pub(super)` or expose
   them via a small `glob` sub-module if needed.
4. The host page's own source path is excluded from the
   result set (matches Q1's default).
5. Inline-record entries (`ListingContents::Inline`) emit
   `Q-12-2` and are skipped in v1.

After step 3, the matched profiles get hydrated into
`ListingItem`s per L2's hydration rules.

**Trade-off accepted.** A pure-glob expander would catch
`.qmd` files Q2 hasn't parsed yet (e.g. files excluded
from the project's render set, or inline-included). The
ProjectIndex-filter approach silently drops them. Per the
2026-05-06 discussion, this is acceptable: any file
absent from `ProjectIndex` is also absent from
`output_href` resolution and would have rendered as a
broken link anyway. Edge cases where the user wants to
list a file that isn't part of the project's render set
get a follow-up bd issue (filed at L3 close-out if
real-world templates need it).

## Render transform: turning doctemplate output into AST blocks (step 6)

A sub-topic of `ListingRenderTransform`. The render
transform's body is one sequence — read `meta.listings.<id>`,
build the template binding, apply doctemplate, splice the
result into the host AST — and **all of it happens inside
the render transform**. The template is never invoked
elsewhere; the generate transform stores semantic data and
nothing more.

This section is specifically about the conversion step in
that sequence (step 6 of the render-transform body):
doctemplate emits a markdown string in step 5, the render
transform converts it to `Vec<Block>` here in step 6, and
step 7 splices those blocks into the host AST.

Why a markdown string in the middle, instead of building
AST nodes directly? Two reasons, settled in
`2026-05-05-listings-design-discussion.md` §"Custom listings
via quarto-doctemplate":

- doctemplate is a **text** template engine. Its output is
  a string. Producing AST directly would require either a
  different rendering engine or shelling the built-ins out
  to Rust code — both lose the property "the built-ins are
  the same surface custom templates use", which was a
  load-bearing C5 argument.
- Custom-template authors (L8) need to write things like
  `### [$it.title$]($it.path$)` and have the markdown
  render as the right AST. That requires the round-trip.

If you want to revisit the round-trip itself, that's a
higher-up design discussion than L3.

**Conversion surface: `pampa::readers::qmd::read(...)`**
(verified 2026-05-06 via grep). The function signature
returns `(Pandoc, SourceContext, Vec<Diagnostic>)`. L3
extracts `Pandoc.blocks` for the splice and consumes the
two side outputs as follows:

- The fresh `SourceContext` is **discarded**. Q2 has no
  facility today to merge a fresh `SourceContext` into
  the host page's; threading is filed as a follow-up
  (see §"Filing reminder" — bd issue listed there).
- The `Vec<Diagnostic>` is **collapsed into a single
  warning** (`Q-12-10`) emitted on the host page's
  diagnostic stream. Format: *"Re-parsing rendered
  listing `<id>` produced N diagnostic(s); first: `<msg>`.
  Inspect the rendered markdown by running with `-v` to
  diagnose."* This trades source-precision for the
  ability to ship L3 in one session; the proper threading
  design is the bd-issue follow-up.

The re-parse is per-listing-render, not per-item, so the
cost is bounded.

The re-parsed markdown is **not** subjected to user
filters; it's already in the post-filter, post-engine slot
in the pipeline. It IS subjected to the transforms that
follow `ListingRenderTransform` in the pipeline order
(`navbar_render`, `sidebar_render`, etc.) but those
transforms ignore the listing slot's contents, so this is
benign.

**Note on the meta-mutation hook (L2 D1 / L3 D2).** L3
stores the resolved listing data at `meta.listings.<id>`
between the generate and render transforms. The L2 plan
described this as a Lua-filter mutation hook; in fact
**there is no Lua filter slot between generate and render
transforms today** — `UserFiltersStage::pre()` runs before
`AstTransformsStage` and `::post()` runs after. The
storage location is therefore correct as forward-
compatibility but is *not* a load-bearing mutation hook
in v1. The proper "between generate and render" filter
slot is a separate follow-up bd issue (see §"Filing
reminder"). L3 leaves a `// TODO(bd-XXXX): no Lua hook
between generate and render today` comment at three
locations: this transform site, `navbar_generate.rs`
(the precedent with the same latent assumption), and
`pipeline.rs`'s navigation-phase comment block.

## Placeholder emission and the L1 fallback contract

Per the epic plan's L7 section (§"Bracketing rules") and
the L1 sub-plan's §"Safeguard contract," **every listing
item must render correctly without L7 running.** L3 honors
this by:

1. **Always emitting the L1 fallback inline.** The item's
   `description` field — already auto-filled by L1 to the
   first plain-text paragraph of the post-include AST —
   appears literally in the rendered markup. Same for
   `image`.
2. **Emitting the placeholder comment as a sibling marker.**
   Q1's format:
   ```html
   <!-- desc(5A0113B34292)[max=175]:posts/foo.html -->
   ```
   This comment sits *next to* the L1 fallback, not
   replacing it. L7's regex finds it; L7 reads
   `posts/foo.html`; if the sibling has a richer
   `firstPara` than the L1 fallback, L7 substitutes; the
   placeholder comment is consumed in the substitution.
3. **L7 is an upgrade, not a substitution.** When L7 runs,
   it replaces the placeholder *and* the L1 fallback with
   the engine-rendered text. When L7 doesn't run (hub-client,
   `quarto preview`), the placeholder comment remains in
   the HTML (harmlessly invisible); the L1 fallback
   displays.

Concretely, L3's `item-default.template` writes (in
doctemplate `$var$` syntax):

```
$if(item.show.description)$
::: {.delink .listing-description}
$item.description$
<!-- desc($listing.id$)[max=$listing.max-description-length$]:$item.outputHref$ -->
:::
$endif$
```

When the listing item is for `posts/foo.qmd` with
`outputHref: posts/foo.html`, the placeholder comment
carries the *output* href (not the source path). L7 reads
that file directly.

The `5A0113B34292` and `9CEB782EFEE6` magic strings in
Q1's regex are stable hex tokens. L3 uses the same
constants so L7's port of Q1's regex matches without
modification. Suggested location:
`crates/quarto-core/src/project/listing/placeholders.rs`
defining `DESC_TOKEN` and `IMG_TOKEN` constants used by
the templates (via `MemoryResolver` substitution at
template-compile time) and by L7's regex.

## Vendored client-side assets (Phase 5 artifact store)

L3 vendors three Q1 client-side assets that the built-in
templates' markup depends on, all routed through Phase
5's `Project`-scoped artifact store and emitted into
`_site/site_libs/listing/` (or the WASM equivalent under
the resolver's VFS root):

| Asset                  | Q1 path                                                                    | Role                                                                                  |
|------------------------|----------------------------------------------------------------------------|---------------------------------------------------------------------------------------|
| `list.min.js`          | `src/resources/projects/website/listing/list.min.js`                       | Third-party (~25KB, MIT). Backs the sort/filter UI markup the templates emit.        |
| `quarto-listing.js`    | `src/resources/projects/website/listing/quarto-listing.js`                 | Q1-owned glue. Provides `window.quartoListingCategory(...)` (clicked from category items). Without it, category clicks are dead JS references. |
| `quarto-listing.scss`  | `src/resources/projects/website/listing/quarto-listing.scss`               | Layout styles for `.quarto-listing`, `.quarto-post`, the grid layout, the table view. |

Per CLAUDE.md §"External Sources Policy", these are
**copied** to a Q2-owned location (`resources/listing/`)
rather than referenced from `external-sources/`. The copy
step is part of L3 impl; subsequent Q1 updates to these
files would re-trigger the copy.

Registration uses the existing `ArtifactStore` API
(`crates/quarto-core/src/artifact.rs`):
`ArtifactStore::insert(key, Artifact { ...
scope: ArtifactScope::Project, ... })`. The render
transform calls this once per project render (idempotent —
re-inserting the same key with the same content is a
no-op per the merge contract). Built-in templates emit
`<link>` / `<script>` references that resolve through
the existing `ResourceResolverContext` —
`page_url_for_site_root_dir() + "site_libs/listing/<file>"` —
so depth-N pages get the correct relative path.

**Spike ordering during impl.** The two JS files are
clean static-vendor cases: byte-for-byte copy, register,
emit `<script>`. The SCSS is the riskier one — Q1's
file `@use`s Bootstrap variables. Two ordering options
during the L3 session:

1. Ship the JS pair first, verify end-to-end. *Then*
   attempt the SCSS.
2. If the SCSS needs Bootstrap-variable wiring that
   forces cross-cutting churn into the theme-CSS
   compilation pipeline, **defer just the SCSS** to a
   follow-up bd issue (filed during L3 impl, not now).
   Ship L3 with unstyled-but-functional listings; the
   markup and JS still match Q1, and the user gets
   default browser styling until the SCSS lands. This is
   the lowest-risk fallback.
3. If `list.min.js` artifact-store wiring itself proves
   invasive (epic decision 3 fallback), defer all three
   assets to a follow-up bd issue and ship L3 with
   markup-only listings.

The L3 session author makes the call after a half-day
spike on the artifact-store integration. **Acceptance for
v1:** at minimum, markup parity with Q1 + the two JS
files registered. SCSS is preferred but not blocking.

## Custom-template handling (L8 deferral)

When the L3 generate transform parses a `Listing` with
`type: custom` *or* a non-empty `template:` field, it:

1. Records a `Q-listing-2` "custom listing templates not
   yet supported" diagnostic with a source span on the
   YAML key.
2. The diagnostic message includes:
   "Custom listing templates land in a follow-up
   (`bd-rqgx`). For now, this listing falls back to the
   `default` built-in. Set `type: default | grid | table`
   to silence this diagnostic."
3. Internally rewrites the listing's `type` to `Default`
   for the rest of the render. The fallback uses the
   default field set, default `image-align`, etc.

This keeps Q1-source projects renderable through L3 —
they get a diagnostic plus a degraded but correct
listing — and gives L8 a clean slot to plug into.

## Pipeline-builder wiring

Two builders need updating:

- `build_html_pipeline_stages_with_apply_config` (native
  CLI path).
- `build_wasm_html_pipeline` (hub-client / WASM path).

In both, insert `ListingGenerateTransform` and
`ListingRenderTransform` into the `AstTransformsStage`'s
transform list, between the existing generate transforms
and the existing render transforms. The exact slot is in
`build_transform_pipeline` /
`build_transform_pipeline_with_…` — confirm names during
impl.

The WASM path is critical: hub-client must render
listings end-to-end (without L7's upgrade — just the L1
fallbacks). If the bridge or the resolver pulls in
WASM-incompatible deps, the hub-client build breaks.
**`cargo xtask verify` (full) is mandatory** before
declaring this task done.

## Tests (TDD)

Per CLAUDE.md: write tests, watch fail, implement, watch
pass.

### Unit tests

In `crates/quarto-core/src/project/listing/`:

1. **`config_parses_minimal`** — `listing: default`
   parses to a `Listing` with `id` synthesized,
   `type: Default`, defaults applied.
2. **`config_parses_explicit_id_and_type`** —
   `listing: { id: foo, type: grid, contents: ["*.qmd"] }`
   round-trips.
3. **`config_parses_multi_listing`** — array of two
   listings produces `Vec<Listing>` with two entries.
4. **`config_parses_listing_true_shorthand`** —
   `listing: true` synthesizes a default listing.
5. **`config_rejects_listing_false`** —
   `listing: false` produces a diagnostic per L2 D-rec,
   not a no-op (per the listing being page-local).
6. **`contents_glob_string_parses`** —
   `contents: ["*.qmd"]` produces one
   `ListingContents::Glob`.
7. **`contents_inline_record_emits_diagnostic`** —
   `contents: [{title: foo}]` parses but L3 records a
   "not yet supported" diagnostic.
8. **`sort_parses_field_only`** — `sort: ["date"]` →
   `ListingSort { field: "date", direction: Asc }`.
9. **`sort_parses_field_with_direction`** —
   `sort: ["date desc"]` → `Direction::Desc`.
10. **`sort_parses_multi_key`** — two-key sort preserves
    declared order.
11. **`include_filter_matches_string_field`** —
    `include: [{author: "Foo"}]` matches an item with
    `author == "Foo"`.
12. **`include_filter_matches_list_field`** —
    `include: [{categories: rust}]` matches an item with
    `categories: [rust, …]`.
12b. **`include_filter_falls_through_to_extra`** (D12) —
    `include: [{status: "published"}]` matches an item
    whose `extra.status == "published"`. No matching
    item exists with a curated `status` field — the
    test verifies the fallback path triggers.
12c. **`include_filter_curated_shadows_extra`** — an item
    has both a curated `categories: [rust]` *and*
    `extra.categories: [draft]`. `include:
    [{categories: rust}]` matches via the curated field;
    `include: [{categories: draft}]` does *not* match
    (because the curated field shadows the extra and
    the curated value doesn't include `"draft"`).
13. **`max_items_truncates`** — N matches, `max-items:
    K`, K < N, output has K items in sort order.
14. **`type_specific_default_fields_applied`** — three
    sub-tests, one per built-in type, asserting the
    L2-documented default `fields` set.
15. **`hydration_falls_back_to_top_level_title`** — item
    with no `listing_item.title` uses `profile.title`.
16. **`hydration_uses_listing_item_title_override`** —
    item with `listing_item.title` overrides
    `profile.title`.
17. **`categories_merge_via_merged_config`** — author
    using `categories: [a, b]` at top level *and*
    `listing-item.categories: !prefer [c]` produces an
    item with `categories == ["c"]`. (Exercises the L0
    `categories_raw` plumbing through `MergedConfig`.)
18. **`item_extra_present_in_binding`** — item with
    `listing-item.extra.status: draft` shows up at
    `binding.items[0].extra.status` after the bridge.

### L4 unit tests

In `crates/quarto-doctemplate/src/`:

19–24. Pipe behavior per the §"L4.1 — Pipe evaluator"
table.

In `crates/quarto-core/src/template_bridge.rs` (or wherever
L4.2 lands):

25. **`bridge_scalar_string_roundtrips`**.
26. **`bridge_list_of_maps_roundtrips`**.
27. **`bridge_nested_extra_map_accessible_via_dotted_path`**.

### Transform tests

In `crates/quarto-core/src/transforms/`:

28. **`generate_skips_when_no_listing_key`** — no `meta.listing`,
    no `meta.listings.<id>`, no error.
29. **`generate_writes_resolved_listing_to_meta`** — fixture
    with a host page + 3 sibling posts produces
    `meta.listings.<id>.items` with 3 entries.
30. **`generate_filters_via_include`** — fixture exercises
    a one-key include filter end to end.
31. **`generate_sorts_via_sort_field`** — items appear in
    sort order.
32. **`generate_excludes_host_page_itself`** — the host's
    own `.qmd` does not appear in its own listing.
33. **`render_emits_div_at_listing_id_slot`** — host
    document contains an explicit `::: {#my-listing}`
    Div; render fills it.
34. **`render_appends_div_when_no_explicit_slot`** — host
    has no `#my-listing` Div; render appends one to
    `ast.blocks`.
35. **`render_idempotent_on_repeat`** — running the
    render transform twice in sequence does not
    duplicate the listing markup. (The
    `data-listing-rendered="1"` marker.)
36. **`render_falls_back_to_default_for_custom_type`** —
    `type: custom` with `template:` set produces a
    diagnostic *and* a default-style listing.
37. **`render_emits_description_placeholder`** — output
    contains `<!-- desc(<id>)[max=175]:posts/foo.html -->`.
38. **`render_emits_image_placeholder_when_first_image_present`** —
    output contains `<!-- img(<id>)[…]:0:posts/foo.html -->`
    when item has an `image` *and* L7's image-substitution
    contract requires the placeholder. **Confirm L7's
    contract during impl — Q1 emits img placeholders
    only when the image is to be discovered from rendered
    HTML, not when the author or L1 has already
    populated `image`.**

### Snapshot tests

39. **`builtin_default_renders_three_items`** — fixture
    with three posts; render produces the canonical
    output. Snapshot file under
    `crates/quarto-core/src/project/listing/snapshots/`.
40. **`builtin_grid_renders_three_items`** — same fixture,
    `type: grid`.
41. **`builtin_table_renders_three_items`** — same
    fixture, `type: table`.
42. **`hub_client_listing_render_smoke`** — WASM path:
    pipeline runs end-to-end on a fixture with
    `listing: default` and produces the same listing
    markup as the native path. (Necessary because the
    L4 bridge and the WASM-restricted Lua stdlib have
    historically been a regression point.)

### Integration test

43. **`pipeline_renders_listing_end_to_end`** — fixture
    project: host page (`index.qmd`) declares
    `listing: default`, three posts in `posts/`, full
    `cargo run --bin q2 -- render` produces an
    `_site/index.html` with three listing items linking
    to the rendered posts. **End-to-end CLI verification
    per CLAUDE.md.**

### End-to-end CLI verification

Per CLAUDE.md §"End-to-end verification before declaring
success", record in this sub-plan after impl:

- Exact invocation used.
- A snippet of the observed `_site/index.html` showing
  three `<div class="quarto-post">` (or equivalent) entries
  with title-as-link, date, description.
- An explicit note that the output was inspected.

### Hub-client smoke

After Rust changes are in, before declaring done:

```bash
cd hub-client
npm run build:all
npm run dev
```

Open the dev server in a browser, load the fixture
project (or a manual one), confirm the listing host page
shows three items. Per CLAUDE.md: "Tests passing alone is
not sufficient" for hub-client changes.

## Open questions

These are non-blocking but the L3 session author should
resolve them inline rather than punt:

1. **Slot-finding semantics.** When walking `ast.blocks`
   for an explicit `::: {#my-listing}` slot, do we recurse
   into nested Divs / sections? **Recommend yes** — Q1
   recurses; users may put a listing inside a tab panel.
   But the recursion must skip already-rendered slots
   (the `data-listing-rendered="1"` marker).
2. **Multi-listing ordering when slots collide.** If two
   listings declare `id: foo`, what happens? **Recommend
   diagnostic + first-wins**.
3. **Author-supplied `id` collision with auto-synthesized.**
   What if the host has 5 listings, 3 with explicit ids
   `a, b, c` and 2 without? **Recommend synthesize
   numerically (`listing-1`, `listing-2`, …) skipping
   explicit ids; collisions still produce a diagnostic.**
4. **`field-display-names` emission in built-in
   templates.** Q1's `default`/`grid` don't honor
   `field-display-names` (only `table` does). **L3
   matches Q1.** Worth a comment in the templates so a
   future maintainer doesn't think it's an oversight.
5. **`feed:` field at the listing config level.** Parsed
   into `ListingFeedOptions`, stored on the resolved
   listing, but **L3 ignores it** — feed emission is L9.
   The data is in the binding so L9 picks it up without a
   re-parse.

## Decisions log

- **D1 (two transforms, generate + render):** confirmed by
  user 2026-05-06. Matches navbar / sidebar precedent.
- **D2 (resolved data lives at `meta.listings.<id>`):**
  recommend per L2's open question 1. The L3 session may
  switch to a side-channel if mid-pipeline `meta`
  mutation becomes a contract problem. **Caveat:** the
  "Lua filter mutation hook" framing is forward-looking
  only; today there is no Lua filter slot between
  generate and render transforms (see D13). Tracked by
  `bd-0fd0`.
- **D3 (markdown re-parse via `pampa::readers::qmd::read`):**
  user-confirmed 2026-05-06. The fresh `SourceContext`
  is discarded; re-parse diagnostics collapse into one
  `Q-12-10` warning on the host page. Full source-info
  threading tracked by `bd-0jyl`.
- **D4 (custom templates fall back to default + diagnostic):**
  L8 ships the real implementation; L3 keeps the diagnostic
  surface stable. Diagnostic code: `Q-12-1`.
- **D5 (vendored client-side assets via Phase-5 artifact
  store):** revised 2026-05-06. Three assets bundled in
  v1: `list.min.js`, `quarto-listing.js`,
  `quarto-listing.scss`. Spike-ordered: JS pair first,
  then SCSS. SCSS-only deferral acceptable if Bootstrap
  variable wiring is invasive; markup-only deferral if
  artifact-store integration itself blocks. See
  §"Vendored client-side assets".
- **D6 (L4 bundled into L3 hand-off):** confirmed by user
  2026-05-06. Single session, single PR. Pipe set
  minimum: `escape`, `escape_xml`, `date_format`. `first`
  / `rest` ship if cheap.
- **D7 (placeholder format = Q1 verbatim):** the
  `5A0113B34292` / `9CEB782EFEE6` tokens are stable
  constants used by both L3's emit and L7's regex.
- **D8 (Listing types live in `quarto-core/src/project/listing/`):**
  per L2's recommendation. Not a separate crate.
- **D9 (built-in templates ship as `include_str!`-embedded):**
  authors can read them as the canonical reference; the
  L4 resolver chain serves them via `MemoryResolver`.
- **D10 (item discovery via `ProjectIndex.profiles()`):**
  revised 2026-05-06. L3 does not walk the filesystem.
  `contents:` globs are resolved by filtering the
  enumerated project profiles against the pattern,
  trying both host-dir-relative and project-relative
  forms. Reuses the existing `glob_match` from
  `discovery.rs`; no new walker. Files outside
  `ProjectIndex` (excluded from the project's render set)
  are silently dropped — accepted trade-off; follow-up
  bd if real-world templates need them.
- **D11 (diagnostic codes use `Q-12-N` / "listing"
  subsystem):** confirmed 2026-05-06. Catalog entries
  added to
  `crates/quarto-error-reporting/error_catalog.json` as
  part of L3. v1 codes Q-12-1 through Q-12-10 listed
  under §"Settled inputs".
- **D12 (filter lookup falls through curated → `extra`):**
  confirmed 2026-05-06. Q1 parity for blogs filtering on
  free-form custom fields like `status: published`.
  Lookup order: curated `ListingItem` field first;
  fallback to `extra.<key>`; absent ⇒ no match. See
  §"Settled inputs" for rationale.
- **D13 (Lua-filter-between-generate-and-render is
  forward-looking only):** new 2026-05-06. The L2 plan's
  hint that "Lua filters running between generate and
  render see this map and can mutate it" is **not true
  today** — `UserFiltersStage::pre()` and `::post()`
  bracket `AstTransformsStage` rather than slotting into
  it. Storing resolved data on `meta.listings.<id>` is
  defensible as forward-compat but is not a load-bearing
  hook in v1. Proper boundary slot is tracked by
  `bd-0fd0`; L3 leaves `// TODO(bd-0fd0):` markers at
  the three relevant source locations
  (`pipeline.rs` navigation-phase comment block,
  `navbar_generate.rs`, and the new
  `listing_generate.rs`).
- **D14 (worktree on `feature/listings`):** confirmed
  2026-05-06. Fresh worktree at
  `.worktrees/bd-ml8z-listings-resolve-transform/`,
  branch `beads/bd-ml8z-listings-resolve-transform`,
  branched off the current `feature/listings` (so it
  inherits L0/L1). Final integration to
  `feature/listings` happens after user approval, same
  pattern as L1.
- **D15 (defer `otherFields` loop to follow-up):**
  confirmed 2026-05-06. Q1's `item-default.ejs.md`
  iterates over fields *not* in a known set and emits a
  `metadata-value` div per such field — dynamic in EJS,
  not expressible in doctemplate without server-side
  precomputation. v1 default listing renders only the
  curated field set. Tracked by `bd-0wyo` for the
  `other_metadata_html` server-pre-rendered helper.
  L3's built-in `item-default.template` carries a
  comment at the gap location pointing at `bd-0wyo`.

## Risks and mitigations

- **Risk: L4's pipe evaluator turns out to be more
  invasive than the `// TODO` comments suggest.**
  *Mitigation:* the L1 / L0 sessions both surfaced
  similar "looks small, is small" outcomes (`?Send` audit
  cleared, `insert_path` already existed). If L4 hits a
  wall, the fallback is to ship the minimum pipe set
  (`escape` + `date_format`) and pre-format every other
  field server-side. The built-ins can mostly avoid
  pipes by binding pre-formatted strings.
- **Risk: WASM build picks up `pampa::parse_to_pandoc`
  via `quarto-core` and the Lua-restricted WASM stdlib
  rejects it.** *Mitigation:* `pampa::parse_to_pandoc` is
  already used in WASM-shaped paths (the
  `wasm-qmd-parser` entry points). `cargo xtask verify`
  (full) confirms.
- **Risk: `list.min.js` artifact-store wiring forces
  cross-cutting churn.** *Mitigation:* per epic decision
  3, defer to a follow-up bd issue and ship L3 without
  sort/filter interactivity. Half-day spike during impl
  to make the call.
- **Risk: doctemplate's truthiness rules for empty
  collections don't match Q1's EJS expectations and
  built-ins behave subtly differently.** *Mitigation:*
  the snapshot tests for the three built-ins are the
  authority. Diff against a representative Q1 fixture's
  rendered HTML during impl to validate semantic parity;
  any divergence the user accepts gets recorded in the
  L3 commit message.
- **Risk: re-parse of the listing markdown produces
  unexpected blocks (e.g. wraps everything in a
  Plain).** *Mitigation:* the integration test (#43)
  asserts the rendered HTML structure is what users
  expect; if pampa's parse output differs from Q1's
  Pandoc-parse output in a way that affects the listing
  shape, file a follow-up bd issue rather than working
  around in L3.
- **Risk: the placeholder format L1 emits collides with
  legitimate user content.** *Mitigation:* Q1 has lived
  with this for years; the magic-hex tokens
  (`5A0113B34292` / `9CEB782EFEE6`) are
  unlikely-to-collide. L7's regex matches these tokens
  exactly, not a fuzzy pattern. Worst case: a user
  writes a literal `<!-- desc(5A0113B34292)... -->` in
  their `.qmd`, L7 substitutes; we accept that
  one-in-a-million failure mode.

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement,
watch pass.

### Preparation

- [ ] Re-read
      `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [ ] Re-read `.claude/rules/wasm.md` (`?Send`,
      WASM-cfg gating).
- [ ] Re-read L2 sub-plan (data model + binding
      contract).
- [x] Create a worktree under
      `.worktrees/bd-ml8z-listings-resolve-transform/`
      per `.claude/rules/worktrees.md` (branch
      `beads/bd-ml8z-listings-resolve-transform`,
      branched off `feature/listings`).
- [x] `npm install` in the worktree.
- [x] Add `.beads/redirect` per worktree rules so `br`
      uses the main repo's `.beads/`.
- [x] Baseline: `cargo xtask verify --skip-hub-build
      --skip-hub-tests` (fresh worktree has no
      wasm-quarto-hub-client built; full hub-build
      gates land in TDD phase 6). Baseline test count:
      **8448 passing**.

### Follow-up bd issues (already filed)

Three discovered-from issues filed 2026-05-06 before impl
began: `bd-0fd0` (Lua injection slot), `bd-0jyl`
(source-info threading), `bd-0wyo` (`other_metadata_html`).
See §"Filing reminder" for descriptions. During impl:

- [ ] Add `// TODO(bd-0fd0):` markers at three locations
      (`pipeline.rs` navigation-phase comment block;
      `navbar_generate.rs`; new `listing_generate.rs`).
- [ ] Add `// TODO(bd-0wyo):` marker in
      `item-default.template` at the otherFields gap.
- [ ] If a conditional follow-up triggers (D5 SCSS
      deferral / list.min.js block / profile-outside-
      ProjectIndex edge case), file the bd issue in-flight
      and back-reference here.

### TDD phase 1 — L4 pipes (bottom-up; no listings dep)

- [x] Write unit tests for the existing grammar pipe set
      (`pairs`, `first`, `last`, `rest`, `allbutlast`,
      `length`, `uppercase`, `lowercase`, `reverse`,
      `chomp`, `nowrap`, `alpha`, `roman`,
      `left`/`center`/`right`).
- [x] Implement `pipes.rs` with `apply_pipe` /
      `apply_pipes` dispatch and individual
      implementations.
- [x] Wire into `evaluator.rs`'s two `// TODO: Apply
      pipes` sites — variable path and partial-output
      path.
- [x] Fix latent parser bug: outer `pipe` rule arm was
      dropping args from `pipe_left/center/right`
      children; bare-partial sibling pipes were silently
      dropped. Both fixed in `parser.rs`
      (`extract_interpolation_parts` now collects
      sibling pipes; `pipe` arm forwards the inner
      Pipe intermediate).
- [x] All doctemplate tests pass (193, +46 new).
      Workspace tests pass: **8494, +46** from 8448
      baseline.

### TDD phase 2 — L4 ConfigValue → TemplateValue bridge

- [x] Discovery: pampa's
      `crates/pampa/src/template/config_merge.rs:64`
      already provides
      `config_to_template_value(cv: &ConfigValue,
      ctx: &mut ConfigConversionContext) ->
      TemplateValue` covering all `ConfigValueKind`
      variants including `PandocInlines/PandocBlocks`
      via `MetaWriter`. L3 calls this directly; no new
      bridge needed in `quarto-core`. Listing-binding
      round-trip is exercised in the binding-builder
      tests under L3 phase 3.

### TDD phase 2b — L4 project-scoped resolver helper

- [x] Add `project_listing_resolver(builtins:
      MemoryResolver) -> ChainedResolver<...>` in
      `quarto-doctemplate::resolver`. Re-export from
      crate root.
- [x] Tests: built-ins served when no FS match;
      filesystem partial shadows built-in.

### TDD phase 3 — Listing types (`config.rs`, `item.rs`,
`filter.rs`, `sort.rs`)

- [x] Implement `crates/quarto-core/src/project/listing/`
      module: `config.rs` (Listing + supporting enums +
      ConfigValue parser), `item.rs` (ListingItem +
      hydration from DocumentProfile), `filter.rs`
      (include/exclude with curated→extra fallback per
      D12), `sort.rs` (multi-key stable sort with
      missing-value-last semantics), `placeholders.rs`
      (Q1-verbatim hex tokens).
- [x] Q-12-N catalog entries added to
      `crates/quarto-error-reporting/error_catalog.json`
      with subsystem="listing".
- [x] Tests: 38 unit tests across the module — config
      parser shapes (10), filters incl. curated/extra
      (5), sort (5), hydration (4), placeholders (2),
      L3-D12 fallback (2 explicit). All pass.
- [x] Workspace test count: **8534, +86** from 8448
      baseline. No regressions.

### TDD phase 4 — Generate transform

- [ ] Write transform tests (#28–32). Fail.
- [ ] Implement `ListingGenerateTransform`. Tests pass.

### TDD phase 5 — Render transform

- [ ] Write transform tests (#33–38) + snapshot tests
      (#39–41). Fail.
- [ ] Embed built-in templates (port from Q1 EJS to
      doctemplate `$var$`).
- [ ] Implement `ListingRenderTransform` (binding build,
      doctemplate apply, markdown reparse, AST splice).
- [ ] Tests pass.

### TDD phase 6 — Pipeline wiring

- [ ] Insert both transforms into both pipeline builders.
- [ ] Run integration test #43 (end-to-end CLI). Iterate
      until pass.
- [ ] Run hub-client smoke test #42. Iterate until pass.

### TDD phase 7 — Vendored client-side assets

Per D5: ship `list.min.js`, `quarto-listing.js`, and
`quarto-listing.scss`. Spike-ordered to manage risk.

- [ ] Half-day spike: locate the `ArtifactStore::insert`
      call sites in existing transforms (precedent: any
      transform that emits site_libs assets — grep for
      `ArtifactScope::Project`). Gauge whether listing JS
      slots in cleanly.
- [ ] **JS pair (low risk):** copy `list.min.js` and
      `quarto-listing.js` to `resources/listing/`,
      register both as `Project`-scoped artifacts, emit
      `<script>` references from built-ins (resolved via
      `ResourceResolverContext` so depth-N pages get
      relative paths). Verify both land at
      `_site/site_libs/listing/<name>.js` and that
      `quartoListingCategory` is callable in a browser
      session.
- [ ] **SCSS (higher risk):** copy
      `quarto-listing.scss` to `resources/listing/`,
      register, emit `<link>`. If Q1's
      Bootstrap-variable `@use` chain forces invasive
      wiring into the theme-CSS compilation pipeline:
      file the conditional bd issue from §"Filing
      reminder" item 5; document the deferral in this
      sub-plan's "Risks" section; ship L3 without
      styling. Markup + JS are unchanged; default
      browser styling renders the listing as a stack of
      anchors. Listing functionality is preserved.
- [ ] **All-three deferral (highest risk).** If
      artifact-store integration itself blocks for an
      unrelated reason, defer all three to a follow-up
      and ship markup-only listings. Same conditional
      bd-issue file; same `<script>` / `<link>`
      references made conditional on the artifact's
      presence in the store.

### Verification and close-out

- [ ] `cargo build --workspace` clean.
- [ ] `cargo nextest run --workspace` — all pass; record
      test-count delta.
- [ ] `cargo xtask lint` clean.
- [ ] `cargo xtask verify` (full, including hub-client +
      WASM build) — all green.
- [ ] End-to-end CLI verification fixture rendered;
      output inspected; recorded inline below the
      §"End-to-end CLI verification" stub.
- [ ] Hub-client browser smoke recorded: dev server up,
      fixture loaded, listing items visible.
- [ ] Stop and request user permission before any push
      (per CLAUDE.md §"GIT PUSH POLICY").
- [ ] After user approval: `br update bd-ml8z --status
      closed` + `br update bd-b5jm --status closed`.
- [ ] `br sync --flush-only && git add .beads/ && git
      commit` from the **main repo** (per
      `.claude/rules/worktrees.md` §"Committing beads
      changes").

### End-to-end CLI verification record (fill in after impl)

To be completed after impl. Fixture, invocation, output
snippet, observation note.

## Filing reminder

This sub-plan corresponds to **two** bd issues:

- `bd-ml8z` — L3, the listings-resolve transforms.
- `bd-b5jm` — L4, the doctemplate enhancements.

Both close together when this hand-off lands. Update both
issue descriptions with a one-line link to this file.
After impl, both close with reasons that reference the
landed commit.

### Follow-up bd issues filed at hand-off

Filed 2026-05-06 under `--deps discovered-from:bd-ml8z`:

1. **`bd-0fd0`** — Lua filter slot between generate and
   render transforms. Strawman: split
   `build_transform_pipeline` into
   `build_generate_transforms()` /
   `build_render_transforms()` with a configurable
   user-filter bridge. L3 leaves `// TODO(bd-0fd0):`
   markers at three source-code locations.
2. **`bd-0jyl`** — Source-info threading through
   listing markdown re-parse. L3's umbrella `Q-12-10`
   warning is the v1 placeholder; a proper design merges
   the fresh `SourceContext` into the host page's
   `SourceContext` and preserves the host-key →
   template-substitution → markdown-span chain.
3. **`bd-0wyo`** — Server-precomputed
   `other_metadata_html` helper for the default
   listing. Restores Q1's `otherFields` behavior. v1
   ships curated-fields-only; the source-code marker
   lands in `item-default.template`.

### Conditional follow-up issues (file during impl if
they trigger)

4. **Profile not in `ProjectIndex` but listed by a
   `contents:` glob.** Edge case discovered while making
   D10. Files outside the project's render set are
   silently dropped today. File only if a real Q1
   migration shows the gap matters.
5. **`quarto-listing.scss` Bootstrap-variable wiring**
   *(conditional).* Filed only if the L3 spike on the
   SCSS hits the deferral path described under D5.
6. **`list.min.js` artifact-store wiring blocked**
   *(conditional).* Filed only if the all-three-asset
   deferral path under D5 triggers.
