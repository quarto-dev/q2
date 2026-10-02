/** Status-line text for the "Download as" control (no React; unit-testable). */
import { download } from '../strings';
import type { DownloadStatus } from './downloadController';
import type { LoadProgress } from './pandocLoader';

const mb = (bytes: number) => `${(bytes / (1024 * 1024)).toFixed(1)} MB`;

/** Text for the visible panel; includes byte counts. */
export function workingText(status: Extract<DownloadStatus, { phase: 'working' }>): string {
  const { stage, load, format } = status;
  if ((stage === 'loading' || stage === 'typst-loading') && load) return loadText(load);
  return stageText(stage, format.label);
}

export function loadText(load: LoadProgress, withBytes = true): string {
  switch (load.phase) {
    case 'download':
      return withBytes ? download.downloadingConverter(mb(load.loaded), load.total ? mb(load.total) : null) : download.downloadingConverterShort;
    case 'cached':
      return download.loadingFromCache;
    case 'verify':
      return download.verifying;
    case 'compile':
      return download.compiling;
  }
}

function stageText(stage: string, label: string): string {
  switch (stage) {
    case 'preparing':
      return download.preparing;
    case 'loading':
      return download.startingConverter;
    case 'starting':
      return download.startingConverter;
    case 'mounting':
      return download.mounting;
    case 'typst-loading':
    case 'typst-starting':
      return download.startingCompiler;
    case 'typst-compiling':
      return download.compilingPdf;
    case 'native':
      return download.renderingNatively(label);
    default:
      return download.converting(label);
  }
}

/** Short text for the live region: no byte counts, so it is announced only on real changes. */
export function liveText(status: DownloadStatus): string {
  switch (status.phase) {
    case 'idle':
      return '';
    case 'working':
      return (status.stage === 'loading' || status.stage === 'typst-loading') && status.load ? loadText(status.load, false) : stageText(status.stage, status.format.label);
    case 'done':
      return download.done(status.fileName);
    case 'cancelled':
      return download.cancelled;
    case 'failed':
      return download.failed[status.state] || status.message || '';
  }
}

