---
title: 'Plan: CSS in Pipeline — Part B2: Theme Inheritance Tests (Phase 5)'
date: 2026-03-09
description: 'Adds end-to-end smoke tests and an `ensureCssRegexMatches` assertion that check theme CSS follows project, directory and document metadata inheritance across six fixture files on native and WASM.'
---

Parent plan: `claude-notes/plans/2026-03-09-css-in-pipeline.md`
Prerequisite: `claude-notes/plans/2026-03-09-css-in-pipeline-a-core.md`
Next: `claude-notes/plans/2026-03-09-css-in-pipeline-b1-migration.md`

This sub-plan adds end-to-end smoke-all tests that verify theme CSS
compilation respects the full metadata hierarchy (project, directory,
document). These tests exercise the `CompileThemeCssStage` through the
complete render pipeline on both native and WASM.

**TDD ordering**: This plan runs BEFORE B1 (migration). Some tests are
expected to fail because B1 hasn't wired up the pipeline's
`CompileThemeCssStage` output to the actual CSS files yet. The exact
failure pattern (native vs WASM, which test cases) should be analyzed
after the fixtures are created. If *no* tests fail, something is wrong —
B1 is supposed to be the fix for these tests.

These tests will NOT be committed until B1 makes them pass. The work
here is to create the assertion infrastructure (`ensureCssRegexMatches`)
and the fixture files, run them, analyze failures, and document what B1
needs to fix. The fixtures and assertions are committed together with
B1 once everything is green.

## Port `ensureCssRegexMatches` from TS Quarto

TS Quarto's `ensureCssRegexMatches` (in `tests/verify.ts`) works by:
1. Parsing the rendered HTML to find `<link rel="stylesheet">` tags
2. Reading each linked CSS file from disk (skipping external URLs)
3. Concatenating all linked CSS content
4. Running regex matches/no-matches against the combined CSS

Same two-array YAML format as `ensureFileRegexMatches`:
```yaml
ensureCssRegexMatches:
  - ["pattern-that-must-match"]
  - ["pattern-that-must-NOT-match"]
```

**Native** (`quarto-test`): Parse the output HTML, find `<link rel="stylesheet">`
hrefs, read those CSS files from the output directory, concatenate, and regex
match. Needs an HTML parser — can use a lightweight regex approach to extract
`<link>` hrefs since the HTML is well-formed template output, or add a
dependency like `scraper` for CSS selector support.

**WASM** (`smokeAll.wasm.test.ts`): Same approach but read CSS from VFS.
After rendering, the HTML contains `<link>` tags pointing to relative paths
like `{stem}_files/styles.css`. These resolve in VFS under the project
prefix. Use `wasm.vfs_read_file()` to read each linked CSS file. Already
has `jsdom` available for HTML parsing (used by `ensureHtmlElements`).

Work items:
- [ ] Add `ensureCssRegexMatches` assertion to `crates/quarto-test/src/assertions/`
  (new file `css_regex.rs`; parses HTML for `<link>` stylesheet hrefs, reads
  CSS files from output dir, concatenates, regex matches)
- [ ] Register in `crates/quarto-test/src/spec.rs` assertion parser
- [ ] Add `ensureCssRegexMatches` handling in `smokeAll.wasm.test.ts`
  (parse HTML with jsdom for `<link>` hrefs, read CSS from VFS, regex match)

## Theme detection strategy

Each Bootswatch theme has a unique `--bs-primary` color value in the compiled
CSS. This is the most reliable single-variable discriminator. Font-family
provides a strong secondary signal for themes that use custom fonts.

**Primary detection** (unique `--bs-primary` values — no two themes share these):
- **darkly**: `--bs-primary:.*#375a7f`
- **flatly**: `--bs-primary:.*#2c3e50`
- **cosmo**: `--bs-primary:.*#2780e3`
- **sketchy**: `--bs-primary:.*#333`

**Secondary detection** (unique font-family strings):
- **flatly**: `Lato` in `--bs-font-sans-serif`
- **cosmo**: `Source Sans Pro` in `--bs-font-sans-serif`
- **sketchy**: `Neucha` in `--bs-font-sans-serif`, `Cabin Sketch` in headings

**Additional discriminators**:
- **darkly**: `--bs-body-bg:.*#222` (only dark-background theme in the set)
- **default** (no theme): static `DEFAULT_CSS` contains `/* ===== Base Styles ===== */`

Source: `resources/scss/bootstrap/themes/{theme}.scss` variable definitions.

## Test fixture layout

All under `crates/quarto/tests/smoke-all/metadata/theme-inheritance/`:

