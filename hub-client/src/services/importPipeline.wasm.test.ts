/**
 * WASM end-to-end tests for the document-import exports (document import epic, P3 T8):
 * `get_import_formats`, `prepare_import`, `finish_import` and `classify_import_failure`.
 *
 * The first describe needs only the Rust wasm, so a missing pandoc.wasm asset must not skip
 * it (unlike `pandocRequest.wasm.test.ts`'s `describe.skipIf(!pandocWasmAvailable())`). The
 * last one runs the whole pipeline (prepare → pandoc.wasm → finish) when the asset is there.
 *
 * Run with: npm run test:wasm
 */
import { describe, it, expect, beforeAll } from 'vitest';
import { readFileSync } from 'node:fs';
import { readFile } from 'fs/promises';
import { dirname, join } from 'path';
import { fileURLToPath } from 'url';
import { execute } from '@quarto/pandoc-host';
import {
  IMPORT_RECORDINGS,
  WASM_PATH,
  loadImportRecording,
  pandocWasmAvailable,
} from '../test-utils/pandocRecordings';

interface Diagnostic {
  kind: string;
  title: string;
  code?: string;
  problem?: string;
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  get_import_formats: () => string;
  prepare_import: (file_name: string, size: number, sha256_hex: string) => string;
  finish_import: (
    json_text: string,
    stderr: string,
    target_qmd_path: string,
    media_manifest_json: string,
    format?: string,
  ) => string;
  classify_import_failure: (kind: string, status: number | null | undefined, stderr: string) => string;
}

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const wasmBytes = await readFile(join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'));
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(wasmBytes);
});

/** The interface-2 media manifest for a recording's collected files (P1 README). */
function manifestFor(name: string): string {
  const rec = loadImportRecording(name);
  return JSON.stringify(
    rec.media.map((m) => ({
      pandoc_path: m.pandocPath,
      status: 'stored',
      sha256: m.sha256,
      ext: m.rel.slice(m.rel.lastIndexOf('.') + 1),
    })),
  );
}

const expectedQmd = (name: string): string =>
  readFileSync(join(IMPORT_RECORDINGS, name, 'expected.qmd'), 'utf8');

describe('import exports in the built Rust wasm', () => {
  it('get_import_formats lists the five formats and the size cap', () => {
    const table = JSON.parse(wasm.get_import_formats());
    expect(table.formats.map((f: { id: string }) => f.id)).toEqual(['docx', 'odt', 'rtf', 'epub', 'pptx']);
    expect(table.formats[0].extensions).toEqual(['.docx']);
    expect(table.formats[2].mime_types).toEqual(['application/rtf', 'text/rtf']);
    expect(table.max_source_bytes).toBe(26214400);
  });

  it('prepare_import: validation only, refusals, and the request', () => {
    const ok = JSON.parse(wasm.prepare_import('Report.DOCX', 1000, ''));
    expect(ok).toMatchObject({ success: true, format: 'docx', diagnostics: [] });
    expect(ok.request).toBeUndefined();

    const unsupported = JSON.parse(wasm.prepare_import('notes.md', 10, ''));
    expect(unsupported.success).toBe(false);
    expect(unsupported.diagnostics[0].code).toBe('Q-24-1');

    const big = JSON.parse(wasm.prepare_import('big.docx', 26214401, ''));
    expect(big.success).toBe(false);
    expect(big.diagnostics[0].code).toBe('Q-24-2');

    const rec = loadImportRecording('basic-docx');
    const sha = rec.request.host_inputs![0].sha256;
    const prepared = JSON.parse(wasm.prepare_import('basic.docx', rec.source.byteLength, sha));
    expect(prepared.success).toBe(true);
    expect(prepared.source_path).toBe(rec.sourcePath);
    expect(prepared.share_tree).toEqual(rec.shareTree);
    // The request Rust builds is the one P1 built by hand from `argv.json`, job id aside.
    expect({ ...prepared.request, job_id: '' }).toEqual({ ...rec.request, job_id: '' });
    expect(prepared.request.job_id).toMatch(/^[0-9a-f]{16}$/);
  });

  it('prepare_import refuses a non-finite size without throwing', () => {
    const r = JSON.parse(wasm.prepare_import('a.docx', Number.NaN, ''));
    expect(r.success).toBe(false);
    expect(r.diagnostics[0].code).toBe('Q-24-12');
  });

  it('finish_import turns a recording into its expected qmd', () => {
    for (const name of ['basic-docx', 'track-changes-docx', 'images-docx']) {
      const rec = loadImportRecording(name);
      const r = JSON.parse(
        wasm.finish_import(JSON.stringify(rec.pandocJson), rec.stderr, `${name}.qmd`, manifestFor(name), 'docx'),
      );
      expect(r.success, name).toBe(true);
      expect(r.qmd).toBe(expectedQmd(name));
      expect(Array.isArray(r.media_plan)).toBe(true);
    }
    const images = JSON.parse(
      wasm.finish_import(
        JSON.stringify(loadImportRecording('images-docx').pandocJson),
        '',
        'images-docx.qmd',
        manifestFor('images-docx'),
        'docx',
      ),
    );
    expect(images.media_plan).toHaveLength(2);
    expect(images.media_plan[0].project_path).toMatch(/^images-docx_media\/[0-9a-f]{12}\.(png|jpg)$/);
  });

  it('finish_import reports a fatal error as success: false with no qmd', () => {
    const r = JSON.parse(wasm.finish_import('not json', '', 'x.qmd', '[]'));
    expect(r.success).toBe(false);
    expect(r.qmd).toBeUndefined();
    expect(r.media_plan).toBeUndefined();
    expect((r.diagnostics as Diagnostic[]).at(-1)?.code).toBe('Q-24-12');
  });

  it('finish_import gives a pptx import format: revealjs when told it is a pptx', () => {
    const rec = loadImportRecording('basic-pptx');
    const withFormat = JSON.parse(wasm.finish_import(JSON.stringify(rec.pandocJson), '', 'deck.qmd', '[]', 'pptx'));
    expect(withFormat.qmd).toContain('format: revealjs');
    const without = JSON.parse(wasm.finish_import(JSON.stringify(rec.pandocJson), '', 'deck.qmd', '[]'));
    expect(without.qmd).not.toContain('revealjs');
  });

  it('classify_import_failure maps the corrupt-docx recording to Q-24-3', () => {
    const rec = loadImportRecording('corrupt-docx');
    const r = JSON.parse(wasm.classify_import_failure('pandoc-exit', rec.status, rec.stderr));
    expect(r.diagnostics).toHaveLength(1);
    expect(r.diagnostics[0].code).toBe('Q-24-3');
    expect(JSON.stringify(r.diagnostics[0])).toContain("couldn't unpack docx container");
    for (const [kind, code] of [
      ['oom', 'Q-24-13'],
      ['crash', 'Q-24-13'],
      ['timeout', 'Q-24-13'],
      ['no-output', 'Q-24-3'],
      ['superseded', 'Q-24-12'],
    ] as const) {
      const d = JSON.parse(wasm.classify_import_failure(kind, null, ''));
      expect(d.diagnostics[0].code, kind).toBe(code);
    }
  });
});

