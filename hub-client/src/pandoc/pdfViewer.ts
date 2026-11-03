/**
 * hub-client's binding of `@quarto/pdf-viewer`: the same viewer with the app's Vite base path as
 * the default, so callers need not pass it. (The preview SPA passes `'./'`.)
 */
import { mountPdfViewer as mount, viewerUrl as url, PDF_HISTORY_KEY, VIEWER_WATCHDOG_MS } from '@quarto/pdf-viewer';
import type { PdfViewerHandle, ShowResult } from '@quarto/pdf-viewer';

export { PDF_HISTORY_KEY, VIEWER_WATCHDOG_MS };
export type { PdfViewerHandle, ShowResult };

export const viewerUrl = (base: string = import.meta.env.BASE_URL, file?: string, restore = false): string =>
  url(base, file, restore);

export const mountPdfViewer = (container: HTMLElement, base: string = import.meta.env.BASE_URL): PdfViewerHandle =>
  mount(container, base);
