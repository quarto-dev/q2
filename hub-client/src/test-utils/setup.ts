/**
 * Test setup file for integration tests (jsdom environment)
 *
 * This file is loaded before all integration tests via vitest.integration.config.ts.
 * It provides polyfills and mocks for browser APIs not available in Node.js/jsdom.
 */

import '@testing-library/jest-dom';
import { vi } from 'vitest';

// Provide IndexedDB in Node.js environment
// fake-indexeddb is a drop-in replacement that works with the 'idb' library
import 'fake-indexeddb/auto';

// Mock crypto.randomUUID for presence service (modern Node has this, but ensure consistency)
if (!globalThis.crypto?.randomUUID) {
  const cryptoPolyfill = {
    ...globalThis.crypto,
    randomUUID: () => 'test-uuid-' + Math.random().toString(36).substring(2, 11),
  } as Crypto;
  Object.defineProperty(globalThis, 'crypto', { value: cryptoPolyfill });
}

// Mock ResizeObserver (jsdom doesn't provide this)
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
  }));
}

// Mock IntersectionObserver (jsdom doesn't provide this)
if (!globalThis.IntersectionObserver) {
  globalThis.IntersectionObserver = vi.fn().mockImplementation(() => ({
    observe: vi.fn(),
    unobserve: vi.fn(),
    disconnect: vi.fn(),
    root: null,
    rootMargin: '',
    thresholds: [],
    takeRecords: () => [],
  }));
}

// Mock matchMedia (jsdom doesn't provide this)
if (!globalThis.matchMedia) {
  globalThis.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

// Mock requestAnimationFrame/cancelAnimationFrame (jsdom provides these, but ensure consistency)
if (!globalThis.requestAnimationFrame) {
  globalThis.requestAnimationFrame = vi.fn().mockImplementation((cb: FrameRequestCallback) => {
    return setTimeout(() => cb(Date.now()), 0);
  });
}

if (!globalThis.cancelAnimationFrame) {
  globalThis.cancelAnimationFrame = vi.fn().mockImplementation((id: number) => {
    clearTimeout(id);
  });
}

// Mock URL.createObjectURL and URL.revokeObjectURL (for blob handling)
if (!URL.createObjectURL) {
  URL.createObjectURL = vi.fn().mockImplementation(() => 'blob:mock-url');
}

if (!URL.revokeObjectURL) {
  URL.revokeObjectURL = vi.fn();
}

// Suppress console.error for expected warnings during tests (optional)
// Uncomment if React's strict mode warnings become too noisy
// const originalConsoleError = console.error;
// console.error = (...args: unknown[]) => {
//   if (typeof args[0] === 'string' && args[0].includes('Warning:')) {
//     return;
//   }
//   originalConsoleError(...args);
// };
