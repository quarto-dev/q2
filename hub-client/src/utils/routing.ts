/**
 * URL routing utilities for hub-client deep linking.
 *
 * URL Scheme:
 *   #/                                    → Project selector
 *   #/p/<local-id>                        → Project with default file
 *   #/p/<local-id>/file/<path>            → Specific file
 *   #/p/<local-id>/file/<path>#<a>        → Specific file + anchor
 *   #/share/<indexDocId>?server=<url>&file=<path>  → Shareable link (temporary)
 *   #/link-project-set/<docId>?server=<url>        → Link project set (temporary)
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

/** * Prefix a path with the hub's mount base path, if any. */
export function hubPath(path: string): string {
  return `${import.meta.env.VITE_HUB_BASE_PATH ?? ''}${path}`;
}

/**
 * Default sync server URL used when not specified in shareable URLs.
 *
 * An explicit `VITE_DEFAULT_SYNC_SERVER` takes precedence, followed by a
 * subpath-aware `<base>/ws`, and finally the public automerge.org sync server.
 */
export const DEFAULT_SYNC_SERVER =
  import.meta.env.VITE_DEFAULT_SYNC_SERVER ||
  (import.meta.env.VITE_HUB_BASE_PATH ? hubPath('/ws') : 'wss://sync.automerge.org');

/**
 * Resolve a sync server URL to an absolute WebSocket URL.
 *
 * When the hub is mounted under a subpath, `DEFAULT_SYNC_SERVER` is a relative
 * path such as `/subpath/ws` (derived from `VITE_HUB_BASE_PATH`).
 * This function expands relative paths (those starting with `/`) against the
 * current page origin at runtime so they become valid WebSocket URLs. Absolute
 * URLs (starting with `wss://`, `ws://`, etc.) are returned unchanged.
 *
 * @param syncServer - The sync server value from config or a shareable URL.
 * @returns An absolute WebSocket URL ready to pass to the WS adapter.
 */
export function resolveSyncServerUrl(syncServer: string): string {
  if (!syncServer.startsWith('/')) {
    return syncServer;
  }
  const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${proto}//${window.location.host}${syncServer}`;
}

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
  /** Sync server URL */
  syncServer: string;
  /** File path to open after connecting */
  filePath: string;
  /** Human-readable project name */
  name: string;
}

/**
 * Route from a link to join/link a project set from another browser.
 *
 * SECURITY: Like ShareRoute, this should only exist transiently.
 * The URL is immediately replaced with the project-selector route.
 *
 * URL format: #/link-project-set/<projectSetDocId>?server=<url>
 */
export interface LinkProjectSetRoute {
  type: 'link-project-set';
  /** bs58-encoded Automerge document ID (without 'automerge:' prefix) */
  projectSetDocId: string;
  /** Sync server URL */
  syncServer: string;
}

/**
 * Route from a collection invite link (explore/projects-collections-ui exploration).
 *
 * The invite carries the collection identity plus the collection's project entries
 * inline, so joining delivers real, syncable projects; only collection
 * membership itself is mock data until shared collections become synced docs.
 *
 * SECURITY: Like ShareRoute, the entries contain bearer document IDs and
 * the route should only exist transiently.
 *
 * URL format: #/join-collection/<collectionId>?name=<collection>&from=<inviter>&entries=<json>
 */
export interface JoinCollectionRoute {
  type: 'join-collection';
  /** Automerge doc id of the collection's ProjectSetDocument (no prefix). */
  collectionId: string;
  collectionName: string;
  /** Display name of the person who sent the invite. */
  inviter: string;
  /** Sync server hosting the collection document. */
  syncServer: string;
  /**
   * Legacy payload from pre-architecture invites (projects inlined in the
   * URL). Still parsed for backward compatibility; the join flow now
   * subscribes to the collection document instead.
   */
  entries: Array<{ indexDocId: string; syncServer: string; description: string }>;
}

/**
 * Route for dev-only harness pages (component previews, visual testing).
 * Only parsed in development builds; in production, #/dev/... falls through to project-selector.
 */
export interface DevRoute {
  type: 'dev';
  /** The dev page to render (e.g. 'setup-migration', 'setup-fresh') */
  page: string;
}

