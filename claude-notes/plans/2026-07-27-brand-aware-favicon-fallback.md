# Brand-aware favicon fallback (bd-97yc)

**Date:** 2026-07-27
**Braid:** bd-97yc (feature, filed 2026-04-27, raised to P3). Its duplicate
**bd-1elkd** was closed 2026-07-27 with a `duplicates → bd-97yc` edge and its
description folded into bd-97yc.
**Branch:** `main`, based at `dd87a8b5` (no worktree created)
**Status:** **Implemented**, Phases 0–5 complete. Unpushed.

## What shipped

In a `website` project with a `brand:` key, the brand's `logo.small` becomes the
favicon when `website.favicon` is unset — the `<link rel="icon">` on every page
and the file copy into the output tree. Paths written relative to the
`_brand.yml` are rebased to project-relative form, URLs are linked but never
copied, and an explicit `website.favicon` always wins.

Six commits on `main`:

| Commit | Phase |
| --- | --- |
| `419e08f1` | 0 — failing tests |
| `1a223f6b` | 1 — resolve the project-level brand at config-parse time |
| `45287ff1` | 2 — rebase brand-relative logo paths |
| `6882d57c` | 3 — the fallback itself |
| `242aa4e5` | 4–5 — docs + E2E verification |
| *(pending)* | self-review fix: diagnostics name the right config key |

Three things surfaced during the work that the design discussion had not
anticipated, each recorded in its phase below:

1. **The fallback needed an explicit project-kind gate.** Every sibling
   transform gates itself implicitly by reading a `website.*` key; this one
   fires on a key's *absence*, so a default project using `_brand.yml` for
   theming alone would have sprouted a favicon. Test 47.
2. **An explicit `website.favicon: https://…` was already broken** — mangled to
   `../https:/example.com/f.ico` by the page-relative resolver. The URL guard
   went into the shared reader, so both paths are fixed.
3. **The missing-file warning blamed a key the user never wrote.** Caught in
   pre-commit self-review; the favicon now carries its origin. Test 48.

## Triage verdict (at investigation time)

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

## Work items

Implementation started 2026-07-27. Phases run in order; each ends green
(`cargo nextest run --workspace`) before the next begins.

### Phase 0 — Tests (TDD, failing first) ✅

Tests 40–46 in `crates/quarto-core/tests/integration/website_post_render.rs`.

- [x] Test 40 — brand `logo.small` → `<link rel="icon">` when `website.favicon`
      is unset, correct page-relative href on a nested page — **fails at HEAD**
- [x] Test 41 — the logo file is copied into `_site/` — **fails at HEAD**
- [x] Test 42 — brand in a subdirectory (`brand: _brand/_brand.yml`), the
      rebasing case — **fails at HEAD**
- [x] Test 45 — external `logo.small` URL → `<link>` emitted verbatim, **no**
      copy attempted — **fails at HEAD**
- [x] Test 43 — explicit `website.favicon` still wins over the brand logo —
      *passes at HEAD* (guard: must keep passing)
- [x] Test 44 — `logo.small` as a light/dark pair → no favicon, no diagnostic
      (deferred to bd-v5z8w) — *passes vacuously at HEAD* (guard)
- [x] Test 46 — no `brand:` key → unchanged behavior — *passes vacuously at
      HEAD* (guard)

**Baseline run** (`cargo nextest run -p quarto-core -E 'binary(integration) &
test(website_post_render::)'`): `20 tests run: 16 passed, 4 failed`. All four
failures are the fallback cases, each with the same message shape:

```
pipeline_brand_favicon_fallback_link_emitted_per_page
  index should use the brand's small logo as favicon: <no rel="icon" line>
pipeline_brand_favicon_fallback_rebases_subdirectory_brand
  brand-relative logo path must be rebased to project-relative: <no rel="icon" line>
pipeline_brand_favicon_external_url_emits_link_without_copy
  external brand logo URL must be emitted verbatim: <no rel="icon" line>
pipeline_brand_favicon_fallback_file_copied_to_output_dir
  brand logo was not copied to _site/
```

`<no rel="icon" line>` is the *expected* failure mode — no fallback exists, so
no favicon is emitted at all. No pre-existing test regressed.

> **Note on the three guards.** 44 and 46 pass *vacuously* today (nothing emits
> a favicon, so "no favicon" is trivially true). They are not evidence of
> correct behavior yet — they earn their keep only after Phase 3, when
> something could plausibly emit the wrong thing. 43 is a real pass: the
> explicit-favicon path already works and must stay byte-identical.

