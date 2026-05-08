# L8 — Custom listing templates (sub-plan)

**Date:** 2026-05-07
**Beads:** `bd-rqgx` (this phase). Parent epic: `bd-61cd`
(`claude-notes/plans/2026-05-05-listings-epic.md`).
**Predecessors:**
- L0–L3 (closed) — listing data model, schema, generate +
  render transforms with the `Custom` type already parsed and
  the `template:` config already collected.
- L4 (closed, bundled with L3) — `quarto-doctemplate` enhancements.
  In particular `quarto_doctemplate::project_listing_resolver(builtins)`
  exists today and returns `ChainedResolver<FileSystemResolver,
  MemoryResolver>` — exactly the chain L8 reuses.
- L7 (closed) — placeholder upgrade. Custom templates that
  emit the `description-placeholder-{begin,end}` and
  `image-placeholder-{begin,end}` bindings get the same L7
  upgrade behavior as the built-in templates without any
  L8-specific work.

**Status:** Draft. Awaiting user approval before hand-off.

## Goal of this phase

Let users supply their own listing template via `listing.template:
my-listing.template`. The custom template gets the same data
binding the built-ins receive (`listing.*`, `items[*]`,
`project.*`), can use built-in partials (`item-default`,
`item-grid`, `item-table`) via the existing resolver chain, and
can shadow built-in partial names with same-named files in the
host-page directory.

L8 ships:

1. **Stop force-downgrading `Custom` to `Default`.** The
   `Q-12-1` "L8 deferral" diagnostic in
   `transforms/listing_render.rs:169` is removed. Custom
   listings now route through a new `render_custom_template`
   path.
2. **Custom-template loader.** Reads the file at
   `<host-dir>/<template>` via `std::fs::read_to_string`,
   compiles it with `Template::compile_with_resolver(...,
   project_listing_resolver(builtins_resolver()), ...)` so the
   custom template can include built-in partials and have its
   own neighboring partial files.
3. **Diagnostic surface (per Q2 "warn + fall back" convention,
   not Q1's "throw"):**
   - **`Q-12-8`** (already in the catalog, currently unused) —
     "Listing template file missing." Fall back to the
     `default` built-in.
   - **`Q-12-14`** (new) — "`type: custom` set but no
     `template:` provided." Fall back to the `default`
     built-in.
   - **`Q-12-10`** (existing) — template compile/render error.
     Already wired in `render_one` for built-ins; the same
     code-path catches compile errors on custom templates too.
   - **`Q-12-7`** (existing) — `template:` set but `type:` not
     `custom`. Already emitted at parse-time. L8 does not
     change this; it remains the "you wrote `template:` but
     forgot `type: custom`" warning.
   - **`Q-12-9`** (existing) — `.ejs.md` deprecation. Already
     emitted at parse-time. L8 still attempts to load the
     file (since the user might have manually converted it to
     doctemplate syntax but kept the extension); compile
     errors then surface as `Q-12-10`.
4. **Tests** covering: end-to-end render with a custom
   template; access to `item.extra` and `listing.template-params`;
   custom template using a built-in partial (`$item-default()$`);
   custom template shadowing a built-in partial name; missing-
   file fallback; missing-template-path fallback;
   compile-error fallback; the existing Q-12-7 / Q-12-9 paths
   keep working.
5. **Pass `cargo xtask verify`** (full, including hub-client +
   WASM build). On WASM, `std::fs::read_to_string` returns
   `NotFound`; custom listings fall back to `default` with
   `Q-12-8` — same UX as a missing file natively. This is
   acceptable for v1 (decision D1).

**Out of scope for L8 (deferred):**

- **WASM/VFS-aware custom-template loading.** Filed as
  follow-up bd. Hub-client preview of pages with `type: custom`
  shows the default fallback today; a future enhancement
  threads `runtime` through to the listing transforms so
  custom templates can be read from the VFS.
- **Project-root and `_extensions/` lookup paths.** Filed as
  follow-up bd. Q1 supports both via its publish-extension
  framework; Q2 doesn't have an extension framework yet, and
  the broader question of "how should YAML-defined paths
  resolve" is expected to be answered by the **`!path` YAML
  tag + Q2 metadata-merging design** (a planned cross-cutting
  feature). When that design lands, listing template paths
  become one consumer among many. Until then, L8 ships
  Q1-parity host-page-relative resolution and nothing more.
- **Compiled-template caching across hosts.** Filed as
  conditional follow-up. v1 recompiles per listing-host
  render-call. Typical projects have 1–2 host pages; cost is
  negligible. If profiling later shows compilation as a
  hotspot, a project-scoped cache (key: absolute path + mtime)
  is a clean follow-up.
- **`field-display-names` honored by custom templates.** Q1
  exposes the field-display-names map to custom templates;
  v1 already exposes `listing.fields` (the ordered field list)
  via the binding. The display-names map can be added later
  without breaking templates that only use what's there now.
- **`utils` helpers that Q1 exposes (`utils.localizedString`,
  `utils.b64EncodeUnicode`, etc.).** Q2 pre-renders helpers
  server-side into the binding (epic decision 3). v1 covers
  what the built-ins need; if a custom template needs more
  pre-rendered helpers, file a follow-up listing the specific
  helper.
- **A `listings.json` index** for search. Listed in the epic
  as out-of-scope; mentioned here only because Q1's custom-
  template path produces that file as a side effect. Q2 punts.

## Reference material

Read first:

- Parent epic: `claude-notes/plans/2026-05-05-listings-epic.md`
  §"L8" + §"Resolved decisions" #1 (template extension =
  `.template`).
- L3 sub-plan:
  `claude-notes/plans/2026-05-06-listings-L3-resolve-transform.md`
  §"Generate transform" + §"Render transform" — describes the
  binding shape custom templates inherit. **L3 was where the
  `Custom`-downgrade decision was made**; L8 explicitly
  reverses it.
- L4 (bundled with L3) — `quarto_doctemplate::project_listing_resolver`
  is the resolver chain L8 reuses without modification.
