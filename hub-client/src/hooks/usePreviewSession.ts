/**
 * usePreviewSession — the serving server's `q2 preview` session config.
 *
 * Fetched once at boot: the values mirror the CLI flags the server was
 * started with and are fixed for its lifetime, so there is no polling.
 * Returns null while loading and whenever the server is not a
 * `q2 preview` session (standalone hub, vite dev) — callers gate UI on
 * an explicit value (e.g. `config?.allowEdit === false`), so null is
 * always the safe, banner-free case.
 */

import { useEffect, useState } from 'react';
import { fetchPreviewSessionConfig, type PreviewSessionConfig } from '../services/previewConfig';

export function usePreviewSession(): PreviewSessionConfig | null {
  const [config, setConfig] = useState<PreviewSessionConfig | null>(null);

  useEffect(() => {
    let cancelled = false;
    void fetchPreviewSessionConfig().then((fetched) => {
      if (!cancelled && fetched) setConfig(fetched);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  return config;
}
