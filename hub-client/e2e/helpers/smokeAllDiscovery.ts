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

// Tests that fail in the hub-client preview because of a known gap in
// the WASM render pipeline. The native CLI runner handles these. Skip
// in the browser until the gap is closed. (bd-izfv's user-grammar entry
// was removed here once the project-render path started threading
// user_grammars through; leave the slot in place for future WASM gaps.)
const SKIP_WASM_UNSUPPORTED: Map<string, string> = new Map([]);

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface RunConfig {
  skip?: string | boolean;
  ci?: boolean;
  os?: string[];
  not_os?: string[];
  /**
   * True when the fixture's format requires a JS runtime to render
   * (e.g. q2-debug renders via React inside an iframe). Honored by
   * runners that can't execute JS — the CLI smoke-all runner skips
   * these. The Playwright runner always has JS, so this is a no-op
   * here; we parse the field for consistency with the Rust side.
   */
  requires_js?: boolean;
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
  /**
   * All project files. Text files have UTF-8 `content`; binary fixtures
   * (e.g. `_quarto/grammars/<lang>/*.wasm`, images) have base64-encoded
   * `content` with `contentType: 'binary'` and an appropriate
   * `mimeType`. The Playwright runner forwards both flavors to
   * `createProjectOnServer`, which routes them to Automerge text vs.
   * binary documents.
   */
  projectFiles: {
    path: string;
    content: string;
    contentType: 'text' | 'binary';
    mimeType?: string;
  }[];
  /** Which file to render (relative to project root) */
  renderPath: string;
  /** Run config from frontmatter */
  runConfig: RunConfig | null;
  /** Format-specific test specs */
  formatSpecs: FormatTestSpec[];
  /**
   * The document's *own* `format:` front-matter value — the scalar, or
   * the first key of a map — or `null` when absent. Distinct from
   * `formatSpecs[].format`, which is the `_quarto: tests:` dimension the
   * fixture is asserted under. The runner needs the document's own value
   * to know which preview iframe hub-client mounts (bd-kltzdhle): a
   * fixture tested under `html` renders in the q2-preview iframe unless
   * its front matter says `format: q2-html-render`.
   */
  documentFormat: string | null;
}

/**
 * Read the document's own `format:` key the way the WASM's
 * `detect_format_from_content` does: a scalar is taken as-is, a map yields
 * its first key, anything else (absent, list) is `null`.
 */