- L7 sub-plan:
  `claude-notes/plans/2026-05-07-listings-L7-postrender-upgrade.md`
  §"Marker design" + §"D14 (image envelope wraps in link)" —
  custom templates that want L7 description/image upgrades
  must emit the same begin/end placeholder bindings the
  built-ins do (`$description-placeholder-begin$` etc.). The
  built-in partials already do this; a custom template that
  uses `$item-default()$` inherits the behavior automatically.
- Q1 reference (read-only):
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing-read.ts:1319`
    — Q1 resolves custom-template paths as
    `join(dirname(source), meta.template)`. Q2 mirrors.
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing.ts:340-358`
    — Q1's "throw on missing template" behavior. Q2 swaps for
    "warn + fall back to default" (Q2 epic-wide convention).
  - `external-sources/quarto-cli/src/project/types/website/listing/website-listing-template.ts:58-170`
    — Q1's `templateMarkdownHandler` shows how the binding
    is shaped for custom templates. Q2's binding (already
    built by `binding.rs`) is the equivalent and serves both
    built-ins and custom uniformly.
- Existing Q2 surface L8 builds on:
  - `crates/quarto-core/src/transforms/listing_render.rs:169-180`
    — the L8-deferral block. **Removed by L8.**
  - `crates/quarto-core/src/transforms/listing_render.rs:188-210`
    — the existing template-compile site. L8 forks this into
    a custom-template branch when `r.listing.kind ==
    ListingType::Custom`.
  - `crates/quarto-core/src/project/listing/config.rs:461-475`
    — `template:` parsing. Already collects the path; emits
    `Q-12-9` on `.ejs.md`. **Unchanged by L8.**
  - `crates/quarto-core/src/project/listing/config.rs:530-539`
    — `Q-12-7` cross-field validation. **Unchanged by L8.**
  - `crates/quarto-core/src/project/listing/binding.rs:141-150`
    — `listing.template-params` is already exposed. **Unchanged
    by L8** — L8 just verifies via tests.
  - `crates/quarto-core/src/project/listing/binding.rs:329-335`
    — `item.extra` is already populated. **Unchanged by L8** —
    L8 just verifies via tests.
  - `crates/quarto-core/src/project/listing/templates.rs:37-46`
    — `builtins_resolver()`. **Unchanged by L8**; the L8 custom-
    template path uses this exact resolver chained via
    `project_listing_resolver`.
  - `crates/quarto-doctemplate/src/resolver.rs:189-193`
    — `project_listing_resolver(builtins)`. The chain order
    is `FileSystemResolver → MemoryResolver`, which is what
    L8 wants: a custom template's neighboring partial file
    shadows a built-in name; built-ins are the fallback when
    no neighbor file exists.
  - `crates/quarto-error-reporting/error_catalog.json` lines
    785-790 — `Q-12-8` already registered with the right
    title. **No catalog edit needed** for `Q-12-8`; just
    start emitting it. `Q-12-14` is a new entry to add.

## Settled inputs

These are decisions, not open questions:

- **L8 ships next.** User-confirmed 2026-05-07. Per the epic's
  ordering recommendation (L8 is the "second major user-
  visible deliverable" after L3).
- **WASM custom-template support deferred.** User-confirmed
  2026-05-07. v1 uses `std::fs::read_to_string` directly,
  matching `quarto_doctemplate::FileSystemResolver`. WASM
  `std::fs` returns `NotFound`; custom listings fall back to
  the `default` built-in with `Q-12-8`. **A follow-up bd is
  filed at L8 close-out** to track plumbing the `runtime`
  through `RenderContext` (or pre-loading template content in
  Pass-1) so hub-client previews can render custom templates.
- **Path resolution: host-page-directory only.** User-confirmed
  2026-05-07. `template: my-listing.template` resolves to
  `<host-dir>/my-listing.template`. Absolute paths are
  accepted (`std::path::PathBuf::is_absolute()`-checked) and
  used as-is. The user noted (2026-05-07) that broader
  resolution semantics — project root, `_extensions/` —
  may eventually fall out of the planned **`!path` YAML
  tag + Q2 metadata-merging design**, where any
  YAML-encoded path is resolved relative to the YAML it was
  defined in. That design is too big for this plan; L8 ships
  Q1-parity host-relative resolution and a follow-up bd
  records the broader question.
- **No template cache in v1.** User-confirmed 2026-05-07.
  Recompile per listing-host render. Filed conditionally —
  add only if profiling demands.
- **No `_extensions/` lookup in v1.** User-confirmed
  2026-05-07. `template:` is a path. Extension catalog
  support is a separate feature that touches `_extensions/`
  discovery.
- **`Q-12-8` already in the catalog** — title "Listing
  Template File Missing." L8 is the first emission site.
- **`Q-12-14` is the next free Q-12 code.** Catalog entry to
  be added by L8.
- **L7 envelope contract carries through to custom templates
  for free.** A custom template that uses `$item-default()$`
  (the built-in partial) inherits the
  `description-placeholder-begin/end` and `image-placeholder-
  begin/end` emission unchanged. A custom template that
  inlines its own item markup is responsible for emitting
  those bindings if it wants L7 to upgrade the previews; if
  it doesn't, listings still render correctly with the L1
  fallbacks (per L1's safeguard contract).
- **Custom templates use the same `TemplateContext` shape
  the built-ins do.** No L8-specific additions to
  `binding.rs`. The existing `listing.template-params` and
  `item.extra` slots cover author-supplied free-form data.

## Architecture

### Stop force-downgrading `Custom`

Today (`transforms/listing_render.rs:169-180`):

```rust
let kind = if r.listing.kind == ListingType::Custom {
    push_diag(
        diags,
        "Q-12-1",
        "Custom listing templates land in a follow-up (bd-rqgx). \
         For now, this listing falls back to the `default` built-in. \
         Set `type: default | grid | table` to silence this diagnostic.",
    );
    ListingType::Default
} else {
    r.listing.kind
};
```

This block is removed. The render flow forks based on `kind`:

```rust
let (template_source, template_path, resolver) =
    match r.listing.kind {
        ListingType::Custom => match load_custom_template(r, &doc_input, diags) {
            Some(loaded) => loaded,
            None => default_template_triple(),  // Q-12-8 / Q-12-14 already emitted
        },
        kind => builtin_template_triple(kind),
    };

let template = match Template::compile_with_resolver(
    &template_source,
    &template_path,
    &resolver,
    0,
) {
    Ok(t) => t,
    Err(e) => {
        push_diag(diags, "Q-12-10",
            format!("Listing `{}` template failed to compile: {:?}. \
                     Listing skipped.", r.listing.id, e));
        return;
    }
};
```

`builtin_template_triple` returns the embedded source +
synthetic path (`Path::new("listing.template")`) + the
existing `builtins_resolver()` (no `FileSystemResolver` needed
for built-ins; their partials are all in the `MemoryResolver`).

`load_custom_template` returns the file contents + the
absolute on-disk path + a
`project_listing_resolver(builtins_resolver())` chain. The
absolute on-disk path is what `Template::compile_with_resolver`
uses as the `base_path` for partial resolution: a partial named
e.g. `my-helper` resolves to `<host-dir>/my-helper.template`
through `FileSystemResolver`, and falls back to the
built-in `MemoryResolver` if no such file exists.

If `load_custom_template` returns `None`, it has already
emitted the appropriate `Q-12-8` / `Q-12-14` diagnostic; the
caller falls back to `default_template_triple()` so the listing
still renders. The L1 safeguard contract (every listing item
must render correctly) holds.

### `load_custom_template`

```rust
// In transforms/listing_render.rs (or a new
// crates/quarto-core/src/project/listing/custom.rs module if
// the function plus its helpers exceed ~80 LOC).
fn load_custom_template(
    r: &ResolvedListing,
    host_input: &Path,
    diags: &mut Vec<DiagnosticMessage>,
) -> Option<(String, PathBuf, ChainedResolver<FileSystemResolver, MemoryResolver>)> {
    let template_rel = match r.listing.template.as_deref() {
        Some(p) => p,
        None => {
            push_diag(diags, "Q-12-14",
                format!("Listing `{}` declares `type: custom` but no `template:` path. \
                         Falling back to the `default` built-in.", r.listing.id));
            return None;
        }
    };

    let template_abs = if template_rel.is_absolute() {
        template_rel.to_path_buf()
    } else {
        host_input
            .parent()
            .map(|p| p.join(template_rel))
            .unwrap_or_else(|| template_rel.to_path_buf())
    };

    match std::fs::read_to_string(&template_abs) {
        Ok(source) => Some((
            source,
            template_abs,
            project_listing_resolver(builtins_resolver()),
        )),
        Err(_) => {
            push_diag(diags, "Q-12-8",
                format!("Listing `{}`: template file `{}` could not be read. \
                         Falling back to the `default` built-in.",
                        r.listing.id, template_abs.display()));
            None
        }
    }
}
```

`host_input` is `RenderContext.document.input` — the absolute
host page path. Its parent is the host directory.

The `Q-12-8` emission deliberately doesn't distinguish "file
not found" from "permission denied" / "not UTF-8" / etc. — they
all fail the same way for the user (the listing falls back).
If users hit non-NotFound errors enough to want differentiated
messages, that's a follow-up.

### Resolver chain — what custom templates can do

Once compiled with `project_listing_resolver(builtins)`:

| Construct in custom template | Resolves to                                                          |
|------------------------------|----------------------------------------------------------------------|
| `$item-default()$`           | The built-in `item-default` partial (from `MemoryResolver`)          |
| `$item-grid()$`              | The built-in `item-grid` partial                                     |
| `$item-table()$`             | The built-in `item-table` partial                                    |
| `$listing-default()$`        | The built-in top-level partial                                       |
| `$my-helper()$`              | `<host-dir>/my-helper.template` if it exists, else `Q-10-3`          |
| `$item-default()$` *and* a file `<host-dir>/item-default.template` exists | The host-dir file wins (FileSystemResolver is primary) |

The last row is a **feature** — Q1 has the same capability via
EJS partial-include override. A user who wants to tweak the
built-in item layout slightly can drop a same-named file and
it shadows the embedded partial.

### Custom-template binding — what's there today

Already populated by `binding.rs::build_listing_context`:

- `listing.id`, `listing.type`, `listing.fields`,
  `listing.page-size`, `listing.max-items`,
  `listing.filter-ui`, `listing.sort-ui`,
  `listing.max-description-length`, `listing.categories`
- `listing.image-align`, `listing.image-height`,
  `listing.image-lazy-loading`,
  `listing.grid-columns`, `listing.grid-item-border`,
  `listing.grid-item-align`,
  `listing.table-striped`, `listing.table-hover`
- `listing.template-params.*` — pass-through from the
  config's `template-params:` map. Custom templates use this
  for author-defined config: `$listing.template-params.color$`.
- `items[*].title`, `.subtitle`, `.description`, `.author`,
  `.authors`, `.date`, `.date-modified`, `.image`,
  `.image-alt`, `.path`, `.filename`, `.reading-time`,
  `.word-count`, `.categories`
- `items[*].show.<field>` — boolean flags from `listing.fields`
- `items[*].image-html`, `.metadata-attrs`,
  `.description-placeholder-begin`, `.description-placeholder-end`,
  `.image-placeholder-begin`, `.image-placeholder-end`,
  `.category-html`
- `items[*].extra.<key>` — pass-through from each profile's
  `listing-item.extra` (and unrecognized
  `listing-item.<key>` entries that L0 captures as `extra`).
- `project.site-url`, `project.title`

L8 confirms by tests that all of these are reachable from a
custom template with no L8-specific binding work.

### Pipeline placement and stage wiring

L8 doesn't touch the stage graph or the
`build_html_pipeline_stages_with_apply_config` /
`build_wasm_html_pipeline` builders. The work is contained in
`ListingRenderTransform` (a Pass-2 transform inside
`AstTransformsStage`). The pipeline runs the same way; only
the per-listing render branch changes.

## Module layout

```
crates/quarto-core/src/
  transforms/listing_render.rs        ← remove L8-deferral block;
                                        add `load_custom_template`
                                        (or extract to new file
                                        listing/custom.rs if size
                                        warrants).
  project/listing/
    config.rs                          ← UNCHANGED. (Q-12-7, Q-12-9
                                        already emitted at parse-time.)
    binding.rs                         ← UNCHANGED. listing.template-
                                        params and item.extra are
                                        already wired.
    templates.rs                       ← Update doc comment on
                                        `top_level_template_source`
                                        — the "Custom downgrade"
                                        comment is stale after L8.
    custom.rs                          ← NEW (optional). Holds
                                        `load_custom_template` if it
                                        grows past ~80 LOC with
                                        helpers. Otherwise inline in
                                        listing_render.rs.

crates/quarto-error-reporting/
  error_catalog.json                   ← +Q-12-14 entry.
                                        Q-12-8 already exists.
```

No new pipeline-builder edits. No new resolver types in
`quarto-doctemplate` (the `project_listing_resolver` constructor
is already there from L4).

## Diagnostic codes

L8 surface:

- **`Q-12-7`** (existing) — `template:` set without `type:
  custom`. Already emitted at parse-time. **No change.**
- **`Q-12-8`** (catalog-only today, no emitter) — Listing
  template file missing. **L8 wires the emission site** in
  `load_custom_template`'s I/O-failure branch.
- **`Q-12-9`** (existing) — `.ejs.md` deprecation. Already
  emitted. **No change.** (A `.ejs.md` file that the user
  has converted to doctemplate syntax in place will still
  load and render; if the file is genuine EJS, compile fails
  with `Q-12-10`.)
- **`Q-12-10`** (existing) — Template compile/render error.
  Already wired. **No change** — the same code path catches
  custom-template compile errors. Note: the catalog title is
  "Listing Markdown Re-parse Diagnostics" but the emitter's
  message text says "template failed to compile"; this
  predates L8 and is not L8's job to clean up. File a
  follow-up if the title/message mismatch matters to a user.
- **`Q-12-14`** (new) — `type: custom` set but no `template:`
  path. **L8 adds catalog entry + emitter.** Catalog text:
  "A `type: custom` listing requires a `template:` field
  pointing at a doctemplate file. Falling back to the
  built-in `default` template."

## Test plan (TDD)

Per CLAUDE.md: write tests, watch fail, implement, watch pass.

### Phase 1 — diagnostic catalog + emitter scaffolding

In `crates/quarto-error-reporting/`:

1. **`error_catalog_has_q_12_14`** — assert the catalog has a
   `Q-12-14` entry with the expected title and message
   template.

In `crates/quarto-core/src/transforms/listing_render.rs`'s
test module:

2. **`custom_listing_without_template_path_emits_q_12_14_and_falls_back`**
   — fixture: host page declares `listing: { type: custom }`
   (no `template:`). After render, listing renders using the
   `default` built-in markup; one `Q-12-14` diagnostic in
   `ctx.diagnostics`.
3. **`custom_listing_with_missing_template_file_emits_q_12_8_and_falls_back`**
   — fixture: host page declares `listing: { type: custom,
   template: nonexistent.template }`. One `Q-12-8` diagnostic;
   `default` markup rendered.
4. **`custom_listing_q_12_1_no_longer_emitted`** — fixture:
   host page declares `type: custom` (with or without
   template). Assert `ctx.diagnostics` does **not** contain
   `Q-12-1` (the old "L8 deferral" code).

### Phase 2 — happy-path custom render

5. **`custom_template_renders_with_listing_and_items_bindings`**
   — fixture: host page + a `simple.template` next to it that
   reads `$listing.id$` and iterates `$for(items)$ - $it.title$\n
   $endfor$`. After render, the host AST contains the expected
   markup.
6. **`custom_template_can_call_built_in_item_default_partial`**
   — fixture: `$listing.id$\n\n$for(items)$\n$item-default()$\n
   $endfor$`. The custom template wraps a built-in partial.
   Verifies the resolver chain works.
7. **`custom_template_can_shadow_a_built_in_partial_with_local_file`**
   — fixture: a `<host-dir>/item-default.template` overrides
   the built-in. Render output contains the local file's
   markup, not the built-in's.

### Phase 3 — binding pass-through

8. **`custom_template_sees_listing_template_params`** —
   fixture: `template-params: { color: red, count: 3 }`,
   custom template emits `$listing.template-params.color$ /
   $listing.template-params.count$`. Output contains
   `red / 3`.
9. **`custom_template_sees_item_extra`** — fixture:
   `posts/foo.qmd` has `listing-item.status: draft`; custom
   template emits `$it.extra.status$` per item. Output
   contains `draft`.
10. **`custom_template_sees_listing_fields_and_per_item_show`**
    — fixture sets `listing.fields: [title, date]`; custom
    template iterates `$listing.fields$` and emits flags from
    `$it.show.title$`. Verifies the existing show-flag wiring
    isn't custom-template-aware in a wrong way.

### Phase 4 — error & fallback edges

11. **`custom_template_with_compile_error_emits_q_12_10_and_skips_listing`**
    — template file exists but contains invalid doctemplate
    syntax (e.g. unbalanced `$if`). Expect `Q-12-10`; the host
    page renders without the listing block but doesn't panic.
12. **`custom_template_with_absolute_path_resolves_correctly`**
    — fixture supplies an absolute path; the file is read
    from that absolute location.
13. **`custom_template_path_uses_host_dir_not_project_root`**
    — fixture: host page `posts/index.qmd` declares
    `template: layout.template`; the resolver looks under
    `posts/`, not at the project root. (Asserts Q1-parity
    semantics.)
14. **`custom_template_with_ejs_md_extension_emits_q_12_9_then_attempts_load`**
    — fixture: `template: legacy.ejs.md`. The
    parse-time `Q-12-9` is emitted (existing behavior); L8
    still tries to read + compile the file; if compile fails
    (typical for EJS syntax), `Q-12-10` is also emitted; the
    listing falls back to the default built-in via the
    compile-error path.
    *Note:* this test exists to lock the behavior; the user-
    facing experience is "deprecation warning + fallback,"
    same as if the user had set both `Q-12-9` and `Q-12-8`.

### Phase 5 — L7 envelope behavior carries through

15. **`custom_template_using_item_default_partial_emits_l7_envelopes`**
    — fixture: custom template wraps `$item-default()$`.
    Rendered HTML contains `<!-- desc-begin(...) -->` and
    `<!-- desc-end(...) -->` markers (same as a built-in
    `default` listing).
16. **`custom_template_inlining_own_item_markup_can_omit_l7_envelopes`**
    — fixture: custom template doesn't use the built-in item
    partials and doesn't reference the placeholder bindings.
    Rendered HTML has no envelope markers; L7's post-render
    step is a no-op for these listings; the listing still
    renders correctly using the static binding values
    (validates the L1 safeguard contract for custom templates
    too).

### Phase 6 — End-to-end CLI verification

17. **`pipeline_e2e_custom_listing`** — fixture project:

    ```
    _quarto.yml         # project.type: website
    index.qmd           # listing host with type: custom,
                        #   template: my-listing.template
    my-listing.template # custom doctemplate
    posts/foo.qmd       # post with listing-item.status: draft
    posts/bar.qmd       # post with listing-item.status: published
    ```

    `cargo run --bin q2 -- render` produces an `_site/index.html`
    whose listing markup comes from the custom template (the
    "two-column status badge" layout the fixture defines, not
    the built-in default). Status badges show `draft` / `published`
    pulled from `item.extra.status`. **End-to-end CLI
    verification per CLAUDE.md.** Record the invocation, the
    grepped HTML showing the custom layout + status badges,
    and an explicit "output inspected" note.

18. **`pipeline_e2e_custom_listing_falls_back_when_missing`**
    — same fixture but `template: missing.template`. Render
    succeeds, output uses the default built-in layout, project
    diagnostics contain `Q-12-8`.

### Hub-client smoke

L8's surface change is L3's render branch + a new file-read
site. In WASM, `std::fs::read_to_string` returns `NotFound`,
so a hub-client preview of a `type: custom` listing host
falls back to the `default` built-in with `Q-12-8`. This is
the **expected v1 behavior** (D1).

The hub-client smoke for L8 confirms:

```bash
cd hub-client
npm run build:all
npm run dev
```

Open a fixture project with a `type: custom` listing in the
hub-client. Confirm:

- Listing renders (using the default fallback).
- Hub-client console / diagnostics surface a `Q-12-8` entry
  (matches the "missing template" path).
- No panics, no blank-page failures.

Note this in the close-out as the WASM behavior; the
follow-up bd records the work to make custom templates work
in hub-client.

### End-to-end CLI verification record

Two fixtures rendered with the real `q2` binary on
2026-05-08; output inspected by hand.

#### Fixture 1 — custom-template happy path (`/tmp/l8-fixture-custom/`)

Layout:

```
_quarto.yml         (project.type: website, output-dir: _site)
index.qmd           (listing: type: custom, template: posts-with-status.template)
posts-with-status.template
                    (custom layout: distinctive class `.l8-status-table`,
                     pipe-table iterating items, emits $it.extra.status$
                     wrapped in inline-code so `draft`/`published` is
                     unmistakably present in the rendered HTML)
posts/foo.qmd       (listing-item.extra.status: draft)
posts/bar.qmd       (listing-item.extra.status: published)
```

Invocation:

```
cargo run --bin q2 --quiet -- render /tmp/l8-fixture-custom
```

stderr was empty (no diagnostics).

Snippets from `_site/index.html` (via `grep -E "l8-status-table|draft|published|foo|bar"`):

```
<div class="list l8-status-table">
<td><a href="posts/bar.html">Bar post</a>
<td><code>published</code>
<td><a href="posts/foo.html">Foo post</a>
<td><code>draft</code>
```

Observations:

- The custom-template wrapper class `l8-status-table` is on
  the rendered Div — confirms the user-supplied template
  drove the render, not the default fallback.
- The `<code>draft</code>` / `<code>published</code>` cells
  pulled from `it.extra.status` confirm the binding
  pass-through reaches custom templates end-to-end through
  the binary (not just the in-process tests).
- The two posts render in alphabetical order (default sort,
  no `sort:` configured), matching expectations for the
  fixture.

Output inspected by hand: ✓

#### Fixture 2 — missing-template fallback (`/tmp/l8-fixture-missing/`)

Same fixture as above but `template: nonexistent-template.template`
with no such file present. Posts unchanged.

Invocation: `cargo run --bin q2 --quiet -- render /tmp/l8-fixture-missing`

Stderr captured:

```
Warning [Q-12-8]: Listing `listing-1`: template file `/private/tmp/l8-fixture-missing/nonexistent-template.template` could not be read. Falling back to the `default` built-in.
```

Snippets from `_site/index.html`:

```
<div class="list quarto-listing-default">
<h3><a href="posts/bar.html" class="no-anchor no-external listing-title">Bar post</a></h3>
<h3 id="-1"><a href="posts/foo.html" class="no-anchor no-external listing-title">Foo post</a></h3>
```

Observations:

- `quarto-listing-default` wrapper class confirms the
  default-built-in fallback rendered.
- `l8-status-table` is **absent** (the custom template's
  marker class never reached the AST).
- The Q-12-8 diagnostic message includes the absolute path
  the loader tried to read, which is the most useful form
  for the author trying to fix a typo.

Output inspected by hand: ✓

## Branch / worktree

L8 starts from the current `feature/listings` head. The L8
worktree lives at:

```
.worktrees/bd-rqgx-listings-custom-templates/
```

Branch: `beads/bd-rqgx-listings-custom-templates`, branched
off `feature/listings`.

Per `.claude/rules/worktrees.md`:

```bash
cd .worktrees/bd-rqgx-listings-custom-templates
echo "../../../.beads" > .beads/redirect
npm install
cargo xtask verify --skip-hub-build  # baseline before changes
```

Before starting, the L8 session must record:

- Current `feature/listings` HEAD hash (was `52944cf2` at plan
  time; verify and re-record).
- Baseline test count (post-L7 close-out — verify and record;
  L7 added several phases of tests, exact count to be
  determined at impl start).

## Pipeline-builder wiring

None. L8's changes are confined to:

- `transforms/listing_render.rs` — the existing transform's
  internal logic.
- `project/listing/custom.rs` (optional new file).
- `templates.rs` — doc-comment freshen-up only.
- `error_catalog.json` — one new entry.

The stage graph (`build_html_pipeline_stages_with_apply_config`,
`build_wasm_html_pipeline`) is unchanged. No new traits, no
new context fields, no new artifacts.

## Risks and mitigations

- **Risk: a custom template with a typo in `$item-default()$`
  partial-name reference resolves silently to nothing
  (`Q-10-3` is emitted but easy to miss).** *Mitigation:*
  this is doctemplate's existing behavior, not L8-specific.
  L8 doesn't wrap it. If users hit this, the `Q-10-3` "Partial
  Not Found" diagnostic surfaces in the project diagnostics
  channel — same UX as for built-in templates.
- **Risk: `std::fs::read_to_string` on Windows returns a
  `NotFound` for a path with the wrong slash convention
  (Q1 historical pain point).** *Mitigation:* `PathBuf::join`
  on Windows uses backslashes; `host_input.parent().join(template_rel)`
  produces a Windows-correct path. The
  `template_rel` came from YAML, where the user might have
  written forward slashes. `PathBuf::from(template_rel)` on
  Windows accepts forward slashes and normalizes; the joined
  path resolves correctly. Test #13 covers this implicitly
  (the test runs on Windows CI per `cross-platform.md`).
  *Explicit Windows test* — verify on Windows CI; if Q1's
  pain shows up, we add normalization.
- **Risk: a custom template depending on a binding key that
  the v1 surface doesn't expose** (e.g. Q1's
  `field-display-names` map). *Mitigation:* doctemplate emits
  `Q-10-2` "Undefined variable" with the key name; users see
  the diagnostic and can either change the template or file
  for the missing key. The v1 binding surface is documented
  in §"Architecture: Custom-template binding"; the L10
  migration docs (eventual) will cover the gap explicitly.
- **Risk: a `_extensions/...`-style template path
  half-resolves** (`<host-dir>/_extensions/foo/bar.template`
  exists by accident). *Mitigation:* this is correct
  v1 behavior — `_extensions` is just another subdirectory
  to v1 — and a feature, not a bug, until the extension
  framework lands. Document in the user-facing docs (when
  they exist; deferred per L7 D13).
- **Risk: hub-client users authoring custom templates
  silently get the default fallback and don't realize.**
  *Mitigation:* the `Q-12-8` diagnostic surfaces in the
  hub-client preview's diagnostics panel (existing wiring);
  users see the "template file could not be read" message.
  The follow-up bd to enable WASM custom-template loading
  records the proper fix.
- **Risk: a custom template emits the L7 placeholder bindings
  but L7 doesn't run (hub-client / `quarto preview`).**
  *Mitigation:* this is exactly the L1 safeguard contract —
  the bindings expand to HTML comments around the static
  fallback content. In hub-client the markers are invisible;
  in CLI render the markers get substituted by L7. Custom
  templates inherit the contract by using the bindings.
- **Risk: a custom template recurses indefinitely
  (`$item-default()$` calls itself if the user names a local
  partial `item-default`).** *Mitigation:* `quarto-doctemplate`
  has cycle detection (depth-limit) — already exercised by
  built-ins. L8 inherits without changes.
- **Risk: a path with `..` in the YAML
  (`template: ../shared/listing.template`) escapes the
  project.** *Mitigation:* `std::fs::read_to_string` allows
  this; v1 follows the user's intent. If the user wants to
  share a template across projects, that's their call —
  same as `_quarto.yml` includes via `$ref`. Document if
  asked.
- **Risk: snapshot churn.** L8 does not modify the built-in
  template output. The built-in render snapshot tests stay
  unchanged. The new custom-template tests carry their own
  expected output. *Mitigation:* run `cargo insta test`
  before commit; expect snapshots to be **additive only**
  (no diffs to existing snapshots).

## Edge-case behavior (settled)

These were once likely "open questions" but are pre-resolved:

1. **Empty `template:` value (`template: ""`).** Treated as
   "no template path" — the empty path can't read a file. v1
   emits `Q-12-14` (no path provided) since the empty string
   is not a meaningful path. Alternative would be `Q-12-8`
   ("file not found") — both produce the same fallback, but
   `Q-12-14` is a clearer signal.
2. **`template:` with a non-string value
   (e.g. `template: { foo: bar }`).** Already filtered at
   parse-time (`as_plain_text()` returns `None` ⇒ `template`
   stays `None`). Reaches L8 as "no template path." Q-12-14
   fires. No new diagnostic needed for the malformed value
   itself (the schema layer will eventually catch it).
3. **`type:` not declared but `template:` is.** Already
   handled by `Q-12-7` at parse-time; the listing's `kind`
   stays at the declared value (or default). L8's branch only
   fires when `kind == Custom`, so this case naturally falls
   through to the built-in path.
4. **Multiple `type: custom` listings on one host page.** Each
   is rendered independently via its own
   `load_custom_template` call. No shared state.
5. **Custom template producing no output (empty file).**
   Compiles fine; renders empty markdown; the listing slot
   stays empty. No diagnostic — same as a built-in template
   producing empty output for an empty `items[]` array.
6. **Custom template writing to `$listing.id$` as part of an
   anchor (`<a id="$listing.id$">`).** Works; the binding
   exposes the listing id verbatim.
7. **`.ejs.md` file that the user has manually rewritten in
   doctemplate syntax but kept the extension.** L8 reads + 
   compiles + renders normally; `Q-12-9` deprecation warning
   already fired at parse-time; no L8-specific behavior.

## Decisions log

- **D1 (`std::fs` for v1; runtime-aware loading deferred):**
  user-confirmed 2026-05-07. Matches `FileSystemResolver`
  precedent. Hub-client falls back to default with `Q-12-8`.
  Follow-up bd files at L8 close-out.
- **D2 (host-page-directory only for path resolution):**
  user-confirmed 2026-05-07. Q1-parity. The user noted a
  broader future direction: the planned **`!path` YAML tag
  + Q2 metadata-merging design** could unify path
  resolution for all YAML-declared paths. L8 is not the
  place to design that; a follow-up bd captures the linkage
  so when the broader design lands, listing template paths
  fold in cleanly.
- **D3 (no template cache in v1):** user-confirmed 2026-05-07.
- **D4 (no `_extensions/` lookup in v1):** user-confirmed
  2026-05-07. File-paths only.
- **D5 (warn + fall back to default, not throw):** Q2-wide
  convention. Q1 throws on missing custom template; Q2 emits
  `Q-12-8` and renders with `default` so the rest of the
  page still works.
- **D6 (Q-12-14 = new code; Q-12-8 already exists):**
  catalog gets a single new entry.
- **D7 (custom templates inherit all existing binding
  surface):** no L8-specific binding work. Tests verify by
  exercising `listing.template-params`, `item.extra`, and
  the L7 envelope bindings.
- **D8 (resolver chain unchanged from L4 design):**
  `project_listing_resolver(builtins_resolver())`. Filesystem
  primary, built-ins fallback. Custom templates can shadow
  built-in partials by name.
- **D9 (worktree on `feature/listings`):** branch
  `beads/bd-rqgx-listings-custom-templates` at
  `.worktrees/bd-rqgx-listings-custom-templates/`, branched
  off the current `feature/listings` head (`52944cf2` at
  plan time — confirm at impl start). Same convention as
  L1 / L3 / L5 / L6 / L7.
- **D10 (remove `Q-12-1` "L8 deferral" emission and the
  associated stale comment in `templates.rs`):** the comment
  in `top_level_template_source` saying *"if we get here
  with `Custom`, the render transform has already emitted
  Q-12-1 and downgraded to `Default`"* becomes false after
  L8. Update the doc comment to reflect the new flow:
  *"Custom listings take a separate code path in
  `render_one`; this function is only called for the built-in
  types."*
- **D11 (no docs/ user-facing wiring):** the user-facing
  Quarto website doesn't exist yet (per L7 D13). The L8
  custom-template feature gets a follow-up bd describing what
  the docs should cover when the website materializes (the
  binding surface, the partial-resolver chain, the diagnostic
  meanings, the v1 limitations: no WASM, no extension
  lookup).

## Implementation steps

Follow CLAUDE.md TDD: write tests, watch fail, implement,
watch pass.

### Preparation

- [x] Re-read `claude-notes/instructions/testing.md` and
      `claude-notes/instructions/coding.md`.
- [x] Re-read `.claude/rules/wasm.md` (`?Send`, WASM-cfg
      gating). L8 is std::fs-only on native and gracefully
      degrades on WASM, so no WASM-cfg gating is needed; just
      verify behavior.
- [x] Confirm `feature/listings` head is the post-L7 merge
      (`52944cf2` confirmed; baseline 8697 tests pass).
- [x] Create the worktree at
      `.worktrees/bd-rqgx-listings-custom-templates/` per
      §"Branch / worktree". Branch
      `beads/bd-rqgx-listings-custom-templates`.
- [x] `npm install` in the worktree.
- [x] Add `.beads/redirect` per worktree rules.
- [x] Baseline: `cargo xtask verify --skip-hub-build
      --skip-hub-tests`; recorded 8697 Rust tests passing.

### TDD phase 1 — diagnostics + scaffolding

- [x] Write tests #1–4. They fail (Q-12-14 missing from
      catalog; Q-12-1 still emitted; Q-12-8/Q-12-14 not
      emitted).
