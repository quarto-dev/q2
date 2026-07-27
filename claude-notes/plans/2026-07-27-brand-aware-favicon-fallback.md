# Brand-aware favicon fallback (bd-97yc / bd-1elkd)

**Date:** 2026-07-27
**Braid:** bd-97yc (P4, feature, filed 2026-04-27) — and its later restatement
**bd-1elkd** (P3, feature, filed 2026-05-21), which says "Likely closes bd-97yc."
**Branch:** `main` @ `dd87a8b5` (investigation committed here; no worktree created)
**Status:** Investigation — pending design alignment with user. **Do not start
implementation until the user gives the go-ahead.**

## Triage verdict

**Ready to design, with one scope decision that materially changes the size of
the work** — the code seam is clear and small (a favicon-source fallback in
`website_config.rs` + a place to resolve `Brand` once), but Q2 currently has
**no `_brand.yml` auto-discovery**, while Q1's favicon fallback fires precisely
for projects that never wrote a `brand:` key. Deciding whether auto-discovery is
in scope is the difference between a ~1-phase change and a ~3-phase change that
touches every brand consumer.

Also: **bd-97yc and bd-1elkd are the same issue**, filed twice. One of them
should be closed as a duplicate before work starts (see design question 5).

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

### Obstacle 3 — Q2 has no `_brand.yml` auto-discovery (the scope question)

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

So a straight port of Q1's fallback would fire only for the subset of projects
that wrote `brand:` explicitly — a strictly narrower behavior than Q1's. Q1's
`brand: false` opt-out has no Q2 equivalent either.

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

## Proposed phases (draft)

Skeleton only — contents wait on the design discussion below, and phase count
depends on the answer to question 1.

- **Phase 0 — Test plan (TDD, failing first).**
  - `website_config.rs` unit tests: brand fallback used iff `website.favicon`
    absent; explicit `website.favicon` always wins; no brand → `None`.
  - `quarto-core/tests/integration/website_post_render.rs`: end-to-end sibling
    of the existing tests 31/32 — `<link>` emitted with the right *page-relative*
    href on a nested page, and the logo file copied into `_site/`.
  - Brand in a subdirectory (`brand: _brand/_brand.yml`) — the rebasing case.
  - `logo.small` as a light/dark pair → no favicon, no warning (deferred to
    bd-v5z8w).
  - External `logo.small` URL → `<link>` emitted, **no** copy attempted.
- **Phase 1 — Get a resolved `Brand` into quarto-core** (seam chosen in
  question 2), including the project-relative rebasing of logo paths
  (question 3).
- **Phase 2 — Fallback in `website_config.rs`** + the two call sites.
- **Phase 3 (conditional on question 1) — `_brand.yml` auto-discovery**, shared
  by the theme path and this one, plus a `brand: false` opt-out.
- **Phase 4 — Docs.** `docs/guides/authoring/brand.qmd` already documents a lot
  of Q1 brand behavior; the favicon fallback needs a line, and the
  auto-discovery answer needs the config-surface section updated either way.
- **Phase 5 — E2E verification** through `cargo run --bin q2 -- render` on the
  committed repro fixture, output inspected and recorded.

## Open design questions for the user

1. **Is `_brand.yml` auto-discovery in scope here, or a separate strand?**
   Q1's fallback fires for projects with no `brand:` key at all; Q2's would not.
   Porting the fallback without discovery gives a feature that is technically
   Q1-compatible but almost never observable. My recommendation: file
   auto-discovery as its own strand (it changes *theme* behavior too, so it is
   not a favicon change), implement the fallback now against the explicit
   `brand:` key, and note the gap in the docs. But if you consider the fallback
   pointless without discovery, we should do them together and this becomes a
   bigger change.

2. **Which seam carries the resolved `Brand` into quarto-core?**
   (a) inject a derived `website.favicon` into project config metadata — smallest
   diff, both call sites unchanged, but writes a computed value into the user's
   config tree; or (b) add `Option<Brand>` + `brand_dir` to `ProjectContext`
   and thread it through `website_config`'s readers — bigger diff, but it is the
   seam bd-hp3tx (navbar logo) needs anyway, and keeps derived data out of the
   config. I lean (b) for exactly that reason, but (a) is defensible if you want
   this to stay a P3/P4-sized change.

3. **Where does brand-relative → project-relative path rebasing belong?**
   Q1 does it inside `Brand` (`resolvePath` uses `projectDir`/`brandDir` fields
   set at construction). Q2's `quarto-brand` is deliberately path-agnostic and
   its one existing consumer passes an explicit prefix. Options: give
   `quarto-brand` an optional `brand_dir`/`project_dir` like Q1; add a
   `favicon_relative_to(project_dir)` helper; or do the rebasing at the
   quarto-core call site from `ResolvedThemeConfig::brand_dir`. The third keeps
   `quarto-brand` clean but duplicates a rule bd-hp3tx will need too.

4. **External logo URLs.** If `logo.small` is `https://…`, Q1 emits the `<link>`
   and copies nothing. Q2's `copy_favicon` has no external-path notion. Confirm
   "emit the link, skip the copy" and I'll add the guard — or should an external
   brand logo simply not be used as a favicon fallback at all?

5. **bd-97yc vs bd-1elkd — which one survives?** They are the same feature; the
   later one (bd-1elkd, P3) has the accurate description and the earlier one
   (bd-97yc, P4) has the stale "Q2 doesn't have brand support yet" premise and
   the useful `discovered-from`/`parent-child` edges. My suggestion: keep
   **bd-97yc** (it carries the epic edges), fold bd-1elkd's description into it,
   and close bd-1elkd as `duplicates` bd-97yc. Say the word and I'll do it —
   I have not closed anything.

## Risks / tradeoffs (draft)

- **The easy test is the misleading one.** With `_brand.yml` at the project
  root, brand-relative and project-relative paths coincide, so a fallback with
  no rebasing passes the obvious test and breaks on `brand: _brand/_brand.yml`.
  The subdirectory case must be in Phase 0, not discovered later.
- **Asymmetric contexts.** `WebsiteFaviconTransform` has no runtime;
  `copy_favicon` has one. Any design that "just reads the brand file" works in
  one call site and not the other. This asymmetry is the real content of
  question 2.
- **WASM.** `copy_favicon` is `#[cfg(not(target_arch = "wasm32"))]`; the
  transform is not. Whatever carries the brand must compile for
  `wasm32-unknown-unknown` (hub-client / `q2 preview`). `quarto-brand` already
  does — it is in `quarto-sass`'s tree, which builds for WASM — but this needs
  `cargo xtask verify` (full, not `--skip-hub-build`) before push.
- **Light/dark.** `Brand::favicon()` returns `None` for a `logo.small`
  light/dark pair by design, deferring the choice to the caller. Doing anything
  other than "no favicon" here would front-run bd-v5z8w.
- **Docs drift, noted in passing (not this strand's job).**
  `docs/guides/authoring/brand.qmd` appears to be largely a port of Q1's brand
  documentation and describes behavior Q2 may not have — e.g. the
  `{{< brand logo … >}}` shortcode (line 734ff) and navbar logo suppression via
  `_quarto.yml` (line 483ff), the latter being bd-hp3tx, which is open. Worth a
  separate audit strand; flagged here so it isn't lost.
