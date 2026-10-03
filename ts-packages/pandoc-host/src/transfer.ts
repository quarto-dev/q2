import type { PandocRequest, ShareTree } from './types.ts';

/**
 * Makes byte views safe to `postMessage`: a view that does not span its whole buffer is copied
 * (structured clone would otherwise copy the entire backing buffer, and transferring a sub-view
 * of wasm memory throws), and the transfer list names each distinct ArrayBuffer once (a
 * duplicate entry throws).
 */
function owner() {
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
  return { own, transfer };
}

/**
 * Prepare a request (and optionally the share tree) for `postMessage`; see `owner` for what
 * is copied and transferred.
 */
export function prepareForPost<T extends PandocRequest | ShareTree>(value: T): { value: T; transfer: ArrayBuffer[] } {
  const { own, transfer } = owner();
  const files = (l: { path: string; bytes: Uint8Array }[]) => l.map((f) => ({ path: f.path, bytes: own(f.bytes) }));
  const v = { ...value } as Record<string, unknown>;
  for (const k of ['files', 'resource_refs'] as const) if (Array.isArray(v[k])) v[k] = files(v[k] as never);
  return { value: v as T, transfer };
}

/**
 * Prepare `execute`'s `inputs` for the `run` message, same rules as `prepareForPost`. Its
 * transfer list is separate, so a caller posting both concatenates the two lists; a buffer shared
 * between a request and an input would then be listed twice, which `postMessage` rejects, so
 * inputs are never views of the request's buffers.
 */
export function prepareInputsForPost(inputs: Record<string, Uint8Array>): { value: Record<string, Uint8Array>; transfer: ArrayBuffer[] } {
  const { own, transfer } = owner();
  const value: Record<string, Uint8Array> = {};
  for (const [path, bytes] of Object.entries(inputs)) value[path] = own(bytes);
  return { value, transfer };
}
