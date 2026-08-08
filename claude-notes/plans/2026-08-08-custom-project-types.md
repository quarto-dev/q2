# Custom project types: extension-contributed project types (website base)

- **Strand:** bd-ad7i1pc6 (discovered-from bd-wch2dotq "Make q2 render the posit-connect docs"; related: bd-mqk49; **absorbs bd-zb2tod5f** — see Phase 5)
- **Date:** 2026-08-08
- **Status:** DESIGN APPROVED 2026-08-08 (decisions recorded below) — awaiting go-ahead to execute

## Overview

Q2 currently **silently falls back to the default project type** when
`_quarto.yml` declares a `project: type:` it doesn't recognize. The
Posit Connect docs testbed (`~/repos/github/cscheid/q2-connect-docs/docs-quarto-2`)
declares `project: type: posit-docs`, supplied by the
`_extensions/posit-dev/posit-docs` extension via `contributes: project:` —
so today Q2 renders it as a bare default project: no website navigation,
no extension theme, wrong output-dir default.

This plan adds Q1-compatible **extension-contributed custom project
types**: common resolution/merge infrastructure for all custom project
kinds, with the website base type fully supported. Custom types whose
base is `book`/`manuscript` get a clear "not yet supported" error
(there are no book projects in Q2 yet); a future session picks those up
when the base types exist.

## What Quarto 1 does (findings)

Full details from the Q1 source study (paths relative to
`external-sources/quarto-cli/src`):

**A custom project type is not code — it is a named config bundle.**
Q1 has exactly four registered `ProjectType` implementations
(`project/types/register.ts:16-19`: book, default, website, manuscript)
and **no API for extensions to register a new one**. An extension
project type is a `_quarto.yml` *fragment* under `contributes.project`
that (a) names a base built-in type and (b) supplies default config.

Resolution flow (`project/project-context.ts`):

1. `projectContext()` reads `_quarto.yml`; if `project.type` is not a
   built-in name (line 199), it calls `resolveProjectExtension()`
   (lines 678-737).
2. That finds the extension by name (exact `org/name` match wins, else
   name-only with org-less first; ambiguity → warning, first match
   used), takes `contributes.project` as a config fragment, and:
   - **strips `project.detect`** (bootstrap-only auto-detection);
   - **deletes the extension's `project.render` wholesale** if the
     user's `_quarto.yml` sets `project.render` (all-or-nothing, no
     array union — the one exception to the general merge rule);
   - **rewrites `project.type` to the extension's base type**
     (`contributes.project.project.type`, defaulting to `"default"`);
   - merges extension-fragment-first, user-config-second →
     **user `_quarto.yml` wins** on scalar/object conflicts; arrays
     union-concat with dedup.
3. Path-looking strings in the fragment are rewritten to be relative to
   the project dir (so the extension's `theme.scss` becomes
   `_extensions/posit-dev/posit-docs/theme.scss`).
4. After the merge, the config is an ordinary `_quarto.yml` for a
   built-in type; `projectType(base)` dispatches as usual and the base
   type's config hook runs on the merged config.

Failure behavior: extension not found → the config is returned
untouched and `projectType()` **throws `Unsupported project type foo`**
(`project/types/project-types.ts:47`). Q1 never falls back silently.

Also relevant but **distinct**: `contributes.metadata.project` is a
separate, later merge with **reversed precedence** (extension wins over
user, `project-context.ts:113-137`). That's bd-zb2tod5f's territory;
we do not implement it here, but the two share the "discover extensions
at project-config time" substrate (see Phase 2). We should *not*
replicate Q1's reversed precedence without deciding to on purpose.

Q1 quirks we deliberately do not copy:
- `extensionProjectType()` (`extension/extension.ts:437-447`) reads the
  wrong nesting level (`contributes.project.type` instead of
  `contributes.project.project.type`) so path fixup always uses the
  default type's resource-ignore fields — a latent Q1 bug.
- `contributes: project: {}` (empty) passes the "contributes something"
  check and then crashes. We validate the fragment shape instead.

## What Q2 has today (findings)

- **The silent fallback**: `crates/quarto-core/src/project/mod.rs:651-656`
  — `ProjectKind::try_from(s).ok().unwrap_or_default()`. The error
  string is discarded; no diagnostic, no log. This is the single parse
  site (native CLI, WASM, and LSP all reach it via
  `ProjectContext::discover`).
