/**
 * WASM-JS Bridge for URL Fetching
 *
 * This module provides a fetch function called from Rust WASM code via
 * wasm-bindgen. Used by `pandoc.mediabag.fetch()` in Lua filters and
 * shortcodes to retrieve remote resources.
 *
 * The function is imported by quarto-system-runtime/src/wasm.rs using:
 *
 *   #[wasm_bindgen(raw_module = "/src/wasm-js-bridge/fetch.js")]
 *
 * Design:
 * - Content is base64-encoded so binary data can be returned as a JSON string,
 *   avoiding complex wasm-bindgen type marshalling for multi-value returns.
 * - Non-ok HTTP responses (4xx, 5xx) are treated as errors.
 */

/**
 * Fetch content from a URL.
 *
 * @param {string} url - The URL to fetch
 * @returns {Promise<string>} JSON string: `{ "mimeType": string, "content": string }`
 *   where `content` is the response body base64-encoded.
 * @throws {Error} If the request fails or the response status is not ok
 */
export async function jsFetchUrl(url) {
  const response = await fetch(url);

  if (!response.ok) {
    throw new Error(
      `HTTP ${response.status} ${response.statusText} for ${url}`
    );
  }

  const mimeType =
    response.headers.get("content-type") || "application/octet-stream";

  const buffer = await response.arrayBuffer();
  const bytes = new Uint8Array(buffer);

  // Base64-encode the binary content for JSON transport.
  // btoa() works on binary strings; we build one from the byte array.
  let binary = "";
  for (let i = 0; i < bytes.byteLength; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  const content = btoa(binary);

  return JSON.stringify({ mimeType, content });
}
