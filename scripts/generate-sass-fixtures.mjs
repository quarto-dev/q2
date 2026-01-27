#!/usr/bin/env node
/**
 * Generate dart-sass reference fixtures for SASS parity testing.
 *
 * This script compiles Bootstrap 5.3.1 and Bootswatch themes using dart-sass
 * (via the npm `sass` package) and saves the output as reference fixtures for
 * comparing against grass (the Rust SASS compiler).
 *
 * Usage:
 *   cd /path/to/kyoto
 *   npm install sass  # or: npx sass (will prompt to install)
 *   node scripts/generate-sass-fixtures.mjs
 *
 * The script will create fixtures in:
 *   crates/quarto-sass/test-fixtures/dart-sass/
 *
 * Copyright (c) 2025 Posit, PBC
 */

import * as sass from 'sass';
import * as fs from 'fs';
import * as path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// Paths
const ROOT_DIR = path.resolve(__dirname, '..');
const RESOURCES_DIR = path.join(ROOT_DIR, 'resources/scss/bootstrap');
const BOOTSTRAP_SCSS_DIR = path.join(RESOURCES_DIR, 'dist/scss');
const THEMES_DIR = path.join(RESOURCES_DIR, 'themes');
const FIXTURES_DIR = path.join(ROOT_DIR, 'crates/quarto-sass/test-fixtures/dart-sass');

// Bootswatch themes
const THEMES = [
  'cerulean', 'cosmo', 'cyborg', 'darkly', 'flatly', 'journal', 'litera',
  'lumen', 'lux', 'materia', 'minty', 'morph', 'pulse', 'quartz',
  'sandstone', 'simplex', 'sketchy', 'slate', 'solar', 'spacelab',
  'superhero', 'united', 'vapor', 'yeti', 'zephyr'
];

/**
 * Assemble Bootstrap SCSS in the correct layer order.
 * Bootstrap must be assembled from separate files (functions, variables, mixins, rules)
 * rather than compiled directly from bootstrap.scss.
 */
function assembleBootstrapScss() {
  const functions = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, '_functions.scss'), 'utf8');
  const variables = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, '_variables.scss'), 'utf8');
  const mixins = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, '_mixins.scss'), 'utf8');
  const rules = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, 'bootstrap.scss'), 'utf8');

  return `// Functions\n${functions}\n\n// Variables\n${variables}\n\n// Mixins\n${mixins}\n\n// Rules\n${rules}`;
}

/**
 * Parse a theme file that uses layer boundary markers.
 *
 * Theme files use Quarto's layer boundary syntax:
 * - /\*-- scss:defaults --*\/ marks variable overrides
 * - /\*-- scss:rules --*\/ marks CSS rules that use Bootstrap mixins
 *
 * Returns { defaults: string, rules: string }
 */
function parseThemeLayers(content) {
  const layers = {
    defaults: '',
    rules: '',
    uses: '',
    functions: '',
    mixins: ''
  };

  // Regex to match layer boundaries
  const boundaryRegex = /^\/\*--\s*scss:(uses|functions|defaults|mixins|rules)\s*--\*\/$/;

  let currentLayer = 'defaults'; // Content before any marker goes to defaults
  const lines = content.split('\n');
  const sectionLines = {
    defaults: [],
    rules: [],
    uses: [],
    functions: [],
    mixins: []
  };

  for (const line of lines) {
    const match = line.match(boundaryRegex);
    if (match) {
      currentLayer = match[1];
    } else {
      sectionLines[currentLayer].push(line);
    }
  }

  for (const [key, lines] of Object.entries(sectionLines)) {
    layers[key] = lines.join('\n');
  }

  return layers;
}

/**
 * Assemble a Bootswatch theme with Bootstrap.
 *
 * Theme files use layer boundaries to separate:
 * - defaults: variable overrides (placed BEFORE Bootstrap variables)
 * - rules: CSS rules using Bootstrap mixins (placed AFTER Bootstrap mixins)
 *
 * Assembly order:
 * 1. Bootstrap functions
 * 2. Theme defaults (variable overrides - take precedence via !default)
 * 3. Bootstrap variables
 * 4. Bootstrap mixins
 * 5. Theme rules (use Bootstrap mixins)
 * 6. Bootstrap rules
 */
