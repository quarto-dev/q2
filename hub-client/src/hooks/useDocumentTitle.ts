import { useEffect } from 'react';

/**
 * Default browser tab title, matching the `<title>` in index.html.
 * Restored whenever the component that set a specific title unmounts.
 */
export const DEFAULT_DOCUMENT_TITLE = 'Quarto Hub';

/**
 * Sets `document.title` while the calling component is mounted and
 * restores {@link DEFAULT_DOCUMENT_TITLE} on unmount.
 *
 * Views that set a specific title (e.g. the Editor's
 * "file — project — Quarto Hub") must not leave it behind after
 * unmount — otherwise the stale title persists on the project
 * selector (https://github.com/quarto-dev/q2/issues/721).
 */
export function useDocumentTitle(title: string): void {
  useEffect(() => {
    document.title = title;
    return () => {
      document.title = DEFAULT_DOCUMENT_TITLE;
    };
  }, [title]);
}
