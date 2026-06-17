// Contract: register on mount, dispose only on unmount (a file switch must not
// tear the providers down); re-tokenise on mount and once on WASM-ready.

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import type * as Monaco from 'monaco-editor';

const registerSpy = vi.fn();
const disposeSpy = vi.fn();
const refreshSpy = vi.fn();
vi.mock('../services/monacoProviders', () => ({
  registerIntelligenceProviders: (...args: unknown[]) => registerSpy(...args),
  disposeIntelligenceProviders: (...args: unknown[]) => disposeSpy(...args),
  refreshSemanticTokens: (...args: unknown[]) => refreshSpy(...args),
}));

import { useIntelligenceProviders } from './useIntelligenceProviders';

type WasmStatus = 'loading' | 'ready' | 'error';
const fakeMonaco = {} as unknown as typeof Monaco;

describe('useIntelligenceProviders', () => {
  beforeEach(() => {
    registerSpy.mockReset();
    disposeSpy.mockReset();
    refreshSpy.mockReset();
  });

  it('registers on editor mount and disposes only on unmount', () => {
    // Each render passes a fresh getter identity, mimicking how the Editor's
    // path getter changes when `currentFile` changes identity.
    const { result, rerender, unmount } = renderHook(
      ({ path }: { path: string }) =>
        useIntelligenceProviders(() => path, 'ready', true),
      { initialProps: { path: 'index.qmd' } }
    );

    // Editor mounts → register once, and force an immediate re-tokenise so the
    // correct colours appear without waiting out Monaco's adaptive debounce.
    result.current(fakeMonaco);
    expect(registerSpy).toHaveBeenCalledTimes(1);
    expect(refreshSpy).toHaveBeenCalled();
    expect(disposeSpy).not.toHaveBeenCalled();

    // currentFile changes identity, same path (no Monaco remount) — must NOT
    // dispose the providers.
    rerender({ path: 'index.qmd' });
    // currentFile changes to a different path.
    rerender({ path: 'other.qmd' });
    expect(disposeSpy).not.toHaveBeenCalled();

    // Only a true unmount disposes, exactly once.
    unmount();
    expect(disposeSpy).toHaveBeenCalledTimes(1);
  });

  it('fires the mount refresh exactly once per editor mount', () => {
    // 'loading' so the WASM-ready effect stays quiet and we isolate the
    // mount-handler refresh.
    const { result } = renderHook(() =>
      useIntelligenceProviders(() => 'index.qmd', 'loading', false)
    );
    expect(refreshSpy).not.toHaveBeenCalled();
    result.current(fakeMonaco);
    expect(refreshSpy).toHaveBeenCalledTimes(1);
  });

  it('re-tokenises once WASM becomes ready after the editor has mounted', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useIntelligenceProviders(() => 'index.qmd', status, ready),
      { initialProps: { status: 'loading' as WasmStatus, ready: false } }
    );
    expect(refreshSpy).not.toHaveBeenCalled();

    // Editor mounts while WASM is still loading — nothing to refresh yet.
    rerender({ status: 'loading', ready: true });
    expect(refreshSpy).not.toHaveBeenCalled();

    // WASM finishes initializing — re-tokenise now.
    rerender({ status: 'ready', ready: true });
    expect(refreshSpy).toHaveBeenCalledTimes(1);
  });

  it('does not fire the WASM-ready refresh before the editor is ready', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useIntelligenceProviders(() => 'index.qmd', status, ready),
      { initialProps: { status: 'loading' as WasmStatus, ready: false } }
    );
    rerender({ status: 'ready', ready: false });
    expect(refreshSpy).not.toHaveBeenCalled();
  });

  it('fires the WASM-ready refresh only once across re-renders', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useIntelligenceProviders(() => 'index.qmd', status, ready),
      { initialProps: { status: 'ready' as WasmStatus, ready: true } }
    );
    expect(refreshSpy).toHaveBeenCalledTimes(1);
    rerender({ status: 'ready', ready: true });
    rerender({ status: 'ready', ready: true });
    expect(refreshSpy).toHaveBeenCalledTimes(1);
  });

  it('never fires the WASM-ready refresh when WASM fails to initialize', () => {
    const { rerender } = renderHook(
      ({ status, ready }: { status: WasmStatus; ready: boolean }) =>
        useIntelligenceProviders(() => 'index.qmd', status, ready),
      { initialProps: { status: 'loading' as WasmStatus, ready: true } }
    );
    rerender({ status: 'error', ready: true });
    expect(refreshSpy).not.toHaveBeenCalled();
  });
});
