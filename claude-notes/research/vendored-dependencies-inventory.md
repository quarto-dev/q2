# Vendored / non-cargo dependency inventory

This is the **source-of-truth catalogue** for every third-party asset
in the repo that is *not* a Cargo crate (and therefore not surfaced by
`cargo update`). It is consumed by the `upgrade-cargo-deps` skill (or
its successor) when running periodic dependency audits — each entry
tells the agent how to recognize the dep, where to look upstream, how
to update it, and what to verify after.

The inventory is **maintained by the audit run**: every survey
re-confirms the listed `Current version` / `Last reviewed` field for
each entry and adds new entries for any vendored asset discovered
during discovery (see `## Discovery strategies`).

---

## Discovery strategies

A future agent looking for non-cargo dependencies in the workspace
should run, in order:

1. **`include_dir!` / `include_str!` / `include_bytes!` sweep.** Any
   compile-time embed of files outside `crates/<self>/src/` is a
   candidate. Filter `target/`, `external-sources/`, `tests/`,
   `node_modules/`, `.worktrees/`:
   ```bash
   grep -rE 'include_(dir|str|bytes)!' --include='*.rs' \
     | grep -vE '(/tests/|/target/|external-sources|/node_modules|\.worktrees)'
   ```
   For each hit, inspect the path. If it points into `resources/`,
   `crates/<x>/resources/`, or anywhere else with a copy of an
   upstream artifact, it belongs in this inventory.

2. **`resources/` tree walk.** The repo-level `resources/` directory
   is the canonical home for vendored upstream artifacts (per
   `CLAUDE.md` § *External Sources Policy*). Every immediate
   subdirectory should either (a) appear in this inventory, or (b)
   have a one-line note in the survey plan explaining why it isn't
   tracked (e.g. our own assets, not vendored).

3. **Per-crate `resources/` directories.** Crates frequently keep
   their own `resources/` (`crates/pampa/resources/`,
   `crates/quarto-core/src/engine/knitr/resources/`,
   `crates/qmd-syntax-helper/resources/`,
   `crates/quarto-csl/test-data/`, etc.). Sweep:
   ```bash
   find crates -type d \( -name resources -o -name 'test-data' -o -name 'resources*' \) \
     -not -path '*/target/*'
   ```

4. **Provenance-headed files.** Vendored assets in this tree
   sometimes carry a header comment with `Source:`/`Vendored:` lines
   (see `resources/highlights/julia/highlights.scm`). Sweep:
   ```bash
   grep -rEln '^[#;/* ]*(Source|Vendored|Upstream|Copied from):' resources crates \
     --include='*.scm' --include='*.lua' --include='*.css' --include='*.scss' --include='*.R' --include='*.html'
   ```
   Cross-check each hit against this inventory.

5. **`hub-client/public/`** and other static-asset roots. Anything
   served verbatim by the web app is potentially a vendored library
   (e.g. `hub-client/public/reveal-menu/menu.css`). List the
   directory and check each file's first lines for license / version
   strings.

6. **Sub-package `package.json`s outside `hub-client/`.** Some Rust
   crates have a private npm sub-package whose bundled output is
   `include_str!`'d into Rust. Find them:
   ```bash
   find . -name package.json -not -path '*/node_modules/*' -not -path '*/dist/*' \
     -not -path '*/target/*' -not -path '*/.worktrees/*'
   ```
   Each one outside `hub-client/` and the workspace root should appear
   in this inventory or be explained in the survey plan.

7. **Tree-sitter vendored grammars.** Tree-sitter parser sources live
   under `crates/tree-sitter-*/` and `tree-sitter-*/grammar/`. They
   are derived from upstream tree-sitter projects; check for
   `parser.c`/`grammar.js` provenance comments and tag the upstream
   commit.

When discovery turns up a candidate **not** in this inventory:
1. Add an entry below using the template at the bottom of the file.
2. Note the addition in the survey plan's TL;DR.
3. If it has no clear update mechanism, file a braid strand (labels
   `deps`, `vendored`, priority `3`) tracking the gap.

