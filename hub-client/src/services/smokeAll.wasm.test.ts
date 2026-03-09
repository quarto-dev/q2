/**
 * WASM Smoke-All Test Runner
 *
 * Exercises the WASM rendering module against the same smoke-all test fixtures
 * used by the native Rust test runner (crates/quarto/tests/smoke_all.rs).
 *
 * Run with: npm run test:wasm
 */

import { describe, it, expect, beforeAll, beforeEach } from 'vitest';
import { readFile, readdir, stat } from 'fs/promises';
import { dirname, join, relative, resolve } from 'path';
import { fileURLToPath } from 'url';
import { parse as parseYaml } from 'yaml';
import { JSDOM } from 'jsdom';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_clear: () => string;
  vfs_list_files: () => string;
  vfs_read_file: (path: string) => string;
  render_qmd: (path: string) => Promise<string>;
}

interface JsonDiagnostic {
  kind: string;
  title: string;
  code?: string;
  problem?: string;
  hints: string[];
}

interface WasmRenderResult {
  success: boolean;
  html?: string;
  error?: string;
  warnings?: JsonDiagnostic[];
  diagnostics?: JsonDiagnostic[];
}

interface RunConfig {
  skip?: string | boolean;
  ci?: boolean;
  os?: string[];
  not_os?: string[];
}

interface FormatSpec {
  format: string;
  assertions: AssertionFn[];
  checkWarnings: boolean;
  expectsError: boolean;
}

type AssertionFn = (result: WasmRenderResult) => void;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const __dirname = dirname(fileURLToPath(import.meta.url));
const SMOKE_ALL_DIR = resolve(__dirname, '../../../crates/quarto/tests/smoke-all');

// Tests where printsMessage assertions are skipped in WASM because the error
// message format differs between native render_to_file and WASM render_qmd.
// The shouldError assertion still runs — only the message text check is skipped.
const SKIP_PRINTS_MESSAGE: Set<string> = new Set([
  'quarto-test/expected-error.qmd',
]);

// ---------------------------------------------------------------------------
// WASM setup
// ---------------------------------------------------------------------------

let wasm: WasmModule;

beforeAll(async () => {
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmPath = join(wasmDir, 'wasm_quarto_hub_client_bg.wasm');
  const wasmBytes = await readFile(wasmPath);

  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);

  // Set up VFS callbacks for the SASS importer so that dart-sass can resolve
  // @use/@import directives against the VFS (Bootstrap SCSS files, etc.)
  const sassModule = await import('../wasm-js-bridge/sass.js');
  sassModule.setVfsCallbacks(
    (path: string): string | null => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
        return result.success && result.content !== undefined ? result.content : null;
      } catch {
        return null;
      }
    },
    (path: string): boolean => {
      try {
        const result = JSON.parse(wasm.vfs_read_file(path)) as { success: boolean; content?: string };
        return result.success && result.content !== undefined;
      } catch {
        return false;
      }
    },
  );
});

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/** Recursively find all .qmd files under a directory, skipping files starting with _. */
async function discoverTestFiles(dir: string): Promise<string[]> {
  const results: string[] = [];

  async function walk(d: string) {
    const entries = await readdir(d, { withFileTypes: true });
    for (const entry of entries) {
      const full = join(d, entry.name);
      if (entry.isDirectory()) {
        await walk(full);
      } else if (entry.isFile() && entry.name.endsWith('.qmd') && !entry.name.startsWith('_')) {
        results.push(full);
      }
    }
  }

  await walk(dir);
  results.sort();
  return results;
}

// ---------------------------------------------------------------------------
// Frontmatter parsing
// ---------------------------------------------------------------------------

/** Extract and parse YAML frontmatter from QMD content. */
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

/** Parse a two-array spec (used by ensureFileRegexMatches and ensureHtmlElements). */
function parseTwoArraySpec(value: unknown): { matches: string[]; noMatches: string[] } {
  if (!Array.isArray(value)) return { matches: [], noMatches: [] };
  const matches = Array.isArray(value[0]) ? (value[0] as string[]) : [];
  const noMatches = value.length > 1 && Array.isArray(value[1]) ? (value[1] as string[]) : [];
  return { matches, noMatches };
}

interface ParseOptions {
  /** Skip printsMessage assertions (for tests where WASM error messages differ). */
  skipPrintsMessage?: boolean;
}

