/**
 * The render tool (CAP-12): materialize the connected project to a temp
 * dir, run `q2 render --json-errors`, and return structured diagnostics.
 * These tests drive the tool against a FAKE q2 (a Node stub script pointed
 * at by QUARTO_Q2_PATH) so the wire parsing, exit-code handling, output
 * collection, and timeout path are deterministic. The real binary end-to-end
 * is render-real.test.ts (gated on target/debug/q2 existing).
 */

import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { mkdtempSync, rmSync, writeFileSync, chmodSync, existsSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import {
  startInMemoryMcp,
  seedProject,
  callTool,
} from './in-memory-fixture.js';
import { outRender, materializeProject } from './render.js';
import type { FilePayload } from '@quarto/quarto-sync-client';

const STUB = `#!/usr/bin/env node
// Fake q2 for render-tool tests. Behavior selected by FAKE_Q2_MODE.
// Invoked as: fake-q2 render <files...> --json-errors (one file per call
// in the loose-files mode). FAKE_Q2_FAIL_FILE names the file that fails.
import { mkdirSync, writeFileSync } from 'node:fs';
import { basename } from 'node:path';
const mode = process.env.FAKE_Q2_MODE ?? 'success';
const args = process.argv.slice(2);
const files = args.slice(1, args.indexOf('--json-errors'));
const failFile = process.env.FAKE_Q2_FAIL_FILE ?? 'bad.qmd';
console.log(JSON.stringify({ argv: args, cwd: process.cwd() }));
if (mode === 'hang') {
  // Never exits; the tool must kill us at its timeout.
  setInterval(() => {}, 1000);
  await new Promise(() => {});
}
if (files.includes(failFile)) {
  const diag = {
    kind: "error",
    title: "Include file not found",
    code: "Q-17-2",
    problem: "Could not read included file: I/O error",
    hints: [],
    start_line: 7,
    start_column: 1,
    source_file: process.cwd() + "/" + failFile,
    details: []
  };
  if (mode === 'pass1') {
    process.stderr.write(JSON.stringify({
      "$schema": "https://quarto.org/schemas/v1/json-pass1-failure.json",
      source_file: process.cwd() + "/" + failFile,
      error: "Error: [Q-17-2] Include file not found",
      diagnostics: [{ "$schema": "https://quarto.org/schemas/v1/json-diagnostic.json", ...diag }]
    }) + "\\n");
  } else {
    process.stderr.write(JSON.stringify({
      "$schema": "https://quarto.org/schemas/v1/json-diagnostic.json",
      ...diag
    }) + "\\n");
  }
  process.exit(1);
}
for (const f of files) {
  mkdirSync('_site', { recursive: true });
  writeFileSync('_site/' + basename(f, '.qmd') + '.html', '<html></html>');
}
process.exit(0);
`;

interface RenderResult {
  ok: boolean;
  exit_code: number | null;
  target: string;
  mode: 'file' | 'project' | 'files';
  files?: string[];
  diagnostics: Array<{
    code: string | null;
    kind: string;
    title: string;
    problem?: string;
    file?: string;
    line?: number;
    column?: number;
  }>;
  outputs: string[];
  duration_ms: number;
  timed_out?: boolean;
  hint?: string;
}

function structured<T>(result: unknown): T {
  const sc = (result as { structuredContent?: unknown }).structuredContent;
  if (sc === undefined || sc === null || typeof sc !== 'object') {
    throw new Error(`expected structuredContent, got: ${JSON.stringify(result)}`);
  }
  return sc as T;
}

let stubDir: string;
let stubPath: string;
let savedQ2Path: string | undefined;
let savedMode: string | undefined;

beforeEach(() => {
  stubDir = mkdtempSync(join(tmpdir(), 'fake-q2-'));
  stubPath = join(stubDir, 'fake-q2.mjs');
  writeFileSync(stubPath, STUB);
  chmodSync(stubPath, 0o755);
  savedQ2Path = process.env['QUARTO_Q2_PATH'];
  savedMode = process.env['FAKE_Q2_MODE'];
  process.env['QUARTO_Q2_PATH'] = stubPath;
});

afterEach(() => {
  if (savedQ2Path === undefined) delete process.env['QUARTO_Q2_PATH'];
  else process.env['QUARTO_Q2_PATH'] = savedQ2Path;
  if (savedMode === undefined) delete process.env['FAKE_Q2_MODE'];
  else process.env['FAKE_Q2_MODE'] = savedMode;
  rmSync(stubDir, { recursive: true, force: true });
});

async function seedAndRender(args: Record<string, unknown>): Promise<RenderResult> {
  const f = await startInMemoryMcp({ allowRender: true });
  try {
    const seed = await seedProject(f, [
      { path: 'bad.qmd', content: '---\ntitle: Bad\n---\n\n# Hi\n\n{{< include missing-file.qmd >}}\n' },
      { path: 'notes/memo.md', content: 'plain markdown\n' },
    ]);
    const result = await callTool(f, 'render', { project: seed.indexDocId, ...args });
    return structured<RenderResult>(result);
  } finally {
    await f.close();
  }
}

describe('render (CAP-12)', () => {
  it('parses a bare json-diagnostic line into structuredContent', async () => {
    const r = await seedAndRender({});
    expect(r.ok).toBe(false);
    expect(r.exit_code).toBe(1);
    expect(r.target).toBe('.');
    // No _quarto.yml in the seed: the loose-files fallback renders each .qmd.
    expect(r.mode).toBe('files');
    expect(r.files).toEqual(['bad.qmd']);
    expect(r.diagnostics).toHaveLength(1);
    const d = r.diagnostics[0]!;
    expect(d.code).toBe('Q-17-2');
    expect(d.kind).toBe('error');
    expect(d.title).toBe('Include file not found');
    // source_file is relativized from the materialized temp dir.
    expect(d.file).toBe('bad.qmd');
    expect(d.line).toBe(7);
    expect(d.column).toBe(1);
    // The result validates against the tool's declared outputSchema.
    expect(outRender.safeParse(r).success).toBe(true);
  });

  it('parses a json-pass1-failure envelope the same way', async () => {
    process.env['FAKE_Q2_MODE'] = 'pass1';
    const r = await seedAndRender({});
    expect(r.ok).toBe(false);
    expect(r.diagnostics).toHaveLength(1);
    expect(r.diagnostics[0]!.code).toBe('Q-17-2');
    expect(r.diagnostics[0]!.file).toBe('bad.qmd');
  });

  it('loose-files mode renders every qmd and unions the results (Q-7-4 avoidance)', async () => {
    process.env['FAKE_Q2_FAIL_FILE'] = 'broken.qmd';
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [
        { path: 'good.qmd', content: '# Good\n\nfine.\n' },
        { path: 'broken.qmd', content: '---\ntitle: Bad\n---\n\n# Hi\n\n{{< include missing-file.qmd >}}\n' },
        { path: 'notes.md', content: 'not a render target\n' },
      ]);
      const r = structured<RenderResult>(
        await callTool(f, 'render', { project: seed.indexDocId }),
      );
      expect(r.mode).toBe('files');
      expect(r.files).toEqual(['broken.qmd', 'good.qmd']);
      expect(r.ok).toBe(false);
      expect(r.diagnostics).toHaveLength(1);
      expect(r.diagnostics[0]!.code).toBe('Q-17-2');
      expect(r.diagnostics[0]!.file).toBe('broken.qmd');
      // The good file still rendered.
      expect(r.outputs).toContain('_site/good.html');
    } finally {
      await f.close();
    }
  });

  it('a clean render reports ok with produced outputs', async () => {
    process.env['FAKE_Q2_MODE'] = 'success';
    const r = await seedAndRender({});
    expect(r.ok).toBe(true);
    expect(r.exit_code).toBe(0);
    expect(r.diagnostics).toEqual([]);
    expect(r.outputs).toContain('_site/bad.html');
  });

  it('a single-file render targets just that file', async () => {
    process.env['FAKE_Q2_MODE'] = 'success';
    const r = await seedAndRender({ path: 'bad.qmd' });
    expect(r.target).toBe('bad.qmd');
    expect(r.mode).toBe('file');
  });

  it('a project render runs the directory when _quarto.yml is present', async () => {
    process.env['FAKE_Q2_MODE'] = 'success';
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [
        { path: '_quarto.yml', content: 'project:\n  type: default\n' },
        { path: 'index.qmd', content: '# Hi\n' },
      ]);
      const r = structured<RenderResult>(
        await callTool(f, 'render', { project: seed.indexDocId }),
      );
      expect(r.mode).toBe('project');
      expect(r.target).toBe('.');
      expect(r.ok).toBe(true);
    } finally {
      await f.close();
    }
  });

  it('errors actionably when there is nothing to render', async () => {
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [{ path: 'data.csv', content: 'a,b\n1,2\n' }]);
      const result = await callTool(f, 'render', { project: seed.indexDocId });
      expect(result.isError).toBe(true);
      const text = (result.content as Array<{ text?: string }>).map((b) => b.text ?? '').join();
      expect(text).toMatch(/nothing to render/);
      expect(text).toMatch(/_quarto\.yml/);
    } finally {
      await f.close();
    }
  });

  it('a runaway render is killed at the timeout and reported', async () => {
    process.env['FAKE_Q2_MODE'] = 'hang';
    const r = await seedAndRender({ timeout_seconds: 1 });
    expect(r.timed_out).toBe(true);
    expect(r.ok).toBe(false);
    expect(r.hint).toMatch(/timeout/i);
  }, 15000);

  it('a missing q2 binary is an actionable error naming QUARTO_Q2_PATH', async () => {
    process.env['QUARTO_Q2_PATH'] = join(stubDir, 'no-such-q2');
    const f = await startInMemoryMcp({ allowRender: true });
    try {
      const seed = await seedProject(f, [{ path: 'a.qmd', content: '# a\n' }]);
      const result = await callTool(f, 'render', { project: seed.indexDocId });
      expect(result.isError).toBe(true);
      const text = (result.content as Array<{ text?: string }>).map((b) => b.text ?? '').join();
      expect(text).toMatch(/QUARTO_Q2_PATH/);
    } finally {
      await f.close();
    }
  });
});

