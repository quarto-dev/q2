# brand.yml font weight ranges collapse to 400; no variable-font axis support (bd-5fseopxy)

**Date:** 2026-09-08
**Braid:** bd-5fseopxy (bug, p2, labels: css, diagnostics, theming)
**Checkout:** `~/rooms/room-1/q2`, branch `main` @ `b7e7c96a` (no worktree/branch created; the
user picks where the fix lands)
**Status:** Merged to `main` 2026-09-08 via https://github.com/quarto-dev/q2/pull/663. Nothing left
to do in the repo; the strand is closed in braid once the user approves.

## Triage verdict

**Ready to design.** The bug reproduces at HEAD exactly as filed, the code is small and
self-contained (`crates/quarto-brand/src/types.rs` + `crates/quarto-sass/src/brand_layer.rs`),
and the only open questions are scope choices (which sources get ranges, how strict to be,
which error code) — not missing information.

One correction to the strand: Quarto 1 does **not** support ranges either. Its schema
(`definitions.yml` `brand-font-weight`) is a closed enum of `100..900` and the keyword names,
so `400..700` is a YAML validation error in Q1 before `brandFontWeightValue` ever runs. The
"Q1 throws `Unknown font weight`" parity point still holds for the *silent-400* half of the bug;
the *range* half is a new feature relative to Q1 that the brand.yml spec (and its Python
reference implementation) defines.

## Issue context

Filed 2026-09-08 by Carlos while testing Google/local variable fonts for cscheid-net-2026.
`weight: 400..700` on a `fonts[]` entry parses without complaint and the Google `@import`
requests only weight 400; bold falls back to synthetic bold. No warning. The same string on a
typography slot (`headings: {weight: 500..700}`) is emitted verbatim as
`$headings-font-weight: 500..700 !default;`, which is not valid SCSS and trips the silent
compile fallback of bd-jsvetdea — two silent failures stack.

## Dependency graph

No `blocks` edges in either direction, no `discovered-from` parent (the strand is its own
discovery record). Two `related` edges:

- **bd-jsvetdea** (in_progress, being fixed in `~/rooms/room-3/q2` by another agent; plan:
  `claude-notes/plans/2026-09-08-theme-scss-compile-error-swallowed.md` there). It makes every
  theme-compile failure a hard `Q-14-6` error and claims **`Q-14-6` and `Q-14-7`**. Once it
  lands, the typography-slot case of *this* bug stops being silent (the render fails with a
  grass error at an assembled-bundle line) but the message will not name the weight value. Any
  new code for this strand must start at **`Q-14-8`** to avoid a collision. The two fixes are
  otherwise independent and touch different files (room-3: `compile_theme_css.rs`,
  `theme_diagnostic.rs`, `quarto-sass/src/error.rs`; here: `quarto-brand/src/types.rs`,
  `quarto-sass/src/brand_layer.rs`). A merge conflict is possible only if this strand adds a
  `theme_diagnostic` arm — see design question 4.
- **bd-qnylgu69** (open) — audit `docs/guides/authoring/brand.qmd` against real Q2 support.
  The strand asks that the docs say ranges are unsupported *until* this lands; if this lands
  first, the doc note becomes "ranges are supported for google/bunny (and file, if chosen)".
  Note the guide currently links to `/docs/reference/metadata/brand.qmd`, which does not exist
  in `docs/` — a separate bd-qnylgu69 finding.

## What the spec says (brand.yml, posit-dev)

From <https://posit-dev.github.io/brand-yml/brand/typography.html> and the Python reference
implementation (`pkg-py/src/brand_yml/typography.py`):

- `fonts[].weight` for `source: google` / `bunny` accepts a number, a keyword, a list of those,
  or a range string `N..M`. "Variable font weights can be written as a string `600..900`."
  Both ends go through the same validator as single weights (keyword → number; numbers must be
  100–900, divisible by 100).
- `fonts[].files[].weight` (`source: file`) **also** accepts a range in the reference
  implementation (`BrandTypographyFontFileWeight`), emitted as CSS `font-weight: 600 900` in
  `@font-face`. The prose docs are silent on this; the code supports it.
- Typography **slots** (`base`, `headings`, …) take a single weight only: "a numeric value
  between 100 and 900, or a string like normal or bold". A range there is a user error.
- Default when `weight` is absent on google/bunny: the reference includes **all of 100–900**;
  Q1 and Q2 both request `400;700`. Out of scope here, but worth knowing (see question 6).
- Google CSS2 axis syntax for a range is `wght@400..700`, and with italics
  `ital,wght@0,400..700;1,400..700`.

