# SASS Parity Testing: Known Differences

This document tracks known differences between grass (Rust SASS compiler) and dart-sass
(reference implementation) when compiling Bootstrap 5.3.1 and Bootswatch themes.

## Parity Test Results

### Bootstrap 5.3.1

| Metric | grass | dart-sass | Difference |
|--------|-------|-----------|------------|
| Expanded size | 287,948 bytes | 289,472 bytes | -0.53% |
| Minified size | 241,402 bytes | 243,437 bytes | -0.84% |
| Missing selectors | 0 | - | - |
| Extra selectors | 0 | - | - |

**Summary**: grass produces slightly smaller CSS due to minor whitespace differences.
Semantically equivalent (no missing or extra selectors).

### Bootswatch Themes

All 18 tested themes pass parity testing with 0 missing selectors:

| Theme | Size Difference |
|-------|----------------|
| cerulean | -0.77% |
| cosmo | -0.55% |
| darkly | -1.13% |
| flatly | -0.55% |
| journal | -0.46% |
| litera | -0.60% |
| lux | -0.49% |
| materia | -0.54% |
| minty | -0.49% |
| morph | -1.46% |
| pulse | -0.53% |
| quartz | -0.39% |
| sandstone | -0.54% |
| solar | -0.48% |
| spacelab | -1.08% |
| united | -0.59% |
| yeti | -0.62% |
| zephyr | -0.50% |

## Themes Not Tested

The following 7 Bootswatch themes failed to compile with our current assembly order.
These themes have complex dependencies that require more sophisticated layer assembly
than our simple approach provides:

### cyborg, slate, superhero
**Issue**: These themes redefine Bootstrap's `color-contrast()` function with default
parameters that reference variables (`$color-contrast-dark`, `$color-contrast-light`).
The function definition is processed before the variables are defined.

**Error**: `Undefined variable $color-contrast-dark`

### lumen, simplex
**Issue**: These themes use `shade-color()` with a variable that ends up being `null`.
The variable is not properly initialized before use.

**Error**: `$color2: null is not a color`

### sketchy
**Issue**: This theme uses a custom variable `$shiny-check` that is defined elsewhere
in the theme's rules section, but the reference occurs before the definition.

**Error**: `Undefined variable $shiny-check`

### vapor
**Issue**: This theme passes a box-shadow value where a color is expected.
This appears to be a variable naming collision or incorrect variable reference.

**Error**: `$color is not a color`

## Resolution Plan

These theme issues are not blockers for the SASS infrastructure. They relate to how
TS Quarto performs more sophisticated layer assembly that handles:

1. Function definitions that reference variables (need variables defined first)
2. Circular dependencies between defaults and rules sections
3. Theme-specific variables that need careful ordering

The solution will involve porting TS Quarto's full layer assembly logic, which is
tracked separately in Phase 6 (Bootstrap Integration) of the SASS implementation plan.

## Regenerating Fixtures

To regenerate the dart-sass reference fixtures:

```bash
cd /path/to/kyoto
npm install --no-save sass
node scripts/generate-sass-fixtures.mjs
```

The fixtures are stored in `crates/quarto-sass/test-fixtures/dart-sass/`.