**Discovered while writing test 45** — `apply_favicon` runs the favicon path
through `ResourceResolverContext::page_url_for`, which does
`site_root.join(path)` + `pathdiff`. For an absolute URL that yields garbage
(`../https:/example.com/logo.png`), so **the external-URL guard is needed in
the `<link>` emission too, not just in `copy_favicon`**. This also affects an
explicit `website.favicon: https://…` today — a pre-existing bug on the same
line of code. Fixing it in the shared reader fixes both; that is the natural
place, not a widening of scope. Test 45 pins the brand half; add an
explicit-`website.favicon` external case in Phase 3.

### Phase 1 — Resolved brand on the project config ✅

- [x] Add `quarto-brand` as a dependency of `quarto-core`.
- [x] `ResolvedBrand { brand, dir }` in **`quarto-brand`**
      (`crates/quarto-brand/src/resolved.rs`).
- [x] `quarto_sass::resolve_brand(config, runtime, base_dir)` — one entry point
      for "what brand does this config name?", reusing the existing
      `extract_brand_ref` rules; `resolve_brand_layers` refactored onto it.
- [x] `ProjectConfig::brand: Option<ResolvedBrand>`, resolved in `parse_config`.
- [x] Failure is silent here; the theme stage keeps the diagnostic (note 1).
- [x] Unit tests (`project::tests::project_brand`, 5/5 pass): no key → `None`;
      root brand → `dir` = project root; subdirectory brand → `dir` =
      the subdirectory; inline block → `dir` = `None`; unresolvable brand →
      `discover` succeeds with `None`.

**Two design choices worth recording.**

*`ProjectConfig`, not `ProjectContext`.* `ProjectContext` has **192**
struct-literal construction sites across the workspace; `ProjectConfig` has 9,
all but one already using `..Default::default()`. So the field costs one real
edit instead of 192 mechanical ones. It is also the better semantic home — the
resolved brand *is* parsed project configuration, sitting next to `metadata`
and `config_path`, and `parse_config` is the single place a `_quarto.yml`
becomes a `ProjectConfig`, so "`config.brand` agrees with `config.metadata`'s
`brand:` key" holds by construction.

*`ResolvedBrand` lives in `quarto-brand`, not `quarto-sass`.* "A brand plus
where it came from" is a brand concept; `quarto-sass` merely happens to be
where brand resolution currently lives. Putting it there also gives Phase 2's
rebasing helper its natural home — a method on `ResolvedBrand`, which already
knows its own `dir`. That is what makes "no directory fields on `Brand`" work
in practice: the directory lives on the *resolution result*.

**Not covered — `q2 preview` / hub-client.** The fallback follows wherever
`parse_config` runs. `create_wasm_project_context`
(`crates/wasm-quarto-hub-client/src/lib.rs:643`) builds a single-file
pseudo-project with `ProjectConfig::default()`, so it has no project brand and
will not get the fallback. Single-file renders having no *project* brand is
correct, but whether the hub-client's **project** render path reaches
`parse_config` is unverified. Related: bd-wjg4h (browser-verify brand under
preview) and bd-k5rxujiy (preview asset walker misses meta-driven images).
Flagged in Phase 5 rather than assumed.

### Phase 2 — Rebasing helper in `quarto-brand` ✅

- [x] `quarto_util::is_external_url` — one shared predicate (6 unit tests).
- [x] `ResolvedBrand::path_prefix_relative_to` /
      `logo_resource_relative_to` / `favicon_relative_to`.
- [x] `LogoEntry::single()` exposed so a rebased logo keeps its alt text;
      `single_path()` reimplemented on top of it.
- [x] 18 unit tests in `crates/quarto-brand/tests/integration/resolved_test.rs`
      (50/50 in the crate pass): root, subdirectory, nested subdirectory,
      logo path with its own subdirectory, sibling directory (upward `..`),
      inline brand, external URL, protocol-relative URL, rooted path, no small
      logo, light/dark pair, named logo with alt, `logo.images.*`, unknown
      name, and the three prefix cases.

**Built on what was already there, rather than beside it.** Two discoveries
changed the shape of this phase:

- `BrandLogoResource::with_path_relative_to(base)` already existed and already
  encoded "URLs and rooted paths pass through untouched" — and it takes a
  *prefix*, exactly Q1's `join(pathPrefix, entry)` model. So the new code
  computes the prefix (`relative(projectDir, brandDir)`, Q1's
  `brand.ts:248`) and delegates, instead of reimplementing the rule.