## What the code looks like today

All paths in the strand still exist with the described shape (checked at `b7e7c96a`):

- `crates/quarto-brand/src/types.rs:623-636` — `BrandFontWeight` is `Number(u32) | Name(String)
  | List(Vec<BrandFontWeightAtom>)`, serde-untagged. `400..700` deserializes as
  `Name("400..700")`; there is no validation of keyword names at parse time. `BrandError`
  (`quarto-brand/src/error.rs`) has only `Parse`, `CircularColorReference`, `UnknownColorName`.
- `crates/quarto-sass/src/brand_layer.rs:538-551` — `enumerate_weights` does
  `weight_name_to_number(s).unwrap_or(400)`: **any** unknown string, not just ranges, silently
  becomes 400 (`weight: bolder` too).
- `brand_layer.rs:350-366` — `font_weight_to_scss` (typography slots and `@font-face`) passes
  unknown strings through verbatim, so slots emit invalid SCSS and `@font-face` emits invalid
  CSS.
- `brand_layer.rs:466-497` / `499-530` — `google_font_import_string` / `bunny_font_import_string`
  join discrete weights with `;` / `,`; no axis form.
- `brand_layer.rs:553-584` — `file_font_face_block` emits one `font-weight: <n>` per file.
- Error plumbing: `brand_to_layers` already returns `Result<_, SassError>`, and the theme
  stage propagates brand errors as a hard `PipelineError::stage_error("brand resolution: …")`
  (`compile_theme_css.rs:343-352` reveal, `:502` html). `SassError::InvalidThemeConfig` maps to
  **`Q-14-1`** in `theme_diagnostic.rs`, but the brand-resolution `?` sites use `stage_error`
  and so currently render code-less. Brand values carry **no source locations** (serde_yaml
  into plain structs), so a diagnostic can name the value and the family but not point into
  `_brand.yml`.

### Repro at HEAD

Fixture: `claude-notes/plans/brand-font-weight-ranges-investigation/repro/` (`_quarto.yml`,
`_brand.yml`, `index.qmd`). `_brand.yml` exercises all three surfaces: a google range, a
`source: file` range, and a slot range.

```
$ cargo run -q --bin q2 -- render claude-notes/plans/brand-font-weight-ranges-investigation/repro
```

Four fixtures under `claude-notes/plans/brand-font-weight-ranges-investigation/` (see its
`README.md`), each a single-page `type: default` project with `theme: brand`; rendered output is
gitignored there. All observed at `b7e7c96a`, output inspected by hand:

```
$ cargo run -q --bin q2 -- render claude-notes/plans/brand-font-weight-ranges-investigation/repro-google-only
Rendered 1 of 1 files …
$ grep -o '@import url([^)]*)' repro-google-only/index_files/quarto/quarto-theme-7f5358dfed34d263.css
@import url("https://fonts.googleapis.com/css2?family=EB+Garamond:ital,wght@0,400;1,400&display=swap")
```

`weight: 400..700` on the google entry → only weight 400 requested (334 KB bundle otherwise
fine). Bold falls back to synthetic bold. No diagnostic.

```
$ cargo run -q --bin q2 -- render …/repro-fonts-only      # google range + file range
[trace] [warn] theme CSS compilation failed: SASS compilation failed: SASS compilation error: Error: expected ";".
12 │     font-weight: 300..800;
   │                     ^
, using default CSS

$ cargo run -q --bin q2 -- render …/repro-slot-only       # headings: {weight: 500..700}
[trace] [warn] theme CSS compilation failed: … Error: expected ";".
966 │ $headings-font-weight: 500..700 !default;
    │                           ^
, using default CSS
```

Both variants "succeed" and ship the 6996-byte `DEFAULT_CSS` (the whole brand — colors, fonts,
everything — is gone). The `[trace]` lines appear only because the fixtures set
`trace: summary`; a normal render prints nothing. So the strand undercounts: **three** surfaces
misbehave, not two — the `source: file` per-file range is not merely unsupported, it takes the
whole theme down via the bd-jsvetdea fallback, exactly like the slot range. Once bd-jsvetdea
lands, the file and slot cases become hard `Q-14-6` errors pointing at an assembled-bundle
line; the google case stays silent.

### Pre-flight verify

`cargo xtask verify --skip-hub-build` at `b7e7c96a`: Rust build and all Rust tests green.
The hub-client vitest leg failed with **23 tests in 6 files**, all `localStorage` undefined
under the local Node (v26) — the same environmental failure room-3 hit on the same HEAD and
filed as **bd-lh30hlvd**. Unrelated to this strand; not re-filed.