/**
 * Union of all possible routes.
 */
export type Route = ProjectSelectorRoute | ProjectRoute | FileRoute | ShareRoute | LinkProjectSetRoute | JoinCollectionRoute | DevRoute;

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
 * parseHashRoute('#/p/abc-123')                          // { type: 'project', projectId: 'abc-123' }
 * parseHashRoute('#/p/abc-123/file/index.qmd')           // { type: 'file', projectId: 'abc-123', filePath: 'index.qmd' }
 * parseHashRoute('#/p/abc-123/file/docs%2Fintro.qmd#section')
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

  // Parse share route: /share/<indexDocId>?server=<url>&file=<path>&name=<name>
  // All three query parameters are required. Missing fields are set to empty
  // strings; App.tsx validates and shows an error for malformed share links.
  if (segments[0] === 'share' && segments[1]) {
    const indexDocId = decodeURIComponent(segments[1]);
    const server = queryParams.get('server') ?? '';
    const fileParam = queryParams.get('file') ?? '';
    const nameParam = queryParams.get('name') ?? '';

    return {
      type: 'share',
      indexDocId,
      syncServer: server,
      filePath: fileParam ? decodeURIComponent(fileParam) : '',
      name: nameParam ? decodeURIComponent(nameParam) : '',
    };
  }

  // Parse link-project-set route: /link-project-set/<docId>?server=<url>
  if (segments[0] === 'link-project-set' && segments[1]) {
    const projectSetDocId = decodeURIComponent(segments[1]);
    const server = queryParams.get('server') ?? '';

    return {
      type: 'link-project-set',
      projectSetDocId,
      syncServer: server,
    };
  }

  // Parse join-collection route: /join-collection/<collectionId>?name=<collection>&from=<inviter>&entries=<json>
  if (segments[0] === 'join-collection' && segments[1]) {
    let entries: JoinCollectionRoute['entries'] = [];
    try {
      const raw = JSON.parse(queryParams.get('entries') ?? '[]');
      if (Array.isArray(raw)) {
        entries = raw
          .filter((e) => typeof e?.d === 'string' && typeof e?.s === 'string')
          .map((e) => ({ indexDocId: e.d, syncServer: e.s, description: String(e.n ?? '') }));
      }
    } catch {
      // Malformed entries — join proceeds with an empty collection
    }
    return {
      type: 'join-collection',
      collectionId: decodeURIComponent(segments[1]),
      collectionName: queryParams.get('name') ?? 'Shared collection',
      inviter: queryParams.get('from') ?? 'A collaborator',
      syncServer: queryParams.get('server') ?? '',
      entries,
    };
  }

  // Parse route based on segments
  if (segments[0] === 'p' && segments[1]) {
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

  // Parse dev route: /dev/<page> (only in development builds)
  if (import.meta.env.DEV && segments[0] === 'dev' && segments[1]) {
    return { type: 'dev', page: decodeURIComponent(segments[1]) };
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
 *   // '#/p/abc-123'
 * buildHashRoute({ type: 'file', projectId: 'abc-123', filePath: 'index.qmd' })
 *   // '#/p/abc-123/file/index.qmd'
 * buildHashRoute({ type: 'file', projectId: 'abc-123', filePath: 'docs/intro.qmd', anchor: 'section' })
 *   // '#/p/abc-123/file/docs%2Fintro.qmd#section'
 */
export function buildHashRoute(route: Route): string {
  switch (route.type) {
    case 'project-selector':
      return '#/';

    case 'project':
      return `#/p/${route.projectId}`;

    case 'file': {
      // Encode the file path to handle slashes and special characters
      const encodedPath = encodeURIComponent(route.filePath);
      const base = `#/p/${route.projectId}/file/${encodedPath}`;
      return route.anchor ? `${base}#${route.anchor}` : base;
    }

    case 'share': {
      // Build shareable URL with query parameters
      const params = new URLSearchParams();
      params.set('server', route.syncServer);
      params.set('file', route.filePath);
      params.set('name', route.name);
      return `#/share/${encodeURIComponent(route.indexDocId)}?${params.toString()}`;
    }

    case 'link-project-set': {
      const params = new URLSearchParams();
      params.set('server', route.syncServer);
      return `#/link-project-set/${encodeURIComponent(route.projectSetDocId)}?${params.toString()}`;
    }

    case 'join-collection': {
      const params = new URLSearchParams();
      params.set('name', route.collectionName);
      params.set('from', route.inviter);
      if (route.syncServer) params.set('server', route.syncServer);
      if (route.entries.length > 0) {
        params.set('entries', JSON.stringify(
          route.entries.map((e) => ({ d: e.indexDocId, s: e.syncServer, n: e.description })),
        ));
      }
      return `#/join-collection/${encodeURIComponent(route.collectionId)}?${params.toString()}`;
    }

    case 'dev':
      return `#/dev/${encodeURIComponent(route.page)}`;
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
 * @param projectName - Human-readable project name
 * @param filePath - File path to open after connecting
 * @returns Full shareable URL
 *
 * @example
 * buildShareableUrl('4XyZabc123', 'wss://sync.automerge.org', 'My Project', 'docs/intro.qmd')
 *   // 'https://example.com/hub/#/share/4XyZabc123?server=...&file=docs%2Fintro.qmd&name=My+Project'
 */
export function buildShareableUrl(
  indexDocId: string,
  syncServer: string,
  projectName: string,
  filePath: string
): string {
  // Remove 'automerge:' prefix if present (we store it without prefix in URLs)
  const cleanIndexDocId = indexDocId.replace(/^automerge:/, '');

  const route: ShareRoute = {
    type: 'share',
    indexDocId: cleanIndexDocId,
    syncServer,
    filePath,
    name: projectName,
  };

  return buildFullUrl(route);
}

/**
 * Build a shareable URL for linking a project set on another browser.
 *
 * Like project share URLs, this contains a bearer-token document ID
 * and should be treated as sensitive.
 *
 * @param projectSetDocId - The Automerge document ID (with or without 'automerge:' prefix)
 * @param syncServer - The sync server URL
 * @returns Full shareable URL
 */
export function buildProjectSetLinkUrl(
  projectSetDocId: string,
  syncServer: string,
): string {
  const cleanDocId = projectSetDocId.replace(/^automerge:/, '');

  const route: LinkProjectSetRoute = {
    type: 'link-project-set',
    projectSetDocId: cleanDocId,
    syncServer,
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

    case 'link-project-set': {
      const bLink = b as LinkProjectSetRoute;
      return (
        a.projectSetDocId === bLink.projectSetDocId &&
        a.syncServer === bLink.syncServer
      );
    }

    case 'join-collection':
      return a.collectionId === (b as JoinCollectionRoute).collectionId;

    case 'dev':
      return a.page === (b as DevRoute).page;
  }
}

// ============================================================================
// Pre-Auth Hash Preservation
// ============================================================================

/** sessionStorage key used to preserve the hash across auth redirects. */
const PRE_AUTH_HASH_KEY = 'quarto-hub-pre-auth-hash';

/**
 * Save the current hash fragment to sessionStorage.
 *
 * Call this when the login screen is shown so that the hash (e.g., a
 * `#/share/...` link) survives the Google OAuth redirect roundtrip.
 * The server redirects back to `/` after auth, which loses the hash.
 */
export function savePreAuthHash(): void {
  const hash = window.location.hash;
  if (hash && hash !== '#' && hash !== '#/') {
    sessionStorage.setItem(PRE_AUTH_HASH_KEY, hash);
  }
}

/**
 * Restore a previously saved hash fragment after auth redirect.
 *
 * Call this early at startup (before React mounts). If a hash was saved
 * and the current URL has no meaningful hash, it is restored.
 *
 * @returns The restored hash, or null if nothing was restored.
 */
export function restorePreAuthHash(): string | null {
  const saved = sessionStorage.getItem(PRE_AUTH_HASH_KEY);
  if (!saved) return null;

  sessionStorage.removeItem(PRE_AUTH_HASH_KEY);

  if (!window.location.hash || window.location.hash === '#' || window.location.hash === '#/') {
    window.location.hash = saved;
    return saved;
  }

  return null;
}

// ============================================================================
// Route Comparison
// ============================================================================

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
