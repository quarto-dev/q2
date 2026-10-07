import { useEffect, useState } from 'react';
import { getImportService, type ImportFormats } from './importService';

/** The import format table, or null until it has loaded (or when `enabled` is false or the load failed). */
export function useImportFormats(enabled: boolean): ImportFormats | null {
  const [formats, setFormats] = useState<ImportFormats | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    getImportService()
      .getImportFormats()
      .then((f) => {
        if (live) setFormats(f);
      })
      .catch((err: unknown) => console.error('[import] could not load the format table:', err));
    return () => {
      live = false;
    };
  }, [enabled]);
  return enabled ? formats : null;
}
