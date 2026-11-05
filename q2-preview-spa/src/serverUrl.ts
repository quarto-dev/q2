/**
 * URLs of the preview server's own routes (`/health`, `/ws`,
 * `/api/preview/*`), resolved against the page instead of the origin.
 *
 * The SPA is served from the server root and builds its assets with
 * `base: './'`, so the page's directory *is* the server root, whatever
 * prefix a path-stripping proxy (Posit Workbench, JupyterHub,
 * code-server, `/p/<token>/`) put in front of it. Resolving against
 * `document.baseURI` keeps every request under that prefix, the way
 * `hub-client/src/pandoc/nativeRender.ts` already does.
 */

/** The absolute URL of the server route `route` (`/health`, `/api/preview/config?…`). */
export function serverUrl(route: string, base: string = document.baseURI): string {
  return new URL(route.replace(/^\//, ''), base).toString();
}

/** The WebSocket URL of the server route `route` (`/ws`). */
export function serverWsUrl(route: string, base: string = document.baseURI): string {
  const url = new URL(serverUrl(route, base));
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  return url.toString();
}
