/**
 * Phase 5 unit tests for the intelligence service's getSemanticTokens helper.
 * The WASM module and initWasm are mocked; this exercises the JSON-decode and
 * graceful-degradation contract (everything collapses to [], never rejects).
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';

const lspGetSemanticTokensMock = vi.fn();

vi.mock('@quarto/preview-runtime', () => ({
  initWasm: vi.fn(async () => {}),
}));

vi.mock('wasm-quarto-hub-client', () => ({
  lsp_get_semantic_tokens: (...args: unknown[]) => lspGetSemanticTokensMock(...args),
}));

import { getSemanticTokens } from './intelligenceService';

describe('getSemanticTokens', () => {
  beforeEach(() => {
    lspGetSemanticTokensMock.mockReset();
  });

  it('returns [] for a non-qmd path without calling WASM', async () => {
    const result = await getSemanticTokens('notes.txt');
    expect(result).toEqual([]);
    expect(lspGetSemanticTokensMock).not.toHaveBeenCalled();
  });

  it('returns the tokens from a success envelope', async () => {
    const tokens = [{ line: 0, character: 0, length: 1, tokenType: 0, modifiers: 0 }];
    lspGetSemanticTokensMock.mockReturnValue(JSON.stringify({ success: true, tokens }));
    const result = await getSemanticTokens('doc.qmd');
    expect(result).toEqual(tokens);
  });

  it('treats .md as a source file and calls WASM (bd-6d2wj4zp Phase 5)', async () => {
    // D11: .md render inputs get the same intelligence treatment as
    // .qmd — the source-file gate must not short-circuit them.
    const tokens = [{ line: 0, character: 0, length: 1, tokenType: 0, modifiers: 0 }];
    lspGetSemanticTokensMock.mockReturnValue(JSON.stringify({ success: true, tokens }));
    const result = await getSemanticTokens('notes.md');
    expect(result).toEqual(tokens);
    expect(lspGetSemanticTokensMock).toHaveBeenCalled();
  });

  it('returns [] for a failure envelope', async () => {
    lspGetSemanticTokensMock.mockReturnValue(JSON.stringify({ success: false, error: 'boom' }));
    const result = await getSemanticTokens('doc.qmd');
    expect(result).toEqual([]);
  });

  it('returns [] when the response is not valid JSON', async () => {
    lspGetSemanticTokensMock.mockReturnValue('not json');
    const result = await getSemanticTokens('doc.qmd');
    expect(result).toEqual([]);
  });

  it('returns [] for a success envelope with no tokens field', async () => {
    lspGetSemanticTokensMock.mockReturnValue(JSON.stringify({ success: true }));
    const result = await getSemanticTokens('doc.qmd');
    expect(result).toEqual([]);
  });
});
