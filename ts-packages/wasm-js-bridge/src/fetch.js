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

/**
 * The URL policy of `jsFetchUrlHardened` (mirrors `validate_fetch_url` in
 * quarto-system-runtime): an absolute `https://` URL with a host. Relative
 * and protocol-relative URLs are refused, never resolved against the page
 * origin, which would reach the hub's own endpoints.
 *
 * @param {string} url
 * @throws {Error} If the URL is not an absolute https URL with a host
 */
export function assertFetchableUrl(url) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    throw new Error(`${url}: only absolute https:// URLs are fetched`);
  }
  if (parsed.protocol !== "https:" || !/^https:\/\//i.test(url.trim())) {
    throw new Error(
      `${url}: only absolute https:// URLs are fetched (relative, protocol-relative and non-https URLs are not)`
    );
  }
  if (!parsed.hostname) {
    throw new Error(`${url}: the URL has no host`);
  }
}

/**
 * Fetch content from a URL a document named (a remote image), under a policy.
 * Same result shape as `jsFetchUrl`.
 *
 * - `https` only (see {@link assertFetchableUrl}); a redirect that ends off
 *   https is refused.
 * - `credentials: 'omit'`, `referrerPolicy: 'no-referrer'`: the request carries
 *   no cookies, HTTP auth or referrer of the hub.
 * - The body is read as a stream and abandoned once it passes `maxBytes`.
 * - `timeoutMs` bounds the whole request, headers and body.
 * - `signal` (optional) is the click's `AbortSignal`: aborting it cancels the
 *   request (design D8.5).
 *
 * @param {string} url
 * @param {number} maxBytes
 * @param {number} timeoutMs
 * @param {AbortSignal | undefined} signal
 * @returns {Promise<string>} JSON string: `{ "mimeType": string, "content": string }`
 * @throws {Error} On a refused URL, network failure, non-ok status, size over
 *   the cap, timeout or abort
 */
export async function jsFetchUrlHardened(url, maxBytes, timeoutMs, signal) {
  assertFetchableUrl(url);

  const controller = new AbortController();
  const abort = (reason) => controller.abort(reason);
  let timedOut = false;
  const timer = setTimeout(() => {
    timedOut = true;
    abort(new Error(`${url}: timed out after ${timeoutMs} ms`));
  }, timeoutMs);
  const onCallerAbort = () =>
    abort(signal.reason ?? new Error(`${url}: fetch aborted`));
  if (signal) {
    if (signal.aborted) {
      clearTimeout(timer);
      throw signal.reason ?? new Error(`${url}: fetch aborted`);
    }
    signal.addEventListener("abort", onCallerAbort, { once: true });
  }

  try {
    const response = await fetch(url, {
      credentials: "omit",
      referrerPolicy: "no-referrer",
      redirect: "follow",
      signal: controller.signal,
    });

    if (!response.ok) {
      throw new Error(
        `HTTP ${response.status} ${response.statusText} for ${url}`
      );
    }
    if (response.url && !/^https:\/\//i.test(response.url)) {
      throw new Error(`${url}: redirected to a non-https URL`);
    }
    const declared = Number(response.headers.get("content-length"));
    if (Number.isFinite(declared) && declared > maxBytes) {
      throw new Error(`${url} is ${declared} bytes; the limit is ${maxBytes}`);
    }

    const mimeType =
      response.headers.get("content-type") || "application/octet-stream";

    const chunks = [];
    let total = 0;
    const reader = response.body.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      total += value.byteLength;
      if (total > maxBytes) {
        controller.abort();
        throw new Error(`${url} is over the ${maxBytes} byte limit`);
      }
      chunks.push(value);
    }

    const bytes = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      bytes.set(chunk, offset);
      offset += chunk.byteLength;
    }
    let binary = "";
    const step = 0x8000;
    for (let i = 0; i < bytes.byteLength; i += step) {
      binary += String.fromCharCode(...bytes.subarray(i, i + step));
    }
    return JSON.stringify({ mimeType, content: btoa(binary) });
  } catch (e) {
    // A timeout surfaces as the abort reason; keep its message readable.
    if (timedOut) throw new Error(`${url}: timed out after ${timeoutMs} ms`);
    throw e;
  } finally {
    clearTimeout(timer);
    if (signal) signal.removeEventListener("abort", onCallerAbort);
  }
}
