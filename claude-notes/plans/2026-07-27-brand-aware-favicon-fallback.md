# Brand-aware favicon fallback (bd-97yc)

**Date:** 2026-07-27
**Braid:** bd-97yc (feature, filed 2026-04-27, raised to P3). Its duplicate
**bd-1elkd** was closed 2026-07-27 with a `duplicates → bd-97yc` edge and its
description folded into bd-97yc.
**Branch:** `main` @ `dd87a8b5` (investigation committed here; no worktree created)
**Status:** Design questions **answered** (2026-07-27, see below). Ready to
implement on the user's go-ahead.

## Triage verdict

**Ready to implement.** The code seam is clear and small: a favicon-source
fallback in `website_config.rs` plus a place to resolve `Brand` once. All five
design questions have been settled; the answers are recorded in
[§Design decisions](#design-decisions-settled-2026-07-27) below and the phase
list has been updated to match.

## Issue context

bd-97yc (2026-04-27, P4):

> Q1 falls back to `brand.light.favicon` when `website.favicon` is unset. Q2
> doesn't have brand support yet — file once brand lands. Q1 reference:
> `external-sources/quarto-cli/src/project/types/website/website.ts:185-205`.
> Originating phase: bd-b9mz.

bd-1elkd (2026-05-21, P3) restates it now that brand *has* landed:

> Now that Q2 has Brand data via quarto-brand, wire it into the favicon
> emission path. `Brand::favicon()` already returns the small logo's path.
> Likely closes bd-97yc.

The precondition in bd-97yc's description ("Q2 doesn't have brand support yet")
is **stale** — `crates/quarto-brand` landed 2026-05-20
(`claude-notes/plans/2026-05-20-brand-yml-support.md`), and that plan's
"out of scope" list explicitly hands logo wiring back to this strand.

## Dependency graph

```
bd-97yc  Brand-aware favicon fallback  [open]
  ├── parent-child  → bd-0tr6  Website projects epic  [open]
  ├── discovered-from → bd-b9mz  Phase 7 — Post-render (sitemap, favicon, site-url/title)  [closed]
  └── related ← bd-1elkd  Brand-aware favicon: read logo.small from _brand.yml  [open]
```

- **discovered-from bd-b9mz** (closed 2026-04-27) — this is the highest-value
  edge. bd-b9mz *built* the favicon path (per-page `<link rel="icon">`
  transform + post-render file copy) and filed bd-97yc as the "and here's the
  Q1 behavior we deliberately skipped" note. So the code this strand modifies
  is exactly the code bd-b9mz shipped, and the sub-plan
  `claude-notes/plans/2026-04-27-websites-phase-7.md` is the design of record
  for it (Decisions 3, 5, 7, 8).
- **parent-child bd-0tr6** (websites epic, open) — the parent is still open, so
  this can land on the epic's integration line or on `main`; per braid
  semantics the open parent does not block the child.
- **related bd-1elkd** — duplicate, see above.
- No **blocks** edges in either direction: nothing is waiting on this, which
  matches the P3/P4 priority. There is no urgency pressure.

Sibling brand follow-ups filed by the same 2026-05-20 plan, none of them
blocking but all sharing the same missing seam ("quarto-core has no access to a
resolved `Brand`"):

- **bd-hp3tx** — wire brand logo into the website navbar. *Same seam.* Whatever
  we build here to get a `Brand` into quarto-core is what bd-hp3tx will consume.
- **bd-v5z8w** — light/dark brand pairs, blocked on Q2's light/dark SCSS seam.
  Relevant because `Brand::favicon()` deliberately returns `None` for a
  `logo.small: {light, dark}` pair.
- **bd-rwxa0** — inline brand block not wired through the single-file path.
- **bd-wjg4h** — browser-verify brand under `q2 preview`.

## What the code looks like today

Every path named in the strand still exists and has the same shape. Nothing has
been refactored out from under it.

### The favicon path (built by bd-b9mz)

Three call sites, all reading through one centralized reader module:

| Site | File | Role |
| --- | --- | --- |
| `website_favicon(meta)` / `normalize_favicon_path(raw)` | `crates/quarto-core/src/project/website_config.rs:64,78` | the only readers of `website.favicon` |
| `WebsiteFaviconTransform` | `crates/quarto-core/src/transforms/website_favicon.rs:78` | per-page: appends `<link rel="icon" href=… type=…>` to `rendered.includes.header` |
| `copy_favicon` | `crates/quarto-core/src/project/website_post_render.rs:124` | post-render: copies `project.dir/<path>` → `output_dir/<path>`, warns if missing |

`website_config.rs` was written *specifically* to be the single place these
keys are read (its module doc says so, citing Phase 7 Decision 7). That is the
natural insertion point for a fallback: if `website_favicon()` returned the
brand's small logo when `website.favicon` is unset, both the `<link>` emission
and the file copy would pick it up with no further edits.

### The brand data (built by the 2026-05-20 plan)

- `quarto_brand::Brand::favicon()` (`crates/quarto-brand/src/resolve.rs:197`)
  already exists and mirrors Q1's `getFavicon`: `logo("small").single_path()`,
  returning `None` for a light/dark pair. Covered by
  `crates/quarto-brand/tests/integration/logo_test.rs:15-37`.
- `Brand` is resolved from a `BrandRef` by
  `quarto_sass::ThemeConfig::resolve(runtime, base_dir)`
  (`crates/quarto-sass/src/config.rs:247`), which also returns a **`brand_dir`**
  — the directory the `_brand.yml` was read from. That field is what makes
  path rebasing possible (see Obstacle 2).

### Obstacle 1 — quarto-core cannot currently see a `Brand`

`crates/quarto-core` has **no dependency on `quarto-brand`** (verified:
`grep -l quarto-brand */Cargo.toml` → only the root, `quarto-brand`, and
`quarto-sass`). The only place quarto-core touches brand at all is
`CompileThemeCssStage`, which calls `quarto_sass::resolve_brand_layers(...)`
(`crates/quarto-core/src/stage/stages/compile_theme_css.rs:307`) and throws the
`Brand` away — it only keeps the derived `SassLayer`s.

Worse for the transform: **`RenderContext` has no `runtime` field** (checked
every field in `crates/quarto-core/src/render.rs:207-340`). `WebsiteFaviconTransform`
therefore *cannot* read `_brand.yml` from disk itself, even if it wanted to.
`copy_favicon` *does* get a `&dyn SystemRuntime`, so the two call sites are
asymmetric.

This is the central design constraint: **the brand must be resolved somewhere
that has a runtime, once, and handed to quarto-core in a form both call sites
can read.** Candidate seams, in rough order of increasing blast radius:

1. **Inject `website.favicon` into project config metadata** at project setup,
   when brand supplies one and the user didn't. Both existing call sites then
   work unchanged and the change is nearly invisible. Cheapest; the cost is
   that a *derived* value is written into the user's config tree (a mild
   layering smell, and it must not leak back out into e.g. a config dump).
2. **Add a resolved `Option<Brand>` (+ `brand_dir`) to `ProjectContext`**,
   populated in `ProjectContext::discover` (which *does* take a runtime,
   `crates/quarto-core/src/project/mod.rs:443`). `website_config::website_favicon`
   then takes the brand as a second argument. This is the seam bd-hp3tx
   (navbar logo) will also need, so it may be worth paying for once.
3. **Resolve in `CompileThemeCssStage`** and stash on the context. Rejected on
   sight: per-document, ordering-fragile, and it is a *theme* stage.

### Obstacle 2 — the logo path is brand-relative, not project-relative

Q1's `Brand.resolvePath` (`external-sources/quarto-cli/src/core/brand/brand.ts:247`)
prefixes every logo path with `relative(projectDir, brandDir)`, so
`getFavicon()` returns a **project-relative** path. Q2's `Brand::favicon()`
returns the path **verbatim from the YAML**, with no rebasing — `quarto-brand`
has no notion of a project dir.

That is fine today (the only consumer, `brand_to_layers`, takes an explicit
`font_path_prefix`), but the favicon consumer needs the project-relative form
for both the `<link href>` and the `copy_favicon` source path. With
`brand: _brand.yml` at the project root the two coincide, which is exactly the
case that would pass a naive test and break for `brand: _brand/_brand.yml`.

Q1 also skips rebasing for external paths (`isExternalPath` → `http(s):`,
protocol-relative). Q2's `copy_favicon` would currently try to `file_copy` such
a URL.

### Obstacle 3 — Q2 has no `_brand.yml` auto-discovery (resolved: by design)

Q1 resolves a project brand even with **no `brand:` key at all**, probing four
paths under the project dir (`external-sources/quarto-cli/src/project/project-shared.ts:620-629`):

```
_brand.yml, _brand.yaml, _brand/_brand.yml, _brand/_brand.yaml
```

Q2 requires an explicit `brand:` key — `resolve_brand_layers` returns an empty
layer vec when `config.get("brand")` is absent
(`crates/quarto-sass/src/config.rs:333`), and there is no probing anywhere
(`grep '_brand\.ya\?ml'` over `quarto-core/src`, `quarto-sass/src`,
`quarto-brand/src` finds only doc comments and test fixtures). The
2026-05-20 plan documents the config surface as two explicit keys and never
mentions discovery.

So a straight port of Q1's fallback fires only for the subset of projects that
wrote `brand:` explicitly — strictly narrower than Q1. Q1's `brand: false`
opt-out has no Q2 equivalent either.

**Resolved (2026-07-27): this is intended, not a gap.** Q2 is deliberately
reducing auto-discovery relative to Q1, for two reasons: metadata merging is
more consistent and predictable, and the diagnostics machinery is good enough
that requiring `brand: _brand.yml` somewhere in the project is a fine thing to
assume and to report on. Auto-discovery may become worth it for some features
later; `_brand.yml` is not one of them yet. **Do not add discovery in this
strand, and do not file a strand for it.** The docs should state the
requirement explicitly — folded into the audit strand bd-qnylgu69.

### Repro at HEAD — confirmed

Pre-flight `cargo xtask verify --skip-hub-build` is **green at `dd87a8b5`** (all
14 steps), so what follows is a missing feature, not a broken tree.

Two fixtures are committed under
`claude-notes/plans/brand-aware-favicon-fallback-investigation/`, identical
except for one `_quarto.yml` line; full transcript in `repro-output.md`.

- **`repro-site/`** — `brand: _brand.yml` with `logo.small: logo.png`, no
  `website.favicon`. `_site/index.html` contains **no** `<link rel="icon">` and
  `_site/logo.png` is **not** written. The brand *was* resolved: the primary
  colour `#4b2e83` reaches `_site/site_libs/quarto/quarto-theme-*.css`.
- **`control-site/`** — same project plus `website.favicon: logo.png`. Emits
  `<link rel="icon" href="logo.png" type="image/png">` and copies
  `_site/logo.png`.

The control is the useful half: every piece of machinery bd-b9mz built already
works. The gap is *only* that `website_config::website_favicon()` reads one key
and gives up — which is why the fallback is a small change once a resolved
`Brand` is reachable (Obstacle 1). Both outputs were inspected directly;
`_site/` and `.quarto/` were removed before committing.

## Design decisions (settled 2026-07-27)

1. **Auto-discovery is out of scope, and no strand should be filed for it.**
   Q2 is deliberately reducing auto-discovery relative to Q1 — metadata merging
   is more consistent, and diagnostics are good enough that we can assume
   `brand: _brand.yml` appears somewhere in the project and report clearly when
   it doesn't. Implement the fallback against the explicit `brand:` key. See
   Obstacle 3 for the full rationale.

2. **Seam: option (b)** — add a resolved `Option<Brand>` (+ its `brand_dir`) to
   `ProjectContext`, populated in `ProjectContext::discover` (which already
   takes a `&dyn SystemRuntime`, `crates/quarto-core/src/project/mod.rs:443`),
   and thread it through `website_config`'s readers. Derived data stays out of
   the user's config tree, and bd-hp3tx (navbar logo) inherits the same seam.

3. **Rebasing: a pure helper in `quarto-brand`, no directory fields on `Brand`.**
   Confirmed compatible with the ConfigValue-`!path` future — see the study
   below.

4. **External logo URLs: emit the `<link>`, copy nothing.** `copy_favicon` needs
   an external-path guard. Note the existing `!path` rebaser already encodes
   exactly this rule (`http://` / `https://` skipped,
   `crates/quarto-core/src/project/mod.rs:236-239`), so reuse its predicate
   rather than writing a second one.

5. **bd-97yc survives**; bd-1elkd closed 2026-07-27 as a duplicate with a
   `duplicates → bd-97yc` edge, its description folded into bd-97yc, priority
   raised to P3. Done.

### Study: does the helper foreclose the ConfigValue-`!path` future?

**No — and the mechanism you have in mind already exists and works.** But it
cannot reach brand paths today, for a reason worth knowing before you commit.

**The machinery is real.** `ConfigValueKind::Path(String)` is a first-class
variant meaning "path to resolve relative to source file"
(`crates/quarto-pandoc-types/src/config_value.rs:204`). `adjust_paths_recursive`
(`crates/quarto-core/src/project/mod.rs:229-263`) walks a `ConfigValue` tree and
rebases every `Path` node, descending through `Array` and `Map` — so it *is*
composable over compound values, exactly as you hoped. It already skips
`http://`/`https://` and uses `quarto_util::is_rooted` rather than
`Path::is_absolute` (so a POSIX-absolute path isn't mangled on Windows), and it
emits forward slashes because the results land in HTML hrefs.

**And it is already used for precisely this kind of problem.** Extensions have
the same brand-shaped issue — a config file in one directory naming paths
relative to itself. `crates/quarto-core/src/extension/read.rs:246-300`
(`mark_path_valued_keys`) walks the parsed extension config and promotes known
path-valued keys (`template`, `template-partials`, `shortcodes`, `filters`) from
`Scalar` to `Path`; `MetadataMergeStage` then calls
`adjust_paths_to_document_dir(&mut config, &ext_dir, &document_dir)`
(`metadata_merge.rs:216-227`). That is the whole pattern, in production, today.

**The blocker is upstream of the rebaser: `_brand.yml` never becomes a
`ConfigValue`.** `quarto-brand`'s dependencies are `quarto-util`, `serde`,
`serde_yaml`, `thiserror` — no `quarto-pandoc-types`, no `quarto-source-map`.
`Brand::from_yaml_str` is a bare `serde_yaml::from_str`
(`crates/quarto-brand/src/lib.rs:27`). A `Brand` therefore carries **no
`SourceInfo`, no `FileId`, and no `Path` nodes at all**. Even the *inline*
brand-block case discards them: `extract_brand_ref` converts the ConfigValue
back down to a `serde_yaml::Value` via `config_value_to_yaml_value`
(`crates/quarto-sass/src/config.rs:421,452`) before handing it to serde.

So routing brand logo paths through the `!path` machinery means first parsing
`_brand.yml` into a `ConfigValue` and marking `logo.*` path-valued, à la
`mark_path_valued_keys` — a real change to `quarto-brand`'s shape, well beyond
this strand.

**Two caveats on the "reasons about where it came from" framing.** First, the
base directory is *caller-supplied*, not self-derived: the signature is
`adjust_paths_to_document_dir(metadata, metadata_dir, document_dir)`. Nothing
today reads a `SourceInfo`'s `FileId` back to a path to discover its own
directory. That is *possible* — `SourceContext` maps `FileId` → `SourceFile.path`
and `RenderContext` carries an `Option<&SourceContext>` — but nothing does it,
and `RenderContext`'s own docs say consumers must tolerate `None`. Second, the
existing rebaser targets the **document dir**, whereas `copy_favicon` needs a
**project-relative** path. Two consumers, two bases — see the risk below.

**Conclusion.** A pure `quarto-brand` helper is the right call and forecloses
nothing. It needs two inputs — the brand dir and the target dir — because
`Brand` has no directory of its own, and both are already available at the
quarto-core call site (`ResolvedThemeConfig::brand_dir`,
`crates/quarto-sass/src/config.rs:276-279`). So this satisfies "no directory
fields on `Brand`": the directory lives on the *resolution result*, where it
already lives today. When `_brand.yml` eventually parses into a `ConfigValue`
with `!path`-marked logo keys, the helper becomes a thin wrapper over the shared
rebaser or disappears; nothing about choosing it now makes that harder.

One consequence worth naming: the rebasing rule should be expressed once and
shared with bd-hp3tx, not written twice. If it lives in `quarto-brand` as
`logo_path_relative_to(...)` with `favicon_relative_to(...)` layered on top,
the navbar work gets it for free.

## Proposed phases

- **Phase 0 — Test plan (TDD, failing first).**
  - `website_config.rs` unit tests: brand fallback used iff `website.favicon`
    absent; explicit `website.favicon` always wins; no brand → `None`.
  - `quarto-brand` unit tests for the rebasing helper: same-dir, subdirectory,
    external URL, rooted path, Windows separators.
  - `quarto-core/tests/integration/website_post_render.rs`: end-to-end sibling
    of the existing tests 31/32 — `<link>` emitted with the right *page-relative*
    href on a nested page, and the logo file copied into `_site/`.
  - Brand in a subdirectory (`brand: _brand/_brand.yml`) — the rebasing case.
  - `logo.small` as a light/dark pair → no favicon, no warning (deferred to
    bd-v5z8w).
  - External `logo.small` URL → `<link>` emitted, **no** copy attempted.
- **Phase 1 — `Option<Brand>` + `brand_dir` on `ProjectContext`**, resolved once
  in `ProjectContext::discover`. Adds a `quarto-brand` dependency to
  `quarto-core`.
- **Phase 2 — Rebasing helper in `quarto-brand`**, shaped for reuse by bd-hp3tx,
  reusing the external-URL/rooted-path predicates from the `!path` rebaser.
- **Phase 3 — Fallback in `website_config::website_favicon`** + the two call
  sites, including `copy_favicon`'s external-path guard.
- **Phase 4 — Docs.** A line on the fallback in
  `docs/guides/authoring/brand.qmd`. The broader "does this page describe Q2 or
  Q1?" question — including stating that Q2 needs an explicit `brand:` key — is
  bd-qnylgu69, filed 2026-07-27, not this strand.
- **Phase 5 — E2E verification** through `cargo run --bin q2 -- render` on the
  committed repro fixture, output inspected and recorded here. Full
  `cargo xtask verify` (not `--skip-hub-build`) before push — see the WASM risk.

## Risks / tradeoffs

- **The easy test is the misleading one.** With `_brand.yml` at the project
  root, brand-relative and project-relative paths coincide, so a fallback with
  no rebasing passes the obvious test and breaks on `brand: _brand/_brand.yml`.
  The subdirectory case must be in Phase 0, not discovered later.
- **Two consumers, two bases.** The `<link href>` wants a *page-relative* URL
  (`WebsiteFaviconTransform` gets there via
  `ResourceResolverContext::page_url_for`, which expects a project-relative
  input); `copy_favicon` wants a *project-relative* source path. The existing
  `!path` rebaser targets the **document dir**, so it is not a drop-in for
  either. Settle on "the helper yields project-relative, and the existing
  page-relative resolver runs on top of it" — that keeps today's `<link>` path
  byte-identical for the explicit-`website.favicon` case, which the control
  render already pins.
- **Asymmetric contexts.** `WebsiteFaviconTransform` has no runtime;
  `copy_favicon` has one. Any design that "just reads the brand file" works in
  one call site and not the other. Resolved by decision 2 (resolve once in
  `ProjectContext::discover`).
- **WASM.** `copy_favicon` is `#[cfg(not(target_arch = "wasm32"))]`; the
  transform is not. Whatever carries the brand must compile for
  `wasm32-unknown-unknown` (hub-client / `q2 preview`). `quarto-brand` already
  does — it is in `quarto-sass`'s tree, which builds for WASM — but this needs
  `cargo xtask verify` (full, not `--skip-hub-build`) before push.
- **Light/dark.** `Brand::favicon()` returns `None` for a `logo.small`
  light/dark pair by design, deferring the choice to the caller. Doing anything
  other than "no favicon" here would front-run bd-v5z8w.
- **Docs drift — now tracked as bd-qnylgu69** (`related` to this strand).
  `docs/guides/authoring/brand.qmd` appears to be largely a port of Q1's brand
  documentation and describes behavior Q2 may not have — e.g. the
  `{{< brand logo … >}}` shortcode (line 734ff) and navbar logo suppression via
  `_quarto.yml` (line 483ff), the latter being bd-hp3tx, which is open. The
  audit strand also owns documenting that Q2 requires an explicit `brand:` key.
- **Possibly related, not investigated:** bd-k5rxujiy — "`q2 preview`: logo
  image (`logo: logo.svg`) 404s — asset walker misses meta-driven raw-HTML
  images". Same neighbourhood (a logo path that reaches HTML but whose file
  never gets copied for preview). If the favicon `<link>` shows the same
  symptom under `q2 preview`, that strand is the reason.
