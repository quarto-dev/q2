/**
 * Byte-level characterization and capture-gating tests for preview renders.
 *
 * Phase 3A must remove only the discarded project pass-2 capture: the
 * response's transformed and untransformed AST bytes must remain unchanged,
 * including the original relative active-path source name.
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

interface WasmModule {
  default: (input?: BufferSource) => Promise<void>;
  render_page_in_project_with_attribution: (
    path: string,
    userGrammars?: unknown,
    attributionJson?: string | null,
    captureGzJson?: Uint8Array,
    preferPreviewFormat?: boolean,
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

async function renderProject(): Promise<RenderResponse> {
  const result = JSON.parse(
    await wasm.render_page_in_project_with_attribution(ACTIVE_PATH, undefined, null),
  ) as RenderResponse;
  expect(result.success, result.error).toBe(true);
  return result;
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