```
theme-inheritance/
├── _quarto.yml                           # theme: darkly
├── root-doc.qmd                          # no theme override → gets darkly
├── chapters/
│   ├── _metadata.yml                     # theme: flatly
│   ├── chapter1.qmd                      # no override → gets flatly
│   ├── chapter2.qmd                      # theme: cosmo → overrides to cosmo
│   └── deep/
│       ├── _metadata.yml                 # (no theme) → inherits flatly from parent
│       └── deep-doc.qmd                  # no override → gets flatly (inherited)
└── appendix/
    ├── appendix-doc.qmd                  # no override → gets darkly (from project)
    └── custom/
        ├── _metadata.yml                 # theme: sketchy
        └── custom-doc.qmd               # no override → gets sketchy
```

## Test cases (6 QMD files)

1. **`root-doc.qmd`** — project theme only
   - `_quarto.yml` sets `theme: darkly`, doc has no theme
   - Assert CSS matches `--bs-primary:.*#375a7f`, does NOT match `#2c3e50` or `Base Styles`

2. **`chapters/chapter1.qmd`** — directory metadata overrides project
   - `chapters/_metadata.yml` sets `theme: flatly`
   - Assert CSS matches `--bs-primary:.*#2c3e50`, does NOT match `#375a7f`

3. **`chapters/chapter2.qmd`** — document overrides directory
   - Document frontmatter sets `theme: cosmo`
   - Assert CSS matches `--bs-primary:.*#2780e3`, does NOT match `#2c3e50` or `#375a7f`

4. **`chapters/deep/deep-doc.qmd`** — inherited directory metadata (no local `_metadata.yml` theme)
   - `deep/_metadata.yml` exists but has no theme → inherits flatly from `chapters/`
   - Assert CSS matches `--bs-primary:.*#2c3e50`

5. **`appendix/appendix-doc.qmd`** — sibling directory without `_metadata.yml`
   - No directory metadata → falls through to project theme (darkly)
   - Assert CSS matches `--bs-primary:.*#375a7f`, does NOT match `#2c3e50`

6. **`appendix/custom/custom-doc.qmd`** — deeper subtree with own theme
   - `custom/_metadata.yml` sets `theme: sketchy`
   - Assert CSS matches `Neucha` (unique sketchy font), does NOT match `#375a7f`

Sibling isolation is implicitly verified: test 2 (chapter1 → flatly) and
test 5 (appendix-doc → darkly) are separate render invocations that must
each inherit correctly from their own metadata ancestry.

## Work items

- [x] Identify reliable CSS detection patterns (see theme detection strategy above)
- [x] Add `ensureCssRegexMatches` assertion to native runner + WASM runner (see above)
- [x] Create fixture directory structure and all 6 QMD files
- [x] Run native smoke-all tests — analyze which pass/fail and why
- [x] Run WASM smoke-all tests — analyze which pass/fail and why
- [x] Document failure analysis (which tests fail, expected vs unexpected)

## Native failure analysis

All 6 theme-inheritance tests fail. All 15 existing tests pass.

Every failure is "Required CSS pattern not found" — the native `write_themed_resources`
path only reads document frontmatter, so project-level (`_quarto.yml`) and
directory-level (`_metadata.yml`) theme settings are ignored. The CSS written
is either DEFAULT_CSS or only reflects frontmatter themes.

Specific failures:
- **root-doc**: wants darkly (`#375a7f`) from `_quarto.yml` — gets default CSS
- **chapter1**: wants flatly (`#2c3e50`) from `chapters/_metadata.yml` — gets default CSS
- **chapter2**: wants cosmo (`#2780e3`) from frontmatter — likely gets default CSS
  (even frontmatter themes may fail if `write_themed_resources` doesn't compile)
- **deep-doc**: wants flatly inherited from `chapters/` — gets default CSS
- **appendix-doc**: wants darkly from `_quarto.yml` — gets default CSS
- **custom-doc**: wants sketchy (`Neucha`) from `custom/_metadata.yml` — gets default CSS

B1 will fix this by: removing `write_themed_resources`, using pipeline's
`CompileThemeCssStage` output (`css:default` artifact) to write CSS files.

## WASM failure analysis

Same 6 tests fail, same pattern. WASM `render_qmd()` runs `CompileThemeCssStage`
in the pipeline (which produces the correct `css:default` artifact), but the
JS-side `compileAndInjectThemeCss` overwrites the pipeline output with
frontmatter-only CSS. The VFS CSS file doesn't contain the correct theme.

B1 Phase 4 will fix this by removing the JS-side `compileAndInjectThemeCss`
call, letting the pipeline's artifact flow through to VFS unchanged.

These fixtures are held uncommitted until B1 makes them all pass.

## Verification (after B1)

- [x] `cargo build --workspace` — compiles
- [x] `cargo nextest run --workspace` — 6602 tests pass (including all 6 new fixtures)
- [x] `cargo xtask verify` — WASM builds and hub-client tests pass (21 WASM smoke-all, 0 failures)