- [x] Add `Q-12-14` to `error_catalog.json`.
- [x] Remove the L8-deferral block in
      `transforms/listing_render.rs`. Replace with a stub
      that branches on `ListingType::Custom` and falls back
      to default + emits the new diagnostics.
- [x] Tests pass.

### TDD phase 2 — happy-path custom render

- [x] Write tests #5–7. Implementation already in place from
      phase 1, so they passed on first run rather than after
      a deliberate red-green cycle. Tests verify (a) custom
      template renders against the standard binding, (b)
      `$items:item-default()$` resolves to the built-in
      partial, (c) a same-named neighboring file shadows the
      built-in via the `FileSystemResolver` primary.
- [x] Implement `load_custom_template` (inlined in
      `listing_render.rs`; under 80 LOC including doc-comment
      and `LoadedCustomTemplate` struct). Wired the
      branch-on-`Custom` path to use the loader's triple.
- [x] Use `project_listing_resolver(builtins_resolver())` so
      the custom template can call built-in partials and have
      its own neighboring partial files.
- [x] Tests pass.

### TDD phase 3 — binding pass-through

- [x] Write tests #8–10. The binding work is unchanged from
      L3+L4 and these tests confirm it carries through to
      custom templates as expected. Test #10's first draft
      used bare `$it.show.author$` (which fires Undefined-
      variable since the `show` map only carries entries for
      fields in `listing.fields`); rewritten to use the real
      `$if(it.show.<field>)$` idiom that the built-in
      templates and Q1's custom templates use.
