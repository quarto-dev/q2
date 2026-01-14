/**
 * SHA-256 hashing utilities using Web Crypto API.
 */

/**
 * Compute SHA-256 hash of binary data.
 * Returns hex-encoded string.
 */
export async function computeSHA256(data: ArrayBuffer | Uint8Array): Promise<string> {
  // Convert Uint8Array to ArrayBuffer if needed
  let buffer: ArrayBuffer;
  if (data instanceof Uint8Array) {
    // Create a new ArrayBuffer from the Uint8Array to avoid SharedArrayBuffer issues
    buffer = new Uint8Array(data).buffer;
  } else {
    buffer = data;
  }
  const hashBuffer = await crypto.subtle.digest('SHA-256', buffer);
  const hashArray = Array.from(new Uint8Array(hashBuffer));
  return hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
}
