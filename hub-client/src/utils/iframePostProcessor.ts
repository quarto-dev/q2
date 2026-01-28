/**
 * Post-processor for iframe content after render.
 *
 * This module handles browser-specific transformations:
 * - Replaces /.quarto/ resource links with data URIs from VFS
 * - Converts .qmd links to click handlers for internal navigation
 */

import { vfsReadFile, vfsReadBinaryFile } from '../services/wasmRenderer';

export interface PostProcessOptions {
  /** Current file path for resolving relative links */
  currentFilePath: string;
  /**
  * Callback when user clicks a .qmd link or anchor link.
  * - targetPath - The resolved path to the target file
  * - anchor - The anchor/fragment identifier (without #)
  */
  onQmdLinkClick?: (arg: { path: string, anchor: string | null } | { anchor: string }) => void;
}

/** Parsed components of a link href */
interface ParsedLink {
  path: string | null; // null for same-document anchors
  anchor: string | null; // null if no anchor
}

/**
 * Parse a link href into path and anchor components.
 * Examples:
 *   "file.qmd" -> { path: "file.qmd", anchor: null }
 *   "file.qmd#section" -> { path: "file.qmd", anchor: "section" }
 *   "#section" -> { path: null, anchor: "section" }
 */
function parseLink(href: string): ParsedLink {
  const hashIndex = href.indexOf('#');
  if (hashIndex === -1) {
    return { path: href, anchor: null };
  }
  const path = hashIndex === 0 ? null : href.substring(0, hashIndex);
  const anchor = href.substring(hashIndex + 1);
  return { path, anchor: anchor || null };
}

/**
 * Post-process iframe content after render.
 * - Replaces /.quarto/ resource links with data URIs
 * - Converts .qmd links to click handlers
 */
export function postProcessIframe(
  iframe: HTMLIFrameElement,
  options: PostProcessOptions
): void {
  const doc = iframe.contentDocument;
  if (!doc) return;

  // Replace CSS links with data URIs
  doc.querySelectorAll('link[rel="stylesheet"]').forEach((link) => {
    const href = link.getAttribute('href');
    if (href?.startsWith('/.quarto/')) {
      const result = vfsReadFile(href);
      if (result.success && result.content) {
        // Use UTF-8 safe base64 encoding (btoa only handles Latin1)
        const dataUri = `data:text/css;base64,${utf8ToBase64(result.content)}`;
        link.setAttribute('href', dataUri);
      }
    }
  });

  // Replace image sources with data URIs
  doc.querySelectorAll('img').forEach((img) => {
    const src = img.getAttribute('src');
    if (!src) return;

    // Skip external URLs and data URIs
    if (src.startsWith('http://') || src.startsWith('https://') || src.startsWith('data:')) {
      return;
    }

    // Handle /.quarto/ paths (built-in resources)
    if (src.startsWith('/.quarto/')) {
      const result = vfsReadFile(src);
      if (result.success && result.content) {
        const mimeType = guessMimeType(src);
        const dataUri = `data:${mimeType};base64,${result.content}`;
        img.setAttribute('src', dataUri);
      }
      return;
    }

    // Handle project-relative paths (images uploaded to project)
    const resolvedPath = resolveRelativePath(options.currentFilePath, src);
    // Remove leading slash for VFS path (VFS stores as "images/foo.png" not "/images/foo.png")
    const vfsPath = resolvedPath.startsWith('/') ? resolvedPath.slice(1) : resolvedPath;

    const result = vfsReadBinaryFile(vfsPath);
    if (result.success && result.content) {
      const mimeType = guessMimeType(src);
      // vfsReadBinaryFile returns base64-encoded content
      const dataUri = `data:${mimeType};base64,${result.content}`;
      img.setAttribute('src', dataUri);
    }
  });

  // Handle external links - open in new tab
  doc.querySelectorAll('a[href^="http://"], a[href^="https://"]').forEach((anchor) => {
    anchor.setAttribute('target', '_blank');
    anchor.setAttribute('rel', 'noopener noreferrer');
  });

  // Convert .qmd links and anchor links to click handlers
  if (options.onQmdLinkClick) {
    // Handle .qmd links (with or without anchors)
    // Match both "file.qmd" and "file.qmd#section"
    doc.querySelectorAll('a[href*=".qmd"]').forEach((anchor) => {
      const href = anchor.getAttribute('href');
      if (href && !href.startsWith('http://') && !href.startsWith('https://')) {
        const parsed = parseLink(href);
        // Only process if the path ends with .qmd (handles "file.qmd" and "file.qmd#section")
        if (parsed.path && parsed.path.endsWith('.qmd')) {
          const path = resolveRelativePath(options.currentFilePath, parsed.path);
          anchor.addEventListener('click', (e) => {
            e.preventDefault();
            options.onQmdLinkClick!({ path, anchor: parsed.anchor });
          });
          // Visual hint that it's an internal link
          anchor.setAttribute('data-internal-link', 'true');
        }
      }
    });

    // Handle same-document anchor links (#section)
    doc.querySelectorAll('a[href^="#"]').forEach((anchor) => {
      const href = anchor.getAttribute('href');
      if (href) {
        const parsed = parseLink(href);
        anchor.addEventListener('click', (e) => {
          e.preventDefault();
          if (parsed.anchor) {
            options.onQmdLinkClick!({ anchor: parsed.anchor });
          }
        });
      }
    });
  }

  // Intercept Ctrl+S / Cmd+S in iframe and notify parent
  doc.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault();
      window.parent.postMessage({ type: 'hub-client-save' }, '*');
    }
  });
}

/** Resolve a relative path against the current file's directory */
function resolveRelativePath(
  currentFile: string,
  relativePath: string
): string {
  if (relativePath.startsWith('/')) {
    return relativePath; // Already absolute
  }
  // Get directory of current file
  const lastSlash = currentFile.lastIndexOf('/');
  const currentDir =
    lastSlash >= 0 ? currentFile.substring(0, lastSlash + 1) : '/';
  return normalizePath(currentDir + relativePath);
}

function normalizePath(path: string): string {
  const parts = path.split('/').filter((p) => p !== '.');
  const result: string[] = [];
  for (const part of parts) {
    if (part === '..') {
      result.pop();
    } else if (part) {
      result.push(part);
    }
  }
  return '/' + result.join('/');
}

function guessMimeType(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase();
  const mimeTypes: Record<string, string> = {
    png: 'image/png',
    jpg: 'image/jpeg',
    jpeg: 'image/jpeg',
    gif: 'image/gif',
    svg: 'image/svg+xml',
    webp: 'image/webp',
    css: 'text/css',
    js: 'text/javascript',
  };
  return mimeTypes[ext || ''] || 'application/octet-stream';
}

/**
 * Encode a UTF-8 string to base64.
 *
 * Unlike btoa(), this handles characters outside the Latin1 range
 * by first encoding to UTF-8 bytes.
 */
function utf8ToBase64(str: string): string {
  // Encode string to UTF-8 bytes
  const bytes = new TextEncoder().encode(str);
  // Convert bytes to binary string
  let binary = '';
  for (let i = 0; i < bytes.length; i++) {
    binary += String.fromCharCode(bytes[i]);
  }
  // Encode binary string to base64
  return btoa(binary);
}