- `quarto-brand` had a **private** `is_external_url` doing
  `http://`/`https://`/`//`. That is now the shared
  `quarto_util::is_external_url`, so brand paths and the rest of the tree
  agree.

**Why the predicate is not Q1's.** Q1 uses `/^\w+:/`
(`external-sources/quarto-cli/src/core/url.ts:13`), which also matches a
Windows drive letter — `C:\logos\brand.png` would be classified as a URL and
emitted into HTML unrebased. Requiring a scheme of **two or more** characters
costs nothing (no real scheme is one character) and removes the trap. Pinned by
`external_url_does_not_match_windows_drive_letters`.

**Deliberately not done here — bd-v2wgzz0h.** `adjust_paths_recursive` (the
`ConfigValueKind::Path` rebaser) still inlines its own `http://`/`https://`
check, so a `data:` URI in a `!path` value gets mangled by `pathdiff`.
Switching it to the shared predicate would fix that, but it changes behavior
for extension templates / filters / css paths — unrelated to the favicon, and
worth its own verification. Filed as a follow-up rather than bundled in.

**Bonus for bd-hp3tx.** `logo_resource_relative_to(name, project_dir)` returns
a full `BrandLogoResource` — rebased path *and* alt text — and reaches both
named logos (`small`/`medium`/`large`) and `logo.images.*`. The navbar work
should not need to touch this crate again.

*Also caught in self-review:* the first version chained
`.and_then(single).or_else(logo_image)`, which meant a named size that existed
but was a light/dark pair fell through to an `images` entry of the same name.
The two are separate namespaces in Q1 (`getLogo` vs `getLogoResource`), so the
lookup now matches on which namespace the name is in and does not fall through.
Pinned by `light_dark_named_size_does_not_fall_through_to_images`.

### Phase 3 — The fallback itself ✅

- [x] `website_config::resolved_website_favicon(meta, project)` — the single
      answer to "what is this site's favicon", covering precedence, the brand
      fallback, leading-slash normalization, URL passthrough, and project-kind
      gating.
- [x] `WebsiteFaviconTransform` consumes it; `apply_favicon` now takes the
      resolved value instead of re-reading the key.
- [x] `copy_favicon` consumes it, with an external-URL guard.
- [x] 14 unit tests for `resolved_website_favicon`; the 11
      `apply_favicon` unit tests reworked to be about link *emission* only,
      plus 2 new URL cases.
- [x] **Test 47** (new): a *default* project with a brand emits no favicon.
- [x] `cargo nextest run --workspace`: **10577 passed, 0 failed**. All four
      Phase 0 failures now pass.

**One function, not two edits.** `website_config.rs` was already documented as
the one place `website.*` keys are read (Phase 7 Decision 7). Adding
`resolved_website_favicon` there means the `<link>` emitter and the file copier
cannot drift on precedence, normalization, or what counts as a URL — they ask
the same function. `website_favicon` (the raw key read) stays as-is.

**The gate that wasn't obvious.** Every other Phase-7 per-page transform gates
itself *implicitly*, by reading a `website.*` key a default project doesn't
have. The brand fallback fires on the **absence** of `website.favicon`, so it
has no such key — without an explicit `ProjectKind::Website` check, any default
project using `_brand.yml` for theming would have started emitting a favicon
and copying the logo. Existing test 39 could not catch this (its default
project has no brand), so test 47 was added first. The explicit
`website.favicon` key is deliberately *not* gated — it already says what it
means, and gating it would be a regression.

**Caught in self-review: the missing-file warning blamed the wrong key.**
`copy_favicon`'s warning was hardcoded to `website.favicon refers to missing
file '…'`. Under the fallback that key doesn't exist anywhere in the project,
so a typo'd brand logo would have sent the reader hunting for a `website.favicon`
they never wrote. `resolved_website_favicon` now returns a `ResolvedFavicon`
carrying a `FaviconOrigin` (`WebsiteFavicon` | `BrandLogo`), and the warning
names the actual source — `the brand's logo.small refers to missing file
'gone.png'`. Pinned by **test 48**, written failing first; it asserts both that
the missing file is named *and* that `website.favicon` is not mentioned.

**Pre-existing bug fixed on the way.** An explicit
`website.favicon: https://example.com/f.ico` used to be run through
`ResourceResolverContext::page_url_for`, which does `site_root.join(..)` +
`pathdiff` and emitted `../https:/example.com/f.ico`. The URL guard lives in
the shared reader, so both the explicit and the brand path are now correct.
Pinned by `explicit_external_url_passes_through` and
`favicon_external_url_bypasses_the_resolver`. Protocol-relative URLs get a
second guard: skipping `normalize_favicon_path` for URLs stops `//host/f.ico`
from being flattened into the site-rooted `/host/f.ico`.

