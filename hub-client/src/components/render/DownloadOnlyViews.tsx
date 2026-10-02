/**
 * The two panes `PreviewRouter` mounts instead of a preview (D8.2, D8.4): "download" (no live
 * preview, a click-only "Download <type>" button for the document's own format) and "neither"
 * (nothing can show or produce it; the explanation is text, also used as the top-bar
 * button's description). Neither pane renders anything on edit.
 */
import { download } from '../../strings';
import '../DownloadAsControl.css';
import type { DownloadFormat } from '../../pandoc/downloadController';

export function DownloadOnlyView({ format, busy, onDownload }: { format: DownloadFormat; busy: boolean; onDownload: () => void }) {
  return (
    <div className="download-only-pane" data-testid="download-only-pane">
      <h2>{download.noPreviewTitle(format.label)}</h2>
      <p>{download.noPreviewBody}</p>
      <button type="button" className="qh-btn primary" onClick={onDownload} aria-disabled={busy || undefined}>
        {download.downloadOwn(format.label)}
      </button>
    </div>
  );
}

export function NeitherView({ formatKey }: { formatKey: string }) {
  return (
    <div className="download-only-pane" data-testid="neither-pane">
      <h2>{download.neitherTitle(formatKey)}</h2>
      <p>{download.neitherBody(formatKey)}</p>
    </div>
  );
}
