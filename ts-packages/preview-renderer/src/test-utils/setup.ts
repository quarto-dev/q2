import '@testing-library/jest-dom';
import { vi } from 'vitest';

if (!globalThis.crypto?.randomUUID) {
  const cryptoPolyfill = {
    ...globalThis.crypto,
    randomUUID: () => 'test-uuid-' + Math.random().toString(36).substring(2, 11),
  } as Crypto;
  Object.defineProperty(globalThis, 'crypto', { value: cryptoPolyfill });
}

if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
  }));
}

if (!globalThis.IntersectionObserver) {
  globalThis.IntersectionObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
    root: null,
    rootMargin: '',
    thresholds: [],
    takeRecords: () => [],
  })) as unknown as typeof IntersectionObserver;
}
