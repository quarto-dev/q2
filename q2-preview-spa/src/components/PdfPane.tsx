/**
 * Live PDF preview for `format: typst` pages (Plan 11, E5).
 *
 * The server compiles with native typst (`POST /api/preview/pdf`, Plan 9) and the bytes show in
 * the bundled pdf.js (`@quarto/pdf-viewer`). Each edit recompiles after a debounce; the previous
 * request is aborted and a sequence number drops any answer that arrives out of order. A failed
 * compile keeps the last good PDF under an error banner.
 */

import { useEffect, useRef, useState } from 'react';
import { mountPdfViewer, type PdfViewerHandle } from '@quarto/pdf-viewer';
import { vfsReadFile } from '@quarto/preview-runtime';
import { stripAnsi } from '@quarto/preview-renderer/utils/stripAnsi';
import type { Diagnostic } from '@quarto/preview-renderer/types/diagnostic';
import { installBackButton } from '../pdfBackButton';
import { pdfMetadata, type RenderBlock } from '../previewConfig';
import './PdfPane.css';

export const PDF_ROUTE = '/api/preview/pdf';
export const DIAGNOSTICS_HEADER = 'x-q2-diagnostics';
export const STATS_HEADER = 'x-q2-stats';
/** Quiet period after the last edit before a recompile starts. */
export const DEBOUNCE_MS = 500;

interface PdfPaneProps {
  /** Project-relative path of the active document. */
  path: string;
  /** Bumps on every synced change that may affect the document. */
  contentTick: number;
  /** `null` hides the Back button. */
  onBack: (() => void) | null;
  /** The session's `--to` / `-M` block; its metadata goes under the pane's own `book-scope: chapter`. */
  render?: RenderBlock;
}

interface Banner {
  message: string;
  diagnostics: Diagnostic[];
}

function decodeHeader<T>(response: Response, name: string, fallback: T): T {
  const raw = response.headers.get(name);
  if (!raw) return fallback;
  try {
    return JSON.parse(decodeURIComponent(raw)) as T;
  } catch {
    return fallback; // a malformed header must not lose the PDF
  }
}

function pdfFileName(path: string): string {
  const base = path.split('/').pop() ?? path;
  return base.replace(/\.[^.]*$/, '') + '.pdf';
}

export function PdfPane({ path, contentTick, onBack, render }: PdfPaneProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const handleRef = useRef<PdfViewerHandle | null>(null);
  const onBackRef = useRef(onBack);
  onBackRef.current = onBack;
  // The render block is fixed for a session; a ref keeps it out of the compile effect's deps.
  const renderRef = useRef(render);
  renderRef.current = render;
  const seqRef = useRef(0);
  const abortRef = useRef<AbortController | null>(null);
  const hasPdfRef = useRef(false);

  const [hasPdf, setHasPdf] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [banner, setBanner] = useState<Banner | null>(null);
  const [warnings, setWarnings] = useState<Diagnostic[]>([]);
  const [unexecuted, setUnexecuted] = useState(0);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const handle = mountPdfViewer(container, './', {
      onFrameReady: (win, doc) => installBackButton(win, doc, onBackRef.current ? () => onBackRef.current?.() : null),
    });
    handleRef.current = handle;
    return () => {
      handle.dispose();
      handleRef.current = null;
      abortRef.current?.abort();
      seqRef.current++; // an answer still in flight is stale
    };
  }, []);

  useEffect(() => {
    // The first compile starts at once; later ones wait for a quiet period.
    const timer = setTimeout(() => void compile(), hasPdfRef.current ? DEBOUNCE_MS : 0);
    return () => clearTimeout(timer);

    async function compile() {
      abortRef.current?.abort();
      const controller = new AbortController();
      abortRef.current = controller;
      const seq = ++seqRef.current;
      const stale = () => seq !== seqRef.current;
      setUpdating(true);
      try {
        const snap = vfsReadFile(path);
        const response = await fetch(PDF_ROUTE, {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify({ path, content: snap.success ? snap.content : undefined, metadata: pdfMetadata(renderRef.current) }),
          signal: controller.signal,
        });
        if (stale()) return;
        if (response.status === 409) return; // superseded or busy: a newer request owns the pane
        if (response.status === 422) {
          const body = (await response.json().catch(() => ({}))) as { error?: string; diagnostics?: Diagnostic[] };
          if (stale()) return;
          setBanner({ message: body.error ?? 'The document did not compile.', diagnostics: body.diagnostics ?? [] });
          return;
        }
        if (!response.ok) {
          let message = `HTTP ${response.status}`;
          const text = await response.text().catch(() => '');
          try {
            message = (JSON.parse(text) as { error?: string }).error ?? message;
          } catch {
            message = text || message;
          }
          if (stale()) return;
          setBanner({ message, diagnostics: [] });
          return;
        }
        const bytes = new Uint8Array(await response.arrayBuffer());
        if (stale()) return;
        setWarnings(decodeHeader<Diagnostic[]>(response, DIAGNOSTICS_HEADER, []));
        setUnexecuted(decodeHeader<{ unexecuted_cells?: number }>(response, STATS_HEADER, {}).unexecuted_cells ?? 0);
        await handleRef.current?.show(bytes, { key: path, fileName: pdfFileName(path) });
        if (stale()) return;
        hasPdfRef.current = true;
        setHasPdf(true);
        setBanner(null);
      } catch (e) {
        if (stale() || controller.signal.aborted) return;
        setBanner({
          message: `Could not reach the preview server: ${e instanceof Error ? e.message : String(e)}`,
          diagnostics: [],
        });
      } finally {
        if (!stale()) setUpdating(false);
      }
    }
  }, [path, contentTick]);

  return (
    <div className="q2-pdf-pane">
      <div ref={containerRef} className="q2-pdf-pane__viewer" />
      {!hasPdf && !banner && (
        <div className="q2-pdf-pane__spinner" role="status">
          Compiling…
        </div>
      )}
      {hasPdf && updating && (
        <div className="q2-pdf-pane__chip" role="status">
          Updating…
        </div>
      )}
      {unexecuted > 0 && (
        <div className="q2-pdf-pane__notice" role="note">
          {unexecuted} code cell{unexecuted === 1 ? '' : 's'} not executed: outputs are missing from this preview.
        </div>
      )}
      {banner && (
        <div className="q2-pdf-pane__error" role="alert">
          <strong>{banner.message}</strong>
          {banner.diagnostics.map((d, i) => (
            <pre key={i}>{stripAnsi(d.rendered ?? [d.code, d.title, d.problem, ...d.hints].filter(Boolean).join('\n'))}</pre>
          ))}
        </div>
      )}
      {warnings.length > 0 && (
        <details className="q2-pdf-pane__warnings">
          <summary>
            {warnings.length} warning{warnings.length === 1 ? '' : 's'}
          </summary>
          {warnings.map((d, i) => (
            <pre key={i}>{stripAnsi(d.rendered ?? [d.code, d.title, d.problem].filter(Boolean).join('\n'))}</pre>
          ))}
        </details>
      )}
    </div>
  );
}