/** Parse test specs from document metadata. Returns run config and format specs. */
function parseTestSpecs(
  metadata: Record<string, unknown>,
  options: ParseOptions = {},
): {
  runConfig: RunConfig | null;
  formatSpecs: FormatSpec[];
} {
  const quarto = metadata['_quarto'] as Record<string, unknown> | undefined;
  if (!quarto) return { runConfig: null, formatSpecs: [] };

  const tests = quarto['tests'] as Record<string, unknown> | undefined;
  if (!tests) return { runConfig: null, formatSpecs: [] };

  // Parse run config
  const runConfig = (tests['run'] as RunConfig) ?? null;

  // Parse format specs
  const formatSpecs: FormatSpec[] = [];
  for (const [key, value] of Object.entries(tests)) {
    if (key === 'run') continue;
    formatSpecs.push(parseFormatSpec(key, value as Record<string, unknown>, options));
  }

  return { runConfig, formatSpecs };
}

/** Parse a single format's test specification. */
function parseFormatSpec(format: string, value: Record<string, unknown>, options: ParseOptions = {}): FormatSpec {
  const assertions: AssertionFn[] = [];
  let checkWarnings = true;
  let expectsError = false;

  if (value && typeof value === 'object') {
    for (const [key, assertionValue] of Object.entries(value)) {
      switch (key) {
        case 'ensureFileRegexMatches': {
          const { matches, noMatches } = parseTwoArraySpec(assertionValue);
          assertions.push(makeEnsureFileRegexMatches(matches, noMatches));
          break;
        }
        case 'ensureHtmlElements': {
          const { matches, noMatches } = parseTwoArraySpec(assertionValue);
          assertions.push(makeEnsureHtmlElements(matches, noMatches));
          break;
        }
        case 'ensureCssRegexMatches': {
          const { matches, noMatches } = parseTwoArraySpec(assertionValue);
          assertions.push(makeEnsureCssRegexMatches(matches, noMatches));
          break;
        }
        case 'noErrors':
          checkWarnings = false;
          assertions.push(assertNoErrors);
          break;
        case 'noErrorsOrWarnings':
          checkWarnings = false;
          assertions.push(assertNoErrorsOrWarnings);
          break;
        case 'shouldError':
          checkWarnings = false;
          expectsError = true;
          assertions.push(assertShouldError);
          break;
        case 'printsMessage': {
          if (!options.skipPrintsMessage) {
            const items = Array.isArray(assertionValue) ? assertionValue : [assertionValue];
            for (const item of items) {
              const pm = item as { level: string; regex: string; negate?: boolean };
              assertions.push(makePrintsMessage(pm.level, pm.regex, pm.negate ?? false));
            }
          }
          break;
        }
        case 'fileExists':
          // Parse but don't check — filesystem assertions are no-ops in WASM
          break;
        case 'pathDoesNotExist':
        case 'pathDoNotExists':
        case 'folderExists':
          // Parse but don't check — filesystem assertions are no-ops in WASM
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

function shouldSkip(runConfig: RunConfig | null): string | null {
  if (!runConfig) return null;

  if (runConfig.skip) {
    return typeof runConfig.skip === 'string' ? runConfig.skip : 'skip: true';
  }

  if (runConfig.ci === false && (process.env.CI || process.env.GITHUB_ACTIONS)) {
    return 'tests.run.ci is false';
  }

  // os/not_os: WASM is platform-independent, but implement for completeness
  const currentOs = process.platform === 'darwin' ? 'darwin' : process.platform === 'win32' ? 'windows' : 'linux';

  if (runConfig.os && !runConfig.os.includes(currentOs)) {
    return `tests.run.os does not include ${currentOs}`;
  }
  if (runConfig.not_os && runConfig.not_os.includes(currentOs)) {
    return `tests.run.not_os includes ${currentOs}`;
  }

  return null;
}

// ---------------------------------------------------------------------------
// VFS population
// ---------------------------------------------------------------------------

/** Find the project root by walking upward from qmdDir looking for _quarto.yml. */
async function findProjectRoot(qmdDir: string): Promise<string> {
  let dir = qmdDir;
  while (dir.startsWith(SMOKE_ALL_DIR)) {
    try {
      await stat(join(dir, '_quarto.yml'));
      return dir;
    } catch {
      const parent = dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
  }
  // No _quarto.yml found — use the QMD file's own directory
  return qmdDir;
}

/** Recursively read all files in a directory tree. */
async function readAllFiles(dir: string): Promise<{ path: string; content: string }[]> {
  const files: { path: string; content: string }[] = [];

  async function walk(d: string) {
    const entries = await readdir(d, { withFileTypes: true });
    for (const entry of entries) {
      const full = join(d, entry.name);
      if (entry.isDirectory()) {
        await walk(full);
      } else if (entry.isFile()) {
        const content = await readFile(full, 'utf-8');
        files.push({ path: full, content });
      }
    }
  }

  await walk(dir);
  return files;
}

/** Populate the WASM VFS with all files from the project root. */
async function populateVfs(qmdPath: string): Promise<string> {
  const qmdDir = dirname(qmdPath);
  const projectRoot = await findProjectRoot(qmdDir);

  const files = await readAllFiles(projectRoot);
  for (const file of files) {
    const rel = relative(projectRoot, file.path);
    const vfsPath = `/project/${rel}`;
    wasm.vfs_add_file(vfsPath, file.content);
  }

  // Return the VFS path for the QMD file
  const relQmd = relative(projectRoot, qmdPath);
  return `/project/${relQmd}`;
}

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

/** Map diagnostic kind string to a level name matching the spec YAML convention. */
function kindToLevel(kind: string): string {
  switch (kind.toLowerCase()) {
    case 'error': return 'ERROR';
    case 'warning': return 'WARN';
    case 'info': return 'INFO';
    case 'note': return 'DEBUG';
    default: return kind.toUpperCase();
  }
}

/** Collect all messages from a render result (diagnostics on failure, warnings on success). */
function collectMessages(result: WasmRenderResult): { level: string; message: string }[] {
  const msgs: { level: string; message: string }[] = [];
  for (const diag of result.diagnostics ?? []) {
    msgs.push({ level: kindToLevel(diag.kind), message: diag.title });
  }
  for (const warn of result.warnings ?? []) {
    msgs.push({ level: kindToLevel(warn.kind), message: warn.title });
  }
  return msgs;
}

function makeEnsureFileRegexMatches(
  matches: string[],
  noMatches: string[],
): AssertionFn {
  return (result: WasmRenderResult) => {
    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html, 'No HTML in render result').toBeTruthy();
    const html = result.html!;

    for (const pattern of matches) {
      expect(
        new RegExp(pattern, 'm').test(html),
        `ensureFileRegexMatches: expected pattern "${pattern}" to match`,
      ).toBe(true);
    }
    for (const pattern of noMatches) {
      expect(
        new RegExp(pattern, 'm').test(html),
        `ensureFileRegexMatches: expected pattern "${pattern}" NOT to match`,
      ).toBe(false);
    }
  };
}

function makeEnsureHtmlElements(
  selectors: string[],
  noMatchSelectors: string[],
): AssertionFn {
  return (result: WasmRenderResult) => {
    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html, 'No HTML in render result').toBeTruthy();
    const dom = new JSDOM(result.html!);
    const doc = dom.window.document;

    for (const selector of selectors) {
      expect(
        doc.querySelector(selector),
        `ensureHtmlElements: expected selector "${selector}" to match`,
      ).not.toBeNull();
    }
    for (const selector of noMatchSelectors) {
      expect(
        doc.querySelector(selector),
        `ensureHtmlElements: expected selector "${selector}" NOT to match`,
      ).toBeNull();
    }
  };
}

function makeEnsureCssRegexMatches(
  matches: string[],
  noMatches: string[],
): AssertionFn {
  return (result: WasmRenderResult) => {
    expect(result.success, `Render failed: ${result.error}`).toBe(true);
    expect(result.html, 'No HTML in render result').toBeTruthy();

    // Parse HTML for <link rel="stylesheet"> hrefs
    const dom = new JSDOM(result.html!);
    const links = dom.window.document.querySelectorAll('link[rel="stylesheet"]');
    let combinedCss = '';

    for (const link of links) {
      const href = link.getAttribute('href');
      if (!href || href.startsWith('http://') || href.startsWith('https://') || href.startsWith('//')) {
        continue;
      }
      // Resolve href relative to /project/ (VFS root)
      const vfsPath = href.startsWith('/') ? href : `/project/${href}`;
      try {
        const readResult = JSON.parse(wasm.vfs_read_file(vfsPath)) as { success: boolean; content?: string; error?: string };
        if (readResult.success && readResult.content) {
          combinedCss += readResult.content + '\n';
        }
      } catch {
        // CSS file not readable — will be caught by pattern assertions below
      }
    }

    expect(
      combinedCss.length,
      'ensureCssRegexMatches: no CSS content found (no local stylesheets readable from VFS)',
    ).toBeGreaterThan(0);

    for (const pattern of matches) {
      expect(
        new RegExp(pattern, 'm').test(combinedCss),
        `ensureCssRegexMatches: expected CSS pattern "${pattern}" to match`,
      ).toBe(true);
    }
    for (const pattern of noMatches) {
      expect(
        new RegExp(pattern, 'm').test(combinedCss),
        `ensureCssRegexMatches: expected CSS pattern "${pattern}" NOT to match`,
      ).toBe(false);
    }
  };
}

function assertNoErrors(result: WasmRenderResult): void {
  const msgs = collectMessages(result);
  const errorMsgs = msgs.filter(m => m.level === 'ERROR').map(m => m.message);
  expect(
    result.success,
    `noErrors: render failed: ${result.error}${errorMsgs.length ? '\n  Diagnostics: ' + errorMsgs.join(', ') : ''}`,
  ).toBe(true);
}

function assertNoErrorsOrWarnings(result: WasmRenderResult): void {
  assertNoErrors(result);
  const msgs = collectMessages(result);
  const warnMsgs = msgs.filter(m => m.level === 'WARN').map(m => m.message);
  expect(
    warnMsgs.length,
    `noErrorsOrWarnings: unexpected warnings: ${warnMsgs.join(', ')}`,
  ).toBe(0);
}

function assertShouldError(result: WasmRenderResult): void {
  expect(result.success, 'shouldError: expected render to fail but it succeeded').toBe(false);
}

function makePrintsMessage(level: string, regex: string, negate: boolean): AssertionFn {
  return (result: WasmRenderResult) => {
    const msgs = collectMessages(result);
    const filtered = msgs.filter(m => m.level === level);
    const re = new RegExp(regex);
    const anyMatch = filtered.some(m => re.test(m.message));

    if (negate) {
      expect(
        anyMatch,
        `printsMessage: expected no ${level} message matching /${regex}/ but found one`,
      ).toBe(false);
    } else {
      expect(
        anyMatch,
        `printsMessage: expected a ${level} message matching /${regex}/ but none found among: [${filtered.map(m => m.message).join(', ')}]`,
      ).toBe(true);
    }
  };
}

// ---------------------------------------------------------------------------
// Test execution
// ---------------------------------------------------------------------------

describe('smoke-all WASM tests', () => {
  let testFiles: string[] = [];

  beforeAll(async () => {
    testFiles = await discoverTestFiles(SMOKE_ALL_DIR);
  });

  beforeEach(() => {
    wasm.vfs_clear();
  });

  it('discovers test files', () => {
    expect(testFiles.length).toBeGreaterThan(0);
  });

  // Dynamically register tests. We use a describe + beforeAll pattern:
  // discover files eagerly, then iterate.
  // Since vitest collects tests synchronously, we use a two-pass approach:
  // first discover synchronously (not possible), so instead we hardcode the
  // discovery inline via a top-level await workaround — or more practically,
  // we run all tests inside a single `it` that iterates.
  //
  // Better approach: use `it.each` after async discovery. But vitest requires
  // the array at collect-time. So we run the full suite in one test case and
  // report individual failures clearly.

  it('all smoke-all fixtures render correctly', async () => {
    const failures: string[] = [];
    let passed = 0;
    let skipped = 0;

    for (const testFile of testFiles) {
      const relPath = relative(SMOKE_ALL_DIR, testFile);
      const content = await readFile(testFile, 'utf-8');
      const metadata = readFrontmatter(content);
      const { runConfig, formatSpecs } = parseTestSpecs(metadata, {
        skipPrintsMessage: SKIP_PRINTS_MESSAGE.has(relPath),
      });

      if (formatSpecs.length === 0) {
        skipped++;
        continue;
      }

      const skipReason = shouldSkip(runConfig);
      if (skipReason) {
        skipped++;
        continue;
      }

      for (const spec of formatSpecs) {
        // WASM only renders HTML
        if (spec.format !== 'html') {
          skipped++;
          continue;
        }

        try {
          wasm.vfs_clear();
          const vfsPath = await populateVfs(testFile);
          const resultJson = await wasm.render_qmd(vfsPath);
          const result: WasmRenderResult = JSON.parse(resultJson);

          // If render failed and we don't expect errors, report immediately
          if (!result.success && !spec.expectsError) {
            failures.push(`${relPath} [${spec.format}]: render failed: ${result.error}`);
            continue;
          }

          // Run explicit assertions
          for (const assertion of spec.assertions) {
            try {
              assertion(result);
            } catch (e) {
              failures.push(`${relPath} [${spec.format}]: ${(e as Error).message}`);
            }
          }

          // Default assertion
          if (spec.checkWarnings) {
            try {
              assertNoErrorsOrWarnings(result);
            } catch (e) {
              failures.push(`${relPath} [${spec.format}]: (default) ${(e as Error).message}`);
            }
          }

          passed++;
        } catch (e) {
          failures.push(`${relPath} [${spec.format}]: ${(e as Error).message}`);
        }
      }
    }

    console.log(`\nSmoke-all WASM results: ${passed} passed, ${skipped} skipped, ${failures.length} failed`);

    if (failures.length > 0) {
      console.log('\nFailures:');
      for (const f of failures) {
        console.log(`  ✗ ${f}`);
      }
    }

    expect(failures, `${failures.length} smoke-all test(s) failed:\n${failures.join('\n')}`).toHaveLength(0);
  });
});