- [x] Tests pass.

### TDD phase 4 — error & fallback edges

- [x] Wrote tests #11–14. They passed on the first run since
      the loader and compile-error path were already wired in
      phase 1 / 2.
- [x] Confirmed the `Q-12-10` compile-error path catches
      custom templates uniformly (compile failure ⇒ skip the
      listing, no default fallback for compile errors —
      symmetric with how built-ins behave on bad source).
- [x] Verified absolute-path branch (test #12), host-dir-not-
      project-root resolution (test #13, with a project-root
      decoy that must NOT win), and `.ejs.md` compile-error
      path (test #14).
- [x] Tests pass.

### TDD phase 5 — L7 envelope inheritance

- [x] Wrote tests #15–16. They pass without any L8-specific
      envelope work, confirming the binding from L3+L7 is
      transparent to template-source choice. Test #16 verifies
      the L1 safeguard contract for custom-templates that
      ignore the envelope bindings — the listing still renders
      correctly via the static `description` / `title` tokens.

### TDD phase 6 — End-to-end CLI

- [x] Built two real-binary fixtures (`/tmp/l8-fixture-custom`
      and `/tmp/l8-fixture-missing`); rendered both via
      `cargo run --bin q2 --quiet -- render`; inspected
      output by hand. Both behave as designed: custom
      template drives the render and surfaces
      `it.extra.status`; missing-template falls back to
      the default with `Q-12-8` carrying the absolute
      attempted-path.
- [x] Recorded the verification in §"End-to-end CLI
      verification record" above.

### Verification and close-out

- [x] Updated doc comment in `templates.rs::top_level_template_source`
      per D10. The stale "Q-12-1 + downgrade" claim is gone;
      the new comment names the L8 fork point (`load_custom_template`)
      and explicitly labels the `Custom` arm as a defensive
      fallback rather than the active path.
- [x] `cargo build --workspace` clean (no warnings).
- [x] `cargo nextest run --workspace` — 8712 tests pass
      (baseline 8697 → +15 new tests added by L8: 1 catalog
      test + 14 in `transforms::listing_render::tests`).
- [x] `cargo xtask lint` clean (696 files checked, no
      violations).
- [x] `cargo xtask verify` (full, including hub-client +
      WASM build) — all 9 steps green. The `std::fs` calls
      in the listing transform compile to wasm32 unchanged;
      runtime fallback to default with Q-12-8 is the
      expected behavior in WASM contexts.
- [ ] **Hub-client browser smoke unrun in this session.**
      A real-browser session is required to confirm the
      hub-client preview surfaces Q-12-8 in its diagnostics
      panel for a `type: custom` fixture; `cargo xtask verify`
      already exercises the WASM build + the existing
      hub-client vitest suite, but the listings-specific
      smoke is browser-dependent. Recording this gap
      explicitly per CLAUDE.md §"End-to-end verification".
- [x] End-to-end CLI verification fixtures rendered; output
      inspected; recorded above in
      §"End-to-end CLI verification record".
- [x] User-facing `docs/` callout deferred per D11; follow-up
      filed as **bd-u4ow** (docs/ page for custom listing
      templates) so the docs work picks it up when the
      website comes online.
- [ ] Stop and request user permission before any push (per
      CLAUDE.md §"GIT PUSH POLICY").
- [ ] After user approval: `br update bd-rqgx --status closed`.
- [ ] `br sync --flush-only && git add .beads/ && git commit`
      from the **main repo** (per `.claude/rules/worktrees.md`).
- [ ] Update the listings epic table
      (`claude-notes/plans/2026-05-05-listings-epic.md`) to
      mark L8 closed with the merge commit hash.

### Filed follow-up bd issues

- **bd-tmka** — WASM/VFS-aware custom listing template
  loading (L8 D1).
- **bd-ubjo** — Broader path resolution for YAML-declared
  paths (L8 D2; tied to the planned `!path` YAML tag design).
- **bd-u4ow** — User-facing `docs/` page for custom listing
  templates (L8 D11).
- **bd-fvuy** — Q-12-10 catalog title/message inconsistency
  (pre-existing, surfaced again during L8 review).

## Filing reminder

This sub-plan corresponds to **one** bd issue:

- `bd-rqgx` — L8, custom listing templates.

After impl, close with a reason that references the landed
commit. Update the issue description with a one-line link to
this file.

### Follow-up bd issues (file during impl if they trigger)

1. **WASM/VFS-aware custom-template loading** *(planned)* —
   today (D1) custom templates use `std::fs::read_to_string`
   directly, so hub-client previews of `type: custom`
   listings fall back to the default built-in. The proper
   fix plumbs `runtime` through `RenderContext` (or
   pre-loads template content during Pass-1) so VFS reads
   work in WASM. **File at L8 close-out.**
2. **Broader path-resolution semantics for YAML-declared
   paths** *(planned)* — the user-noted future direction:
   the **`!path` YAML tag + Q2 metadata-merging design**
   would unify path resolution across `_quarto.yml`,
   per-page frontmatter, and listing templates. Listing
   `template:` paths would resolve relative to the YAML they
   were defined in (host-page frontmatter today; potentially
   project YAML or extension YAML later). L8 ships
   host-page-only and lets the broader design absorb the
   case when it lands. **File at L8 close-out** so the
   future design owner sees the linkage.
3. **`_extensions/`-aware template lookup** *(conditional)*
   — only after Q2's extension framework lands. v1 doesn't
   have one; this is purely descriptive of the future work.
   File only when the extension framework epic exists.
4. **Compiled-template cache** *(conditional)* — only if
   profiling shows compilation as a hotspot in projects
   with many `type: custom` listing hosts. v1 recompiles
   per call; cost is negligible for typical projects.
5. **Q-12-10 catalog title/message inconsistency
   cleanup** *(planned, low priority)* — the catalog title
   for `Q-12-10` is "Listing Markdown Re-parse Diagnostics"
   but the emitter uses it for both compile errors and
   re-parse diagnostics. Either split into two codes or
   broaden the title. Pre-existing; not L8's job, but L8's
   tests ride alongside the inconsistency.
6. **`field-display-names` exposure for custom templates**
   *(conditional)* — Q1 exposes the field-display-names map
   to custom templates; v1 doesn't. File only if a user
   reports needing it.
7. **User-facing `docs/` page for custom listing templates**
   *(planned, deferred — D11)* — when the Quarto-website
   tree under `docs/` becomes a real user-facing site, add
   a custom-listing-templates reference covering: the
   binding surface, the partial-resolver chain, the
   diagnostic meanings, the v1 limitations (no WASM custom-
   template loading; no extension lookup; no template
   cache). Wording and scope locked here; future docs work
   copies and expands. File at L8 close-out so the docs
   work picks it up when the website comes online.
