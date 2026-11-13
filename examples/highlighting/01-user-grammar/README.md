# 01-user-grammar — highlighting a language Quarto doesn't ship

A `format: html` document that highlights TOML with a tree-sitter
grammar shipped inside the project, plus a small style rule that
restyles one capture.

## What this demonstrates

- **`_quarto/grammars/<name>/`.** A folder holding `toml.wasm` (the
  grammar compiled to WASM) and `highlights.scm` (its highlight query)
  registers the language `toml`. No `_quarto.yml` setting is needed.
- **The language name comes from the `.wasm` file name.** `toml.wasm`
  serves ` ```toml ` blocks and `` `…`{.toml} `` inline code.
- **Captures become CSS classes.** The query captures keys as `@type`,
  emitted as `<span class="hl-type">`; a `<style>` block in the
  document makes those bold. (It is inline rather than a `css:` file so
  the docs site's staged copy of this example keeps it.)

## How to run

From the repository root:

```bash
cargo run --bin q2 -- render examples/highlighting/01-user-grammar
```

The page is written next to the source as `document.html`.

## What to look for

- `<pre class="sourceCode toml">` wrapping `hl-*` spans: `hl-comment`,
  `hl-type` (keys), `hl-property` (each whole `key = value` pair),
  `hl-string`, `hl-number`, `hl-boolean`, `hl-string-special` (the
  date-time), and `hl-punctuation-bracket`.
- No warnings in the render output. A grammar that fails to load (for
  example a `highlights.scm` naming a node type the grammar doesn't
  have) produces a `failed to load user grammars` warning, and the
  block renders unhighlighted.
- Rename `toml.wasm` to `tomlx.wasm` and the ` ```toml ` block loses
  its highlighting with no warning, because the grammar now serves
  ` ```tomlx ` blocks instead.

The grammar files are documented in `_quarto/grammars/toml/PROVENANCE.md`.