function assembleThemeScss(themeName) {
  const themePath = path.join(THEMES_DIR, `${themeName}.scss`);
  const themeContent = fs.readFileSync(themePath, 'utf8');

  // Parse theme into layers
  const themeLayers = parseThemeLayers(themeContent);

  // Read Bootstrap components
  const functions = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, '_functions.scss'), 'utf8');
  const variables = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, '_variables.scss'), 'utf8');
  const mixins = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, '_mixins.scss'), 'utf8');
  const rules = fs.readFileSync(path.join(BOOTSTRAP_SCSS_DIR, 'bootstrap.scss'), 'utf8');

  // Assemble in correct order:
  // 1. Bootstrap functions (needed by everything)
  // 2. Theme defaults (variable overrides - !default means first wins)
  // 3. Bootstrap variables (use !default, so theme values take precedence)
  // 4. Bootstrap mixins (needed by theme rules)
  // 5. Theme rules (CSS rules that may use Bootstrap mixins)
  // 6. Bootstrap rules
  const parts = [
    '// Bootstrap Functions',
    functions,
    '',
    '// Theme Defaults',
    themeLayers.defaults,
    '',
    '// Bootstrap Variables',
    variables,
    '',
    '// Bootstrap Mixins',
    mixins,
    '',
    '// Theme Rules',
    themeLayers.rules,
    '',
    '// Bootstrap Rules',
    rules
  ];

  return parts.join('\n');
}

/**
 * Compile SCSS to CSS using dart-sass.
 */
function compileSass(scss, loadPaths, minified = false) {
  const result = sass.compileString(scss, {
    loadPaths: loadPaths,
    style: minified ? 'compressed' : 'expanded',
    silenceDeprecations: ['global-builtin', 'color-functions', 'import'],
    logger: {
      warn: () => {}, // Suppress warnings
      debug: () => {}
    }
  });
  return result.css;
}

/**
 * Main entry point.
 */
async function main() {
  console.log('Generating dart-sass reference fixtures...\n');

  // Check if resources/scss exists
  if (!fs.existsSync(BOOTSTRAP_SCSS_DIR)) {
    console.error(`Error: Bootstrap SCSS not found at ${BOOTSTRAP_SCSS_DIR}`);
    console.error('Make sure resources/scss/bootstrap is present.');
    process.exit(1);
  }

  // Create fixtures directory
  fs.mkdirSync(FIXTURES_DIR, { recursive: true });
  fs.mkdirSync(path.join(FIXTURES_DIR, 'themes'), { recursive: true });

  const loadPaths = [BOOTSTRAP_SCSS_DIR];
  const results = {
    dartsass_version: sass.info,
    generated_at: new Date().toISOString(),
    bootstrap: {},
    themes: {}
  };

  // Compile Bootstrap (expanded and minified)
  console.log('Compiling Bootstrap 5.3.1...');
  const bootstrapScss = assembleBootstrapScss();

  try {
    const bootstrapExpanded = compileSass(bootstrapScss, loadPaths, false);
    const bootstrapMinified = compileSass(bootstrapScss, loadPaths, true);

    fs.writeFileSync(path.join(FIXTURES_DIR, 'bootstrap.css'), bootstrapExpanded);
    fs.writeFileSync(path.join(FIXTURES_DIR, 'bootstrap.min.css'), bootstrapMinified);

    results.bootstrap = {
      expanded_size: bootstrapExpanded.length,
      minified_size: bootstrapMinified.length,
      expanded_lines: bootstrapExpanded.split('\n').length,
      minified_lines: bootstrapMinified.split('\n').length
    };

    console.log(`  Expanded: ${bootstrapExpanded.length} bytes, ${results.bootstrap.expanded_lines} lines`);
    console.log(`  Minified: ${bootstrapMinified.length} bytes, ${results.bootstrap.minified_lines} lines`);
  } catch (err) {
    console.error(`  Error compiling Bootstrap: ${err.message}`);
    process.exit(1);
  }

  // Compile each Bootswatch theme
  console.log('\nCompiling Bootswatch themes...');
  for (const theme of THEMES) {
    process.stdout.write(`  ${theme}...`);

    try {
      const themeScss = assembleThemeScss(theme);
      const themeExpanded = compileSass(themeScss, loadPaths, false);
      const themeMinified = compileSass(themeScss, loadPaths, true);

      fs.writeFileSync(path.join(FIXTURES_DIR, 'themes', `${theme}.css`), themeExpanded);
      fs.writeFileSync(path.join(FIXTURES_DIR, 'themes', `${theme}.min.css`), themeMinified);

      results.themes[theme] = {
        expanded_size: themeExpanded.length,
        minified_size: themeMinified.length
      };

      console.log(` ${themeExpanded.length} bytes`);
    } catch (err) {
      console.log(` ERROR: ${err.message}`);
      results.themes[theme] = { error: err.message };
    }
  }

  // Write metadata
  fs.writeFileSync(
    path.join(FIXTURES_DIR, 'manifest.json'),
    JSON.stringify(results, null, 2)
  );

  console.log('\nDone! Fixtures written to:');
  console.log(`  ${FIXTURES_DIR}`);
  console.log(`\nManifest: ${path.join(FIXTURES_DIR, 'manifest.json')}`);
}

main().catch(err => {
  console.error('Fatal error:', err);
  process.exit(1);
});
