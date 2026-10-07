/**
 * MCP resources surface (BP-5): every file in every connected project
 * is a first-class MCP resource under the `hub://` URI contract (Q-2),
 * subscribable on both protocol eras:
 *
 * - modern (2026-07-28): the client opens `subscriptions/listen` with a
 *   `resourceSubscriptions` filter; the serving entry (serveStdio's
 *   listen router) owns filtering and delivery, so this module simply
 *   emits every change — the router drops what nobody requested.
 * - legacy (2025-11-25): the client sends `resources/subscribe`; this
 *   module tracks the subscribed URIs and gates
 *   `notifications/resources/updated` to them (the 2025 model is
 *   unsolicited delivery, but only for subscribed URIs).
 *
 * The handlers are registered on the underlying low-level `Server`,
 * not via `McpServer.registerResource`: the high-level `resources/list`
 * handler does not paginate, and the contract here requires
 * `nextCursor` pagination (a connected project can hold hundreds of
 * files).
 *
 * URI contract (Q-2 — a public, stability-promised shape):
 *
 *     hub://project/<indexDocId>/<path>[?server=<url-encoded ws url>]
 *
 * - The index doc id rides in the PATH, never the host: RFC 3986 host
 *   normalizers lowercase, and automerge doc ids are case-sensitive
 *   base58. (Node's URL parser happens to preserve case for
 *   non-special schemes, but the URI is a public contract other
 *   parsers will normalize.)
 * - The literal `project` segment leaves room for other resource kinds
 *   (`hub://collection/<docId>`, …) without a breaking change.
 * - `?server=` appears exactly when a share URL routed the project to
 *   a foreign hub (bd-qt7h8h5g) — it mirrors the share-URL grammar, so
 *   a resource URI identifies precisely what a share URL identifies.
 */

import { Buffer } from 'node:buffer';

import {
  ProtocolError,
  ProtocolErrorCode,
  ResourceNotFoundError,
  SdkError,
  SdkErrorCode,
  type McpServer,
  type Resource,
} from '@modelcontextprotocol/server';
import {
  inferMimeType,
  type FilePayload,
} from '@quarto/quarto-sync-client';

import { ConnectionManager, type ProjectEvent } from './connection-manager.js';
import { serversMatch } from './share-url.js';
import { QUARTO_ICON } from './icon.js';

/** Page size for `resources/list` (a 500-file project is five pages). */
export const RESOURCE_PAGE_SIZE = 100;

const HUB_URI_PREFIX = 'hub://project/';

/**
 * The URI template advertised on `resources/templates/list`. Kept
 * deliberately conservative — `{?server}`-style optional-query
 * expressions are valid RFC 6570 but poorly supported by host-side
 * template matchers, so the template shows the common form and the
 * `server=` variant is documented in the description.
 */
export const PROJECT_FILE_URI_TEMPLATE = 'hub://project/{indexDocId}/{path}';

/** Build a file's resource URI. `server` only for a foreign (routed) hub. */
export function buildFileResourceUri(
  project: string,
  path: string,
  server?: string,
): string {
  const encodedPath = path.split('/').map(encodeURIComponent).join('/');
  const base = `${HUB_URI_PREFIX}${encodeURIComponent(project)}/${encodedPath}`;
  return server === undefined ? base : `${base}?server=${encodeURIComponent(server)}`;
}

/** Parse a resource URI back into its parts; null when it isn't ours. */
export function parseFileResourceUri(
  uri: string,
): { project: string; path: string; server?: string } | null {
  let url: URL;
  try {
    url = new URL(uri);
  } catch {
    return null;
  }
  if (url.protocol !== 'hub:' || url.host !== 'project') return null;
  const segments = url.pathname.split('/').filter((s) => s !== '');
  if (segments.length < 2) return null;
  const [rawProject, ...rawPath] = segments as [string, ...string[]];
  try {
    return {
      project: decodeURIComponent(rawProject),
      path: rawPath.map(decodeURIComponent).join('/'),
      server: url.searchParams.get('server') ?? undefined,
    };
  } catch {
    // A malformed percent-escape in the path.
    return null;
  }
}

/**
 * The MIME type a resource advertises. Binary payloads know their type
 * (set at write time); text payloads get `text/markdown` for the qmd/md
 * mainstream and honest `text/plain` otherwise — `inferMimeType` only
 * knows binary extensions, so a known inference still wins (an `.svg`
 * written as text, say).
 */
