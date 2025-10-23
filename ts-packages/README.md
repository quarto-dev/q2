# TypeScript Packages

This directory contains standalone TypeScript packages associated with the Kyoto Rust workspace.

Following the convention used in Rust monorepos (similar to how `target/` contains build artifacts),
this `ts-packages/` directory contains TypeScript packages that complement the Rust crates.

## Packages

- **rust-qmd-json** (`@quarto/rust-qmd-json`): Converts quarto-markdown-pandoc JSON output
  to AnnotatedParse structures compatible with quarto-cli's YAML validation infrastructure.

## Development

Each package is independent with its own `package.json` and can be developed/tested separately:

```bash
cd ts-packages/rust-qmd-json
npm install
npm test
npm run build
```

## Publishing

Packages are published to npm under the `@quarto` scope and can be consumed by quarto-cli
and other projects:

```bash
cd ts-packages/rust-qmd-json
npm publish
```
