// Client for the preview server's `POST /api/preview/render` (pandoc-wasm H4b, design D7).
// The preview embed carries no pandoc.wasm, so its "Download as" asks the native side to
// render. The wire contract is documented in crates/quarto-preview/src/render_download.rs.

import type { RenderMetadata } from './bookScope';

export interface NativeRenderRequest {
  /** Project-relative path of the document (must be in the project index). */
  path: string;
  /** A key of the route's format table (docx, odt, pptx, epub, typst, typst-pdf); Rust owns the table (R2), not repeated here. */
  format: string;
  /**
   * The editor's current text; the server renders it instead of the disk copy. Always sent: the server takes an
   * omitted `content` as "render the disk copy", but an empty string as the source itself.
   */
  content: string;
  /**
   * The render's runtime-layer metadata (a JSON object), e.g. `{ 'book-scope': 'chapter' }` (`bookScopeMetadata`);
   * absent means the user did not choose and the target's default applies.
   */
  metadata?: RenderMetadata;
  /** The authorship JSON for docx and pptx comment and track-change authors (`attributionJsonFor`). */
  attributionJson?: string;
}

/** A diagnostic as the server serializes it (`JsonDiagnostic`); the UI treats it opaquely. */
export type NativeDiagnostic = Record<string, unknown>;

export type NativeRenderOutcome =
  | { kind: 'ok'; blob: Blob; fileName: string; warnings: NativeDiagnostic[]; unexecutedCells: number; book?: { chapters: number } }
  /** The render had errors (HTTP 422): nothing downloads, show these. */
  | { kind: 'failed'; error?: string; diagnostics: NativeDiagnostic[] }
  /** HTTP 409 `{"superseded":true}`: a newer request for the same document replaced this one; drop it silently. */
  | { kind: 'superseded' }
  /** HTTP 409 `{"busy":true}`: the server is rendering something that cannot overlap; tell the user. */
  | { kind: 'busy' }
  /** Anything else: a bad request, or the server could not be reached. */
  | { kind: 'error'; message: string };

export const NATIVE_RENDER_ROUTE = '/api/preview/render';
/** The inline, supersede-aware PDF route the preview pane uses. */
export const NATIVE_PDF_ROUTE = '/api/preview/pdf';
export const DIAGNOSTICS_HEADER = 'x-q2-diagnostics';
export const STATS_HEADER = 'x-q2-stats';

/** The percent-encoded JSON header `name`, parsed; undefined when absent or malformed (it must never lose the file). */
function jsonHeader<T>(response: Response, name: string): T | undefined {
  const raw = response.headers.get(name);
  if (!raw) return undefined;
  try {
    return JSON.parse(decodeURIComponent(raw)) as T;
  } catch {
    return undefined;
  }
}

/** `filename*=UTF-8''…` wins over `filename="…"`, as in RFC 6266. */
export function fileNameFromDisposition(header: string | null, fallback: string): string {
  if (!header) return fallback;
  const star = /filename\*=UTF-8''([^;]+)/i.exec(header);
  if (star) {
    try {
      return decodeURIComponent(star[1].trim());
    } catch {
      // fall through to the ASCII form
    }
  }
  const plain = /filename="([^"]*)"/i.exec(header);
  return plain ? plain[1] : fallback;
}

async function post(
  route: string,
  request: NativeRenderRequest,
  opts: { signal?: AbortSignal; fetchFn?: typeof fetch; baseUrl?: string },
): Promise<NativeRenderOutcome> {
  const fetchFn = opts.fetchFn ?? fetch;
  // Same origin as the page by default; `new URL` keeps a subpath deployment working.
  const url = new URL(route.replace(/^\//, ''), opts.baseUrl ?? document.baseURI).toString();
  let response: Response;
  try {
    response = await fetchFn(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(request),
      signal: opts.signal,
    });
  } catch (e) {
    if (opts.signal?.aborted) throw e;
    return { kind: 'error', message: `Could not reach the preview server: ${e instanceof Error ? e.message : String(e)}` };
  }
  if (response.status === 422) {
    const body = (await response.json().catch(() => ({}))) as { error?: string; diagnostics?: NativeDiagnostic[] };
    return { kind: 'failed', error: body.error, diagnostics: body.diagnostics ?? [] };
  }
  if (response.status === 409) {
    const text = await response.text().catch(() => '');
    let body: { superseded?: boolean; busy?: boolean } = {};
    try {
      body = JSON.parse(text) as typeof body;
    } catch {
      // not the contract's body: fall through to a plain error
    }
    if (body.superseded) return { kind: 'superseded' };
    if (body.busy) return { kind: 'busy' };
    return { kind: 'error', message: text || 'HTTP 409' };
  }
  if (!response.ok) {
    return { kind: 'error', message: (await response.text().catch(() => '')) || `HTTP ${response.status}` };
  }
  const warnings = jsonHeader<NativeDiagnostic[]>(response, DIAGNOSTICS_HEADER) ?? [];
  const stats = jsonHeader<{ unexecuted_cells?: number; book?: { scope: 'book' | 'chapter' | 'none'; chapters: number } | null }>(response, STATS_HEADER);
  return {
    kind: 'ok',
    blob: await response.blob(),
    // The inline PDF route sends no `Content-Disposition`: an empty name makes the caller name the file.
    fileName: fileNameFromDisposition(response.headers.get('content-disposition'), route === NATIVE_PDF_ROUTE ? '' : `download.${request.format}`),
    warnings,
    unexecutedCells: stats?.unexecuted_cells ?? 0,
    // Only a whole-book file is a book download; a chapter or page of a book is one document (as in the wasm path).
    ...(stats?.book?.scope === 'book' ? { book: { chapters: stats.book.chapters } } : {}),
  };
}

type NativeOptions = { signal?: AbortSignal; fetchFn?: typeof fetch; baseUrl?: string };

/** `POST /api/preview/render`: the file as an attachment (docx, odt, pptx, epub, typst source, typst PDF). */
export function renderNatively(request: NativeRenderRequest, opts: NativeOptions = {}): Promise<NativeRenderOutcome> {
  return post(NATIVE_RENDER_ROUTE, request, opts);
}

/** `POST /api/preview/pdf`: the PDF inline; a newer request for the same path answers this one `superseded`. */
export function renderPdfNatively(request: NativeRenderRequest, opts: NativeOptions = {}): Promise<NativeRenderOutcome> {
  return post(NATIVE_PDF_ROUTE, request, opts);
}