describe('materializeProject', () => {
  it('writes text and binary payloads, creating parent dirs', () => {
    const dir = mkdtempSync(join(tmpdir(), 'mat-'));
    try {
      const files = new Map<string, FilePayload>([
        ['index.qmd', { type: 'text', text: '# Hi\n' }],
        ['notes/memo.md', { type: 'text', text: 'memo\n' }],
        ['img/logo.png', { type: 'binary', data: new Uint8Array([1, 2, 3]), mimeType: 'image/png' }],
      ]);
      const { written, skipped } = materializeProject(files, dir);
      expect(written.sort()).toEqual(['img/logo.png', 'index.qmd', 'notes/memo.md']);
      expect(skipped).toEqual([]);
      expect(readFileSync(join(dir, 'index.qmd'), 'utf8')).toBe('# Hi\n');
      expect(readFileSync(join(dir, 'notes/memo.md'), 'utf8')).toBe('memo\n');
      expect(readFileSync(join(dir, 'img/logo.png'))).toEqual(Buffer.from([1, 2, 3]));
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  it('refuses paths that escape the temp dir', () => {
    const dir = mkdtempSync(join(tmpdir(), 'mat-'));
    const sibling = join(dir, '..', `escape-${process.pid}.txt`);
    try {
      const files = new Map<string, FilePayload>([
        ['../escape.txt', { type: 'text', text: 'owned\n' }],
        ['/abs/escape.txt', { type: 'text', text: 'owned\n' }],
        ['ok.qmd', { type: 'text', text: 'fine\n' }],
      ]);
      const { written, skipped } = materializeProject(files, dir);
      expect(written).toEqual(['ok.qmd']);
      expect(skipped.map((s) => s.path).sort()).toEqual(['../escape.txt', '/abs/escape.txt']);
      expect(existsSync(sibling)).toBe(false);
      expect(existsSync('/abs/escape.txt')).toBe(false);
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
