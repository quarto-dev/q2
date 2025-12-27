/**
 * Post-processor for iframe content after render.
 *
 * This module handles browser-specific transformations:
 * - Replaces /.quarto/ resource links with data URIs from VFS
 * - Converts .qmd links to click handlers for internal navigation
 */

import { vfsReadFile } from '../services/wasmRenderer';

export interface PostProcessOptions {
  /** Current file path for resolving relative links */
  currentFilePath: string;
  /** Callback when user clicks a .qmd link */
  onQmdLinkClick?: (targetPath: string) => void;
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
        // btoa is safe here since CSS is UTF-8 text
        const dataUri = `data:text/css;base64,${btoa(result.content)}`;
        link.setAttribute('href', dataUri);
      }
    }
  });

  // Replace image sources with data URIs
  doc.querySelectorAll('img').forEach((img) => {
    const src = img.getAttribute('src');
    if (src?.startsWith('/.quarto/')) {
      const result = vfsReadFile(src);
      if (result.success && result.content) {
        const mimeType = guessMimeType(src);
        // For binary files, content should already be base64 encoded
        const dataUri = `data:${mimeType};base64,${result.content}`;
        img.setAttribute('src', dataUri);
      }
    }
  });

  // Convert .qmd links to click handlers
  if (options.onQmdLinkClick) {
    doc.querySelectorAll('a[href$=".qmd"]').forEach((anchor) => {
      const href = anchor.getAttribute('href');
      if (href) {
        const targetPath = resolveRelativePath(options.currentFilePath, href);
        anchor.addEventListener('click', (e) => {
          e.preventDefault();
          options.onQmdLinkClick!(targetPath);
        });
        // Visual hint that it's an internal link
        anchor.setAttribute('data-internal-link', 'true');
      }
    });
  }
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
