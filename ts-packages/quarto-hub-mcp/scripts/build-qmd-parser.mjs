#!/usr/bin/env node
/**
 * CLI wrapper for the shared pkg builder: `node scripts/build-qmd-parser.mjs [--force]`.
 * See scripts/qmd-parser-pkg.mjs for the freshness model.
 */

import { ensureQmdParserPkg } from './qmd-parser-pkg.mjs';

const force = process.argv.includes('--force');
ensureQmdParserPkg({ force });
