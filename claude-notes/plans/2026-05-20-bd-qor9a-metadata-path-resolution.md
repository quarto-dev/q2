---
title: 'bd-qor9a — Resolve metadata paths relative to where they were declared'
date: 2026-05-20
description: 'Resolves sidebar, navbar and footer hrefs relative to the file where they were written, instead of always treating them as project-root-relative, so missing-document warnings disappear.'
status: in-progress  # In progress. bd-8d6rk landed; implementation under way.
braid:
  strand: bd-qor9a
  priority: P2
---

**Blocked by**: bd-8d6rk (structured navigation diagnostics)
**Related design precedent**: `claude-notes/plans/2026-02-17-dir-metadata-path-resolution.md`

## Reproducer

`docs/guide/index.qmd`:
```yaml
---
title: Guide
sidebar:
  contents:
    - text: "Introduction"
      href: introduction.qmd
    - text: "Markdown"
      href: ../authoring/markdown/index.qmd
---
```

Both targets exist relative to the file the YAML was written in:

- `introduction.qmd` → `docs/guide/introduction.qmd` ✓
- `../authoring/markdown/index.qmd` → `docs/authoring/markdown/index.qmd` ✓

Running `cargo run --bin q2 -- render docs/guide/index.qmd` emits:

```
Warning: Sidebar references missing document information for 'introduction.qmd'
Warning: Sidebar references missing document information for '../authoring/markdown/index.qmd'
```

…because `SidebarGenerateTransform` treats hrefs as project-root-relative
regardless of where the YAML was written.

## Root cause

Two cooperating losses of source context:

1. **`SidebarEntry::from_plain_string`** (`crates/quarto-navigation/src/sidebar.rs:252-262`)
   takes `&str` and constructs a `NavigationItem { href: Some(s.to_string()), … }`.
   The originating `ConfigValue.source_info` is dropped on the floor —
   the bare-string entry shape never sees it.

2. **`SidebarEntry::Section { href: Option<String>, … }`**
   (`crates/quarto-navigation/src/sidebar.rs:162-175`) and `NavigationItem.href`
   are typed as `String`/`Option<String>`. Even when `from_config_value`
   has the full `ConfigValue` in hand, the `href` field can only store
   the string. `source_info` is again lost.

Then in `SidebarGenerateTransform`
(`crates/quarto-core/src/transforms/sidebar_generate.rs:167`) and the
later `SidebarRenderTransform`, lookup is
`index.lookup_by_source(Path::new(h))` on the bare string — no way to
know whether `h` came from `_quarto.yml` (project-root-relative) or
from a doc frontmatter (doc-relative).

Same pattern affects:
- navbar items (`crates/quarto-navigation/src/navbar.rs`),
- page footer items (`crates/quarto-navigation/src/footer.rs`),
- page-nav (`crates/quarto-navigation/src/page_nav.rs`),
- and any future surface that reads `website.*` hrefs.

## Existing precedent: directory-metadata `!path` adjustment

`claude-notes/plans/2026-02-17-dir-metadata-path-resolution.md` already
solves the same conceptual problem for `_metadata.yml` files via
`adjust_paths_to_document_dir` in `crates/quarto-core/src/project.rs`.
That implementation:

- only adjusts `ConfigValueKind::Path` values (i.e. values authored as
  `!path foo.qmd`);
- recursively walks `Map` / `Array`;
- uses `pathdiff::diff_paths(metadata_dir.join(path), document_dir)` to
  rewrite the relative path to be doc-dir-relative.

This works for hand-tagged `!path` values in `_metadata.yml`, but it
does *not* help sidebar / navbar entries, because authors write plain
strings there (`- introduction.qmd`, `href: ../foo.qmd`), not
`!path`-tagged values. Asking users to add `!path` tags everywhere is
the verbose option we rejected upfront.

## Strategy (decided): hybrid — implicit `!path` semantics for known path-shaped keys

The sidebar/navbar/footer parsers *already know* which YAML keys are
paths: `href`, `file`, `contents` items (bare-string form), section
`href`. For those positions, we treat any string value as if it were
`!path`-tagged — retain its `SourceInfo`, then resolve at Generate time
against `dirname(source_info.file)`.

Properties:

- Authors keep writing the obvious YAML (`- introduction.qmd`). No tag
  burden.
- The resolution rule is the same one the directory-metadata code already
  uses (pathdiff against the metadata-dir).
- Bare strings *outside* known path-shaped positions are unaffected —
  no heuristic about "what looks like a path."
- The strategy degrades cleanly when `source_info.file` is unset
  (programmatically-constructed sidebars, tests): no rewriting happens,
  matching today's behaviour.

## Decisions locked in (post bd-8d6rk)