- **`ProjectKind`** (`project/mod.rs:279-289`) is a closed `Copy` enum
  {Default, Website, Book, Manuscript}; a doc comment says it is a pure
  dispatch tag. `project_type_for` (`project/orchestrator.rs:418-431`)
  maps it to `DefaultProjectType`/`WebsiteProjectType` (book/manuscript
  currently also map to default, silently).
- **`contributes.project` is already parsed and never consumed**:
  `extension/read.rs:158` → `Contributes::project: Option<ConfigValue>`
  (`extension/types.rs:90`). The reading side exists end-to-end.
- **Extension discovery today is per-document and too late**: it runs in
  `StageContext::new` (`stage/context.rs:233-243`), after
  `ProjectContext::parse_config` has already frozen `project_kind`,
  `output_dir`, `render_patterns`, `resources`, `brand`, and
  pre/post-render scripts. To let an extension supply project config we
  must run a **project-scoped discovery pass inside
  `ProjectContext::discover`**, between reading raw `_quarto.yml`
  metadata and the field extraction at `mod.rs:651-697`. This is the
  same prerequisite bd-zb2tod5f identifies for
  `contributes.metadata.project.pre-render`.
- **Website behavior is mostly config-gated, not kind-gated**: navbar/
  sidebar/footer/listing transforms fire on config-key presence
  (`resolve_website_value`), not on `ProjectKind::Website`. Only a
  handful of sites dispatch on the kind (output-dir default, lib_dir,
  post_render sitemap/robots/favicon, bootstrap-icons transform,
  page-nav default, favicon brand fallback, is_multi_document). So once
  the merged config is in place and the kind resolves to `Website`,
  essentially everything downstream just works.
- **Config merge machinery is ready**: `ConfigValue` carries
  `SourceInfo` provenance, `MergeOp` (default Concat for arrays,
  `!prefer` for replace), and `quarto-config`'s layered `MergedConfig`.
  The per-document metadata merge (`stage/stages/metadata_merge.rs`)
  already rebases extension-relative `Path`-kind values.

## The testbed: what posit-docs actually exercises

`_extensions/posit-dev/posit-docs/_extension.yml` contributes:

```yaml
contributes:
  project:
    project:
      type: website          # base-type rewrite
    website:
      favicon: "assets/images/favicon.svg"     # ext-relative path
      bread-crumbs: true
      navbar: { pinned, logo (ext-relative), logo-alt, right: [Help menu] }
      search: { copy-button, show-item-context }
    format:
      html:
        theme: { light: [theme.scss], dark: [theme-dark.scss] }  # ext-relative, imports _posit-colors.scss
        highlight-style: { light: github, dark: arrow }
        link-external-icon / link-external-newwindow / code-copy: true
        toc: true / toc-expand: true
        include-in-header: ["assets/_analytics.html"]            # ext-relative
```

The user's `_quarto.yml` collides on purpose in several places —
`website.favicon`, `navbar.logo`, `format.html.include-in-header`
(three `!path` entries) — so the testbed exercises user-wins scalar
precedence *and* array concat semantics in one project. It also chains
`type: posit-docs → website`, giving us the full resolution path.

(The same project also needs `contributes.metadata` from quarto-openapi
[bd-zb2tod5f], `.ts` pre-render script execution, `_environment`, `!path`
includes, profiles, `llms-txt` — all tracked separately under
bd-wch2dotq. This strand only makes `type: posit-docs` work.)

## Design

### D1. Representation: resolve to base kind, remember the name

`ProjectKind` **stays a closed `Copy` enum**, matching Q1's semantics:
a custom type always resolves to a built-in base kind before anything
dispatches on it. We add to `ProjectConfig` (project/mod.rs:321):

```rust
/// Present when project.type named an extension-contributed type.
pub custom_project_type: Option<CustomProjectType>,

pub struct CustomProjectType {
    pub name: String,            // "posit-docs" as written by the user
    pub extension_id: String,    // "posit-dev/posit-docs"
    pub extension_dir: PathBuf,  // for diagnostics / path provenance
}
```