function resourceMimeType(path: string, payload: FilePayload): string {
  if (payload.type === 'binary') return payload.mimeType;
  const inferred = inferMimeType(path);
  if (inferred !== 'application/octet-stream') return inferred;
  const ext = path.includes('.') ? path.slice(path.lastIndexOf('.') + 1).toLowerCase() : '';
  if (ext === 'qmd' || ext === 'md' || ext === 'markdown') return 'text/markdown';
  return 'text/plain';
}

function payloadSize(payload: FilePayload): number {
  return payload.type === 'text'
    ? Buffer.byteLength(payload.text, 'utf8')
    : payload.data.byteLength;
}

/**
 * The `hub://`-addressable form of one connected project's file, for
 * `resources/list`. The URI names the foreign hub only when the
 * project was routed to one (the configured hub is the default).
 */
function fileToResource(
  manager: ConnectionManager,
  server: string,
  indexDocId: string,
  path: string,
  payload: FilePayload,
): Resource {
  const foreign = serversMatch(server, manager.configuredServerUrl)
    ? undefined
    : server;
  return {
    uri: buildFileResourceUri(indexDocId, path, foreign),
    name: path,
    mimeType: resourceMimeType(path, payload),
    size: payloadSize(payload),
  };
}

/** Every file of every connected project, sorted by URI (deterministic pages). */
function collectResources(manager: ConnectionManager): Resource[] {
  const out: Resource[] = [];
  for (const { server, indexDocId } of manager.connectedProjects()) {
    const state = manager.get(indexDocId, { server });
    if (!state) continue;
    for (const [path, payload] of state.files) {
      out.push(fileToResource(manager, server, indexDocId, path, payload));
    }
  }
  out.sort((a, b) => (a.uri < b.uri ? -1 : a.uri > b.uri ? 1 : 0));
  return out;
}

/**
 * Coalescing delay for `resources/list_changed`. A project connect
 * fires one `added` per existing file (initial sync), and a sync batch
 * can add/remove many files at once — the notification says "the list
 * changed", not "an entry changed", so a trailing-edge timer collapses
 * a burst into one emission.
 */
const LIST_CHANGED_DEBOUNCE_MS = 150;

/**
 * Register the resources capability and handlers, and bridge project
 * events to resource notifications. Called once from `createServer`,
 * before any transport connects (capability registration is
 * construction-only).
 *
 * `era` tells the bridge which subscription model the pinned
 * connection uses: on `legacy` it gates `resources/updated` to URIs
 * the client explicitly subscribed; on `modern` it emits every change
 * and lets the serving entry's listen router filter.
 */
