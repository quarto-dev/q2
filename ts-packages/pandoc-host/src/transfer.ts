import type { PandocRequest, ShareTree } from './types.ts';

/**
 * Prepare a request (and optionally the share tree) for `postMessage`: byte views that
 * do not span their whole buffer are copied (structured clone would otherwise copy the
 * entire backing buffer, and transferring a sub-view of wasm memory throws), and the
 * transfer list names each distinct ArrayBuffer once (a duplicate entry throws).
 */
export function prepareForPost<T extends PandocRequest | ShareTree>(value: T): { value: T; transfer: ArrayBuffer[] } {
  const seen = new Set<ArrayBuffer>();
  const transfer: ArrayBuffer[] = [];
  const own = (b: Uint8Array): Uint8Array => {
    let v = b;
    const buf = b.buffer;
    if (!(buf instanceof ArrayBuffer) || b.byteOffset !== 0 || b.byteLength !== buf.byteLength) v = b.slice();
    // `slice()` returns a fresh buffer; either way each buffer is listed once.
    if (!seen.has(v.buffer as ArrayBuffer)) {
      seen.add(v.buffer as ArrayBuffer);
      transfer.push(v.buffer as ArrayBuffer);
    }
    return v;
  };
  const files = (l: { path: string; bytes: Uint8Array }[]) => l.map((f) => ({ path: f.path, bytes: own(f.bytes) }));
  const v = { ...value } as Record<string, unknown>;
  for (const k of ['files', 'resource_refs'] as const) if (Array.isArray(v[k])) v[k] = files(v[k] as never);
  return { value: v as T, transfer };
}
