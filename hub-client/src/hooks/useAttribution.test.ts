/**
 * Tests for useAttribution hook — React hook for per-character attribution.
 *
 * Test spec 4 from the plan.
 *
 * @vitest-environment jsdom
 */

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';

// Mock the attribution service
vi.mock('../services/attribution', () => ({
  buildAttributionMap: vi.fn(),
  updateAttributionMap: vi.fn(),
  buildByteToCharMap: vi.fn(),
  HistoryCompactedError: class HistoryCompactedError extends Error {
    constructor() { super('History compacted'); this.name = 'HistoryCompactedError'; }
  },
}));

// Mock automergeSync
vi.mock('../services/automergeSync', () => ({
  getFileHandle: vi.fn(),
}));

import { useAttribution } from './useAttribution';
import {
  buildAttributionMap,
  updateAttributionMap,
  buildByteToCharMap,
  HistoryCompactedError,
} from '../services/attribution';
import { getFileHandle } from '../services/automergeSync';
import type { AttributionMap } from '../services/attribution';

const mockBuildAttributionMap = vi.mocked(buildAttributionMap);
const mockUpdateAttributionMap = vi.mocked(updateAttributionMap);
const mockBuildByteToCharMap = vi.mocked(buildByteToCharMap);
const mockGetFileHandle = vi.mocked(getFileHandle);

function createMockMap(overrides?: Partial<AttributionMap>): AttributionMap {
  return {
    entries: [{ actor: 'actor1', time: 1000 }],
    processedHeads: ['h1'],
    processedHistoryIndex: 1,
    ...overrides,
  };
}

describe('useAttribution', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.useFakeTimers();
    mockBuildByteToCharMap.mockReturnValue([0, 1]);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns null when getFileHandle() returns null (offline)', () => {
    mockGetFileHandle.mockReturnValue(null);

    const { result } = renderHook(() =>
      useAttribution('index.qmd', 'hello')
    );

    expect(result.current).toBeNull();
    expect(mockBuildAttributionMap).not.toHaveBeenCalled();
  });

  it('starts async buildAttributionMap on mount, returns null until resolved', async () => {
    const mockMap = createMockMap();
    const handle = { __mock: true };
    mockGetFileHandle.mockReturnValue(handle as any);

    let resolvePromise: (map: AttributionMap) => void;
    mockBuildAttributionMap.mockReturnValue(
      new Promise(resolve => { resolvePromise = resolve as any; })
    );

    const { result } = renderHook(() =>
      useAttribution('index.qmd', 'hello')
    );

    // Initially null while building
    expect(result.current).toBeNull();
    expect(mockBuildAttributionMap).toHaveBeenCalledOnce();

    // Resolve the build
    await act(async () => {
      resolvePromise!(mockMap);
    });

    // Now should have the map
    expect(result.current).not.toBeNull();
    expect(result.current!.entries).toBe(mockMap.entries);
  });

  it('calls updateAttributionMap on sourceText change when map exists', async () => {
    const mockMap = createMockMap();
    const updatedMap = createMockMap({ processedHistoryIndex: 2, processedHeads: ['h2'] });
    const handle = { __mock: true };
    mockGetFileHandle.mockReturnValue(handle as any);
    mockBuildAttributionMap.mockResolvedValue(mockMap);
    mockUpdateAttributionMap.mockReturnValue(updatedMap);

    const { result, rerender } = renderHook(
      ({ text }) => useAttribution('index.qmd', text),
      { initialProps: { text: 'hello' } },
    );

    // Wait for initial build
    await act(async () => {});

    expect(result.current).not.toBeNull();

    // Trigger a text change
    rerender({ text: 'hello world' });

    // Advance debounce timer
    await act(async () => {
      vi.advanceTimersByTime(600);
    });

    expect(mockUpdateAttributionMap).toHaveBeenCalled();
  });

  it('catches HistoryCompactedError and triggers fresh buildAttributionMap', async () => {
    const mockMap = createMockMap();
    const freshMap = createMockMap({ processedHistoryIndex: 5 });
    const handle = { __mock: true };
    mockGetFileHandle.mockReturnValue(handle as any);
    mockBuildAttributionMap
      .mockResolvedValueOnce(mockMap)    // initial build
      .mockResolvedValueOnce(freshMap);  // rebuild after compaction

    mockUpdateAttributionMap.mockImplementation(() => {
      throw new HistoryCompactedError();
    });

    const { result, rerender } = renderHook(
      ({ text }) => useAttribution('index.qmd', text),
      { initialProps: { text: 'hello' } },
    );

    // Wait for initial build
    await act(async () => {});

    expect(result.current).not.toBeNull();

    // Trigger text change → updateAttributionMap throws HistoryCompactedError
    rerender({ text: 'hello world' });

    await act(async () => {
      vi.advanceTimersByTime(600);
    });

    // Should have started a new build
    expect(mockBuildAttributionMap).toHaveBeenCalledTimes(2);

    // Wait for rebuild
    await act(async () => {});

    expect(result.current!.entries).toBe(freshMap.entries);
  });

  it('aborts in-flight build and starts fresh on filePath change', async () => {
    const handle = { __mock: true };
    mockGetFileHandle.mockReturnValue(handle as any);

    let resolveFirst: (map: AttributionMap | null) => void;
    mockBuildAttributionMap
      .mockReturnValueOnce(
        new Promise(resolve => { resolveFirst = resolve as any; })
      )
      .mockResolvedValueOnce(createMockMap({ processedHistoryIndex: 2 }));

    const { result, rerender } = renderHook(
      ({ path }) => useAttribution(path, 'hello'),
      { initialProps: { path: 'file1.qmd' } },
    );

    expect(mockBuildAttributionMap).toHaveBeenCalledTimes(1);

    // Switch file before first build completes
    rerender({ path: 'file2.qmd' });

    // The first build's signal should be aborted
    const firstSignal = mockBuildAttributionMap.mock.calls[0][2];
    expect(firstSignal?.aborted).toBe(true);

    // A fresh build should start
    expect(mockBuildAttributionMap).toHaveBeenCalledTimes(2);

    // Resolve the first build (should be ignored since signal was aborted)
    await act(async () => {
      resolveFirst!(null);
    });

    // Wait for second build to resolve
    await act(async () => {});

    expect(result.current).not.toBeNull();
    expect(result.current).not.toBeNull();
    expect(result.current!.entries).toHaveLength(1);
  });

  it('aborts in-flight build on unmount', async () => {
    const handle = { __mock: true };
    mockGetFileHandle.mockReturnValue(handle as any);
    mockBuildAttributionMap.mockReturnValue(new Promise(() => {})); // never resolves

    const { unmount } = renderHook(() =>
      useAttribution('index.qmd', 'hello')
    );

    expect(mockBuildAttributionMap).toHaveBeenCalledTimes(1);

    unmount();

    // Signal should have been aborted
    const signal = mockBuildAttributionMap.mock.calls[0][2];
    expect(signal?.aborted).toBe(true);
  });
});