## Design decisions (user answers, 2026-09-08)

1. **Scope.** Ranges are supported on `fonts[].weight` for `source: google` and `source: bunny`,
   **and** on `fonts[].files[].weight` for `source: file` (emitted as `font-weight: N M`).
   Heads-up from the user: another agent suspects deeper bugs in Q2's `source: file` handling
   and a separate plan is being drawn up for them. This strand only changes the `font-weight`
   line of the `@font-face` block; if the file path/URL side misbehaves during Phase 5, note it
   in that plan rather than widening this one.
2. **Range syntax is numeric only.** `N..M` with integers; both ends in `[100, 900]`, `N <= M`
   (`N..N` allowed), any integer in range (CSS and Google accept non-round values). Keyword
   ends (`regular..bold`) are **rejected** — only the brand.yml Python package accepts them,
   Google Fonts and the spec prose use numbers, and the user does not want a second spelling.
3. **Unknown weight strings are a hard error** (Q1 parity), never a silent 400. This is one
   check — "is it in the keyword table?" — so `bolder`/`lighter`/`Bold`/`semibold` all fall out
   of it for free; no dedicated handling.
4. **New diagnostic with a real `_brand.yml` span.** The infrastructure exists:
   `quarto_yaml::parse_file(content, filename)` yields a node tree whose `SourceInfo`s carry
   `file_id_for_filename(filename)`, navigable with `get_hash_value` / `get_array_item`; and
   `quarto_core::config_sources::bind_config_source` registers the file whose path hashes to
   that id (never a non-match). So the stage can point at the exact `weight:` scalar. Shape:
   - `quarto-brand` validates weights after the serde parse and reports each failure as a
     **YAML path** (`typography.fonts[0].weight`, `typography.fonts[1].files[0].weight`,
     `typography.headings.weight`) plus the offending text and the family/slot.
   - `quarto-sass::load_split_brand` already holds the brand text and path; it parses the text
     with quarto-yaml (new dep — quarto-sass already depends on quarto-source-map), walks the
     path to the node, and returns a new `SassError::InvalidBrandFontWeight { message,
     location }`. Inline `brand:` blocks walk the `ConfigValue` instead, which already carries
     `source_info`.
   - `theme_diagnostic` gains an arm for it under **`Q-14-8`** "Invalid brand font weight",
     with the brand file path added to the candidate list for `bind_config_source`.
   - If the node cannot be located (should not happen; defensive), the error renders
     span-less with the path in prose. Never bind a wrong file.
5. **Slot ranges are a hard error** (same `Q-14-8`), message says slots take a single weight.
6. **Default weights stay `400;700`.** Filed as **bd-b1rrnzp1** (discovered-from this strand).

## Work items

### Phase 0 — Tests (TDD; written first, verified failing)

- [x] `quarto-brand` unit: `weight: 400..700` → `Range(400, 700)`; `weight: 400` and
      `weight: bold` unchanged; `[400, 700]` unchanged.
- [x] `quarto-brand` unit: each of `700..400`, `50..700`, `400..1000`, `400..`, `..700`,
      `regular..bold`, `bolder`, `Bold`, `semibold` → validation error whose YAML path and
      offending text are as expected; a valid brand yields no errors.
- [x] `quarto-brand` unit: `headings: {weight: 500..700}` → validation error at
      `typography.headings.weight`.
- [x] `quarto-sass` `brand_layer_test.rs`: google range → `wght@400..700`; with
      `style: [normal, italic]` → `ital,wght@0,400..700;1,400..700`; range + discrete list is
      not representable (list atoms stay discrete) — assert the list path is unchanged.
- [x] `quarto-sass` `brand_layer_test.rs`: bunny range → `400,500,600,700` (and the `i`
      italic form); file range → `font-weight: 300 800`.
- [x] `quarto-sass` config test: a `_brand.yml` on disk with a bad weight →
      `SassError::InvalidBrandFontWeight` whose `location` has the file's
      `file_id_for_filename` id and offsets covering the `400..7` scalar; inline brand block →
      location from the `ConfigValue`.
- [x] `quarto-sass` `brand_compile_test.rs`: the google-range brand compiles to a full bundle
      (regression guard for the SCSS-validity of the emitted lines).
- [x] `theme_diagnostic.rs`: `Q-14-8` renders with code, family, offending text, and a span
      into `_brand.yml`; catalog-registration test lists the new code.
