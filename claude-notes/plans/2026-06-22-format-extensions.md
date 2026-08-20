# Format Extensions (resolution & apply) — STUB

**Status:** STUB / research — scoping, not yet an implementation plan. Needs a
research pass before it becomes a checklist.
**Part of:** the extensions epic (`claude-notes/plans/2026-03-16-extensions-grand-plan.md`,
Phase 5a).
**Distinct from:** Phase 5 (Custom Writers) — see "Not custom writers" below.
**Created:** 2026-06-22, after an audit found format extensions have no
first-class home in the grand plan (the ingredients exist across Phases 1–4,
but the resolution-and-apply glue is unbuilt, and Phase 5 covers the narrower
custom-writer case).

## What a format extension is

A **format extension** is the most common Quarto 1 extension type — the
`_extension.yml` `contributes: formats:` mechanism, keyed by a **known base
format** (`html`, `pdf`, `typst`, `revealjs`, …). It layers configuration onto
that base: metadata defaults, per-format filters, shortcodes,
`template-partials`, `format-resources`, and SCSS/theme. The canonical examples
are **journal templates** (ACM, AGU, JSS, Elsevier) and presentation themes.

It does **not** define a new Pandoc output target — `acm-pdf` still renders as
`pdf`; the `acm` extension just layers its bundle over the `pdf` base.

### Not custom writers (the Phase 5 distinction)

A **custom writer** (grand-plan Phase 5) is a Pandoc **`.lua` writer** that
*defines a new output target*, detected by `extname(format) === ".lua"`. That
is a separate, rarer mechanism. The two are orthogonal: a format extension
*may* also carry a custom writer (Q1 puts `writer: publish.lua` under a format
key — the confluence example), in which case the `.lua`-writer path routes to
Phase 5. **Most real Q1 extensions are format extensions, not custom writers**,
which is why this needs its own plan rather than being folded into Phase 5.

## How Quarto 1 does it (reference)

Resolution + apply, for `quarto render --to acm-pdf`:

1. **Parse the format string** (`src/core/pandoc/pandoc-formats.ts`
   `parseFormatString`): `"acm-pdf"` → `baseFormat: "pdf"`, `extension: "acm"`
   (+ any `+modifiers`). The base is split off the end; the prefix is the
   extension name.
2. **Look up the extension and read its format bundle**
   (`src/command/render/render-contexts.ts` `readExtensionFormat`): find the
   `acm` extension, read `contributes.formats[fmtTarget] || formats[baseFormat]`,
   merged with the special `formats.common` key.
3. **Merge** (`mergeFormatMetadata`): `defaultWriterFormat(base)` →
   extension format metadata → user front-matter. (base defaults, then
   extension, then user — last wins.)
4. **Apply the bundle**: inject the extension's per-format `filters`
   (honoring `at: pre-quarto | post-quarto`) and `shortcodes`; resolve
   `template-partials` relative to the extension dir; copy `format-resources`
   (`.cls`, `.sty`, …) into the output dir; layer SCSS/`theme`; register any
   `revealjs-plugins`.

Real example (AGU journal `_extension.yml`): a `common` block (csl, filters,
number-sections) plus per-format `pdf` (documentclass, header-includes,
`template-partials`, `format-resources`) and `html` (toc) blocks.

## What q2 already has (the ingredients)

- **`Contributes.formats: HashMap<String, ConfigValue>`** is parsed today
  (Phase 1; `crates/quarto-core/src/extension/types.rs`).
- **`parse_format_descriptor()`** (`extension/discover.rs`) splits `acm-html` →
  `extension_name="acm"`, `base_format="html"`, against a `KNOWN_BASE_FORMATS`
  list.
- **`Format::from_format_string()`** (`format.rs`) already has the extension
  path: `acm-html` → `FormatIdentifier::Html` + `extension_name="acm"` (so a
  format extension resolves to a *known* base format plus `extension_name` —
  never a synthetic or unknown format identifier).
- **`Format`** carries `target_format`, `extension_name`, `display_name`.
- **Phases 1–4 (merged)** give the apply-side primitives: metadata merge
  (`MetadataMergeStage`), extension-aware **filter resolution**
  (`filter_resolve.rs`, Phase 2), **shortcode resolution** (Phase 3),
  **templates & partials** (`ApplyTemplateStage`, Phase 4).