**Source-info access path.** `SourceInfo` only carries `FileId`. Path
lookup requires `SourceContext::get_file(file_id)`. Two places this
context lives today:

- `pampa::pandoc::ASTContext::source_context` — owned by
  `DocumentSource`/`DocumentAst` and passed to
  `TransformPipeline::execute` as `&doc.ast_context`.
- `crate::stage::data::DocumentSource::source_context` — already
  present, mirrors the same data.

`RenderContext` does **not** currently carry the source context.
**Phase 1 adds a `source_context: Option<&SourceContext>` field** and
wires it through `AstTransformsStage` from `&doc.ast_context.source_context`.
Defensive `Option` so unit tests can construct `RenderContext` without
a context (today's behaviour preserved).

**Type-level shape: paired `href_source` field (not a wrapper).** I
considered three shapes:

- (A) `href: Option<NavPath { raw, source }>` — wrap. Cleaner type but
  invasive across every consumer of `NavigationItem.href`.
- (B) Add a paired `href_source: SourceInfo` next to `href: Option<String>`.
  Defaults to `SourceInfo::default()` for back-compat. Minimal blast
  radius.
- (C) Carry the source_info only on the ConfigValue tree; resolve and
  discard at Generate time. Cleanest if we never want source-info on
  the parsed types, but defeats Phase 4's diagnostic-location goal.

Going with (B). The `href_source` field defaults to
`SourceInfo::default()` so all existing programmatic construction sites
(tests, in-memory navbar builders) keep compiling and run unchanged
through the resolution helper (which short-circuits when the file is
the anonymous default).

**Resolution timing: Generate.** Paths get resolved to project-root-
relative once, in `SidebarGenerateTransform` / `NavbarGenerateTransform` /
`PageFooterGenerateTransform`. The output ConfigValue stored at
`navigation.*` carries already-resolved hrefs, so the Render side
continues to assume project-root-relative input (today's contract).

**Why not "rewrite the ConfigValue tree before parsing"?** That would
keep all changes inside `_metadata.yml`-style path-adjustment territory.
But it would lose the source-info-on-href in the parsed `Sidebar`, which
Phase 4's diagnostic-location work needs. Carrying source_info on the
parsed type lets us pass it as the `location: Option<SourceInfo>` arg
that `resolve_href_for_html` already accepts (bd-8d6rk).

**Resolution helper.** New function in `quarto-core::transforms::navigation_href`
(co-located with the other path helpers):

```rust
/// Resolve an href that was written in YAML to its project-root-relative
/// form. `source` is the `SourceInfo` of the originating YAML scalar; we
/// use it to look up the source file in `source_context`, compute that
/// file's directory relative to `project_root`, and join with `raw`.
///
/// Returns `raw` unchanged when:
/// - `source` is `SourceInfo::default()` (anonymous / programmatically constructed),
/// - the source file lookup fails (FileId not in context),
/// - the path is external / fragment-only (delegates to existing classifier),
/// - the source file lives outside `project_root` (defensive — keep raw).
pub fn resolve_metadata_path(
    raw: &str,
    source: &SourceInfo,
    source_context: &SourceContext,
    project_root: &Path,
) -> String;
```

Internally calls `resolve_to_project_root(source_file_dir, raw)` (the
existing private helper) once we know the source-file directory.

## Working checklist

- [x] Phase 0.1: Integration test asserting `docs/guide/index.qmd`-style frontmatter sidebar resolves correctly (no Q-13-1 fires; hrefs become project-root-relative)
- [x] Phase 0.2: Regression test asserting `_quarto.yml`-rooted sidebar entries still resolve as project-root-relative (passing already)
- [x] Phase 0.3: Diagnostic-location test for genuinely-broken case (fails: location is None today)
- [x] Phase 1.1: `RenderContext` gains `source_context: Option<&SourceContext>` field
- [x] Phase 1.2: `AstTransformsStage` bridges `doc.ast_context.source_context` into `RenderContext`
- [x] Phase 2.1: `NavigationItem` gains `href_source: SourceInfo` field (default `SourceInfo::default()`)
- [x] Phase 2.2: `SidebarEntry::Section` gains paired `href_source: SourceInfo`
- [x] Phase 2.3: `NavigationItem::from_config_value` populates `href_source` from the ConfigValue's source_info
- [x] Phase 2.4: `SidebarEntry::from_config_value` / `from_plain_string` populates `href_source`
- [x] Phase 2.5: `to_config_value` round-trip preserves `href_source`
- [x] Phase 3.1: `resolve_metadata_path` helper in `navigation_href.rs`
- [x] Phase 3.2: `resolve_metadata_path` unit tests (frontmatter-rooted, \_quarto.yml-rooted, default/anonymous, external/fragment, outside-project-root edge case, leading-`/`, Substring chain)
- [x] Phase 3.3: `SidebarGenerateTransform` calls `resolve_metadata_path` on every href before storing `navigation.sidebar`
- [x] Phase 3.4: `NavbarGenerateTransform` wires `resolve_metadata_path` for left/right items and `logo_href` (Navbar gained paired `logo_href_source` field)
- [x] Phase 3.5: `FooterGenerateTransform` wires `resolve_metadata_path` for left/center/right `Items` regions
- [x] Phase 3.6: `PageNavGenerateTransform` derives prev/next from already-resolved `navigation.sidebar`, so no extra wiring needed here; `PageNavRenderTransform` updated to pass `href_source` through to the diagnostic helper

**Discovery during Phase 3 implementation:** `SourceInfo::default()` is
`Original { file_id: FileId(0), start: 0, end: 0 }`, but a real
document parsed via `ASTContext::with_filename` *also* uses
`FileId(0)` for its own contents (the file is the first registered in
its fresh `SourceContext`). The helper detects the
"programmatic default" case by checking full equality with
`SourceInfo::default()`, not by `FileId(0)` alone.
- [x] Phase 4.1: Resolution helpers (`resolve_href_for_html` callers) populate `location: Some(href_source)` so Q-13-1..7 diagnostics carry the source location (sidebar_render, navbar_render, footer_render, page_nav_render all migrated)
- [x] Phase 4.2: Diagnostic-location test case asserts `d.location.is_some()` (frontmatter_sidebar_missing_document_diagnostic_carries_location)
- [x] Phase 5.1: End-to-end `cargo run --bin q2 -- render docs/guide/index.qmd` produces zero warnings; rendered sidebar HTML contains `href="introduction.html"` and `href="../authoring/markdown/index.html"` (page-relativized correctly)
- [x] Phase 5.2: End-to-end deliberately-broken case (`frontmatter_sidebar_missing_document_diagnostic_carries_location` integration test) confirms Q-13-1 fires with `location.is_some()`
- [x] Phase 5.3: `cargo xtask verify --skip-hub-build` clean (all 12 steps green)
- [x] Phase 6: Generalization audit deferred to discovered-from issue **bd-hjv5o** (covers body-link source-info, AutoSpec paths, listing contents, format.html.css / theme / bibliography paths in frontmatter)

## High-level phases

### Phase 0 — Tests for the bug (failing first)

Pick the smallest possible reproducer fixture under
`crates/quarto-core/tests/` (or `crates/quarto/tests/smoke-all/`)
that exercises:

- A doc frontmatter sidebar with sibling-relative `href`s.
- A doc frontmatter sidebar with `../`-relative `href`s.
- A `_quarto.yml` sidebar with hrefs that should *stay*
  project-root-relative (regression guard).
- A bare-string contents entry (`- introduction.qmd`) in doc frontmatter.

Tests assert that `lookup_by_source` resolves correctly and that no
"missing document information" diagnostic fires.

The `docs/guide/index.qmd` reproducer becomes an end-to-end
smoke test in addition.

### Phase 1 — Carry SourceInfo on `href` in navigation types

Change the navigation types so href fields retain `SourceInfo`. Two
shapes to evaluate during planning iteration:

- **Option A**: replace `Option<String>` href with a small wrapper:
  ```rust
  pub struct NavPath {
      pub raw: String,
      pub source: SourceInfo,
  }
  ```
  Used uniformly across `NavigationItem`, `SidebarEntry::Section`, and
  any other surface. `source: SourceInfo::default()` for
  programmatically-constructed values (back-compat).
- **Option B**: keep href as `ConfigValue` instead of unwrapping to
  `String` during parse. `as_plain_text()` is still available when
  needed; `source_info` is reachable directly.

Recommendation: A. It keeps the navigation types ergonomic (no
ConfigValue plumbing in render code) and the wrapper is purpose-built
for the resolution call.

### Phase 2 — Resolve at Generate time

Add a helper on `RenderContext` (or as a free function in
`crates/quarto-core/src/transforms/navigation_href.rs`) that takes a
`NavPath` and returns a project-root-relative path:

```rust
fn resolve_nav_path(path: &NavPath, project_root: &Path) -> String {
    let source_file = path.source.file_path()?; // returns Option<&Path>
    let metadata_dir = source_file.parent().unwrap_or_else(|| Path::new(""));
    let project_rel_metadata_dir = metadata_dir.strip_prefix(project_root).unwrap_or(metadata_dir);
    resolve_to_project_root(
        &project_rel_metadata_dir.to_string_lossy(),
        &path.raw,
    )
}
```

(Sketch — final shape during implementation. `resolve_to_project_root`
already exists in `navigation_href.rs:259-294` and is exactly the join +
`..` / `.` normalizer we want.)

`SidebarGenerateTransform` calls this once per entry before
`index.lookup_by_source(...)`. `SidebarRenderTransform` no longer needs
to re-resolve — Generate already replaced the href with the
project-root-relative form (one-shot at Generate, consistent with the
Phase 2 / Phase 3 Generate-vs-Render split documented in the websites
epic).

### Phase 3 — Same wiring for navbar / page-footer / page-nav

Re-use the same `NavPath` type for navbar and footer items. The Generate
transforms for each surface (currently
`crates/quarto-core/src/transforms/navbar_generate.rs`, `footer_generate.rs`)
get the same `resolve_nav_path` call.

### Phase 4 — Fill in diagnostic locations

Once `NavPath.source` is plumbed end-to-end, the missing-document
warning helper from bd-8d6rk gets a `with_location(nav_path.source.clone())`
call. The Q-13-\* diagnostics now point at the exact YAML scalar that
introduced the broken reference.

Update the smoke tests under
`crates/quarto-core/tests/render_page_in_project.rs` to assert that the
diagnostic carries a `location` and that the location's file path is
the expected source (`docs/guide/index.qmd` vs `docs/_quarto.yml`
depending on the case).

### Phase 5 — End-to-end verification on the reproducer

```bash
cargo run --bin q2 -- render docs/guide/index.qmd
```

…should produce no `Sidebar references missing document information`
warning. The rendered HTML should contain working sidebar links to
`docs/guide/introduction.html` and `docs/authoring/markdown/index.html`
relativized correctly via `ResourceResolverContext::page_url_for`.

`--json` output for a deliberately-broken case (rename
`docs/guide/introduction.qmd` to `docs/guide/intro.qmd` temporarily)
should produce a `Q-13-1` diagnostic with `location` pointing at
`docs/guide/index.qmd:6:13` (or wherever the YAML scalar lives).

### Phase 6 — Generalization audit (deferred)

The same source-location-driven resolution would benefit:

- `format.html.css` / `theme` / `template` paths in doc frontmatter,
- `bibliography` paths,
- `include-in-header` / `include-before-body` /  `include-after-body`,
- listing `contents:` paths,
- crossref-resolve absolute-file references.

Many of these already go through `!path`-tagged paths via
`adjust_paths_to_document_dir`, so the gap is specifically *un*-tagged
string-shaped paths in frontmatter. Audit these surfaces and open
discovered-from issues for any that still use bare strings.

This phase is out of scope for the initial PR but worth listing so the
follow-up doesn't get lost.

## What is *not* in scope for bd-qor9a

- Migrating non-navigation diagnostics (theorem, crossref, attribution)
  to structured form — separate scope, see bd-8d6rk's "what is not in
  scope" section.
- Changing the project-root-relative interpretation of paths declared in
  `_quarto.yml`. Those keep working because their `source_info.file`
  *is* `_quarto.yml`, which lives at the project root, and the
  metadata_dir-of-source-file resolution gives back project-root for
  hrefs written there. Direct regression guard test required.
- Re-architecting `ConfigValueKind::Path` / `!path` tags. Existing
  machinery stays; this issue only widens the set of values treated *as
  if* they were `!path`.

## Open questions for iteration

1. **Wrapper type or keep ConfigValue?** Recommendation: `NavPath { raw, source }`
   as a purpose-built wrapper. Cleaner ergonomics in render code.

2. **Where does the resolution actually happen — at parse
   (`SidebarEntry::from_config_value`) or at Generate (`SidebarGenerateTransform`)?**
   Recommendation: at Generate. The parser doesn't know the project root;
   the Generate transform does. Keeping resolution at Generate also means
   tests that construct sidebars in-memory don't have to fake a
   project root.

3. **What does `SourceInfo::file_path()` return when source_info was
   never set (in-memory programmatic construction)?** It returns `None`,
   in which case the helper returns `path.raw` unchanged (today's
   behaviour). Confirm this is the actual API of `quarto_source_map::SourceInfo`
   during implementation.

4. **Does the existing `adjust_paths_to_document_dir` still apply to
   sidebar entries when the sidebar is declared in `_metadata.yml`?**
   Probably yes (the directory-metadata merge runs over the full config
   tree before sidebar parsing), but worth a test case to confirm we
   don't double-resolve.

5. **Should we extend this to `auto:` paths too?**
   `AutoSpec::Paths(Vec<String>)` carries glob/path roots that today are
   project-root-relative-only. Same source-info argument applies. Lean
   yes, with regression guards.
