# TypeScript Packages

This directory contains standalone TypeScript packages associated with the Kyoto Rust workspace.

Following the convention used in Rust monorepos (similar to how `target/` contains build artifacts),
this `ts-packages/` directory contains TypeScript packages that complement the Rust crates.

## Packages

- **annotated-qmd** (`@quarto/annotated-qmd`): Converts quarto-markdown-pandoc JSON output
  to AnnotatedParse structures compatible with quarto-cli's YAML validation infrastructure.

## Development

Each package is independent with its own `package.json` and can be developed/tested separately:

```bash
cd ts-packages/annotated-qmd
npm install
npm test
npm run build
```