### When discovery turns up *dead* scaffolding

A separate, important outcome of a vendored audit: discovery may
find files that *look* vendored but are actually dead weight
inherited from an old upstream fork — multi-language packaging
scaffolding for languages we don't build (`package.json`,
`setup.py`, `pyproject.toml`, `go.mod`, `Package.swift`,
`binding.gyp`, `CMakeLists.txt`, `Makefile`,
`bindings/{go,node,python,swift}/`, etc.) where the Rust crate's
`Cargo.toml` `include` list, `build.rs`, and any active workflow
make no reference to them.

**Delete it.** Leaving dead scaffolding in place misleads the
*next* audit run into re-classifying the crate as vendored, and
each cycle wastes a session on the same triage. Cleanup is in
scope for the audit, not over-stepping.

**Worked example — bd-7co9 (2026-05-04).** The
`crates/tree-sitter-qmd/` crate originated as a fork of MDeiml's
`tree-sitter-markdown`, but has been developed independently for a
long time. The first audit pass misclassified it as "vendored"
(see *Note on `tree-sitter-qmd`'s stale "fork" framing* under
entry H below).

The cleanup landed in two commits on
`.worktrees/7co9-fork-framing-cleanup`:

1. **Narrow metadata fixes** (the explicit issue scope): rewrite
   `README.md` to drop the "fork of" lede, rewrite `tree-sitter.json`
   metadata, delete `package.json`/`package-lock.json` and the
   verbatim upstream `README.tree-sitter-md.md`.
2. **Broad scaffolding deletion** (follow-up after user
   confirmation): delete `binding.gyp`, `CMakeLists.txt`,
   `CONTRIBUTING.md`, `go.mod`, `Makefile`, `Package.{swift,resolved}`,
   `pyproject.toml`, `setup.py`, `bindings/{go,node,python,swift}/`,
   `scripts/`, `common/common.mak`, plus a parallel cleanup under
   `tree-sitter-markdown/`.

Files kept: `LICENSE` (preserves the original MIT copyright as
required), `bindings/rust/`, `tree-sitter.json` (used by the
`tree-sitter` CLI), `common/common.js` and
`common/html_entities.json` (referenced by `Cargo.toml`'s
`include`), the grammar/queries/sources, and developer utilities
still referenced from source (e.g.
`tree-sitter-markdown/scripts/unicode-ranges.py` is mentioned in a
`grammar.js` comment).

Verification both commits: `cargo xtask verify --skip-hub-build
--skip-hub-tests` and `cargo xtask lint` (`--skip-hub-*` is fine
when the change is purely scaffolding outside any include path).

---

## Inventory

Each entry uses the following fields:

- **Path** — repo-relative path to the vendored artifact(s).
- **Upstream** — canonical upstream source (URL + version/commit if available).
- **Bundled via** — how the artifact reaches the binary (e.g. `include_dir!`, `include_bytes!`, copied at build time, served as static asset).
- **Consumed by** — the crate(s) that read it.
- **Update procedure** — terse, actionable steps. Where applicable, point to the README that lives next to the asset.
- **Verification** — what to run/inspect after an update to confirm nothing regressed.
- **License** — SPDX-style.
- **Current version** — the version we're at right now.
- **Last reviewed** — `YYYY-MM-DD` when the audit confirmed currency.

---

### A. Bootstrap SCSS (5.3.1)

- **Path:** `resources/scss/bootstrap/dist/{scss,sass-utils}/`,
  `resources/scss/bootstrap/themes/`,
  `resources/scss/bootstrap/_bootstrap-*.scss`,
  `resources/scss/html/templates/title-block.scss`
