/**
 * Resource Service
 *
 * Utilities for handling binary files (images, PDFs, etc.) in the hub-client.
 * Provides SHA-256 hashing, MIME type detection, and conflict-aware naming.
 */

import { inferMimeType } from '../types/project';

/**
 * Compute SHA-256 hash of binary data using Web Crypto API.
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

/**
 * Get the first 8 characters of a hash for filename suffixes.
 */
export function getHashPrefix(hash: string): string {
  return hash.slice(0, 8);
}

/**
 * Generate a unique filename by appending hash prefix.
 * Example: "diagram.png" -> "diagram-a1b2c3d4.png"
 */
export function generateHashedFilename(originalPath: string, hash: string): string {
  const lastSlash = originalPath.lastIndexOf('/');
  const dir = lastSlash >= 0 ? originalPath.slice(0, lastSlash + 1) : '';
  const filename = lastSlash >= 0 ? originalPath.slice(lastSlash + 1) : originalPath;

  const lastDot = filename.lastIndexOf('.');
  if (lastDot <= 0) {
    // No extension or hidden file
    return `${dir}${filename}-${getHashPrefix(hash)}`;
  }

  const name = filename.slice(0, lastDot);
  const ext = filename.slice(lastDot);
  return `${dir}${name}-${getHashPrefix(hash)}${ext}`;
}

/**
 * Read a File object as ArrayBuffer
 */
export async function readFileAsArrayBuffer(file: File): Promise<ArrayBuffer> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as ArrayBuffer);
    reader.onerror = () => reject(new Error('Failed to read file'));
    reader.readAsArrayBuffer(file);
  });
}

/**
 * Process a file for upload: read content, compute hash, detect MIME type
 */
export async function processFileForUpload(file: File): Promise<{
  content: Uint8Array;
  mimeType: string;
  hash: string;
  originalName: string;
}> {
  const arrayBuffer = await readFileAsArrayBuffer(file);
  const content = new Uint8Array(arrayBuffer);
  const hash = await computeSHA256(content);

  // Use browser-provided MIME type, fall back to extension-based detection
  const mimeType = file.type || inferMimeType(file.name);

  return {
    content,
    mimeType,
    hash,
    originalName: file.name,
  };
}

/**
 * Convert binary content to a data URL for display.
 * Used for rendering images in the preview.
 */
export function binaryToDataUrl(content: Uint8Array, mimeType: string): string {
  // Convert Uint8Array to binary string
  let binaryStr = '';
  const len = content.length;
  for (let i = 0; i < len; i++) {
    binaryStr += String.fromCharCode(content[i]);
  }

  // Encode as base64
  const base64 = btoa(binaryStr);
  return `data:${mimeType};base64,${base64}`;
}

/**
 * Unicode whitespace pattern: matches all Unicode whitespace characters.
 * Includes ASCII whitespace (\s), non-breaking space, em/en/thin/hair spaces,
 * line/paragraph separators, narrow no-break space, mathematical space,
 * ideographic space, and zero-width no-break space (BOM).
 */
const UNICODE_WHITESPACE = /[\s\u00A0\u2000-\u200B\u2028\u2029\u202F\u205F\u3000\uFEFF]+/g;

/**
 * Sanitize a filename for use in markdown references.
 *
 * - Replaces all Unicode whitespace with hyphens
 * - Replaces interior dots (all except the last, which is the extension separator) with hyphens
 * - Collapses consecutive hyphens into a single hyphen
 * - Trims leading/trailing hyphens (preserving leading dots for dotfiles)
 */
export function sanitizeFilename(name: string): string {
  // Step 1: Strip leading/trailing whitespace (before any replacement,
  // so we can detect dotfiles like "  .file.png" → ".file.png")
  let result = name.replace(/^[\s\u00A0\u2000-\u200B\u2028\u2029\u202F\u205F\u3000\uFEFF]+/, '')
    .replace(/[\s\u00A0\u2000-\u200B\u2028\u2029\u202F\u205F\u3000\uFEFF]+$/, '');

  // Step 2: Replace interior whitespace with hyphens
  result = result.replace(UNICODE_WHITESPACE, '-');

  // Step 3: Replace interior dots with hyphens.
  // Find the last dot — that's the extension separator. Replace all earlier dots.
  // For dotfiles (e.g., ".gitignore"), lastDot is 0, so no replacement happens.
  // For dotfiles with extensions (e.g., ".file.png"), preserve the leading dot.
  const lastDot = result.lastIndexOf('.');
  if (lastDot > 0) {
    const stem = result.slice(0, lastDot);
    const ext = result.slice(lastDot);
    // Preserve leading dot for dotfiles: only replace dots after position 0
    const sanitizedStem = stem.startsWith('.')
      ? '.' + stem.slice(1).replace(/\./g, '-')
      : stem.replace(/\./g, '-');
    result = sanitizedStem + ext;
  }

  // Step 4: Collapse consecutive hyphens
  result = result.replace(/-{2,}/g, '-');

  // Step 5: Trim leading/trailing hyphens, but preserve leading dot for dotfiles
  if (result.startsWith('.')) {
    const afterDot = result.slice(1).replace(/^-+/, '').replace(/-+$/, '');
    result = '.' + afterDot;
  } else {
    result = result.replace(/^-+/, '').replace(/-+$/, '');
  }

  return result;
}

/**
 * Size limits for binary files
 */
export const FILE_SIZE_LIMITS = {
  /** Maximum size for a single file (10 MB) */
  MAX_FILE_SIZE: 10 * 1024 * 1024,
  /** Maximum total project resources (100 MB) */
  MAX_PROJECT_RESOURCES: 100 * 1024 * 1024,
} as const;

/**
 * Validate file size against limits
 */
export function validateFileSize(size: number): { valid: boolean; error?: string } {
  if (size > FILE_SIZE_LIMITS.MAX_FILE_SIZE) {
    const maxMB = FILE_SIZE_LIMITS.MAX_FILE_SIZE / (1024 * 1024);
    const sizeMB = (size / (1024 * 1024)).toFixed(2);
    return {
      valid: false,
      error: `File size (${sizeMB} MB) exceeds maximum allowed (${maxMB} MB)`,
    };
  }
  return { valid: true };
}