- [x] CLI e2e `crates/quarto/tests/integration/brand_font_weight.rs` (registered in
      `main.rs`): render `repro-google-only` → CSS contains `wght@0,400..700;1,400..700`;
      render a bad-weight fixture → exit non-zero, stderr has `Q-14-8` and a `_brand.yml`
      excerpt pointing at the `weight:` line.
- [x] Run them; confirm each fails for the expected reason (silent 400 / verbatim
      passthrough / no code).

### Phase 1 — Type + validation (`quarto-brand`)

*Implemented 2026-09-08.* `BrandFontWeight::Range(BrandFontWeightRange { min, max })` sits
between `Number` and `Name` in the untagged order; its `Deserialize` accepts only
`<digits>..<digits>` so every other string (keyword ends, half-open) stays `Name` for
validation to report verbatim. `Brand::validate()` lives in `quarto-brand/src/validate.rs`
with `BrandPath` (`typography.fonts[0].files[1].weight`). A quoted `"400"` arrives as
`Name("400")` and is treated as the number. Numbers everywhere must be in `100..=900`.


- [x] `BrandFontWeight::Range(u32, u32)` variant; `BrandFontWeightAtom` unchanged (a list
      never contains a range). Deserialize `N..M` strings into it; every other string stays
      `Name`.
- [x] Move `weight_name_to_number` into `quarto-brand` (single table shared by validation and
      emission); keep a re-export or call-through in `brand_layer.rs`.
- [x] `BrandError::InvalidFontWeight { path: String, value: String, reason: String }` and a
      `Brand::validate() -> Vec<BrandError>` (or validation inside `from_yaml_str` /
      `UnifiedBrand::split`) covering `fonts[].weight`, `files[].weight`, and every slot's
      `weight` (slots reject `Range`).
- [x] Decide whether file-entry `weight` and slot `weight` should share the `BrandFontWeight`
      type or slots get a narrower type; the plan assumes shared type + validation (smaller
      change).

### Phase 2 — Emission (`quarto-sass/src/brand_layer.rs`)

- [x] `enumerate_weights` → returns an enum `Weights { Discrete(Vec<u32>), Range(u32, u32) }`;
      google emits `N..M` (with `ital,` pairs when italic); bunny expands the range to every
      multiple of 100 in `[N, M]` (plus the `i` italics).
- [x] `file_font_face_block`: `Range(n, m)` → `font-weight: n m`.
- [x] `font_weight_to_scss` (slots): `Range` is unreachable after validation — `unreachable!`
      is not acceptable; return an `Err` through `typography_layer` instead.

### Phase 3 — Location + diagnostic (`quarto-sass`, `quarto-core`)

*Implemented 2026-09-08.* Path form: `load_split_brand` validates after the serde parse and,
on failure, re-parses the text with `quarto_yaml::parse_file(text, full_path)` and walks the
`BrandPath` to the node (`locate_in_brand_yaml`). Inline form: validated at *extraction* time
in `extract_single_brand_ref`, while the `ConfigValue` tree is in scope
(`locate_in_config_value`; a `Scalar(Yaml::Hash)` block falls back to the block's own span).
The variant carries `brand_file` so `theme_diagnostic` can add it as a bind candidate itself —
the stage's candidate list does not know about `_brand.yml`. The duplicate `brand_err` in
`config.rs` was removed in favour of `brand_layer::brand_err`, which now maps
`InvalidFontWeight` too (the location-less emission backstop).


- [x] `quarto-sass` depends on `quarto-yaml`; `load_split_brand` runs `Brand::validate()`,
      maps each failure's YAML path to a `SourceInfo` via `quarto_yaml::parse_file` (path
      form) or the `ConfigValue` (inline form).
- [x] `SassError::InvalidBrandFontWeight { message, location: Option<SourceInfo> }`;
      `with_location` covers it.
- [x] `theme_diagnostic.rs`: `Q-14-8` arm; brand file path added to the bind candidates (the
      stage knows `brand_ref` and the project dir). **Land after bd-jsvetdea** to avoid a
      conflict in this file.
- [x] Route the two `stage_error("brand resolution: …")` sites in `compile_theme_css.rs`
      through `theme_diagnostic` so the code renders (today they are code-less).

### Phase 4 — Catalog + docs + lint

- [x] `error_catalog.json`: `Q-14-8` (subsystem `theme`); `docs/errors/theme/Q-14-8.qmd`;
      sidebar entry in code order; `cargo xtask lint` clean.