describe.skipIf(!pandocWasmAvailable())('prepare → pandoc.wasm → finish', () => {
  let module: WebAssembly.Module;
  beforeAll(async () => {
    module = await WebAssembly.compile(readFileSync(WASM_PATH));
  });

  for (const name of ['basic-docx', 'track-changes-docx', 'images-docx', 'emf-docx']) {
    it(`${name} imports to its expected qmd`, async () => {
      const rec = loadImportRecording(name);
      const file = rec.sourcePath.slice(rec.sourcePath.lastIndexOf('/') + 1).replace('source', name);
      const prepared = JSON.parse(wasm.prepare_import(file, rec.source.byteLength, rec.request.host_inputs![0].sha256));
      expect(prepared.success).toBe(true);
      const run = await execute(prepared.request, prepared.share_tree, {
        module,
        inputs: { [prepared.source_path]: rec.source.slice() },
      });
      if (!run.ok) throw new Error(`${run.kind}: ${run.stderr}`);
      const manifest = run.collected.map((f) => {
        const ext = f.path.slice(f.path.lastIndexOf('.') + 1);
        const entry: Record<string, unknown> = {
          pandoc_path: f.path,
          status: 'stored',
          sha256: rec.media.find((m) => m.pandocPath === f.path)!.sha256,
          ext,
        };
        if (name === 'emf-docx' && (ext === 'emf' || ext === 'wmf')) entry.conversion_failed = true;
        return entry;
      });
      const finished = JSON.parse(
        wasm.finish_import(
          new TextDecoder().decode(run.output),
          run.stderr,
          `${name}.qmd`,
          JSON.stringify(manifest),
          prepared.format,
        ),
      );
      expect(finished.success, JSON.stringify(finished.diagnostics)).toBe(true);
      expect(finished.qmd).toBe(expectedQmd(name));
    });
  }

  it('corrupt-docx fails as pandoc-exit and classifies to Q-24-3', async () => {
    const rec = loadImportRecording('corrupt-docx');
    const prepared = JSON.parse(wasm.prepare_import('corrupt.docx', rec.source.byteLength, rec.request.host_inputs![0].sha256));
    const run = await execute(prepared.request, prepared.share_tree, {
      module,
      inputs: { [prepared.source_path]: rec.source.slice() },
    });
    expect(run.ok).toBe(false);
    if (run.ok) return;
    expect(run.kind).toBe('pandoc-exit');
    const d = JSON.parse(wasm.classify_import_failure(run.kind, run.status ?? null, run.stderr));
    expect(d.diagnostics[0].code).toBe('Q-24-3');
  });
});
