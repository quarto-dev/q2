// Client for the preview server's `POST /api/preview/import` (Plan 7, P7-D1..D4).
// The `q2 preview` embed carries no pandoc.wasm, so the import service's one pandoc step
// (`ImportDeps.runner`) goes to the native side instead. The wire contract is documented in
// crates/quarto-preview/src/import_route.rs.
//
// This module is the only place that maps native exit statuses onto failure kinds; the
// route reports pandoc's raw status and stderr (plus `timedOut`/`aborted`) and invents no
// user-facing text.
import type { Diagnostic, PandocRequest, RequestFile, RunStats, ShareTree } from '@quarto/pandoc-host';
import type { RunFailure, RunFailureKind, RunOptions, RunOutcome } from './pandocRunner';

export const NATIVE_IMPORT_ROUTE = '/api/preview/import';

/** pandoc's exit status at the `+RTS -M` heap cap (the server passes `-M512M`). */
const HEAP_EXHAUSTED_STATUS = 251;

/** The `ImportDeps.runner` shape. */
export interface ImportRunner {
  run(request: PandocRequest, shareTree: ShareTree, options?: RunOptions): Promise<RunOutcome>;
}

interface FrameHeader {
  status: number | null;
  stderr: string;
  stdout: string;
  timedOut: boolean;
  aborted: boolean;
  outputLen: number;
  collected: { path: string; len: number }[];
  diagnostics: Diagnostic[];
}

/** A decoded `[u32 BE header_len][JSON header][output][media…]` body. */
export interface Frame {
  header: FrameHeader;
  output: Uint8Array;
  collected: RequestFile[];
}

/** Decodes the route's response body; throws on a truncated or inconsistent one. */
export function parseFrame(body: Uint8Array): Frame {
  if (body.byteLength < 4) throw new Error('the import response is truncated (no header length)');
  const headerLen = new DataView(body.buffer, body.byteOffset, body.byteLength).getUint32(0, false);
  if (4 + headerLen > body.byteLength) throw new Error('the import response is truncated (header)');
  const header = JSON.parse(new TextDecoder().decode(body.subarray(4, 4 + headerLen))) as FrameHeader;
  let at = 4 + headerLen;
  const take = (len: number): Uint8Array => {
    if (at + len > body.byteLength) throw new Error('the import response is truncated (payload)');
    const bytes = body.slice(at, at + len);
    at += len;
    return bytes;
  };
  const output = take(header.outputLen);
  const collected = header.collected.map((c) => ({ path: c.path, bytes: take(c.len) }));
  if (at !== body.byteLength) throw new Error('the import response has trailing bytes');
  return { header, output, collected };
}

const NO_STATS: RunStats = { mountMs: 0, instanceMs: 0, runMs: 0, memoryBytes: 0, mountedBytes: 0 };

function failure(kind: RunFailureKind, status: number | null, stderr: string, stdout = '', diagnostics: Diagnostic[] = []): RunFailure {
  return { ok: false, kind, status, stderr, stdout, diagnostics, notices: [] };
}

/** The one mapping from the route's raw outcome onto a failure kind (P7-D3). */
function failureKindFor(header: FrameHeader): RunFailureKind {
  if (header.aborted) return 'aborted';
  if (header.timedOut) return 'timeout';
  if (header.status === HEAP_EXHAUSTED_STATUS && /Heap exhausted/.test(header.stderr)) return 'oom';
  if (header.status === null) return 'crash';
  return 'pandoc-exit';
}

/** Server diagnostics are Rust's own words; tag them so `ImportDiagnostic` accepts them. */
const asRust = (d: Diagnostic): Diagnostic => ('origin' in d && d.origin ? d : ({ ...d, origin: 'rust' } as Diagnostic));

export interface NativeImportRunnerOptions {
  fetchFn?: typeof fetch;
  baseUrl?: string;
}

export function createNativeImportRunner(opts: NativeImportRunnerOptions = {}): ImportRunner {
  return {
    async run(request, _shareTree, options = {}) {
      const { signal, onStage } = options;
      if (signal?.aborted) return failure('aborted', null, '');
      const sourcePath = request.host_inputs?.[0]?.path;
      const source = sourcePath === undefined ? undefined : options.inputs?.[sourcePath];
      if (sourcePath === undefined || source === undefined) {
        return failure('invalid-request', null, 'the import request names no source bytes');
      }
      // The server selects the format from the extension only; the runner's interface carries no
      // original file name, so the basename of the virtual source path (`source.<ext>`) is sent.
      const fileName = sourcePath.slice(sourcePath.lastIndexOf('/') + 1);
      const fetchFn = opts.fetchFn ?? fetch;
      const url = new URL(NATIVE_IMPORT_ROUTE.replace(/^\//, ''), opts.baseUrl ?? document.baseURI).toString();

      onStage?.('running');
      let response: Response;
      try {
        response = await fetchFn(url, { method: 'POST', headers: { file_name: fileName, 'content-type': 'application/octet-stream' }, body: source as BodyInit, signal });
      } catch (e) {
        if (signal?.aborted) return failure('aborted', null, '');
        return failure('crash', null, `Could not reach the preview server: ${e instanceof Error ? e.message : String(e)}`);
      }

      if (response.status !== 200) {
        const text = await response.text().catch(() => '');
        // A refusal Rust worded itself (Q-20-*, Q-24-1, Q-24-2): show it as it is.
        try {
          const body = JSON.parse(text) as { diagnostics?: Diagnostic[] };
          if (Array.isArray(body.diagnostics) && body.diagnostics.length > 0) return failure('refused', null, '', '', body.diagnostics.map(asRust));
        } catch {
          // not JSON: fall through
        }
        return failure('invalid-request', null, text || `HTTP ${response.status}`);
      }

      let frame: Frame;
      try {
        frame = parseFrame(new Uint8Array(await response.arrayBuffer()));
      } catch (e) {
        if (signal?.aborted) return failure('aborted', null, '');
        return failure('invalid-request', null, e instanceof Error ? e.message : String(e));
      }
      const { header } = frame;
      const diagnostics = (header.diagnostics ?? []).map(asRust);
      if (header.aborted || header.timedOut || header.status !== 0) {
        return failure(failureKindFor(header), header.status, header.stderr, header.stdout, diagnostics);
      }
      if (frame.output.byteLength === 0) return failure('no-output', 0, header.stderr, header.stdout, diagnostics);
      return {
        ok: true,
        status: 0,
        output: frame.output,
        outputPath: request.output_path,
        collected: frame.collected,
        stderr: header.stderr,
        stdout: header.stdout,
        diagnostics,
        stats: NO_STATS,
        notices: [],
      };
    },
  };
}
