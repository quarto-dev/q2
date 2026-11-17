# 01-user-grammar — highlighting a language Quarto doesn't ship

A `format: html` document that highlights Java properties files with a
tree-sitter grammar shipped inside the project, plus a small style rule
that restyles one capture.

## What this demonstrates

- **`_quarto/grammars/<name>/`.** A folder holding `properties.wasm` (the
  grammar compiled to WASM) and `highlights.scm` (its highlight query)
  registers the language `properties`. No `_quarto.yml` setting is needed.
- **The language name comes from the `.wasm` file name.** `properties.wasm`
  serves ` ```properties ` blocks and `` `…`{.properties} `` inline code.
- **Captures become CSS classes.** The query captures keys as `@property`,
  emitted as `<span class="hl-property">`; a `<style>` block in the
  document makes those bold. (It is inline rather than a `css:` file so
  the docs site's staged copy of this example keeps it.)

This example used TOML until TOML became a built-in language.

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/highlighting/01-user-grammar
```

The page is written next to the source as `document.html`.

## What to look for

- `<pre class="sourceCode properties">` wrapping `hl-*` spans:
  `hl-comment`, `hl-property` (keys), `hl-punctuation-delimiter` (the dots
  in dotted keys), `hl-operator`, `hl-string` (values), and, for the
  `${HOME}` substitution, `hl-punctuation-special` and `hl-constant`.
- No warnings in the render output. A grammar that fails to load (for
  example a `highlights.scm` naming a node type the grammar doesn't
  have) produces a `failed to load user grammars` warning, and the
  block renders unhighlighted.
- Rename `properties.wasm` to `propertiesx.wasm` and the
  ` ```properties ` block loses its highlighting with no warning, because
  the grammar now serves ` ```propertiesx ` blocks instead.

The grammar files are documented in
`_quarto/grammars/properties/PROVENANCE.md`.
