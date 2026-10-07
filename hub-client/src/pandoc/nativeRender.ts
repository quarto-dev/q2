// Client for the preview server's `POST /api/preview/render` (pandoc-wasm H4b, design D7).
// The preview embed carries no pandoc.wasm, so its "Download as" asks the native side to
// render. The wire contract is documented in crates/quarto-preview/src/render_download.rs.

export interface NativeRenderRequest {
  /** Project-relative path of the document (must be in the project index). */
  path: string;
  /** `docx`, `pptx` or `epub`; the menu's format table is Rust-owned (R2), not repeated here. */
  format: string;
  /** The editor's current text; the server renders it instead of the disk copy. */
  content: string;
}

/** A diagnostic as the server serializes it (`JsonDiagnostic`); the UI treats it opaquely. */
export type NativeDiagnostic = Record<string, unknown>;

export type NativeRenderOutcome =
  | { kind: 'ok'; blob: Blob; fileName: string; warnings: NativeDiagnostic[] }
  /** The render had errors (HTTP 422): nothing downloads, show these. */
  | { kind: 'failed'; error?: string; diagnostics: NativeDiagnostic[] }
  /** Anything else: a bad request, or the server could not be reached. */
  | { kind: 'error'; message: string };

export const NATIVE_RENDER_ROUTE = '/api/preview/render';
export const DIAGNOSTICS_HEADER = 'x-q2-diagnostics';

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

export async function renderNatively(
  request: NativeRenderRequest,
  opts: { signal?: AbortSignal; fetchFn?: typeof fetch; baseUrl?: string } = {},
): Promise<NativeRenderOutcome> {
  const fetchFn = opts.fetchFn ?? fetch;
  // Same origin as the page by default; `new URL` keeps a subpath deployment working.
  const url = new URL(NATIVE_RENDER_ROUTE.replace(/^\//, ''), opts.baseUrl ?? document.baseURI).toString();
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
  if (!response.ok) {
    return { kind: 'error', message: (await response.text().catch(() => '')) || `HTTP ${response.status}` };
  }
  const warningsHeader = response.headers.get(DIAGNOSTICS_HEADER);
  let warnings: NativeDiagnostic[] = [];
  if (warningsHeader) {
    try {
      warnings = JSON.parse(decodeURIComponent(warningsHeader)) as NativeDiagnostic[];
    } catch {
      // A malformed header must not lose the download.
    }
  }
  return {
    kind: 'ok',
    blob: await response.blob(),
    fileName: fileNameFromDisposition(response.headers.get('content-disposition'), `download.${request.format}`),
    warnings,
  };
}
