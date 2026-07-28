/**
 * Local WASM renderer backing `get_errors` (v2).
 *
 * Renders the project files the MCP already holds using the SAME
 * wasm-quarto-hub-client module the browser preview runs, and returns
 * the diagnostics of exactly what was rendered. No CRDT choreography:
 * validity is a function of content, per the v2 plan
 * (claude-notes/plans/2026-07-28-hub-mcp-get-errors-v2.md).
 *
 * The WASM lives in a prebundled host module (dist/wasm-host.mjs, built
 * by scripts/build-wasm-host.mjs) loaded lazily on first use — server
 * startup stays instant and projects that never call get_errors never
 * pay the ~38 MB init. `QUARTO_HUB_MCP_WASM_HOST` overrides the host
 * location (used by vitest, whose import.meta.url points at src/).
 */

import { createHash } from 'node:crypto';
import type { FilePayload } from '@quarto/quarto-sync-client';

/** Structured diagnostic as produced by the WASM render pipeline. */
export interface RenderedDiagnostic {
  kind: 'error' | 'warning' | 'info' | 'note';
  title: string;
  code?: string;
  problem?: string;
  hints: string[];
  start_line?: number;
  start_column?: number;
  end_line?: number;
  end_column?: number;
  details: unknown[];
}

export interface SiblingFailure {
  /** Project-relative path of the failing sibling (VFS prefix stripped). */
  path: string;
  errors: RenderedDiagnostic[];
}

export interface LocalRenderResult {
  /** `sha256:<hex>` of the text that was rendered for `path`. */
  checkedContentSha256: string;
  errors: RenderedDiagnostic[];
  warnings: RenderedDiagnostic[];
  /** Pass-1 failures in OTHER project files, keyed by their own path. */
  pass1Failures: SiblingFailure[];
}

interface WasmHost {
  ensureInit(): Promise<unknown>;
  vfs_clear(): string;
  vfs_add_file(path: string, content: string): string;
  vfs_add_binary_file(path: string, content: Uint8Array): string;
  render_page_in_project(path: string): Promise<string>;
}

interface WasmRenderResponse {
  success: boolean;
  error?: string;
  diagnostics?: RenderedDiagnostic[];
  warnings?: RenderedDiagnostic[];
  pass1_failures?: Array<{
    source_file: string;
    error: string;
    diagnostics: RenderedDiagnostic[];
  }>;
}

let hostPromise: Promise<WasmHost> | null = null;

function loadHost(): Promise<WasmHost> {
  hostPromise ??= (async () => {
    const spec =
      process.env['QUARTO_HUB_MCP_WASM_HOST'] ?? new URL('./wasm-host.mjs', import.meta.url).href;
    const host = (await import(spec)) as WasmHost;
    await host.ensureInit();
    return host;
  })();
  return hostPromise;
}

/** Strip the `/project/` VFS prefix (and any leading slash) from a WASM-reported path. */
function normalizeProjectPath(p: string): string {
  const noVfs = p.startsWith('/project/') ? p.slice('/project/'.length) : p;
  return noVfs.startsWith('/') ? noVfs.slice(1) : noVfs;
}

/** The WASM VFS is instance-global state — renders must not interleave. */
let renderChain: Promise<unknown> = Promise.resolve();

/**
 * Render `path` against a VFS filled with `files` and return the
 * structured diagnostics the render produced. Throws only on host
 * failures; render errors come back as diagnostics.
 */
export function renderDiagnostics(
  files: Map<string, FilePayload>,
  path: string,
): Promise<LocalRenderResult> {
  const run = renderChain.then(async (): Promise<LocalRenderResult> => {
    const host = await loadHost();

    const target = files.get(path);
    if (!target || target.type !== 'text') {
      throw new Error(`Not a text file in this project: ${path}`);
    }

    host.vfs_clear();
    for (const [p, payload] of files) {
      if (payload.type === 'text') {
        host.vfs_add_file(`/project/${p}`, payload.text);
      } else {
        host.vfs_add_binary_file(`/project/${p}`, payload.data);
      }
    }

    const response = JSON.parse(await host.render_page_in_project(path)) as WasmRenderResponse;

    const errors: RenderedDiagnostic[] = [];
    const warnings: RenderedDiagnostic[] = [];
    for (const d of response.diagnostics ?? []) {
      (d.kind === 'error' ? errors : warnings).push(d);
    }
    for (const d of response.warnings ?? []) {
      warnings.push(d);
    }

    const pass1Failures: SiblingFailure[] = [];
    for (const failure of response.pass1_failures ?? []) {
      const sibling = normalizeProjectPath(failure.source_file);
      if (sibling === path) continue; // active-page failures are in `errors`
      pass1Failures.push({
        path: sibling,
        errors:
          failure.diagnostics.length > 0
            ? failure.diagnostics
            : [{ kind: 'error', title: failure.error, hints: [], details: [] }],
      });
    }

    // A failed render whose error names the ACTIVE page but produced no
    // structured diagnostics still needs to surface (defensive).
    if (!response.success && errors.length === 0 && response.error !== undefined) {
      const named = normalizeProjectPath(
        /Pass 1 failed for (\S+?):/.exec(response.error)?.[1] ?? path,
      );
      if (named === path) {
        errors.push({
          kind: 'error',
          // eslint-disable-next-line no-control-regex
          title: response.error.replace(/\[[0-9;]*m/g, '').slice(0, 300),
          hints: [],
          details: [],
        });
      }
    }

    return {
      checkedContentSha256: `sha256:${createHash('sha256').update(target.text, 'utf8').digest('hex')}`,
      errors,
      warnings,
      pass1Failures,
    };
  });
  // Keep the chain alive whether or not this render succeeded.
  renderChain = run.catch(() => undefined);
  return run;
}
