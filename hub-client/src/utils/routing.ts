/**
 * URL routing utilities for hub-client deep linking.
 *
 * URL Scheme:
 *   #/                                    → Project selector
 *   #/project/<local-id>                  → Project with default file
 *   #/project/<local-id>/file/<path>      → Specific file
 *   #/project/<local-id>/file/<path>#<a>  → Specific file + anchor
 *   #/share/<indexDocId>?server=<url>&file=<path>  → Shareable link (temporary)
 *
 * Security: We use the local IndexedDB project ID (a UUID) instead of
 * the indexDocId (Automerge DocumentId). The indexDocId acts like a bearer
 * token and should never appear in URLs, browser history, or logs.
 *
 * The local ID is only meaningful on the same browser/device, which means
 * URLs are not shareable across devices. This is intentional - sharing a
 * project requires an explicit "share" flow that generates a temporary
 * shareable URL containing the indexDocId. When such a URL is visited,
 * it should be immediately replaced with a local URL to prevent the
 * sensitive indexDocId from appearing in browser history or bookmarks.
 */

/** Default sync server URL used when not specified in shareable URLs */
export const DEFAULT_SYNC_SERVER = 'wss://sync.automerge.org';

// ============================================================================
// Types
// ============================================================================

/**
 * Route to the project selector (home screen).
 */
export interface ProjectSelectorRoute {
  type: 'project-selector';
}

/**
 * Route to a project with default file selection.
 */
export interface ProjectRoute {
  type: 'project';
  projectId: string;
}

/**
 * Route to a specific file within a project.
 */
export interface FileRoute {
  type: 'file';
  projectId: string;
  filePath: string;
  anchor?: string;
}

/**
 * Route from a shareable link containing an Automerge document ID.
 *
 * SECURITY: This route type should only exist transiently during URL resolution.
 * The URL should be immediately replaced with a local URL (project-selector,
 * project, or file) to prevent the sensitive indexDocId from appearing in
 * browser history or bookmarks.
 *
 * The indexDocId is stored WITHOUT the 'automerge:' prefix for URL brevity.
 * It should be normalized (prefix added) before use with Automerge APIs.
 */
export interface ShareRoute {
  type: 'share';
  /** bs58-encoded Automerge document ID (without 'automerge:' prefix) */
  indexDocId: string;
  /** Sync server URL (defaults to DEFAULT_SYNC_SERVER if not in URL) */
  syncServer: string;
  /** Optional file path to open after connecting */
  filePath?: string;
}

/**
 * Union of all possible routes.
 */
export type Route = ProjectSelectorRoute | ProjectRoute | FileRoute | ShareRoute;

// ============================================================================
// URL Parsing
// ============================================================================

/**
 * Parse a hash fragment into a Route object.
 *
 * @param hash - The hash fragment from location.hash (including the leading #)
 * @returns The parsed route
 *
 * @example
 * parseHashRoute('')                                    // { type: 'project-selector' }
 * parseHashRoute('#/')                                  // { type: 'project-selector' }
 * parseHashRoute('#/project/abc-123')                   // { type: 'project', projectId: 'abc-123' }
 * parseHashRoute('#/project/abc-123/file/index.qmd')    // { type: 'file', projectId: 'abc-123', filePath: 'index.qmd' }
 * parseHashRoute('#/project/abc-123/file/docs%2Fintro.qmd#section')
 *   // { type: 'file', projectId: 'abc-123', filePath: 'docs/intro.qmd', anchor: 'section' }
 */
export function parseHashRoute(hash: string): Route {
  // Default to project selector for empty or root hash
  if (!hash || hash === '#' || hash === '#/') {
    return { type: 'project-selector' };
  }

  // Remove leading # if present
  let path = hash.startsWith('#') ? hash.slice(1) : hash;

  // Extract query parameters (for share URLs)
  let queryParams = new URLSearchParams();
  const queryIndex = path.indexOf('?');
  if (queryIndex !== -1) {
    queryParams = new URLSearchParams(path.slice(queryIndex + 1));
    path = path.slice(0, queryIndex);
  }

  // Extract anchor (everything after the last # in the path portion)
  // Note: The anchor is after the hash fragment marker in the URL
  let anchor: string | undefined;
  const anchorIndex = path.indexOf('#');
  if (anchorIndex !== -1) {
    anchor = path.slice(anchorIndex + 1);
    path = path.slice(0, anchorIndex);
  }

  // Remove leading slash
  if (path.startsWith('/')) {
    path = path.slice(1);
  }

  // Split into segments
  const segments = path.split('/');

  // Parse share route: /share/<indexDocId>
  if (segments[0] === 'share' && segments[1]) {
    const indexDocId = decodeURIComponent(segments[1]);
    const syncServer = queryParams.get('server') || DEFAULT_SYNC_SERVER;
    const fileParam = queryParams.get('file');
    const filePath = fileParam ? decodeURIComponent(fileParam) : undefined;

    return {
      type: 'share',
      indexDocId,
      syncServer,
      ...(filePath && { filePath }),
    };
  }

  // Parse route based on segments
  if (segments[0] === 'project' && segments[1]) {
    const projectId = segments[1];

    // Check for file path: /project/<id>/file/<path>
    if (segments[2] === 'file' && segments.length > 3) {
      // Join remaining segments and decode the path
      const encodedPath = segments.slice(3).join('/');
      const filePath = decodeURIComponent(encodedPath);

      // If file path is empty after decoding, treat as project route
      if (!filePath) {
        return { type: 'project', projectId };
      }

      return {
        type: 'file',
        projectId,
        filePath,
        ...(anchor && { anchor }),
      };
    }

    // Just project, no file
    return { type: 'project', projectId };
  }

  // Unknown route format, default to project selector
  return { type: 'project-selector' };
}

