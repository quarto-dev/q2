/**
 * Post-processor for iframe content after render.
 *
 * This module handles browser-specific transformations:
 * - Replaces /.quarto/ resource links with data URIs from VFS
 * - Converts .qmd links to click handlers for internal navigation
 * - Reverse-maps website-rewritten artifact-rooted .html links
 *   back to their source-side .qmd path so cross-doc clicks
 *   switch the active editor file (bd-lnd3).
 */

import { vfsReadFile, vfsReadBinaryFile } from '@quarto/preview-runtime';
import { resolveRelativePath, guessMimeType } from './vfsPaths';

/**
 * VFS path under which the website renderer flushes its
 * project-scoped artifacts. Cross-doc links emitted by
 * `quarto-core`'s navigation / body-link transforms are rooted
 * here in their output-side `.html` form (e.g.
 * `/.quarto/project-artifacts/about.html`). Hub-client's click
 * handler reverse-maps these to the source-side `.qmd` file.
 *
 * bd-msp0: this constant is duplicated in
 * `crates/wasm-quarto-hub-client/src/lib.rs` (`RenderToHtmlRenderer::new`
 * argument). When the service-worker resource-resolution work
 * lands, hoist this into a single shared constant exposed across
 * the WASM bridge.
 */
const ARTIFACT_ROOT = '/.quarto/project-artifacts/';

/**
 * Source-file extensions that the website renderer rewrites to
 * `.html` in its output. When reverse-mapping a clicked
 * artifact-rooted `.html` URL, we try each of these extensions
 * in order and intercept iff the resulting project path matches
 * a real `FileEntry`.
 *
 * Today only `.qmd` is renderable; `.md` / `.ipynb` are reserved
 * for when Q2 supports them.
 */
const RENDERABLE_EXTS: readonly string[] = ['.qmd'];

export interface PostProcessOptions {
  /** Current file path for resolving relative links */
  currentFilePath: string;
  /**
   * Project file paths (no leading slash). Used to reverse-map
   * artifact-rooted `.html` URLs to their source-side `.qmd`
   * (or future `.md` / `.ipynb`) so cross-document clicks
   * switch the active editor file. The lookup is **strict**:
   * we only intercept artifact-rooted `.html` clicks if the
   * reverse-mapped path matches a known project file. Other
   * artifact-rooted `.html` links (e.g. a future listing page
   * with no `.qmd` source) are left alone.
   *
   * bd-lnd3.
   */
  projectFilePaths?: readonly string[];
  /**
  * Callback when user clicks a .qmd link or anchor link.
  * - targetPath - The resolved path to the target file
  * - anchor - The anchor/fragment identifier (without #)
  */
  onQmdLinkClick?: (arg: { path: string, anchor: string | null } | { anchor: string }) => void;
}

/**
 * Reverse-map an artifact-rooted `.html` URL — or the bare artifact
 * root (directory URL) — to a source-side project path, or return
 * `null` if the URL doesn't look like a cross-doc website link or
 * doesn't correspond to a known project file.
 *
 * Examples (with `projectFilePaths = ['index.qmd', 'about.qmd', 'posts/first.qmd']`):
 *
 *   /.quarto/project-artifacts/about.html         → { path: 'about.qmd',         anchor: null  }
 *   /.quarto/project-artifacts/about.html#intro   → { path: 'about.qmd',         anchor: 'intro' }
 *   /.quarto/project-artifacts/posts/first.html   → { path: 'posts/first.qmd',   anchor: null  }
 *   /.quarto/project-artifacts/                   → { path: 'index.qmd',         anchor: null  }
 *   /.quarto/project-artifacts/#intro             → { path: 'index.qmd',         anchor: 'intro' }
 *   /.quarto/project-artifacts/notes.html         → null  (no notes.qmd in project)
 *   /.quarto/project-artifacts/styles.css         → null  (not .html)
 *   ./about.qmd                                   → null  (not artifact-rooted)
 *
 * The bare-root case (bd-ql55q) mirrors the browser's static-server
 * "directory URL = serve index.html" convention — the navbar brand
 * (and other site-root surfaces) fall back to
 * `page_url_for_site_root_dir()`, which in VFS-root mode emits
 * `/.quarto/project-artifacts/`. We map that to the project's
 * `index` source file if it exists in `projectFilePaths`; otherwise
 * return null per this surface's strict policy.
 *
 * Exported for unit testing.
 */