- **Upstream:** Bootstrap (https://github.com/twbs/bootstrap), v5.3.1.
  Re-vendored via `external-sources/quarto-cli/src/resources/formats/html/bootstrap/`
  (TypeScript Quarto's copy is the proximate source).
- **Bundled via:** `include_dir!` in `crates/quarto-sass/src/resources.rs`.
- **Consumed by:** `quarto-sass` for theme compilation.
- **Update procedure:** see `resources/scss/README.md` ("Updating"
  section). Copy from `external-sources/quarto-cli/...`, regenerate
  dart-sass fixtures, run parity tests.
- **Verification:** `cargo nextest run -p quarto-sass parity`, plus
  full `cargo xtask verify`.
- **License:** MIT (Bootstrap), MIT (Bootswatch themes).
- **Current version:** Bootstrap 5.3.1 (banner in
  `resources/scss/bootstrap/dist/scss/mixins/_banner.scss`).
- **Last reviewed:** 2026-05-04.

### B. Bootstrap Icons (1.13.1)

- **Path:** `resources/bootstrap-icons/{bootstrap-icons.css,bootstrap-icons.woff}`
- **Upstream:** Bootstrap Icons (https://icons.getbootstrap.com/),
  v1.13.1. Re-vendored from
  `external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/`.
- **Bundled via:** `include_bytes!` in
  `crates/quarto-core/src/transforms/website_bootstrap_icons.rs`.
- **Consumed by:** `quarto-core` website rendering (prev/next nav
  strip, any `bi-*` class).
- **Update procedure:** see `resources/bootstrap-icons/README.md`.
  Copy `bootstrap-icons.css` and `bootstrap-icons.woff` together (CSS
  references woff by hashed query string).
- **Verification:** website render (`cargo run --bin q2 -- render
  examples/websites/<fixture>`), inspect `_site/site_libs/bootstrap/`
  for the pair, and confirm icons render in browser.
- **License:** MIT.
- **Current version:** 1.13.1 (header comment in
  `bootstrap-icons.css`).
- **Last reviewed:** 2026-05-04. Tracking issue: bd-bsut.

### C. Quarto built-in extensions

- **Path:** `resources/extensions/quarto/{kbd,video,lipsum,version,placeholder}/`
- **Upstream:** mostly authored by Posit / Charles Teague / Carlos
  Scheidegger; vendored from quarto-cli's
  `src/resources/extensions/quarto/`. They have their own `version:`
  in `_extension.yml`.
- **Bundled via:** `include_dir!` in
  `crates/quarto-core/src/extension/mod.rs` and
  `crates/wasm-quarto-hub-client/src/lib.rs`.
- **Consumed by:** `quarto-core`, `wasm-quarto-hub-client`.
- **Update procedure:** copy each extension's directory from
  upstream quarto-cli (or their canonical GitHub source) on a
  per-extension basis. Bump the `version:` field in
  `_extension.yml` only if upstream has bumped.
- **Verification:** integration tests covering shortcodes; manual
  render of a fixture using each shortcode.
- **License:** MIT (per quarto-cli).
- **Current versions** (as of 2026-05-04):
  - `kbd` — no `version:` field
  - `video` — no `version:` field
  - `lipsum` — `1.0.2`, `quarto-required: ">=1.3.0"`
  - `version` — no `version:` field
  - `placeholder` — `0.0.1`, `quarto-required: ">=1.5.0"`
- **Last reviewed:** 2026-05-04.

### D. knitr R scripts

- **Path:** `crates/quarto-core/src/engine/knitr/resources/rmd/{execute,hooks,ojs_static,ojs,patch,rmd}.R`
- **Upstream:** quarto-cli's
  `src/resources/rmd/` (the R helper scripts that drive knitr). These
  are Posit-authored scripts, not the upstream knitr CRAN package.
- **Bundled via:** `include_dir!` in
  `crates/quarto-core/src/engine/knitr/mod.rs`.
- **Consumed by:** `quarto-core` knitr engine.
- **Update procedure:** copy from `external-sources/quarto-cli/src/resources/rmd/`
  when a knitr-engine fix lands upstream.
- **Verification:** `cargo nextest run -p quarto-core engine::knitr`,
  plus a real knitr render
  (`cargo run --bin q2 -- render <fixture>.Rmd`).
- **License:** GPL (matches quarto-cli's R-side licensing).
- **Current version:** track by quarto-cli git SHA at copy time;
  not currently recorded — **inventory gap**, see braid strand at
  the bottom of this file.
- **Last reviewed:** 2026-05-04.

### E. Pandoc HTML templates

- **Path:** `crates/pampa/resources/templates/html/{main.html,styles.html,styles.citations.html}`
- **Upstream:** Pandoc's default HTML5 template
  (https://github.com/jgm/pandoc/blob/main/data/templates/default.html5)
  and Quarto's HTML CSS partials.
- **Bundled via:** `include_str!` in
  `crates/pampa/src/template/builtin.rs`.
- **Consumed by:** `pampa` HTML rendering.
- **Update procedure:** when Pandoc bumps the default HTML5
  template, diff against `main.html` and pull selected changes.
  This is rarely a clean copy because Quarto-specific markers
  intersect.
- **Verification:** snapshot tests under `pampa`; manual render and
  diff against TS Quarto for a representative document.
- **License:** GPL (Pandoc) for the upstream template; the
  `styles.*.html` partials are derived from quarto-cli (MIT).
- **Current version:** not recorded by SHA — **inventory gap**.
- **Last reviewed:** 2026-05-04.

### F. CSL: chicago-author-date style

- **Path:** `crates/pampa/resources/csl/chicago-author-date.csl`
- **Upstream:** CSL styles repo
  (https://github.com/citation-style-language/styles), specific file
  `chicago-author-date.csl`. The `<updated>` field inside the file
  is the upstream date — currently `2025-08-07`.
- **Bundled via:** `include_str!` in
  `crates/pampa/src/citeproc_filter.rs` (`DEFAULT_CSL_STYLE`).
- **Consumed by:** `pampa` citation processing default style.
- **Update procedure:** copy the latest `chicago-author-date.csl`
  from the CSL styles repo. Compare `<updated>` timestamps.
- **Verification:** `cargo nextest run -p quarto-citeproc` and
  `cargo nextest run -p pampa citeproc`.
- **License:** CC-BY-SA-3.0 (CSL styles repo).
- **Current version:** `<updated>2025-08-07T00:00:00+00:00</updated>`.
- **Last reviewed:** 2026-05-04.

### G. Tree-sitter highlight queries

- **Path:** `resources/highlights/<lang>/highlights.scm`
- **Upstream:** per-language tree-sitter repos. Each file carries a
  provenance header (Source URL, Commit SHA, License, Vendored
  date). Currently only `julia` is vendored:
  `tree-sitter/tree-sitter-julia` @ `e0f9dcd180fdcfcfa8d79a3531e11d99e79321d3`.
- **Bundled via:** `include_str!` in
  `crates/quarto-highlight/src/langs/<lang>.rs`.
- **Consumed by:** `quarto-highlight` for syntax highlighting.
- **Update procedure:** check upstream tree-sitter repo for the
  language; if it has updated `queries/highlights.scm`, copy and
  update the header (Commit, Vendored date).
- **Verification:** `cargo nextest run -p quarto-highlight` and the
  golden snapshot tests under
  `crates/quarto-highlight/tests/golden.rs`.
- **License:** per-language; recorded in each file's header (Julia
  is MIT).
- **Current version:** see provenance header in each
  `highlights.scm`.
- **Last reviewed:** 2026-05-04.

### H. Tree-sitter parser grammars (qmd, doctemplate) — **NOT vendored**

- **Path:** `crates/tree-sitter-qmd/`,
  `crates/tree-sitter-doctemplate/`
- **Status:** **repo-native, locally developed.** Listed here only
  to suppress repeated discovery and to record the historical
  confusion (see *Note on tree-sitter-qmd's stale "fork" framing*
  below).
- **Upstream:** none currently. `tree-sitter-doctemplate` was
  authored in-repo (grammar credits "Posit, PBC"; first commit
  `03c1fca4 hello doctemplate`). `tree-sitter-qmd` *originated* as
  a fork of
  `tree-sitter-grammars/tree-sitter-markdown` but has not been
  rebased against upstream — its `grammar.js` and `scanner.c` have
  evolved entirely in-tree since the initial import at commit
  `3509f210 add tree-sitter-qmd`.
- **Bundled via:** generated parser sources committed to the repo;
  rebuilt via `tree-sitter generate; tree-sitter build` (see
  `CLAUDE.md`).
- **Consumed by:** parser crates throughout the workspace.
- **Update procedure:** N/A — local code, edited in place. **No
  upstream sync.**
- **Verification:** `cargo nextest run -p tree-sitter-qmd` and the
  full `cargo xtask verify`.
- **License:** MIT (matches the original tree-sitter-markdown).
- **Last reviewed:** 2026-05-04.

#### Note on `tree-sitter-qmd`'s stale "fork" framing

Several files in `crates/tree-sitter-qmd/` still describe the crate
as a fork of upstream `tree-sitter-markdown`. They are out of date:
the crate has been independently developed for a long time and we
do not pull from upstream. Sources of confusion that misled this
audit's first pass:

- `crates/tree-sitter-qmd/README.md` line 3 — *"`tree-sitter-qmd`
  is a fork of [`tree-sitter-markdown`]…"*. Should be reworded to
  "originated as a fork of, but is now developed independently".
- `crates/tree-sitter-qmd/package.json` — still declares
  `"name": "@tree-sitter-grammars/tree-sitter-markdown"`,
  `"version": "0.4.0"`, `"author": {"name": "MDeiml"}`, and a
  `repository` field pointing at the upstream repo. These should
  be replaced with Quarto-Rust metadata or the file removed if
  unused.
- `crates/tree-sitter-qmd/tree-sitter.json` — same problem; the
  `metadata.version`, `authors`, and `links.repository` are
  upstream values.
- `crates/tree-sitter-qmd/README.tree-sitter-md.md` — verbatim
  upstream README. Either delete or relocate with a header noting
  it is a historical reference, not the current crate's README.
- The nested directory `crates/tree-sitter-qmd/tree-sitter-markdown/`
  implies vendoring by layout. Renaming or restructuring is out of
  scope for this audit but worth noting.

These cleanups are filed as a follow-up braid strand (see *Inventory
gaps*).

### I. quarto-system-runtime JS bundles

- **Path:** `crates/quarto-system-runtime/js/`
  (sources + `package.json`); built artifacts at
  `crates/quarto-system-runtime/js/dist/{simple-template-bundle.js,ejs-bundle.js}`.
- **Upstream:** the `js/package.json` declares devDependencies
  `esbuild ^0.20.0` and `ejs ^3.1.10`. The bundles are *built* from
  these npm packages.
- **Bundled via:** `include_str!` of `dist/*.js` in
  `crates/quarto-system-runtime/src/js_native.rs`.
- **Consumed by:** the system runtime when executing JS templates
  natively.
- **Update procedure:** bump versions in
  `crates/quarto-system-runtime/js/package.json`, run `npm install`
  + `npm run build` inside that directory, commit both the
  package-lock and the regenerated `dist/`.
- **Verification:** `cargo nextest run -p quarto-system-runtime`,
  plus integration tests that exercise EJS template rendering.
- **License:** MIT (esbuild), MIT (ejs).
- **Current version:** `esbuild ^0.20.0`, `ejs ^3.1.10` (as declared
  in `package.json`; the lockfile has the resolved versions).
- **Last reviewed:** 2026-05-04.

### J. reveal.js-menu (vendored CSS only)

- **Path:** `hub-client/public/reveal-menu/menu.css`
- **Upstream:** reveal.js-menu
  (https://github.com/denehyg/reveal.js-menu); the npm package is
  also a hub-client dependency (see `hub-client/package.json` →
  `reveal.js-menu ^2.1.0`), but the CSS is served as a static asset
  from `public/` (referenced by
  `hub-client/src/components/render/RevealjsReactAstSlideRenderer.tsx:134`
  via `path: '/reveal-menu/'`).
- **Bundled via:** copied into `hub-client/public/`, served by Vite
  as a static asset.
- **Consumed by:** hub-client revealjs preview.
- **Update procedure:** when the npm package is bumped, copy the
  matching `menu.css` from
  `node_modules/reveal.js-menu/menu.css` into
  `hub-client/public/reveal-menu/menu.css`. Two-step coordination
  with the npm dep is required.
- **Verification:** start `hub-client` dev server, open a revealjs
  document, exercise the menu.
- **License:** MIT.
- **Current version:** matches `reveal.js-menu ^2.1.0` from
  `hub-client/package.json` (verify the bundled CSS hash matches the
  lockfile-resolved version).
- **Last reviewed:** 2026-05-04.

### K. CSL test fixtures

- **Path:** `crates/quarto-csl/test-data/{default,ieee,apa,chicago-note-bibliography}.csl`
- **Upstream:** CSL styles repo. Used as test fixtures only; not
  embedded into any binary.
- **Bundled via:** read at test time via `include_str!` in tests.
- **Consumed by:** `quarto-csl` tests.
- **Update procedure:** these are reference fixtures — refresh only
  when validating against a newer CSL spec; otherwise leave alone
  to keep tests deterministic.
- **Verification:** `cargo nextest run -p quarto-csl`.
- **License:** CC-BY-SA-3.0.
- **Current version:** N/A (frozen fixtures).
- **Last reviewed:** 2026-05-04.

### L. Other Lua filters / shortcodes

- **Path:**
  `crates/pampa/tools/grid-table-fixer/grid-table-to-list-table.lua`,
  `crates/pampa/tools/definition-list-converter/definition-list-to-div.lua`,
  `crates/qmd-syntax-helper/resources/filters/{grid-table-to-list-table,definition-list-to-div}.lua`
- **Upstream:** repo-native (Quarto-Rust authored). **Not** vendored.
- **Why listed:** discovery sweep finds these `.lua` files; this
  entry exists so future audit runs don't repeatedly flag them.
- **Update procedure:** N/A — repo-native code.

---

## Inventory gaps (file as braid strands)

The following entries above lack a recorded upstream version /
SHA, which makes "are we behind?" decisions guesswork:

- **Entry D** — knitr R scripts: no recorded quarto-cli SHA.
- **Entry E** — Pandoc HTML templates: no recorded Pandoc SHA.
- **Entry C** — `kbd`, `video`, `version` extensions have no
  `version:` field in `_extension.yml`.
- **Entry H** — `tree-sitter-qmd` has stale "fork" framing in its
  README, `package.json`, `tree-sitter.json`, and the retained
  upstream README copy. The crate is repo-native; the metadata
  needs to be rewritten or removed. See sub-section above for the
  exact list of files.

When the cargo-upgrade skill runs and hits any of these, it should
record "version unknown" in the survey plan and (if not already
filed) open a braid strand under labels `deps`, `vendored`,
`inventory-gap`
to capture the missing SHA on next refresh.

---

## Entry template (for new vendored deps)

```
### <letter>. <Short name and version>

- **Path:** <repo-relative paths>
- **Upstream:** <URL + version/commit>
- **Bundled via:** <include_dir!/include_str!/include_bytes!/copied/static asset>
- **Consumed by:** <crate(s)>
- **Update procedure:** <terse, actionable; point to README if one exists>
- **Verification:** <cargo nextest -p X / specific render command>
- **License:** <SPDX>
- **Current version:** <version or commit SHA>
- **Last reviewed:** <YYYY-MM-DD>
```
