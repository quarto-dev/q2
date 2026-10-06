/**
 * The render tool's engine (CAP-12): materialize a connected project's
 * files to a temp dir, run `q2 render --json-errors` on it, and parse the
 * NDJSON diagnostic wire into a structured result. Everything here is
 * transport-free; tools.ts's `render` registration is a thin adapter.
 *
 * Security model: rendering executes project code (computations, filters,
 * engines) on this machine, so the tool exists only under `--allow-render`
 * and never on the `--read-only` surface. The temp dir is the only thing
 * we write to and it is always removed; path-escape entries in the index
 * are refused at materialization.
 */

import { spawn } from 'node:child_process';
import { mkdirSync, readdirSync, realpathSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, sep } from 'node:path';

import { z } from 'zod';
import type { FilePayload } from '@quarto/quarto-sync-client';

/** One diagnostic, parsed from the `q2 render --json-errors` NDJSON wire. */
export interface RenderDiagnostic {
  code: string | null;
  kind: string;
  title: string;
  problem?: string;
  /** Project-relative source file (the temp-dir prefix is stripped). */
  file?: string;
  line?: number;
  column?: number;
  hints?: string[];
}

export interface MaterializeResult {
  written: string[];
  skipped: Array<{ path: string; reason: string }>;
}

/**
 * Write every file in the project to `destDir`. Entries whose paths would
 * escape the destination are refused (a hostile or corrupt index must not
 * turn the render tool into an arbitrary writer).
 */
export function materializeProject(
  files: ReadonlyMap<string, FilePayload>,
  destDir: string,
): MaterializeResult {
  const written: string[] = [];
  const skipped: Array<{ path: string; reason: string }> = [];
  const root = resolve(destDir);
  for (const [path, payload] of files) {
    const segments = path.split('/');
    const unsafe =
      path.startsWith('/') ||
      /^[A-Za-z]:[\\/]/.test(path) ||
      path.includes('\0') ||
      segments.some((s) => s === '..');
    // Belt and braces: even a clean-looking path must resolve inside.
    const dest = resolve(root, path);
    if (unsafe || (dest !== root && !dest.startsWith(root + sep))) {
      skipped.push({ path, reason: 'path escapes the render directory' });
      continue;
    }
    mkdirSync(join(dest, '..'), { recursive: true });
    if (payload.type === 'text') {
      writeFileSync(dest, payload.text, 'utf8');
    } else {
      writeFileSync(dest, Buffer.from(payload.data));
    }
    written.push(path);
  }
  return { written, skipped };
}

export interface RunRenderOptions {
  q2Path: string;
  cwd: string;
  /**
   * Render targets, passed as `q2 render <targets...> --json-errors`:
   * `['.']` for a project render (a `_quarto.yml` is present), or an
   * explicit list of renderable files for the loose-files fallback
   * (without `_quarto.yml`, directory/no-arg renders are Q-7-7/Q-7-3).
   */
  targets: string[];
  timeoutMs: number;
  signal?: AbortSignal;
}

export interface RenderOutcome {
  /** Spawn-level failure (e.g. ENOENT — no q2 at the path). */
  spawnError?: string;
  exitCode: number | null;
  timedOut: boolean;
  stdout: string;
  stderr: string;
  durationMs: number;
}

/** Per-stream capture cap so a runaway log cannot exhaust memory. */
const STREAM_CAP = 1024 * 1024;

/** Run `q2 render <targets...> --json-errors` in `cwd`, bounded by timeout. */
export function runRender(opts: RunRenderOptions): Promise<RenderOutcome> {
  const { q2Path, cwd, targets, timeoutMs, signal } = opts;
  const started = Date.now();
  return new Promise((resolvePromise) => {
    let child;
    try {
      child = spawn(q2Path, ['render', ...targets, '--json-errors'], {
        cwd,
        env: process.env,
        stdio: ['ignore', 'pipe', 'pipe'],
      });
    } catch (err) {
      // Synchronous spawn errors (rare; ENOENT arrives via 'error').
      resolvePromise({
        spawnError: err instanceof Error ? err.message : String(err),
        exitCode: null,
        timedOut: false,
        stdout: '',
        stderr: '',
        durationMs: Date.now() - started,
      });
      return;
    }

    let stdout = '';
    let stderr = '';
    let timedOut = false;
    let killTimer: NodeJS.Timeout | undefined;
    let forceTimer: NodeJS.Timeout | undefined;

    const onAbort = () => {
      child.kill('SIGTERM');
      forceTimer = setTimeout(() => child.kill('SIGKILL'), 2000);
      forceTimer.unref();
    };
    killTimer = setTimeout(() => {
      timedOut = true;
      onAbort();
    }, timeoutMs);
    killTimer.unref();
    const signalCleanup = signal
      ? (() => {
          if (signal.aborted) onAbort();
          else signal.addEventListener('abort', onAbort, { once: true });
          return () => signal.removeEventListener('abort', onAbort);
        })()
      : undefined;

    child.stdout.on('data', (chunk: Buffer) => {
      if (stdout.length < STREAM_CAP) stdout += chunk.toString('utf8');
    });
    child.stderr.on('data', (chunk: Buffer) => {
      if (stderr.length < STREAM_CAP) stderr += chunk.toString('utf8');
    });
    child.on('error', (err) => {
      if (killTimer) clearTimeout(killTimer);
      if (forceTimer) clearTimeout(forceTimer);
      signalCleanup?.();
      resolvePromise({
        spawnError: err.message,
        exitCode: null,
        timedOut: false,
        stdout,
        stderr,
        durationMs: Date.now() - started,
      });
    });
    child.on('close', (code) => {
      if (killTimer) clearTimeout(killTimer);
      if (forceTimer) clearTimeout(forceTimer);
      signalCleanup?.();
      resolvePromise({
        exitCode: code,
        timedOut,
        stdout,
        stderr,
        durationMs: Date.now() - started,
      });
    });
  });
}