export function registerResources(
  server: McpServer,
  manager: ConnectionManager,
  opts: { era: 'modern' | 'legacy' },
): void {
  server.server.registerCapabilities({
    resources: { subscribe: true, listChanged: true },
  });

  // -- resources/list (paginated; the high-level API's isn't) --------
  server.server.setRequestHandler('resources/list', async (request) => {
    const cursor = request.params?.cursor;
    let offset = 0;
    if (cursor !== undefined && cursor !== '') {
      offset = Number.parseInt(cursor, 10);
      if (!Number.isInteger(offset) || offset < 0 || String(offset) !== cursor) {
        throw new ProtocolError(
          ProtocolErrorCode.InvalidParams,
          `Invalid cursor "${cursor}" — pass the nextCursor value from a previous ` +
            'resources/list result verbatim.',
          { reason: 'invalid_cursor' },
        );
      }
    }
    const all = collectResources(manager);
    const page = all.slice(offset, offset + RESOURCE_PAGE_SIZE);
    const nextOffset = offset + RESOURCE_PAGE_SIZE;
    return {
      resources: page,
      ...(nextOffset < all.length ? { nextCursor: String(nextOffset) } : {}),
    };
  });

  // -- resources/read -------------------------------------------------
  server.server.setRequestHandler('resources/read', async (request) => {
    const uri = request.params.uri;
    const parsed = parseFileResourceUri(uri);
    if (parsed === null) {
      throw new ProtocolError(
        ProtocolErrorCode.InvalidParams,
        `Resource URI ${uri} is not a hub:// project file URI ` +
          `(${PROJECT_FILE_URI_TEMPLATE}). List this server's resources to see ` +
          'what is available; connect_project makes a project\u2019s files readable.',
        { reason: 'invalid_uri' },
      );
    }
    const state = await manager.connect(parsed.project, { server: parsed.server });
    const payload = state.files.get(parsed.path);
    if (!payload) {
      throw new ResourceNotFoundError(uri);
    }
    if (payload.type === 'binary') {
      return {
        contents: [
          {
            uri,
            mimeType: payload.mimeType,
            blob: Buffer.from(payload.data).toString('base64'),
          },
        ],
      };
    }
    return {
      contents: [
        {
          uri,
          mimeType: resourceMimeType(parsed.path, payload),
          text: payload.text,
        },
      ],
    };
  });

  // -- resources/templates/list --------------------------------------
  server.server.setRequestHandler('resources/templates/list', async () => ({
    resourceTemplates: [
      {
        name: 'project-file',
        title: 'Quarto Hub project file',
        uriTemplate: PROJECT_FILE_URI_TEMPLATE,
        description:
          'A file in a Quarto Hub project. indexDocId is the project\u2019s automerge ' +
          'index document id (as in a quarto-hub.com share URL); path is the file path ' +
          'within the project. A project on a non-default hub carries its sync server ' +
          'as ?server=<url-encoded ws url>. Connect to the project (connect_project) ' +
          'before reading; subscribe for change notifications.',
        icons: [QUARTO_ICON],
      },
    ],
  }));

  // -- legacy subscriptions (2025-11-25 era) --------------------------
  // On a modern connection these requests never reach the instance
  // (the 2026-07-28 wire has no resources/subscribe; the entry serves
  // subscriptions/listen itself), so the set is exactly the legacy
  // connection's subscriptions.
  const legacySubscriptions = new Set<string>();
  server.server.setRequestHandler('resources/subscribe', async (request) => {
    const uri = request.params.uri;
    if (parseFileResourceUri(uri) === null) {
      throw new ProtocolError(
        ProtocolErrorCode.InvalidParams,
        `Cannot subscribe to ${uri} — not a hub:// project file URI ` +
          `(${PROJECT_FILE_URI_TEMPLATE}).`,
        { reason: 'invalid_uri' },
      );
    }
    legacySubscriptions.add(uri);
    return {};
  });
  server.server.setRequestHandler('resources/unsubscribe', async (request) => {
    legacySubscriptions.delete(request.params.uri);
    return {};
  });

  // -- the change bridge ----------------------------------------------
  // Notifications are best-effort fan-out: a file change can land after
  // the client hung up (fixture teardown, a dead stdio pipe), and the
  // send then rejects with a 'Not connected' SdkError. That close-race
  // is benign — there is no client left to notify — so it is swallowed;
  // anything else is logged to stderr, never thrown: delivery failures
  // must not break the sync callbacks the bridge rides on.
  const sendNotification = (send: () => Promise<void>): void => {
    if (!server.isConnected()) return;
    send().catch((err: unknown) => {
      if (err instanceof SdkError && err.code === SdkErrorCode.NotConnected) return;
      console.error(
        '[hub-mcp] resource notification failed:',
        err instanceof Error ? err.message : String(err),
      );
    });
  };

  let listChangedTimer: NodeJS.Timeout | undefined;
  const emitListChanged = (): void => {
    if (listChangedTimer !== undefined) clearTimeout(listChangedTimer);
    listChangedTimer = setTimeout(() => {
      listChangedTimer = undefined;
      sendNotification(() => server.server.sendResourceListChanged());
    }, LIST_CHANGED_DEBOUNCE_MS);
    // A pending debounce must never hold the stdio process open.
    listChangedTimer.unref();
  };

  manager.onProjectEvent((ev: ProjectEvent) => {
    switch (ev.kind) {
      case 'changed': {
        const foreign = serversMatch(ev.serverUrl, manager.configuredServerUrl)
          ? undefined
          : ev.serverUrl;
        const uri = buildFileResourceUri(ev.indexDocId, ev.path, foreign);
        if (opts.era === 'modern' || legacySubscriptions.has(uri)) {
          sendNotification(() => server.server.sendResourceUpdated({ uri }));
        }
        break;
      }
      case 'added':
      case 'removed':
      case 'connected':
      case 'disconnected':
        emitListChanged();
        break;
    }
  });
}