// ============================================================================
// URL Building
// ============================================================================

/**
 * Build a hash fragment from a Route object.
 *
 * @param route - The route to encode
 * @returns The hash fragment (including leading #)
 *
 * @example
 * buildHashRoute({ type: 'project-selector' })
 *   // '#/'
 * buildHashRoute({ type: 'project', projectId: 'abc-123' })
 *   // '#/project/abc-123'
 * buildHashRoute({ type: 'file', projectId: 'abc-123', filePath: 'index.qmd' })
 *   // '#/project/abc-123/file/index.qmd'
 * buildHashRoute({ type: 'file', projectId: 'abc-123', filePath: 'docs/intro.qmd', anchor: 'section' })
 *   // '#/project/abc-123/file/docs%2Fintro.qmd#section'
 */
export function buildHashRoute(route: Route): string {
  switch (route.type) {
    case 'project-selector':
      return '#/';

    case 'project':
      return `#/project/${route.projectId}`;

    case 'file': {
      // Encode the file path to handle slashes and special characters
      const encodedPath = encodeURIComponent(route.filePath);
      const base = `#/project/${route.projectId}/file/${encodedPath}`;
      return route.anchor ? `${base}#${route.anchor}` : base;
    }

    case 'share': {
      // Build shareable URL with query parameters
      const params = new URLSearchParams();
      params.set('server', route.syncServer);
      if (route.filePath) {
        params.set('file', route.filePath);
      }
      return `#/share/${encodeURIComponent(route.indexDocId)}?${params.toString()}`;
    }
  }
}

// ============================================================================
// Navigation Helpers
// ============================================================================

/**
 * Build a full URL for opening in a new tab.
 *
 * @param route - The route to navigate to
 * @returns Full URL including origin and pathname
 */
export function buildFullUrl(route: Route): string {
  const hash = buildHashRoute(route);
  return `${window.location.origin}${window.location.pathname}${hash}`;
}

/**
 * Build a shareable URL for a project.
 *
 * This URL contains the Automerge indexDocId and should be treated as sensitive.
 * The recipient can use this URL to connect to the project. When they visit it,
 * the app should immediately replace the URL with a local URL to prevent the
 * sensitive data from appearing in browser history.
 *
 * @param indexDocId - The Automerge document ID (without 'automerge:' prefix)
 * @param syncServer - The sync server URL
 * @param filePath - Optional file path to open after connecting
 * @returns Full shareable URL
 *
 * @example
 * buildShareableUrl('4XyZabc123', 'wss://sync.automerge.org', 'docs/intro.qmd')
 *   // 'https://example.com/hub/#/share/4XyZabc123?server=wss%3A%2F%2Fsync.automerge.org&file=docs%2Fintro.qmd'
 */
export function buildShareableUrl(
  indexDocId: string,
  syncServer: string,
  filePath?: string
): string {
  // Remove 'automerge:' prefix if present (we store it without prefix in URLs)
  const cleanIndexDocId = indexDocId.replace(/^automerge:/, '');

  const route: ShareRoute = {
    type: 'share',
    indexDocId: cleanIndexDocId,
    syncServer,
    ...(filePath && { filePath }),
  };

  return buildFullUrl(route);
}

/**
 * Update the browser URL without triggering navigation.
 *
 * @param route - The route to set
 * @param options - Navigation options
 * @param options.replace - If true, use replaceState (no history entry).
 *                          If false, use pushState (adds history entry).
 */
export function updateUrl(route: Route, options: { replace?: boolean } = {}): void {
  const hash = buildHashRoute(route);
  const url = `${window.location.pathname}${window.location.search}${hash}`;

  if (options.replace) {
    window.history.replaceState({ route }, '', url);
  } else {
    window.history.pushState({ route }, '', url);
  }
}

/**
 * Get the current route from the browser's location.
 */
export function getCurrentRoute(): Route {
  return parseHashRoute(window.location.hash);
}

// ============================================================================
// Route Comparison
// ============================================================================

/**
 * Check if two routes are equivalent.
 *
 * @param a - First route
 * @param b - Second route
 * @returns True if routes point to the same location
 */
export function routesEqual(a: Route, b: Route): boolean {
  if (a.type !== b.type) {
    return false;
  }

  switch (a.type) {
    case 'project-selector':
      return true;

    case 'project':
      return a.projectId === (b as ProjectRoute).projectId;

    case 'file': {
      const bFile = b as FileRoute;
      return (
        a.projectId === bFile.projectId &&
        a.filePath === bFile.filePath &&
        a.anchor === bFile.anchor
      );
    }

    case 'share': {
      const bShare = b as ShareRoute;
      return (
        a.indexDocId === bShare.indexDocId &&
        a.syncServer === bShare.syncServer &&
        a.filePath === bShare.filePath
      );
    }
  }
}

/**
 * Check if two routes point to the same file (ignoring anchor).
 *
 * @param a - First route
 * @param b - Second route
 * @returns True if routes are both file routes to the same file
 */
export function sameFile(a: Route, b: Route): boolean {
  if (a.type !== 'file' || b.type !== 'file') {
    return false;
  }
  return a.projectId === b.projectId && a.filePath === b.filePath;
}
