/**
 * Smoke-all test discovery and frontmatter parsing.
 *
 * Ported from hub-client/src/services/smokeAll.wasm.test.ts for use
 * in Playwright E2E tests.
 */

import { readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { parse as parseYaml } from 'yaml';

const SMOKE_ALL_DIR = resolve(
  import.meta.dirname,
  '../../../crates/quarto/tests/smoke-all',
);

// Tests where printsMessage assertions are skipped because the error
// message format differs between native render_to_file and WASM render_qmd.
const SKIP_PRINTS_MESSAGE: Set<string> = new Set([
  'quarto-test/expected-error.qmd',
]);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface RunConfig {
  skip?: string | boolean;
  ci?: boolean;
  os?: string[];
  not_os?: string[];
}

export interface FormatTestSpec {
  format: string;
  assertions: AssertionSpec[];
  checkWarnings: boolean;
  expectsError: boolean;
}

export type AssertionSpec =
  | { type: 'ensureFileRegexMatches'; matches: string[]; noMatches: string[] }
  | { type: 'ensureHtmlElements'; selectors: string[]; noMatchSelectors: string[] }
  | { type: 'ensureCssRegexMatches'; matches: string[]; noMatches: string[] }
  | { type: 'noErrors' }
  | { type: 'noErrorsOrWarnings' }
  | { type: 'shouldError' }
  | { type: 'printsMessage'; level: string; regex: string; negate: boolean };

export interface DiscoveredTest {
  /** Absolute path to the .qmd file */
  qmdPath: string;
  /** Path relative to SMOKE_ALL_DIR (for display) */
  relPath: string;
  /** Absolute path to the project root (containing _quarto.yml) */
  projectRoot: string;
  /** All project files as { relativePath, content } */
  projectFiles: { path: string; content: string }[];
  /** Which file to render (relative to project root) */
  renderPath: string;
  /** Run config from frontmatter */
  runConfig: RunConfig | null;
  /** Format-specific test specs */
  formatSpecs: FormatTestSpec[];
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/** Recursively find all .qmd files, skipping files starting with _. */
function discoverTestFiles(dir: string): string[] {
  const results: string[] = [];

  function walk(d: string) {
    const entries = readdirSync(d, { withFileTypes: true });
    for (const entry of entries) {
      const full = join(d, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (
        entry.isFile() &&
        entry.name.endsWith('.qmd') &&
        !entry.name.startsWith('_')
      ) {
        results.push(full);
      }
    }
  }

  walk(dir);
  results.sort();
  return results;
}

// ---------------------------------------------------------------------------
// Frontmatter parsing
// ---------------------------------------------------------------------------

function readFrontmatter(content: string): Record<string, unknown> {
  const trimmed = content.trimStart();
  if (!trimmed.startsWith('---')) return {};

  const rest = trimmed.slice(3);
  const end = rest.indexOf('\n---');
  if (end === -1) return {};

  const yamlStr = rest.slice(0, end);
  return (parseYaml(yamlStr) as Record<string, unknown>) ?? {};
}

// ---------------------------------------------------------------------------
// Test spec parsing
// ---------------------------------------------------------------------------

function parseTwoArraySpec(value: unknown): { matches: string[]; noMatches: string[] } {
  if (!Array.isArray(value)) return { matches: [], noMatches: [] };
  const matches = Array.isArray(value[0]) ? (value[0] as string[]) : [];
  const noMatches = value.length > 1 && Array.isArray(value[1]) ? (value[1] as string[]) : [];
  return { matches, noMatches };
}

function parseTestSpecs(
  metadata: Record<string, unknown>,
  options: { skipPrintsMessage?: boolean } = {},
): {
  runConfig: RunConfig | null;
  formatSpecs: FormatTestSpec[];
} {
  const quarto = metadata['_quarto'] as Record<string, unknown> | undefined;
  if (!quarto) return { runConfig: null, formatSpecs: [] };

  const tests = quarto['tests'] as Record<string, unknown> | undefined;
  if (!tests) return { runConfig: null, formatSpecs: [] };

  const runConfig = (tests['run'] as RunConfig) ?? null;

  const formatSpecs: FormatTestSpec[] = [];
  for (const [key, value] of Object.entries(tests)) {
    if (key === 'run') continue;
    formatSpecs.push(parseFormatSpec(key, value as Record<string, unknown>, options));
  }

  return { runConfig, formatSpecs };
}

function parseFormatSpec(
  format: string,
  value: Record<string, unknown>,
  options: { skipPrintsMessage?: boolean } = {},
): FormatTestSpec {
  const assertions: AssertionSpec[] = [];
  let checkWarnings = true;
  let expectsError = false;

  if (value && typeof value === 'object') {
    for (const [key, assertionValue] of Object.entries(value)) {
      switch (key) {
        case 'ensureFileRegexMatches': {
          const { matches, noMatches } = parseTwoArraySpec(assertionValue);
          assertions.push({ type: 'ensureFileRegexMatches', matches, noMatches });
          break;
        }
        case 'ensureHtmlElements': {
          const { matches, noMatches } = parseTwoArraySpec(assertionValue);
          assertions.push({ type: 'ensureHtmlElements', selectors: matches, noMatchSelectors: noMatches });
          break;
        }
        case 'ensureCssRegexMatches': {
          const { matches, noMatches } = parseTwoArraySpec(assertionValue);
          assertions.push({ type: 'ensureCssRegexMatches', matches, noMatches });
          break;
        }
        case 'noErrors':
          checkWarnings = false;
          assertions.push({ type: 'noErrors' });
          break;
        case 'noErrorsOrWarnings':
          checkWarnings = false;
          assertions.push({ type: 'noErrorsOrWarnings' });
          break;
        case 'shouldError':
          checkWarnings = false;
          expectsError = true;
          assertions.push({ type: 'shouldError' });
          break;
        case 'printsMessage': {
          if (!options.skipPrintsMessage) {
            const items = Array.isArray(assertionValue) ? assertionValue : [assertionValue];
            for (const item of items) {
              const pm = item as { level: string; regex: string; negate?: boolean };
              assertions.push({
                type: 'printsMessage',
                level: pm.level,
                regex: pm.regex,
                negate: pm.negate ?? false,
              });
            }
          }
          break;
        }
        case 'fileExists':
        case 'pathDoesNotExist':
        case 'pathDoNotExists':
        case 'folderExists':
          // Filesystem assertions are no-ops in browser
          break;
        default:
          throw new Error(`Unknown assertion type: '${key}' in format '${format}'`);
      }
    }
  }

  return { format, assertions, checkWarnings, expectsError };
}

// ---------------------------------------------------------------------------
// Skip logic
// ---------------------------------------------------------------------------

export function shouldSkip(runConfig: RunConfig | null): string | null {
  if (!runConfig) return null;

  if (runConfig.skip) {
    return typeof runConfig.skip === 'string' ? runConfig.skip : 'skip: true';
  }

  if (runConfig.ci === false && (process.env.CI || process.env.GITHUB_ACTIONS)) {
    return 'tests.run.ci is false';
  }

  const currentOs =
    process.platform === 'darwin'
      ? 'darwin'
      : process.platform === 'win32'
        ? 'windows'
        : 'linux';

  if (runConfig.os && !runConfig.os.includes(currentOs)) {
    return `tests.run.os does not include ${currentOs}`;
  }
  if (runConfig.not_os && runConfig.not_os.includes(currentOs)) {
    return `tests.run.not_os includes ${currentOs}`;
  }

  return null;
}

// ---------------------------------------------------------------------------
// Project file reading
// ---------------------------------------------------------------------------

/** Find project root by walking up from qmdDir looking for _quarto.yml. */
function findProjectRoot(qmdDir: string): string {
  let dir = qmdDir;
  while (dir.startsWith(SMOKE_ALL_DIR)) {
    try {
      statSync(join(dir, '_quarto.yml'));
      return dir;
    } catch {
      const parent = dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
  }
  return qmdDir;
}

/** Recursively read all files in a directory. */
function readAllFiles(dir: string): { path: string; content: string }[] {
  const files: { path: string; content: string }[] = [];

  function walk(d: string) {
    const entries = readdirSync(d, { withFileTypes: true });
    for (const entry of entries) {
      const full = join(d, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.isFile()) {
        const content = readFileSync(full, 'utf-8');
        files.push({ path: full, content });
      }
    }
  }

  walk(dir);
  return files;
}

// ---------------------------------------------------------------------------
// Main discovery function
// ---------------------------------------------------------------------------

/**
 * Discover all smoke-all test fixtures and parse their metadata.
 *
 * Returns only HTML format tests (E2E tests can only render HTML).
 */
export function discoverSmokeAllTests(): DiscoveredTest[] {
  const qmdFiles = discoverTestFiles(SMOKE_ALL_DIR);
  const tests: DiscoveredTest[] = [];

  for (const qmdPath of qmdFiles) {
    const content = readFileSync(qmdPath, 'utf-8');
    const relPath = relative(SMOKE_ALL_DIR, qmdPath);
    const metadata = readFrontmatter(content);
    const { runConfig, formatSpecs } = parseTestSpecs(metadata, {
      skipPrintsMessage: SKIP_PRINTS_MESSAGE.has(relPath),
    });

    // Only include HTML format specs
    const htmlSpecs = formatSpecs.filter((s) => s.format === 'html');
    if (htmlSpecs.length === 0) continue;

    const qmdDir = dirname(qmdPath);
    const projectRoot = findProjectRoot(qmdDir);
    const allFiles = readAllFiles(projectRoot);
    const projectFiles = allFiles.map((f) => ({
      path: relative(projectRoot, f.path),
      content: f.content,
    }));

    tests.push({
      qmdPath,
      relPath,
      projectRoot,
      projectFiles,
      renderPath: relative(projectRoot, qmdPath),
      runConfig,
      formatSpecs: htmlSpecs,
    });
  }

  return tests;
}