interface RawDiagnostic {
  code?: unknown;
  kind?: unknown;
  title?: unknown;
  problem?: unknown;
  hints?: unknown;
  start_line?: unknown;
  start_column?: unknown;
  source_file?: unknown;
}

/**
 * Parse the `--json-errors` wire: one JSON object per stderr line, either
 * a bare `json-diagnostic` or a `json-pass1-failure` envelope carrying a
 * `diagnostics` array. Non-JSON lines (progress, warnings) are ignored.
 * `stripPrefix` (the temp render dir) is relativized out of `source_file`.
 */
export function parseDiagnostics(stderr: string, stripPrefix: string): RenderDiagnostic[] {
  const out: RenderDiagnostic[] = [];
  const strip = (f: unknown): string | undefined => {
    if (typeof f !== 'string' || f === '') return undefined;
    // realpath both sides: q2 reports source_file through its (resolved)
    // cwd, while our temp dir path may cross a symlink (macOS /tmp).
    let root = resolve(stripPrefix);
    try {
      root = realpathSync(root);
    } catch {
      // keep the unresolved form — a missing dir can't be a prefix anyway
    }
    const abs = resolve(f);
    if (abs.startsWith(root + sep)) return abs.slice(root.length + 1);
    return f;
  };
  const mapOne = (d: RawDiagnostic): RenderDiagnostic => ({
    code: typeof d.code === 'string' ? d.code : null,
    kind: typeof d.kind === 'string' ? d.kind : 'error',
    title: typeof d.title === 'string' ? d.title : '',
    ...(typeof d.problem === 'string' ? { problem: d.problem } : {}),
    ...(strip(d.source_file) !== undefined ? { file: strip(d.source_file) } : {}),
    ...(typeof d.start_line === 'number' ? { line: d.start_line } : {}),
    ...(typeof d.start_column === 'number' ? { column: d.start_column } : {}),
    ...(Array.isArray(d.hints) ? { hints: d.hints.filter((h): h is string => typeof h === 'string') } : {}),
  });
  for (const line of stderr.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed.startsWith('{')) continue;
    let obj: { $schema?: unknown; diagnostics?: unknown };
    try {
      obj = JSON.parse(trimmed);
    } catch {
      continue;
    }
    const schema = typeof obj.$schema === 'string' ? obj.$schema : '';
    if (schema.endsWith('json-diagnostic.json')) {
      out.push(mapOne(obj as RawDiagnostic));
    } else if (schema.endsWith('json-pass1-failure.json') && Array.isArray(obj.diagnostics)) {
      for (const d of obj.diagnostics as RawDiagnostic[]) out.push(mapOne(d));
    }
  }
  return out;
}

/**
 * Files the render produced: everything under `dir` that was not an input,
 * excluding dot-directories (.quarto caches, .git). Capped for safety.
 */
export function collectOutputs(dir: string, inputPaths: ReadonlySet<string>, cap = 100): string[] {
  const out: string[] = [];
  const walk = (rel: string) => {
    if (out.length >= cap) return;
    const abs = rel === '' ? dir : join(dir, rel);
    let entries;
    try {
      entries = readdirSync(abs);
    } catch {
      return;
    }
    for (const name of entries) {
      if (out.length >= cap) return;
      if (name.startsWith('.')) continue;
      const childRel = rel === '' ? name : `${rel}/${name}`;
      const childAbs = join(dir, childRel);
      if (statSync(childAbs).isDirectory()) {
        walk(childRel);
      } else if (!inputPaths.has(childRel)) {
        out.push(childRel);
      }
    }
  };
  walk('');
  return out.sort();
}

/** The tool's declared outputSchema (validated against real results in tests). */
export const outRender = z.object({
  ok: z.boolean(),
  exit_code: z.number().nullable(),
  target: z.string(),
  /**
   * How the target was rendered: `file` (one `path`), `project` (directory
   * render against a `_quarto.yml`), or `files` (loose-files fallback —
   * each .qmd rendered explicitly, listed in `files`).
   */
  mode: z.enum(['file', 'project', 'files']),
  files: z.array(z.string()).optional(),
  diagnostics: z.array(
    z.object({
      code: z.string().nullable(),
      kind: z.string(),
      title: z.string(),
      problem: z.string().optional(),
      file: z.string().optional(),
      line: z.number().optional(),
      column: z.number().optional(),
      hints: z.array(z.string()).optional(),
    }),
  ),
  outputs: z.array(z.string()),
  duration_ms: z.number(),
  timed_out: z.boolean().optional(),
  hint: z.string().optional(),
  skipped_inputs: z
    .array(z.object({ path: z.string(), reason: z.string() }))
    .optional(),
});

/** A fresh temp dir for a render (the caller removes it in a finally). */
export function makeRenderDir(): string {
  return mkdtempSync(join(tmpdir(), 'quarto-hub-mcp-render-'));
}

/** Remove a render dir; the finally-partner of {@link makeRenderDir}. */
export function removeRenderDir(dir: string): void {
  rmSync(dir, { recursive: true, force: true });
}
