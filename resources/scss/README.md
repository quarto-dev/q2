# SCSS Resources

This directory contains SCSS resources used for Bootstrap/theme compilation in Rust Quarto. These files are copied from the TypeScript Quarto repository (`quarto-cli`) and maintained locally.

## Contents

- `bootstrap/` - Bootstrap 5.3.1 SCSS and Quarto's Bootstrap customization layer
  - `dist/scss/` - Bootstrap 5.3.1 SCSS distribution
  - `dist/sass-utils/` - Bootstrap SASS utility functions
  - `themes/` - Bootswatch theme files (25 themes)
  - `_bootstrap-*.scss` - Quarto's Bootstrap customization files
- `html/templates/` - HTML template SCSS files
  - `title-block.scss` - Title block styling

## Source

These files are copied from:
- `quarto-cli/src/resources/formats/html/bootstrap/` (Bootstrap and themes)
- `quarto-cli/src/resources/formats/html/templates/` (title-block.scss)

## Updating

To update these files when quarto-cli updates Bootstrap or themes:

1. Copy the updated files from quarto-cli:
   ```bash
   # From repository root, with quarto-cli checked out at external-sources/quarto-cli
   cp -r external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/scss resources/scss/bootstrap/dist/
   cp -r external-sources/quarto-cli/src/resources/formats/html/bootstrap/dist/sass-utils resources/scss/bootstrap/dist/
   cp -r external-sources/quarto-cli/src/resources/formats/html/bootstrap/themes resources/scss/bootstrap/
   cp external-sources/quarto-cli/src/resources/formats/html/bootstrap/_bootstrap-*.scss resources/scss/bootstrap/
   cp external-sources/quarto-cli/src/resources/formats/html/templates/title-block.scss resources/scss/html/templates/
   ```

2. Regenerate the dart-sass fixtures:
   ```bash
   npm install --no-save sass
   node scripts/generate-sass-fixtures.mjs
   ```

3. Run the parity tests:
   ```bash
   cargo nextest run -p quarto-sass parity
   ```

## Why Local Copy?

These files are maintained as a local copy rather than referencing `external-sources/` directly because:

1. **Build reproducibility**: The build should work without external-sources being checked out
2. **Embedded at compile time**: Files are embedded into the binary via `include_dir!`
3. **Version control**: Changes to these resources are tracked in the repository
4. **CI/CD compatibility**: CI builds don't need to check out quarto-cli

## License

Bootstrap is licensed under the MIT License.
Bootswatch themes are licensed under the MIT License.
