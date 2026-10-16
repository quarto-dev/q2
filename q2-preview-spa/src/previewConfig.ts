/**
 * The session's preview config, from `GET /api/preview/config` (bd-ov4gqk3m, F8-D2).
 *
 * `allowEdit` mirrors `--allow-edit`. `render` carries the live viewer's `--to` and `-M`
 * (absent when neither flag was given, so a flagless or older server changes nothing); the
 * SPA sends it on every render call. `--profile` never travels: the browser cannot honor it.
 */

/** The viewer's `--to` and `-M`. */
export interface RenderBlock {
  /** The requested format; wins over the document's and the project's. */
  to?: string;
  /** The overlay metadata (`-M`), a JSON object. */
  metadata?: Record<string, unknown>;
}

export interface PreviewConfig {
  allowEdit: boolean;
  /** Absent when the session has no render flags. */
  render?: RenderBlock;
}

/** The config could not be read, and the session must not be treated as flagless. */
export class PreviewConfigError extends Error {
  constructor(message: string) {
    super(message);
    this.name = 'PreviewConfigError';
  }
}

const isObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

/**
 * Validate a config body. A malformed `render` block throws: a session known to carry flags
 * must not silently render as unflagged html.
 */
export function parsePreviewConfig(body: unknown): PreviewConfig {
  if (!isObject(body)) throw new PreviewConfigError('the preview config is not a JSON object');
  const config: PreviewConfig = { allowEdit: body.allowEdit === true };
  if (body.render === undefined) return config;
  const block = body.render;
  if (!isObject(block)) throw new PreviewConfigError('the preview config `render` block is not an object');
  const render: RenderBlock = {};
  if (block.to !== undefined) {
    if (typeof block.to !== 'string' || block.to === '') {
      throw new PreviewConfigError('the preview config `render.to` is not a non-empty string');
    }
    render.to = block.to;
  }
  if (block.metadata !== undefined) {
    if (!isObject(block.metadata)) {
      throw new PreviewConfigError('the preview config `render.metadata` is not an object');
    }
    render.metadata = block.metadata;
  }
  if (render.to !== undefined || render.metadata !== undefined) config.render = render;
  return config;
}

export interface FetchPreviewConfigOptions {
  /** Attempts before giving up (default 3). */
  attempts?: number;
  /** Delay before the second attempt, doubled each time (default 250 ms). */
  delayMs?: number;
}

const sleep = (ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms));

/**
 * Read the session config.
 *
 * - A `404` means the server predates the endpoint: read-only, no flags (as before).
 * - A network error, another non-2xx status or a malformed body is retried; if it keeps
 *   failing the call rejects with a {@link PreviewConfigError}, so the app shows an error
 *   instead of rendering a flagged session as plain html.
 */
export async function fetchPreviewConfig(
  loc: Pick<Location, 'protocol' | 'host'> = window.location,
  { attempts = 3, delayMs = 250 }: FetchPreviewConfigOptions = {},
): Promise<PreviewConfig> {
  let lastError: unknown;
  for (let attempt = 0; attempt < attempts; attempt++) {
    if (attempt > 0) await sleep(delayMs * 2 ** (attempt - 1));
    try {
      const resp = await fetch(`${loc.protocol}//${loc.host}/api/preview/config`);
      if (resp.status === 404) return { allowEdit: false };
      if (!resp.ok) {
        lastError = new PreviewConfigError(`GET /api/preview/config answered HTTP ${resp.status}`);
        continue;
      }
      return parsePreviewConfig(await resp.json());
    } catch (e) {
      lastError = e;
    }
  }
  const reason = lastError instanceof Error ? lastError.message : String(lastError);
  throw new PreviewConfigError(`Could not read the preview settings from the server (${reason}).`);
}

/** The wasm render options for a session's render block, or `undefined` when it has none. */
export function wasmRenderOptions(render: RenderBlock | undefined): RenderBlock | undefined {
  if (!render) return undefined;
  const options: RenderBlock = {};
  if (render.to !== undefined) options.to = render.to;
  if (render.metadata !== undefined) options.metadata = render.metadata;
  return options;
}

/**
 * The metadata the PDF pane sends: the CLI's overlay under the pane's own `book-scope: chapter`
 * (a shallow spread, the key is top level), so a CLI `-M book-scope:book` cannot turn a one-page
 * preview into a whole-book PDF (F8-D3).
 */
export function pdfMetadata(render: RenderBlock | undefined): Record<string, unknown> {
  return { ...(render?.metadata ?? {}), 'book-scope': 'chapter' };
}
