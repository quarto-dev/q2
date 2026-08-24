/**
 * Smoke-all fixture / VFS helpers shared by every `*.wasm.test.*` runner.
 *
 * Exercises the same smoke-all test fixtures used by the native Rust test
 * runner (crates/quarto/tests/integration/smoke_all.rs): project-root
 * discovery, `/project/`-prefixed VFS population, binary files, and user
 * grammars. Kept in one place so a spike or a new runner (e.g. a
 * preview↔render parity runner) uses the exact same fixture → VFS
 * semantics as the existing smoke-all sweep, rather than hand-rolling VFS
 * loading and reporting divergences the real runner never sees.
 */

import { readFile, readdir, stat } from 'fs/promises';
import { dirname, join, relative, resolve } from 'path';
import { fileURLToPath } from 'url';
import { parse as parseYaml } from 'yaml';

import { discoverUserGrammars } from '@quarto/preview-runtime/userGrammar/Discovery';
import { loadUserGrammar } from '@quarto/preview-runtime/userGrammar/Highlight';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface JsUserGrammarsHandle {
  register(
    languageClass: string,
    highlightFn: (class_: string, source: string) => string | null | undefined,
  ): void;
  free(): void;
}

export interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  vfs_add_file: (path: string, content: string) => string;
  vfs_add_binary_file: (path: string, content: Uint8Array) => string;
  vfs_clear: () => string;
  vfs_list_files: () => string;
  vfs_read_file: (path: string) => string;
  render_qmd: (path: string, user_grammars?: unknown) => Promise<string>;
  render_page_in_project: (path: string, user_grammars?: unknown) => Promise<string>;
  render_page_for_preview: (
    path: string,
    user_grammars?: unknown,
    capture_gz_json?: Uint8Array,
  ) => Promise<string>;
  JsUserGrammars: new () => JsUserGrammarsHandle;
}

