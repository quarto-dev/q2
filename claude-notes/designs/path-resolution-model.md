# Path resolution contract (config-authored paths)

**Status:** Normative contract. Promoted 2026-08-19 from the 2026-06-10
index note, after the #455/#524 divergence assessment
(`claude-notes/research/2026-08-19-path-resolution-class-assessment.md`).
Peer of `transform-pipeline-phases.md` and `document-profile-contract.md`:
code that consumes config-authored paths MUST conform; deviations are bugs
even when no test pins them yet.

**The one-sentence rule for authors:** *a consumer of a config-originated
path never chooses its own base directory.*

## The two rules (normative)

Quarto 2 interprets a path written in source (`.qmd` front matter,
`_quarto.yml`, `_metadata.yml`, extension config) by two rules:

1. **No leading `/` → relative to the directory of the file that *declared*
   the path.** Not "relative to the project root" in general — that is the
   special case for `_quarto.yml`, which lives at the project root.
   Concretely:
   - a path in `_quarto.yml` resolves against the project root,
   - a path in `docs/foo/_metadata.yml` resolves against `docs/foo/`,
   - a path in `docs/foo/bar.qmd`'s front matter resolves against `docs/foo/`.
   Getting this right requires **provenance**: the resolver must know which
   file declared the value, not which document is consuming it. Provenance is
   captured either as `SourceInfo` retained through the merge, or by
   marking + rebasing the value at merge time (see mechanisms below).

