/**
 * Byte-level characterization and capture-gating tests for preview renders.
 *
 * Phase 3A removed only the discarded project pass-2 capture. Phase 3B1 makes
 * the response capture optional without changing the transformed AST or the
 * default capture-on contract, including the original relative active-path
 * source name.
 */

import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { beforeAll, beforeEach, describe, expect, it } from 'vitest';
import { initWasm, vfsAddFile, vfsClear } from '@quarto/preview-runtime';

interface RenderResponse {
  success: boolean;
  error?: string;
  ast_json?: string;
  untransformed_ast_json?: string;
}

interface CaptureOptions {
  captureUntransformedAst?: boolean;
}

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  render_page_in_project_with_attribution: (
    path: string,
    userGrammars?: unknown,
    attributionJson?: string | null,
    captureGzJson?: Uint8Array,
    preferPreviewFormat?: boolean,
    options?: CaptureOptions,
  ) => Promise<string>;
  render_page_for_preview: (
    path: string,
    userGrammars?: unknown,
    captureGzJson?: Uint8Array,
    options?: CaptureOptions,
  ) => Promise<string>;
}

const ACTIVE_PATH = 'index.qmd';
const PROJECT_YML = 'project:\n  type: default\n';
const INDEX_QMD = `---
title: "Capture fixture"
format: q2-preview
---

# Heading

A *small* paragraph with [a link](other.qmd).
`;
const OTHER_QMD = `---
title: Other
---

# Other

A sibling page.
`;

// Pinned on the unmodified Phase 3A build. These are byte contracts, not
// snapshots: the optimization is invalid if either serialized AST changes.
const EXPECTED_AST_SHA256 = 'c98fd4c8a875b4f8a8cb5e910153b0ba8b3e6cb1502c0d8bbbe15cd3021afeab';
const EXPECTED_SINGLE_AST_SHA256 = '8599765d7edeaabd48916d0c4629010262189443614f440d867e8798f6e0314c';
const EXPECTED_UNTRANSFORMED_AST_SHA256 = '9b22dd9a3495eba60a3ae6b3efecbfbd254e0761125a73ba8587eaa8a2003733';

let wasm: WasmModule;

beforeAll(async () => {
  const here = dirname(fileURLToPath(import.meta.url));
  const bytes = await readFile(
    join(here, '../../wasm-quarto-hub-client', 'wasm_quarto_hub_client_bg.wasm'),
  );
  wasm = (await import('wasm-quarto-hub-client')) as unknown as WasmModule;
  await wasm.default(bytes);
  await initWasm();
});

beforeEach(() => {
  vfsClear();
  vfsAddFile('_quarto.yml', PROJECT_YML);
  vfsAddFile(ACTIVE_PATH, INDEX_QMD);
  vfsAddFile('other.qmd', OTHER_QMD);
});

function sha256(value: string): string {
  return createHash('sha256').update(value).digest('hex');
}

async function successfulResponse(render: Promise<string>): Promise<RenderResponse> {
  const result = JSON.parse(await render) as RenderResponse;
  expect(result.success, result.error).toBe(true);
  return result;
}

async function renderProject(): Promise<RenderResponse> {
  return successfulResponse(wasm.render_page_in_project_with_attribution(ACTIVE_PATH, undefined, null));
}

async function renderProjectWithOptions(options: CaptureOptions): Promise<RenderResponse> {
  return successfulResponse(
    wasm.render_page_in_project_with_attribution(
      ACTIVE_PATH,
      undefined,
      null,
      undefined,
      undefined,
      options,
    ),
  );
}

async function renderSingleDocument(): Promise<RenderResponse> {
  return successfulResponse(wasm.render_page_for_preview(ACTIVE_PATH));
}

async function renderSingleDocumentWithOptions(options: CaptureOptions): Promise<RenderResponse> {
  return successfulResponse(wasm.render_page_for_preview(ACTIVE_PATH, undefined, undefined, options));
}

function expectSameCaptureOnBytes(actual: RenderResponse, expected: RenderResponse): void {
  expect(actual.ast_json).toBe(expected.ast_json);
  expect(actual.untransformed_ast_json).toBe(expected.untransformed_ast_json);
}

function expectCaptureOff(off: RenderResponse, on: RenderResponse): void {
  expect(off).not.toHaveProperty('untransformed_ast_json');
  expect(off.ast_json).toBeDefined();
  expect(off.ast_json).toBe(on.ast_json);
}

describe('project preview capture characterization (Phase 3A)', () => {
  it('preserves both AST payloads and the original relative source name', async () => {
    const result = await renderProject();

    expect(result.ast_json).toBeDefined();
    expect(result.untransformed_ast_json).toBeDefined();
    expect(sha256(result.ast_json!)).toBe(EXPECTED_AST_SHA256);
    expect(sha256(result.untransformed_ast_json!)).toBe(EXPECTED_UNTRANSFORMED_AST_SHA256);

    const untransformed = JSON.parse(result.untransformed_ast_json!);
    expect(untransformed.astContext.files[0].name).toBe(ACTIVE_PATH);
    expect(untransformed.astContext.files[0].name).not.toBe('/project/index.qmd');
  });
});

describe('project preview capture gating (Phase 3B1)', () => {
  it('omits only the untransformed AST when capture is disabled', async () => {
    const on = await renderProject();
    const off = await renderProjectWithOptions({ captureUntransformedAst: false });

    expectCaptureOff(off, on);
  });

  it('keeps omitted options, an omitted flag, and explicit capture-on byte-identical', async () => {
    const omittedOptions = await renderProject();
    const omittedFlag = await renderProjectWithOptions({});
    const explicitOn = await renderProjectWithOptions({ captureUntransformedAst: true });

    expectSameCaptureOnBytes(omittedFlag, omittedOptions);
    expectSameCaptureOnBytes(explicitOn, omittedOptions);
    expect(sha256(explicitOn.ast_json!)).toBe(EXPECTED_AST_SHA256);
    expect(sha256(explicitOn.untransformed_ast_json!)).toBe(EXPECTED_UNTRANSFORMED_AST_SHA256);
  });
});

describe('single-document preview capture gating (Phase 3B1)', () => {
  beforeEach(() => {
    vfsClear();
    vfsAddFile(ACTIVE_PATH, INDEX_QMD);
    vfsAddFile('other.qmd', OTHER_QMD);
  });

  it('omits only the untransformed AST when capture is disabled', async () => {
    const on = await renderSingleDocument();
    const off = await renderSingleDocumentWithOptions({ captureUntransformedAst: false });

    expectCaptureOff(off, on);
  });

  it('keeps omitted options, an omitted flag, and explicit capture-on byte-identical', async () => {
    const omittedOptions = await renderSingleDocument();
    const omittedFlag = await renderSingleDocumentWithOptions({});
    const explicitOn = await renderSingleDocumentWithOptions({ captureUntransformedAst: true });

    expectSameCaptureOnBytes(omittedFlag, omittedOptions);
    expectSameCaptureOnBytes(explicitOn, omittedOptions);
    expect(sha256(explicitOn.ast_json!)).toBe(EXPECTED_SINGLE_AST_SHA256);
    expect(sha256(explicitOn.untransformed_ast_json!)).toBe(EXPECTED_UNTRANSFORMED_AST_SHA256);
    const untransformed = JSON.parse(explicitOn.untransformed_ast_json!);
    expect(untransformed.astContext.files[0].name).toBe(ACTIVE_PATH);
  });
});