export function documentFormatFromFrontmatter(
  metadata: Record<string, unknown>,
): string | null {
  const format = metadata.format;
  if (typeof format === 'string') return format;
  if (format && typeof format === 'object' && !Array.isArray(format)) {
    const [first] = Object.keys(format as Record<string, unknown>);
    return first ?? null;
  }
  return null;
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
          // Matches Q1 semantics (tests/smoke/smoke-all.test.ts:
          // resolveTestSpecs): printsMessage alone does NOT suppress the
          // default noErrorsOrWarnings. Fixtures that expect messages pair
          // printsMessage with an explicit noErrors / noErrorsOrWarnings /
          // shouldError.
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
        case 'dom-parity':
          // Opt-in flag for the preview <-> render DOM parity runner
          // (hub-client/src/services/smokeAllParity.wasm.test.tsx). Not an
          // assertion here.
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

export function shouldSkip(
  runConfig: RunConfig | null,
  relPath?: string,
): string | null {
  if (relPath) {
    const wasmReason = SKIP_WASM_UNSUPPORTED.get(relPath);
    if (wasmReason) return `WASM unsupported: ${wasmReason}`;
  }

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

/**
 * Extension → MIME mapping for binary fixture files. The set is
 * narrow: only formats the smoke-all suite actually carries (user
 * grammars and a couple of image fixtures). Add new entries here
 * when a future fixture needs a new binary type.
 */
const BINARY_EXTENSIONS: Record<string, string> = {
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.otf': 'font/otf',
};

function binaryMimeFor(path: string): string | null {
  const dot = path.lastIndexOf('.');
  if (dot < 0) return null;
  return BINARY_EXTENSIONS[path.slice(dot).toLowerCase()] ?? null;
}

/**
 * Recursively read all files in a directory. Text fixtures land as
 * `contentType: 'text'` with UTF-8 content; binary fixtures (see
 * {@link BINARY_EXTENSIONS}) land as `contentType: 'binary'` with the
 * raw bytes base64-encoded — matching the shape
 * `createProjectOnServer` expects, which forwards binary content into
 * Automerge as a binary document instead of corrupting it through
 * UTF-8 decoding.
 */
function readAllFiles(dir: string): {
  path: string;
  content: string;
  contentType: 'text' | 'binary';
  mimeType?: string;
}[] {
  const files: {
    path: string;
    content: string;
    contentType: 'text' | 'binary';
    mimeType?: string;
  }[] = [];

  function walk(d: string) {
    const entries = readdirSync(d, { withFileTypes: true });
    for (const entry of entries) {
      const full = join(d, entry.name);
      if (entry.isDirectory()) {
        walk(full);
      } else if (entry.isFile()) {
        const mimeType = binaryMimeFor(full);
        if (mimeType) {
          const bytes = readFileSync(full);
          files.push({
            path: full,
            content: bytes.toString('base64'),
            contentType: 'binary',
            mimeType,
          });
        } else {
          files.push({
            path: full,
            content: readFileSync(full, 'utf-8'),
            contentType: 'text',
          });
        }
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
/**
 * Fixtures whose `ensureHtmlElements` assertions (written against the HTML
 * writer's DOM) do not yet hold in the q2-preview iframe, which is where a
 * plain document renders now that q2-preview is hub-client's default
 * renderer (bd-kltzdhle, plan D8). Each entry names the parity strand that
 * owns the gap. While listed, the runner still renders the fixture in the
 * default iframe and runs every *other* assertion (regex, CSS, diagnostics);
 * only the DOM selectors are skipped, with a `[smoke-diag]` line so the
 * skip is visible in the report.
 *
 * This is a staging device, not an end state: Phase 4b of the plan works
 * the list back down (fix the gap, or replace the selector with one that
 * holds in both DOMs). Add an entry only with a strand; remove it when the
 * strand closes.
 */
export const DOM_ASSERTIONS_PENDING_PARITY: ReadonlyMap<string, string> = new Map([
  // Mermaid blocks: no `pre.mermaid` component in q2-preview yet.
  ['mermaid/basic.qmd', 'bd-c3dtpe36'],
  // User-declared `css:` links are not emitted in VFS/preview mode.
  ['metadata/dir-metadata-paths/chapters/intro/doc.qmd', 'bd-b3oq2fsy'],
  // TOC copy of repo actions dropped at the default toc-location.
  ['repo-actions/actions.qmd', 'bd-fandfn60'],
  // Callout body heading not sectionized in preview.
  ['toc-containers/callout-body-heading-not-in-toc.qmd', 'bd-bg0jze2i'],
  // Heading inside a blockquote: `blockquote > h4` not matched in preview.
  ['toc-containers/div-heading-becomes-section.qmd', 'bd-q2wqj24c'],
  // Tabsets are not rendered as a component in q2-preview.
  ['toc-containers/tabset-pane-heading-not-in-toc.qmd', 'bd-47afd5ro'],
]);

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

    // Only formats the e2e runner knows how to drive: html, q2-preview
    // (both in the Q2PreviewIframe — html is q2-preview's default since
    // bd-kltzdhle, unless the document itself declares
    // `format: q2-html-render`), and q2-debug (Q2DebugIframe). The
    // runner picks the iframe kind from `documentFormat`; see
    // smoke-all.spec.ts.
    const supportedSpecs = formatSpecs.filter(
      (s) =>
        s.format === 'html' ||
        s.format === 'q2-debug' ||
        s.format === 'q2-preview',
    );
    if (supportedSpecs.length === 0) continue;
    const documentFormat = documentFormatFromFrontmatter(metadata);

    const qmdDir = dirname(qmdPath);
    const projectRoot = findProjectRoot(qmdDir);
    const allFiles = readAllFiles(projectRoot);
    const projectFiles = allFiles.map((f) => ({
      path: relative(projectRoot, f.path),
      content: f.content,
      contentType: f.contentType,
      mimeType: f.mimeType,
    }));

    tests.push({
      qmdPath,
      relPath,
      projectRoot,
      projectFiles,
      renderPath: relative(projectRoot, qmdPath),
      runConfig,
      formatSpecs: supportedSpecs,
      documentFormat,
    });
  }

  return tests;
}