- **Adjacent machinery**: the SCSS/theme pipeline (`CompileThemeCssStage`,
  `resources/scss/`) and the resource-report / `OutputSink` copying path
  (bd-o8pr) are plausible homes for SCSS layering and `format-resources`
  copying.

## What's missing (the glue)

1. **Extension context is not wired into format resolution.**
   `Format::from_format_string()` is called *without* the discovered
   extensions, so it parses `acm-html` into base + `extension_name` but never
   *loads* the `acm` extension's `contributes.formats.html` bundle. Q1 threads
   the `ExtensionContext` into format resolution; q2 does not.
2. **No application of the per-format bundle as a unit.** The `common` + base
   merge, the per-format `filters`/`shortcodes`/`template-partials`/
   `format-resources`/SCSS that belong to *this* format key, are not assembled
   and applied when resolving `acm-html`.
3. **No `format-resources` copying** for format extensions (the `.cls`/`.sty`
   case).
4. **No validation** that the named extension actually contributes the
   requested base format (Q1 errors loudly; q2 should match).

## Open questions (to resolve in the research pass)

- **Where does extension-aware format resolution live?** Extend
  `Format::from_format_string` to take `&[Extension]`, or a dedicated
  resolution step after extension discovery that loads + merges the format
  bundle? (q2's `ProjectContext` already holds discovered extensions.)
- **Do per-format `filters`/`shortcodes` route through the existing Phase 2/3
  resolvers** (which already do extension lookup), or do format extensions
  need a distinct path? Likely reuse — confirm.
- **`format-resources` copying** — reuse the resource-report / `OutputSink`
  machinery (bd-o8pr), including its `ArtifactScope::Project` handling for
  website renders?
- **SCSS/theme layering** — how does an extension's `theme`/SCSS array layer
  into `CompileThemeCssStage` + `resources/scss/`? (HTML/revealjs formats only.)
- **Merge precedence** — `common` + base + user must compose with q2's
  `MergedConfig` layering; watch the `as_array`/`as_value` kind-drop behavior
  the multi-engine merge work documented (a higher array layer drops lower
  scalar layers).
- **Custom-writer seam** — a format extension carrying `writer: x.lua` routes
  to Phase 5; define the seam, defer the implementation.
- **revealjs-plugins seam** — a format extension carrying `revealjs-plugins`
  routes to Phase 6; define the seam.
- **WASM** — `format-resources` copying and SCSS in the WASM/preview path.
- **Project pipeline** — interaction with the two-pass orchestrator /
  `DocumentProfile` when a format extension is active project-wide.

## Rough shape (to be turned into phases after research)

- **A — Resolve:** wire extension context into format resolution; `<ext>-<base>`
  loads the extension's `formats[base]` (+ `common`) bundle.
- **B — Apply:** metadata merge (base→ext→user); per-format filters/shortcodes
  (reuse Phases 2/3); `template-partials` (Phase 4); `format-resources` copying;
  SCSS/theme layering.
- **C — Validate + test:** loud error when the extension lacks the base format;
  end-to-end test against a real journal fixture (ACM/AGU).
- **Seams (defer):** custom writer → Phase 5; `revealjs-plugins` → Phase 6.

## References

- Grand plan: `claude-notes/plans/2026-03-16-extensions-grand-plan.md` (Phase 5a / Phase 5 / Phase 6).
- Q1 format resolution: `~/src/quarto-cli/src/core/pandoc/pandoc-formats.ts`
  (`parseFormatString`), `src/command/render/render-contexts.ts`
  (`readExtensionFormat`, the merge), `src/extension/extension.ts`
  (format/custom-writer handling).
- Q1 schema: `~/src/quarto-cli/src/resources/schema/extension.yml` (`contributes.formats`).
- q2 ingredients: `crates/quarto-core/src/extension/{types.rs,discover.rs,read.rs,filter_resolve.rs}`,
  `crates/quarto-core/src/format.rs`, `CompileThemeCssStage`, `ApplyTemplateStage`, `resources/scss/`.