export function reverseMapArtifactHref(
  href: string,
  projectFilePaths: readonly string[],
): { path: string; anchor: string | null } | null {
  if (!href.startsWith(ARTIFACT_ROOT)) return null;
  const stripped = href.slice(ARTIFACT_ROOT.length);
  const { path: stem, anchor } = parseLink(stripped);
  // Bare artifact root (with or without anchor): map to the
  // project's `index.<ext>` source file if present. Strict policy
  // preserved — return null when no such file exists. `stem === null`
  // happens when `stripped` starts with `#`; `stem === ''` when
  // `stripped` is empty. Both shapes are the bare-root case.
  if (stem === null || stem === '') {
    for (const ext of RENDERABLE_EXTS) {
      const candidate = 'index' + ext;
      if (projectFilePaths.includes(candidate)) {
        return { path: candidate, anchor };
      }
    }
    return null;
  }
  if (!stem.endsWith('.html')) return null;
  const base = stem.slice(0, -'.html'.length);
  for (const ext of RENDERABLE_EXTS) {
    const candidate = base + ext;
    if (projectFilePaths.includes(candidate)) {
      return { path: candidate, anchor };
    }
  }
  return null;
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
 * Read an embeddable iframe resource from the VFS for inlining as
 * `srcdoc`, mapping a root-relative output URL to its VFS **source**
 * path. (bd-kjrpya2d)
 *
 * `q2 render` resolves the deck `src` against `_site/` on disk; in
 * `q2 preview` the page renders in-browser with no server, so we read
 * the bytes from the VFS instead. A static `resources:` asset (an
 * embedded `.embed-example-iframe` deck) lives at its VFS **source**
 * path — nothing copies it under the artifact root in the WASM render —
 * so after trying the literal `src` we retry at the source path. Two
 * src shapes occur in practice:
 *
 *   - **page/site-relative `/X`** — the embed feature's chosen output
 *     form (bd-z1smhvuo commit 867aa7c1 "page-relative iframe src",
 *     e.g. `/examples/presentations/03-fragments/slides.html`). Strip
 *     the leading `/` and read the source path `X`.
 *   - **artifact-rooted `/.quarto/project-artifacts/X`** — the
 *     cross-doc href form `page_url_for` emits in VFS-root mode. Strip
 *     `ARTIFACT_ROOT` and read `X`.
 *
 * Returns the first successful read, or `{ success: false }` so the
 * caller leaves the iframe untouched — external URLs and any path not
 * in the VFS genuinely need a network load and must not be clobbered.
 */
function readArtifactOrSource(src: string): ReturnType<typeof vfsReadFile> {
  const direct = vfsReadFile(src);
  if (direct.success && direct.content) return direct;

  // Map the output URL to its VFS source key. ARTIFACT_ROOT is a more
  // specific prefix than a bare `/`, so test it first.
  let sourcePath: string | null = null;
  if (src.startsWith(ARTIFACT_ROOT)) {
    sourcePath = src.slice(ARTIFACT_ROOT.length);
  } else if (src.startsWith('/')) {
    sourcePath = src.slice(1);
  }

  if (sourcePath) {
    const atSource = vfsReadFile(sourcePath);
    if (atSource.success && atSource.content) return atSource;
  }
  return { success: false };
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

  // Replace CSS links with data URIs (both /.quarto/ and libs/ paths)
  doc.querySelectorAll('link[rel="stylesheet"]').forEach((link) => {
    const href = link.getAttribute('href');
    if (href && (href.startsWith('/.quarto/') || href.startsWith('libs/'))) {
      const result = vfsReadFile(href);
      if (result.success && result.content) {
        // Use UTF-8 safe base64 encoding (btoa only handles Latin1)
        const dataUri = `data:text/css;base64,${utf8ToBase64(result.content)}`;
        link.setAttribute('href', dataUri);
      }
    }
  });

  // DISABLED: Script inlining in the preview iframe is disabled until we
  // determine a safe way to allow script execution in the sandboxed iframe.
  // The iframe sandbox does not include allow-scripts, so even if scripts
  // were inlined they would not execute. Extension JS (kbd.js, video.min.js)
  // works in native renders where the browser loads <script src="..."> normally.
  //
  // To re-enable, uncomment the block below AND add allow-scripts to the
  // sandbox attribute in DoubleBufferedIframe.tsx and MorphIframe.tsx.
  //
  // let didInlineScripts = false;
  // doc.querySelectorAll('script[src]').forEach((script) => {
  //   const src = script.getAttribute('src');
  //   if (src && (src.startsWith('/.quarto/') || src.startsWith('libs/'))) {
  //     const result = vfsReadFile(src);
  //     if (result.success && result.content) {
  //       const inline = doc.createElement('script');
  //       inline.textContent = result.content;
  //       script.parentNode?.appendChild(inline);
  //       script.remove();
  //       didInlineScripts = true;
  //     }
  //   }
  // });
  //
  // if (didInlineScripts) {
  //   doc.dispatchEvent(new Event('DOMContentLoaded', { bubbles: true }));
  // }

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

  // Inline embedded-resource <iframe> sources (e.g. `.embed-example-iframe`
  // decks) from the VFS via `srcdoc`, so the sandboxed preview never issues
  // a network request for them (there is no server to answer it — the page
  // is rendered in-browser). The embed feature emits a page/site-relative
  // src (`/examples/.../slides.html`; bd-z1smhvuo); `readArtifactOrSource`
  // maps any root-relative src to its VFS source path. We process every
  // root-relative src and inline ONLY on a successful VFS read, so external
  // (`http(s)://`, protocol-relative `//`) and not-in-VFS iframes that
  // genuinely need a network load are left untouched. (bd-kjrpya2d)
  doc.querySelectorAll('iframe').forEach((frame) => {
    const src = frame.getAttribute('src');
    if (!src || !src.startsWith('/') || src.startsWith('//')) return;
    const result = readArtifactOrSource(src);
    if (result.success && result.content) {
      frame.removeAttribute('src');
      frame.setAttribute('srcdoc', result.content);
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

    // Handle website cross-doc links: the `navigation_href.rs` /
    // body-link transforms rewrite `[A](about.qmd)` into
    // `<a href="/.quarto/project-artifacts/about.html">`. We
    // reverse-map back to the source `.qmd` so the editor
    // switches files (bd-lnd3). Strict: only intercept if the
    // reverse-mapped path matches a known project file.
    const filePaths = options.projectFilePaths;
    if (filePaths && filePaths.length > 0) {
      doc.querySelectorAll(`a[href^="${ARTIFACT_ROOT}"]`).forEach((anchor) => {
        const href = anchor.getAttribute('href');
        if (!href) return;
        const mapped = reverseMapArtifactHref(href, filePaths);
        if (!mapped) return;
        anchor.addEventListener('click', (e) => {
          e.preventDefault();
          options.onQmdLinkClick!({ path: mapped.path, anchor: mapped.anchor });
        });
        anchor.setAttribute('data-internal-link', 'true');
      });
    }
  }

  // Intercept Ctrl+S / Cmd+S in iframe and notify parent
  doc.addEventListener('keydown', (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault();
      window.parent.postMessage({ type: 'hub-client-save' }, '*');
    }
  });

  // Inject responsive CSS for hub-client preview
  // Hides TOC and adjusts layout for narrow containers since media queries
  // check viewport width (not iframe/container width)
  injectPreviewStyles(doc);
}

/**
 * Inject CSS to make the preview more responsive in narrow containers.
 * This is needed because Quarto's media queries check viewport width,
 * not the iframe container width.
 */
function injectPreviewStyles(doc: Document): void {
  const style = doc.createElement('style');
  style.setAttribute('data-hub-client', 'true');
  style.textContent = `
    /* Hub-client preview overrides */
    /* Hide the page-level TOC — it doesn't work well in narrow
       iframe containers. Quarto's website sidebar shares the
       \`role="doc-toc"\` ARIA role (it's the document's
       table-of-contents navigation), so we exclude
       \`.sidebar-navigation\` to avoid collateral-killing the
       sidebar (bd-f5yi). */
    nav[role="doc-toc"]:not(.sidebar-navigation) {
      display: none !important;
    }

    /* Ensure body content doesn't overflow */
    body {
      overflow-x: hidden;
    }

    /* Constrain page columns to container width */
    .page-columns {
      max-width: 100%;
    }

    /* Ensure main content doesn't overflow */
    main {
      max-width: 100%;
      overflow-x: auto;
    }
  `;
  doc.head.appendChild(style);
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
