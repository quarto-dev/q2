/**
 * The editor-mount semantic-tokens refresh can fire before the WASM highlighter
 * has finished initializing (cold start, first file), in which case the first
 * fetch returns no tokens and the Monarch base shows until the next edit. This
 * hook re-fires the refresh on the loading→ready transition so the correct
 * colours appear as soon as the highlighter is available.
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';

const refreshSpy = vi.fn();
vi.mock('../services/monacoProviders', () => ({
  refreshSemanticTokens: (...args: unknown[]) => refreshSpy(...args),
}));

import { useSemanticTokensWasmRefresh } from './useSemanticTokensWasmRefresh';

type WasmStatus = 'loading' | 'ready' | 'error';

describe('useSemanticTokensWasmRefresh', () => {
  beforeEach(() => {
    refreshSpy.mockReset();
  });

  it('fires once WASM becomes ready after the editor has mounted', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useSemanticTokensWasmRefresh(status, ready),
      { initialProps: { status: 'loading', ready: false } }
    );
    expect(refreshSpy).not.toHaveBeenCalled();

    // Editor mounts while WASM is still loading — nothing to refresh yet.
    rerender({ status: 'loading', ready: true });
    expect(refreshSpy).not.toHaveBeenCalled();

    // WASM finishes initializing — re-tokenise now.
    rerender({ status: 'ready', ready: true });
    expect(refreshSpy).toHaveBeenCalledTimes(1);
  });

  it('does not fire while the editor is not ready', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useSemanticTokensWasmRefresh(status, ready),
      { initialProps: { status: 'loading', ready: false } }
    );
    rerender({ status: 'ready', ready: false });
    expect(refreshSpy).not.toHaveBeenCalled();
  });

  it('fires only once across subsequent re-renders', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useSemanticTokensWasmRefresh(status, ready),
      { initialProps: { status: 'ready', ready: true } }
    );
    expect(refreshSpy).toHaveBeenCalledTimes(1);
    rerender({ status: 'ready', ready: true });
    rerender({ status: 'ready', ready: true });
    expect(refreshSpy).toHaveBeenCalledTimes(1);
  });

  it('never fires when WASM fails to initialize', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useSemanticTokensWasmRefresh(status, ready),
      { initialProps: { status: 'loading', ready: true } }
    );
    rerender({ status: 'error', ready: true });
    expect(refreshSpy).not.toHaveBeenCalled();
  });
});