export interface RunConfig {
  skip?: string | boolean;
  ci?: boolean;
  os?: string[];
  not_os?: string[];
  /**
   * Set on fixtures whose format requires a JS runtime to render. Parsed
   * for consistency with the Rust runner, which uses it to skip CLI runs.
   * Not enforced here — the WASM unit test already filters by
   * `format !== 'html'`, so q2-debug fixtures are skipped at the format
   * gate before this matters.
   */
  requires_js?: boolean;
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const __dirname = dirname(fileURLToPath(import.meta.url));
export const SMOKE_ALL_DIR = resolve(__dirname, '../../../crates/quarto/tests/smoke-all');

// ---------------------------------------------------------------------------
// WASM setup
// ---------------------------------------------------------------------------

/**
 * Initialise the WASM module from the checked-in build and wire the
 * dart-sass VFS callbacks. Shared by every `*.wasm.test.*` smoke-all
 * runner; call once from `beforeAll`.
 */
export async function loadSmokeWasm(): Promise<WasmModule> {
  const wasmDir = join(__dirname, '../../wasm-quarto-hub-client');
  const wasmBytes = await readFile(join(wasmDir, 'wasm_quarto_hub_client_bg.wasm'));
  const wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
  const sassModule = await import('/src/wasm-js-bridge/sass.js');
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
  return wasm;
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/** Recursively find all .qmd files under a directory, skipping files starting with _. */
export async function discoverTestFiles(dir: string): Promise<string[]> {
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
export function readFrontmatter(content: string): Record<string, unknown> {
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

/**
 * The `_quarto.tests` block split into its `run` config and its
 * per-format raw mappings. Returns null when the fixture has no tests
 * block. A format entry that is not a mapping (e.g. `html: default`) is
 * normalised to `{}`, matching the Rust parser
 * (crates/quarto-test/src/spec.rs `parse_format_spec`: `value.as_mapping()`
 * optional). Runners parse the per-format mapping themselves (their
 * assertion models differ); this keeps the *shape* of the DSL in one
 * place.
 */
export function readTestsBlock(
  metadata: Record<string, unknown>,
): { run: RunConfig | null; formats: Record<string, Record<string, unknown>> } | null {
  const quarto = metadata['_quarto'] as Record<string, unknown> | undefined;
  const tests = quarto?.['tests'] as Record<string, unknown> | undefined;
  if (!tests) return null;
  const formats: Record<string, Record<string, unknown>> = {};
  for (const [key, value] of Object.entries(tests)) {
    if (key === 'run') continue;
    formats[key] =
      value !== null && typeof value === 'object' && !Array.isArray(value)
        ? (value as Record<string, unknown>)
        : {};
  }
  return { run: (tests['run'] as RunConfig) ?? null, formats };
}

/** Parse a two-array spec (used by ensureFileRegexMatches and ensureHtmlElements). */
export function parseTwoArraySpec(value: unknown): { matches: string[]; noMatches: string[] } {
  if (!Array.isArray(value)) return { matches: [], noMatches: [] };
  const matches = Array.isArray(value[0]) ? (value[0] as string[]) : [];
  const noMatches = value.length > 1 && Array.isArray(value[1]) ? (value[1] as string[]) : [];
  return { matches, noMatches };
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
export async function findProjectRoot(qmdDir: string): Promise<string> {
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

/**
 * File extensions routed through `vfs_add_binary_file` rather than
 * `vfs_add_file`. Reading these as UTF-8 corrupts their bytes (and in
 * the `.wasm` case makes the user-grammar loader fail silently).
 */
const BINARY_EXTENSIONS = new Set<string>([
  '.wasm',
  '.png',
  '.jpg',
  '.jpeg',
  '.gif',
  '.pdf',
  '.ico',
  '.webp',
  '.ttf',
  '.woff',
  '.woff2',
  '.zip',
]);

function isBinaryFilename(filename: string): boolean {
  const lower = filename.toLowerCase();
  const dot = lower.lastIndexOf('.');
  if (dot < 0) return false;
  return BINARY_EXTENSIONS.has(lower.slice(dot));
}

export interface FileEntry {
  path: string; // absolute on disk
  projectRelPath: string; // relative to the smoke-all project root, no leading slash
  content: string | Uint8Array;
}

/** Recursively read every file under `dir`, binary-aware. */
export async function readAllFiles(dir: string, projectRoot: string): Promise<FileEntry[]> {
  const files: FileEntry[] = [];

  async function walk(d: string) {
    const entries = await readdir(d, { withFileTypes: true });
    for (const entry of entries) {
      const full = join(d, entry.name);
      if (entry.isDirectory()) {
        await walk(full);
      } else if (entry.isFile()) {
        const content: string | Uint8Array = isBinaryFilename(entry.name)
          ? new Uint8Array(await readFile(full))
          : await readFile(full, 'utf-8');
        files.push({
          path: full,
          projectRelPath: relative(projectRoot, full),
          content,
        });
      }
    }
  }

  await walk(dir);
  return files;
}

/**
 * Populate the WASM VFS with all files from the project root. Returns
 * both the `/project/`-prefixed VFS path for the QMD being rendered,
 * and the flat list of project-relative file paths (consumed by the
 * user-grammar discovery step downstream).
 */
export async function populateVfs(
  wasm: WasmModule,
  qmdPath: string,
): Promise<{ vfsPath: string; projectFiles: FileEntry[] }> {
  const qmdDir = dirname(qmdPath);
  const projectRoot = await findProjectRoot(qmdDir);

  const files = await readAllFiles(projectRoot, projectRoot);
  for (const file of files) {
    const vfsPath = `/project/${file.projectRelPath}`;
    if (typeof file.content === 'string') {
      wasm.vfs_add_file(vfsPath, file.content);
    } else {
      wasm.vfs_add_binary_file(vfsPath, file.content);
    }
  }

  const relQmd = relative(projectRoot, qmdPath);
  return { vfsPath: `/project/${relQmd}`, projectFiles: files };
}

/**
 * Given the project's file list, discover any user-defined tree-sitter
 * grammars under `_quarto/grammars/<name>/`, load them, and return a
 * `JsUserGrammars` handle ready to pass into `render_qmd`. Returns
 * `undefined` when the project has no grammars, which lets callers
 * pass `undefined` to the render and take the built-ins-only path.
 *
 * This mirrors the runtime flow in `wasmRenderer.ts:prepareUserGrammarsHandle`
 * but without the long-lived cache — smokeAll renders each fixture
 * once per test invocation, so loading on demand is fine.
 */
export async function buildUserGrammarsHandle(
  wasm: WasmModule,
  files: readonly FileEntry[],
): Promise<JsUserGrammarsHandle | undefined> {
  const paths = files.map((f) => f.projectRelPath);
  const descriptors = discoverUserGrammars(paths);
  if (descriptors.length === 0) return undefined;

  const byPath = new Map<string, FileEntry>();
  for (const f of files) byPath.set(f.projectRelPath, f);

  const handle = new wasm.JsUserGrammars();
  for (const desc of descriptors) {
    const wasmEntry = byPath.get(desc.wasmPath);
    const scmEntry = byPath.get(desc.highlightsPath);
    if (!wasmEntry || !scmEntry) continue;
    if (!(wasmEntry.content instanceof Uint8Array)) continue;
    if (typeof scmEntry.content !== 'string') continue;

    const highlighter = await loadUserGrammar({
      name: desc.class,
      wasmBytes: wasmEntry.content,
      highlightsScm: scmEntry.content,
    });
    handle.register(desc.class, (_cls, source) => highlighter.highlight(source));
  }
  return handle;
}