- [x] `docs/guides/authoring/brand.qmd`: the `weight` bullet — slots take one weight;
      `fonts[].weight` accepts a number, keyword, list, or `N..M` (google/bunny/file); bunny
      is expanded to discrete weights; unknown strings are errors. Note for bd-qnylgu69.

### Phase 5 — End-to-end verification

- [x] `cargo run --bin q2 -- render` each fixture; record `@import` / `@font-face` output and
      the `Q-14-8` stderr in this plan (below). *Browser check of synthetic vs variable bold
      not done — the fixture's local `.woff2` does not exist, and the Google request is
      verified by the URL form Google documents for variable axes.*
- [x] `cargo nextest run --workspace`; `cargo xtask verify --skip-hub-tests` green through
      Rust build, clippy (`-D warnings`), workspace tests, ts-packages, hub-client `build:all`
      (WASM) and the q2-preview-spa build (2026-09-08). Hub-client vitest skipped: Node 26
      `localStorage` environment failure, bd-lh30hlvd. The preview-renderer integration
      suite failed once on a stale local `node_modules` (KaTeX 0.17 installed, 0.18.4 pinned)
      and passed after `npm install` (640 tests).
- [x] PR #663 merged 2026-09-08 (`9a49ba4ee`); commented on bd-qnylgu69 with the docs change.
      Closing bd-5fseopxy is the only remaining bookkeeping step, done in braid, not in the repo.

## Risks / tradeoffs

- **Coordination with bd-jsvetdea.** Codes `Q-14-6`/`Q-14-7` are taken; `theme_diagnostic.rs`
  and `quarto-sass/src/error.rs` are being edited in room-3. Prefer landing this after that
  merges (decision 4 adds one `theme_diagnostic` arm and one `SassError` variant).
- **`source: file` handling.** Another agent suspects deeper bugs there (user, 2026-09-08). This
  strand changes only the `font-weight` line; anything else found goes to that plan.
- **Turning silent into fatal.** Any existing brand with a typo'd weight (`Bold`, `semibold`)
  currently renders at 400; after the fix it fails the render. That is the intended Q1-parity
  behavior, but worth a release-note line.
- **Bunny has no range syntax.** Checked 2026-09-08: `fonts.bunny.net/css?family=inter:400..700`
  returns `@font-face` rules for weight 400 only — the range is silently ignored, the same
  failure mode this strand is fixing. For `source: bunny` a range must therefore be **expanded
  to discrete weights** (`400,500,600,700`) rather than passed through. Google is the only
  source that serves a true variable axis via `wght@N..M`.
- **WASM leg.** `quarto-brand` and `quarto-sass` are in the hub-client WASM closure; full
  `cargo xtask verify` is required, not `--skip-hub-build`.
- **Doc drift.** `brand.qmd` links to a nonexistent reference page; that belongs to
  bd-qnylgu69, not here.

## End-to-end verification (2026-09-08, real binary, output inspected)

All four fixtures under `claude-notes/plans/brand-font-weight-ranges-investigation/`, rendered
with `cargo run -q --bin q2 -- render <dir>` on the topic branch:

```
$ … render repro-google-only        # weight: 400..700 on a google font
Rendered 1 of 1 files
quarto-theme-27d9b6d07ebeed24.css   334046 bytes
@import url("https://fonts.googleapis.com/css2?family=EB+Garamond:ital,wght@0,400..700;1,400..700&display=swap")

$ … render repro-fonts-only         # + files[0].weight: 300..800 on a local file
Rendered 1 of 1 files
quarto-theme-dfd19eb23f7f5b75.css   334163 bytes
@import url("https://fonts.googleapis.com/css2?family=EB+Garamond:ital,wght@0,400..700;1,400..700&display=swap")
@font-face{font-family:"Local Var";src:url("LocalVar-VariableFont_wght.woff2");font-weight:300 800;font-style:normal}

$ … render repro-slot-only          # headings: {weight: 500..700}
Error: [Q-14-8] Invalid brand font weight
   ╭─[ …/repro-slot-only/_brand.yml:9:13 ]
 9 │     weight: 500..700
   │             ────┬───
   │                 ╰───── `500..700` at `typography.headings.weight` is not a font weight Quarto
                            understands: a typography slot takes a single weight, not a range; …
ℹ Use a number from 100 to 900, a keyword such as `bold` or `semi-bold`, a list of those, or —
  on `typography.fonts` entries — a numeric range such as `400..700`?
Rendered 0 of 1 files — 1 error
```

Before the fix the first two shipped `wght@0,400;1,400` and the 6996-byte `DEFAULT_CSS`
respectively, and the last two rendered "successfully" with `DEFAULT_CSS`.

