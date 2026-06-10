const encoder = new TextEncoder();
const decoder = new TextDecoder();

/**
 * Slice a string by UTF-8 byte offsets (not JS character offsets).
 *
 * Tree-sitter emits byte offsets into the UTF-8 representation of source
 * text. This function converts a substring bounded by those offsets back
 * to a JS string without the caller having to manage a TextEncoder /
 * TextDecoder pair directly.
 *
 * The `encoder` and `decoder` instances are module-level singletons so
 * callers that need to slice many spans from the same source string can
 * call `encoder.encode(text)` once and use `sliceEncodedUtf8` on the
 * resulting `Uint8Array` directly, keeping only this function for the
 * one-shot case.
 */
export function sliceUtf8(s: string, start: number, end: number): string {
  return decoder.decode(encoder.encode(s).subarray(start, end));
}

/**
 * Decode a subarray of pre-encoded UTF-8 bytes back to a string.
 *
 * Use this variant when the caller already holds the encoded `Uint8Array`
 * (e.g. because it is slicing many spans from the same source string and
 * wants to avoid re-encoding on every call).
 */
export function sliceEncodedUtf8(
  bytes: Uint8Array,
  start: number,
  end: number,
): string {
  return decoder.decode(bytes.subarray(start, end));
}