Rationale: `ProjectKind::Custom(String)` would break `Copy` and
propagate through ~7 dispatch sites for no benefit — nothing downstream
ever needs to behave differently for `posit-docs` vs `website` once the
config is merged. The record exists for diagnostics ("type: posit-docs
(website, from extension posit-dev/posit-docs)") and future features.

### D2. Resolution point and discovery

Inside `ProjectContext::parse_config`, after raw `_quarto.yml` metadata
is available and **before** the field extraction at mod.rs:651-697:

1. Read `project.type` as a string. If it is a built-in name → done
   (unchanged fast path).
2. Otherwise run **project-scoped extension discovery**: project root
   `_extensions/` plus the embedded built-in extensions (same loader as
   `extension/discover.rs`, restricted to the project root — Q1's
   project resolution effectively does the same since input = project
   dir). Build this as a reusable function so bd-zb2tod5f can call it
   for `contributes.metadata.project` later.
3. Name resolution, Q1-compatible: `org/name` exact match wins; else
   name-only match with org-less first; 2+ matches → warning diagnostic,
   first match used. Only extensions with a non-empty
   `contributes.project` participate.
4. Not found → **error diagnostic, abort render** (new Q-code; see D5).
   No silent fallback.

Chaining is not supported (Q1 parity): the extension's declared base
type must be a built-in name, else error.

### D3. Merge semantics (Q1-compatible)

Given the extension fragment `F` (a `ConfigValue` map) and user config
`U`:

1. Clone `F`; strip `F.project.detect` (unsupported for now; see
   out-of-scope) — if present, note it in verbose output.
2. ~~If `U.project.render` exists, delete `F.project.render` entirely
   (all-or-nothing, Q1 parity).~~ **Dropped (Carlos, 2026-08-08):** no
   Q1-parity special cases in the merge. Q2's standard semantics apply
   uniformly: arrays concat by default; a project that wants to fully
   replace an extension-contributed list (render globs, includes, css)
   annotates its own value with `!prefer`. Custom project types should
   feel like "just additional configuration options" under Q2's
   existing merge model; the Connect docs `_quarto.yml` may be adjusted
   with `!prefer`/`!concat` where Q1 and Q2 semantics differ.
3. Read `F.project.type` → base kind. Missing → `default` (Q1 parity,
   but with a warning diagnostic since it's almost certainly an
   authoring mistake). `book`/`manuscript` → error "custom project
   types with base 'book' are not yet supported in Quarto 2".
4. Rebase path-valued entries of `F` from extension-dir-relative to
   project-root-relative (D4).
5. Merge: `merged = layer(F) then layer(U)` using the existing
   `quarto-config` machinery — **user wins** on scalars/maps, arrays
   concat (extension entries first). Set `merged.project.type` to the
   base type name.
6. Continue `parse_config` field extraction against `merged` — so
   `output_dir`, `render_patterns`, `resources`, `brand`, pre/post
   render scripts, and the per-document layer-1 project metadata all
   see the merged config with zero further changes. The per-document
   extension layer (layer 2, `contributes.formats`) is unaffected — no
   double-counting, since `contributes.project.format.html` flows in
   via layer 1.

Known divergence from Q1: Q1 dedups array unions by JSON equality; Q2's
Concat does not dedup. Accepted for now (documented), revisit if the
testbed shows duplicate includes/css.

### D4. Path rebasing (the fiddly part)

Q1 rewrites *every path-looking string* in the fragment via
`toInputRelativePaths` with a per-key ignore list. Q2 is explicit
instead: `ConfigValueKind::Path` values get rebased during merge.

Approach: at extension read time (or at resolution time in D2), mark
path-valued keys in the `contributes.project` fragment as
`ConfigValueKind::Path`, using a **named key list** (extending the
existing `PATH_VALUED_KEYS` mechanism in `extension/read.rs`), then
rewrite them ext-dir → project-root before the merge. Initial key list,
driven by what posit-docs + the Q1 built-ins (confluence, hugo,
docusaurus) actually use:

- `format.*.theme` (scalar, list, and light/dark map forms), `css`,
  `include-in-header` / `include-before-body` / `include-after-body`,
  `format-resources`, `template`, `template-partials`
- `website.favicon`, `website.navbar.logo`, `website.sidebar.*.logo`,
  `website.page-footer.*` image entries
- `project.pre-render` / `project.post-render` (script paths),
  `project.resources`

Each key gets a test. **Verification item:** check how layer 2 handles
`contributes.formats.html.theme` today — `theme` is not in
`PATH_VALUED_KEYS` (only template/template-partials/shortcodes/filters),
so format-extension themes may already be broken; if so, file a
discovered-from strand and fix the shared list once.

SCSS: the rebased theme path
(`_extensions/posit-dev/posit-docs/theme.scss`) `@import`s
`_posit-colors.scss` — verify the sass include-path setup resolves
imports relative to the theme file's own directory (test in Phase 4).

### D5. Diagnostics (no more silent anything)

New error-catalog entries (Q-16 block is extension-related):

- **Unknown project type** (error): `project: type: foo` is not a
  built-in and no extension in `_extensions/` contributes project type
  `foo`. Hint: list available project-contributing extensions found, and
  the built-in names. Replaces the `.ok().unwrap_or_default()` at
  mod.rs:651-656. (`ProjectKind::try_from`'s error stops being
  discarded.)
- **Unsupported base type** (error): extension resolves but declares
  base `book`/`manuscript`.
- **Ambiguous extension match** (warning): 2+ name-only matches;
  names which one was chosen.
- **Missing base type in contribution** (warning): fragment has no
  `project.type`; defaulted to `default`.

Also fix the adjacent silent hole: today a *built-in* `type: book` /
`type: manuscript` silently renders as default via `project_type_for`
(orchestrator.rs:429). Emit a warning ("book projects are not yet
supported; rendering as default") — small, in scope, same code
neighborhood. `q2 render` output line should show
`type: posit-docs (website)` instead of `type: default`.

### D6. `contributes.metadata` (bd-zb2tod5f, folded into this strand)

Decision 2026-08-08: implement `contributes.metadata` here too, since
it shares the Phase 2 substrate and the Connect docs testbed needs both
(quarto-openapi contributes `metadata.project.pre-render`). Following
bd-zb2tod5f's own proposal, in two parts:

1. **Project-level**: merge each discovered extension's
   `contributes.metadata.project` into the project config at
   `parse_config` time, *before* field extraction (so `pre-render`/
   `post-render`/`resources` contributions take effect). Precedence:
   **user wins**, arrays concat — deliberately diverging from Q1,
   whose `mergeExtensionMetadata` lets the extension override the user
   (`project-context.ts:113-137`); that reversed precedence looks like
   an accident of implementation order, not a design, and bd-zb2tod5f
   already proposed user-wins. Paths in the fragment (notably script
   paths like `pre-render`) rebase ext-dir → project-root via the D4
   machinery.
2. **Document-level**: non-`project` keys of `contributes.metadata`
   join the per-document merge as part of the existing extension layer
   (layer 2 in `metadata_merge.rs`), below directory/document/runtime
   layers.

Unlike `contributes.project` (opt-in by naming the type),
`contributes.metadata` applies from **every discovered extension**
unconditionally — same as Q1. Ordering among multiple extensions:
discovery order (built-ins first, then project `_extensions`), which
the tests pin down.

### D7. What we explicitly do NOT build now

- `contributes.project.project.detect` (project-less bootstrap
  detection) — stripped with a verbose note; future strand.
- `preview.serve` external-preview contributions (hugo/docusaurus
  style) — future strand.
- Custom **book**/**manuscript** base types — blocked on Q2 growing
  those project types at all.
- A pluggable `ProjectType`-trait registry / pipeline-stage
  contribution (bd-mqk49) — nothing in Q1's model needs it; custom
  types are config bundles.
- `q2 add` / template install — extensions arrive in-tree for now.
- Running quarto-openapi's `.ts` pre-render script (needs a Deno/Node
  story) — that's script *execution*, tracked under bd-wch2dotq; here
  we only make the contributed `pre-render` entry land in project
  config.

## Test plan (TDD — tests first in every phase)

Fixture: `crates/quarto-core/tests/fixtures/custom-project-type/` (or
alongside existing project fixtures): a minimal project with
`_extensions/acme/fancysite/_extension.yml` contributing
`project.type: website`, website config with ext-relative favicon/logo,
`format.html.theme` + `include-in-header`, plus a user `_quarto.yml`
that collides on favicon (user-wins check) and declares its own
`render:` list (all-or-nothing check). A second org-less extension for
name-resolution tests.

Unit/integration tests (crates/quarto-core `tests/integration/`):

1. Resolution: exact org match beats name-only; org-less first;
   ambiguity warning; not-found → error diagnostic (assert Q-code).
2. Type rewrite: `project_kind() == Website`,
   `custom_project_type` populated.
3. Merge: user scalar wins; extension fills gaps; arrays concat with
   extension first; `render` all-or-nothing; `detect` stripped.
4. Path rebasing: one assertion per key in the D4 list.
5. Field extraction sees merged config: `output_dir` defaults to
   `_site` (website base), pre/post-render scripts, resources.
6. Diagnostics: built-in `book` warning; unknown-type error; missing
   base-type warning.
7. End-to-end (`render_document_to_file`-level, then real binary):
   render the fixture project, assert navbar/theme/include-in-header
   markup in output HTML.

End-to-end verification (per CLAUDE.md policy): run
`cargo run --bin q2 -- render` on the fixture **and** on
`q2-connect-docs/docs-quarto-2`, inspect output HTML for the posit-docs
theme/navbar contributions, and record invocation + output snippet in
this plan before closing.

## Work items

### Phase 1 — kill the silent fallback (standalone value) ✅ 2026-08-08
- [x] Tests: unknown `project: type:` → error diagnostic; built-in
      `book`/`manuscript` → "not yet supported" warning
      (`tests/integration/project_type_parsing.rs`, 8 tests, written
      first and observed failing)
- [x] Error-catalog entries — **Q-5-17** (Unknown Project Type, error)
      and **Q-5-18** (Project Type Not Yet Implemented, warning); the
      project subsystem is Q-5, not Q-16 as originally guessed. Wired
      into `parse_config` via `extract_project_kind` /
      `project_type_error`; Q-5-18 via `project_kind_diagnostics`,
      printed next to the `underscore_typo_diagnostics` precedent in
      `quarto/src/commands/render.rs` and `quarto-preview/src/lib.rs`.
- [x] `q2 render` banner accuracy: banner still prints the parsed
      kind; the Q-5-18 warning directly above it explains the
      book→default behavior. Custom-name display (`posit-docs
      (website)`) lands with Phase 3.

Phase 1 end-to-end evidence (2026-08-08, output inspected):

```
$ cargo run --bin q2 -- render <tmp>/unknown   # _quarto.yml: type: posit-docs
Error: Project discovery failed: Error: [Q-5-17] Unknown project type
 2 │  type: posit-docs
   │        ─────┬────  ╰─ `posit-docs` is not a recognized project type.
ℹ Built-in project types are `default`, `website`, `book`, and `manuscript`.
(exit 1)

$ cargo run --bin q2 -- render <tmp>/book      # _quarto.yml: type: book
Warning [Q-5-18]: `book` projects are not yet implemented
...renders with default-project behavior...
Rendering project: ... (type: book)
Rendered 1 of 1 files (exit 0)
```

Full workspace suite: 11,077 passed. `cargo xtask lint` clean; clippy
clean on quarto-core/quarto/quarto-preview.

### Phase 2 — project-scoped extension discovery (shared substrate) ✅ 2026-08-08
- [x] Tests: discovery from project root `_extensions/` + embedded
      built-ins at project-config time (4 unit tests in
      `extension::discover`; subdirectory `_extensions/` deliberately
      excluded at project scope)
- [x] Reusable `discover_project_extensions(project_dir, builtin_dir,
      runtime)`; `builtin_extensions_path` hoisted from
      `stage/context.rs` to `extension::`; no change to per-document
      discovery (commit e15f173c)

### Phase 3 — resolution + merge (the core) ✅ 2026-08-08
- [x] Tests: `tests/integration/custom_project_type.rs` (14 tests,
      written first, observed failing) — resolution, user-wins /
      concat merge, `!prefer`, render-glob concat, `detect` stripping,
      Q-5-17 candidate listing, Q-16-7 base-type errors, Q-16-8
      ambiguity, Q-16-9 missing base
- [x] `CustomProjectType` + `config_diagnostics` on `ProjectConfig`;
      `resolve_project_type` / `resolve_custom_project_type` in
      `parse_config`; `project_type_label()` drives the render banner
      (`type: fancy-docs (website)`); config diagnostics printed at
      the render + preview sites; catalog entries Q-16-7/8/9 and
      updated Q-5-17
- [x] Fragment validation: non-map fragment and non-string/chained/
      book/manuscript base → Q-16-7 error; missing base → Q-16-9
      warning + `default`

**Discovered (bd-43lc07w1, P1):** `quarto-yaml` 0.1.1 only captures
YAML tags on *scalars* — `Event::SequenceStart`/`MappingStart` discard
theirs — so `!prefer`/`!concat` written on an array or map in any YAML
file never reaches the merge machinery. This blocks the sanctioned
"replace an extension-contributed list with `!prefer`" pattern. Fix is
upstream (posit-dev/quarto-yaml) + a version bump here. The pinning
test `user_prefer_replaces_extension_array` is `#[ignore]`d with the
strand id until the fix ships.

Phase 3 end-to-end evidence (2026-08-08, output inspected):

```
$ cargo run --bin q2 -- render <tmp>   # type: fancy-docs; navbar only in extension
Rendering project: <tmp> (type: fancy-docs (website))
Rendered 2 of 2 files to <tmp>/_site
$ grep -o '<nav[^>]*>' <tmp>/_site/index.html
<nav class="navbar navbar-expand-lg">      # extension-contributed navbar rendered
$ grep -o '<title>[^<]*</title>' <tmp>/_site/index.html
<title>E2E Site</title>                    # user's website.title wins
```

Full workspace suite: 11,094 passed (1 ignored pending bd-43lc07w1).

### Phase 4 — path rebasing + real-world verification
- [ ] Tests: item 4 (per-key rebasing), SCSS import resolution
- [ ] Rebase implementation (D4); verify/fix layer-2 `theme` gap,
      filing discovered-from strand if it's a pre-existing bug
- [ ] End-to-end: fixture + q2-connect-docs render, evidence recorded
      here
- [ ] Full workspace verify (`cargo xtask verify` — quarto-core touched,
      so full WASM leg)

### Phase 5 — `contributes.metadata` (absorbed bd-zb2tod5f)
- [ ] Tests: project-level `contributes.metadata.project` merge
      (user-wins, pre-render path rebased, multiple-extension
      ordering); document-level layer for non-project keys
- [ ] Project-level merge in `parse_config` (D6.1)
- [ ] Document-level layer extension in `metadata_merge.rs` (D6.2)
- [ ] End-to-end: quarto-openapi's contributed
      `project.pre-render` entry appears in resolved project config for
      q2-connect-docs (execution of the .ts script itself is out of
      scope — bd-wch2dotq)
- [ ] Close bd-zb2tod5f pointing here

### Phase 6 — docs + follow-ups
- [ ] `docs/guides/authoring/extensions.qmd`: replace `TBD.` with at
      least the `contributes.project` + `contributes.metadata` sections
      (user-facing usage, not internals); render docs/ with q2 to
      verify
- [ ] File follow-up strands: `detect` bootstrap, `preview.serve`,
      custom book/manuscript (blocked on base types), array-dedup
      parity decision if the testbed surfaces duplicates

## Design decisions (resolved 2026-08-08 with Carlos)

1. **Unknown project type → hard error** (new Q-16 code, hint lists
   built-ins + discovered project-contributing extensions). No silent
   or warned fallback.
2. **Representation**: closed `ProjectKind` + `CustomProjectType`
   record on `ProjectConfig` (D1). No `ProjectKind::Custom` variant.
3. **Extension base `book`/`manuscript` → hard error** ("not yet
   supported in Quarto 2"). Built-in `type: book`/`manuscript` keep
   rendering as default but gain a visible warning.
4. **bd-zb2tod5f folded in**: this strand implements both
   `contributes.project` and `contributes.metadata` on the shared
   project-scoped discovery substrate (D6, Phase 5).

Still open (minor, decide during implementation, flag in review):
array concat without dedup (divergence from Q1); ambiguity = warning +
first match (Q1 parity); built-in embedded extensions participate in
project-type resolution.
