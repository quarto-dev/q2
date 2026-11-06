# Execution-unit conformance fixtures (EXR-2 Phase 3)

Each `<name>.qmd` is an input document. `<name>.units.json` lists, in
document order, the units `quarto_core::engine::units::scan_units` must find.
`tests/integration/unit_fixtures.rs` loads every pair.

## Format

A JSON array. Each element:

| field        | meaning                                                         |
| ------------ | --------------------------------------------------------------- |
| `kind`       | `"cell"` (braced fence) or `"span"` (inline `` `{lang} expr` ``) |
| `lang`       | the language token as written (`python.marimo`, `Python`, `R`)  |
| `index`      | cells only: document-wide, counts every braced non-raw fence    |
| `span_index` | spans only: document-wide, counts every span, owned or not      |
| `line`       | 1-based line of the unit's first byte                           |
| `code`       | the cell body (container prefixes and `\r` kept) or the span expression |

EXR-4 later adds `content_hash` and `cell_id`. No ownership is recorded: a
fixture lists everything braced; each engine's view decides what it owns.

Not a snapshot: when the scanner changes deliberately, regenerate with
`UPDATE_UNIT_FIXTURES=1 cargo nextest run -p quarto-core -E 'test(unit_fixtures::)'`
and review the JSON diff by hand.

## Running the same files elsewhere

Positron's side (EXR-16 spike S1) reads the same `.qmd` files and compares its
own cell list to `kind == "cell"` entries (`lang`, `index`, `line`, `code`). The
fixtures do not yet carry a `positron_index`; EXR-16 S1 fills one in where
Positron's parser differs on nesting (epic decision 39). Nothing is sent from
here.

## Expected divergences, recorded 2026-10-31

AST walker (`engine_cell_lang` over pampa's parse) versus the scanner, in
`unit_fixtures.rs::EXPECTED_DIVERGENCES`:

* `cell_unclosed`: pampa (CommonMark) runs an unclosed fence to the end of the
  document, so the AST has a cell; the scanner yields none.

The Quarto 1 TypeScript scanners, run with Deno over this corpus
(`breakQuartoMd` from `src/core/lib/break-quarto-md.ts`; the regex of
`languagesWithClasses` from `src/core/pandoc/pandoc-partition.ts:149`). Not
asserted by any test; the record of how they differ:

* `breakQuartoMd` misses the blockquote-prefixed cell in `cells_nested` (it
  finds the list, four-backtick, indented and top-level ones), and misses
  `{py-thon}` in `cells_dotted_and_class` (its language class has no hyphen).
  It reports raw `{=html}` fences as cells (`span_in_any_fence`, `cells_basic`);
  the scanner does not. It finds no cell in `cell_unclosed`, like the scanner.
* `languagesWithClasses` is line-based with no fence awareness: it also reports
  the `{python}` shown inside a display fence (`cell_inside_display_fence`),
  reports an unclosed fence (`cell_unclosed`), and reads `{py-thon}` as `py`.
  It returns a set of languages, so it has no indices at all. It finds no spans.
* Neither TypeScript scanner finds inline spans.
