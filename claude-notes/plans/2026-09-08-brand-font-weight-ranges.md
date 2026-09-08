# brand.yml font weight ranges collapse to 400; no variable-font axis support (bd-5fseopxy)

**Date:** 2026-09-08
**Braid:** bd-5fseopxy (bug, p2, labels: css, diagnostics, theming)
**Checkout:** `~/rooms/room-1/q2`, branch `main` @ `b7e7c96a` (no worktree/branch created; the
user picks where the fix lands)
**Status:** Investigation — pending design alignment with user. **Do not start implementation
until the user gives the go-ahead.**

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

## Proposed phases (draft)

Skeleton only — contents depend on the design answers below.

- **Phase 0 — Tests (TDD, written first, verified failing).**
  `quarto-brand` unit tests: `400..700` parses to a range; `bold..black` parses via keywords;
  `700..400`, `450..700`, `400..`, `bolder` are rejected with a message naming the value.
  `quarto-sass/tests/integration/brand_layer_test.rs`: google emits `wght@400..700`
  (and the `ital,wght@0,400..700;1,400..700` form); bunny expands to `400,500,600,700`; file emits
  `font-weight: 300 800`; a slot range is an `Err`, not `$…: 400..700`. A brand-compile test
  showing the full bundle compiles with the range brand. CLI e2e: render the repro fixture,
  grep the emitted CSS for the axis form; a bad weight string exits non-zero with the new code.
- **Phase 1 — Type.** `BrandFontWeight::Range(u32, u32)` (or a dedicated
  `BrandFontWeightRange` used by `fonts[].weight` and `files[].weight` only), with a custom
  `Deserialize` that resolves keywords at parse time and rejects unknown strings and malformed
  ranges. Move `weight_name_to_number` into `quarto-brand` so parsing and emission share one
  table.
- **Phase 2 — Emission.** `enumerate_weights` → a `WeightSpec` that carries discrete values
  *or* a range; google/bunny builders emit the axis form; `file_font_face_block` emits
  `font-weight: N M`; `font_weight_to_scss` rejects ranges for slots (or the slot type simply
  cannot hold one, per Phase 1).
- **Phase 3 — Diagnostics.** Unknown weight strings and slot ranges become a structured error
  (code TBD, question 4), naming the family/slot and the offending value.
- **Phase 4 — Docs + catalog + lint.** `docs/guides/authoring/brand.qmd` weight bullet;
  `error_catalog.json` + `docs/errors/theme/Q-14-N.qmd` + sidebar entry; `cargo xtask lint`.
- **Phase 5 — E2E.** Render the repro with the real binary; record the `@import` and
  `@font-face` output here; open the page and confirm bold renders in the variable font (not
  synthetic bold); `cargo xtask verify` (full; `quarto-brand`/`quarto-sass` feed the WASM leg).

## Open design questions for the user

1. **Scope of ranges.** Google + Bunny only (what the spec prose says), or also `source: file`
   per-file weights emitted as `font-weight: N M` (what the Python reference does, and what
   the strand asks for)? Recommendation: include file — it is a three-line change and is the
   only way to declare a local `*-VariableFont_wght.woff2` correctly.
2. **Strictness on the range itself.** Reject `700..400` (reversed), `450..700` (not a multiple
   of 100), `400..400` (degenerate)? Recommendation: reject reversed and out-of-[100,900];
   allow any integer in range (CSS allows 1–1000, Google serves non-round values) and allow
   `N..N`. Keyword ends (`regular..bold`) accepted, matching the reference.
3. **Unknown weight strings.** Q1 fails hard on any string outside the keyword table. Make Q2
   do the same (a hard error, never a silent 400)? This is the strand's item 4 and I recommend
   yes. It also covers `weight: bolder`/`lighter`, which the spec does not allow.
4. **Error code and plumbing.** Options: (a) a new `Q-14-8` "Invalid brand font weight" carried
   by a new `SassError::InvalidBrandValue`-style variant, or (b) reuse `Q-14-1` (Invalid theme
   configuration) via the existing `brand_err → InvalidThemeConfig` mapping, and separately
   route the brand-resolution `stage_error` sites through `theme_diagnostic` so it renders with
   its code. (b) is smaller and stays clear of room-3's `theme_diagnostic.rs` edits; (a) gives
   the user a dedicated docs page. Either way there is no `_brand.yml` span to point at — is a
   message like `font "EB Garamond": invalid weight "400..7"` acceptable, or should this wait
   for source-located brand parsing (quarto-yaml instead of serde_yaml — a much larger change)?
5. **Slot ranges.** `headings: {weight: 500..700}` is not meaningful (a slot picks one weight).
   Hard error, or accept and use the lower bound with a warning? Recommendation: hard error,
   same code as question 4.
6. **Default weights (out of scope unless you say otherwise).** Q1/Q2 request `400;700` when
   `weight` is absent; the brand.yml reference requests all nine. Leave as is (Q1 parity) and
   file a separate strand, or fold it in?

## Risks / tradeoffs (draft)

- **Coordination with bd-jsvetdea.** Codes `Q-14-6`/`Q-14-7` are taken; `theme_diagnostic.rs`
  and `quarto-sass/src/error.rs` are being edited in room-3. Prefer landing this after that
  merges, or keep this strand's diagnostics out of those two files (option 4b).
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