### Phase 4 — Docs ✅

- [x] New "Brand logo as favicon" section (`#brand-favicon`) in
      `docs/guides/authoring/brand.qmd`: the fallback, `website.favicon`
      precedence, brand-relative paths, URLs, and the two no-favicon cases
      (light/dark pair, non-website project).
- [x] Corrected the logo-preference table row from `website`/`book` to
      `website`. Q2's fallback gates on `ProjectKind::Website`, and the book
      project type is explicitly out of the websites-epic MVP — the row
      described Q1. This one line is in scope because it documents *this*
      feature; the page-wide "Q1 or Q2?" audit is bd-qnylgu69.
- [x] Rendered with Q2 (`cargo run --bin q2 -- render docs/guides/authoring/brand.qmd`)
      and the output inspected: section renders, `#brand-favicon` anchor
      exists, and the cross-link to `#light-and-dark-logos` resolves. The two
      `Q-13-4` warnings on that page are pre-existing broken links at lines 977
      and 1036, unrelated to this change.

### Phase 5 — Verification

- [x] E2E through `cargo run --bin q2 -- render` on three fixtures; output
      inspected and recorded in `repro-output.md`.
- [x] `cargo nextest run --workspace`: **10577 passed, 0 failed**.
- [x] Full `cargo xtask verify` (**not** `--skip-hub-build`): **all 14 steps
      passed**, including the WASM rebuild and the hub-client build + tests.
      This is the step that matters for the WASM risk below — `quarto-brand`
      and the new `ProjectConfig` field both compile for
      `wasm32-unknown-unknown`.
- [x] `cargo xtask lint`: all checks passed (883 files).
- [x] `cargo fmt --check`: clean.

**E2E results** (full transcript in
`brand-aware-favicon-fallback-investigation/repro-output.md`):

| Fixture | Result |
| --- | --- |
| `repro-site/` (brand at root) | `<link rel="icon" href="logo.png" type="image/png">`; `_site/logo.png` copied byte-identically |
| `subdir-brand-site/` (`brand: _brand/_brand.yml`) | root page `href="_brand/logo.png"`, nested page `href="../_brand/logo.png"`, file at `_site/_brand/logo.png` |
| `control-site/` (explicit `favicon.ico`) | `href="favicon.ico"`; `logo.png` neither linked nor copied |

The subdirectory fixture is checked by resolving the nested page's href against
the filesystem (`ls _site/docs/../_brand/logo.png`) — it lands on the copied
file, so the link a browser follows is real, not merely well-formed.

`control-site/` originally used `favicon: logo.png`, which after this change
proved nothing — the explicit key and the fallback would both have produced
`logo.png`. It now names a distinct file so precedence is observable.

### Resolved implementation notes

1. **Brand-resolution failure in `ProjectContext::discover` → resolve
   tolerantly; the theme path keeps owning the diagnostic.**

   The worry was that resolving brand twice is a smell. On inspection the two
   resolutions answer *different questions*, so it isn't duplication:

   - `CompileThemeCssStage` resolves from `doc.ast.meta` — the **merged**
     project + document metadata — because a document may set its own `brand:`
     in frontmatter and theme it individually
     (`compile_theme_css.rs:483-486`).
   - `ProjectContext` resolves from `project.config.metadata` — **project-level
     only**. That is precisely the right scope for a favicon, which is a
     site-wide artifact. Q1 agrees: it calls `project.resolveBrand()`, not the
     per-file variant (`website.ts:190`).

   Given they are different queries, `discover` stores `None` on failure rather
   than raising. The theme path already hard-errors on a missing or malformed
   `_brand.yml` with a source-located diagnostic
   (`compile_theme_css.rs:487-490`); duplicating that from `discover` would
   produce two errors for one mistake, and the earlier one would have the worse
   message. A project whose brand cannot be resolved therefore fails in the
   theme stage as it does today, and simply gets no favicon fallback along the
   way — which is moot, because the render fails anyway.

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