2. **Leading `/` → project-root-relative, uniformly** (decision 4 of
   bd-root-relative-paths-design-fc5pvkcv, extended to filesystem space by
   the #455 discussion). It is never an OS-absolute filesystem path. What
   "resolve" then means depends on the space (next section).

Carve-outs rule 2 must not swallow: protocol-relative `//host/x` URLs,
`data:` URIs, full URLs (all classified external before any path handling);
WASM VFS `/project/...` paths (a filesystem-space *internal* convention,
never authored config); genuinely OS-absolute *input-file* paths where a key
documents that it accepts them (checked with `quarto_util::is_rooted`, not
`Path::is_absolute`, for Windows correctness).

## Two spaces, one pivot, two exits

Every config-authored path belongs to exactly one of two spaces, and a
resolved path has a different terminal form in each:

- **Filesystem space** — the renderer reads the file at build time
  (`include-in-header`, `theme`, `template`, `filters`, `bibliography`,
  `csl`, …). Terminal operation: a `SystemRuntime` read against
  *declaring-dir ⊕ relative path*, or *project root ⊕ leading-`/` path*.
- **URL space** — the string survives into emitted HTML (image `src`, link
  `href`, logo, css `<link>`, …). Terminal operation: a **page-relative**
  href computed per consuming page (`ResourceResolverContext::page_url_for`),
  usually paired with a copy-to-`_site` action. No emitted HTML ever
  contains a `/`-absolute link, so a built `_site/` is relocatable under any
  deploy subpath.

The stable pivot between declaration and either exit is the
**project-root-relative canonical form with forward slashes** (or,
equivalently under mechanism 2 below, the doc-relative form produced by a
provenance-aware rebase). Resolution to a native absolute path or a final
href is always a lazy, terminal operation — never stored state, because
these values round-trip through serialization, hashing, and the qmd writer.

**The seam warning (the #524/#455 lesson):** any decree, fix, or audit
scoped to one space MUST record the other space's corresponding work as a
braid strand linked to bd-oejuizi9 before it ships. PR #524 fixed URL space
and carved out filesystem space without filing the complement; issue #455
sat in the carve-out for another release cycle.

## Author rules (MUST)

1. **Never choose a base directory at a consumption site.** No
   `doc_dir.join(config_string)`, no `Path::new(config_string)`, no CWD
   reads. Consume a value already marked/rebased at merge time
   (`ConfigValueKind::Path`), or call a blessed resolver (below) with the
   value's provenance.
2. **New path-shaped config keys** must be added to the marking registry
   (currently the tables listed under mechanisms) *and* to the inventory
   table in this document, in the same commit that introduces the key.
3. **Leading `/` handling lives in the resolvers only.** Do not add per-site
   leading-slash special cases; if a blessed resolver lacks the behavior you
   need, extend the resolver.
4. **Scope-outs are strands.** If your fix deliberately leaves a
   nonconforming site untouched, file the strand and link it before merging.

## Blessed mechanisms (current state; convergence target below)

Three provenance-correct mechanisms coexist today. New code should prefer
(3)'s shape; the convergence work will fold them together.

1. **`resolve_metadata_path`**
   (`crates/quarto-core/src/transforms/navigation_href.rs:583`) —
   `SourceInfo` → declaring file's dir → project-root-relative string, at
   Generate time. Used by navigation surfaces (sidebar/navbar/footer
   generate transforms). Caveat: `_quarto.yml`'s FileId is usually not in
   the per-document `SourceContext`, so the helper degrades to the raw
   string — correct only for callers that treat input as
   project-root-relative.
2. **`ConfigValueKind::Path` + `adjust_paths_to_document_dir`**
   (`crates/quarto-core/src/project/mod.rs:253-296`) — merge-time rebase of
   `Path`-kind values from declaring dir to consuming-doc dir. Fed by
   explicit `!path` tags and by the extension marking tables
   (`FRAGMENT_PATH_PATTERNS`, `FORMAT_ASSET_PATTERNS`, `PATH_VALUED_KEYS`).
   Blind spot: user-authored plain strings are never marked.
3. **Per-layer `layer_base` marking** (`crates/quarto-core/src/project/format_css.rs:104-130`,
   for `css`) — the merge passes the declaring base per layer
   (project dir / `_metadata.yml` parent / doc dir), marks the value `Path`,
   anchors leading `/` at the project root. The most general mechanism: it
   handles `_quarto.yml` without SourceInfo lookup and runs before any
   consumer.
4. **`BaseDirContext`** (`crates/quarto-core/src/glob/provenance.rs:47`) —
   the provenance engine for glob-valued keys (`listing.contents`,
   front-matter `resources:`). Globs stay on this machinery; they are
   pattern sets, not single paths.

**Convergence target** (assessment §6.2, tracked under bd-oejuizi9 +
bd-hjv5o): generalize mechanism 3 into a single path-shaped-key registry
unifying the four scattered tables plus the annotation table
(`crates/pampa/src/pandoc/meta_annotations.rs`, whose `Interpretation::Path`
is currently unused), so values arrive at consumers already marked and
declaration-dir-resolved, and consumers' existing `doc_dir.join` reads
become correct as written. Enforcement: the `config-path-base` xtask lint
(see strand reference in the inventory's gap list).

## Inventory of consumption sites (living — keep current)

Snapshot verified 2026-08-19 (full evidence in the assessment doc §4).
Update this table when adding keys or migrating sites.

### Conforming

| Site | Keys | Mechanism |
|---|---|---|
| `transforms/{navbar,sidebar,footer}_generate.rs` | nav hrefs, logos | (1) `resolve_metadata_path` |
| `glob/provenance.rs` + `project/listing/glob_resolve.rs`, `project_resources.rs` | `listing.contents`, front-matter `resources:` | (4) `BaseDirContext` |
| `project/format_css.rs` + `metadata_merge.rs` call sites | `css` | (3) layer_base marking |
| `project/mod.rs` fragment rebase; `extension/{paths,read}.rs` | extension-contributed theme/css/include-*/template/filters | (2) force-marked `Path` |
| `website_config.rs`, `website_post_render.rs` | `favicon`, navbar logo / footer image copy | project-root by construction (`_quarto.yml`-only keys) |
| `discovery.rs`, `project_resources.rs`, `sidebar_auto.rs`, `quarto-sass/src/config.rs` | `project.render`, `project.resources`, sidebar `auto:`, `brand:` | project-root by construction |
| URL-space emitters (`link_rewrite`, `navbar/footer_render`, `website_favicon`, `transforms/format_css`, `example_embed`, listing `item.rs`) | emitted hrefs/srcs | `page_url_for` family (rule-2 exit) |
| `include_expansion.rs:689` (`resolve_include_target`) | `{{< include >}}` (markdown space) | project-root leading-`/` + includer-dir |

### VIOLATIONS (each tracked by a strand — do not fix one without checking its siblings)

| Site | Keys | Defect | Strand |
|---|---|---|---|
| `stage/stages/include_resolve.rs:499` | `include-in-header`/`-before-body`/`-after-body` | consuming-doc-dir join; leading `/` OS-absolute | bd-oejuizi9, bd-rdcvjy2s, GH #455 |
| `quarto-sass/src/themes.rs:469` (+ `compile_theme_css.rs`, `revealjs/theme.rs`) | `theme` custom scss | same | bd-oejuizi9, bd-rdcvjy2s |
| `stage/stages/apply_template.rs:199,415` | `template`, `template-partials` | same | bd-rdcvjy2s (base-dir fix: file under bd-hjv5o) |
| `filter_resolve.rs:255-273` | `filters` | same | bd-rdcvjy2s (base-dir fix: file under bd-hjv5o) |
| `transforms/title_banner.rs:200` | `title-block-banner` image probe | doc-dir probe, raw string emitted | bd-hjv5o scope |
| `project_resources.rs:721,835` | engine/filter-declared resources | doc-dir join | bd-hjv5o scope |
| `pampa/src/citeproc_filter.rs:133,151` | `csl`, `bibliography` | **process-CWD** read (no base at all) | bd-oqoozmtr |

Related open strands: bd-hjv5o (the generalization audit this table
operationalizes), bd-r1y48cx0 (`css:` copy — possibly resolved by
`37758160`; verify end-to-end before closing).

## How to audit (the sweep that works)

Do **not** enumerate implementations of the convention (grepping for
leading-`/` handlers is structurally blind to sites that do a bare
`Path::join` and handle nothing — exactly how #524's survey missed #455).
Enumerate **consumers** and check each against this table:

- `rg 'runtime\.file_read|fs::read' crates/quarto-core crates/pampa crates/quarto-sass`
  — every hit that reads a config-originated path must trace to a blessed
  mechanism.
- `rg '\.join\(' crates/quarto-core/src/stage crates/quarto-core/src/transforms`
  — joins whose right-hand side derives from `as_plain_text()`/`as_str()`
  on a config value are suspect unless the value is `Path`-kind (mechanism 2
  output) or the site is a blessed resolver.
- Cross-check the key lists in the marking tables against this inventory.

If a bug report mentions a path resolving against the wrong directory,
assume it is an instance of this class: verify the *sibling* keys in the
same table row before scoping the fix to the reported key.

## History / references

- `claude-notes/research/2026-08-19-path-resolution-class-assessment.md` —
  why #524 missed #455; full evidence for the inventory; remediation plan.
- `claude-notes/plans/2026-05-20-bd-qor9a-metadata-path-resolution.md` —
  mechanism 1 and the deferred audit (bd-hjv5o).
- `claude-notes/plans/2026-08-13-site-root-relative-paths.md` — PR #524,
  the URL-space decree and its carve-outs.
- `claude-notes/plans/2026-02-17-dir-metadata-path-resolution.md` —
  mechanism 2 origin.
- `claude-notes/designs/body-link-resolution-contract.md` — URL-space
  resolution rules for body links.
- `claude-notes/designs/provenance-contract.md` — SourceInfo provenance.
