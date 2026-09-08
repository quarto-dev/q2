// Prototype: when Node >= 25 shadows `localStorage`/`sessionStorage` with its own
// (undefined-returning) accessor, vitest's jsdom environment skips copying jsdom's
// Storage onto the global. Re-point the globals at the jsdom window's real Storage.
type G = typeof globalThis & { jsdom?: { window: Window } };
const g = globalThis as G;
for (const key of ['localStorage', 'sessionStorage'] as const) {
  if (g[key] === undefined && g.jsdom?.window?.[key]) {
    Object.defineProperty(g, key, { value: g.jsdom.window[key], configurable: true, writable: true, enumerable: true });
  }
}
